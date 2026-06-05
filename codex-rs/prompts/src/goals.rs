use codex_protocol::protocol::ThreadGoal;
use codex_utils_template::Template;
use codex_utils_template::TemplateParseError;
use std::sync::LazyLock;

pub type GoalPromptTemplate = Template;

const OBJECTIVE: &str = "objective";
const TOKENS_USED: &str = "tokens_used";
const TOKEN_BUDGET: &str = "token_budget";
const REMAINING_TOKENS: &str = "remaining_tokens";
const TIME_USED_SECONDS: &str = "time_used_seconds";
const ALLOWED_PLACEHOLDERS: [&str; 5] = [
    OBJECTIVE,
    TOKENS_USED,
    TOKEN_BUDGET,
    REMAINING_TOKENS,
    TIME_USED_SECONDS,
];

static CONTINUATION_PROMPT_TEMPLATE: LazyLock<Template> =
    LazyLock::new(
        || match Template::parse(include_str!("../templates/goals/continuation.md")) {
            Ok(template) => template,
            Err(err) => panic!("embedded goals/continuation.md template is invalid: {err}"),
        },
    );

static BUDGET_LIMIT_PROMPT_TEMPLATE: LazyLock<Template> =
    LazyLock::new(
        || match Template::parse(include_str!("../templates/goals/budget_limit.md")) {
            Ok(template) => template,
            Err(err) => panic!("embedded goals/budget_limit.md template is invalid: {err}"),
        },
    );

static OBJECTIVE_UPDATED_PROMPT_TEMPLATE: LazyLock<Template> = LazyLock::new(|| {
    match Template::parse(include_str!("../templates/goals/objective_updated.md")) {
        Ok(template) => template,
        Err(err) => {
            panic!("embedded goals/objective_updated.md template is invalid: {err}")
        }
    }
});

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GoalPromptTemplates {
    pub continuation_prompt: Option<Template>,
    pub objective_updated_prompt: Option<Template>,
    pub budget_limit_prompt: Option<Template>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GoalPromptTemplateError {
    Parse(TemplateParseError),
    UnknownPlaceholder { name: String },
}

impl std::fmt::Display for GoalPromptTemplateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Parse(err) => write!(f, "{err}"),
            Self::UnknownPlaceholder { name } => {
                write!(
                    f,
                    "goal prompt template contains unknown placeholder `{name}`"
                )
            }
        }
    }
}

impl std::error::Error for GoalPromptTemplateError {}

impl From<TemplateParseError> for GoalPromptTemplateError {
    fn from(value: TemplateParseError) -> Self {
        Self::Parse(value)
    }
}

pub fn parse_goal_prompt_template(source: &str) -> Result<Template, GoalPromptTemplateError> {
    let template = Template::parse(source)?;
    for placeholder in template.placeholders() {
        if !ALLOWED_PLACEHOLDERS.contains(&placeholder) {
            return Err(GoalPromptTemplateError::UnknownPlaceholder {
                name: placeholder.to_string(),
            });
        }
    }
    Ok(template)
}

/// Builds the hidden prompt used to continue an active goal after the previous
/// turn completes.
pub fn continuation_prompt(goal: &ThreadGoal) -> String {
    continuation_prompt_with_templates(goal, &GoalPromptTemplates::default())
}

pub fn continuation_prompt_with_templates(
    goal: &ThreadGoal,
    templates: &GoalPromptTemplates,
) -> String {
    render_goal_prompt(
        templates
            .continuation_prompt
            .as_ref()
            .unwrap_or(&CONTINUATION_PROMPT_TEMPLATE),
        goal,
        "goals/continuation.md",
    )
}

/// Builds the hidden prompt used to ask the model to wrap up after a goal
/// exhausts its budget.
pub fn budget_limit_prompt(goal: &ThreadGoal) -> String {
    budget_limit_prompt_with_templates(goal, &GoalPromptTemplates::default())
}

pub fn budget_limit_prompt_with_templates(
    goal: &ThreadGoal,
    templates: &GoalPromptTemplates,
) -> String {
    render_goal_prompt(
        templates
            .budget_limit_prompt
            .as_ref()
            .unwrap_or(&BUDGET_LIMIT_PROMPT_TEMPLATE),
        goal,
        "goals/budget_limit.md",
    )
}

/// Builds the hidden prompt used after a user edits an active goal.
pub fn objective_updated_prompt(goal: &ThreadGoal) -> String {
    objective_updated_prompt_with_templates(goal, &GoalPromptTemplates::default())
}

pub fn objective_updated_prompt_with_templates(
    goal: &ThreadGoal,
    templates: &GoalPromptTemplates,
) -> String {
    render_goal_prompt(
        templates
            .objective_updated_prompt
            .as_ref()
            .unwrap_or(&OBJECTIVE_UPDATED_PROMPT_TEMPLATE),
        goal,
        "goals/objective_updated.md",
    )
}

fn render_goal_prompt(template: &Template, goal: &ThreadGoal, template_name: &str) -> String {
    let values = GoalPromptValues::new(goal);
    let variables = template
        .placeholders()
        .map(|placeholder| (placeholder, values.value_for(placeholder)))
        .collect::<Vec<_>>();
    template
        .render(variables)
        .unwrap_or_else(|err| panic!("{template_name} template failed to render: {err}"))
}

struct GoalPromptValues {
    objective: String,
    tokens_used: String,
    token_budget: String,
    remaining_tokens: String,
    time_used_seconds: String,
}

impl GoalPromptValues {
    fn new(goal: &ThreadGoal) -> Self {
        let token_budget = goal
            .token_budget
            .map(|budget| budget.to_string())
            .unwrap_or_else(|| "none".to_string());
        let remaining_tokens = goal
            .token_budget
            .map(|budget| (budget - goal.tokens_used).max(0).to_string())
            .unwrap_or_else(|| "unbounded".to_string());
        Self {
            objective: escape_xml_text(&goal.objective),
            tokens_used: goal.tokens_used.to_string(),
            token_budget,
            remaining_tokens,
            time_used_seconds: goal.time_used_seconds.to_string(),
        }
    }

    fn value_for(&self, placeholder: &str) -> &str {
        match placeholder {
            OBJECTIVE => &self.objective,
            TOKENS_USED => &self.tokens_used,
            TOKEN_BUDGET => &self.token_budget,
            REMAINING_TOKENS => &self.remaining_tokens,
            TIME_USED_SECONDS => &self.time_used_seconds,
            _ => panic!("unknown goal prompt placeholder `{placeholder}`"),
        }
    }
}

fn escape_xml_text(input: &str) -> String {
    input
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

#[cfg(test)]
#[path = "goals_tests.rs"]
mod goals_tests;
