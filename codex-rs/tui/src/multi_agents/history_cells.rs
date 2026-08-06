//! Collab tool-call history-row rendering for multi-agent turns.

use crate::history_cell::PlainHistoryCell;
use crate::text_formatting::truncate_text;
use codex_app_server_protocol::AskParentMode;
use codex_app_server_protocol::CollabAgentState;
use codex_app_server_protocol::CollabAgentStatus;
use codex_app_server_protocol::CollabAgentTool;
use codex_app_server_protocol::CollabAgentToolCallStatus;
use codex_app_server_protocol::SpawnContextInheritance;
use codex_app_server_protocol::ThreadItem;
use codex_protocol::ThreadId;
use codex_protocol::items::ASK_PARENT_REQUIRES_AUTHORITATIVE_MESSAGE;
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::text::Span;
use std::collections::HashMap;

use super::AgentMetadata;
use super::SpawnRequestSummary;
use super::render::COLLAB_PROMPT_PREVIEW_GRAPHEMES;
use super::render::agent_label;
use super::render::agent_label_line;
use super::render::agent_label_spans;
use super::render::collab_event;
use super::render::context_inheritance_detail;
use super::render::first_agent_state;
use super::render::is_fast_service_tier;
use super::render::parse_thread_id;
use super::render::status_summary_line;
use super::render::title_spans_line;
use super::render::title_text;
use super::render::title_with_agent;
use super::render::wait_complete_lines;

pub(crate) fn tool_call_history_cell(
    item: &ThreadItem,
    agent_metadata: impl FnMut(ThreadId) -> AgentMetadata,
) -> Option<PlainHistoryCell> {
    tool_call_history_cell_with_spawn_request(
        item,
        /*cached_spawn_request*/ None,
        agent_metadata,
    )
}

pub(crate) fn spawn_request_summary(item: &ThreadItem) -> Option<SpawnRequestSummary> {
    match item {
        ThreadItem::CollabAgentToolCall {
            tool: CollabAgentTool::SpawnAgent,
            model: Some(model),
            reasoning_effort: Some(reasoning_effort),
            context_inheritance,
            ..
        } => Some(SpawnRequestSummary {
            model: model.clone(),
            reasoning_effort: reasoning_effort.clone(),
            context_inheritance: context_inheritance.clone(),
        }),
        _ => None,
    }
}

pub(crate) fn tool_call_history_cell_with_spawn_request(
    item: &ThreadItem,
    cached_spawn_request: Option<&SpawnRequestSummary>,
    mut agent_metadata: impl FnMut(ThreadId) -> AgentMetadata,
) -> Option<PlainHistoryCell> {
    let ThreadItem::CollabAgentToolCall {
        tool,
        status,
        receiver_thread_ids,
        prompt,
        service_tier,
        context_inheritance,
        mode,
        agents_states,
        ..
    } = item
    else {
        return None;
    };

    let first_receiver = receiver_thread_ids
        .first()
        .and_then(|id| parse_thread_id(id));

    match tool {
        CollabAgentTool::SpawnAgent => {
            if matches!(status, CollabAgentToolCallStatus::InProgress) {
                return None;
            }
            let fallback_spawn_request = spawn_request_summary(item);
            Some(spawn_end(
                first_receiver,
                prompt.as_deref().unwrap_or_default(),
                cached_spawn_request.or(fallback_spawn_request.as_ref()),
                service_tier.as_deref(),
                context_inheritance.as_ref(),
                &mut agent_metadata,
            ))
        }
        CollabAgentTool::SendInput => {
            if matches!(status, CollabAgentToolCallStatus::InProgress) {
                return None;
            }
            first_receiver
                .map(|receiver_thread_id| interaction_end(receiver_thread_id, &mut agent_metadata))
        }
        CollabAgentTool::AskParent => Some(parent_decision(status, mode.as_ref(), agents_states)),
        CollabAgentTool::ResumeAgent => first_receiver.map(|receiver_thread_id| {
            if matches!(status, CollabAgentToolCallStatus::InProgress) {
                resume_begin(receiver_thread_id, &mut agent_metadata)
            } else {
                let state = first_agent_state(receiver_thread_ids, agents_states);
                resume_end(receiver_thread_id, state, &mut agent_metadata)
            }
        }),
        CollabAgentTool::Wait => {
            if matches!(status, CollabAgentToolCallStatus::InProgress) {
                Some(waiting_begin(receiver_thread_ids, &mut agent_metadata))
            } else {
                Some(waiting_end(
                    receiver_thread_ids,
                    agents_states,
                    &mut agent_metadata,
                ))
            }
        }
        CollabAgentTool::CloseAgent => {
            if matches!(status, CollabAgentToolCallStatus::InProgress) {
                return None;
            }
            first_receiver
                .map(|receiver_thread_id| close_end(receiver_thread_id, &mut agent_metadata))
        }
    }
}

