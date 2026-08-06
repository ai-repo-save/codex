//! Sub-agent activity display helpers.

use crate::history_cell::PlainHistoryCell;
use codex_app_server_protocol::SpawnContextInheritance;
use codex_app_server_protocol::SubAgentActivityKind;
use codex_app_server_protocol::SubAgentActivityOperation;
use codex_app_server_protocol::SubAgentActivityOutcome;
use codex_app_server_protocol::ThreadItem;
use codex_protocol::openai_models::ReasoningEffort;
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::text::Span;

use super::SubAgentActivityDisplay;
use super::render::collab_event;
use super::render::context_inheritance_detail;
use super::render::is_fast_service_tier;
use super::render::parse_thread_id;
use super::render::title_spans_line;

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
        service_tier,
        context_inheritance,
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
            service_tier.as_deref(),
            context_inheritance.as_ref(),
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
    service_tier: Option<&str>,
    context_inheritance: Option<&SpawnContextInheritance>,
) -> String {
    let action = sub_agent_activity_action(kind, operation, outcome);
    let details = sub_agent_activity_execution_details(
        action,
        model,
        reasoning_effort,
        service_tier,
        context_inheritance,
    );
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
    service_tier: Option<&str>,
    context_inheritance: Option<&SpawnContextInheritance>,
) -> Line<'static> {
    let mut spans = vec![
        Span::from(format!("{} ", action.title_prefix())).bold(),
        Span::from(format!("`{agent_path}`")).cyan(),
    ];
    if let Some(details) = sub_agent_activity_execution_details(
        action,
        model,
        reasoning_effort,
        service_tier,
        context_inheritance,
    ) {
        spans.push(Span::from(details).dim());
    }
    title_spans_line(spans)
}

fn sub_agent_activity_execution_details(
    action: SubAgentActivityAction,
    model: Option<&str>,
    reasoning_effort: Option<&ReasoningEffort>,
    service_tier: Option<&str>,
    context_inheritance: Option<&SpawnContextInheritance>,
) -> Option<String> {
    if !matches!(action, SubAgentActivityAction::Started) {
        return None;
    }

    let mut details = Vec::new();
    if let Some(model) = model.filter(|model| !model.is_empty()) {
        details.push(model.to_string());
    }
    if let Some(reasoning_effort) = reasoning_effort {
        details.push(reasoning_effort.to_string());
    }
    if is_fast_service_tier(service_tier) {
        details.push("fast".to_string());
    }
    let execution_details = (!details.is_empty()).then(|| format!(" ({})", details.join(", ")));
    match (
        execution_details,
        context_inheritance_detail(context_inheritance),
    ) {
        (Some(execution_details), Some(context_detail)) => {
            Some(format!("{execution_details}{context_detail}"))
        }
        (Some(execution_details), None) => Some(execution_details),
        (None, Some(context_detail)) => Some(context_detail),
        (None, None) => None,
    }
}
