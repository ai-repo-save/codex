//! Helpers for rendering and navigating multi-agent state in the TUI.
//!
//! This module owns the shared presentation contracts for multi-agent history rows, `/agent` picker
//! entries, and the fast-switch keyboard shortcuts. Higher-level coordination, such as deciding
//! which thread becomes active or when a thread closes, stays in [`crate::app::App`].

use crate::history_cell::PlainHistoryCell;
use crate::render::line_utils::prefix_lines;
use codex_app_server_protocol::AskParentMode;
use codex_app_server_protocol::CollabAgentState;
use codex_app_server_protocol::CollabAgentStatus;
use codex_app_server_protocol::CollabAgentTool;
use codex_app_server_protocol::CollabAgentToolCallStatus;
use codex_app_server_protocol::SubAgentActivityKind;
use codex_app_server_protocol::SubAgentActivityOperation;
use codex_app_server_protocol::SubAgentActivityOutcome;
use codex_app_server_protocol::ThreadItem;
use codex_protocol::ThreadId;
use codex_protocol::items::ASK_PARENT_REQUIRES_AUTHORITATIVE_MESSAGE;
use codex_protocol::openai_models::ReasoningEffort;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
#[cfg(target_os = "macos")]
use crossterm::event::KeyEventKind;
#[cfg(target_os = "macos")]
use crossterm::event::KeyModifiers;
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::text::Span;
use std::collections::HashMap;
use std::collections::HashSet;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AgentPickerThreadEntry {
    /// Human-friendly nickname shown in picker rows and footer labels.
    pub(crate) agent_nickname: Option<String>,
    /// Agent type shown in brackets when present, for example `worker`.
    pub(crate) agent_role: Option<String>,
    /// Canonical v2 agent path, when the thread was observed through v2 activity.
    pub(crate) agent_path: Option<String>,
    /// Whether the latest liveness refresh says the agent thread is actively working.
    pub(crate) is_running: bool,
    /// Whether the thread has emitted a close event and should render dimmed.
    pub(crate) is_closed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SubAgentActivityDisplay {
    pub(crate) thread_id: ThreadId,
    pub(crate) agent_path: String,
    /// An explicit liveness transition, when this activity represents one.
    ///
    /// `None` means that the activity is informational and must not affect the
    /// existing picker entry's liveness or closed state.
    pub(crate) running_update: Option<bool>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct AgentMetadata {
    /// Human-friendly nickname shown in rendered tool-call rows.
    pub(crate) agent_nickname: Option<String>,
    /// Agent type shown in brackets when present, for example `worker`.
    pub(crate) agent_role: Option<String>,
}

#[derive(Clone, Copy)]
struct AgentLabel<'a> {
    thread_id: Option<ThreadId>,
    nickname: Option<&'a str>,
    role: Option<&'a str>,
}

pub(crate) fn agent_picker_status_dot_spans(is_closed: bool) -> Vec<Span<'static>> {
    let dot = if is_closed {
        "•".into()
    } else {
        "•".green()
    };
    vec![dot, " ".into()]
}

pub(crate) fn format_agent_picker_item_name(
    agent_nickname: Option<&str>,
    agent_role: Option<&str>,
    is_primary: bool,
) -> String {
    if is_primary {
        return "Main [default]".to_string();
    }

    let agent_nickname = agent_nickname
        .map(str::trim)
        .filter(|nickname| !nickname.is_empty());
    let agent_role = agent_role.map(str::trim).filter(|role| !role.is_empty());
    match (agent_nickname, agent_role) {
        (Some(agent_nickname), Some(agent_role)) => format!("{agent_nickname} [{agent_role}]"),
        (Some(agent_nickname), None) => agent_nickname.to_string(),
        (None, Some(agent_role)) => format!("[{agent_role}]"),
        (None, None) => "Agent".to_string(),
    }
}

pub(crate) fn previous_agent_shortcut() -> crate::key_hint::KeyBinding {
    crate::key_hint::alt(KeyCode::Left)
}

pub(crate) fn next_agent_shortcut() -> crate::key_hint::KeyBinding {
    crate::key_hint::alt(KeyCode::Right)
}