pub(crate) fn collab_tool_summary(item: &ThreadItem) -> Option<String> {
    let ThreadItem::CollabAgentToolCall {
        tool,
        status,
        context_inheritance,
        mode,
        agents_states,
        ..
    } = item
    else {
        return None;
    };

    let mut summary = match tool {
        CollabAgentTool::SpawnAgent => match status {
            CollabAgentToolCallStatus::InProgress => "Spawning an agent".to_string(),
            CollabAgentToolCallStatus::Completed => "Spawned an agent".to_string(),
            CollabAgentToolCallStatus::Failed => "Agent spawn failed".to_string(),
        },
        CollabAgentTool::SendInput => match status {
            CollabAgentToolCallStatus::InProgress => "Sending input to an agent".to_string(),
            CollabAgentToolCallStatus::Completed => "Sent input to an agent".to_string(),
            CollabAgentToolCallStatus::Failed => "Failed to send input to an agent".to_string(),
        },
        CollabAgentTool::AskParent => parent_decision_summary(status, mode.as_ref(), agents_states),
        CollabAgentTool::ResumeAgent => match status {
            CollabAgentToolCallStatus::InProgress => "Resuming an agent".to_string(),
            CollabAgentToolCallStatus::Completed => "Resumed an agent".to_string(),
            CollabAgentToolCallStatus::Failed => "Failed to resume an agent".to_string(),
        },
        CollabAgentTool::Wait => match status {
            CollabAgentToolCallStatus::InProgress => "Waiting for an agent".to_string(),
            CollabAgentToolCallStatus::Completed => "Finished waiting for an agent".to_string(),
            CollabAgentToolCallStatus::Failed => "Failed while waiting for an agent".to_string(),
        },
        CollabAgentTool::CloseAgent => match status {
            CollabAgentToolCallStatus::InProgress => "Closing an agent".to_string(),
            CollabAgentToolCallStatus::Completed => "Closed an agent".to_string(),
            CollabAgentToolCallStatus::Failed => "Failed to close an agent".to_string(),
        },
    };
    if matches!(
        (tool, status),
        (
            CollabAgentTool::SpawnAgent,
            CollabAgentToolCallStatus::Completed
        )
    ) && let Some(context_detail) = context_inheritance_detail(context_inheritance.as_ref())
    {
        summary.push_str(&context_detail);
    }
    Some(summary)
}

fn spawn_end(
    new_thread_id: Option<ThreadId>,
    prompt: &str,
    spawn_request: Option<&SpawnRequestSummary>,
    service_tier: Option<&str>,
    context_inheritance: Option<&SpawnContextInheritance>,
    agent_metadata: &mut impl FnMut(ThreadId) -> AgentMetadata,
) -> PlainHistoryCell {
    let title = match new_thread_id {
        Some(thread_id) => {
            let mut spans = vec![Span::from("Spawned ").bold()];
            spans.extend(agent_label_spans(agent_label(
                thread_id,
                &agent_metadata(thread_id),
            )));
            let mut detail_parts = spawn_request.map_or_else(Vec::new, |spawn_request| {
                let model = spawn_request.model.trim();
                if model.is_empty() {
                    vec![spawn_request.reasoning_effort.to_string()]
                } else {
                    vec![
                        model.to_string(),
                        spawn_request.reasoning_effort.to_string(),
                    ]
                }
            });
            if is_fast_service_tier(service_tier) {
                detail_parts.push("fast".to_string());
            }
            if !detail_parts.is_empty() {
                let details = format!("({})", detail_parts.join(" "));
                spans.push(Span::from(" ").dim());
                spans.push(Span::from(details).magenta());
            }
            if let Some(context_detail) = context_inheritance_detail(context_inheritance) {
                spans.push(Span::from(context_detail).dim());
            }
            title_spans_line(spans)
        }
        None => title_text("Agent spawn failed"),
    };

    let prompt = prompt.trim();
    let details = if prompt.is_empty() {
        Vec::new()
    } else {
        vec![Line::from(Span::from(truncate_text(
            prompt,
            COLLAB_PROMPT_PREVIEW_GRAPHEMES,
        )))]
    };
    collab_event(title, details)
}

