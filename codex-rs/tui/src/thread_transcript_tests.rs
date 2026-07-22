use super::*;
use crate::test_support::PathBufExt;
use crate::test_support::test_path_buf;
use codex_app_server_protocol::CollabAgentTool;
use codex_app_server_protocol::CollabAgentToolCallStatus;
use codex_app_server_protocol::SubAgentActivityKind;
use codex_app_server_protocol::ThreadStatus;
use codex_app_server_protocol::Turn;
use codex_app_server_protocol::TurnItemsView;
use codex_app_server_protocol::TurnStatus;
use codex_protocol::openai_models::ReasoningEffort;

#[test]
fn persisted_multi_agent_items_render_safe_transcript_summaries() {
    let thread_id = ThreadId::new();
    let thread = Thread {
        id: thread_id.to_string(),
        extra: None,
        session_id: thread_id.to_string(),
        forked_from_id: None,
        parent_thread_id: None,
        preview: "preview".to_string(),
        ephemeral: false,
        history_mode: Default::default(),
        model_provider: "openai".to_string(),
        created_at: 1,
        updated_at: 1,
        recency_at: Some(1),
        status: ThreadStatus::Idle,
        path: None,
        cwd: test_path_buf("/tmp").abs(),
        cli_version: "0.0.0".to_string(),
        source: codex_app_server_protocol::SessionSource::Cli,
        thread_source: None,
        agent_nickname: None,
        agent_role: None,
        git_info: None,
        name: None,
        turns: vec![Turn {
            id: "turn-1".to_string(),
            items_view: TurnItemsView::Full,
            items: vec![
                ThreadItem::SubAgentActivity {
                    id: "activity-1".to_string(),
                    kind: SubAgentActivityKind::Started,
                    agent_thread_id: "00000000-0000-0000-0000-000000000002".to_string(),
                    agent_path: "/root/research".to_string(),
                    operation: None,
                    outcome: None,
                    model: Some("gpt-5.6".to_string()),
                    reasoning_effort: Some(ReasoningEffort::High),
                },
                ThreadItem::MemoryMutation(codex_app_server_protocol::MemoryMutation {
                    id: "memory-write-1".to_string(),
                    action: codex_app_server_protocol::MemoryMutationAction::Write,
                    scope: codex_app_server_protocol::MemoryMutationScope::Project,
                    status: codex_app_server_protocol::MemoryMutationStatus::Succeeded,
                    title: Some("Repository conventions".to_string()),
                    path: Some("memories/project/repository-conventions.md".to_string()),
                    preview: Some("Run focused tests remotely.".to_string()),
                }),
                ThreadItem::CollabAgentToolCall {
                    id: "spawn-1".to_string(),
                    tool: CollabAgentTool::SpawnAgent,
                    status: CollabAgentToolCallStatus::Completed,
                    sender_thread_id: thread_id.to_string(),
                    receiver_thread_ids: Vec::new(),
                    prompt: Some("private agent instructions".to_string()),
                    model: Some("gpt-5.6".to_string()),
                    reasoning_effort: None,
                    mode: None,
                    snapshot_revision: None,
                    agents_states: Default::default(),
                },
            ],
            status: TurnStatus::Completed,
            error: None,
            started_at: None,
            completed_at: None,
            duration_ms: None,
        }],
    };

    let rendered = thread_to_transcript_cells(&thread, RawReasoningVisibility::Hidden)
        .into_iter()
        .flat_map(|cell| cell.transcript_lines(/*width*/ 80))
        .map(|line| line.to_string())
        .collect::<Vec<_>>()
        .join("\n");

    insta::assert_snapshot!(
        rendered,
        @r###"
Started /root/research (gpt-5.6, high)
Wrote memory · scope: project · title: Repository conventions · path: memories/project/repository-conventions.md · preview: Run focused tests remotely.
Spawned an agent
"###,
    );
}
