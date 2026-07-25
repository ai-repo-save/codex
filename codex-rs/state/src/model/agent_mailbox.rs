use anyhow::Result;
use anyhow::anyhow;
use chrono::DateTime;
use chrono::Utc;
use codex_protocol::ThreadId;
use serde::Deserialize;
use serde::Serialize;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum AgentMailboxCategory {
    Progress,
    Result,
    ActionRequired,
}

impl AgentMailboxCategory {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Progress => "progress",
            Self::Result => "result",
            Self::ActionRequired => "action_required",
        }
    }
}

impl TryFrom<&str> for AgentMailboxCategory {
    type Error = anyhow::Error;

    fn try_from(value: &str) -> Result<Self> {
        match value {
            "progress" => Ok(Self::Progress),
            "result" => Ok(Self::Result),
            "action_required" => Ok(Self::ActionRequired),
            other => Err(anyhow!("unknown agent mailbox category `{other}`")),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum AgentMailboxPayload {
    Plaintext { content: String },
    Encrypted { encrypted_content: String },
}

impl AgentMailboxPayload {
    pub(crate) fn kind_and_content(&self) -> (&'static str, &str) {
        match self {
            Self::Plaintext { content } => ("plaintext", content),
            Self::Encrypted { encrypted_content } => ("encrypted", encrypted_content),
        }
    }

    pub(crate) fn from_parts(kind: &str, content: String) -> Result<Self> {
        match kind {
            "plaintext" => Ok(Self::Plaintext { content }),
            "encrypted" => Ok(Self::Encrypted {
                encrypted_content: content,
            }),
            other => Err(anyhow!("unknown agent mailbox payload kind `{other}`")),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AgentMailboxMessageInput {
    pub id: String,
    pub root_thread_id: ThreadId,
    pub sender_thread_id: ThreadId,
    pub sender_agent_path: String,
    pub recipient_thread_id: ThreadId,
    pub recipient_agent_path: String,
    pub category: AgentMailboxCategory,
    pub payload: AgentMailboxPayload,
    pub created_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AgentMailboxMessage {
    pub id: String,
    pub root_thread_id: ThreadId,
    pub sender_thread_id: ThreadId,
    pub sender_agent_path: String,
    pub recipient_thread_id: ThreadId,
    pub recipient_agent_path: String,
    pub category: AgentMailboxCategory,
    pub payload: AgentMailboxPayload,
    pub created_at: DateTime<Utc>,
    pub sequence: i64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AgentMailboxUnreadSnapshot {
    pub total: i64,
    pub progress: i64,
    pub result: i64,
    pub action_required: i64,
    pub revision: i64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AgentMailboxEnqueueOutcome {
    pub inserted: bool,
    pub message: AgentMailboxMessage,
    pub snapshot: AgentMailboxUnreadSnapshot,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AgentMailboxReadRequest {
    pub root_thread_id: ThreadId,
    pub recipient_thread_id: ThreadId,
    pub sender_thread_id: Option<ThreadId>,
    pub sender_agent_path: Option<String>,
    pub category: Option<AgentMailboxCategory>,
    pub limit: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AgentMailboxReadOutcome {
    pub messages: Vec<AgentMailboxMessage>,
    pub snapshot: AgentMailboxUnreadSnapshot,
}
