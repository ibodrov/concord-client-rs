use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Serialize, Deserialize)]
pub struct SessionToken(String);

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct ProcessId(pub Uuid);

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct AgentId(pub Uuid);
