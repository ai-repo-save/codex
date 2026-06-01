use std::sync::Arc;

use codex_core_skills::SkillLoadOutcome;
use codex_core_skills::injection::load_skill_injection;
use codex_tools::ToolName;
use codex_tools::ToolSpec;
use serde::Deserialize;

use crate::context::ContextualUserFragment;
use crate::context::SkillInstructions;
use crate::function_tool::FunctionCallError;
use crate::tools::context::FunctionToolOutput;
use crate::tools::context::ToolInvocation;
use crate::tools::context::ToolPayload;
use crate::tools::context::boxed_tool_output;
use crate::tools::handlers::parse_arguments;
use crate::tools::handlers::use_skill_spec::USE_SKILL_TOOL_NAME;
use crate::tools::handlers::use_skill_spec::create_use_skill_tool;
use crate::tools::registry::CoreToolRuntime;
use crate::tools::registry::ToolExecutor;

#[derive(Clone)]
pub struct UseSkillHandler {
    skills: Arc<SkillLoadOutcome>,
}

#[derive(Debug, Deserialize)]
struct UseSkillArgs {
    name: String,
}

impl UseSkillHandler {
    pub(crate) fn new(skills: Arc<SkillLoadOutcome>) -> Self {
        Self { skills }
    }
}

#[async_trait::async_trait]
impl ToolExecutor<ToolInvocation> for UseSkillHandler {
    fn tool_name(&self) -> ToolName {
        ToolName::plain(USE_SKILL_TOOL_NAME)
    }

    fn spec(&self) -> ToolSpec {
        create_use_skill_tool()
    }

    async fn handle(
        &self,
        invocation: ToolInvocation,
    ) -> Result<Box<dyn crate::tools::context::ToolOutput>, FunctionCallError> {
        let ToolInvocation { payload, .. } = invocation;
        let ToolPayload::Function { arguments } = payload else {
            return Err(FunctionCallError::RespondToModel(
                "use_skill handler received unsupported payload".to_string(),
            ));
        };
        let UseSkillArgs { name } = parse_arguments(&arguments)?;
        let skill = enabled_skill_by_name(&self.skills, &name)?;
        let skill_injection = load_skill_injection(skill, Some(&self.skills))
            .await
            .map_err(FunctionCallError::RespondToModel)?;
        let response = SkillInstructions::from(&skill_injection).body();
        Ok(boxed_tool_output(FunctionToolOutput::from_text(
            response,
            Some(true),
        )))
    }
}

impl CoreToolRuntime for UseSkillHandler {}

fn enabled_skill_by_name<'a>(
    skills: &'a SkillLoadOutcome,
    name: &str,
) -> Result<&'a codex_core_skills::SkillMetadata, FunctionCallError> {
    let enabled_matches = skills
        .skills
        .iter()
        .filter(|skill| skill.name == name && skills.is_skill_enabled(skill))
        .collect::<Vec<_>>();

    match enabled_matches.as_slice() {
        [skill] => Ok(skill),
        [] => {
            if skills.skills.iter().any(|skill| skill.name == name) {
                Err(FunctionCallError::RespondToModel(format!(
                    "skill `{name}` is disabled"
                )))
            } else {
                Err(FunctionCallError::RespondToModel(format!(
                    "skill `{name}` was not found in the available skills list"
                )))
            }
        }
        matches => {
            let paths = matches
                .iter()
                .map(|skill| skill.path_to_skills_md.to_string_lossy())
                .collect::<Vec<_>>()
                .join(", ");
            Err(FunctionCallError::RespondToModel(format!(
                "skill name `{name}` is ambiguous; matching SKILL.md paths: {paths}"
            )))
        }
    }
}

#[cfg(test)]
#[path = "use_skill_tests.rs"]
mod tests;
