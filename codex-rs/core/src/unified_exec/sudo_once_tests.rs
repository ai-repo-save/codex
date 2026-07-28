use super::sudo_once::SudoPromptFilter;
use super::sudo_once::SudoAuthenticationState;
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

#[test]
fn finish_withholds_marker_prefix_after_ordinary_output() {
    let mut input = b"ordinary".to_vec();
    input.extend_from_slice(&PROMPT_SENTINEL.as_bytes()[..4]);
    let mut filter =
        SudoPromptFilter::new(PROMPT_SENTINEL.to_string(), STARTED_SENTINEL.to_string());
    let mut output = filter
        .push(input)
        .into_iter()
        .filter_map(|action| match action {
            super::sudo_once::SudoPromptAction::Output(chunk) => Some(chunk),
            super::sudo_once::SudoPromptAction::Prompt | super::sudo_once::SudoPromptAction::Started => None,
        })
        .flatten()
        .collect::<Vec<_>>();
    if let Some(tail) = filter.finish() {
        output.extend(tail);
    }

    assert_eq!(output, b"ordinary");
}

#[test]
fn authentication_state_rejects_credential_requests_after_startup() {
    let mut state = SudoAuthenticationState::default();

    assert!(state.permits_credential_request());
    assert!(state.mark_started());
    assert!(!state.permits_credential_request());
    assert!(!state.mark_started());
}