/// Matches the canonical "previous agent" binding plus platform-specific fallbacks that keep agent
/// navigation working when enhanced key reporting is unavailable.
pub(crate) fn previous_agent_shortcut_matches(
    key_event: KeyEvent,
    allow_word_motion_fallback: bool,
) -> bool {
    previous_agent_shortcut().is_press(key_event)
        || previous_agent_word_motion_fallback(key_event, allow_word_motion_fallback)
}

/// Matches the canonical "next agent" binding plus platform-specific fallbacks that keep agent
/// navigation working when enhanced key reporting is unavailable.
pub(crate) fn next_agent_shortcut_matches(
    key_event: KeyEvent,
    allow_word_motion_fallback: bool,
) -> bool {
    next_agent_shortcut().is_press(key_event)
        || next_agent_word_motion_fallback(key_event, allow_word_motion_fallback)
}

#[cfg(target_os = "macos")]
fn previous_agent_word_motion_fallback(
    key_event: KeyEvent,
    allow_word_motion_fallback: bool,
) -> bool {
    // Some terminals, especially on macOS, send Option+b/f as word-motion keys instead of
    // Option+arrow events unless enhanced keyboard reporting is enabled. Callers should only
    // enable this fallback when the composer is empty so draft editing retains the expected
    // word-wise motion behavior.
    allow_word_motion_fallback
        && matches!(
            key_event,
            KeyEvent {
                code: KeyCode::Char('b'),
                modifiers: KeyModifiers::ALT,
                kind: KeyEventKind::Press | KeyEventKind::Repeat,
                ..
            }
        )
}

#[cfg(not(target_os = "macos"))]
fn previous_agent_word_motion_fallback(
    _key_event: KeyEvent,
    _allow_word_motion_fallback: bool,
) -> bool {
    false
}

#[cfg(target_os = "macos")]
fn next_agent_word_motion_fallback(key_event: KeyEvent, allow_word_motion_fallback: bool) -> bool {
    // Some terminals, especially on macOS, send Option+b/f as word-motion keys instead of
    // Option+arrow events unless enhanced keyboard reporting is enabled. Callers should only
    // enable this fallback when the composer is empty so draft editing retains the expected
    // word-wise motion behavior.
    allow_word_motion_fallback
        && matches!(
            key_event,
            KeyEvent {
                code: KeyCode::Char('f'),
                modifiers: KeyModifiers::ALT,
                kind: KeyEventKind::Press | KeyEventKind::Repeat,
                ..
            }
        )
}

#[cfg(not(target_os = "macos"))]
fn next_agent_word_motion_fallback(
    _key_event: KeyEvent,
    _allow_word_motion_fallback: bool,
) -> bool {
    false
}

pub(crate) fn tool_call_history_cell(
    item: &ThreadItem,
    mut agent_metadata: impl FnMut(ThreadId) -> AgentMetadata,
) -> Option<PlainHistoryCell> {
    let ThreadItem::CollabAgentToolCall {
        tool,
        status,
        receiver_thread_ids,
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
            Some(spawn_end(first_receiver, &mut agent_metadata))
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
        mode,
        agents_states,
        ..
    } = item
    else {
        return None;
    };

    let summary = match tool {
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
    Some(summary)
}

pub(crate) fn sub_agent_activity_display(item: &ThreadItem) -> Option<SubAgentActivityDisplay> {
    let ThreadItem::SubAgentActivity {
        kind,
        operation,
        outcome,
        agent_thread_id,
        agent_path,
        ..
    } = item
    else {
        return None;
    };
    Some(SubAgentActivityDisplay {
        thread_id: parse_thread_id(agent_thread_id)?,
        agent_path: agent_path.clone(),
        running_update: sub_agent_activity_running_update(*kind, *operation, *outcome),
    })
}

pub(crate) fn sub_agent_activity_history_cell(item: &ThreadItem) -> Option<PlainHistoryCell> {
    let ThreadItem::SubAgentActivity {
        kind,
        operation,
        outcome,
        agent_path,
        model,
        reasoning_effort,
        ..
    } = item
    else {
        return None;
    };
    Some(collab_event(
        sub_agent_activity_title(
            sub_agent_activity_action(*kind, *operation, *outcome),
            agent_path,
            model.as_deref(),
            reasoning_effort.as_ref(),
        ),
        Vec::new(),
    ))
}

pub(crate) fn sub_agent_activity_summary(
    kind: SubAgentActivityKind,
    operation: Option<SubAgentActivityOperation>,
    outcome: Option<SubAgentActivityOutcome>,
    agent_path: &str,
    model: Option<&str>,
    reasoning_effort: Option<&ReasoningEffort>,
) -> String {
    let action = sub_agent_activity_action(kind, operation, outcome);
    let details = sub_agent_activity_execution_details(action, model, reasoning_effort);
    format!(
        "{} {agent_path}{}",
        action.title_prefix(),
        details.unwrap_or_default()
    )
}

#[derive(Clone, Copy)]
enum SubAgentActivityAction {
    Started,
    Interacted,
    Interrupted,
    SentMessage,
    FailedToSendMessage,
    SentFollowup,
    FailedToSendFollowup,
    Replied,
    FailedToReply,
    Inspected,
    FailedToInspect,
}

impl SubAgentActivityAction {
    fn title_prefix(self) -> &'static str {
        match self {
            Self::Started => "Started",
            Self::Interacted => "Interacted with",
            Self::Interrupted => "Interrupted",
            Self::SentMessage => "Sent message to",
            Self::FailedToSendMessage => "Failed to send message to",
            Self::SentFollowup => "Sent follow-up to",
            Self::FailedToSendFollowup => "Failed to send follow-up to",
            Self::Replied => "Replied to",
            Self::FailedToReply => "Failed to reply to",
            Self::Inspected => "Inspected",
            Self::FailedToInspect => "Failed to inspect",
        }
    }
}

