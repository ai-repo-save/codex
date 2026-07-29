use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use chrono::DateTime;
use chrono::Utc;
use codex_extension_api::AgentMailboxHost;
use codex_extension_api::AgentMailboxHostError;
use codex_extension_api::AgentMailboxHostHandle;
use codex_extension_api::AgentMailboxTarget;
use codex_extension_api::ExtensionTurnItem;
use codex_extension_api::FunctionCallError;
use codex_extension_api::NoopAgentMailboxHost;
use codex_extension_api::NoopTurnItemEmitter;
use codex_extension_api::ToolCall;
use codex_extension_api::ToolExecutor;
use codex_extension_api::ToolName;
use codex_extension_api::ToolPayload;
use codex_extension_api::TurnItemEmissionFuture;
use codex_extension_api::TurnItemEmitter;
use codex_extension_items::ExtensionItem;
use codex_extension_items::agent_mailbox_action::AgentMailboxAction;
use codex_extension_items::agent_mailbox_action::AgentMailboxActionStatus;
use codex_extension_items::agent_mailbox_action::AgentMailboxMessageCategory;
use codex_extension_items::agent_mailbox_action::AgentMailboxMessagePreview;
use codex_protocol::AgentPath;
use codex_protocol::ThreadId;
use codex_protocol::models::FunctionCallOutputBody;
use codex_protocol::models::FunctionCallOutputContentItem;
use codex_protocol::models::ResponseInputItem;
use codex_protocol::protocol::TruncationPolicy;
use codex_state::AgentMailboxCategory;
use codex_state::AgentMailboxMessageInput;
use codex_state::AgentMailboxPayload;
use codex_state::MAX_AGENT_MAILBOX_READ_LIMIT;
use codex_state::SqliteConfig;
use codex_state::StateRuntime;
use pretty_assertions::assert_eq;
use serde_json::json;

use super::AgentMailboxTool;
use crate::AGENT_MAILBOX_NAMESPACE;
use crate::MAX_AGENT_MAILBOX_PAYLOAD_BYTES;
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
const LARGE_MESSAGE_BYTES: usize = 512;
const READ_OUTPUT_POLICY_BYTES: usize = 1_200;
const RECEIVED_AT_SECONDS: i64 = 1_700_000_000;

#[tokio::test]
async fn send_emits_a_resolved_agent_mailbox_action_lifecycle() -> anyhow::Result<()> {
    let temporary_home = tempfile::tempdir()?;
    let state = StateRuntime::init(
        SqliteConfig::new_for_testing(temporary_home.path().try_into()?),
        "test".to_string(),
    )
    .await?;
    let recipient_thread_id = thread_id("00000000-0000-0000-0000-000000000803");
    let tool = AgentMailboxTool::send(
        runtime(thread_id(SENDER_THREAD_ID), AgentPath::root()),
        state,
        AgentMailboxHostHandle::new(ResolvedTargetHost {
            target: AgentMailboxTarget {
                thread_id: recipient_thread_id,
                agent_path: AgentPath::from_string("/root/recipient".to_string())
                    .expect("mailbox test recipient path should be valid"),
            },
        }),
        Arc::new(NoopAgentMailboxStatusNotifier),
    );
    let emitter = Arc::new(RecordingTurnItemEmitter::default());

    tool.handle(tool_call_with_emitter(
        SEND_TOOL_NAME,
        function_payload(json!({
            "target": "/root/recipient",
            "message": "\n  completed\twork\nignored",
            "category": "result",
        })),
        emitter.clone(),
    ))
    .await
    .expect("mailbox send should succeed");

    let started = AgentMailboxAction::send(
        "call-1".to_string(),
        "/root/recipient".to_string(),
        AgentMailboxMessageCategory::Result,
        "completed work",
    );
    let completed = started
        .clone()
        .with_recipient("/root/recipient".to_string())
        .with_status(AgentMailboxActionStatus::Succeeded);
    assert_eq!((vec![started], vec![completed]), emitter.actions());
    Ok(())
}

