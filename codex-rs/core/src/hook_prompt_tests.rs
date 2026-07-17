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
use super::prompt_for_request;
use crate::client_common::ResponseEvent;
use crate::client_common::ResponseStream;

const VALID_OUTPUT: &str = r#"{"continue":true}"#;
const FENCED_OUTPUT: &str = "```json\n{\"continue\":true}\n```";

#[test]
fn prompt_request_uses_strict_schema_without_titles_or_tools() {
    let output_schema = json!({
        "title": "PromptHookOutput",
        "type": "object",
        "properties": {
            "continue": {
                "default": true,
                "title": "Continue",
                "type": "boolean"
            },
            "reason": {"default": null, "type": "string"}
        }
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
                    "reason": {
                        "anyOf": [
                            {"type": "string"},
                            {"type": "null"}
                        ]
                    }
                },
                "required": ["continue", "reason"],
                "additionalProperties": false
            })),
            true,
        )
    );
}

#[test]
fn pre_tool_use_schema_is_strict_and_accepts_safe_and_deny_outputs() {
    let schema = prompt_for_request(PromptHookRequest {
        rendered_prompt: "evaluate this input".to_string(),
        model: None,
        reasoning_effort: None,
        event_name: HookEventName::PreToolUse,
        output_schema: pre_tool_use_output_schema(),
    })
    .output_schema
    .expect("prompt request should include an output schema");

    assert_strict_schema(&schema);
    assert_eq!(schema.get("$schema"), None);
    assert!(schema.get("$defs").is_some());
    assert_eq!(schema.get("definitions"), None);
    assert_eq!(
        schema.pointer("/$defs/PreToolUseHookSpecificOutputWire/properties/updatedInput"),
        Some(&json!({"type": "null"}))
    );
    assert_schema_accepts(
        &schema,
        &json!({
            "continue": true,
            "decision": null,
            "hookSpecificOutput": null,
            "reason": null,
            "stopReason": null,
            "suppressOutput": false,
            "systemMessage": null
        }),
        &schema,
    );
    assert_schema_accepts(
        &schema,
        &json!({
            "continue": true,
            "decision": null,
            "hookSpecificOutput": {
                "additionalContext": null,
                "hookEventName": "PreToolUse",
                "permissionDecision": "deny",
                "permissionDecisionReason": "do not run that",
                "updatedInput": null
            },
            "reason": null,
            "stopReason": null,
            "suppressOutput": false,
            "systemMessage": null
        }),
        &schema,
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

fn pre_tool_use_output_schema() -> serde_json::Value {
    json!({
        "$schema": "http://json-schema.org/draft-07/schema#",
        "definitions": {
            "PreToolUseDecisionWire": {
                "enum": ["approve", "block"],
                "type": "string"
            },
            "PreToolUseHookSpecificOutputWire": {
                "additionalProperties": false,
                "properties": {
                    "additionalContext": {"default": null, "type": "string"},
                    "hookEventName": {"const": "PreToolUse", "type": "string"},
                    "permissionDecision": {
                        "allOf": [{"$ref": "#/definitions/PreToolUsePermissionDecisionWire"}],
                        "default": null
                    },
                    "permissionDecisionReason": {"default": null, "type": "string"},
                    "updatedInput": {"default": null}
                },
                "required": ["hookEventName"],
                "type": "object"
            },
            "PreToolUsePermissionDecisionWire": {
                "enum": ["allow", "deny", "ask"],
                "type": "string"
            }
        },
        "properties": {
            "continue": {"default": true, "type": "boolean"},
            "decision": {
                "allOf": [{"$ref": "#/definitions/PreToolUseDecisionWire"}],
                "default": null
            },
            "hookSpecificOutput": {
                "allOf": [{"$ref": "#/definitions/PreToolUseHookSpecificOutputWire"}],
                "default": null
            },
            "reason": {"default": null, "type": "string"},
            "stopReason": {"default": null, "type": "string"},
            "suppressOutput": {"default": false, "type": "boolean"},
            "systemMessage": {"default": null, "type": "string"}
        },
        "type": "object"
    })
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
        "const",
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
        ["type", "$ref", "anyOf", "enum", "const"]
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

fn assert_schema_accepts(
    schema: &serde_json::Value,
    value: &serde_json::Value,
    root: &serde_json::Value,
) {
    let schema = schema
        .as_object()
        .expect("test schema nodes should be objects");
    if let Some(reference) = schema.get("$ref").and_then(serde_json::Value::as_str) {
        let definition = reference
            .strip_prefix("#/$defs/")
            .and_then(|name| root.pointer(&format!("/$defs/{name}")))
            .expect("test schema reference should resolve");
        assert_schema_accepts(definition, value, root);
    }
    if let Some(variants) = schema.get("anyOf").and_then(serde_json::Value::as_array) {
        assert!(
            variants
                .iter()
                .any(|variant| schema_accepts(variant, value, root)),
            "value {value} should match an anyOf variant in {schema:?}"
        );
    }
    if let Some(variants) = schema.get("allOf").and_then(serde_json::Value::as_array) {
        for variant in variants {
            assert_schema_accepts(variant, value, root);
        }
    }
    if let Some(expected) = schema.get("const") {
        assert_eq!(value, expected);
    }
    if let Some(allowed) = schema.get("enum").and_then(serde_json::Value::as_array) {
        assert!(allowed.contains(value));
    }
    match schema.get("type").and_then(serde_json::Value::as_str) {
        Some("null") => assert!(value.is_null()),
        Some("boolean") => assert!(value.is_boolean()),
        Some("string") => assert!(value.is_string()),
        Some("object") => {
            let object = value.as_object().expect("value should be an object");
            let properties = schema
                .get("properties")
                .and_then(serde_json::Value::as_object)
                .expect("object schema should declare properties");
            let required = schema
                .get("required")
                .and_then(serde_json::Value::as_array)
                .expect("object schema should declare required properties");
            for name in required.iter().filter_map(serde_json::Value::as_str) {
                assert!(
                    object.contains_key(name),
                    "missing required property {name}"
                );
            }
            for (name, child) in object {
                let property_schema = properties
                    .get(name)
                    .expect("additional properties should be rejected");
                assert_schema_accepts(property_schema, child, root);
            }
        }
        Some(schema_type) => panic!("unsupported test schema type {schema_type}"),
        None => {}
    }
}

fn schema_accepts(
    schema: &serde_json::Value,
    value: &serde_json::Value,
    root: &serde_json::Value,
) -> bool {
    std::panic::catch_unwind(|| assert_schema_accepts(schema, value, root)).is_ok()
}