fn interaction_end(
    receiver_thread_id: ThreadId,
    agent_metadata: &mut impl FnMut(ThreadId) -> AgentMetadata,
) -> PlainHistoryCell {
    let title = title_with_agent(
        "Sent input to",
        agent_label(receiver_thread_id, &agent_metadata(receiver_thread_id)),
    );

    collab_event(title, Vec::new())
}

fn parent_decision(
    status: &CollabAgentToolCallStatus,
    mode: Option<&AskParentMode>,
    agents_states: &HashMap<String, CollabAgentState>,
) -> PlainHistoryCell {
    if matches!(mode, Some(AskParentMode::Consult)) {
        return parent_consultation(status, agents_states);
    }

    let title = parent_decision_title(status, mode, agents_states);
    collab_event(title_text(title), Vec::new())
}

fn parent_decision_title(
    status: &CollabAgentToolCallStatus,
    mode: Option<&AskParentMode>,
    agents_states: &HashMap<String, CollabAgentState>,
) -> &'static str {
    if matches!(mode, Some(AskParentMode::Consult)) {
        return parent_consultation_title(status, agents_states);
    }

    let parent_status = agents_states.values().next().map(|state| &state.status);
    match (status, parent_status) {
        (CollabAgentToolCallStatus::InProgress, _) => "Waiting for parent decision",
        (_, Some(CollabAgentStatus::Completed)) => "Received parent decision",
        (_, Some(CollabAgentStatus::Interrupted)) => "Parent decision timed out",
        (_, Some(CollabAgentStatus::NotFound | CollabAgentStatus::Shutdown)) => {
            "Parent decision unavailable"
        }
        (CollabAgentToolCallStatus::Completed, _) => "Received parent decision",
        (CollabAgentToolCallStatus::Failed, _) => "Parent decision unavailable",
    }
}

fn parent_decision_summary(
    status: &CollabAgentToolCallStatus,
    mode: Option<&AskParentMode>,
    agents_states: &HashMap<String, CollabAgentState>,
) -> String {
    let title = parent_decision_title(status, mode, agents_states);
    if matches!(mode, Some(AskParentMode::Consult))
        && matches!(title, "Advisory from parent context snapshot")
    {
        return format!("{title} (may be stale; not authoritative)");
    }
    if matches!(mode, Some(AskParentMode::Consult))
        && matches!(title, "Parent context requires an authoritative decision")
    {
        return format!("{title}; use ask_parent with mode: authoritative");
    }
    title.to_string()
}

fn parent_consultation(
    status: &CollabAgentToolCallStatus,
    agents_states: &HashMap<String, CollabAgentState>,
) -> PlainHistoryCell {
    let title = parent_consultation_title(status, agents_states);
    if matches!(status, CollabAgentToolCallStatus::InProgress) {
        let details = vec!["Consulting a snapshot of the parent context".dim().into()];
        return collab_event(title_text(title), details);
    }

    if parent_requires_authoritative_decision(agents_states) {
        let details = vec![
            "Use ask_parent with mode: authoritative for a parent decision"
                .yellow()
                .into(),
        ];
        return collab_event(title_text(title), details);
    }

    if matches!(title, "Advisory from parent context snapshot") {
        let details = vec![
            "May be stale; this is not an authoritative parent decision"
                .yellow()
                .into(),
        ];
        return collab_event(title_text(title), details);
    }

    collab_event(title_text(title), Vec::new())
}

