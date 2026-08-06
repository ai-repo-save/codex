use super::*;
use crate::history_cell::HistoryCell;
use codex_app_server_protocol::AskParentMode;
use codex_app_server_protocol::CollabAgentState;
use codex_app_server_protocol::CollabAgentStatus;
use codex_app_server_protocol::CollabAgentTool;
use codex_app_server_protocol::CollabAgentToolCallStatus;
use codex_app_server_protocol::SubAgentActivityKind;
use codex_app_server_protocol::SubAgentActivityOperation;
use codex_app_server_protocol::SubAgentActivityOutcome;
use codex_app_server_protocol::ThreadItem;
use codex_protocol::config_types::ServiceTier;
use codex_protocol::items::ASK_PARENT_REQUIRES_AUTHORITATIVE_MESSAGE;
#[cfg(target_os = "macos")]
use crossterm::event::KeyEvent;
#[cfg(target_os = "macos")]
use crossterm::event::KeyModifiers;
use insta::assert_snapshot;
use pretty_assertions::assert_eq;
use ratatui::style::Color;
use ratatui::style::Modifier;
use std::collections::HashMap;

#[test]
fn interacted_sub_agent_activity_does_not_change_liveness() {
    let item = ThreadItem::SubAgentActivity {
        id: "activity-1".to_string(),
        kind: SubAgentActivityKind::Interacted,
        agent_thread_id: ThreadId::new().to_string(),
        agent_path: "/root/child".to_string(),
        operation: Some(SubAgentActivityOperation::SendMessage),
        outcome: Some(SubAgentActivityOutcome::Succeeded),
        model: None,
        reasoning_effort: None,
        service_tier: None,
        context_inheritance: None,
    };

    assert_eq!(
        sub_agent_activity_display(&item).and_then(|display| display.running_update),
        None
    );
}

