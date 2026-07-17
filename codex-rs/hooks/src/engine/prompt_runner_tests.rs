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

    assert_eq!(
        render_prompt("Before $$ARGUMENTS after $$ARGUMENTS", event_json),
        Ok(format!("Before {event_json} after {event_json}"))
    );
}

#[test]
fn prompt_rendering_appends_fixed_untrusted_envelope() {
    assert_eq!(
        render_prompt("Review the event.", "{}"),
        Ok(
            "Review the event.\n\n<untrusted-hook-event-json>\n{}\n</untrusted-hook-event-json>"
                .to_string()
        )
    );
}

#[test]
fn prompt_rendering_rejects_oversized_event_and_total_input() {
    assert!(render_prompt("$$ARGUMENTS", &"x".repeat(PROMPT_HOOK_EVENT_JSON_LIMIT + 1)).is_err());
    assert!(render_prompt(&"word ".repeat(40_000), "{}").is_err());
}

#[tokio::test]
async fn prompt_runner_receives_raw_override_event_and_pre_tool_schema() {
    let requests = Arc::new(Mutex::new(Vec::new()));
    let runner = RecordingRunner {
        requests: requests.clone(),
        output: "{}".to_string(),
    };
    let handler = prompt_handler(/*fail_closed*/ false);

    let result = run_prompt(Some(&runner), &handler, "{}").await;

    assert_eq!(result.exit_code, Some(0));
    assert_eq!(result.stdout, "{}");
    let requests = requests.lock().expect("request lock");
    assert_eq!(
        requests.as_slice(),
        &[PromptHookRequest {
            rendered_prompt: "Review {}".to_string(),
            model: Some("gpt-override".to_string()),
            reasoning_effort: Some(ReasoningEffort::High),
            event_name: HookEventName::PreToolUse,
            output_schema: super::super::schema_loader::generated_hook_schemas()
                .pre_tool_use_command_output
                .clone(),
        }]
    );
}

fn prompt_handler(fail_closed: bool) -> ConfiguredHandler {
    ConfiguredHandler {
        event_name: HookEventName::PreToolUse,
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
