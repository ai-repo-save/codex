use crate::function_tool::FunctionCallError;
use crate::tools::context::ToolInvocation;
use crate::tools::context::ToolOutput;
use crate::tools::context::ToolPayload;
use crate::tools::context::boxed_tool_output;
use crate::tools::handlers::parse_arguments;
use crate::tools::handlers::request_context_compaction_spec::REQUEST_CONTEXT_COMPACTION_TOOL_NAME;
use crate::tools::handlers::request_context_compaction_spec::create_request_context_compaction_tool;
use crate::tools::registry::CoreToolRuntime;
use crate::tools::registry::ToolExecutor;
use codex_protocol::models::FunctionCallOutputPayload;
use codex_protocol::models::ResponseInputItem;
use codex_tools::ToolName;
use codex_tools::ToolSpec;
use serde::Deserialize;
use serde::Serialize;
use serde_json::Value as JsonValue;

pub(crate) const MAX_CONTEXT_COMPACTION_NOTE_BYTES: usize = 16_384;

pub struct RequestContextCompactionHandler;

#[derive(Deserialize)]
struct RequestContextCompactionArgs {
    note: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
struct RequestContextCompactionResponse {
    compacted: bool,
    mode: &'static str,
    note: String,
    note_bytes: usize,
}

struct RequestContextCompactionOutput {
    response: RequestContextCompactionResponse,
    text: String,
}

impl RequestContextCompactionOutput {
    fn new(note: String) -> Result<Self, FunctionCallError> {
        let note_bytes = note.len();
        let response = RequestContextCompactionResponse {
            compacted: true,
            mode: "mid_turn",
            note,
            note_bytes,
        };
        let text = serde_json::to_string(&response).map_err(|err| {
            FunctionCallError::Fatal(format!(
                "failed to serialize {REQUEST_CONTEXT_COMPACTION_TOOL_NAME} response: {err}"
            ))
        })?;
        Ok(Self { response, text })
    }
}

impl ToolOutput for RequestContextCompactionOutput {
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
        serde_json::to_value(&self.response).unwrap_or_else(|err| {
            JsonValue::String(format!(
                "failed to serialize {REQUEST_CONTEXT_COMPACTION_TOOL_NAME} response: {err}"
            ))
        })
    }
}

#[async_trait::async_trait]
impl ToolExecutor<ToolInvocation> for RequestContextCompactionHandler {
    fn tool_name(&self) -> ToolName {
        ToolName::plain(REQUEST_CONTEXT_COMPACTION_TOOL_NAME)
    }

    fn spec(&self) -> ToolSpec {
        create_request_context_compaction_tool()
    }

    async fn handle(
        &self,
        invocation: ToolInvocation,
    ) -> Result<Box<dyn crate::tools::context::ToolOutput>, FunctionCallError> {
        let ToolInvocation { payload, .. } = invocation;

        let ToolPayload::Function { arguments } = payload else {
            return Err(FunctionCallError::RespondToModel(format!(
                "{REQUEST_CONTEXT_COMPACTION_TOOL_NAME} handler received unsupported payload"
            )));
        };

        let args: RequestContextCompactionArgs = parse_arguments(&arguments)?;
        let note = args.note.trim().to_string();
        if note.is_empty() {
            return Err(FunctionCallError::RespondToModel(
                "`note` must be a non-empty string".to_string(),
            ));
        }
        let note_bytes = note.len();
        if note_bytes > MAX_CONTEXT_COMPACTION_NOTE_BYTES {
            return Err(FunctionCallError::RespondToModel(format!(
                "`note` is {note_bytes} bytes, but the maximum is {MAX_CONTEXT_COMPACTION_NOTE_BYTES} bytes"
            )));
        }

        Ok(boxed_tool_output(RequestContextCompactionOutput::new(
            note,
        )?))
    }
}

impl CoreToolRuntime for RequestContextCompactionHandler {}

#[cfg(test)]
#[path = "request_context_compaction_tests.rs"]
mod tests;
