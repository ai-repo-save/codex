use std::path::Path;

use codex_protocol::protocol::HookEventName;
use codex_protocol::protocol::HookSource;
use codex_utils_absolute_path::AbsolutePathBuf;
use pretty_assertions::assert_eq;

use super::super::CommandShell;
use super::super::ConfiguredHandler;
use super::super::ConfiguredHandlerKind;
use super::super::ConfiguredPromptFilter;
use super::PromptFilterOutcome;
use super::run_prompt_filter;

const FILTER_INPUT: &str = "{}";
const FILTER_PROMPT: &str = "Review $$ARGUMENTS";
const FILTER_COMMAND_TIMEOUT_SEC: u64 = 1;
const FILTER_OUTPUT_BYTES: usize = 8 * 1024 + 1;

#[tokio::test]
async fn prompt_filter_accepts_strict_run_and_skip_json() {
    for (command, expected) in [
        (stdout_command(r#"{"version":1,"decision":"run"}"#), PromptFilterOutcome::Run),
        (
            stdout_command(r#"{"version":1,"decision":"skip"}"#),
            PromptFilterOutcome::Skip,
        ),
    ] {
        assert_eq!(
            run_filter(command, FILTER_COMMAND_TIMEOUT_SEC).await,
            expected
        );
    }
}

#[tokio::test]
async fn prompt_filter_falls_back_for_invalid_or_unsuccessful_output() {
    for command in [
        stdout_command(r#"{"version":1,"decision":"run","unexpected":true}"#),
        stdout_command(r#"{"version":1,"decision":"skip"} trailing"#),
        empty_output_command(),
        nonzero_exit_command(),
    ] {
        assert_eq!(
            run_filter(command, FILTER_COMMAND_TIMEOUT_SEC).await,
            PromptFilterOutcome::Run
        );
    }
}

#[tokio::test]
async fn prompt_filter_falls_back_for_timeout_and_unbounded_streams() {
    for command in [
        timeout_command(),
        oversized_stdout_command(FILTER_OUTPUT_BYTES),
        oversized_stderr_command(FILTER_OUTPUT_BYTES),
    ] {
        assert_eq!(
            run_filter(command, FILTER_COMMAND_TIMEOUT_SEC).await,
            PromptFilterOutcome::Run
        );
    }
}

async fn run_filter(command: String, timeout_sec: u64) -> PromptFilterOutcome {
    run_prompt_filter(
        &shell(),
        &filter_handler(command, timeout_sec),
        FILTER_INPUT,
        Path::new("."),
    )
    .await
}

fn filter_handler(command: String, timeout_sec: u64) -> ConfiguredHandler {
    ConfiguredHandler {
        event_name: HookEventName::PreToolUse,
        matcher: None,
        kind: ConfiguredHandlerKind::Prompt {
            prompt: FILTER_PROMPT.to_string(),
            filter: Some(ConfiguredPromptFilter {
                command,
                timeout_sec,
            }),
            model: None,
            reasoning_effort: None,
            timeout_sec: 30,
            fail_closed: false,
        },
        status_message: None,
        source_path: AbsolutePathBuf::current_dir().expect("current directory"),
        source: HookSource::User,
        display_order: 0,
        env: std::collections::HashMap::new(),
    }
}

#[cfg(not(windows))]
fn shell() -> CommandShell {
    CommandShell {
        program: "sh".to_string(),
        args: vec!["-c".to_string()],
    }
}

#[cfg(windows)]
fn shell() -> CommandShell {
    CommandShell {
        program: "cmd.exe".to_string(),
        args: vec!["/D".to_string(), "/S".to_string(), "/C".to_string()],
    }
}

#[cfg(not(windows))]
fn stdout_command(output: &str) -> String {
    format!("printf '%s' '{output}'")
}

#[cfg(windows)]
fn stdout_command(output: &str) -> String {
    format!("echo|set /p={output}")
}

#[cfg(not(windows))]
fn empty_output_command() -> String {
    ":".to_string()
}

#[cfg(windows)]
fn empty_output_command() -> String {
    "rem".to_string()
}

#[cfg(not(windows))]
fn nonzero_exit_command() -> String {
    "exit 7".to_string()
}

#[cfg(windows)]
fn nonzero_exit_command() -> String {
    "exit /b 7".to_string()
}

#[cfg(not(windows))]
fn timeout_command() -> String {
    "sleep 2".to_string()
}

#[cfg(windows)]
fn timeout_command() -> String {
    "ping -n 3 127.0.0.1 > nul".to_string()
}

#[cfg(not(windows))]
fn oversized_stdout_command(bytes: usize) -> String {
    format!("head -c {bytes} /dev/zero | tr '\\0' x")
}

#[cfg(windows)]
fn oversized_stdout_command(bytes: usize) -> String {
    format!("for /L %i in (1,1,{bytes}) do @<nul set /p =x")
}

#[cfg(not(windows))]
fn oversized_stderr_command(bytes: usize) -> String {
    format!("head -c {bytes} /dev/zero >&2")
}

#[cfg(windows)]
fn oversized_stderr_command(bytes: usize) -> String {
    format!("for /L %i in (1,1,{bytes}) do @<nul set /p =x 1>&2")
}