#[test]
fn collab_events_snapshot() {
    let sender_thread_id = ThreadId::from_string("00000000-0000-0000-0000-000000000001")
        .expect("valid sender thread id");
    let robie_id = ThreadId::from_string("00000000-0000-0000-0000-000000000002")
        .expect("valid robie thread id");
    let bob_id = ThreadId::from_string("00000000-0000-0000-0000-000000000003")
        .expect("valid bob thread id");

    let spawn = tool_call_history_cell(
        &ThreadItem::CollabAgentToolCall {
            id: "call-spawn".to_string(),
            tool: CollabAgentTool::SpawnAgent,
            status: CollabAgentToolCallStatus::Completed,
            sender_thread_id: sender_thread_id.to_string(),
            receiver_thread_ids: vec![robie_id.to_string()],
            prompt: Some("Compute 11! and reply with just the integer result.".to_string()),
            model: None,
            reasoning_effort: None,
            service_tier: Some(ServiceTier::Fast.request_value().to_string()),
            context_inheritance: Some(SpawnContextInheritance::Full),
            mode: None,
            snapshot_revision: None,
            agents_states: HashMap::from([(
                robie_id.to_string(),
                agent_state(CollabAgentStatus::PendingInit, /*message*/ None),
            )]),
        },
        |thread_id| metadata_for(thread_id, robie_id, bob_id),
    )
    .expect("spawn item renders");

    let send = tool_call_history_cell(
        &ThreadItem::CollabAgentToolCall {
            id: "call-send".to_string(),
            tool: CollabAgentTool::SendInput,
            status: CollabAgentToolCallStatus::Completed,
            sender_thread_id: sender_thread_id.to_string(),
            receiver_thread_ids: vec![robie_id.to_string()],
            prompt: Some("Please continue and return the answer only.".to_string()),
            model: None,
            reasoning_effort: None,
            service_tier: None,
            context_inheritance: None,
            mode: None,
            snapshot_revision: None,
            agents_states: HashMap::from([(
                robie_id.to_string(),
                agent_state(CollabAgentStatus::Running, /*message*/ None),
            )]),
        },
        |thread_id| metadata_for(thread_id, robie_id, bob_id),
    )
    .expect("send-input item renders");

    let ask_parent = tool_call_history_cell(
        &ThreadItem::CollabAgentToolCall {
            id: "call-ask-parent".to_string(),
            tool: CollabAgentTool::AskParent,
            status: CollabAgentToolCallStatus::InProgress,
            sender_thread_id: robie_id.to_string(),
            receiver_thread_ids: vec![sender_thread_id.to_string()],
            prompt: Some("Should I preserve the existing wire format?".to_string()),
            model: None,
            reasoning_effort: None,
            service_tier: None,
            context_inheritance: None,
            mode: Some(AskParentMode::Authoritative),
            snapshot_revision: None,
            agents_states: HashMap::new(),
        },
        |thread_id| metadata_for(thread_id, robie_id, bob_id),
    )
    .expect("ask-parent item renders");

    let ask_parent_answered = tool_call_history_cell(
        &ThreadItem::CollabAgentToolCall {
            id: "call-ask-parent-answered".to_string(),
            tool: CollabAgentTool::AskParent,
            status: CollabAgentToolCallStatus::Completed,
            sender_thread_id: robie_id.to_string(),
            receiver_thread_ids: vec![sender_thread_id.to_string()],
            prompt: Some("Should I preserve the existing wire format?".to_string()),
            model: None,
            reasoning_effort: None,
            service_tier: None,
            context_inheritance: None,
            mode: Some(AskParentMode::Authoritative),
            snapshot_revision: None,
            agents_states: HashMap::from([(
                sender_thread_id.to_string(),
                agent_state(CollabAgentStatus::Completed, Some("Yes.")),
            )]),
        },
        |thread_id| metadata_for(thread_id, robie_id, bob_id),
    )
    .expect("answered ask-parent item renders");

    let ask_parent_timed_out = tool_call_history_cell(
        &ThreadItem::CollabAgentToolCall {
            id: "call-ask-parent-timed-out".to_string(),
            tool: CollabAgentTool::AskParent,
            status: CollabAgentToolCallStatus::Completed,
            sender_thread_id: robie_id.to_string(),
            receiver_thread_ids: vec![sender_thread_id.to_string()],
            prompt: Some("Choose the compatibility policy.".to_string()),
            model: None,
            reasoning_effort: None,
            service_tier: None,
            context_inheritance: None,
            mode: Some(AskParentMode::Authoritative),
            snapshot_revision: None,
            agents_states: HashMap::from([(
                sender_thread_id.to_string(),
                agent_state(CollabAgentStatus::Interrupted, /*message*/ None),
            )]),
        },
        |thread_id| metadata_for(thread_id, robie_id, bob_id),
    )
    .expect("timed-out ask-parent item renders");

    let ask_parent_unavailable = tool_call_history_cell(
        &ThreadItem::CollabAgentToolCall {
            id: "call-ask-parent-unavailable".to_string(),
            tool: CollabAgentTool::AskParent,
            status: CollabAgentToolCallStatus::Completed,
            sender_thread_id: robie_id.to_string(),
            receiver_thread_ids: vec![sender_thread_id.to_string()],
            prompt: Some("Resolve the ownership conflict.".to_string()),
            model: None,
            reasoning_effort: None,
            service_tier: None,
            context_inheritance: None,
            mode: Some(AskParentMode::Authoritative),
            snapshot_revision: None,
            agents_states: HashMap::from([(
                sender_thread_id.to_string(),
                agent_state(CollabAgentStatus::NotFound, /*message*/ None),
            )]),
        },
        |thread_id| metadata_for(thread_id, robie_id, bob_id),
    )
    .expect("unavailable ask-parent item renders");

    let consult_parent = tool_call_history_cell(
        &ThreadItem::CollabAgentToolCall {
            id: "call-consult-parent".to_string(),
            tool: CollabAgentTool::AskParent,
            status: CollabAgentToolCallStatus::InProgress,
            sender_thread_id: robie_id.to_string(),
            receiver_thread_ids: vec![sender_thread_id.to_string()],
            prompt: Some("What constraints did the parent already identify?".to_string()),
            model: None,
            reasoning_effort: None,
            service_tier: None,
            context_inheritance: None,
            mode: Some(AskParentMode::Consult),
            snapshot_revision: Some("history-18/items-42".to_string()),
            agents_states: HashMap::new(),
        },
        |thread_id| metadata_for(thread_id, robie_id, bob_id),
    )
    .expect("consult-parent item renders");

    let consult_parent_answered = tool_call_history_cell(
        &ThreadItem::CollabAgentToolCall {
            id: "call-consult-parent-answered".to_string(),
            tool: CollabAgentTool::AskParent,
            status: CollabAgentToolCallStatus::Completed,
            sender_thread_id: robie_id.to_string(),
            receiver_thread_ids: vec![sender_thread_id.to_string()],
            prompt: Some("What constraints did the parent already identify?".to_string()),
            model: None,
            reasoning_effort: None,
            service_tier: None,
            context_inheritance: None,
            mode: Some(AskParentMode::Consult),
            snapshot_revision: Some("history-18/items-42".to_string()),
            agents_states: HashMap::from([(
                sender_thread_id.to_string(),
                agent_state(
                    CollabAgentStatus::Completed,
                    Some("The parent favors reuse."),
                ),
            )]),
        },
        |thread_id| metadata_for(thread_id, robie_id, bob_id),
    )
    .expect("answered consult-parent item renders");

    let consult_parent_requires_authoritative = tool_call_history_cell(
        &ThreadItem::CollabAgentToolCall {
            id: "call-consult-parent-requires-authoritative".to_string(),
            tool: CollabAgentTool::AskParent,
            status: CollabAgentToolCallStatus::Completed,
            sender_thread_id: robie_id.to_string(),
            receiver_thread_ids: vec![sender_thread_id.to_string()],
            prompt: Some("May I commit the compatibility change?".to_string()),
            model: None,
            reasoning_effort: None,
            service_tier: None,
            context_inheritance: None,
            mode: Some(AskParentMode::Consult),
            snapshot_revision: Some("history-18/items-42".to_string()),
            agents_states: HashMap::from([(
                sender_thread_id.to_string(),
                agent_state(
                    CollabAgentStatus::Completed,
                    Some(ASK_PARENT_REQUIRES_AUTHORITATIVE_MESSAGE),
                ),
            )]),
        },
        |thread_id| metadata_for(thread_id, robie_id, bob_id),
    )
    .expect("consult-parent requires-authoritative item renders");

    let waiting = tool_call_history_cell(
        &ThreadItem::CollabAgentToolCall {
            id: "call-wait".to_string(),
            tool: CollabAgentTool::Wait,
            status: CollabAgentToolCallStatus::InProgress,
            sender_thread_id: sender_thread_id.to_string(),
            receiver_thread_ids: vec![robie_id.to_string()],
            prompt: None,
            model: None,
            reasoning_effort: None,
            service_tier: None,
            context_inheritance: None,
            mode: None,
            snapshot_revision: None,
            agents_states: HashMap::new(),
        },
        |thread_id| metadata_for(thread_id, robie_id, bob_id),
    )
    .expect("wait begin item renders");

    let finished = tool_call_history_cell(
        &ThreadItem::CollabAgentToolCall {
            id: "call-wait".to_string(),
            tool: CollabAgentTool::Wait,
            status: CollabAgentToolCallStatus::Completed,
            sender_thread_id: sender_thread_id.to_string(),
            receiver_thread_ids: vec![robie_id.to_string(), bob_id.to_string()],
            prompt: None,
            model: None,
            reasoning_effort: None,
            service_tier: None,
            context_inheritance: None,
            mode: None,
            snapshot_revision: None,
            agents_states: HashMap::from([
                (
                    robie_id.to_string(),
                    agent_state(CollabAgentStatus::Completed, Some("39916800")),
                ),
                (
                    bob_id.to_string(),
                    agent_state(CollabAgentStatus::Errored, Some("tool timeout")),
                ),
            ]),
        },
        |thread_id| metadata_for(thread_id, robie_id, bob_id),
    )
    .expect("wait end item renders");

    let close = tool_call_history_cell(
        &ThreadItem::CollabAgentToolCall {
            id: "call-close".to_string(),
            tool: CollabAgentTool::CloseAgent,
            status: CollabAgentToolCallStatus::Completed,
            sender_thread_id: sender_thread_id.to_string(),
            receiver_thread_ids: vec![robie_id.to_string()],
            prompt: None,
            model: None,
            reasoning_effort: None,
            service_tier: None,
            context_inheritance: None,
            mode: None,
            snapshot_revision: None,
            agents_states: HashMap::from([(
                robie_id.to_string(),
                agent_state(CollabAgentStatus::Completed, Some("39916800")),
            )]),
        },
        |thread_id| metadata_for(thread_id, robie_id, bob_id),
    )
    .expect("close item renders");

    let snapshot = [
        spawn,
        send,
        ask_parent,
        ask_parent_answered,
        ask_parent_timed_out,
        ask_parent_unavailable,
        consult_parent,
        consult_parent_answered,
        consult_parent_requires_authoritative,
        waiting,
        finished,
        close,
    ]
    .iter()
    .map(cell_to_text)
    .collect::<Vec<_>>()
    .join("\n\n");
    assert_snapshot!("collab_agent_transcript", snapshot);
}

