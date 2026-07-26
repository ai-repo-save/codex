//! History-cell rendering for agent mailbox actions.

use crate::history_cell::HistoryCell;
use crate::history_cell::PlainHistoryCell;
use crate::render::line_utils::prefix_lines;
use crate::wrapping::RtOptions;
use crate::wrapping::word_wrap_lines;
use codex_app_server_protocol::AgentMailboxAction;
use codex_app_server_protocol::AgentMailboxActionKind;
use codex_app_server_protocol::AgentMailboxMessageCategory;
use codex_app_server_protocol::AgentMailboxMessagePreview;
use codex_app_server_protocol::AgentMailboxMessagePreviewContent;
use codex_app_server_protocol::AgentMailboxActionStatus;
use codex_app_server_protocol::ThreadItem;
use ratatui::style::Stylize;
use ratatui::text::Line;

#[derive(Debug)]
pub(crate) struct AgentMailboxActionCell {
    action: AgentMailboxAction,
}

impl AgentMailboxActionCell {
    pub(crate) fn id(&self) -> &str {
        &self.action.id
    }

    pub(crate) fn action(&self) -> &AgentMailboxAction {
        &self.action
    }

    pub(crate) fn update(&mut self, action: AgentMailboxAction) {
        self.action = action;
    }

    pub(crate) fn mark_failed(&mut self) {
        self.action.status = AgentMailboxActionStatus::Failed;
    }

    fn plain_cell(&self) -> PlainHistoryCell {
        PlainHistoryCell::new(agent_mailbox_action_lines(&self.action))
    }
}

impl HistoryCell for AgentMailboxActionCell {
    fn display_lines(&self, width: u16) -> Vec<Line<'static>> {
        if width == 0 {
            return Vec::new();
        }

        let mut lines = word_wrap_lines(
            std::iter::once(agent_mailbox_action_title_line(&self.action)),
            RtOptions::new(width as usize),
        );
        for (index, detail) in agent_mailbox_action_details(&self.action)
            .into_iter()
            .enumerate()
        {
            lines.extend(word_wrap_lines(
                std::iter::once(detail),
                RtOptions::new(width as usize)
                    .initial_indent(if index == 0 {
                        "  └ ".dim().into()
                    } else {
                        "    ".into()
                    })
                    .subsequent_indent("    ".into()),
            ));
        }
        lines
    }

    fn raw_lines(&self) -> Vec<Line<'static>> {
        self.plain_cell().raw_lines()
    }
}

pub(crate) fn agent_mailbox_action_history_cell(
    item: &ThreadItem,
) -> Option<AgentMailboxActionCell> {
    let ThreadItem::AgentMailboxAction(action) = item else {
        return None;
    };

    Some(AgentMailboxActionCell {
        action: action.clone(),
    })
}

pub(crate) fn agent_mailbox_action_lines(action: &AgentMailboxAction) -> Vec<Line<'static>> {
    let mut lines = vec![agent_mailbox_action_title_line(action)];
    lines.extend(prefix_lines(
        agent_mailbox_action_details(action),
        "  └ ".dim(),
        "    ".into(),
    ));
    lines
}

fn agent_mailbox_action_title_line(action: &AgentMailboxAction) -> Line<'static> {
    vec!["• ".dim(), agent_mailbox_action_title(action).bold()].into()
}

fn agent_mailbox_action_details(action: &AgentMailboxAction) -> Vec<Line<'static>> {
    match &action.action {
        AgentMailboxActionKind::Send {
            target,
            recipient,
            category,
            preview,
        } => {
            let mut details = vec![vec![
                if recipient.is_some() {
                    "Recipient: ".dim()
                } else {
                    "Target: ".dim()
                },
                recipient.as_deref().unwrap_or(target).to_string().into(),
            ]
            .into()];
            details.push(
                vec!["Category: ".dim(), category_name(category).into()].into(),
            );
            if let Some(preview) = preview {
                details.push(vec!["Preview: ".dim(), preview.to_string().into()].into());
            }
            details
        }
        AgentMailboxActionKind::Read {
            sender,
            category,
            limit,
            messages,
        } => {
            let mut details = Vec::new();
            if let Some(sender) = sender {
                details.push(vec!["Sender: ".dim(), sender.to_string().into()].into());
            }
            if let Some(category) = category {
                details.push(
                    vec!["Category: ".dim(), category_name(category).into()].into(),
                );
            }
            details.push(vec!["Limit: ".dim(), limit.to_string().into()].into());
            if matches!(action.status, AgentMailboxActionStatus::Succeeded) {
                details.extend(messages.iter().map(agent_mailbox_message_line));
            }
            details
        }
    }
}

fn agent_mailbox_action_title(action: &AgentMailboxAction) -> String {
    match (&action.action, action.status) {
        (AgentMailboxActionKind::Send { .. }, AgentMailboxActionStatus::InProgress) => {
            "Sending mailbox message".to_string()
        }
        (AgentMailboxActionKind::Send { .. }, AgentMailboxActionStatus::Succeeded) => {
            "Sent mailbox message".to_string()
        }
        (AgentMailboxActionKind::Send { .. }, AgentMailboxActionStatus::Failed) => {
            "Failed to send mailbox message".to_string()
        }
        (
            AgentMailboxActionKind::Read { .. },
            AgentMailboxActionStatus::InProgress,
        ) => "Reading agent mailbox".to_string(),
        (
            AgentMailboxActionKind::Read { messages, .. },
            AgentMailboxActionStatus::Succeeded,
        ) if messages.is_empty() => "No mailbox messages found".to_string(),
        (
            AgentMailboxActionKind::Read { messages, .. },
            AgentMailboxActionStatus::Succeeded,
        ) => format!(
            "Read {} mailbox message{}",
            messages.len(),
            if messages.len() == 1 { "" } else { "s" }
        ),
        (AgentMailboxActionKind::Read { .. }, AgentMailboxActionStatus::Failed) => {
            "Failed to read agent mailbox".to_string()
        }
    }
}

fn agent_mailbox_message_line(
    message: &AgentMailboxMessagePreview,
) -> Line<'static> {
    let preview = match &message.content {
        AgentMailboxMessagePreviewContent::Plaintext { preview } => {
            preview.as_deref().unwrap_or("<empty message>").to_string()
        }
        AgentMailboxMessagePreviewContent::Encrypted => "<encrypted message>".to_string(),
    };
    vec![
        format!("{} · {}: ", message.sender, category_name(&message.category)).dim(),
        preview.into(),
    ]
    .into()
}

fn category_name(category: &AgentMailboxMessageCategory) -> &'static str {
    match category {
        AgentMailboxMessageCategory::Progress => "progress",
        AgentMailboxMessageCategory::Result => "result",
        AgentMailboxMessageCategory::ActionRequired => "action required",
    }
}
