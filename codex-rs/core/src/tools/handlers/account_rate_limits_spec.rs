use codex_tools::JsonSchema;
use codex_tools::ResponsesApiTool;
use codex_tools::ToolSpec;
use std::collections::BTreeMap;

pub const GET_ACCOUNT_RATE_LIMITS_TOOL_NAME: &str = "get_account_rate_limits";

pub fn create_get_account_rate_limits_tool() -> ToolSpec {
    ToolSpec::Function(ResponsesApiTool {
        name: GET_ACCOUNT_RATE_LIMITS_TOOL_NAME.to_string(),
        description: "Returns a bounded snapshot of the current ChatGPT account rate-limit buckets, including used and remaining percentages for each window. The response reports the total bucket count and whether buckets or backend-provided strings were truncated. Returns a structured unavailable result when the current provider or authentication mode cannot query account limits."
            .to_string(),
        strict: false,
        defer_loading: None,
        parameters: JsonSchema::object(BTreeMap::new(), /*required*/ None, Some(false.into())),
        output_schema: None,
    })
}