fn sub_agent_activity_action(
    kind: SubAgentActivityKind,
    operation: Option<SubAgentActivityOperation>,
    outcome: Option<SubAgentActivityOutcome>,
) -> SubAgentActivityAction {
    let failed = matches!(outcome, Some(SubAgentActivityOutcome::Failed));
    match operation {
        Some(SubAgentActivityOperation::SendMessage) if failed => {
            SubAgentActivityAction::FailedToSendMessage
        }
        Some(SubAgentActivityOperation::SendMessage) => SubAgentActivityAction::SentMessage,
        Some(SubAgentActivityOperation::FollowupTask) if failed => {
            SubAgentActivityAction::FailedToSendFollowup
        }
        Some(SubAgentActivityOperation::FollowupTask) => SubAgentActivityAction::SentFollowup,
        Some(SubAgentActivityOperation::ParentReply) if failed => {
            SubAgentActivityAction::FailedToReply
        }
        Some(SubAgentActivityOperation::ParentReply) => SubAgentActivityAction::Replied,
        Some(SubAgentActivityOperation::InspectAgent) if failed => {
            SubAgentActivityAction::FailedToInspect
        }
        Some(SubAgentActivityOperation::InspectAgent) => SubAgentActivityAction::Inspected,
        None => match kind {
            SubAgentActivityKind::Started => SubAgentActivityAction::Started,
            SubAgentActivityKind::Interacted => SubAgentActivityAction::Interacted,
            SubAgentActivityKind::Interrupted => SubAgentActivityAction::Interrupted,
        },
    }
}

fn sub_agent_activity_running_update(
    kind: SubAgentActivityKind,
    operation: Option<SubAgentActivityOperation>,
    outcome: Option<SubAgentActivityOutcome>,
) -> Option<bool> {
    if matches!(outcome, Some(SubAgentActivityOutcome::Failed)) {
        return None;
    }

    match kind {
        SubAgentActivityKind::Started => Some(true),
        SubAgentActivityKind::Interrupted => Some(false),
        SubAgentActivityKind::Interacted => match operation {
            Some(
                SubAgentActivityOperation::FollowupTask | SubAgentActivityOperation::ParentReply,
            ) if matches!(outcome, Some(SubAgentActivityOutcome::Succeeded)) => Some(true),
            Some(_) => None,
            // Preserve the legacy operation-free activity behavior.
            None if outcome.is_none() => Some(true),
            None => Some(false),
        },
    }
}

fn sub_agent_activity_title(
    action: SubAgentActivityAction,
    agent_path: &str,
    model: Option<&str>,
    reasoning_effort: Option<&ReasoningEffort>,
) -> Line<'static> {
    let mut spans = vec![
        Span::from(format!("{} ", action.title_prefix())).bold(),
        Span::from(format!("`{agent_path}`")).cyan(),
    ];
    if let Some(details) = sub_agent_activity_execution_details(action, model, reasoning_effort) {
        spans.push(Span::from(details).dim());
    }
    title_spans_line(spans)
}

