use codex_extension_api::ToolOutput;
use codex_extension_api::ToolPayload;
use codex_protocol::models::FunctionCallOutputContentItem;
use codex_protocol::models::FunctionCallOutputPayload;
use codex_protocol::models::ResponseInputItem;
use codex_state::AgentMailboxMessage;
use codex_state::AgentMailboxPayload;
use codex_state::AgentMailboxUnreadSnapshot;
use serde_json::Value;
use serde_json::json;

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
                    content_items.push(FunctionCallOutputContentItem::InputText {
                        text: json!({
                            "message": metadata,
                            "content": content,
                        })
                        .to_string(),
                    });
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
        content_items.push(FunctionCallOutputContentItem::InputText {
            text: json!({ "remaining": remaining }).to_string(),
        });
        Self {
            content_items,
            sanitized: json!({
                "messages": sanitized_messages,
                "remaining": remaining,
            }),
        }
    }
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
