use std::fmt::Debug;

use futures_util::{SinkExt, StreamExt};
use http::{
    header::{
        AUTHORIZATION, CONNECTION, HOST, SEC_WEBSOCKET_KEY, SEC_WEBSOCKET_VERSION, UPGRADE,
        USER_AGENT,
    },
    Request, Uri,
};
use serde::{Deserialize, Serialize};
use tokio::net::TcpStream;
use tokio_tungstenite::{
    tungstenite::{self, handshake::client::generate_key},
    MaybeTlsStream, WebSocketStream,
};
use uuid::Uuid;

#[derive(Debug)]
pub struct ApiError {
    pub message: String,
}

impl ApiError {
    pub fn simple(message: &str) -> Self {
        ApiError {
            message: message.to_string(),
        }
    }
}

impl std::error::Error for ApiError {}

impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl From<tungstenite::Error> for ApiError {
    fn from(e: tungstenite::Error) -> Self {
        ApiError {
            message: e.to_string(),
        }
    }
}

impl From<serde_json::Error> for ApiError {
    fn from(e: serde_json::Error) -> Self {
        ApiError {
            message: e.to_string(),
        }
    }
}

#[derive(Serialize, Deserialize)]
pub struct SessionToken(String);

impl Debug for SessionToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "SessionToken [REDACTED]")
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "messageType", rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Message {
    CommandRequest {
        #[serde(rename = "correlationId")]
        correlation_id: i64,
        agent_id: Uuid,
    },
    CommandResponse {
        #[serde(rename = "correlationId")]
        correlation_id: i64,
        // TODO
    },
    ProcessRequest {
        #[serde(rename = "correlationId")]
        correlation_id: i64,
        capabilities: serde_json::Value,
    },
    ProcessResponse {
        #[serde(rename = "correlationId")]
        correlation_id: i64,
        #[serde(rename = "sessionToken")]
        session_token: SessionToken,
        #[serde(rename = "processId")]
        process_id: Uuid,
        // TODO imports
    },
    Ping,
    Pong,
}

pub struct Config {
    pub uri: Uri,
    pub auth_header: String,
}

pub struct QueueClient {
    ws_stream: WebSocketStream<MaybeTlsStream<TcpStream>>,
}

impl QueueClient {
    pub async fn connect(config: Config) -> Result<Self, ApiError> {
        let req = QueueClient::create_connect_request(&config.uri, &config.auth_header)?;

        let (ws_stream, _) = tokio_tungstenite::connect_async(req).await?;

        Ok(QueueClient { ws_stream })
    }

    pub async fn send_message(&mut self, message: Message) -> Result<(), ApiError> {
        let msg = serde_json::to_string(&message)?;
        let text = tungstenite::Message::Text(msg);
        self.ws_stream.send(text).await?;
        Ok(())
    }

    pub async fn receive_message(&mut self) -> Result<Message, ApiError> {
        let msg = self
            .ws_stream
            .next()
            .await
            .ok_or_else(|| ApiError::simple("No message received (possibly a bug)"))?;

        let msg = match msg? {
            tungstenite::Message::Text(text) => serde_json::from_str::<Message>(&text)?,
            tungstenite::Message::Ping(_) => Message::Ping,
            tungstenite::Message::Pong(_) => Message::Pong,
            msg => {
                return Err(ApiError::simple(&format!(
                    "Unexpected message type (possibly a bug): {:?}",
                    msg
                )))
            }
        };

        Ok(msg)
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
