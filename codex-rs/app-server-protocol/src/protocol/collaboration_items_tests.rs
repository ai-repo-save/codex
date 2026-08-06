use crate::protocol::common::ServerNotification;
use crate::protocol::event_mapping::item_event_to_server_notification;
use crate::protocol::v2::CollabAgentState;
use crate::protocol::v2::CollabAgentTool;
use crate::protocol::v2::CollabAgentToolCallStatus;
use crate::protocol::v2::ItemCompletedNotification;
use crate::protocol::v2::ItemStartedNotification;
use crate::protocol::v2::SubAgentActivityKind;
use crate::protocol::v2::SubAgentActivityOperation;
use crate::protocol::v2::SubAgentActivityOutcome;
use crate::protocol::v2::ThreadItem;
use codex_protocol::AgentPath;
use codex_protocol::ThreadId;
use codex_protocol::protocol::CollabResumeBeginEvent;
use codex_protocol::protocol::CollabResumeEndEvent;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::SubAgentActivityEvent;
use codex_protocol::protocol::SubAgentActivityKind as CoreSubAgentActivityKind;
use codex_protocol::protocol::SubAgentActivityOperation as CoreSubAgentActivityOperation;
use codex_protocol::protocol::SubAgentActivityOutcome as CoreSubAgentActivityOutcome;
use pretty_assertions::assert_eq;
use std::collections::HashMap;

fn assert_item_started_server_notification(
    notification: ServerNotification,
    expected: ItemStartedNotification,
) {
    match notification {
        ServerNotification::ItemStarted(payload) => assert_eq!(payload, expected),
        other => panic!("expected item started notification, got {other:?}"),
    }
}

fn assert_item_completed_server_notification(
    notification: ServerNotification,
    expected: ItemCompletedNotification,
) {
    match notification {
        ServerNotification::ItemCompleted(payload) => assert_eq!(payload, expected),
        other => panic!("expected item completed notification, got {other:?}"),
    }
}

#[test]
fn collab_resume_begin_maps_to_item_started_resume_agent() {
    let event = CollabResumeBeginEvent {
        call_id: "call-1".to_string(),
        started_at_ms: 123,
        sender_thread_id: ThreadId::new(),
        receiver_thread_id: ThreadId::new(),
        receiver_agent_nickname: None,
        receiver_agent_role: None,
    };

    let notification = item_event_to_server_notification(
        EventMsg::CollabResumeBegin(event.clone()),
        "thread-1",
        "turn-1",
    );
    assert_item_started_server_notification(
        notification,
        ItemStartedNotification {
            thread_id: "thread-1".to_string(),
            turn_id: "turn-1".to_string(),
            started_at_ms: event.started_at_ms,
            item: ThreadItem::CollabAgentToolCall {
                id: event.call_id,
                tool: CollabAgentTool::ResumeAgent,
                status: CollabAgentToolCallStatus::InProgress,
                sender_thread_id: event.sender_thread_id.to_string(),
                receiver_thread_ids: vec![event.receiver_thread_id.to_string()],
                prompt: None,
                model: None,
                reasoning_effort: None,
                service_tier: None,
                context_inheritance: None,
                mode: None,
                snapshot_revision: None,
                agents_states: HashMap::new(),
            },
        },
    );
}

#[test]
fn collab_resume_end_maps_to_item_completed_resume_agent() {
    let event = CollabResumeEndEvent {
        call_id: "call-2".to_string(),
        completed_at_ms: 456,
        sender_thread_id: ThreadId::new(),
        receiver_thread_id: ThreadId::new(),
        receiver_agent_nickname: None,
        receiver_agent_role: None,
        status: codex_protocol::protocol::AgentStatus::NotFound,
    };

    let receiver_id = event.receiver_thread_id.to_string();
    let notification = item_event_to_server_notification(
        EventMsg::CollabResumeEnd(event.clone()),
        "thread-2",
        "turn-2",
    );
    assert_item_completed_server_notification(
        notification,
        ItemCompletedNotification {
            thread_id: "thread-2".to_string(),
            turn_id: "turn-2".to_string(),
            completed_at_ms: event.completed_at_ms,
            item: ThreadItem::CollabAgentToolCall {
                id: event.call_id,
                tool: CollabAgentTool::ResumeAgent,
                status: CollabAgentToolCallStatus::Failed,
                sender_thread_id: event.sender_thread_id.to_string(),
                receiver_thread_ids: vec![receiver_id.clone()],
                prompt: None,
                model: None,
                reasoning_effort: None,
                service_tier: None,
                context_inheritance: None,
                mode: None,
                snapshot_revision: None,
                agents_states: [(
                    receiver_id,
                    CollabAgentState::from(codex_protocol::protocol::AgentStatus::NotFound),
                )]
                .into_iter()
                .collect(),
            },
        },
    );
}

#[test]
fn sub_agent_activity_maps_model_to_completed_item() {
    let event = SubAgentActivityEvent {
        event_id: "activity-1".to_string(),
        occurred_at_ms: 456,
        agent_thread_id: ThreadId::new(),
        agent_path: AgentPath::try_from("/root/worker").expect("valid agent path"),
        kind: CoreSubAgentActivityKind::Started,
        operation: Some(CoreSubAgentActivityOperation::FollowupTask),
        outcome: Some(CoreSubAgentActivityOutcome::Succeeded),
        model: Some("gpt-5.4".to_string()),
        reasoning_effort: Some(codex_protocol::openai_models::ReasoningEffort::High),
        service_tier: Some("priority".to_string()),
        context_inheritance: None,
    };

    let notification = item_event_to_server_notification(
        EventMsg::SubAgentActivity(event.clone()),
        "thread-1",
        "turn-1",
    );
    assert_item_completed_server_notification(
        notification,
        ItemCompletedNotification {
            thread_id: "thread-1".to_string(),
            turn_id: "turn-1".to_string(),
            completed_at_ms: event.occurred_at_ms,
            item: ThreadItem::SubAgentActivity {
                id: event.event_id,
                kind: SubAgentActivityKind::Started,
                agent_thread_id: event.agent_thread_id.to_string(),
                agent_path: String::from(event.agent_path),
                operation: Some(SubAgentActivityOperation::FollowupTask),
                outcome: Some(SubAgentActivityOutcome::Succeeded),
                model: event.model,
                reasoning_effort: event.reasoning_effort,
                service_tier: event.service_tier,
                context_inheritance: event.context_inheritance.map(Into::into),
            },
        },
    );
}
