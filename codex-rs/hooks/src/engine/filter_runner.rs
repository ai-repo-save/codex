use std::path::Path;

use serde::Deserialize;

use super::CommandShell;
use super::ConfiguredHandler;
use super::ConfiguredHandlerKind;
use super::command_runner::ShellCommandRequest;
use super::command_runner::run_shell_command;

const FILTER_OUTPUT_LIMIT: usize = 8 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PromptFilterOutcome {
    Run,
    Skip,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PromptFilterOutput {
    version: u8,
    decision: PromptFilterDecision,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
enum PromptFilterDecision {
    Run,
    Skip,
}

pub(crate) async fn run_prompt_filter(
    shell: &CommandShell,
    handler: &ConfiguredHandler,
    input_json: &str,
    cwd: &Path,
) -> PromptFilterOutcome {
    let ConfiguredHandlerKind::Prompt { filter, .. } = &handler.kind else {
        return PromptFilterOutcome::Run;
    };
    let Some(filter) = filter else {
        return PromptFilterOutcome::Run;
    };
    let completion = run_shell_command(ShellCommandRequest {
        shell,
        command_text: &filter.command,
        env: &handler.env,
        input_json,
        cwd,
        timeout_sec: filter.timeout_sec,
        output_limit: Some(FILTER_OUTPUT_LIMIT),
        timeout_error: format!("prompt hook filter timed out after {}s", filter.timeout_sec),
    })
    .await;

    let failure = if completion.error.is_some() {
        Some(completion.outcome)
    } else if completion.exit_code != Some(0) {
        Some("nonzero_exit")
    } else if completion.stdout.trim().is_empty() {
        Some("empty_output")
    } else {
        None
    };
    if let Some(category) = failure {
        log_filter_failure(
            handler,
            category,
            completion.stdout_len,
            completion.stderr_len,
        );
        return PromptFilterOutcome::Run;
    }

    let output = match serde_json::from_str::<PromptFilterOutput>(&completion.stdout) {
        Ok(output) if output.version == 1 => output,
        Ok(_) => {
            log_filter_failure(
                handler,
                "unsupported_version",
                completion.stdout_len,
                completion.stderr_len,
            );
            return PromptFilterOutcome::Run;
        }
        Err(_) => {
            log_filter_failure(
                handler,
                "invalid_output",
                completion.stdout_len,
                completion.stderr_len,
            );
            return PromptFilterOutcome::Run;
        }
    };
    match output.decision {
        PromptFilterDecision::Run => PromptFilterOutcome::Run,
        PromptFilterDecision::Skip => PromptFilterOutcome::Skip,
    }
}

fn log_filter_failure(
    handler: &ConfiguredHandler,
    category: &'static str,
    stdout_len: usize,
    stderr_len: usize,
) {
    tracing::warn!(
        hook.event_name = super::dispatcher::hook_event_name_label(handler.event_name),
        hook.source = super::dispatcher::hook_source_label(handler.source),
        hook.display_order = handler.display_order,
        hook.filter_failure = category,
        hook.filter_stdout_len = stdout_len,
        hook.filter_stderr_len = stderr_len,
        "prompt hook filter failed; falling back to model evaluation"
    );
}

#[cfg(test)]
#[path = "filter_runner_tests.rs"]
mod tests;
