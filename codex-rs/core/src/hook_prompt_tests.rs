use std::sync::Arc;
use std::time::Duration;

use codex_hooks::PromptHookRequest;
use codex_protocol::models::ContentItem;
use codex_protocol::models::ResponseItem;
use codex_protocol::protocol::HookEventName;
use pretty_assertions::assert_eq;
use serde_json::json;
use tokio::sync::Semaphore;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use super::PROMPT_HOOK_MAX_OUTPUT_BYTES;
use super::PromptHookEvaluationError;
use super::acquire_prompt_hook_permit;
use super::collect_final_json;
use super::normalize_strict_schema;
use super::prompt_for_request;
use crate::client_common::ResponseEvent;
use crate::client_common::ResponseStream;

const VALID_OUTPUT: &str = r#"{"continue":true}"#;
const FENCED_OUTPUT: &str = "```json\n{\"continue\":true}\n```";

#[test]
fn strict_schema_intersects_constant_and_enum_constraints() {
    let mut compatible = json!({"const": "PreToolUse", "enum": ["PreToolUse", "Stop"]});
    normalize_strict_schema(&mut compatible);
    assert_eq!(compatible, json!({"enum": ["PreToolUse"]}));

    let mut incompatible = json!({"const": "PreToolUse", "enum": ["Stop"]});
    normalize_strict_schema(&mut incompatible);
    assert_eq!(incompatible, json!({"enum": []}));
}

#[test]
fn prompt_request_uses_strict_schema_without_titles_constants_or_tools() {
    let output_schema = json!({
        "title": "PromptHookOutput",
        "type": "object",
        "properties": {
            "continue": {
                "default": true,
                "title": "Continue",
                "type": "boolean"
            },
            "hookEventName": {"const": "PreToolUse"},
            "reason": {"default": null, "type": "string"}
        },
        "required": ["hookEventName"]
    });
    let prompt = prompt_for_request(PromptHookRequest {
        rendered_prompt: "evaluate this input".to_string(),
        model: None,
        reasoning_effort: None,
        event_name: HookEventName::PreToolUse,
        output_schema: output_schema.clone(),
    });

    assert_eq!(
        (
            prompt.tools,
            prompt.parallel_tool_calls,
            prompt.output_schema,
            prompt.output_schema_strict,
        ),
        (
            Vec::new(),
            false,
            Some(json!({
                "type": "object",
                "properties": {
                    "continue": {"type": "boolean"},
                    "hookEventName": {"enum": ["PreToolUse"]},
                    "reason": {
                        "anyOf": [
                            {"type": "string"},
                            {"type": "null"}
                        ]
                    }
                },
                "required": ["continue", "hookEventName", "reason"],
                "additionalProperties": false
            })),
            true,
        )
    );
}

#[test]
fn strict_schema_validator_rejects_empty_all_of_and_unsupported_keywords() {
    for invalid_schema in [
        json!({}),
        json!({"allOf": [{"type": "string"}]}),
        json!({"type": "string", "minLength": 1}),
    ] {
        assert!(
            std::panic::catch_unwind(|| assert_strict_schema(&invalid_schema)).is_err(),
            "schema should be rejected: {invalid_schema}"
        );
    }
}

#[tokio::test]
async fn final_output_requires_one_completed_json_value() {
    let mut valid_stream = response_stream(vec![
        ResponseEvent::OutputItemDone(output_message(VALID_OUTPUT)),
        completed_event(),
    ]);
    assert_eq!(
        collect_final_json(&mut valid_stream).await,
        Ok(VALID_OUTPUT.to_string())
    );

    let mut multiple_stream = response_stream(vec![
        ResponseEvent::OutputItemDone(output_message(VALID_OUTPUT)),
        ResponseEvent::OutputItemDone(output_message(VALID_OUTPUT)),
        completed_event(),
    ]);
    assert_eq!(
        collect_final_json(&mut multiple_stream).await,
        Err(PromptHookEvaluationError::InvalidOutput)
    );

    let mut fenced_stream = response_stream(vec![
        ResponseEvent::OutputItemDone(output_message(FENCED_OUTPUT)),
        completed_event(),
    ]);
    assert_eq!(
        collect_final_json(&mut fenced_stream).await,
        Err(PromptHookEvaluationError::InvalidOutput)
    );

    let mut tool_call_stream = response_stream(vec![ResponseEvent::ToolCallInputDelta {
        item_id: "item-id".to_string(),
        call_id: Some("call-id".to_string()),
        delta: "{}".to_string(),
    }]);
    assert_eq!(
        collect_final_json(&mut tool_call_stream).await,
        Err(PromptHookEvaluationError::ToolCall)
    );
}

