use std::path::Path;
use std::time::Duration;
use std::time::Instant;

use codex_protocol::openai_models::ReasoningEffort;
use codex_protocol::protocol::HookEventName;
use codex_utils_output_truncation::approx_token_count;
use futures::future::BoxFuture;
use serde_json::Value;
use tokio::time::timeout;

use super::CommandShell;
use super::ConfiguredHandler;
use super::ConfiguredHandlerKind;
use super::HandlerRunResult;
use super::command_runner::CommandRunResult;
use super::filter_runner::PromptFilterOutcome;
use super::filter_runner::run_prompt_filter;

const PROMPT_ARGUMENTS_PLACEHOLDER: &str = "$$ARGUMENTS";
const PROMPT_HOOK_EVENT_JSON_LIMIT: usize = 64 * 1024;
const PROMPT_HOOK_INPUT_TOKEN_LIMIT: usize = 8_192;
const UNTRUSTED_EVENT_JSON_PREFIX: &str = "<untrusted-hook-event-json>\n";
const UNTRUSTED_EVENT_JSON_SUFFIX: &str = "\n</untrusted-hook-event-json>";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromptHookRequest {
    pub rendered_prompt: String,
    pub model: Option<String>,
    pub reasoning_effort: Option<ReasoningEffort>,
    pub event_name: HookEventName,
    pub output_schema: Value,
}

/// Executes model-backed hook requests without coupling the hooks crate to a model client.
///
/// Implementations must return exactly one JSON value conforming to `output_schema` and must not
/// expose tool execution to the evaluator.
pub trait PromptHookRunner: Send + Sync {
    fn run(&self, request: PromptHookRequest) -> BoxFuture<'static, anyhow::Result<String>>;
}

pub(crate) fn supports_event(event_name: HookEventName) -> bool {
    matches!(
        event_name,
        HookEventName::PreToolUse
            | HookEventName::PermissionRequest
            | HookEventName::ApprovalReviewRoute
            | HookEventName::PreCompact
            | HookEventName::SessionStart
            | HookEventName::UserPromptSubmit
            | HookEventName::SubagentStart
    )
}

pub(crate) async fn run_prompt(
    runner: Option<&dyn PromptHookRunner>,
    shell: &CommandShell,
    handler: &ConfiguredHandler,
    input_json: &str,
    cwd: &Path,
) -> HandlerRunResult {
    let started_at = chrono::Utc::now().timestamp();
    let started = Instant::now();
    let ConfiguredHandlerKind::Prompt {
        prompt,
        model,
        reasoning_effort,
        timeout_sec,
        ..
    } = &handler.kind
    else {
        return HandlerRunResult::completed(failed_prompt_run(
            started_at,
            started,
            "command handler cannot run as a prompt hook".to_string(),
        ));
    };
    if run_prompt_filter(shell, handler, input_json, cwd).await == PromptFilterOutcome::Skip {
        return HandlerRunResult::prompt_filter_skipped(prompt_run_result(
            started_at,
            started,
            Some(0),
            String::new(),
            /*error*/ None,
        ));
    }
    let Some(runner) = runner else {
        return HandlerRunResult::completed(failed_prompt_run(
            started_at,
            started,
            "prompt hook cannot run because no prompt runner is configured".to_string(),
        ));
    };
    let rendered_prompt = match render_prompt(prompt, input_json) {
        Ok(rendered_prompt) => rendered_prompt,
        Err(error) => {
            return HandlerRunResult::completed(failed_prompt_run(started_at, started, error));
        }
    };
    let schemas = super::schema_loader::generated_hook_schemas();
    let output_schema = match handler.event_name {
        HookEventName::PreToolUse => schemas.pre_tool_use_prompt_output.clone(),
        HookEventName::PermissionRequest => schemas.permission_request_command_output.clone(),
        HookEventName::ApprovalReviewRoute => schemas.approval_review_route_command_output.clone(),
        HookEventName::PreCompact => schemas.pre_compact_command_output.clone(),
        HookEventName::SessionStart => schemas.session_start_command_output.clone(),
        HookEventName::UserPromptSubmit => schemas.user_prompt_submit_command_output.clone(),
        HookEventName::SubagentStart => schemas.subagent_start_command_output.clone(),
        HookEventName::PostToolUse
        | HookEventName::SessionEnd
        | HookEventName::SubagentStop
        | HookEventName::PostCompact
        | HookEventName::Stop => {
            return HandlerRunResult::completed(failed_prompt_run(
                started_at,
                started,
                format!(
                    "prompt hooks are not supported for {}",
                    super::dispatcher::hook_event_name_label(handler.event_name)
                ),
            ));
        }
    };
    let request = PromptHookRequest {
        rendered_prompt,
        model: model.clone(),
        reasoning_effort: reasoning_effort.clone(),
        event_name: handler.event_name,
        output_schema,
    };

    match timeout(Duration::from_secs(*timeout_sec), runner.run(request)).await {
        Ok(Ok(stdout)) => HandlerRunResult::completed(prompt_run_result(
            started_at,
            started,
            Some(0),
            stdout,
            /*error*/ None,
        )),
        Ok(Err(error)) => {
            HandlerRunResult::completed(failed_prompt_run(started_at, started, error.to_string()))
        }
        Err(_) => HandlerRunResult::completed(failed_prompt_run(
            started_at,
            started,
            format!("prompt hook timed out after {timeout_sec}s"),
        )),
    }
}

fn render_prompt(prompt: &str, input_json: &str) -> Result<String, String> {
    if input_json.len() > PROMPT_HOOK_EVENT_JSON_LIMIT {
        return Err(format!(
            "prompt hook event JSON exceeds the {PROMPT_HOOK_EVENT_JSON_LIMIT}-byte limit"
        ));
    }
    let escaped_input_json = input_json.replace('<', "\\u003c").replace('>', "\\u003e");
    let untrusted_event_json =
        format!("{UNTRUSTED_EVENT_JSON_PREFIX}{escaped_input_json}{UNTRUSTED_EVENT_JSON_SUFFIX}");
    let rendered = if prompt.contains(PROMPT_ARGUMENTS_PLACEHOLDER) {
        prompt.replace(PROMPT_ARGUMENTS_PLACEHOLDER, &untrusted_event_json)
    } else {
        format!("{prompt}\n\n{untrusted_event_json}")
    };
    let estimated_tokens = approx_token_count(&rendered);
    if estimated_tokens > PROMPT_HOOK_INPUT_TOKEN_LIMIT {
        return Err(format!(
            "prompt hook input is estimated at {estimated_tokens} tokens, exceeding the {PROMPT_HOOK_INPUT_TOKEN_LIMIT}-token limit"
        ));
    }
    Ok(rendered)
}

fn failed_prompt_run(started_at: i64, started: Instant, error: String) -> CommandRunResult {
    prompt_run_result(
        started_at,
        started,
        /*exit_code*/ None,
        String::new(),
        Some(error),
    )
}

fn prompt_run_result(
    started_at: i64,
    started: Instant,
    exit_code: Option<i32>,
    stdout: String,
    error: Option<String>,
) -> CommandRunResult {
    CommandRunResult {
        started_at,
        completed_at: chrono::Utc::now().timestamp(),
        duration_ms: started.elapsed().as_millis().try_into().unwrap_or(i64::MAX),
        exit_code,
        stdout,
        stderr: String::new(),
        error,
    }
}

#[cfg(test)]
#[path = "prompt_runner_tests.rs"]
mod tests;