#[test]
fn sub_agent_activity_snapshot() {
    let activities = [
        sub_agent_activity_item(
            "activity-started",
            SubAgentActivityKind::Started,
            /*operation*/ None,
            /*outcome*/ None,
        ),
        sub_agent_activity_item(
            "activity-interacted",
            SubAgentActivityKind::Interacted,
            /*operation*/ None,
            /*outcome*/ None,
        ),
        sub_agent_activity_item(
            "activity-interrupted",
            SubAgentActivityKind::Interrupted,
            /*operation*/ None,
            /*outcome*/ None,
        ),
        sub_agent_activity_item(
            "activity-send-message",
            SubAgentActivityKind::Interacted,
            Some(SubAgentActivityOperation::SendMessage),
            Some(SubAgentActivityOutcome::Succeeded),
        ),
        sub_agent_activity_item(
            "activity-send-message-failed",
            SubAgentActivityKind::Interacted,
            Some(SubAgentActivityOperation::SendMessage),
            Some(SubAgentActivityOutcome::Failed),
        ),
        sub_agent_activity_item(
            "activity-followup",
            SubAgentActivityKind::Interacted,
            Some(SubAgentActivityOperation::FollowupTask),
            Some(SubAgentActivityOutcome::Succeeded),
        ),
        sub_agent_activity_item(
            "activity-followup-failed",
            SubAgentActivityKind::Interacted,
            Some(SubAgentActivityOperation::FollowupTask),
            Some(SubAgentActivityOutcome::Failed),
        ),
        sub_agent_activity_item(
            "activity-reply",
            SubAgentActivityKind::Interacted,
            Some(SubAgentActivityOperation::ParentReply),
            Some(SubAgentActivityOutcome::Succeeded),
        ),
        sub_agent_activity_item(
            "activity-reply-failed",
            SubAgentActivityKind::Interacted,
            Some(SubAgentActivityOperation::ParentReply),
            Some(SubAgentActivityOutcome::Failed),
        ),
        sub_agent_activity_item(
            "activity-inspect",
            SubAgentActivityKind::Interacted,
            Some(SubAgentActivityOperation::InspectAgent),
            Some(SubAgentActivityOutcome::Succeeded),
        ),
        sub_agent_activity_item(
            "activity-inspect-failed",
            SubAgentActivityKind::Interacted,
            Some(SubAgentActivityOperation::InspectAgent),
            Some(SubAgentActivityOutcome::Failed),
        ),
    ];

    let history = activities
        .iter()
        .filter_map(sub_agent_activity_history_cell)
        .map(|cell| cell_to_text(&cell))
        .collect::<Vec<_>>()
        .join("\n");
    let transcript = activities
        .iter()
        .map(|item| match item {
            ThreadItem::SubAgentActivity {
                kind,
                operation,
                outcome,
                agent_path,
                model,
                reasoning_effort,
                service_tier,
                context_inheritance,
                ..
            } => sub_agent_activity_summary(
                *kind,
                *operation,
                *outcome,
                agent_path,
                model.as_deref(),
                reasoning_effort.as_ref(),
                service_tier.as_deref(),
                context_inheritance.as_ref(),
            ),
            _ => unreachable!("activity item"),
        })
        .collect::<Vec<_>>()
        .join("\n");
    let snapshot = format!("History:\n{history}\n\nTranscript:\n{transcript}");
    assert_snapshot!("sub_agent_activity", snapshot);
}

