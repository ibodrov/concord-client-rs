use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Serialize, Deserialize)]
pub struct SessionToken(String);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiToken(String);

impl ApiToken {
    pub fn new(v: String) -> Self {
        Self(v)
    }
}

impl TryFrom<ApiToken> for http::HeaderValue {
    type Error = http::header::InvalidHeaderValue;

    fn try_from(value: ApiToken) -> Result<Self, Self::Error> {
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
