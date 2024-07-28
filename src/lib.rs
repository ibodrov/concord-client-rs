use std::fmt::Debug;

use futures_util::{SinkExt, StreamExt};
use http::{Request, Uri};
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
}

pub struct QueueClient {
    ws_stream: WebSocketStream<MaybeTlsStream<TcpStream>>,
}

impl QueueClient {
    pub async fn connect(uri: Uri, auth_header: String) -> Result<Self, ApiError> {
        let req = QueueClient::create_connect_request(uri.clone(), auth_header)?;

        let (ws_stream, _) = tokio_tungstenite::connect_async(req)
            .await
            .map_err(|e| ApiError {
                message: e.to_string(),
            })?;

        Ok(QueueClient { ws_stream })
    }

    pub async fn send_message(&mut self, message: Message) -> Result<(), ApiError> {
        let msg = serde_json::to_string(&message)?;
        let text = tungstenite::Message::Text(msg);
        self.ws_stream.send(text).await?;
        Ok(())
    }

    pub async fn receive_message(&mut self) -> Result<Option<Message>, ApiError> {
        let msg = self
            .ws_stream
            .next()
            .await
            .ok_or_else(|| ApiError::simple("No message received (possibly a bug)"))?;

        let text = match msg? {
            tungstenite::Message::Text(text) => text,
            tungstenite::Message::Ping(_) => {
                self.ws_stream
                    .send(tungstenite::Message::Pong(vec![]))
                    .await?;
                return Ok(None);
            }
            _ => return Err(ApiError::simple("Unexpected message type (possibly a bug)")),
        };

        let msg = serde_json::from_str::<Message>(&text)?;
        Ok(Some(msg))
    }

    fn create_connect_request(
        uri: Uri,
        auth_header: String,
    ) -> Result<http::Request<()>, ApiError> {
        let host = format!(
            "{}:{}",
            uri.host().unwrap_or("localhost"),
            uri.port_u16().unwrap_or(8001)
        );

        let ws_key = generate_key();

        Request::builder()
            .uri(uri.clone())
            .header("Host", host)
            .header("Authorization", auth_header)
            .header("Connection", "Upgrade")
            .header("Upgrade", "websocket")
            .header("Sec-WebSocket-Version", "13")
            .header("Sec-WebSocket-Key", ws_key)
            .header("User-Agent", "runtime-rs")
            .body(())
            .map_err(|e| ApiError {
                message: e.to_string(),
            })
    }
}
