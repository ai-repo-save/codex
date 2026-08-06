//! Shared prompt-hook stdout classification for event parsers.
//!
//! Fork-owned helpers keep empty-output / invalid-JSON prompt rules out of each
//! upstream-shared `events/*.rs` parse path.

use codex_protocol::protocol::HookHandlerType;
use codex_protocol::protocol::HookOutputEntry;
use codex_protocol::protocol::HookOutputEntryKind;
use codex_protocol::protocol::HookRunStatus;

use crate::engine::ConfiguredHandler;
use crate::engine::output_parser;

pub(crate) const EMPTY_PROMPT_OUTPUT_ERROR: &str = "prompt hook returned empty output";

/// Result of inspecting exit-code-0 stdout before event-specific JSON parsing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ExitZeroStdout<'a> {
    /// Command hooks treat empty stdout as success with no payload.
    EmptyCommandNoop,
    /// Prompt hooks require non-empty JSON output.
    EmptyPromptFailed,
    NonEmpty(&'a str),
}

pub(crate) fn classify_exit_zero_stdout<'a>(
    handler: &ConfiguredHandler,
    stdout: &'a str,
) -> ExitZeroStdout<'a> {
    let trimmed = stdout.trim();
    if trimmed.is_empty() {
        if handler.handler_type() == HookHandlerType::Prompt {
            ExitZeroStdout::EmptyPromptFailed
        } else {
            ExitZeroStdout::EmptyCommandNoop
        }
    } else {
        ExitZeroStdout::NonEmpty(trimmed)
    }
}

pub(crate) fn push_empty_prompt_output_error(
    status: &mut HookRunStatus,
    entries: &mut Vec<HookOutputEntry>,
) {
    *status = HookRunStatus::Failed;
    entries.push(HookOutputEntry {
        kind: HookOutputEntryKind::Error,
        text: EMPTY_PROMPT_OUTPUT_ERROR.to_string(),
    });
}

/// Prompt hooks always fail on unparsable stdout; command hooks only fail when
/// the payload looks like JSON.
pub(crate) fn should_fail_unparsed_stdout(handler: &ConfiguredHandler, stdout: &str) -> bool {
    handler.handler_type() == HookHandlerType::Prompt || output_parser::looks_like_json(stdout)
}

pub(crate) fn push_invalid_json_output_error(
    status: &mut HookRunStatus,
    entries: &mut Vec<HookOutputEntry>,
    message: impl Into<String>,
) {
    *status = HookRunStatus::Failed;
    entries.push(HookOutputEntry {
        kind: HookOutputEntryKind::Error,
        text: message.into(),
    });
}
