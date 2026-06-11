use codex_tools::JsonSchema;
use codex_tools::ResponsesApiTool;
use codex_tools::ToolSpec;
use std::collections::BTreeMap;

pub const REQUEST_CONTEXT_COMPACTION_TOOL_NAME: &str = "request_context_compaction";

pub fn create_request_context_compaction_tool() -> ToolSpec {
    let mut properties = BTreeMap::new();
    properties.insert(
        "note".to_string(),
        JsonSchema::string(Some(
            "A concise note for the agent after compaction completes.".to_string(),
        )),
    );

    ToolSpec::Function(ResponsesApiTool {
        name: REQUEST_CONTEXT_COMPACTION_TOOL_NAME.to_string(),
        description: "Synchronously compacts this agent's current context mid-turn and returns a note that will be written into the compacted history."
            .to_string(),
        strict: false,
        defer_loading: None,
        parameters: JsonSchema::object(properties, Some(vec!["note".to_string()]), Some(false.into())),
        output_schema: None,
    })
}
