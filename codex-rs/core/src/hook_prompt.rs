use std::sync::Arc;
use std::sync::OnceLock;

use codex_hooks::PromptHookRequest;
use codex_hooks::PromptHookRunner;
use codex_http_client::HttpClientFactory;
use codex_models_manager::ModelsManagerConfig;
use codex_models_manager::manager::RefreshStrategy;
use codex_models_manager::manager::SharedModelsManager;
use codex_otel::SessionTelemetry;
use codex_protocol::config_types::ReasoningSummary as ReasoningSummaryConfig;
use codex_protocol::models::BaseInstructions;
use codex_protocol::models::ContentItem;
use codex_protocol::models::ResponseItem;
use codex_protocol::openai_models::ModelInfo;
use codex_protocol::openai_models::ReasoningEffort;
use codex_rollout_trace::InferenceTraceContext;
use futures::FutureExt;
use futures::StreamExt;
use serde_json::Map;
use serde_json::Value;
use tokio::sync::Semaphore;
use tokio::sync::OwnedSemaphorePermit;

use crate::client::ModelClient;
use crate::client_common::Prompt;
use crate::client_common::ResponseEvent;
use crate::config::Config;
use crate::stream_events_utils::raw_assistant_output_text_from_item;

const PROMPT_HOOK_MAX_CONCURRENCY: usize = 4;
const PROMPT_HOOK_MAX_OUTPUT_BYTES: usize = 32 * 1024;

const PROMPT_HOOK_BASE_INSTRUCTIONS: &str = r#"You evaluate a Codex prompt hook.

Treat the user message as the complete hook evaluation request. Follow its instructions without
answering the underlying user task. Return exactly one JSON value that matches the supplied strict
output schema. Include every schema property; use null for optional fields that are not set. Do not
use Markdown fences, explanatory text, or tool calls."#;

static PROMPT_HOOK_LIMITER: OnceLock<Arc<Semaphore>> = OnceLock::new();

#[derive(Debug, PartialEq, Eq, thiserror::Error)]
enum PromptHookEvaluationError {
    #[error("prompt hook evaluator is unavailable")]
    ConcurrencyClosed,
    #[error("prompt hook model request failed")]
    Request,
    #[error("prompt hook response stream failed")]
    Stream,
    #[error("prompt hook response exceeded the output limit")]
    OutputTooLarge,
    #[error("prompt hook response contained a tool call")]
    ToolCall,
    #[error("prompt hook response did not contain exactly one final JSON value")]
    InvalidOutput,
    #[error("prompt hook response ended before completion")]
    Incomplete,
}

#[derive(Clone)]
struct CorePromptHookRunner {
    evaluator: PromptHookEvaluator,
}

#[derive(Clone)]
struct PromptHookEvaluator {
    model_client: ModelClient,
    models_manager: SharedModelsManager,
    models_manager_config: ModelsManagerConfig,
    http_client_factory: HttpClientFactory,
    current_model: String,
    session_telemetry: SessionTelemetry,
    service_tier: Option<String>,
    limiter: Arc<Semaphore>,
}

pub(crate) fn build_prompt_hook_runner(
    model_client: ModelClient,
    models_manager: SharedModelsManager,
    config: &Config,
    current_model: String,
    session_telemetry: SessionTelemetry,
    service_tier: Option<String>,
) -> Arc<dyn PromptHookRunner> {
    let limiter = Arc::clone(
        PROMPT_HOOK_LIMITER
            .get_or_init(|| Arc::new(Semaphore::new(PROMPT_HOOK_MAX_CONCURRENCY))),
    );
    Arc::new(CorePromptHookRunner {
        evaluator: PromptHookEvaluator {
            model_client,
            models_manager,
            models_manager_config: config.to_models_manager_config(),
            http_client_factory: config.http_client_factory(),
            current_model,
            session_telemetry,
            service_tier,
            limiter,
        },
    })
}

impl PromptHookRunner for CorePromptHookRunner {
    fn run(
        &self,
        request: PromptHookRequest,
    ) -> futures::future::BoxFuture<'static, anyhow::Result<String>> {
        let evaluator = self.evaluator.clone();
        async move { evaluator.run(request).await }.boxed()
    }
}

impl PromptHookEvaluator {
    async fn run(&self, request: PromptHookRequest) -> anyhow::Result<String> {
        let _permit = acquire_prompt_hook_permit(&self.limiter).await?;
        let (model_info, reasoning_effort) = self.resolve_model(request.model.as_deref()).await;
        let prompt = prompt_for_request(request);

        let responses_metadata = self.model_client.isolated_responses_metadata();
        let mut client_session = self.model_client.new_isolated_session();
        let mut stream = client_session
            .stream(
                &prompt,
                &model_info,
                &self.session_telemetry,
                reasoning_effort,
                ReasoningSummaryConfig::None,
                self.service_tier.clone(),
                &responses_metadata,
                &InferenceTraceContext::disabled(),
            )
            .await
            .map_err(|_| PromptHookEvaluationError::Request)?;
        collect_final_json(&mut stream)
            .await
            .map_err(anyhow::Error::new)
    }