fn parent_consultation_title(
    status: &CollabAgentToolCallStatus,
    agents_states: &HashMap<String, CollabAgentState>,
) -> &'static str {
    if matches!(status, CollabAgentToolCallStatus::InProgress) {
        return "Consulting parent context snapshot";
    }

    if parent_requires_authoritative_decision(agents_states) {
        return "Parent context requires an authoritative decision";
    }

    let parent_status = agents_states.values().next().map(|state| &state.status);
    match (status, parent_status) {
        (_, Some(CollabAgentStatus::Interrupted)) => "Parent context consultation timed out",
        (_, Some(CollabAgentStatus::NotFound | CollabAgentStatus::Shutdown)) => {
            "Parent context consultation unavailable"
        }
        (CollabAgentToolCallStatus::Failed, _) => "Parent context consultation unavailable",
        _ => "Advisory from parent context snapshot",
    }
}

fn parent_requires_authoritative_decision(
    agents_states: &HashMap<String, CollabAgentState>,
) -> bool {
    agents_states.values().any(|state| {
        matches!(state.status, CollabAgentStatus::Completed)
            && state.message.as_deref() == Some(ASK_PARENT_REQUIRES_AUTHORITATIVE_MESSAGE)
    })
}

fn waiting_begin(
    receiver_thread_ids: &[String],
    agent_metadata: &mut impl FnMut(ThreadId) -> AgentMetadata,
) -> PlainHistoryCell {
    let receiver_agents = receiver_thread_ids
        .iter()
        .filter_map(|thread_id| parse_thread_id(thread_id))
        .map(|thread_id| (thread_id, agent_metadata(thread_id)))
        .collect::<Vec<_>>();

    let title = match receiver_agents.as_slice() {
        [(thread_id, metadata)] => {
            title_with_agent("Waiting for", agent_label(*thread_id, metadata))
        }
        [] => title_text("Waiting for agents"),
        _ => title_text(format!("Waiting for {} agents", receiver_agents.len())),
    };

    let details = if receiver_agents.len() > 1 {
        receiver_agents
            .iter()
            .map(|(thread_id, metadata)| agent_label_line(agent_label(*thread_id, metadata)))
            .collect()
    } else {
        Vec::new()
    };

    collab_event(title, details)
}

fn waiting_end(
    receiver_thread_ids: &[String],
    agents_states: &std::collections::HashMap<String, CollabAgentState>,
    agent_metadata: &mut impl FnMut(ThreadId) -> AgentMetadata,
) -> PlainHistoryCell {
    let details = wait_complete_lines(receiver_thread_ids, agents_states, agent_metadata);
    collab_event(title_text("Finished waiting"), details)
}

fn close_end(
    receiver_thread_id: ThreadId,
    agent_metadata: &mut impl FnMut(ThreadId) -> AgentMetadata,
) -> PlainHistoryCell {
    collab_event(
        title_with_agent(
            "Closed",
            agent_label(receiver_thread_id, &agent_metadata(receiver_thread_id)),
        ),
        Vec::new(),
    )
}

fn resume_begin(
    receiver_thread_id: ThreadId,
    agent_metadata: &mut impl FnMut(ThreadId) -> AgentMetadata,
) -> PlainHistoryCell {
    collab_event(
        title_with_agent(
            "Resuming",
            agent_label(receiver_thread_id, &agent_metadata(receiver_thread_id)),
        ),
        Vec::new(),
    )
}

fn resume_end(
    receiver_thread_id: ThreadId,
    status: Option<&CollabAgentState>,
    agent_metadata: &mut impl FnMut(ThreadId) -> AgentMetadata,
) -> PlainHistoryCell {
    collab_event(
        title_with_agent(
            "Resumed",
            agent_label(receiver_thread_id, &agent_metadata(receiver_thread_id)),
        ),
        vec![status_summary_line(status)],
    )
}
