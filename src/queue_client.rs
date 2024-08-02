use std::{
    collections::HashMap,
    fmt::Debug,
    sync::{atomic::AtomicI64, Arc},
    time::Duration,
};

use futures::{SinkExt, StreamExt};
use http::{
    header::{
        AUTHORIZATION, CONNECTION, HOST, SEC_WEBSOCKET_KEY, SEC_WEBSOCKET_VERSION, UPGRADE,
        USER_AGENT,
    },
    Request, Uri,
};
use serde::{Deserialize, Serialize};
use tokio::{
    sync::{mpsc, oneshot, Mutex},
    task::JoinHandle,
    time::interval,
};
use tokio_tungstenite::tungstenite::{self, handshake::client::generate_key};
use tracing::{debug, info, warn};

use crate::{
    api_error,
    error::ApiError,
    model::{AgentId, ProcessId, SessionToken},
};

#[derive(Debug, Copy, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct CorrelationId(i64);

impl CorrelationId {
    pub fn new(i: i64) -> Self {
        CorrelationId(i)
    }
}

#[derive(Clone)]
pub struct CorrelationIdGenerator {
    v: Arc<AtomicI64>,
}

impl Default for CorrelationIdGenerator {
    fn default() -> Self {
        Self::new()
    }
}

impl CorrelationIdGenerator {
    pub fn new() -> Self {
        CorrelationIdGenerator {
            v: Arc::new(AtomicI64::new(0)),
        }
    }

    pub fn next(&self) -> CorrelationId {
        CorrelationId(self.v.fetch_add(1, std::sync::atomic::Ordering::SeqCst))
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "messageType", rename_all = "SCREAMING_SNAKE_CASE")]
enum Message {
    CommandRequest {
        #[serde(rename = "correlationId")]
        correlation_id: CorrelationId,
        agent_id: AgentId,
    },
    CommandResponse {
        #[serde(rename = "correlationId")]
        correlation_id: CorrelationId,
        // TODO
    },
    ProcessRequest {
        #[serde(rename = "correlationId")]
        correlation_id: CorrelationId,
        capabilities: serde_json::Value,
    },
    ProcessResponse {
        #[serde(rename = "correlationId")]
        correlation_id: CorrelationId,
        #[serde(rename = "sessionToken")]
        session_token: SessionToken,
        #[serde(rename = "processId")]
        process_id: ProcessId,
        // TODO imports
    },
}

#[derive(Debug)]
pub struct CommandResponse {
    pub correlation_id: CorrelationId,
    // TODO
}

#[derive(Debug)]
pub struct ProcessResponse {
    pub correlation_id: CorrelationId,
    pub session_token: SessionToken,
    pub process_id: ProcessId,
}

type Reply = Result<Message, ApiError>;

struct Responder(CorrelationId, oneshot::Sender<Reply>);

struct MessageToSend {
    msg: tungstenite::Message,
    resp: Option<Responder>,
}

impl MessageToSend {
    fn ping() -> Self {
        MessageToSend {
            msg: tungstenite::Message::Ping(vec![]),
            resp: None,
        }
    }

    fn pong() -> Self {
        MessageToSend {
            msg: tungstenite::Message::Pong(vec![]),
            resp: None,
        }
    }

    fn text(text: String, resp: Responder) -> Self {
        MessageToSend {
            msg: tungstenite::Message::Text(text),
            resp: Some(resp),
        }
    }
}

pub struct Config {
    pub agent_id: AgentId,
    pub uri: Uri,
    pub auth_header: String,
    pub capabilities: serde_json::Value,
    pub ping_interval: Duration,
}

pub struct QueueClient {
    agent_id: AgentId,
    capabilities: serde_json::Value,
    tx: mpsc::Sender<MessageToSend>,
    _ping_task: JoinHandle<()>,
    _write_task: JoinHandle<()>,
    _read_task: JoinHandle<()>,
}

