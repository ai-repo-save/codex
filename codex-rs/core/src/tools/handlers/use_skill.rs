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
use codex_protocol::items::SkillLoadItem;
use codex_protocol::items::SkillLoadStatus;
use codex_protocol::items::TurnItem;

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

impl ToolExecutor<ToolInvocation> for UseSkillHandler {
    fn tool_name(&self) -> ToolName {
        ToolName::plain(USE_SKILL_TOOL_NAME)
    }

    fn spec(&self) -> ToolSpec {
        create_use_skill_tool()
    }

    fn handle(&self, invocation: ToolInvocation) -> codex_tools::ToolExecutorFuture<'_> {
        Box::pin(self.handle_call(invocation))
    }
}

impl UseSkillHandler {
    async fn handle_call(
        &self,
        invocation: ToolInvocation,
    ) -> Result<Box<dyn crate::tools::context::ToolOutput>, FunctionCallError> {
        let ToolInvocation { payload, .. } = &invocation;
        let ToolPayload::Function { arguments } = payload else {
            return Err(FunctionCallError::RespondToModel(
                "use_skill handler received unsupported payload".to_string(),
            ));
        };
        let UseSkillArgs { name } = parse_arguments(arguments)?;
        let skill = match enabled_skill_by_name(&self.skills, &name) {
            Ok(skill) => skill,
            Err(err) => {
                emit_skill_load_item(
                    &invocation,
                    &name,
                    skill_path_for_name(&self.skills, &name),
                    Err(&err),
                )
                .await;
                return Err(err);
            }
        };
        let skill_injection = match load_skill_injection(skill, Some(&self.skills)).await {
            Ok(skill_injection) => skill_injection,
            Err(message) => {
                let err = FunctionCallError::RespondToModel(message);
                emit_skill_load_item(
                    &invocation,
                    &name,
                    Some(skill.path_to_skills_md.clone()),
                    Err(&err),
                )
                .await;
                return Err(err);
            }
        };
        emit_skill_load_item(
            &invocation,
            &name,
            Some(skill.path_to_skills_md.clone()),
            Ok(()),
        )
        .await;
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

async fn emit_skill_load_item(
    invocation: &ToolInvocation,
    name: &str,
    path: Option<codex_utils_absolute_path::AbsolutePathBuf>,
    result: Result<(), &FunctionCallError>,
) {
    let (status, error) = match result {
        Ok(()) => (SkillLoadStatus::Completed, None),
        Err(err) => (SkillLoadStatus::Failed, Some(err.to_string())),
    };
    invocation
        .session
        .emit_turn_item_completed(
            invocation.turn.as_ref(),
            TurnItem::SkillLoad(SkillLoadItem {
                id: invocation.call_id.clone(),
                name: name.to_string(),
                path,
                status,
                error,
            }),
        )
        .await;
}

fn skill_path_for_name(
    skills: &SkillLoadOutcome,
    name: &str,
) -> Option<codex_utils_absolute_path::AbsolutePathBuf> {
    let mut matches = skills
        .skills
        .iter()
        .filter(|skill| skill.name == name)
        .map(|skill| skill.path_to_skills_md.clone());
    let path = matches.next()?;
    if matches.next().is_some() {
        None
    } else {
        Some(path)
    }
}

#[cfg(test)]
#[path = "use_skill_tests.rs"]
mod tests;
