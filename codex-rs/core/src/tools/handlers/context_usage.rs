use crate::function_tool::FunctionCallError;
use crate::tools::context::ToolInvocation;
use crate::tools::context::ToolOutput;
use crate::tools::context::ToolPayload;
use crate::tools::context::boxed_tool_output;
use crate::tools::handlers::context_usage_spec::GET_CONTEXT_USAGE_TOOL_NAME;
use crate::tools::handlers::context_usage_spec::create_get_context_usage_tool;
use crate::tools::registry::CoreToolRuntime;
use crate::tools::registry::ToolExecutor;
use codex_protocol::models::FunctionCallOutputPayload;
use codex_protocol::models::ResponseInputItem;
use codex_protocol::protocol::TokenUsageInfo;
use codex_tools::ToolName;
use codex_tools::ToolSpec;
use serde::Serialize;
use serde_json::Value as JsonValue;

pub struct ContextUsageHandler;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum ContextUsageSource {
    TokenUsageInfo,
    ModelContextWindowOnly,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
struct ContextUsageSnapshot {
    usage_known: bool,
    model_context_window: Option<i64>,
    used_tokens: Option<i64>,
    remaining_tokens: Option<i64>,
    remaining_percent: Option<i64>,
    source: ContextUsageSource,
}

impl ContextUsageSnapshot {
    fn from_usage_info(
        usage_info: Option<TokenUsageInfo>,
        model_context_window: Option<i64>,
    ) -> Self {
        let Some(usage_info) = usage_info else {
            return Self {
                usage_known: false,
                model_context_window,
                used_tokens: None,
                remaining_tokens: None,
                remaining_percent: None,
                source: ContextUsageSource::ModelContextWindowOnly,
            };
        };

        let used_tokens = usage_info.last_token_usage.tokens_in_context_window();
        let remaining_tokens =
            model_context_window.map(|context_window| context_window.saturating_sub(used_tokens));
        let remaining_percent = model_context_window.map(|context_window| {
            usage_info
                .last_token_usage
                .percent_of_context_window_remaining(context_window)
        });

        Self {
            usage_known: true,
            model_context_window,
            used_tokens: Some(used_tokens),
            remaining_tokens,
            remaining_percent,
            source: ContextUsageSource::TokenUsageInfo,
        }
    }
}

struct ContextUsageOutput {
    snapshot: ContextUsageSnapshot,
    text: String,
}

impl ContextUsageOutput {
    fn new(snapshot: ContextUsageSnapshot) -> Result<Self, FunctionCallError> {
        let text = serde_json::to_string(&snapshot).map_err(|err| {
            FunctionCallError::Fatal(format!(
                "failed to serialize {GET_CONTEXT_USAGE_TOOL_NAME} response: {err}"
            ))
        })?;
        Ok(Self { snapshot, text })
    }
}

impl ToolOutput for ContextUsageOutput {
    fn log_preview(&self) -> String {
        self.text.clone()
    }

    fn success_for_logging(&self) -> bool {
        true
    }

    fn to_response_item(&self, call_id: &str, _payload: &ToolPayload) -> ResponseInputItem {
        let mut output = FunctionCallOutputPayload::from_text(self.text.clone());
        output.success = Some(true);

        ResponseInputItem::FunctionCallOutput {
            call_id: call_id.to_string(),
            output,
        }
    }

    fn code_mode_result(&self, _payload: &ToolPayload) -> JsonValue {
        serde_json::to_value(&self.snapshot).unwrap_or_else(|err| {
            JsonValue::String(format!(
                "failed to serialize {GET_CONTEXT_USAGE_TOOL_NAME} response: {err}"
            ))
        })
    }
}

impl ToolExecutor<ToolInvocation> for ContextUsageHandler {
    fn tool_name(&self) -> ToolName {
        ToolName::plain(GET_CONTEXT_USAGE_TOOL_NAME)
    }

    fn spec(&self) -> ToolSpec {
        create_get_context_usage_tool()
    }

    fn handle(&self, invocation: ToolInvocation) -> codex_tools::ToolExecutorFuture<'_> {
        Box::pin(self.handle_call(invocation))
    }
}

impl ContextUsageHandler {
    async fn handle_call(
        &self,
        invocation: ToolInvocation,
    ) -> Result<Box<dyn crate::tools::context::ToolOutput>, FunctionCallError> {
        let ToolInvocation {
            session,
            turn,
            payload,
            ..
        } = invocation;

        if !matches!(payload, ToolPayload::Function { .. }) {
            return Err(FunctionCallError::RespondToModel(format!(
                "{GET_CONTEXT_USAGE_TOOL_NAME} handler received unsupported payload"
            )));
        }

        let usage_info = session.token_usage_info().await;
        let snapshot =
            ContextUsageSnapshot::from_usage_info(usage_info, turn.model_context_window());
        Ok(boxed_tool_output(ContextUsageOutput::new(snapshot)?))
    }
}

impl CoreToolRuntime for ContextUsageHandler {}

#[cfg(test)]
#[path = "context_usage_tests.rs"]
mod tests;
