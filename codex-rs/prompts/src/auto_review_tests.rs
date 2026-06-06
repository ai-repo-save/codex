use super::parse_auto_review_prompt_template;
use super::render_auto_review_prompt_template;

#[test]
fn override_auto_review_prompt_rejects_unknown_placeholder() {
    let err = parse_auto_review_prompt_template("review {{ action }}")
        .expect_err("auto-review prompt placeholders are not part of the v1 contract");

    assert_eq!(
        err.to_string(),
        "auto-review prompt template contains unknown placeholder `action`"
    );
}

#[test]
fn override_auto_review_prompt_renders_literal_text() {
    let template = parse_auto_review_prompt_template("review exactly this action")
        .expect("literal auto-review prompt should parse");

    assert_eq!(
        render_auto_review_prompt_template(&template),
        "review exactly this action"
    );
}
