use super::*;
use pretty_assertions::assert_eq;

#[test]
fn use_skill_tool_requires_name_argument() {
    let ToolSpec::Function(tool) = create_use_skill_tool() else {
        panic!("use_skill should be a function tool");
    };

    assert_eq!(tool.name, USE_SKILL_TOOL_NAME);
    assert_eq!(tool.parameters.required, Some(vec!["name".to_string()]));
    assert!(
        tool.parameters
            .properties
            .as_ref()
            .expect("parameters should have properties")
            .contains_key("name")
    );
}
