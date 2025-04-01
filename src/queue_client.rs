use bytes::Bytes;
use futures::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use tokio_tungstenite::tungstenite::{self, Utf8Bytes};
use tokio_util::sync::CancellationToken;
use tracing::{debug, warn};

use crate::{
    api_err,
    error::ApiError,
    model::{AgentId, ApiToken, ProcessId, SessionToken, USER_AGENT_VALUE},
};

#[derive(Debug, Copy, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct CorrelationId(i64);

#[derive(Clone, Default)]
struct CorrelationIdGenerator {
    v: std::sync::Arc<std::sync::atomic::AtomicI64>,
}

impl CorrelationIdGenerator {
    fn next(&self) -> CorrelationId {
        let id = self.v.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        CorrelationId(id)
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

struct Responder(CorrelationId, tokio::sync::oneshot::Sender<Reply>);

#[derive(Default, Clone)]
struct ResponseQueue(std::sync::Arc<tokio::sync::Mutex<std::collections::HashMap<CorrelationId, Responder>>>);

impl ResponseQueue {
    async fn lock(&self) -> tokio::sync::MutexGuard<std::collections::HashMap<CorrelationId, Responder>> {
        self.0.lock().await
    }
}

struct MessageToSend {
    msg: tungstenite::Message,
    resp: Option<Responder>,
}

impl MessageToSend {
    fn ping() -> Self {
        MessageToSend {
            msg: tungstenite::Message::Ping(Bytes::new()),
            resp: None,
        }
    }

    fn pong() -> Self {
        MessageToSend {
            msg: tungstenite::Message::Pong(Bytes::new()),
            resp: None,
        }
    }

    fn text(text: String, resp: Responder) -> Self {
        MessageToSend {
            msg: tungstenite::Message::Text(text.into()),
            resp: Some(resp),
        }
    }
}

pub struct Config {
    pub agent_id: AgentId,
    pub uri: http::Uri,
    pub api_token: ApiToken,
    pub capabilities: serde_json::Value,
    pub ping_interval: std::time::Duration,
}

pub struct QueueClient {
    agent_id: AgentId,
    capabilities: serde_json::Value,
    tx: tokio::sync::mpsc::Sender<MessageToSend>,
    cancellation_token: CancellationToken,
    correlation_id_gen: CorrelationIdGenerator,
}

impl QueueClient {
    pub async fn connect(config: &Config) -> Result<Self, ApiError> {
        let req = QueueClient::create_connect_request(config)?;
        let (ws_stream, _) = tokio_tungstenite::connect_async(req).await?;

        let (mut ws_write, mut ws_read) = ws_stream.split();

        // a channel to communicate between tasks
        let (tx, mut rx) = tokio::sync::mpsc::channel::<MessageToSend>(1);

        // used to match responses to requests using correlation IDs
        let response_queue = ResponseQueue::default();

        let cancellation_token = CancellationToken::new();

        // task to send pings
        let token = cancellation_token.clone();
        let _ping_task = {
            let tx = tx.clone();
            let ping_interval = config.ping_interval;
            tokio::spawn(async move {
                let mut interval = tokio::time::interval(ping_interval);
                loop {
                    tokio::select! {
                        _ = token.cancelled() => {
                            debug!("Stopping ping task...");
                            break
                        }
                        _ = interval.tick() => {
                            debug!("Sending a ping...");
                            if let Err(e) = tx.send(MessageToSend::ping()).await {
                                warn!("Failed to send a ping message to the server: {e}");
                            }
                        }
                    }
                }
            })
        };

        // task to send messages to the server
        let token = cancellation_token.clone();
        let _write_task = {
            let response_queue = response_queue.clone();
            tokio::spawn(async move {
                loop {
                    tokio::select! {
                        _ = token.cancelled() => {
                            debug!("Stopping message sender...");
                            break
                        },
                        msg = rx.recv() => {
                            if let Some(MessageToSend { msg, resp }) = msg {
                                match ws_write.send(msg).await {
                                    Ok(_) => {
                                        // message sent successfully, register the responder if needed
                                        if let Some(Responder(correlation_id, channel)) = resp {
                                            let mut response_queue = response_queue.lock().await;
                                            let responder = Responder(correlation_id, channel);
                                            response_queue.insert(correlation_id, responder);
                                        }
                                    }
                                    Err(e) => {
                                        // message failed to send, notify the responder if needed
                                        let err = format!("Write error: {e}");
                                        warn!("{}", err);
                                        if let Some(Responder(_, channel)) = resp {
                                            if channel.send(Err(ApiError::simple(&err))).is_err() {
                                                warn!("Responder error (most likely a bug)");
                                            }
                                        }
                                    }
                                }
                            } else {
                                break
                            }
                        }
                    }
                }
            })
        };

        // task to receive messages from the server
        let token = cancellation_token.clone();
        let _read_task = {
            let tx = tx.clone();
            tokio::spawn(async move {
                loop {
                    tokio::select! {
                        _ = token.cancelled() => {
                            debug!("Stopping message received...");
                            break
                        }
                        msg = ws_read.next() => {
                            match msg {
                                Some(Ok(msg)) => match msg {
                                    tungstenite::Message::Ping(_) => {
                                        // respond to pings
                                        if let Err(e) = tx.send(MessageToSend::pong()).await {
                                            warn!("Failed to send a pong response to the server: {e}");
                                        }
                                    }
                                    tungstenite::Message::Pong(_) => {
                                        // log pongs
                                        debug!("Received a pong");
                                    }
                                    tungstenite::Message::Text(text) => {
                                        debug!("Received message: {}", text);
                                        QueueClient::handle_text_message(text, &response_queue).await;
                                    }
                                    _ => {
                                        // log and ignore bad messages
                                        warn!("Unexpected message (possibly a bug): {msg:?}");
                                    }
                                },
                                Some(Err(e)) => {
                                    // complain about network errors and stop
                                    warn!("Read error: {e}");
                                    token.cancel();
                                }
                                None => break,
                            }
                        }
                    }
                }
            })
        };

        Ok(QueueClient {
            agent_id: config.agent_id,
            capabilities: config.capabilities.clone(),
            tx,
            cancellation_token,
            correlation_id_gen: Default::default(),
        })
    }

    pub async fn next_command(&self) -> Result<CommandResponse, ApiError> {
        let correlation_id = self.correlation_id_gen.next();

        let msg = Message::CommandRequest {
            correlation_id,
            agent_id: self.agent_id,
        };

        match self.send_and_wait_for_reply(correlation_id, msg).await {
            Ok(Message::CommandResponse {
                correlation_id: reply_correlation_id,
            }) => {
                if correlation_id == reply_correlation_id {
                    Ok(CommandResponse { correlation_id })
                } else {
                    api_err!("Unexpected correlation ID: {reply_correlation_id:?}")
                }
            }
            Ok(msg) => api_err!("Unexpected message: {msg:?}"),
            Err(e) => api_err!("Error while parsing message: {e}"),
        }
    }

    pub async fn next_process(&self) -> Result<ProcessResponse, ApiError> {
        let correlation_id = self.correlation_id_gen.next();

        let msg = Message::ProcessRequest {
            correlation_id,
            capabilities: serde_json::json!(&self.capabilities),
        };

        match self.send_and_wait_for_reply(correlation_id, msg).await {
            Ok(Message::ProcessResponse {
                correlation_id: reply_correlation_id,
                session_token,
                process_id,
            }) => {
                if correlation_id == reply_correlation_id {
                    Ok(ProcessResponse {
                        correlation_id,
                        session_token,
                        process_id,
                    })
                } else {
                    api_err!("Unexpected correlation ID: {reply_correlation_id:?}")
                }
            }
            Ok(msg) => api_err!("Unexpected message: {msg:?}"),
            Err(e) => api_err!("Error while parsing message: {e}"),
        }
    }

    async fn send_and_wait_for_reply(&self, correlation_id: CorrelationId, msg: Message) -> Reply {
        let json = serde_json::to_string(&msg)?;

        let (reply_sender, reply_receiver) = tokio::sync::oneshot::channel::<Reply>();

        let msg = MessageToSend::text(json, Responder(correlation_id, reply_sender));
        if let Err(e) = self.tx.send(msg).await {
            return api_err!("Send error: {e}");
        }

        match reply_receiver.await {
            Ok(reply) => reply,
            Err(e) => api_err!("Error while receiving reply: {e}"),
        }
    }

    async fn handle_text_message(text: Utf8Bytes, message_queue: &ResponseQueue) {
        match serde_json::from_str::<Message>(&text) {
            Ok(
                cmd @ Message::CommandResponse { correlation_id, .. }
                | cmd @ Message::ProcessResponse { correlation_id, .. },
            ) => {
                let mut message_queue = message_queue.lock().await;
                if let Some(Responder(_, responder)) = message_queue.remove(&correlation_id) {
                    if responder.send(Ok(cmd)).is_err() {
                        warn!("Responder error (most likely a bug)");
                    }
                }
            }
            Ok(msg) => {
                // log and ignore bad messages
                warn!("Unexpected message body (most likely a bug): {:?}", msg);
            }
            Err(e) => {
                // complain about parsing errors
                warn!("Error while parsing message (possibly a bug): {e}");
            }
        }
    }

    fn create_connect_request(
        Config {
            uri,
            api_token,
            agent_id,
            ..
        }: &Config,
    ) -> Result<http::Request<()>, ApiError> {
        let host = format!(
            "{}:{}",
            uri.host().unwrap_or("localhost"),
            uri.port_u16().unwrap_or(8001)
        );

        let ws_key = tungstenite::handshake::client::generate_key();

        use http::{Request, header};
        Request::builder()
            .uri(uri.clone())
            .header(header::HOST, host)
            .header(header::AUTHORIZATION, api_token)
            .header(header::CONNECTION, "Upgrade")
            .header(header::UPGRADE, "websocket")
            .header(header::SEC_WEBSOCKET_VERSION, "13")
            .header(header::SEC_WEBSOCKET_KEY, ws_key)
            .header(header::USER_AGENT, USER_AGENT_VALUE)
            .header("X-Concord-Agent-Id", agent_id)
            .header("X-Concord-Agent", USER_AGENT_VALUE)
            .body(())
            .map_err(|e| ApiError {
                message: e.to_string(),
            })
    }
}

impl Drop for QueueClient {
    fn drop(&mut self) {
        self.cancellation_token.cancel();
    }
}
