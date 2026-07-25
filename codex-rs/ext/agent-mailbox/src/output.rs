use codex_extension_api::ToolOutput;
use codex_extension_api::ToolPayload;
use codex_protocol::models::FunctionCallOutputContentItem;
use codex_protocol::models::FunctionCallOutputPayload;
use codex_protocol::models::ResponseInputItem;
use codex_state::AgentMailboxMessage;
use codex_state::AgentMailboxMessageInput;
use codex_state::AgentMailboxPayload;
use codex_state::AgentMailboxUnreadSnapshot;
use serde_json::Value;
use serde_json::json;

use crate::MAX_AGENT_MAILBOX_PAYLOAD_BYTES;
use crate::MAX_AGENT_MAILBOX_READ_OUTPUT_BYTES;
use crate::MAX_AGENT_MAILBOX_SINGLE_OUTPUT_BYTES;

pub(crate) struct AgentMailboxReadOutput {
    content_items: Vec<FunctionCallOutputContentItem>,
    sanitized: Value,
}

impl AgentMailboxReadOutput {
    pub(crate) fn new(
        messages: Vec<AgentMailboxMessage>,
        snapshot: AgentMailboxUnreadSnapshot,
    ) -> Self {
        let mut content_items = Vec::new();
        let mut sanitized_messages = Vec::new();
        for message in messages {
            let metadata = message_metadata(&message);
            match message.payload {
                AgentMailboxPayload::Plaintext { content } => {
                    content_items.push(plaintext_content_item(metadata.clone(), content));
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
                        encrypted_content,
                    });
                    sanitized_messages.push(json!({
                        "message": metadata,
                        "encrypted": true,
                    }));
                }
            }
        }
        let remaining = snapshot_json(&snapshot);
        content_items.push(remaining_content_item(remaining.clone()));
        debug_assert!(
            rendered_content_items_bytes(&content_items) <= MAX_AGENT_MAILBOX_READ_OUTPUT_BYTES
        );
        Self {
            content_items,
            sanitized: json!({
                "messages": sanitized_messages,
                "remaining": remaining,
            }),
        }
    }
}

pub(crate) fn validate_message_input_for_read_output(
    input: &AgentMailboxMessageInput,
) -> Result<(), String> {
    let payload_bytes = match &input.payload {
        AgentMailboxPayload::Plaintext { content } => content.len(),
        AgentMailboxPayload::Encrypted { encrypted_content } => encrypted_content.len(),
    };
    if payload_bytes > MAX_AGENT_MAILBOX_PAYLOAD_BYTES {
        return Err(format!(
            "agent mailbox message exceeds the {MAX_AGENT_MAILBOX_PAYLOAD_BYTES}-byte limit"
        ));
    }

    let message = AgentMailboxMessage {
        id: input.id.clone(),
        root_thread_id: input.root_thread_id,
        sender_thread_id: input.sender_thread_id,
        sender_agent_path: input.sender_agent_path.clone(),
        recipient_thread_id: input.recipient_thread_id,
        recipient_agent_path: input.recipient_agent_path.clone(),
        category: input.category,
        payload: input.payload.clone(),
        created_at: input.created_at,
        sequence: 0,
    };
    if message_content_item_bytes(&message) > MAX_AGENT_MAILBOX_SINGLE_OUTPUT_BYTES {
        return Err("agent mailbox message metadata and content exceed the read output limit".to_string());
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

fn message_content_item_bytes(message: &AgentMailboxMessage) -> usize {
    let metadata = message_metadata(message);
    let content_items = match &message.payload {
        AgentMailboxPayload::Plaintext { content } => {
            vec![plaintext_content_item(metadata, content.clone())]
        }
        AgentMailboxPayload::Encrypted { encrypted_content } => vec![
            FunctionCallOutputContentItem::InputText {
                text: json!({
                    "message": metadata,
                    "encrypted": true,
                })
                .to_string(),
            },
            FunctionCallOutputContentItem::EncryptedContent {
                encrypted_content: encrypted_content.clone(),
            },
        ],
    };
    rendered_content_items_bytes(&content_items)
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
