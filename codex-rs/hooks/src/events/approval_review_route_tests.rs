use std::sync::Arc;
use std::sync::Mutex;

use anyhow::anyhow;
use codex_protocol::ThreadId;
use codex_protocol::protocol::HookEventName;
use codex_protocol::protocol::HookRunStatus;
use codex_protocol::protocol::HookSource;
use futures::FutureExt;
use futures::future::BoxFuture;
use pretty_assertions::assert_eq;
use serde_json::json;

use super::*;
use crate::engine::ConfiguredHandlerKind;
use crate::engine::PromptHookRequest;

#[derive(Clone)]
struct RecordingRunner {
    requests: Arc<Mutex<Vec<PromptHookRequest>>>,
    result: Result<String, String>,
}

impl PromptHookRunner for RecordingRunner {
    fn run(&self, request: PromptHookRequest) -> BoxFuture<'static, anyhow::Result<String>> {
        self.requests.lock().expect("request lock").push(request);
        let result = self.result.clone();
        async move { result.map_err(|error| anyhow!("{error}")) }.boxed()
    }
}

#[test]
fn approval_review_route_uses_last_non_continue_reviewer() {
    let handler_data = [
        route_data(Some(ApprovalReviewRouteDecision::AutoReview)),
        route_data(None),
        route_data(Some(ApprovalReviewRouteDecision::User)),
    ];

    assert_eq!(
        resolve_approval_review_route_decision(handler_data),
        Some(ApprovalReviewRouteDecision::User)
    );
}

#[test]
fn approval_review_route_returns_none_when_handlers_continue() {
    let handler_data = [route_data(None), route_data(None)];

    assert_eq!(resolve_approval_review_route_decision(handler_data), None);
}

#[test]
fn fail_closed_prompt_failure_forces_user_over_static_aggregation() {
    let handler_data = [
        ApprovalReviewRouteHandlerData {
            decision: None,
            force_user: true,
        },
        route_data(Some(ApprovalReviewRouteDecision::AutoReview)),
    ];

    assert_eq!(
        resolve_approval_review_route_decision(handler_data),
        Some(ApprovalReviewRouteDecision::User)
    );
}

#[tokio::test]
async fn prompt_route_uses_command_dto_schema_and_static_decision_aggregation() {
    let requests = Arc::new(Mutex::new(Vec::new()));
    let runner = RecordingRunner {
        requests: requests.clone(),
        result: Ok(route_output("auto_review")),
    };
    let request = request();
    let expected_input = serde_json::to_string(&build_command_input(&request)).expect("input JSON");
    let expected_prompt = format!(
        "<untrusted-hook-event-json>\n{expected_input}\n</untrusted-hook-event-json>"
    );

    let outcome = run(
        &[prompt_handler(/*fail_closed*/ false)],
        &shell(),
        Some(&runner),
        request,
    )
    .await;

    assert_eq!(
        outcome.decision,
        Some(ApprovalReviewRouteDecision::AutoReview)
    );
    assert_eq!(outcome.hook_events.len(), 1);
    assert_eq!(outcome.hook_events[0].run.status, HookRunStatus::Completed);
    let requests = requests.lock().expect("request lock");
    let schemas = crate::engine::schema_loader::generated_hook_schemas();
    assert_eq!(
        requests.as_slice(),
        &[PromptHookRequest {
            rendered_prompt: expected_prompt,
            model: None,
            reasoning_effort: None,
            event_name: HookEventName::ApprovalReviewRoute,
            output_schema: schemas.approval_review_route_command_output.clone(),
        }]
    );
}

#[tokio::test]
async fn prompt_route_failure_is_failed_and_fails_open() {
    let runner = failing_runner();

    let outcome = run(
        &[prompt_handler(/*fail_closed*/ false)],
        &shell(),
        Some(&runner),
        request(),
    )
    .await;

    assert_eq!(outcome.decision, None);
    assert_eq!(outcome.hook_events.len(), 1);
    assert_eq!(outcome.hook_events[0].run.status, HookRunStatus::Failed);
}

#[tokio::test]
async fn prompt_route_failure_is_failed_and_fails_closed_to_user() {
    let runner = failing_runner();

    let outcome = run(
        &[prompt_handler(/*fail_closed*/ true)],
        &shell(),
        Some(&runner),
        request(),
    )
    .await;

    assert_eq!(outcome.decision, Some(ApprovalReviewRouteDecision::User));
    assert_eq!(outcome.hook_events.len(), 1);
    assert_eq!(outcome.hook_events[0].run.status, HookRunStatus::Failed);
}

#[tokio::test]
async fn empty_prompt_output_honors_failure_policy() {
    for (fail_closed, expected_decision) in [
        (false, None),
        (true, Some(ApprovalReviewRouteDecision::User)),
    ] {
        let runner = RecordingRunner {
            requests: Arc::new(Mutex::new(Vec::new())),
            result: Ok(String::new()),
        };

        let outcome = run(
            &[prompt_handler(fail_closed)],
            &shell(),
            Some(&runner),
            request(),
        )
        .await;

        assert_eq!(outcome.decision, expected_decision);
        assert_eq!(outcome.hook_events.len(), 1);
        assert_eq!(outcome.hook_events[0].run.status, HookRunStatus::Failed);
    }
}

