use std::fmt::Debug;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub static USER_AGENT_VALUE: &str = concat!(env!("CARGO_PKG_NAME"), "/", env!("CARGO_PKG_VERSION"));

#[derive(Debug, Serialize, Deserialize)]
pub struct SessionToken(String);

#[derive(Clone, Serialize, Deserialize)]
pub struct ApiToken(String);

impl ApiToken {
    pub fn new(v: String) -> Self {
        Self(v)
    }
}

impl Debug for ApiToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("ApiToken").field(&"********").finish()
    }
}

impl TryFrom<&ApiToken> for http::HeaderValue {
    type Error = http::header::InvalidHeaderValue;

    fn try_from(value: &ApiToken) -> Result<Self, Self::Error> {
        http::HeaderValue::from_str(&value.0)
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct ProcessId(Uuid);

impl ProcessId {
    pub fn new(v: Uuid) -> Self {
        Self(v)
    }
}

impl std::fmt::Display for ProcessId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct AgentId(pub Uuid);

impl std::fmt::Display for AgentId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl TryFrom<&AgentId> for http::HeaderValue {
    type Error = http::header::InvalidHeaderValue;

    fn try_from(value: &AgentId) -> Result<Self, Self::Error> {
        http::HeaderValue::from_str(&value.0.to_string())
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum ProcessStatus {
    New,
    Preparing,
    Enqueued,
    Waiting,
    Starting,
    Running,
    Suspended,
    Resuming,
    Finished,
    Failed,
    Cancelled,
    TimedOut,
}

impl std::fmt::Display for ProcessStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ProcessStatus::New => write!(f, "NEW"),
            ProcessStatus::Preparing => write!(f, "PREPARING"),
            ProcessStatus::Enqueued => write!(f, "ENQUEUED"),
            ProcessStatus::Waiting => write!(f, "WAITING"),
            ProcessStatus::Starting => write!(f, "STARTING"),
            ProcessStatus::Running => write!(f, "RUNNING"),
            ProcessStatus::Suspended => write!(f, "SUSPENDED"),
            ProcessStatus::Resuming => write!(f, "RESUMING"),
            ProcessStatus::Finished => write!(f, "FINISHED"),
            ProcessStatus::Failed => write!(f, "FAILED"),
            ProcessStatus::Cancelled => write!(f, "CANCELLED"),
            ProcessStatus::TimedOut => write!(f, "TIMED_OUT"),
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SegmentCorrelationId(Uuid);

impl SegmentCorrelationId {
    pub fn new(v: Uuid) -> Self {
        Self(v)
    }
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct LogSegmentId(i64);

impl LogSegmentId {
    pub fn new(v: i64) -> Self {
        Self(v)
    }
}

impl std::fmt::Display for LogSegmentId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum LogSegmentStatus {
    Ok,
    Failed,
    Running,
    Suspended,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct LogSegmentRequest {
    #[serde(rename = "correlationId")]
    pub correlation_id: SegmentCorrelationId,
    pub name: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct LogSegmentOperationResponse {
    pub id: LogSegmentId,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct LogSegmentUpdateRequest {
    pub status: Option<LogSegmentStatus>,
    pub warnings: Option<u16>,
    pub errors: Option<u16>,
}