#[test]
fn sub_agent_activity_running_updates_distinguish_liveness_from_informational_activity() {
    let cases = [
        (
            SubAgentActivityKind::Started,
            None,
            None,
            /*expected*/ Some(true),
        ),
        (
            SubAgentActivityKind::Started,
            None,
            Some(SubAgentActivityOutcome::Succeeded),
            /*expected*/ Some(true),
        ),
        (
            SubAgentActivityKind::Interacted,
            None,
            None,
            /*expected*/ Some(true),
        ),
        (
            SubAgentActivityKind::Interrupted,
            None,
            None,
            /*expected*/ Some(false),
        ),
        (
            SubAgentActivityKind::Interacted,
            Some(SubAgentActivityOperation::SendMessage),
            Some(SubAgentActivityOutcome::Succeeded),
            /*expected*/ None,
        ),
        (
            SubAgentActivityKind::Interacted,
            Some(SubAgentActivityOperation::FollowupTask),
            Some(SubAgentActivityOutcome::Succeeded),
            /*expected*/ Some(true),
        ),
        (
            SubAgentActivityKind::Interacted,
            Some(SubAgentActivityOperation::ParentReply),
            Some(SubAgentActivityOutcome::Succeeded),
            /*expected*/ Some(true),
        ),
        (
            SubAgentActivityKind::Interacted,
            Some(SubAgentActivityOperation::InspectAgent),
            Some(SubAgentActivityOutcome::Succeeded),
            /*expected*/ None,
        ),
        (
            SubAgentActivityKind::Interacted,
            Some(SubAgentActivityOperation::SendMessage),
            Some(SubAgentActivityOutcome::Failed),
            /*expected*/ None,
        ),
        (
            SubAgentActivityKind::Interacted,
            Some(SubAgentActivityOperation::FollowupTask),
            Some(SubAgentActivityOutcome::Failed),
            /*expected*/ None,
        ),
        (
            SubAgentActivityKind::Interacted,
            Some(SubAgentActivityOperation::ParentReply),
            Some(SubAgentActivityOutcome::Failed),
            /*expected*/ None,
        ),
        (
            SubAgentActivityKind::Interacted,
            Some(SubAgentActivityOperation::InspectAgent),
            Some(SubAgentActivityOutcome::Failed),
            /*expected*/ None,
        ),
    ];

    for (index, (kind, operation, outcome, expected)) in cases.into_iter().enumerate() {
        let item = sub_agent_activity_item(
            &format!("activity-running-{index}"),
            kind,
            operation,
            outcome,
        );
        let display = sub_agent_activity_display(&item).expect("activity display");
        assert_eq!(display.running_update, expected);
    }
}

