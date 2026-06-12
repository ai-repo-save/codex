use super::ContextualUserFragment;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RootContextReminder {
    pub(crate) remaining_percent: i64,
}

impl RootContextReminder {
    pub(crate) fn new(remaining_percent: i64) -> Self {
        Self { remaining_percent }
    }
}

impl ContextualUserFragment for RootContextReminder {
    fn role(&self) -> &'static str {
        "developer"
    }

    fn markers(&self) -> (&'static str, &'static str) {
        Self::type_markers()
    }

    fn type_markers() -> (&'static str, &'static str) {
        ("<root_context_reminder>", "</root_context_reminder>")
    }

    fn body(&self) -> String {
        format!(
            "\nContext remaining is about {}%. Before continuing substantial work, call `request_context_compaction` and preserve the goal, verified state, current changes, and next step.\n",
            self.remaining_percent
        )
    }
}