#[tokio::test]
async fn non_json_prompt_output_honors_failure_policy() {
    for (fail_closed, expected_decision) in [
        (false, None),
        (true, Some(ApprovalReviewRouteDecision::User)),
    ] {
        let runner = RecordingRunner {
            requests: Arc::new(Mutex::new(Vec::new())),
            result: Ok("not json".to_string()),
        };

        let outcome = run(
            &[prompt_handler(fail_closed)],
            &shell(),
            Some(&runner),
            request(),
        )
        .await;

        assert_eq!(outcome.decision, expected_decision);
        assert_eq!(outcome.hook_events.len(), 1);
        assert_eq!(outcome.hook_events[0].run.status, HookRunStatus::Failed);
    }
}

#[test]
fn command_route_output_still_selects_reviewer() {
    let handler = command_handler();
    let parsed = parse_completed(
        &handler,
        command_result(route_output("user")),
        Some("turn-1".to_string()),
    );

    assert_eq!(
        parsed.data,
        ApprovalReviewRouteHandlerData {
            decision: Some(ApprovalReviewRouteDecision::User),
            force_user: false,
        }
    );
    assert_eq!(parsed.completed.run.status, HookRunStatus::Completed);
}

#[test]
fn command_route_failure_still_returns_no_decision() {
    let handler = command_handler();
    let mut result = command_result(String::new());
    result.exit_code = Some(1);

    let parsed = parse_completed(&handler, result, Some("turn-1".to_string()));

    assert_eq!(parsed.data, ApprovalReviewRouteHandlerData::default());
    assert_eq!(parsed.completed.run.status, HookRunStatus::Failed);
}

#[test]
fn command_route_empty_and_non_json_output_remain_completed() {
    for stdout in [String::new(), "not json".to_string()] {
        let parsed = parse_completed(
            &command_handler(),
            command_result(stdout),
            Some("turn-1".to_string()),
        );

        assert_eq!(parsed.data, ApprovalReviewRouteHandlerData::default());
        assert_eq!(parsed.completed.run.status, HookRunStatus::Completed);
    }
}

fn request() -> ApprovalReviewRouteRequest {
    ApprovalReviewRouteRequest {
        session_id: ThreadId::new(),
        turn_id: "turn-1".to_string(),
        subagent: None,
        cwd: std::env::current_dir().expect("current directory"),
        transcript_path: None,
        model: "gpt-test".to_string(),
        permission_mode: "default".to_string(),
        tool_name: "shell".to_string(),
        matcher_aliases: Vec::new(),
        run_id_suffix: "call-1".to_string(),
        tool_input: json!({"cmd": "echo test"}),
        approval_kind: "command".to_string(),
        approval_policy: "on-request".to_string(),
        strict_auto_review: false,
        static_auto_review_enabled: true,
        retry_reason: None,
    }
}

fn prompt_handler(fail_closed: bool) -> ConfiguredHandler {
    handler(ConfiguredHandlerKind::Prompt {
        prompt: "$$ARGUMENTS".to_string(),
        model: None,
        reasoning_effort: None,
        timeout_sec: 30,
        fail_closed,
    })
}

fn command_handler() -> ConfiguredHandler {
    handler(ConfiguredHandlerKind::Command {
        command: "echo hook".to_string(),
        timeout_sec: 5,
    })
}

fn handler(kind: ConfiguredHandlerKind) -> ConfiguredHandler {
    ConfiguredHandler {
        event_name: HookEventName::ApprovalReviewRoute,
        matcher: None,
        kind,
        status_message: None,
        source_path: codex_utils_absolute_path::AbsolutePathBuf::current_dir().expect("cwd"),
        source: HookSource::User,
        display_order: 0,
        env: std::collections::HashMap::new(),
    }
}

fn shell() -> CommandShell {
    CommandShell {
        program: "sh".to_string(),
        args: vec!["-c".to_string()],
    }
}

fn failing_runner() -> RecordingRunner {
    RecordingRunner {
        requests: Arc::new(Mutex::new(Vec::new())),
        result: Err("review failed".to_string()),
    }
}

fn route_output(reviewer: &str) -> String {
    json!({
        "hookSpecificOutput": {
            "hookEventName": "ApprovalReviewRoute",
            "reviewer": reviewer,
        }
    })
    .to_string()
}

fn command_result(stdout: String) -> CommandRunResult {
    CommandRunResult {
        started_at: 1,
        completed_at: 2,
        duration_ms: 1,
        exit_code: Some(0),
        stdout,
        stderr: String::new(),
        error: None,
    }
}

fn route_data(decision: Option<ApprovalReviewRouteDecision>) -> ApprovalReviewRouteHandlerData {
    ApprovalReviewRouteHandlerData {
        decision,
        force_user: false,
    }
}