fn sub_agent_activity_execution_details(
    action: SubAgentActivityAction,
    model: Option<&str>,
    reasoning_effort: Option<&ReasoningEffort>,
) -> Option<String> {
    if !matches!(action, SubAgentActivityAction::Started) {
        return None;
    }

    match (model.filter(|model| !model.is_empty()), reasoning_effort) {
        (Some(model), Some(reasoning_effort)) => Some(format!(" ({model}, {reasoning_effort})")),
        (Some(model), None) => Some(format!(" ({model})")),
        (None, Some(reasoning_effort)) => Some(format!(" ({reasoning_effort})")),
        (None, None) => None,
    }
}

fn spawn_end(
    new_thread_id: Option<ThreadId>,
    agent_metadata: &mut impl FnMut(ThreadId) -> AgentMetadata,
) -> PlainHistoryCell {
    let title = match new_thread_id {
        Some(thread_id) => title_with_agent(
            "Spawned",
            agent_label(thread_id, &agent_metadata(thread_id)),
        ),
        None => title_text("Agent spawn failed"),
    };

    collab_event(title, Vec::new())
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

fn collab_event(title: Line<'static>, details: Vec<Line<'static>>) -> PlainHistoryCell {
    let mut lines: Vec<Line<'static>> = vec![title];
    if !details.is_empty() {
        lines.extend(prefix_lines(details, "  └ ".dim(), "    ".into()));
    }
    PlainHistoryCell::new(lines)
}

fn title_text(title: impl Into<String>) -> Line<'static> {
    title_spans_line(vec![Span::from(title.into()).bold()])
}

fn title_with_agent(prefix: &str, agent: AgentLabel<'_>) -> Line<'static> {
    let mut spans = vec![Span::from(format!("{prefix} ")).bold()];
    spans.extend(agent_label_spans(agent));
    title_spans_line(spans)
}

fn title_spans_line(mut spans: Vec<Span<'static>>) -> Line<'static> {
    let mut title = Vec::with_capacity(spans.len() + 1);
    title.push(Span::from("• ").dim());
    title.append(&mut spans);
    title.into()
}

fn parse_thread_id(thread_id: &str) -> Option<ThreadId> {
    ThreadId::from_string(thread_id).ok()
}

fn agent_label(thread_id: ThreadId, metadata: &AgentMetadata) -> AgentLabel<'_> {
    AgentLabel {
        thread_id: Some(thread_id),
        nickname: metadata.agent_nickname.as_deref(),
        role: metadata.agent_role.as_deref(),
    }
}

fn agent_label_line(agent: AgentLabel<'_>) -> Line<'static> {
    agent_label_spans(agent).into()
}

fn agent_label_spans(agent: AgentLabel<'_>) -> Vec<Span<'static>> {
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

fn wait_complete_lines(
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

fn first_agent_state<'a>(
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

fn status_summary_line(status: Option<&CollabAgentState>) -> Line<'static> {
    match status {
        Some(status) => status_summary_spans(status).into(),
        None => error_summary_spans().into(),
    }
}

fn status_summary_spans(status: &CollabAgentState) -> Vec<Span<'static>> {
    match status.status {
        CollabAgentStatus::PendingInit => vec![Span::from("Pending init").cyan()],
        CollabAgentStatus::Running => vec![Span::from("Running").cyan().bold()],
        // Allow `.yellow()`
        #[allow(clippy::disallowed_methods)]
        CollabAgentStatus::Interrupted => vec![Span::from("Interrupted").yellow()],
        CollabAgentStatus::Completed => vec![Span::from("Completed").green()],
        CollabAgentStatus::Errored => error_summary_spans(),
        CollabAgentStatus::Shutdown => vec![Span::from("Shutdown")],
        CollabAgentStatus::NotFound => vec![Span::from("Not found").red()],
    }
}

fn error_summary_spans() -> Vec<Span<'static>> {
    vec![Span::from("Error").red()]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::history_cell::HistoryCell;
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
                    ..
                } => sub_agent_activity_summary(
                    *kind,
                    *operation,
                    *outcome,
                    agent_path,
                    model.as_deref(),
                    reasoning_effort.as_ref(),
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
}