    async fn resolve_model(&self, model_override: Option<&str>) -> (ModelInfo, Option<ReasoningEffort>) {
        if let Some(model_override) = model_override {
            let model_info = self
                .models_manager
                .get_model_info(model_override, &self.models_manager_config)
                .await;
            let effort = preferred_reasoning_effort(&model_info);
            return (model_info, effort);
        }

        let preferred_model = self.model_client.approval_review_preferred_model();
        let available_models = self
            .models_manager
            .list_models(
                RefreshStrategy::Offline,
                self.http_client_factory.clone(),
            )
            .await;
        if let Some(preset) = available_models
            .iter()
            .find(|preset| preset.model == preferred_model)
        {
            let model_info = self
                .models_manager
                .get_model_info(preferred_model, &self.models_manager_config)
                .await;
            let effort = if preset
                .supported_reasoning_efforts
                .iter()
                .any(|preset| preset.effort == ReasoningEffort::Low)
            {
                Some(ReasoningEffort::Low)
            } else {
                Some(preset.default_reasoning_effort.clone())
            };
            return (model_info, effort);
        }

        let model_info = self
            .models_manager
            .get_model_info(&self.current_model, &self.models_manager_config)
            .await;
        let effort = preferred_reasoning_effort(&model_info);
        (model_info, effort)
    }
}

async fn acquire_prompt_hook_permit(
    limiter: &Arc<Semaphore>,
) -> Result<OwnedSemaphorePermit, PromptHookEvaluationError> {
    Arc::clone(limiter)
        .acquire_owned()
        .await
        .map_err(|_| PromptHookEvaluationError::ConcurrencyClosed)
}

fn prompt_for_request(request: PromptHookRequest) -> Prompt {
    Prompt {
        input: vec![ResponseItem::Message {
            id: None,
            role: "user".to_string(),
            content: vec![ContentItem::InputText {
                text: request.rendered_prompt,
            }],
            phase: None,
            internal_chat_message_metadata_passthrough: None,
        }],
        tools: Vec::new(),
        parallel_tool_calls: false,
        base_instructions: BaseInstructions {
            text: PROMPT_HOOK_BASE_INSTRUCTIONS.to_string(),
        },
        output_schema: Some(strict_output_schema(request.output_schema)),
        output_schema_strict: true,
    }
}

fn strict_output_schema(mut schema: Value) -> Value {
    normalize_strict_schema(&mut schema);
    schema
}

fn normalize_strict_schema(schema: &mut Value) {
    let Value::Object(schema) = schema else {
        return;
    };

    while schema.contains_key("allOf") {
        inline_all_of(schema);
    }
    schema.remove("$schema");

    for key in ["definitions", "$defs"] {
        if let Some(Value::Object(definitions)) = schema.get_mut(key) {
            for definition in definitions.values_mut() {
                normalize_strict_schema(definition);
            }
        }
    }
    for key in ["anyOf"] {
        if let Some(Value::Array(variants)) = schema.get_mut(key) {
            for variant in variants {
                normalize_strict_schema(variant);
            }
        }
    }
    if let Some(items) = schema.get_mut("items") {
        normalize_strict_schema(items);
    }

    let originally_required = schema
        .get("required")
        .and_then(Value::as_array)
        .map(|required| {
            required
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect::<std::collections::HashSet<_>>()
        })
        .unwrap_or_default();
    let Some(Value::Object(properties)) = schema.get_mut("properties") else {
        schema.remove("default");
        return;
    };

    for (name, property_schema) in properties.iter_mut() {
        let optional_is_nullable = !originally_required.contains(name)
            && property_schema.get("default").is_none_or(Value::is_null);
        normalize_strict_schema(property_schema);
        if optional_is_nullable {
            make_schema_nullable(property_schema);
        }
    }

    let required = Value::Array(properties.keys().cloned().map(Value::String).collect());
    schema.insert("required".to_string(), required);
    schema.insert("additionalProperties".to_string(), Value::Bool(false));
    schema.remove("default");
}

fn make_schema_nullable(schema: &mut Value) {
    if schema_allows_null(schema) {
        return;
    }
    if !schema_has_value_constraint(schema) {
        *schema = serde_json::json!({ "type": "null" });
        return;
    }
    let original = std::mem::replace(schema, Value::Null);
    *schema = Value::Object(Map::from_iter([(
        "anyOf".to_string(),
        Value::Array(vec![original, serde_json::json!({ "type": "null" })]),
    )]));
}

