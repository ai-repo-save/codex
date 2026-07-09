use std::borrow::Cow;

use codex_protocol::models::SearchToolCallParams;

/// Canonical payload shapes accepted by model-visible tool runtimes.
#[derive(Clone, Debug, PartialEq)]
pub enum ToolPayload {
    Function { arguments: String },
    ToolSearch { arguments: SearchToolCallParams },
    Custom { input: String },
}

impl ToolPayload {
    pub fn log_payload(&self) -> Cow<'_, str> {
        match self {
            ToolPayload::Function { arguments } => Cow::Borrowed(arguments),
            ToolPayload::ToolSearch { arguments } => Cow::Owned(
                arguments
                    .query
                    .as_deref()
                    .or(arguments.mcp_prefix.as_deref())
                    .unwrap_or_default()
                    .to_string(),
            ),
            ToolPayload::Custom { input } => Cow::Borrowed(input),
        }
    }
}
