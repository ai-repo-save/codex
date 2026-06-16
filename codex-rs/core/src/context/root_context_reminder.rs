use super::ContextualUserFragment;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RootContextReminder<'a> {
    pub(crate) remaining_percent: i64,
    pub(crate) message_template: &'a str,
}

impl<'a> RootContextReminder<'a> {
    pub(crate) fn new(remaining_percent: i64, message_template: &'a str) -> Self {
        Self {
            remaining_percent,
            message_template,
        }
    }
}

impl ContextualUserFragment for RootContextReminder<'_> {
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
        let remaining_percent = self.remaining_percent.to_string();
        let message = self
            .message_template
            .replace("{remaining_percent}", &remaining_percent);
        format!("\n{message}\n")
    }
}