impl QueueClient {
    pub async fn connect(config: Config) -> Result<Self, ApiError> {
        let req = QueueClient::create_connect_request(&config.uri, &config.auth_header)?;
        let (ws_stream, _) = tokio_tungstenite::connect_async(req).await?;
        let (mut write, mut read) = ws_stream.split();

        // a channel to communicate between tasks
        let (tx, mut rx) = mpsc::channel::<MessageToSend>(1);

        // used to match responses to requests using correlation IDs
        let message_queue = Arc::new(Mutex::new(HashMap::<CorrelationId, Responder>::new()));

        // task to send pings
        let _ping_task = {
            let tx = tx.clone();
            tokio::spawn(async move {
                let mut interval = interval(config.ping_interval);
                loop {
                    interval.tick().await;
                    if let Err(e) = tx.send(MessageToSend::ping()).await {
                        warn!("Ping error: {}", e);
                    }
                }
            })
        };

        // send messages to the server
        let _write_task = {
            let message_queue = message_queue.clone();
            tokio::spawn(async move {
                while let Some(MessageToSend { msg, resp }) = rx.recv().await {
                    match write.send(msg).await {
                        Ok(_) => {
                            // message sent successfully, register the responder if needed
                            if let Some(Responder(correlation_id, channel)) = resp {
                                message_queue
                                    .lock()
                                    .await
                                    .insert(correlation_id, Responder(correlation_id, channel));
                            }
                        }
                        Err(e) => {
                            // message failed to send, notify the responder if needed
                            let err = format!("Write error: {}", e);
                            warn!("{}", err);
                            if let Some(Responder(_, channel)) = resp {
                                if channel.send(Err(ApiError::simple(&err))).is_err() {
                                    warn!("Responder error (most likely a bug)");
                                }
                            }
                        }
                    }
                }
            })
        };

        // receive messages from the server
        let _read_task = {
            let tx = tx.clone();
            tokio::spawn(async move {
                while let Some(msg) = read.next().await {
                    match msg {
                        Ok(msg) => match msg {
                            tungstenite::Message::Ping(_) => {
                                // respond to pings
                                if let Err(e) = tx.send(MessageToSend::pong()).await {
                                    warn!("Pong error: {}", e);
                                }
                            }
                            tungstenite::Message::Pong(_) => {
                                // log pongs
                                debug!("Received a pong");
                            }
                            tungstenite::Message::Text(text) => {
                                match serde_json::from_str::<Message>(&text) {
                                    Ok(cmd @ Message::CommandResponse { correlation_id, .. }) => {
                                        info!("Received a command: {:?}", cmd);
                                        if let Some(Responder(_, responder)) =
                                            message_queue.lock().await.remove(&correlation_id)
                                        {
                                            if responder.send(Ok(cmd)).is_err() {
                                                warn!("Responder error (most likely a bug)");
                                            }
                                        }
                                    }
                                    Ok(proc @ Message::ProcessResponse { correlation_id, .. }) => {
                                        info!("Received a process: {:?}", proc);
                                        if let Some(Responder(_, responder)) =
                                            message_queue.lock().await.remove(&correlation_id)
                                        {
                                            if responder.send(Ok(proc)).is_err() {
                                                warn!("Responder error (most likely a bug)");
                                            }
                                        }
                                    }
                                    Ok(msg) => {
                                        warn!("Unexpected message body: {:?}", msg);
                                    }
                                    Err(e) => {
                                        warn!("Error while parsing message: {}", e);
                                    }
                                }
                            }
                            _ => {
                                // log and ignore bad messages
                                warn!("Unexpected message: {:?}", msg);
                            }
                        },
                        Err(e) => {
                            // complain on network errors
                            warn!("Read error: {}", e);
                        }
                    }
                }
            })
        };

        Ok(QueueClient {
            agent_id: config.agent_id,
            capabilities: config.capabilities,
            tx,
            _ping_task,
            _write_task,
            _read_task,
        })
    }

    pub async fn next_command(
        &self,
        correlation_id: CorrelationId,
    ) -> Result<CommandResponse, ApiError> {
        // prepare a request
        let req = Message::CommandRequest {
            correlation_id,
            agent_id: self.agent_id,
        };

        // serialize to JSON
        let json = serde_json::to_string(&req)?;

        // send the request
        let (reply_sender, reply_receiver) = oneshot::channel::<Reply>();
        let resp = Responder(correlation_id, reply_sender);
        let msg = MessageToSend::text(json, resp);
        if let Err(e) = self.tx.send(msg).await {
            return Err(api_error!("Send error: {}", e));
        }

        // wait for the reply
        match reply_receiver.await {
            Ok(Ok(Message::CommandResponse {
                correlation_id: reply_correlation_id,
            })) => {
                if correlation_id == reply_correlation_id {
                    Ok(CommandResponse { correlation_id })
                } else {
                    Err(api_error!(
                        "Unexpected correlation ID (most likely a bug): {:?}",
                        reply_correlation_id
                    ))
                }
            }
            Ok(msg) => Err(api_error!("Unexpected message: {:?}", msg)),
            Err(e) => Err(api_error!("Error while parsing message: {}", e)),
        }
    }

    pub async fn next_process(
        &self,
        correlation_id: CorrelationId,
    ) -> Result<ProcessResponse, ApiError> {
        // prepare a request
        let req = Message::ProcessRequest {
            correlation_id,
            capabilities: serde_json::json!(&self.capabilities),
        };

        // serialize to JSON
        let json = serde_json::to_string(&req)?;

        // send the request
        let (reply_sender, reply_receiver) = oneshot::channel::<Reply>();
        let resp = Responder(correlation_id, reply_sender);
        let msg = MessageToSend::text(json, resp);
        if let Err(e) = self.tx.send(msg).await {
            return Err(api_error!("Send error: {}", e));
        }

        // wait for the reply
        match reply_receiver.await {
            Ok(Ok(Message::ProcessResponse {
                correlation_id: reply_correlation_id,
                session_token,
                process_id,
            })) => {
                if correlation_id == reply_correlation_id {
                    Ok(ProcessResponse {
                        correlation_id,
                        session_token,
                        process_id,
                    })
                } else {
                    Err(api_error!(
                        "Unexpected correlation ID (most likely a bug): {:?}",
                        reply_correlation_id
                    ))
                }
            }
            Ok(msg) => Err(api_error!("Unexpected message: {:?}", msg)),
            Err(e) => Err(api_error!("Error while parsing message: {}", e)),
        }
    }

    fn create_connect_request(uri: &Uri, auth_header: &str) -> Result<http::Request<()>, ApiError> {
        let host = format!(
            "{}:{}",
            uri.host().unwrap_or("localhost"),
            uri.port_u16().unwrap_or(8001)
        );

        let ws_key = generate_key();

        Request::builder()
            .uri(uri.clone())
            .header(HOST, host)
            .header(AUTHORIZATION, auth_header)
            .header(CONNECTION, "Upgrade")
            .header(UPGRADE, "websocket")
            .header(SEC_WEBSOCKET_VERSION, "13")
            .header(SEC_WEBSOCKET_KEY, ws_key)
            .header(
                USER_AGENT,
                format!("concord-client-rs/{}", env!("CARGO_PKG_VERSION")),
            )
            .body(())
            .map_err(|e| ApiError {
                message: e.to_string(),
            })
    }
}
