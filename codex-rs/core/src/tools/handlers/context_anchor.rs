use crate::function_tool::FunctionCallError;
use crate::tools::context::ToolInvocation;
use crate::tools::context::ToolOutput;
use crate::tools::context::ToolPayload;
use crate::tools::context::boxed_tool_output;
use crate::tools::handlers::context_anchor_spec::REWIND_CONTEXT_TO_ANCHOR_TOOL_NAME;
use crate::tools::handlers::context_anchor_spec::SAVE_CONTEXT_ANCHOR_TOOL_NAME;
use crate::tools::handlers::context_anchor_spec::create_rewind_context_to_anchor_tool;
use crate::tools::handlers::context_anchor_spec::create_save_context_anchor_tool;
use crate::tools::handlers::parse_arguments;
use crate::tools::registry::CoreToolRuntime;
use crate::tools::registry::ToolExecutor;
use crate::turn_timing::now_unix_timestamp_ms;
use codex_protocol::models::FunctionCallOutputPayload;
use codex_protocol::models::ResponseInputItem;
use codex_tools::ToolName;
use codex_tools::ToolSpec;
use serde::Deserialize;
use serde::Serialize;
use serde_json::Value as JsonValue;

pub(crate) const MAX_CONTEXT_ANCHOR_LABEL_BYTES: usize = 256;
pub(crate) const MAX_CONTEXT_REWIND_NOTE_BYTES: usize = 8 * 1024;

pub struct SaveContextAnchorHandler;
pub struct RewindContextToAnchorHandler;

#[derive(Deserialize)]
struct SaveContextAnchorArgs {
    label: Option<String>,
}

#[derive(Deserialize)]
struct RewindContextToAnchorArgs {
    anchor_id: String,
    note: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
pub(crate) struct SaveContextAnchorResponse {
    pub(crate) anchor_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) label: Option<String>,
    pub(crate) created_at: i64,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
pub(crate) struct RewindContextToAnchorRequest {
    pub(crate) anchor_id: String,
    pub(crate) note: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
pub(crate) struct RewindContextToAnchorResponse {
    pub(crate) anchor_id: String,
    pub(crate) dropped_turns: u32,
}

struct JsonToolOutput<T> {
    value: T,
    text: String,
}

impl<T> JsonToolOutput<T>
where
    T: Serialize,
{
    fn new(value: T, tool_name: &str) -> Result<Self, FunctionCallError> {
        let text = serde_json::to_string(&value).map_err(|err| {
            FunctionCallError::Fatal(format!("failed to serialize {tool_name} response: {err}"))
        })?;
        Ok(Self { value, text })
    }
}

impl<T> ToolOutput for JsonToolOutput<T>
where
    T: Serialize + Send + Sync,
{
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
        serde_json::to_value(&self.value).unwrap_or_else(|err| {
            JsonValue::String(format!(
                "failed to serialize context anchor response: {err}"
            ))
        })
    }
}

#[async_trait::async_trait]
impl ToolExecutor<ToolInvocation> for SaveContextAnchorHandler {
    fn tool_name(&self) -> ToolName {
        ToolName::plain(SAVE_CONTEXT_ANCHOR_TOOL_NAME)
    }

    fn spec(&self) -> ToolSpec {
        create_save_context_anchor_tool()
    }

    async fn handle(
        &self,
        invocation: ToolInvocation,
    ) -> Result<Box<dyn crate::tools::context::ToolOutput>, FunctionCallError> {
        let ToolInvocation { payload, .. } = invocation;

        let ToolPayload::Function { arguments } = payload else {
            return Err(FunctionCallError::RespondToModel(format!(
                "{SAVE_CONTEXT_ANCHOR_TOOL_NAME} handler received unsupported payload"
            )));
        };

        let args: SaveContextAnchorArgs = parse_arguments(&arguments)?;
        let label = args.label.map(|label| label.trim().to_string());
        let label = match label {
            Some(label) if label.is_empty() => None,
            Some(label) => {
                let label_bytes = label.len();
                if label_bytes > MAX_CONTEXT_ANCHOR_LABEL_BYTES {
                    return Err(FunctionCallError::RespondToModel(format!(
                        "`label` is {label_bytes} bytes, but the maximum is {MAX_CONTEXT_ANCHOR_LABEL_BYTES} bytes"
                    )));
                }
                Some(label)
            }
            None => None,
        };
        let response = SaveContextAnchorResponse {
            anchor_id: format!("ctx-{}", uuid::Uuid::now_v7()),
            label,
            created_at: now_unix_timestamp_ms() / 1000,
        };
        Ok(boxed_tool_output(JsonToolOutput::new(
            response,
            SAVE_CONTEXT_ANCHOR_TOOL_NAME,
        )?))
    }
}

impl CoreToolRuntime for SaveContextAnchorHandler {}

#[async_trait::async_trait]
impl ToolExecutor<ToolInvocation> for RewindContextToAnchorHandler {
    fn tool_name(&self) -> ToolName {
        ToolName::plain(REWIND_CONTEXT_TO_ANCHOR_TOOL_NAME)
    }

    fn spec(&self) -> ToolSpec {
        create_rewind_context_to_anchor_tool()
    }

    async fn handle(
        &self,
        invocation: ToolInvocation,
    ) -> Result<Box<dyn crate::tools::context::ToolOutput>, FunctionCallError> {
        let ToolInvocation { payload, .. } = invocation;

        let ToolPayload::Function { arguments } = payload else {
            return Err(FunctionCallError::RespondToModel(format!(
                "{REWIND_CONTEXT_TO_ANCHOR_TOOL_NAME} handler received unsupported payload"
            )));
        };

        let args: RewindContextToAnchorArgs = parse_arguments(&arguments)?;
        let anchor_id = args.anchor_id.trim().to_string();
        if anchor_id.is_empty() {
            return Err(FunctionCallError::RespondToModel(
                "`anchor_id` must be a non-empty string".to_string(),
            ));
        }

        let note = args.note.trim().to_string();
        if note.is_empty() {
            return Err(FunctionCallError::RespondToModel(
                "`note` must be a non-empty string".to_string(),
            ));
        }
        let note_bytes = note.len();
        if note_bytes > MAX_CONTEXT_REWIND_NOTE_BYTES {
            return Err(FunctionCallError::RespondToModel(format!(
                "`note` is {note_bytes} bytes, but the maximum is {MAX_CONTEXT_REWIND_NOTE_BYTES} bytes"
            )));
        }

        let request = RewindContextToAnchorRequest { anchor_id, note };
        Ok(boxed_tool_output(JsonToolOutput::new(
            request,
            REWIND_CONTEXT_TO_ANCHOR_TOOL_NAME,
        )?))
    }
}

impl CoreToolRuntime for RewindContextToAnchorHandler {}

#[cfg(test)]
#[path = "context_anchor_tests.rs"]
mod tests;