#[tokio::test]
async fn response_output_is_capped_before_accumulation() {
    let mut stream = response_stream(vec![ResponseEvent::OutputTextDelta(
        "x".repeat(PROMPT_HOOK_MAX_OUTPUT_BYTES + 1),
    )]);

    assert_eq!(
        collect_final_json(&mut stream).await,
        Err(PromptHookEvaluationError::OutputTooLarge)
    );
}

#[tokio::test]
async fn injected_limiter_releases_capacity_when_permit_drops() {
    let limiter = Arc::new(Semaphore::new(1));
    let first = acquire_prompt_hook_permit(&limiter)
        .await
        .expect("first evaluator should acquire the injected limiter");
    let second = acquire_prompt_hook_permit(&limiter);
    tokio::pin!(second);

    assert!(
        tokio::time::timeout(Duration::from_millis(10), &mut second)
            .await
            .is_err()
    );
    drop(first);
    assert!(
        tokio::time::timeout(Duration::from_secs(1), second)
            .await
            .expect("second evaluator should resume after the permit is dropped")
            .is_ok()
    );
}

fn response_stream(events: Vec<ResponseEvent>) -> ResponseStream {
    let (tx, rx_event) = mpsc::channel(events.len().max(1));
    for event in events {
        tx.try_send(Ok(event))
            .expect("test stream should have capacity");
    }
    drop(tx);
    ResponseStream {
        rx_event,
        consumer_dropped: CancellationToken::new(),
    }
}

fn output_message(text: &str) -> ResponseItem {
    ResponseItem::Message {
        id: None,
        role: "assistant".to_string(),
        content: vec![ContentItem::OutputText {
            text: text.to_string(),
        }],
        phase: None,
        internal_chat_message_metadata_passthrough: None,
    }
}

fn completed_event() -> ResponseEvent {
    ResponseEvent::Completed {
        response_id: "response-id".to_string(),
        token_usage: None,
        end_turn: Some(true),
    }
}

fn assert_strict_schema(schema: &serde_json::Value) {
    let schema = schema
        .as_object()
        .expect("strict schema nodes should be objects");
    assert!(!schema.is_empty(), "strict schema nodes must not be empty");
    const SUPPORTED_KEYWORDS: &[&str] = &[
        "$ref",
        "$defs",
        "additionalProperties",
        "anyOf",
        "description",
        "enum",
        "items",
        "properties",
        "required",
        "type",
    ];
    for key in schema.keys() {
        assert!(
            SUPPORTED_KEYWORDS.contains(&key.as_str()),
            "unsupported strict schema keyword {key} in {schema:?}"
        );
    }
    if let Some(reference) = schema.get("$ref").and_then(serde_json::Value::as_str) {
        assert!(
            reference.starts_with("#/$defs/"),
            "strict schema reference should use the $defs dialect: {reference}"
        );
    }
    assert!(
        ["type", "$ref", "anyOf", "enum"]
            .into_iter()
            .any(|key| schema.contains_key(key)),
        "strict schema node has no supported value constraint: {schema:?}"
    );
    if let Some(properties) = schema
        .get("properties")
        .and_then(serde_json::Value::as_object)
    {
        assert_eq!(schema.get("additionalProperties"), Some(&json!(false)));
        let required = schema
            .get("required")
            .and_then(serde_json::Value::as_array)
            .expect("strict object schema should declare required properties");
        assert_eq!(
            required,
            &properties
                .keys()
                .cloned()
                .map(serde_json::Value::String)
                .collect::<Vec<_>>()
        );
        for property in properties.values() {
            assert_strict_schema(property);
        }
    }
    if let Some(definitions) = schema.get("$defs").and_then(serde_json::Value::as_object) {
        for definition in definitions.values() {
            assert_strict_schema(definition);
        }
    }
    if let Some(variants) = schema.get("anyOf").and_then(serde_json::Value::as_array) {
        for variant in variants {
            assert_strict_schema(variant);
        }
    }
    if let Some(items) = schema.get("items") {
        assert_strict_schema(items);
    }
}