#[cfg(target_os = "macos")]
#[test]
fn agent_shortcut_matches_option_arrow_word_motion_fallbacks_only_when_allowed() {
    assert!(previous_agent_shortcut_matches(
        KeyEvent::new(KeyCode::Left, KeyModifiers::ALT),
        /*allow_word_motion_fallback*/ false,
    ));
    assert!(next_agent_shortcut_matches(
        KeyEvent::new(KeyCode::Right, KeyModifiers::ALT),
        /*allow_word_motion_fallback*/ false,
    ));
    assert!(previous_agent_shortcut_matches(
        KeyEvent::new(KeyCode::Char('b'), KeyModifiers::ALT),
        /*allow_word_motion_fallback*/ true,
    ));
    assert!(next_agent_shortcut_matches(
        KeyEvent::new(KeyCode::Char('f'), KeyModifiers::ALT),
        /*allow_word_motion_fallback*/ true,
    ));
    assert!(!previous_agent_shortcut_matches(
        KeyEvent::new(KeyCode::Char('b'), KeyModifiers::ALT),
        /*allow_word_motion_fallback*/ false,
    ));
    assert!(!next_agent_shortcut_matches(
        KeyEvent::new(KeyCode::Char('f'), KeyModifiers::ALT),
        /*allow_word_motion_fallback*/ false,
    ));
}

#[cfg(not(target_os = "macos"))]
#[test]
fn agent_shortcut_matches_option_arrows_only() {
    assert!(previous_agent_shortcut_matches(
        KeyEvent::new(KeyCode::Left, crossterm::event::KeyModifiers::ALT,),
        /*allow_word_motion_fallback*/ false
    ));
    assert!(next_agent_shortcut_matches(
        KeyEvent::new(KeyCode::Right, crossterm::event::KeyModifiers::ALT,),
        /*allow_word_motion_fallback*/ false
    ));
    assert!(!previous_agent_shortcut_matches(
        KeyEvent::new(KeyCode::Char('b'), crossterm::event::KeyModifiers::ALT,),
        /*allow_word_motion_fallback*/ false
    ));
    assert!(!next_agent_shortcut_matches(
        KeyEvent::new(KeyCode::Char('f'), crossterm::event::KeyModifiers::ALT,),
        /*allow_word_motion_fallback*/ false
    ));
}

