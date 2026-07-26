use schemars::JsonSchema;
use serde::Deserialize;
use serde::Deserializer;
use serde::Serialize;
use ts_rs::TS;
use unicode_segmentation::UnicodeSegmentation;

pub const AGENT_MAILBOX_ACTION_PREVIEW_MAX_GRAPHEMES: usize = 160;
pub const AGENT_MAILBOX_AGENT_PATH_MAX_GRAPHEMES: usize = 240;

/// A visible action performed through the durable agent mailbox.
#[derive(Debug, Clone, Serialize, TS, JsonSchema, PartialEq)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct AgentMailboxAction {
    id: String,
    status: AgentMailboxActionStatus,
    action: AgentMailboxActionKind,
}

impl AgentMailboxAction {
    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn status(&self) -> AgentMailboxActionStatus {
        self.status
    }

    pub fn action(&self) -> &AgentMailboxActionKind {
        &self.action
    }

    pub fn send(
        id: String,
        target: String,
        category: AgentMailboxMessageCategory,
        message: &str,
    ) -> Self {
        Self {
            id,
            status: AgentMailboxActionStatus::InProgress,
            action: AgentMailboxActionKind::Send {
                target: normalize_agent_path(&target),
                recipient: None,
                category,
                preview: first_non_empty_line_preview(message),
            },
        }
    }

    pub fn read(
        id: String,
        sender: Option<String>,
        category: Option<AgentMailboxMessageCategory>,
        limit: usize,
    ) -> Self {
        Self {
            id,
            status: AgentMailboxActionStatus::InProgress,
            action: AgentMailboxActionKind::Read {
                sender: sender.map(|sender| normalize_agent_path(&sender)),
                category,
                limit: u32::try_from(limit).unwrap_or(u32::MAX),
                messages: Vec::new(),
            },
        }
    }

    pub fn with_status(mut self, status: AgentMailboxActionStatus) -> Self {
        self.status = status;
        self
    }

    pub fn with_recipient(mut self, recipient: String) -> Self {
        if let AgentMailboxActionKind::Send { recipient: value, .. } = &mut self.action {
            *value = Some(normalize_agent_path(&recipient));
        }
        self
    }

    pub fn with_messages(mut self, messages: Vec<AgentMailboxMessagePreview>) -> Self {
        if let AgentMailboxActionKind::Read {
            messages: value, ..
        } = &mut self.action
        {
            *value = messages;
        }
        self
    }
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, TS, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub enum AgentMailboxActionStatus {
    InProgress,
    Succeeded,
    Failed,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, TS, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub enum AgentMailboxMessageCategory {
    Progress,
    Result,
    ActionRequired,
}

#[derive(Debug, Clone, Deserialize, Serialize, TS, JsonSchema, PartialEq)]
#[serde(tag = "type", rename_all = "camelCase")]
#[ts(tag = "type", rename_all = "camelCase")]
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

#[derive(Debug, Clone, Serialize, TS, JsonSchema, PartialEq)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct AgentMailboxMessagePreview {
    sender: String,
    category: AgentMailboxMessageCategory,
    content: AgentMailboxMessagePreviewContent,
}

impl AgentMailboxMessagePreview {
    pub fn plaintext(
        sender: String,
        category: AgentMailboxMessageCategory,
        content: &str,
    ) -> Self {
        Self {
            sender: normalize_agent_path(&sender),
            category,
            content: AgentMailboxMessagePreviewContent::Plaintext {
                preview: first_non_empty_line_preview(content),
            },
        }
    }

    pub fn encrypted(sender: String, category: AgentMailboxMessageCategory) -> Self {
        Self {
            sender: normalize_agent_path(&sender),
            category,
            content: AgentMailboxMessagePreviewContent::Encrypted,
        }
    }

    pub fn sender(&self) -> &str {
        &self.sender
    }

    pub fn category(&self) -> AgentMailboxMessageCategory {
        self.category
    }

    pub fn content(&self) -> &AgentMailboxMessagePreviewContent {
        &self.content
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, TS, JsonSchema, PartialEq)]
#[serde(tag = "type", rename_all = "camelCase")]
#[ts(tag = "type", rename_all = "camelCase")]
pub enum AgentMailboxMessagePreviewContent {
    Plaintext { preview: Option<String> },
    Encrypted,
}

impl AgentMailboxMessagePreviewContent {
    pub fn preview(&self) -> Option<&str> {
        match self {
            Self::Plaintext { preview } => preview.as_deref(),
            Self::Encrypted => None,
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SerializedAgentMailboxAction {
    id: String,
    status: AgentMailboxActionStatus,
    action: AgentMailboxActionKind,
}

impl<'de> Deserialize<'de> for AgentMailboxAction {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let item = SerializedAgentMailboxAction::deserialize(deserializer)?;
        Ok(Self {
            id: item.id,
            status: item.status,
            action: normalize_action(item.action),
        })
    }
}

impl<'de> Deserialize<'de> for AgentMailboxMessagePreview {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct SerializedMessage {
            sender: String,
            category: AgentMailboxMessageCategory,
            content: AgentMailboxMessagePreviewContent,
        }

        let message = SerializedMessage::deserialize(deserializer)?;
        Ok(Self {
            sender: normalize_agent_path(&message.sender),
            category: message.category,
            content: normalize_content(message.content),
        })
    }
}

fn normalize_action(action: AgentMailboxActionKind) -> AgentMailboxActionKind {
    match action {
        AgentMailboxActionKind::Send {
            target,
            recipient,
            category,
            preview,
        } => AgentMailboxActionKind::Send {
            target: normalize_agent_path(&target),
            recipient: recipient.map(|recipient| normalize_agent_path(&recipient)),
            category,
            preview: preview.and_then(|preview| first_non_empty_line_preview(&preview)),
        },
        AgentMailboxActionKind::Read {
            sender,
            category,
            limit,
            messages,
        } => AgentMailboxActionKind::Read {
            sender: sender.map(|sender| normalize_agent_path(&sender)),
            category,
            limit,
            messages,
        },
    }
}

fn normalize_content(
    content: AgentMailboxMessagePreviewContent,
) -> AgentMailboxMessagePreviewContent {
    match content {
        AgentMailboxMessagePreviewContent::Plaintext { preview } => {
            AgentMailboxMessagePreviewContent::Plaintext {
                preview: preview.and_then(|preview| first_non_empty_line_preview(&preview)),
            }
        }
        AgentMailboxMessagePreviewContent::Encrypted => AgentMailboxMessagePreviewContent::Encrypted,
    }
}

fn first_non_empty_line_preview(value: &str) -> Option<String> {
    value
        .lines()
        .find(|line| !line.trim().is_empty())
        .map(normalize_preview)
}

fn normalize_preview(value: &str) -> String {
    let collapsed = value.split_whitespace().collect::<Vec<_>>().join(" ");
    truncate_graphemes(&collapsed, AGENT_MAILBOX_ACTION_PREVIEW_MAX_GRAPHEMES)
}

fn normalize_agent_path(value: &str) -> String {
    truncate_graphemes(value, AGENT_MAILBOX_AGENT_PATH_MAX_GRAPHEMES)
}

fn truncate_graphemes(value: &str, max_graphemes: usize) -> String {
    value.graphemes(true).take(max_graphemes).collect()
}
