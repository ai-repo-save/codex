//! Shared multi-agent history-row rendering helpers.

use crate::history_cell::PlainHistoryCell;
use crate::render::line_utils::prefix_lines;
use crate::text_formatting::truncate_text;
use codex_app_server_protocol::CollabAgentState;
use codex_app_server_protocol::CollabAgentStatus;
use codex_app_server_protocol::SpawnContextInheritance;
use codex_protocol::ThreadId;
use codex_protocol::config_types::ServiceTier;
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::text::Span;
use std::collections::HashSet;

use super::AgentMetadata;

pub(super) const COLLAB_PROMPT_PREVIEW_GRAPHEMES: usize = 160;
pub(super) const COLLAB_AGENT_ERROR_PREVIEW_GRAPHEMES: usize = 160;
pub(super) const COLLAB_AGENT_RESPONSE_PREVIEW_GRAPHEMES: usize = 240;

#[derive(Clone, Copy)]
pub(super) struct AgentLabel<'a> {
    thread_id: Option<ThreadId>,
    nickname: Option<&'a str>,
    role: Option<&'a str>,
}

pub(super) fn context_inheritance_detail(
    context_inheritance: Option<&SpawnContextInheritance>,
) -> Option<String> {
    match context_inheritance? {
        SpawnContextInheritance::Full => Some(" · context: all".to_string()),
        SpawnContextInheritance::None => Some(" · context: none".to_string()),
        SpawnContextInheritance::LastNTurns { turns } => {
            let unit = if *turns == 1 { "turn" } else { "turns" };
            Some(format!(" · context: last {turns} {unit}"))
        }
    }
}

pub(super) fn is_fast_service_tier(service_tier: Option<&str>) -> bool {
    matches!(
        service_tier.and_then(ServiceTier::from_request_value),
        Some(ServiceTier::Fast)
    )
}

pub(super) fn collab_event(title: Line<'static>, details: Vec<Line<'static>>) -> PlainHistoryCell {
    let mut lines: Vec<Line<'static>> = vec![title];
    if !details.is_empty() {
        lines.extend(prefix_lines(details, "  └ ".dim(), "    ".into()));
    }
    PlainHistoryCell::new(lines)
}

pub(super) fn title_text(title: impl Into<String>) -> Line<'static> {
    title_spans_line(vec![Span::from(title.into()).bold()])
}

