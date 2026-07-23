use codex_protocol::protocol::HookEventName;
use codex_protocol::protocol::HookRunStatus;
use codex_utils_absolute_path::test_support::PathBufExt;
use codex_utils_absolute_path::test_support::test_path_buf;
use pretty_assertions::assert_eq;

use super::PermissionRequestDecision;
use super::parse_completed;
use super::resolve_permission_request_decision;
use crate::engine::ConfiguredHandler;
use crate::engine::ConfiguredHandlerKind;
use crate::engine::HandlerRunResult;
use crate::engine::command_runner::CommandRunResult;

const RUNTIME_FAILURE: &str = "model request failed";
const TIMEOUT_FAILURE: &str = "prompt hook timed out after 30s";

#[test]
fn permission_request_deny_overrides_earlier_allow() {
    let decisions = [
        PermissionRequestDecision::Allow,
        PermissionRequestDecision::Deny {
            message: "repo deny".to_string(),
        },
    ];

    assert_eq!(
        resolve_permission_request_decision(decisions.iter()),
        Some(PermissionRequestDecision::Deny {
            message: "repo deny".to_string(),
        })
    );
}

#[test]
fn permission_request_returns_allow_when_no_handler_denies() {
    let decisions = [
        PermissionRequestDecision::Allow,
        PermissionRequestDecision::Allow,
    ];

    assert_eq!(
        resolve_permission_request_decision(decisions.iter()),
        Some(PermissionRequestDecision::Allow)
    );
}

#[test]
fn permission_request_returns_none_when_no_handler_decides() {
    let decisions = Vec::<PermissionRequestDecision>::new();

    assert_eq!(resolve_permission_request_decision(decisions.iter()), None);
}

#[test]
fn failed_prompt_hook_honors_fail_closed_without_changing_failed_status() {
    for failure in [RUNTIME_FAILURE, TIMEOUT_FAILURE] {
        let fail_open = parse_completed(
            &prompt_handler(/*fail_closed*/ false),
            failed_run(failure),
            Some("turn-1".to_string()),
        );
        let fail_closed = parse_completed(
            &prompt_handler(/*fail_closed*/ true),
            failed_run(failure),
            Some("turn-1".to_string()),
        );

        assert_eq!(fail_open.completed.run.status, HookRunStatus::Failed);
        assert_eq!(fail_open.data.decision, None);
        assert_eq!(fail_closed.completed.run.status, HookRunStatus::Failed);
        assert_eq!(
            fail_closed.data.decision,
            Some(PermissionRequestDecision::Deny {
                message: failure.to_string(),
            })
        );
    }
}

#[test]
fn invalid_prompt_output_honors_failure_policy_and_preserves_failed_status() {
    let fail_open = parse_completed(
        &prompt_handler(/*fail_closed*/ false),
        run_result(/*exit_code*/ Some(0), "not json", ""),
        Some("turn-1".to_string()),
    );
    let fail_closed = parse_completed(
        &prompt_handler(/*fail_closed*/ true),
        run_result(/*exit_code*/ Some(0), "not json", ""),
        Some("turn-1".to_string()),
    );

    assert_eq!(fail_open.completed.run.status, HookRunStatus::Failed);
    assert_eq!(fail_open.data.decision, None);
    assert_eq!(fail_closed.completed.run.status, HookRunStatus::Failed);
    assert!(matches!(
        fail_closed.data.decision,
        Some(PermissionRequestDecision::Deny { .. })
    ));
}

#[test]
fn valid_prompt_output_uses_permission_request_decision_contract() {
    let parsed = parse_completed(
        &prompt_handler(/*fail_closed*/ true),
        run_result(
            /*exit_code*/ Some(0),
            r#"{"hookSpecificOutput":{"hookEventName":"PermissionRequest","decision":{"behavior":"allow"}}}"#,
            "",
        ),
        Some("turn-1".to_string()),
    );

    assert_eq!(parsed.completed.run.status, HookRunStatus::Completed);
    assert_eq!(parsed.data.decision, Some(PermissionRequestDecision::Allow));
}

#[test]
fn command_failure_remains_fail_open() {
    let parsed = parse_completed(
        &command_handler(),
        failed_run("command timed out"),
        Some("turn-1".to_string()),
    );

    assert_eq!(parsed.completed.run.status, HookRunStatus::Failed);
    assert_eq!(parsed.data.decision, None);
}

fn command_handler() -> ConfiguredHandler {
    ConfiguredHandler {
        event_name: HookEventName::PermissionRequest,
        matcher: Some("^Bash$".to_string()),
        kind: ConfiguredHandlerKind::Command {
            command: "echo hook".to_string(),
            timeout_sec: 5,
        },
        status_message: None,
        additional_context_limit: Default::default(),
        source_path: test_path_buf("/tmp/hooks.json").abs(),
        source: codex_protocol::protocol::HookSource::User,
        display_order: 0,
        env: std::collections::HashMap::new(),
    }
}

fn prompt_handler(fail_closed: bool) -> ConfiguredHandler {
    ConfiguredHandler {
        event_name: HookEventName::PermissionRequest,
        matcher: Some("^Bash$".to_string()),
        kind: ConfiguredHandlerKind::Prompt {
            prompt: "Review $$ARGUMENTS".to_string(),
            filter: None,
            model: None,
            reasoning_effort: None,
            timeout_sec: 30,
            fail_closed,
        },
        status_message: None,
        additional_context_limit: Default::default(),
        source_path: test_path_buf("/tmp/hooks.json").abs(),
        source: codex_protocol::protocol::HookSource::User,
        display_order: 0,
        env: std::collections::HashMap::new(),
    }
}

fn failed_run(error: &str) -> HandlerRunResult {
    let mut result = run_result(/*exit_code*/ None, "", "");
    result.error = Some(error.to_string());
    result
}

fn run_result(exit_code: Option<i32>, stdout: &str, stderr: &str) -> HandlerRunResult {
    HandlerRunResult::completed(CommandRunResult {
        started_at: 1,
        completed_at: 2,
        duration_ms: 1,
        exit_code,
        stdout: stdout.to_string(),
        stderr: stderr.to_string(),
        error: None,
    })
}
