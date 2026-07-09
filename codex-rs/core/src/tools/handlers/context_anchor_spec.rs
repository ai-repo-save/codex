use codex_tools::JsonSchema;
use codex_tools::ResponsesApiTool;
use codex_tools::ToolSpec;
use std::collections::BTreeMap;

pub const SAVE_CONTEXT_ANCHOR_TOOL_NAME: &str = "save_context_anchor";
pub const REWIND_CONTEXT_TO_ANCHOR_TOOL_NAME: &str = "rewind_context_to_anchor";
pub const LIST_CONTEXT_ANCHORS_TOOL_NAME: &str = "list_context_anchors";

pub fn create_save_context_anchor_tool() -> ToolSpec {
    let mut properties = BTreeMap::new();
    properties.insert(
        "label".to_string(),
        JsonSchema::string(Some(
            "Optional short label describing why this context anchor was saved.".to_string(),
        )),
    );

    ToolSpec::Function(ResponsesApiTool {
        name: SAVE_CONTEXT_ANCHOR_TOOL_NAME.to_string(),
        description: "Saves the current committed model context as a stable anchor that can be rewound to later in this thread."
            .to_string(),
        strict: false,
        defer_loading: None,
        parameters: JsonSchema::object(properties, /*required*/ None, Some(false.into())),
        output_schema: None,
    })
}

pub fn create_rewind_context_to_anchor_tool() -> ToolSpec {
    let mut properties = BTreeMap::new();
    properties.insert(
        "anchor_id".to_string(),
        JsonSchema::string(Some(
            "Anchor id returned by save_context_anchor in this thread.".to_string(),
        )),
    );
    properties.insert(
        "note".to_string(),
        JsonSchema::string(Some(
            "Bounded information from the discarded future to carry forward after rewinding."
                .to_string(),
        )),
    );

    ToolSpec::Function(ResponsesApiTool {
        name: REWIND_CONTEXT_TO_ANCHOR_TOOL_NAME.to_string(),
        description: "Rewinds this thread's model context to a saved anchor, discarding later context while carrying forward the provided note. A successful rewind atomically replaces the target anchor and reports the replacement anchor id plus estimated reclaimed context."
            .to_string(),
        strict: false,
        defer_loading: None,
        parameters: JsonSchema::object(
            properties,
            Some(vec!["anchor_id".to_string(), "note".to_string()]),
            Some(false.into()),
        ),
        output_schema: None,
    })
}

pub fn create_list_context_anchors_tool() -> ToolSpec {
    let mut properties = BTreeMap::new();
    properties.insert(
        "limit".to_string(),
        JsonSchema::number(Some(
            "Maximum number of active anchors to return. Defaults to 20 and cannot exceed 100."
                .to_string(),
        )),
    );

    ToolSpec::Function(ResponsesApiTool {
        name: LIST_CONTEXT_ANCHORS_TOOL_NAME.to_string(),
        description: "Lists active context anchors in this thread with bounded distance estimates so you can choose a useful rewind target."
            .to_string(),
        strict: false,
        defer_loading: None,
        parameters: JsonSchema::object(properties, /*required*/ None, Some(false.into())),
        output_schema: None,
    })
}
