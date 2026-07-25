use chrono::DateTime;
use codex_extension_api::ToolOutput;
use codex_extension_api::ToolPayload;
use codex_protocol::ThreadId;
use codex_protocol::models::FunctionCallOutputBody;
use codex_protocol::models::FunctionCallOutputContentItem;
use codex_protocol::models::ResponseInputItem;
use codex_state::AgentMailboxCategory;
use codex_state::AgentMailboxMessage;
use codex_state::AgentMailboxPayload;
use codex_state::AgentMailboxUnreadSnapshot;
use pretty_assertions::assert_eq;
use serde_json::json;

use super::AgentMailboxReadOutput;

const ROOT_THREAD_ID: &str = "00000000-0000-0000-0000-000000000601";
const SENDER_THREAD_ID: &str = "00000000-0000-0000-0000-000000000602";
const PLAINTEXT_ID: &str = "message-plaintext";
const ENCRYPTED_ID: &str = "message-encrypted";
const PLAINTEXT_BODY: &str = "visible only after an explicit mailbox read";
const ENCRYPTED_BODY: &str = "encrypted-message-body";

#[test]
fn read_output_keeps_encrypted_bodies_out_of_plain_json_envelopes() {
    let root_thread_id = thread_id(ROOT_THREAD_ID);
    let sender_thread_id = thread_id(SENDER_THREAD_ID);
    let snapshot = AgentMailboxUnreadSnapshot {
        total: 0,
        progress: 0,
        result: 0,
        action_required: 0,
        revision: 9,
    };
    let output = AgentMailboxReadOutput::new(
        vec![
            message(
                PLAINTEXT_ID,
                root_thread_id,
                sender_thread_id,
                AgentMailboxCategory::Result,
                AgentMailboxPayload::Plaintext {
                    content: PLAINTEXT_BODY.to_string(),
                },
                /*sequence*/ 1,
            ),
            message(
                ENCRYPTED_ID,
                root_thread_id,
                sender_thread_id,
                AgentMailboxCategory::ActionRequired,
                AgentMailboxPayload::Encrypted {
                    encrypted_content: ENCRYPTED_BODY.to_string(),
                },
                /*sequence*/ 2,
            ),
        ],
        snapshot,
    );

    let ResponseInputItem::FunctionCallOutput {
        output: response_output,
        ..
    } = output.to_response_item("call-read", &function_payload())
    else {
        panic!("agent mailbox read should return function call output");
    };
    let FunctionCallOutputBody::ContentItems(content_items) = response_output.body else {
        panic!("agent mailbox read should return structured content items");
    };
    assert_eq!(
        vec![
            FunctionCallOutputContentItem::InputText {
                text: json!({
                    "message": {
                        "id": PLAINTEXT_ID,
                        "sender": "/root/worker",
                        "senderThreadId": sender_thread_id,
                        "category": "result",
                        "receivedAt": 1_700_000_001_i64,
                    },
                    "content": PLAINTEXT_BODY,
                })
                .to_string(),
            },
            FunctionCallOutputContentItem::InputText {
                text: json!({
                    "message": {
                        "id": ENCRYPTED_ID,
                        "sender": "/root/worker",
                        "senderThreadId": sender_thread_id,
                        "category": "actionRequired",
                        "receivedAt": 1_700_000_002_i64,
                    },
                    "encrypted": true,
                })
                .to_string(),
            },
            FunctionCallOutputContentItem::EncryptedContent {
                encrypted_content: ENCRYPTED_BODY.to_string(),
            },
            FunctionCallOutputContentItem::InputText {
                text: json!({
                    "remaining": {
                        "total": 0,
                        "progress": 0,
                        "result": 0,
                        "actionRequired": 0,
                        "revision": 9,
                    },
                })
                .to_string(),
            },
        ],
        content_items
    );
    assert_eq!(
        json!({
            "messages": [
                {
                    "message": {
                        "id": PLAINTEXT_ID,
                        "sender": "/root/worker",
                        "senderThreadId": sender_thread_id,
                        "category": "result",
                        "receivedAt": 1_700_000_001_i64,
                    },
                    "encrypted": false,
                },
                {
                    "message": {
                        "id": ENCRYPTED_ID,
                        "sender": "/root/worker",
                        "senderThreadId": sender_thread_id,
                        "category": "actionRequired",
                        "receivedAt": 1_700_000_002_i64,
                    },
                    "encrypted": true,
                },
            ],
            "remaining": {
                "total": 0,
                "progress": 0,
                "result": 0,
                "actionRequired": 0,
                "revision": 9,
            },
        }),
        output.code_mode_result(&function_payload())
    );
}

fn message(
    id: &str,
    root_thread_id: ThreadId,
    sender_thread_id: ThreadId,
    category: AgentMailboxCategory,
    payload: AgentMailboxPayload,
    sequence: i64,
) -> AgentMailboxMessage {
    AgentMailboxMessage {
        id: id.to_string(),
        root_thread_id,
        sender_thread_id,
        sender_agent_path: "/root/worker".to_string(),
        recipient_thread_id: root_thread_id,
        recipient_agent_path: "/root".to_string(),
        category,
        payload,
        created_at: DateTime::from_timestamp(1_700_000_000 + sequence, 0)
            .expect("mailbox test timestamp should be valid"),
        sequence,
    }
}

fn thread_id(value: &str) -> ThreadId {
    ThreadId::from_string(value).expect("mailbox test thread ID should be valid")
}

fn function_payload() -> ToolPayload {
    ToolPayload::Function {
        arguments: "{}".to_string(),
    }
}
