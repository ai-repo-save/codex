use std::sync::Arc;
use std::sync::Weak;
use std::sync::atomic::AtomicBool;

use chrono::Utc;
use codex_extension_api::FunctionCallError;
use codex_extension_api::NoopTurnItemEmitter;
use codex_extension_api::ToolCall;
use codex_extension_api::ToolExecutor;
use codex_extension_api::ToolName;
use codex_extension_api::ToolPayload;
use codex_protocol::AgentPath;
use codex_protocol::ThreadId;
use codex_protocol::protocol::TruncationPolicy;
use codex_state::AgentMailboxCategory;
use codex_state::AgentMailboxMessageInput;
use codex_state::AgentMailboxPayload;
use codex_state::StateRuntime;
use pretty_assertions::assert_eq;
use serde_json::json;

use super::AgentMailboxTool;
use crate::AGENT_MAILBOX_NAMESPACE;
use crate::MAX_AGENT_MAILBOX_PAYLOAD_BYTES;
use crate::MAX_AGENT_MAILBOX_READ_MESSAGES;
use crate::NoopAgentMailboxStatusNotifier;
use crate::READ_TOOL_NAME;
use crate::SEND_TOOL_NAME;
use crate::extension::AgentMailboxRuntime;

const ROOT_THREAD_ID: &str = "00000000-0000-0000-0000-000000000801";
const SENDER_THREAD_ID: &str = "00000000-0000-0000-0000-000000000802";
const MESSAGE_ONE_ID: &str = "message-one";
const MESSAGE_TWO_ID: &str = "message-two";
const MESSAGE_THREE_ID: &str = "message-three";
const MESSAGE_CONTENT: &str = "bounded mailbox content";

#[tokio::test]
async fn send_rejects_a_message_larger_than_the_model_input_budget() -> anyhow::Result<()> {
    let temporary_home = tempfile::tempdir()?;
    let state = Arc::new(StateRuntime::init(temporary_home.path().to_path_buf(), "test".to_string()).await?);
    let tool = AgentMailboxTool::send(
        runtime(thread_id(SENDER_THREAD_ID), AgentPath::root()),
        state,
        Weak::new(),
        Arc::new(NoopAgentMailboxStatusNotifier),
    );
    let result = tool.handle(tool_call(
        SEND_TOOL_NAME,
        json!({
            "target": "/root",
            "message": "x".repeat(MAX_AGENT_MAILBOX_PAYLOAD_BYTES + 1),
            "category": "result",
        }),
    )).await;

    let Err(FunctionCallError::RespondToModel(message)) = result else {
        panic!("oversized mailbox message should be rejected");
    };
    assert_eq!(
        format!(
            "agent mailbox message exceeds the {MAX_AGENT_MAILBOX_PAYLOAD_BYTES}-byte limit"
        ),
        message
    );
    Ok(())
}

#[tokio::test]
async fn bounded_batch_read_leaves_later_messages_unread() -> anyhow::Result<()> {
    let temporary_home = tempfile::tempdir()?;
    let state = Arc::new(StateRuntime::init(temporary_home.path().to_path_buf(), "test".to_string()).await?);
    let root_thread_id = thread_id(ROOT_THREAD_ID);
    let sender_thread_id = thread_id(SENDER_THREAD_ID);
    for id in [MESSAGE_ONE_ID, MESSAGE_TWO_ID, MESSAGE_THREE_ID] {
        state
            .agent_mailbox()
            .enqueue(message(id, root_thread_id, sender_thread_id))
            .await?;
    }
    let tool = AgentMailboxTool::read(
        runtime(root_thread_id, AgentPath::root()),
        Arc::clone(&state),
        Weak::new(),
        Arc::new(NoopAgentMailboxStatusNotifier),
    );

    let oversized_read = tool
        .handle(tool_call(
            READ_TOOL_NAME,
            json!({ "limit": MAX_AGENT_MAILBOX_READ_MESSAGES + 1 }),
        ))
        .await;
    let Err(FunctionCallError::RespondToModel(error)) = oversized_read else {
        panic!("read larger than the output budget should be rejected");
    };
    assert_eq!(
        format!(
            "agent mailbox read limit must be at most {MAX_AGENT_MAILBOX_READ_MESSAGES} to fit the output budget"
        ),
        error
    );
    assert_eq!(
        3,
        state
            .agent_mailbox()
            .unread_snapshot(root_thread_id, root_thread_id)
            .await?
            .total
    );

    tool.handle(tool_call(READ_TOOL_NAME, json!({ "limit": MAX_AGENT_MAILBOX_READ_MESSAGES })))
        .await
        .expect("bounded batch read should succeed");
    assert_eq!(
        1,
        state
            .agent_mailbox()
            .unread_snapshot(root_thread_id, root_thread_id)
            .await?
            .total
    );

    tool.handle(tool_call(READ_TOOL_NAME, json!({ "limit": 1 })))
        .await
        .expect("remaining message should be readable later");
    assert_eq!(
        0,
        state
            .agent_mailbox()
            .unread_snapshot(root_thread_id, root_thread_id)
            .await?
            .total
    );
    Ok(())
}

fn runtime(mailbox_thread_id: ThreadId, agent_path: AgentPath) -> Arc<AgentMailboxRuntime> {
    Arc::new(AgentMailboxRuntime {
        thread_id: mailbox_thread_id,
        root_thread_id: thread_id(ROOT_THREAD_ID),
        agent_path,
        persistent_thread_state_available: true,
        enabled: AtomicBool::new(true),
    })
}

fn message(id: &str, root_thread_id: ThreadId, sender_thread_id: ThreadId) -> AgentMailboxMessageInput {
    AgentMailboxMessageInput {
        id: id.to_string(),
        root_thread_id,
        sender_thread_id,
        sender_agent_path: "/root/worker".to_string(),
        recipient_thread_id: root_thread_id,
        recipient_agent_path: "/root".to_string(),
        category: AgentMailboxCategory::Result,
        payload: AgentMailboxPayload::Plaintext {
            content: MESSAGE_CONTENT.to_string(),
        },
        created_at: Utc::now(),
    }
}

fn tool_call(name: &str, arguments: serde_json::Value) -> ToolCall {
    ToolCall {
        turn_id: "turn-1".to_string(),
        call_id: "call-1".to_string(),
        tool_name: ToolName::namespaced(AGENT_MAILBOX_NAMESPACE, name),
        model: "gpt-test".to_string(),
        codex_turn_metadata: None,
        truncation_policy: TruncationPolicy::Bytes(1024),
        conversation_history: Default::default(),
        turn_item_emitter: Arc::new(NoopTurnItemEmitter),
        environments: Vec::new(),
        payload: ToolPayload::Function {
            arguments: arguments.to_string(),
        },
    }
}

fn thread_id(value: &str) -> ThreadId {
    ThreadId::from_string(value).expect("mailbox test thread ID should be valid")
}