pub(super) fn title_with_agent(prefix: &str, agent: AgentLabel<'_>) -> Line<'static> {
    let mut spans = vec![Span::from(format!("{prefix} ")).bold()];
    spans.extend(agent_label_spans(agent));
    title_spans_line(spans)
}

pub(super) fn title_spans_line(mut spans: Vec<Span<'static>>) -> Line<'static> {
    let mut title = Vec::with_capacity(spans.len() + 1);
    title.push(Span::from("• ").dim());
    title.append(&mut spans);
    title.into()
}

pub(super) fn parse_thread_id(thread_id: &str) -> Option<ThreadId> {
    ThreadId::from_string(thread_id).ok()
}

pub(super) fn agent_label(thread_id: ThreadId, metadata: &AgentMetadata) -> AgentLabel<'_> {
    AgentLabel {
        thread_id: Some(thread_id),
        nickname: metadata.agent_nickname.as_deref(),
        role: metadata.agent_role.as_deref(),
    }
}

pub(super) fn agent_label_line(agent: AgentLabel<'_>) -> Line<'static> {
    agent_label_spans(agent).into()
}

pub(super) fn agent_label_spans(agent: AgentLabel<'_>) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    let nickname = agent
        .nickname
        .map(str::trim)
        .filter(|nickname| !nickname.is_empty());
    let role = agent.role.map(str::trim).filter(|role| !role.is_empty());

    if let Some(nickname) = nickname {
        spans.push(Span::from(nickname.to_string()).cyan().bold());
    } else if let Some(thread_id) = agent.thread_id {
        spans.push(Span::from(thread_id.to_string()).cyan());
    } else {
        spans.push(Span::from("agent").cyan());
    }

    if let Some(role) = role {
        spans.push(Span::from(" ").dim());
        spans.push(Span::from(format!("[{role}]")));
    }

    spans
}

pub(super) fn wait_complete_lines(
    receiver_thread_ids: &[String],
    agents_states: &std::collections::HashMap<String, CollabAgentState>,
    agent_metadata: &mut impl FnMut(ThreadId) -> AgentMetadata,
) -> Vec<Line<'static>> {
    let mut seen = HashSet::new();
    let mut entries = receiver_thread_ids
        .iter()
        .filter_map(|thread_id| {
            let parsed_thread_id = parse_thread_id(thread_id)?;
            let status = agents_states.get(thread_id)?;
            seen.insert(parsed_thread_id);
            Some((parsed_thread_id, agent_metadata(parsed_thread_id), status))
        })
        .collect::<Vec<_>>();

    let mut extras = agents_states
        .iter()
        .filter_map(|(thread_id, status)| {
            let parsed_thread_id = parse_thread_id(thread_id)?;
            (!seen.contains(&parsed_thread_id))
                .then(|| (parsed_thread_id, agent_metadata(parsed_thread_id), status))
        })
        .collect::<Vec<_>>();
    extras.sort_by_key(|entry| entry.0.to_string());
    entries.extend(extras);

    if entries.is_empty() {
        vec![Line::from(Span::from("No agents completed yet"))]
    } else {
        entries
            .into_iter()
            .map(|(thread_id, metadata, status)| {
                let mut spans = agent_label_spans(agent_label(thread_id, &metadata));
                spans.push(Span::from(": ").dim());
                spans.extend(status_summary_spans(status));
                spans.into()
            })
            .collect()
    }
}

pub(super) fn first_agent_state<'a>(
    receiver_thread_ids: &[String],
    agents_states: &'a std::collections::HashMap<String, CollabAgentState>,
) -> Option<&'a CollabAgentState> {
    receiver_thread_ids
        .iter()
        .find_map(|thread_id| agents_states.get(thread_id))
        .or_else(|| {
            agents_states
                .iter()
                .min_by(|left, right| left.0.cmp(right.0))
                .map(|(_, status)| status)
        })
}

pub(super) fn status_summary_line(status: Option<&CollabAgentState>) -> Line<'static> {
    match status {
        Some(status) => status_summary_spans(status).into(),
        None => error_summary_spans(/*error*/ None).into(),
    }
}

pub(super) fn status_summary_spans(status: &CollabAgentState) -> Vec<Span<'static>> {
    match status.status {
        CollabAgentStatus::PendingInit => vec![Span::from("Pending init").cyan()],
        CollabAgentStatus::Running => vec![Span::from("Running").cyan().bold()],
        // Allow `.yellow()`
        #[allow(clippy::disallowed_methods)]
        CollabAgentStatus::Interrupted => vec![Span::from("Interrupted").yellow()],
        CollabAgentStatus::Completed => {
            let mut spans = vec![Span::from("Completed").green()];
            if let Some(message) = status.message.as_ref() {
                let message_preview = truncate_text(
                    &message.split_whitespace().collect::<Vec<_>>().join(" "),
                    COLLAB_AGENT_RESPONSE_PREVIEW_GRAPHEMES,
                );
                if !message_preview.is_empty() {
                    spans.push(Span::from(" - ").dim());
                    spans.push(Span::from(message_preview));
                }
            }
            spans
        }
        CollabAgentStatus::Errored => error_summary_spans(status.message.as_deref()),
        CollabAgentStatus::Shutdown => vec![Span::from("Shutdown")],
        CollabAgentStatus::NotFound => vec![Span::from("Not found").red()],
    }
}

pub(super) fn error_summary_spans(error: Option<&str>) -> Vec<Span<'static>> {
    let mut spans = vec![Span::from("Error").red()];
    if let Some(error) = error {
        let error_preview = truncate_text(
            &error.split_whitespace().collect::<Vec<_>>().join(" "),
            COLLAB_AGENT_ERROR_PREVIEW_GRAPHEMES,
        );
        if !error_preview.is_empty() {
            spans.push(Span::from(" - ").dim());
            spans.push(Span::from(error_preview));
        }
    }
    spans
}
