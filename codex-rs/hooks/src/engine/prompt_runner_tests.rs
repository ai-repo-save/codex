use std::sync::Arc;
use std::sync::Mutex;

use codex_protocol::openai_models::ReasoningEffort;
use futures::FutureExt;
use pretty_assertions::assert_eq;

use super::*;

#[derive(Clone)]
struct RecordingRunner {
    requests: Arc<Mutex<Vec<PromptHookRequest>>>,
    output: String,
}

impl PromptHookRunner for RecordingRunner {
    fn run(&self, request: PromptHookRequest) -> BoxFuture<'static, anyhow::Result<String>> {
        self.requests.lock().expect("request lock").push(request);
        let output = self.output.clone();
        async move { Ok(output) }.boxed()
    }
}

#[test]
fn prompt_rendering_replaces_every_arguments_placeholder() {
    let event_json = r#"{"hook_event_name":"PreToolUse"}"#;
    let untrusted_event_json =
        format!("<untrusted-hook-event-json>\n{event_json}\n</untrusted-hook-event-json>");

    assert_eq!(
        render_prompt("Before $$ARGUMENTS after $$ARGUMENTS", event_json),
        Ok(format!(
            "Before {untrusted_event_json} after {untrusted_event_json}"
        ))
    );
}

#[test]
fn prompt_rendering_appends_fixed_untrusted_envelope() {
    let untrusted_event_json = "<untrusted-hook-event-json>\n{}\n</untrusted-hook-event-json>";

    assert_eq!(
        render_prompt("Review the event.", "{}"),
        Ok(format!("Review the event.\n\n{untrusted_event_json}"))
    );
    assert_eq!(
        render_prompt("Review $$ARGUMENTS", "{}"),
        Ok(format!("Review {untrusted_event_json}"))
    );
}

#[test]
fn prompt_rendering_escapes_event_json_envelope_delimiters() {
    let closing_tag = UNTRUSTED_EVENT_JSON_SUFFIX.trim_start_matches('\n');
    let event = serde_json::json!({"value": closing_tag});
    let event_json = serde_json::to_string(&event).expect("serialize event");

    for prompt in [PROMPT_ARGUMENTS_PLACEHOLDER, "Review the event."] {
        let rendered = render_prompt(prompt, &event_json).expect("render prompt");

        assert_eq!(rendered.matches(closing_tag).count(), 1);
        let (_, enclosed) = rendered
            .split_once(UNTRUSTED_EVENT_JSON_PREFIX)
            .expect("untrusted event prefix");
        let (escaped_event_json, trailing) = enclosed
            .split_once(UNTRUSTED_EVENT_JSON_SUFFIX)
            .expect("untrusted event suffix");
        assert_eq!(trailing, "");
        let parsed_event = serde_json::from_str::<serde_json::Value>(escaped_event_json)
            .expect("parse escaped event JSON");
        assert_eq!(parsed_event, event);
    }
}

#[test]
fn prompt_rendering_rejects_oversized_event_and_total_input() {
    assert!(render_prompt("$$ARGUMENTS", &"x".repeat(PROMPT_HOOK_EVENT_JSON_LIMIT + 1)).is_err());
    assert!(render_prompt(&"word ".repeat(40_000), "{}").is_err());
}

#[tokio::test]
async fn prompt_runner_receives_raw_event_and_event_output_schema() {
    let requests = Arc::new(Mutex::new(Vec::new()));
    let runner = RecordingRunner {
        requests: requests.clone(),
        output: "{}".to_string(),
    };
    let schemas = super::super::schema_loader::generated_hook_schemas();
    for event_name in [
        HookEventName::PreToolUse,
        HookEventName::PermissionRequest,
        HookEventName::ApprovalReviewRoute,
        HookEventName::PreCompact,
        HookEventName::SessionStart,
        HookEventName::UserPromptSubmit,
        HookEventName::SubagentStart,
    ] {
        let handler = prompt_handler(event_name, /*fail_closed*/ false);

        let result = run_prompt(Some(&runner), &handler, "{}").await;

        assert_eq!(result.exit_code, Some(0));
        assert_eq!(result.stdout, "{}");
    }
    let requests = requests.lock().expect("request lock");
    assert_eq!(
        requests.as_slice(),
        &[
            prompt_request(
                HookEventName::PreToolUse,
                schemas.pre_tool_use_command_output.clone(),
            ),
            prompt_request(
                HookEventName::PermissionRequest,
                schemas.permission_request_command_output.clone(),
            ),
            prompt_request(
                HookEventName::ApprovalReviewRoute,
                schemas.approval_review_route_command_output.clone(),
            ),
            prompt_request(
                HookEventName::PreCompact,
                schemas.pre_compact_command_output.clone(),
            ),
            prompt_request(
                HookEventName::SessionStart,
                schemas.session_start_command_output.clone(),
            ),
            prompt_request(
                HookEventName::UserPromptSubmit,
                schemas.user_prompt_submit_command_output.clone(),
            ),
            prompt_request(
                HookEventName::SubagentStart,
                schemas.subagent_start_command_output.clone(),
            ),
        ]
    );
}

fn prompt_request(event_name: HookEventName, output_schema: Value) -> PromptHookRequest {
    PromptHookRequest {
        rendered_prompt: "Review <untrusted-hook-event-json>\n{}\n</untrusted-hook-event-json>"
            .to_string(),
        model: Some("gpt-override".to_string()),
        reasoning_effort: Some(ReasoningEffort::High),
        event_name,
        output_schema,
    }
}

fn prompt_handler(event_name: HookEventName, fail_closed: bool) -> ConfiguredHandler {
    ConfiguredHandler {
        event_name,
        matcher: None,
        kind: ConfiguredHandlerKind::Prompt {
            prompt: "Review $$ARGUMENTS".to_string(),
            model: Some("gpt-override".to_string()),
            reasoning_effort: Some(ReasoningEffort::High),
            timeout_sec: 30,
            fail_closed,
        },
        status_message: None,
        source_path: codex_utils_absolute_path::AbsolutePathBuf::current_dir().expect("cwd"),
        source: codex_protocol::protocol::HookSource::User,
        display_order: 0,
        env: std::collections::HashMap::new(),
    }
}
