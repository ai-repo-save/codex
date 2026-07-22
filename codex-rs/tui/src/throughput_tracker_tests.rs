use super::*;
use pretty_assertions::assert_eq;

const FOUR_TOKENS_IN_UTF8_BYTES: usize = 16;
const TWENTY_TOKENS_IN_UTF8_BYTES: usize = 80;

fn start_tracker() -> (ThroughputTracker, Instant) {
    let start = Instant::now();
    let mut tracker = ThroughputTracker::default();
    tracker.begin_sampling(start);
    (tracker, start)
}

#[test]
fn excludes_pre_delta_waiting_time_from_the_live_estimate() {
    let (mut tracker, start) = start_tracker();
    let first_delta_at = start + Duration::from_secs(10);
    tracker.record_utf8_bytes(FOUR_TOKENS_IN_UTF8_BYTES, first_delta_at);

    assert_eq!(
        tracker.display(first_delta_at),
        Some(ThroughputDisplay::Waiting)
    );
    assert_eq!(
        tracker.display(first_delta_at + MINIMUM_SAMPLE_DURATION - Duration::from_millis(1)),
        Some(ThroughputDisplay::Waiting)
    );
    assert_eq!(
        tracker.display(first_delta_at + MINIMUM_SAMPLE_DURATION),
        Some(ThroughputDisplay::Approximate(8.0))
    );
}

#[test]
fn evicts_output_after_the_five_second_window() {
    let (mut tracker, start) = start_tracker();
    tracker.record_utf8_bytes(TWENTY_TOKENS_IN_UTF8_BYTES, start);

    assert_eq!(
        tracker.display(start + WINDOW_DURATION),
        Some(ThroughputDisplay::Approximate(0.0))
    );
}

#[test]
fn freezes_an_approximation_when_the_server_has_no_final_measurement() {
    let (mut tracker, start) = start_tracker();
    tracker.record_utf8_bytes(TWENTY_TOKENS_IN_UTF8_BYTES, start);
    let measured_at = start + MINIMUM_SAMPLE_DURATION;

    tracker.finish_sampling(/*tokens_per_second*/ None, measured_at);
    tracker.record_utf8_bytes(
        TWENTY_TOKENS_IN_UTF8_BYTES,
        measured_at + Duration::from_secs(1),
    );

    assert_eq!(
        tracker.display(measured_at + WINDOW_DURATION),
        Some(ThroughputDisplay::Approximate(40.0))
    );
}

#[test]
fn freezes_an_approximation_when_a_turn_is_aborted() {
    let (mut tracker, start) = start_tracker();
    tracker.record_utf8_bytes(TWENTY_TOKENS_IN_UTF8_BYTES, start);
    let frozen_at = start + MINIMUM_SAMPLE_DURATION;

    tracker.freeze(frozen_at);
    tracker.record_utf8_bytes(
        TWENTY_TOKENS_IN_UTF8_BYTES,
        frozen_at + Duration::from_secs(1),
    );

    assert_eq!(
        tracker.display(frozen_at + WINDOW_DURATION),
        Some(ThroughputDisplay::Approximate(40.0))
    );
}

#[test]
fn prefers_the_exact_server_measurement_after_sampling_finishes() {
    let (mut tracker, start) = start_tracker();
    tracker.record_utf8_bytes(TWENTY_TOKENS_IN_UTF8_BYTES, start);

    tracker.finish_sampling(Some(12.34), start + MINIMUM_SAMPLE_DURATION);

    assert_eq!(
        tracker.display(start + WINDOW_DURATION),
        Some(ThroughputDisplay::Exact(12.34))
    );
}

#[test]
fn new_sampling_replaces_a_previous_exact_measurement() {
    let (mut tracker, start) = start_tracker();
    tracker.finish_sampling(Some(12.34), start);

    tracker.begin_sampling(start + Duration::from_secs(1));

    assert_eq!(
        tracker.display(start + Duration::from_secs(1)),
        Some(ThroughputDisplay::Waiting)
    );
}

#[test]
fn reset_clears_values_and_replay_has_no_active_sample() {
    let (mut tracker, start) = start_tracker();
    tracker.record_utf8_bytes(TWENTY_TOKENS_IN_UTF8_BYTES, start);
    tracker.reset();
    tracker.record_utf8_bytes(TWENTY_TOKENS_IN_UTF8_BYTES, start + Duration::from_secs(1));

    assert_eq!(tracker.display(start + Duration::from_secs(1)), None);
}