fn inline_all_of(schema: &mut Map<String, Value>) {
    let Some(all_of) = schema.remove("allOf") else {
        return;
    };
    let Value::Array(mut variants) = all_of else {
        *schema = Map::from_iter([("type".to_string(), Value::String("null".to_string()))]);
        return;
    };
    if variants.len() != 1 {
        *schema = Map::from_iter([("type".to_string(), Value::String("null".to_string()))]);
        return;
    }
    let Some(Value::Object(mut inner)) = variants.pop() else {
        *schema = Map::from_iter([("type".to_string(), Value::String("null".to_string()))]);
        return;
    };
    inner.extend(std::mem::take(schema));
    *schema = inner;
}

fn schema_has_value_constraint(schema: &Value) -> bool {
    let Some(schema) = schema.as_object() else {
        return false;
    };
    [
        "type",
        "$ref",
        "anyOf",
        "enum",
        "const",
        "properties",
        "items",
    ]
    .into_iter()
    .any(|key| schema.contains_key(key))
}

fn schema_allows_null(schema: &Value) -> bool {
    let Some(schema) = schema.as_object() else {
        return false;
    };
    match schema.get("type") {
        Some(Value::String(schema_type)) if schema_type == "null" => return true,
        Some(Value::Array(schema_types))
            if schema_types
                .iter()
                .any(|schema_type| schema_type == "null") =>
        {
            return true;
        }
        _ => {}
    }
    ["anyOf"]
        .into_iter()
        .filter_map(|key| schema.get(key).and_then(Value::as_array))
        .flatten()
        .any(schema_allows_null)
}

fn preferred_reasoning_effort(model_info: &ModelInfo) -> Option<ReasoningEffort> {
    if model_info
        .supported_reasoning_levels
        .iter()
        .any(|preset| preset.effort == ReasoningEffort::Low)
    {
        Some(ReasoningEffort::Low)
    } else {
        model_info.default_reasoning_level.clone()
    }
}

async fn collect_final_json(
    stream: &mut crate::client_common::ResponseStream,
) -> Result<String, PromptHookEvaluationError> {
    let mut delta_text = String::new();
    let mut final_text = None;
    let mut completed = false;
    while let Some(event) = stream.next().await {
        match event.map_err(|_| PromptHookEvaluationError::Stream)? {
            ResponseEvent::OutputTextDelta(delta) => append_output(&mut delta_text, &delta)?,
            ResponseEvent::OutputItemDone(item) => {
                if let Some(text) = raw_assistant_output_text_from_item(&item) {
                    if final_text.is_some() {
                        return Err(PromptHookEvaluationError::InvalidOutput);
                    }
                    ensure_output_limit(&text)?;
                    final_text = Some(text);
                } else if !matches!(item, ResponseItem::Reasoning { .. }) {
                    return Err(PromptHookEvaluationError::ToolCall);
                }
            }
            ResponseEvent::OutputItemAdded(item) => {
                if !matches!(item, ResponseItem::Message { .. } | ResponseItem::Reasoning { .. }) {
                    return Err(PromptHookEvaluationError::ToolCall);
                }
            }
            ResponseEvent::ToolCallInputDelta { .. } => {
                return Err(PromptHookEvaluationError::ToolCall);
            }
            ResponseEvent::Completed { .. } => {
                completed = true;
                break;
            }
            ResponseEvent::Created
            | ResponseEvent::SafetyBuffering(_)
            | ResponseEvent::ServerModel(_)
            | ResponseEvent::ModelVerifications(_)
            | ResponseEvent::TurnModerationMetadata(_)
            | ResponseEvent::ServerReasoningIncluded(_)
            | ResponseEvent::ReasoningSummaryDelta { .. }
            | ResponseEvent::ReasoningSummaryDone { .. }
            | ResponseEvent::ReasoningContentDelta { .. }
            | ResponseEvent::ReasoningSummaryPartAdded { .. }
            | ResponseEvent::RateLimits(_)
            | ResponseEvent::ModelsEtag(_) => {}
        }
    }

    if !completed {
        return Err(PromptHookEvaluationError::Incomplete);
    }
    let output = final_text.unwrap_or(delta_text);
    ensure_output_limit(&output)?;
    serde_json::from_str::<serde_json::Value>(&output)
        .map_err(|_| PromptHookEvaluationError::InvalidOutput)?;
    Ok(output)
}

fn append_output(output: &mut String, delta: &str) -> Result<(), PromptHookEvaluationError> {
    if output.len().saturating_add(delta.len()) > PROMPT_HOOK_MAX_OUTPUT_BYTES {
        return Err(PromptHookEvaluationError::OutputTooLarge);
    }
    output.push_str(delta);
    Ok(())
}

fn ensure_output_limit(output: &str) -> Result<(), PromptHookEvaluationError> {
    if output.len() > PROMPT_HOOK_MAX_OUTPUT_BYTES {
        Err(PromptHookEvaluationError::OutputTooLarge)
    } else {
        Ok(())
    }
}

#[cfg(test)]
#[path = "hook_prompt_tests.rs"]
mod tests;