#[tokio::test]
async fn read_emits_consumed_messages_without_exposing_encrypted_content() -> anyhow::Result<()> {
    let temporary_home = tempfile::tempdir()?;
    let state = StateRuntime::init(
        SqliteConfig::new_for_testing(temporary_home.path().try_into()?),
        "test".to_string(),
    )
    .await?;
    let root_thread_id = thread_id(ROOT_THREAD_ID);
    let sender_thread_id = thread_id(SENDER_THREAD_ID);
    state
        .agent_mailbox()
        .enqueue(message_with_content(
            MESSAGE_ONE_ID,
            root_thread_id,
            sender_thread_id,
            "\n  first\tmessage\nignored".to_string(),
        ))
        .await?;
    state
        .agent_mailbox()
        .enqueue(AgentMailboxMessageInput {
            id: MESSAGE_TWO_ID.to_string(),
            root_thread_id,
            sender_thread_id,
            sender_agent_path: "/root/other".to_string(),
            recipient_thread_id: root_thread_id,
            recipient_agent_path: "/root".to_string(),
            category: AgentMailboxCategory::ActionRequired,
            payload: AgentMailboxPayload::Encrypted {
                encrypted_content: "encrypted-mailbox-body".to_string(),
            },
            created_at: DateTime::<Utc>::from_timestamp(RECEIVED_AT_SECONDS, 0)
                .expect("mailbox test timestamp should be valid"),
        })
        .await?;
    let tool = AgentMailboxTool::read(
        runtime(root_thread_id, AgentPath::root()),
        state,
        noop_host(),
        Arc::new(NoopAgentMailboxStatusNotifier),
    );
    let emitter = Arc::new(RecordingTurnItemEmitter::default());

    tool.handle(tool_call_with_emitter(
        READ_TOOL_NAME,
        function_payload(json!({ "limit": 2 })),
        emitter.clone(),
    ))
    .await
    .expect("mailbox read should succeed");

    let started = AgentMailboxAction::read("call-1".to_string(), None, None, 2);
    let completed = started
        .clone()
        .with_messages(vec![
            AgentMailboxMessagePreview::plaintext(
                "/root/worker".to_string(),
                AgentMailboxMessageCategory::Result,
                "first message",
            ),
            AgentMailboxMessagePreview::encrypted(
                "/root/other".to_string(),
                AgentMailboxMessageCategory::ActionRequired,
            ),
        ])
        .with_status(AgentMailboxActionStatus::Succeeded);
    assert_eq!((vec![started], vec![completed]), emitter.actions());
    Ok(())
}

#[tokio::test]
async fn read_sender_filter_failure_emits_a_failed_action() -> anyhow::Result<()> {
    let temporary_home = tempfile::tempdir()?;
    let state = StateRuntime::init(
        SqliteConfig::new_for_testing(temporary_home.path().try_into()?),
        "test".to_string(),
    )
    .await?;
    let tool = AgentMailboxTool::read(
        runtime(thread_id(ROOT_THREAD_ID), AgentPath::root()),
        state,
        noop_host(),
        Arc::new(NoopAgentMailboxStatusNotifier),
    );
    let emitter = Arc::new(RecordingTurnItemEmitter::default());

    let result = tool
        .handle(tool_call_with_emitter(
            READ_TOOL_NAME,
            function_payload(json!({ "sender": "../sibling" })),
            emitter.clone(),
        ))
        .await;

    assert!(matches!(result, Err(FunctionCallError::RespondToModel(_))));
    let started = AgentMailboxAction::read(
        "call-1".to_string(),
        Some("../sibling".to_string()),
        None,
        1,
    );
    let failed = started
        .clone()
        .with_status(AgentMailboxActionStatus::Failed);
    assert_eq!((vec![started], vec![failed]), emitter.actions());
    Ok(())
}

#[tokio::test]
async fn send_rejects_a_message_larger_than_the_storage_payload_limit() -> anyhow::Result<()> {
    let temporary_home = tempfile::tempdir()?;
    let state = StateRuntime::init(
        SqliteConfig::new_for_testing(temporary_home.path().try_into()?),
        "test".to_string(),
    )
    .await?;
    let tool = AgentMailboxTool::send(
        runtime(thread_id(SENDER_THREAD_ID), AgentPath::root()),
        state,
        noop_host(),
        Arc::new(NoopAgentMailboxStatusNotifier),
    );
    let result = tool
        .handle(tool_call(
            SEND_TOOL_NAME,
            json!({
                "target": "/root",
                "message": "x".repeat(MAX_AGENT_MAILBOX_PAYLOAD_BYTES + 1),
                "category": "result",
            }),
        ))
        .await;

    let Err(FunctionCallError::RespondToModel(message)) = result else {
        panic!("oversized mailbox message should be rejected");
    };
    assert_eq!(
        format!("agent mailbox message exceeds the {MAX_AGENT_MAILBOX_PAYLOAD_BYTES}-byte limit"),
        message
    );
    Ok(())
}