#[test]
fn title_styles_nickname_and_role() {
    let sender_thread_id = ThreadId::from_string("00000000-0000-0000-0000-000000000001")
        .expect("valid sender thread id");
    let robie_id = ThreadId::from_string("00000000-0000-0000-0000-000000000002")
        .expect("valid robie thread id");
    let cell = tool_call_history_cell(
        &ThreadItem::CollabAgentToolCall {
            id: "call-spawn".to_string(),
            tool: CollabAgentTool::SpawnAgent,
            status: CollabAgentToolCallStatus::Completed,
            sender_thread_id: sender_thread_id.to_string(),
            receiver_thread_ids: vec![robie_id.to_string()],
            prompt: Some(String::new()),
            model: None,
            reasoning_effort: None,
            service_tier: None,
            context_inheritance: None,
            mode: None,
            snapshot_revision: None,
            agents_states: HashMap::from([(
                robie_id.to_string(),
                agent_state(CollabAgentStatus::PendingInit, /*message*/ None),
            )]),
        },
        |thread_id| metadata_for(thread_id, robie_id, ThreadId::new()),
    )
    .expect("spawn item renders");

    let lines = cell.display_lines(/*width*/ 200);
    let title = &lines[0];
    assert_eq!(title.spans[2].content.as_ref(), "Robie");
    assert_eq!(title.spans[2].style.fg, Some(Color::Cyan));
    assert!(title.spans[2].style.add_modifier.contains(Modifier::BOLD));
    assert_eq!(title.spans[4].content.as_ref(), "[explorer]");
    assert_eq!(title.spans[4].style.fg, None);
    assert!(!title.spans[4].style.add_modifier.contains(Modifier::DIM));
}

#[test]
fn collab_resume_interrupted_snapshot() {
    let sender_thread_id = ThreadId::from_string("00000000-0000-0000-0000-000000000001")
        .expect("valid sender thread id");
    let robie_id = ThreadId::from_string("00000000-0000-0000-0000-000000000002")
        .expect("valid robie thread id");

    let cell = tool_call_history_cell(
        &ThreadItem::CollabAgentToolCall {
            id: "call-resume".to_string(),
            tool: CollabAgentTool::ResumeAgent,
            status: CollabAgentToolCallStatus::Completed,
            sender_thread_id: sender_thread_id.to_string(),
            receiver_thread_ids: vec![robie_id.to_string()],
            prompt: None,
            model: None,
            reasoning_effort: None,
            service_tier: None,
            context_inheritance: None,
            mode: None,
            snapshot_revision: None,
            agents_states: HashMap::from([(
                robie_id.to_string(),
                agent_state(CollabAgentStatus::Interrupted, /*message*/ None),
            )]),
        },
        |thread_id| metadata_for(thread_id, robie_id, ThreadId::new()),
    )
    .expect("resume item renders");

    assert_snapshot!("collab_resume_interrupted", cell_to_text(&cell));
}

fn agent_state(status: CollabAgentStatus, message: Option<&str>) -> CollabAgentState {
    CollabAgentState {
        status,
        message: message.map(str::to_string),
    }
}

fn metadata_for(thread_id: ThreadId, robie_id: ThreadId, bob_id: ThreadId) -> AgentMetadata {
    if thread_id == robie_id {
        AgentMetadata {
            agent_nickname: Some("Robie".to_string()),
            agent_role: Some("explorer".to_string()),
        }
    } else if thread_id == bob_id {
        AgentMetadata {
            agent_nickname: Some("Bob".to_string()),
            agent_role: Some("worker".to_string()),
        }
    } else {
        AgentMetadata::default()
    }
}

fn sub_agent_activity_item(
    id: &str,
    kind: SubAgentActivityKind,
    operation: Option<SubAgentActivityOperation>,
    outcome: Option<SubAgentActivityOutcome>,
) -> ThreadItem {
    ThreadItem::SubAgentActivity {
        id: id.to_string(),
        kind,
        agent_thread_id: "00000000-0000-0000-0000-000000000002".to_string(),
        agent_path: "/root/task".to_string(),
        operation,
        outcome,
        model: Some("gpt-5.6".to_string()),
        reasoning_effort: Some(ReasoningEffort::High),
        service_tier: Some(ServiceTier::Fast.request_value().to_string()),
        context_inheritance: matches!(kind, SubAgentActivityKind::Started)
            .then_some(SpawnContextInheritance::LastNTurns { turns: 1 }),
    }
}

fn cell_to_text(cell: &PlainHistoryCell) -> String {
    cell.display_lines(/*width*/ 200)
        .iter()
        .map(line_to_text)
        .collect::<Vec<_>>()
        .join("\n")
}

fn line_to_text(line: &Line<'static>) -> String {
    line.spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect::<Vec<_>>()
        .join("")
}
