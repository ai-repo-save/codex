use codex_tools::JsonSchema;
use codex_tools::ResponsesApiTool;
use codex_tools::ToolSpec;
use std::collections::BTreeMap;

pub const GET_CONTEXT_USAGE_TOOL_NAME: &str = "get_context_usage";

pub fn create_get_context_usage_tool() -> ToolSpec {
    ToolSpec::Function(ResponsesApiTool {
        name: GET_CONTEXT_USAGE_TOOL_NAME.to_string(),
        description: "Returns a read-only JSON snapshot of this agent's current context window usage: whether usage is known, the effective model context window, used tokens, remaining tokens, and remaining percentage."
            .to_string(),
        strict: false,
        defer_loading: None,
        parameters: JsonSchema::object(BTreeMap::new(), /*required*/ None, Some(false.into())),
        output_schema: None,
    })
}
