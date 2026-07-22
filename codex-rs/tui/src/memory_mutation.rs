//! History-cell rendering for scoped memory mutations.

use crate::history_cell::PlainHistoryCell;
use crate::render::line_utils::prefix_lines;
use codex_app_server_protocol::MemoryMutation;
use codex_app_server_protocol::MemoryMutationAction;
use codex_app_server_protocol::MemoryMutationScope;
use codex_app_server_protocol::MemoryMutationStatus;
use codex_app_server_protocol::ThreadItem;
use ratatui::style::Stylize;

pub(crate) fn memory_mutation_history_cell(item: &ThreadItem) -> Option<PlainHistoryCell> {
    let ThreadItem::MemoryMutation(mutation) = item else {
        return None;
    };

    let mut lines = vec![vec!["• ".dim(), memory_mutation_title(mutation).bold()].into()];
    let mut details = vec![
        vec![
            "Scope: ".dim(),
            memory_mutation_scope(mutation.scope).into(),
        ]
        .into(),
    ];
    if let Some(title) = mutation.title.as_deref() {
        details.push(vec!["Title: ".dim(), title.to_string().into()].into());
    }
    if let Some(path) = mutation.path.as_deref() {
        details.push(vec!["Path: ".dim(), path.to_string().into()].into());
    }
    if let Some(preview) = mutation.preview.as_deref() {
        details.push(vec!["Preview: ".dim(), preview.to_string().into()].into());
    }
    lines.extend(prefix_lines(details, "  └ ".dim(), "    ".into()));
    Some(PlainHistoryCell::new(lines))
}

pub(crate) fn memory_mutation_summary(mutation: &MemoryMutation) -> String {
    let mut details = vec![format!("scope: {}", memory_mutation_scope(mutation.scope))];
    if let Some(title) = mutation.title.as_deref() {
        details.push(format!("title: {title}"));
    }
    if let Some(path) = mutation.path.as_deref() {
        details.push(format!("path: {path}"));
    }
    if let Some(preview) = mutation.preview.as_deref() {
        details.push(format!("preview: {preview}"));
    }
    format!(
        "{} · {}",
        memory_mutation_title(mutation),
        details.join(" · ")
    )
}

fn memory_mutation_title(mutation: &MemoryMutation) -> &'static str {
    match (mutation.action, mutation.status) {
        (MemoryMutationAction::Write, MemoryMutationStatus::InProgress) => "Writing memory",
        (MemoryMutationAction::Write, MemoryMutationStatus::Succeeded) => "Wrote memory",
        (MemoryMutationAction::Write, MemoryMutationStatus::Failed) => "Failed to write memory",
        (MemoryMutationAction::Delete, MemoryMutationStatus::InProgress) => "Deleting memory",
        (MemoryMutationAction::Delete, MemoryMutationStatus::Succeeded) => "Deleted memory",
        (MemoryMutationAction::Delete, MemoryMutationStatus::Failed) => "Failed to delete memory",
    }
}

fn memory_mutation_scope(scope: MemoryMutationScope) -> &'static str {
    match scope {
        MemoryMutationScope::Global => "global",
        MemoryMutationScope::Session => "session",
        MemoryMutationScope::Project => "project",
    }
}
