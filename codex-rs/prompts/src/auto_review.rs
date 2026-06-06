use codex_utils_template::Template;
use codex_utils_template::TemplateParseError;

pub type AutoReviewPromptTemplate = Template;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AutoReviewPromptTemplateError {
    Parse(TemplateParseError),
    UnknownPlaceholder { name: String },
}

impl std::fmt::Display for AutoReviewPromptTemplateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Parse(err) => write!(f, "{err}"),
            Self::UnknownPlaceholder { name } => {
                write!(
                    f,
                    "auto-review prompt template contains unknown placeholder `{name}`"
                )
            }
        }
    }
}

impl std::error::Error for AutoReviewPromptTemplateError {}

impl From<TemplateParseError> for AutoReviewPromptTemplateError {
    fn from(value: TemplateParseError) -> Self {
        Self::Parse(value)
    }
}

pub fn parse_auto_review_prompt_template(
    source: &str,
) -> Result<Template, AutoReviewPromptTemplateError> {
    let template = Template::parse(source)?;
    if let Some(placeholder) = template.placeholders().next() {
        return Err(AutoReviewPromptTemplateError::UnknownPlaceholder {
            name: placeholder.to_string(),
        });
    }
    Ok(template)
}

pub fn render_auto_review_prompt_template(template: &Template) -> String {
    template
        .render(Vec::<(&str, &str)>::new())
        .unwrap_or_else(|err| panic!("auto-review prompt template failed to render: {err}"))
}

#[cfg(test)]
#[path = "auto_review_tests.rs"]
mod auto_review_tests;
