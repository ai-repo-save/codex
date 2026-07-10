use super::ContextualUserFragment;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ContextReminder<'a> {
    pub(crate) remaining_percent: Option<i64>,
    pub(crate) used_tokens: i64,
    pub(crate) used_tokens_threshold: Option<i64>,
    pub(crate) message_template: &'a str,
}

impl<'a> ContextReminder<'a> {
    pub(crate) fn new(
        remaining_percent: Option<i64>,
        used_tokens: i64,
        used_tokens_threshold: Option<i64>,
        message_template: &'a str,
    ) -> Self {
        Self {
            remaining_percent,
            used_tokens,
            used_tokens_threshold,
            message_template,
        }
    }
}

impl ContextualUserFragment for ContextReminder<'_> {
    fn role(&self) -> &'static str {
        "developer"
    }

    fn markers(&self) -> (&'static str, &'static str) {
        Self::type_markers()
    }

    fn type_markers() -> (&'static str, &'static str) {
        ("<context_reminder>", "</context_reminder>")
    }

    fn body(&self) -> String {
        let remaining_percent = self
            .remaining_percent
            .map_or_else(|| "unknown".to_string(), |value| value.to_string());
        let used_tokens = self.used_tokens.to_string();
        let used_tokens_threshold = self
            .used_tokens_threshold
            .map_or_else(|| "not configured".to_string(), |value| value.to_string());
        let message = self
            .message_template
            .replace("{remaining_percent}", &remaining_percent)
            .replace("{used_tokens}", &used_tokens)
            .replace("{used_tokens_threshold}", &used_tokens_threshold);
        format!("\n{message}\n")
    }
}
