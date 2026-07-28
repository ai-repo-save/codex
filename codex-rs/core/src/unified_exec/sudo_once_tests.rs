use super::sudo_once::SudoPromptFilter;
use pretty_assertions::assert_eq;

const PROMPT_SENTINEL: &str = "__PROMPT__";
const STARTED_SENTINEL: &str = "__STARTED__";

#[test]
fn finish_preserves_non_marker_output() {
    let mut filter =
        SudoPromptFilter::new(PROMPT_SENTINEL.to_string(), STARTED_SENTINEL.to_string());
    filter.push(b"ordinary".to_vec());

    assert_eq!(filter.finish(), Some(b"ordinary".to_vec()));
}

#[test]
fn finish_discards_partial_authentication_markers() {
    let mut prompt_filter =
        SudoPromptFilter::new(PROMPT_SENTINEL.to_string(), STARTED_SENTINEL.to_string());
    prompt_filter.push(PROMPT_SENTINEL.as_bytes()[..4].to_vec());
    assert_eq!(prompt_filter.finish(), None);

    let mut started_filter =
        SudoPromptFilter::new(PROMPT_SENTINEL.to_string(), STARTED_SENTINEL.to_string());
    started_filter.push(STARTED_SENTINEL.as_bytes()[..4].to_vec());
    assert_eq!(started_filter.finish(), None);
}
