use codex_tools::JsonSchema;
use codex_tools::ResponsesApiTool;
use codex_tools::ToolSpec;
use std::collections::BTreeMap;

pub(crate) const USE_SKILL_TOOL_NAME: &str = "use_skill";

pub(crate) fn create_use_skill_tool() -> ToolSpec {
    let properties = BTreeMap::from([(
        "name".to_string(),
        JsonSchema::string(Some(
            "Exact name of the skill to load from the available skills list.".to_string(),
        )),
    )]);

    ToolSpec::Function(ResponsesApiTool {
        name: USE_SKILL_TOOL_NAME.to_string(),
        description: "Load a skill by name using Codex's canonical skill registry. Returns the SKILL.md body without YAML frontmatter.".to_string(),
        strict: false,
        defer_loading: None,
        parameters: JsonSchema::object(properties, Some(vec!["name".to_string()]), Some(false.into())),
        output_schema: None,
    })
}

#[cfg(test)]
#[path = "use_skill_spec_tests.rs"]
mod tests;
