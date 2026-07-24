use super::*;
use codex_app_server_protocol::CollabAgentTool;
use codex_app_server_protocol::CollabAgentToolCallStatus;
use codex_app_server_protocol::CommandExecutionSource;
use codex_app_server_protocol::CommandExecutionStatus;
use codex_app_server_protocol::ItemCompletedNotification;
use codex_app_server_protocol::ItemStartedNotification;
use codex_app_server_protocol::SkillLoadStatus;
use codex_app_server_protocol::SubAgentActivityKind;
use codex_app_server_protocol::SubAgentActivityOperation;
use codex_app_server_protocol::SubAgentActivityOutcome;
use codex_protocol::openai_models::ReasoningEffort;
use codex_utils_absolute_path::AbsolutePathBuf;

#[test]
fn agent_status_describes_pending_parent_decision() {
    let mut store = ThreadEventStore::new(/*capacity*/ 8);
    store.push_notification(ServerNotification::ItemStarted(ItemStartedNotification {
        item: ThreadItem::CollabAgentToolCall {
            id: "call-ask-parent".to_string(),
            tool: CollabAgentTool::AskParent,
            status: CollabAgentToolCallStatus::InProgress,
            sender_thread_id: "thread-child".to_string(),
            receiver_thread_ids: vec!["thread-parent".to_string()],
            prompt: Some("Choose the compatibility policy.".to_string()),
            model: None,
            reasoning_effort: None,
            service_tier: None,
            mode: None,
            snapshot_revision: None,
            agents_states: Default::default(),
        },
        thread_id: "thread-child".to_string(),
        turn_id: "turn-1".to_string(),
        started_at_ms: 1,
    }));

    let preview = AgentStatusThreadPreview::from_store("/root/worker".to_string(), &store);
    let cell = AgentStatusHistoryCell::new(vec![preview]);
    let rendered = cell
        .display_lines(/*width*/ 80)
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("\n");

    insta::assert_snapshot!(rendered, @r###"
    /agent
    Sub-agents running

      • `/root/worker`
        Waiting for parent decision
    "###);
}

#[test]
fn agent_status_distinguishes_parent_decision_timeout() {
    let mut store = ThreadEventStore::new(/*capacity*/ 8);
    store.push_notification(ServerNotification::ItemCompleted(
        ItemCompletedNotification {
            item: ThreadItem::CollabAgentToolCall {
                id: "call-ask-parent".to_string(),
                tool: CollabAgentTool::AskParent,
                status: CollabAgentToolCallStatus::Completed,
                sender_thread_id: "thread-child".to_string(),
                receiver_thread_ids: vec!["thread-parent".to_string()],
                prompt: Some("Choose the compatibility policy.".to_string()),
                model: None,
                reasoning_effort: None,
                service_tier: None,
                mode: None,
                snapshot_revision: None,
                agents_states: [(
                    "thread-parent".to_string(),
                    codex_app_server_protocol::CollabAgentState {
                        status: codex_app_server_protocol::CollabAgentStatus::Interrupted,
                        message: None,
                    },
                )]
                .into_iter()
                .collect(),
            },
            thread_id: "thread-child".to_string(),
            turn_id: "turn-1".to_string(),
            completed_at_ms: 1,
        },
    ));

    let preview = AgentStatusThreadPreview::from_store("/root/worker".to_string(), &store);
    let cell = AgentStatusHistoryCell::new(vec![preview]);
    let rendered = cell
        .display_lines(/*width*/ 80)
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("\n");

    insta::assert_snapshot!(rendered, @r###"
    /agent
    Sub-agents running

      • `/root/worker`
        Parent decision timed out
    "###);
}

#[test]
fn agent_status_excludes_command_and_message_bodies() {
    let mut store = ThreadEventStore::new(/*capacity*/ 8);
    store.push_notification(ServerNotification::ItemCompleted(
        ItemCompletedNotification {
            item: ThreadItem::CommandExecution {
                id: "command-1".to_string(),
                command: "cargo test -p codex-tui".to_string(),
                cwd: AbsolutePathBuf::try_from("/workspace")
                    .expect("absolute path")
                    .into(),
                process_id: None,
                source: CommandExecutionSource::Agent,
                status: CommandExecutionStatus::Completed,
                command_actions: Vec::new(),
                aggregated_output: Some("unbounded output\n".repeat(10_000)),
                exit_code: Some(0),
                duration_ms: Some(42),
            },
            thread_id: "thread-child".to_string(),
            turn_id: "turn-1".to_string(),
            completed_at_ms: 1,
        },
    ));
    store.push_notification(ServerNotification::ItemCompleted(
        ItemCompletedNotification {
            item: ThreadItem::AgentMessage {
                id: "message-1".to_string(),
                text: "Finished checking the focused TUI tests.".to_string(),
                phase: None,
                memory_citation: None,
            },
            thread_id: "thread-child".to_string(),
            turn_id: "turn-1".to_string(),
            completed_at_ms: 2,
        },
    ));

    let preview = AgentStatusThreadPreview::from_store("/root/reviewer".to_string(), &store);
    let cell = AgentStatusHistoryCell::new(vec![preview]);
    let rendered = cell
        .display_lines(/*width*/ 80)
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("\n");

    insta::assert_snapshot!(rendered, @r###"
    /agent
    Sub-agents running

      • `/root/reviewer`
        No recent activity yet.
    "###);
    assert!(!rendered.contains("cargo test -p codex-tui"));
    assert!(!rendered.contains("Finished checking the focused TUI tests."));
    assert!(!rendered.contains("unbounded output"));
}

#[test]
fn agent_status_excludes_reasoning_bodies() {
    let mut store = ThreadEventStore::new(/*capacity*/ 8);
    store.push_notification(ServerNotification::ItemCompleted(
        ItemCompletedNotification {
            item: ThreadItem::Reasoning {
                id: "reasoning-with-summary".to_string(),
                summary: vec!["safe summary".to_string()],
                content: vec!["hidden raw reasoning".to_string()],
            },
            thread_id: "thread-child".to_string(),
            turn_id: "turn-1".to_string(),
            completed_at_ms: 1,
        },
    ));
    store.push_notification(ServerNotification::ItemCompleted(
        ItemCompletedNotification {
            item: ThreadItem::Reasoning {
                id: "reasoning-without-summary".to_string(),
                summary: Vec::new(),
                content: vec!["raw-only reasoning".to_string()],
            },
            thread_id: "thread-child".to_string(),
            turn_id: "turn-1".to_string(),
            completed_at_ms: 2,
        },
    ));

    let preview = AgentStatusThreadPreview::from_store("/root/reviewer".to_string(), &store);
    let cell = AgentStatusHistoryCell::new(vec![preview]);
    let rendered = cell
        .display_lines(/*width*/ 80)
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("\n");

    insta::assert_snapshot!(rendered, @r###"
    /agent
    Sub-agents running

      • `/root/reviewer`
        No recent activity yet.
    "###);
    assert!(!rendered.contains("safe summary"));
    assert!(!rendered.contains("hidden raw reasoning"));
    assert!(!rendered.contains("raw-only reasoning"));
}

#[test]
fn agent_status_excludes_skill_details() {
    let mut store = ThreadEventStore::new(/*capacity*/ 8);
    store.push_notification(ServerNotification::ItemCompleted(
        ItemCompletedNotification {
            item: ThreadItem::SkillLoad {
                id: "skill-load-1".to_string(),
                name: "code-review".to_string(),
                path: Some(
                    AbsolutePathBuf::try_from("/skills/code-review").expect("absolute path"),
                ),
                status: SkillLoadStatus::Completed,
                error: None,
            },
            thread_id: "thread-child".to_string(),
            turn_id: "turn-1".to_string(),
            completed_at_ms: 1,
        },
    ));
    store.push_notification(ServerNotification::ItemCompleted(
        ItemCompletedNotification {
            item: ThreadItem::SkillLoad {
                id: "skill-load-2".to_string(),
                name: "missing-skill".to_string(),
                path: None,
                status: SkillLoadStatus::Failed,
                error: Some("not found".to_string()),
            },
            thread_id: "thread-child".to_string(),
            turn_id: "turn-1".to_string(),
            completed_at_ms: 2,
        },
    ));

    let preview = AgentStatusThreadPreview::from_store("/root/reviewer".to_string(), &store);
    let cell = AgentStatusHistoryCell::new(vec![preview]);
    let rendered = cell
        .display_lines(/*width*/ 80)
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("\n");

    insta::assert_snapshot!(rendered, @r###"
    /agent
    Sub-agents running

      • `/root/reviewer`
        No recent activity yet.
    "###);
    assert!(!rendered.contains("code-review"));
    assert!(!rendered.contains("missing-skill"));
    assert!(!rendered.contains("/skills/code-review"));
    assert!(!rendered.contains("not found"));
}

#[test]
fn agent_status_describes_started_agent_with_model_and_effort() {
    let mut store = ThreadEventStore::new(/*capacity*/ 8);
    store.push_notification(ServerNotification::ItemCompleted(
        ItemCompletedNotification {
            item: ThreadItem::SubAgentActivity {
                id: "activity-1".to_string(),
                kind: SubAgentActivityKind::Started,
                agent_thread_id: "thread-child".to_string(),
                agent_path: "/root/reviewer".to_string(),
                operation: None,
                outcome: None,
                model: Some("gpt-5.6".to_string()),
                reasoning_effort: Some(ReasoningEffort::High),
                service_tier: None,
            },
            thread_id: "thread-child".to_string(),
            turn_id: "turn-1".to_string(),
            completed_at_ms: 1,
        },
    ));

    let preview = AgentStatusThreadPreview::from_store("/root/reviewer".to_string(), &store);
    let cell = AgentStatusHistoryCell::new(vec![preview]);
    let rendered = cell
        .display_lines(/*width*/ 80)
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("\n");

    insta::assert_snapshot!(rendered, @r###"
    /agent
    Sub-agents running

      • `/root/reviewer`
        Started /root/reviewer (gpt-5.6, high)
    "###);
}

#[test]
fn agent_status_summarizes_sub_agent_activity_operations() {
    let items = [
        sub_agent_activity_item(
            "send-message",
            SubAgentActivityOperation::SendMessage,
            SubAgentActivityOutcome::Succeeded,
        ),
        sub_agent_activity_item(
            "send-message-failed",
            SubAgentActivityOperation::SendMessage,
            SubAgentActivityOutcome::Failed,
        ),
        sub_agent_activity_item(
            "followup",
            SubAgentActivityOperation::FollowupTask,
            SubAgentActivityOutcome::Succeeded,
        ),
        sub_agent_activity_item(
            "followup-failed",
            SubAgentActivityOperation::FollowupTask,
            SubAgentActivityOutcome::Failed,
        ),
        sub_agent_activity_item(
            "reply",
            SubAgentActivityOperation::ParentReply,
            SubAgentActivityOutcome::Succeeded,
        ),
        sub_agent_activity_item(
            "reply-failed",
            SubAgentActivityOperation::ParentReply,
            SubAgentActivityOutcome::Failed,
        ),
        sub_agent_activity_item(
            "inspect",
            SubAgentActivityOperation::InspectAgent,
            SubAgentActivityOutcome::Succeeded,
        ),
        sub_agent_activity_item(
            "inspect-failed",
            SubAgentActivityOperation::InspectAgent,
            SubAgentActivityOutcome::Failed,
        ),
    ];

    let rendered = items
        .iter()
        .filter_map(activity_summary)
        .collect::<Vec<_>>()
        .join("\n");

    insta::assert_snapshot!(rendered, @r###"
    Sent message to /root/reviewer
    Failed to send message to /root/reviewer
    Sent follow-up to /root/reviewer
    Failed to send follow-up to /root/reviewer
    Replied to /root/reviewer
    Failed to reply to /root/reviewer
    Inspected /root/reviewer
    Failed to inspect /root/reviewer
    "###);
}

fn sub_agent_activity_item(
    id: &str,
    operation: SubAgentActivityOperation,
    outcome: SubAgentActivityOutcome,
) -> ThreadItem {
    ThreadItem::SubAgentActivity {
        id: id.to_string(),
        kind: SubAgentActivityKind::Interacted,
        agent_thread_id: "thread-child".to_string(),
        agent_path: "/root/reviewer".to_string(),
        operation: Some(operation),
        outcome: Some(outcome),
        model: None,
        reasoning_effort: None,
        service_tier: None,
    }
}