#[tokio::test]
async fn user_requested_batch_limit_leaves_mailbox_unchanged_when_rejected() -> anyhow::Result<()> {
    let temporary_home = tempfile::tempdir()?;
    let state = StateRuntime::init(
        SqliteConfig::new_for_testing(temporary_home.path().try_into()?),
        "test".to_string(),
    )
    .await?;
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
        noop_host(),
        Arc::new(NoopAgentMailboxStatusNotifier),
    );

    let oversized_read = tool
        .handle(tool_call(
            READ_TOOL_NAME,
            json!({ "limit": MAX_AGENT_MAILBOX_READ_LIMIT + 1 }),
        ))
        .await;
    let Err(FunctionCallError::RespondToModel(error)) = oversized_read else {
        panic!("read larger than the storage query limit should be rejected");
    };
    assert_eq!(
        format!("agent mailbox read limit must be at most {MAX_AGENT_MAILBOX_READ_LIMIT}"),
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

    tool.handle(tool_call(READ_TOOL_NAME, json!({ "limit": 2 })))
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

#[tokio::test]
async fn read_consumes_only_messages_fully_delivered_within_invocation_budget() -> anyhow::Result<()>
{
    let temporary_home = tempfile::tempdir()?;
    let state = StateRuntime::init(
        SqliteConfig::new_for_testing(temporary_home.path().try_into()?),
        "test".to_string(),
    )
    .await?;
    let root_thread_id = thread_id(ROOT_THREAD_ID);
    let sender_thread_id = thread_id(SENDER_THREAD_ID);
    let content = "x".repeat(LARGE_MESSAGE_BYTES);
    for id in [MESSAGE_ONE_ID, MESSAGE_TWO_ID, MESSAGE_THREE_ID] {
        state
            .agent_mailbox()
            .enqueue(message_with_content(
                id,
                root_thread_id,
                sender_thread_id,
                content.clone(),
            ))
            .await?;
    }
    let tool = AgentMailboxTool::read(
        runtime(root_thread_id, AgentPath::root()),
        Arc::clone(&state),
        noop_host(),
        Arc::new(NoopAgentMailboxStatusNotifier),
    );

    let payload = function_payload(json!({ "limit": 3 }));
    let output = tool
        .handle(tool_call_with_policy(
            READ_TOOL_NAME,
            payload.clone(),
            TruncationPolicy::Bytes(READ_OUTPUT_POLICY_BYTES),
        ))
        .await
        .expect("budgeted mailbox read should succeed");
    let ResponseInputItem::FunctionCallOutput {
        output: response_output,
        ..
    } = output.to_response_item("call-1", &payload)
    else {
        panic!("mailbox read should return function call output");
    };
    let FunctionCallOutputBody::ContentItems(content_items) = response_output.body else {
        panic!("mailbox read should return structured content items");
    };
    assert_eq!(
        vec![
            FunctionCallOutputContentItem::InputText {
                text: json!({
                    "message": {
                        "id": MESSAGE_ONE_ID,
                        "sender": "/root/worker",
                        "senderThreadId": sender_thread_id,
                        "category": "result",
                        "receivedAt": RECEIVED_AT_SECONDS,
                    },
                    "content": content,
                })
                .to_string(),
            },
            FunctionCallOutputContentItem::InputText {
                text: json!({
                    "remaining": {
                        "total": 2,
                        "progress": 0,
                        "result": 2,
                        "actionRequired": 0,
                        "revision": 4,
                    },
                })
                .to_string(),
            },
        ],
        content_items
    );
    assert_eq!(
        2,
        state
            .agent_mailbox()
            .unread_snapshot(root_thread_id, root_thread_id)
            .await?
            .total
    );
    Ok(())
}

fn noop_host() -> AgentMailboxHostHandle {
    AgentMailboxHostHandle::new(NoopAgentMailboxHost)
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

fn message(
    id: &str,
    root_thread_id: ThreadId,
    sender_thread_id: ThreadId,
) -> AgentMailboxMessageInput {
    message_with_content(
        id,
        root_thread_id,
        sender_thread_id,
        MESSAGE_CONTENT.to_string(),
    )
}

fn message_with_content(
    id: &str,
    root_thread_id: ThreadId,
    sender_thread_id: ThreadId,
    content: String,
) -> AgentMailboxMessageInput {
    AgentMailboxMessageInput {
        id: id.to_string(),
        root_thread_id,
        sender_thread_id,
        sender_agent_path: "/root/worker".to_string(),
        recipient_thread_id: root_thread_id,
        recipient_agent_path: "/root".to_string(),
        category: AgentMailboxCategory::Result,
        payload: AgentMailboxPayload::Plaintext { content },
        created_at: DateTime::<Utc>::from_timestamp(RECEIVED_AT_SECONDS, 0)
            .expect("mailbox test timestamp should be valid"),
    }
}

fn tool_call(name: &str, arguments: serde_json::Value) -> ToolCall {
    tool_call_with_policy(
        name,
        function_payload(arguments),
        TruncationPolicy::Bytes(10_000),
    )
}

fn tool_call_with_emitter(
    name: &str,
    payload: ToolPayload,
    turn_item_emitter: Arc<dyn TurnItemEmitter>,
) -> ToolCall {
    ToolCall {
        turn_id: "turn-1".to_string(),
        call_id: "call-1".to_string(),
        tool_name: ToolName::namespaced(AGENT_MAILBOX_NAMESPACE, name),
        model: "gpt-test".to_string(),
        codex_turn_metadata: None,
        truncation_policy: TruncationPolicy::Bytes(10_000),
        conversation_history: Default::default(),
        turn_item_emitter,
        environments: Vec::new(),
        payload,
    }
}

fn tool_call_with_policy(
    name: &str,
    payload: ToolPayload,
    truncation_policy: TruncationPolicy,
) -> ToolCall {
    ToolCall {
        turn_id: "turn-1".to_string(),
        call_id: "call-1".to_string(),
        tool_name: ToolName::namespaced(AGENT_MAILBOX_NAMESPACE, name),
        model: "gpt-test".to_string(),
        codex_turn_metadata: None,
        truncation_policy,
        conversation_history: Default::default(),
        turn_item_emitter: Arc::new(NoopTurnItemEmitter),
        environments: Vec::new(),
        payload,
    }
}

fn function_payload(arguments: serde_json::Value) -> ToolPayload {
    ToolPayload::Function {
        arguments: arguments.to_string(),
    }
}

fn thread_id(value: &str) -> ThreadId {
    ThreadId::from_string(value).expect("mailbox test thread ID should be valid")
}

struct ResolvedTargetHost {
    target: AgentMailboxTarget,
}

impl AgentMailboxHost for ResolvedTargetHost {
    fn resolve_target(
        &self,
        _current_thread_id: ThreadId,
        _target: &str,
    ) -> impl std::future::Future<Output = Result<AgentMailboxTarget, AgentMailboxHostError>> + Send
    {
        std::future::ready(Ok(self.target.clone()))
    }

    fn notify_activity(
        &self,
        _recipient_thread_id: ThreadId,
    ) -> impl std::future::Future<Output = Result<(), AgentMailboxHostError>> + Send {
        std::future::ready(Ok(()))
    }
}

#[derive(Default)]
struct RecordingTurnItemEmitter {
    started: std::sync::Mutex<Vec<ExtensionTurnItem>>,
    completed: std::sync::Mutex<Vec<ExtensionTurnItem>>,
}

impl RecordingTurnItemEmitter {
    fn actions(&self) -> (Vec<AgentMailboxAction>, Vec<AgentMailboxAction>) {
        (
            self.started
                .lock()
                .expect("started item lock")
                .iter()
                .map(agent_mailbox_action)
                .collect(),
            self.completed
                .lock()
                .expect("completed item lock")
                .iter()
                .map(agent_mailbox_action)
                .collect(),
        )
    }
}

impl TurnItemEmitter for RecordingTurnItemEmitter {
    fn emit_started<'a>(&'a self, item: ExtensionTurnItem) -> TurnItemEmissionFuture<'a> {
        Box::pin(async move {
            self.started.lock().expect("started item lock").push(item);
        })
    }

    fn emit_completed<'a>(&'a self, item: ExtensionTurnItem) -> TurnItemEmissionFuture<'a> {
        Box::pin(async move {
            self.completed
                .lock()
                .expect("completed item lock")
                .push(item);
        })
    }
}

fn agent_mailbox_action(item: &ExtensionTurnItem) -> AgentMailboxAction {
    let ExtensionItem::AgentMailboxAction(action) = &item.item else {
        panic!("agent mailbox tools should only emit agent mailbox action items");
    };
    action.clone()
}
