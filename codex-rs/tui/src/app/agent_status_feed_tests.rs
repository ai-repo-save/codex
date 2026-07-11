use super::*;
use codex_app_server_protocol::CollabAgentTool;
use codex_app_server_protocol::CollabAgentToolCallStatus;
use codex_app_server_protocol::CommandExecutionSource;
use codex_app_server_protocol::CommandExecutionStatus;
use codex_app_server_protocol::ItemCompletedNotification;
use codex_app_server_protocol::ItemStartedNotification;
use codex_app_server_protocol::SkillLoadStatus;
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
        Waiting for parent agent decision
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
        Parent agent decision timed out
    "###);
}

#[test]
fn agent_status_uses_bounded_buffered_activity() {
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
        $ cargo test -p codex-tui
        Finished checking the focused TUI tests.
    "###);
    assert!(!rendered.contains("unbounded output"));
}

#[test]
fn agent_status_uses_reasoning_summaries_only() {
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
        safe summary
    "###);
    assert!(!rendered.contains("hidden raw reasoning"));
    assert!(!rendered.contains("raw-only reasoning"));
}

#[test]
fn agent_status_summarizes_skill_load_items() {
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
        Read skill code-review
        Failed to read skill missing-skill
    "###);
    assert!(!rendered.contains("/skills/code-review"));
    assert!(!rendered.contains("not found"));
}

#[test]
fn agent_status_includes_started_agent_model() {
    let mut store = ThreadEventStore::new(/*capacity*/ 8);
    store.push_notification(ServerNotification::ItemCompleted(
        ItemCompletedNotification {
            item: ThreadItem::SubAgentActivity {
                id: "activity-1".to_string(),
                kind: SubAgentActivityKind::Started,
                agent_thread_id: "thread-child".to_string(),
                agent_path: "/root/reviewer".to_string(),
                model: Some("gpt-5.6".to_string()),
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
        Started /root/reviewer (gpt-5.6)
    "###);
}
