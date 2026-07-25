use codex_extension_api::ToolOutput;
use codex_extension_api::ToolPayload;
use codex_protocol::models::FunctionCallOutputContentItem;
use codex_protocol::models::FunctionCallOutputPayload;
use codex_protocol::models::ResponseInputItem;
use codex_protocol::protocol::TruncationPolicy;
use codex_state::AgentMailboxMessage;
use codex_state::AgentMailboxPayload;
use codex_state::AgentMailboxUnreadSnapshot;
use serde_json::Value;
use serde_json::json;

use crate::MAX_AGENT_MAILBOX_PAYLOAD_BYTES;

pub(crate) struct AgentMailboxReadOutput {
    content_items: Vec<FunctionCallOutputContentItem>,
    sanitized: Value,
}

impl AgentMailboxReadOutput {
    pub(crate) fn new(
        messages: Vec<AgentMailboxMessage>,
        snapshot: AgentMailboxUnreadSnapshot,
    ) -> Self {
        let (content_items, sanitized) = render_output(&messages, &snapshot);
        Self {
            content_items,
            sanitized,
        }
    }

    pub(crate) fn fits_truncation_policy(
        messages: &[AgentMailboxMessage],
        snapshot: &AgentMailboxUnreadSnapshot,
        policy: TruncationPolicy,
    ) -> bool {
        let (content_items, _) = render_output(messages, snapshot);
        rendered_content_items_bytes(&content_items) <= policy.byte_budget()
    }
}

fn render_output(
    messages: &[AgentMailboxMessage],
    snapshot: &AgentMailboxUnreadSnapshot,
) -> (Vec<FunctionCallOutputContentItem>, Value) {
    let mut content_items = Vec::new();
    let mut sanitized_messages = Vec::new();
    for message in messages {
        let metadata = message_metadata(message);
        match &message.payload {
            AgentMailboxPayload::Plaintext { content } => {
                content_items.push(plaintext_content_item(metadata.clone(), content.clone()));
                sanitized_messages.push(json!({
                    "message": metadata,
                    "encrypted": false,
                }));
            }
            AgentMailboxPayload::Encrypted { encrypted_content } => {
                content_items.push(FunctionCallOutputContentItem::InputText {
                    text: json!({
                        "message": metadata,
                        "encrypted": true,
                    })
                    .to_string(),
                });
                content_items.push(FunctionCallOutputContentItem::EncryptedContent {
                    encrypted_content: encrypted_content.clone(),
                });
                sanitized_messages.push(json!({
                    "message": metadata,
                    "encrypted": true,
                }));
            }
        }
    }
    let remaining = snapshot_json(snapshot);
    content_items.push(remaining_content_item(remaining.clone()));
    (
        content_items,
        json!({
            "messages": sanitized_messages,
            "remaining": remaining,
        }),
    )
}

pub(crate) fn validate_payload(payload: &AgentMailboxPayload) -> Result<(), String> {
    let payload_bytes = match payload {
        AgentMailboxPayload::Plaintext { content } => content.len(),
        AgentMailboxPayload::Encrypted { encrypted_content } => encrypted_content.len(),
    };
    validate_payload_bytes(payload_bytes)
}

pub(crate) fn validate_payload_bytes(payload_bytes: usize) -> Result<(), String> {
    if payload_bytes > MAX_AGENT_MAILBOX_PAYLOAD_BYTES {
        return Err(format!(
            "agent mailbox message exceeds the {MAX_AGENT_MAILBOX_PAYLOAD_BYTES}-byte limit"
        ));
    }
    Ok(())
}

fn plaintext_content_item(
    metadata: serde_json::Value,
    content: String,
) -> FunctionCallOutputContentItem {
    FunctionCallOutputContentItem::InputText {
        text: json!({
            "message": metadata,
            "content": content,
        })
        .to_string(),
    }
}

fn remaining_content_item(remaining: serde_json::Value) -> FunctionCallOutputContentItem {
    FunctionCallOutputContentItem::InputText {
        text: json!({ "remaining": remaining }).to_string(),
    }
}

fn rendered_content_items_bytes(content_items: &[FunctionCallOutputContentItem]) -> usize {
    serde_json::to_vec(content_items)
        .expect("agent mailbox output content items should serialize")
        .len()
}

impl ToolOutput for AgentMailboxReadOutput {
    fn log_preview(&self) -> String {
        self.sanitized.to_string()
    }

    fn success_for_logging(&self) -> bool {
        true
    }

    fn to_response_item(&self, call_id: &str, _payload: &ToolPayload) -> ResponseInputItem {
        ResponseInputItem::FunctionCallOutput {
            call_id: call_id.to_string(),
            output: FunctionCallOutputPayload::from_content_items(self.content_items.clone()),
        }
    }

    fn post_tool_use_response(&self, _call_id: &str, _payload: &ToolPayload) -> Option<Value> {
        Some(self.sanitized.clone())
    }

    fn code_mode_result(&self, _payload: &ToolPayload) -> Value {
        self.sanitized.clone()
    }
}

pub(crate) fn snapshot_json(snapshot: &AgentMailboxUnreadSnapshot) -> Value {
    json!({
        "total": snapshot.total,
        "progress": snapshot.progress,
        "result": snapshot.result,
        "actionRequired": snapshot.action_required,
        "revision": snapshot.revision,
    })
}

fn message_metadata(message: &AgentMailboxMessage) -> Value {
    json!({
        "id": message.id,
        "sender": message.sender_agent_path,
        "senderThreadId": message.sender_thread_id,
        "category": message.category,
        "receivedAt": message.created_at.timestamp(),
    })
}

#[cfg(test)]
#[path = "output_tests.rs"]
mod tests;
