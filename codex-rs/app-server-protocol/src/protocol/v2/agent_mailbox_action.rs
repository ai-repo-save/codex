use codex_extension_items::agent_mailbox_action::AgentMailboxAction as CoreAgentMailboxAction;
use codex_extension_items::agent_mailbox_action::AgentMailboxActionKind as CoreAgentMailboxActionKind;
use codex_extension_items::agent_mailbox_action::AgentMailboxActionStatus as CoreAgentMailboxActionStatus;
use codex_extension_items::agent_mailbox_action::AgentMailboxMessageCategory as CoreAgentMailboxMessageCategory;
use codex_extension_items::agent_mailbox_action::AgentMailboxMessagePreview as CoreAgentMailboxMessagePreview;
use codex_extension_items::agent_mailbox_action::AgentMailboxMessagePreviewContent as CoreAgentMailboxMessagePreviewContent;
use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;
use ts_rs::TS;

/// A visible action performed through the durable agent mailbox.
#[derive(Debug, Clone, Deserialize, Serialize, TS, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase", export_to = "v2/")]
pub struct AgentMailboxAction {
    pub id: String,
    pub status: AgentMailboxActionStatus,
    pub action: AgentMailboxActionKind,
}

impl From<CoreAgentMailboxAction> for AgentMailboxAction {
    fn from(value: CoreAgentMailboxAction) -> Self {
        Self {
            id: value.id().to_string(),
            status: value.status().into(),
            action: value.action().clone().into(),
        }
    }
}

/// Execution state for an agent mailbox action.
#[derive(Debug, Clone, Copy, Deserialize, Serialize, TS, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase", export_to = "v2/")]
pub enum AgentMailboxActionStatus {
    InProgress,
    Succeeded,
    Failed,
}

impl From<CoreAgentMailboxActionStatus> for AgentMailboxActionStatus {
    fn from(value: CoreAgentMailboxActionStatus) -> Self {
        match value {
            CoreAgentMailboxActionStatus::InProgress => Self::InProgress,
            CoreAgentMailboxActionStatus::Succeeded => Self::Succeeded,
            CoreAgentMailboxActionStatus::Failed => Self::Failed,
        }
    }
}

/// The mailbox operation represented by an action item.
#[derive(Debug, Clone, Deserialize, Serialize, TS, JsonSchema, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "camelCase")]
#[ts(tag = "type", rename_all = "camelCase", export_to = "v2/")]
pub enum AgentMailboxActionKind {
    Send {
        target: String,
        recipient: Option<String>,
        category: AgentMailboxMessageCategory,
        preview: Option<String>,
    },
    Read {
        sender: Option<String>,
        category: Option<AgentMailboxMessageCategory>,
        limit: u32,
        messages: Vec<AgentMailboxMessagePreview>,
    },
}

impl From<CoreAgentMailboxActionKind> for AgentMailboxActionKind {
    fn from(value: CoreAgentMailboxActionKind) -> Self {
        match value {
            CoreAgentMailboxActionKind::Send {
                target,
                recipient,
                category,
                preview,
            } => Self::Send {
                target,
                recipient,
                category: category.into(),
                preview,
            },
            CoreAgentMailboxActionKind::Read {
                sender,
                category,
                limit,
                messages,
            } => Self::Read {
                sender,
                category: category.map(Into::into),
                limit,
                messages: messages.into_iter().map(Into::into).collect(),
            },
        }
    }
}

/// A mailbox message summary exposed by a read action.
#[derive(Debug, Clone, Deserialize, Serialize, TS, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase", export_to = "v2/")]
pub struct AgentMailboxMessagePreview {
    pub sender: String,
    pub category: AgentMailboxMessageCategory,
    pub content: AgentMailboxMessagePreviewContent,
}

impl From<CoreAgentMailboxMessagePreview> for AgentMailboxMessagePreview {
    fn from(value: CoreAgentMailboxMessagePreview) -> Self {
        Self {
            sender: value.sender().to_string(),
            category: value.category().into(),
            content: value.content().clone().into(),
        }
    }
}

/// Category assigned to a mailbox message.
#[derive(Debug, Clone, Copy, Deserialize, Serialize, TS, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase", export_to = "v2/")]
pub enum AgentMailboxMessageCategory {
    Progress,
    Result,
    ActionRequired,
}

impl From<CoreAgentMailboxMessageCategory> for AgentMailboxMessageCategory {
    fn from(value: CoreAgentMailboxMessageCategory) -> Self {
        match value {
            CoreAgentMailboxMessageCategory::Progress => Self::Progress,
            CoreAgentMailboxMessageCategory::Result => Self::Result,
            CoreAgentMailboxMessageCategory::ActionRequired => Self::ActionRequired,
        }
    }
}

/// Safe content projection for a mailbox message.
#[derive(Debug, Clone, Deserialize, Serialize, TS, JsonSchema, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "camelCase")]
#[ts(tag = "type", rename_all = "camelCase", export_to = "v2/")]
pub enum AgentMailboxMessagePreviewContent {
    Plaintext { preview: Option<String> },
    Encrypted,
}

impl From<CoreAgentMailboxMessagePreviewContent> for AgentMailboxMessagePreviewContent {
    fn from(value: CoreAgentMailboxMessagePreviewContent) -> Self {
        match value {
            CoreAgentMailboxMessagePreviewContent::Plaintext { preview } => {
                Self::Plaintext { preview }
            }
            CoreAgentMailboxMessagePreviewContent::Encrypted => Self::Encrypted,
        }
    }
}
