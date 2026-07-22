use codex_analytics::TurnProfile;
use codex_protocol::items::AgentMessageItem;
use codex_protocol::items::TurnItem;
use codex_protocol::models::ContentItem;
use codex_protocol::models::FunctionCallOutputPayload;
use codex_protocol::models::ResponseItem;
use codex_protocol::protocol::TokenUsage;
use pretty_assertions::assert_eq;
use std::time::Duration;
use std::time::Instant;
use std::time::SystemTime;
use std::time::UNIX_EPOCH;

use super::OutputThroughputVisibleDelta;
use super::TurnProfilePhase;
use super::TurnProfileState;
use super::TurnTimingState;
use super::response_item_records_turn_ttft;
use crate::ResponseEvent;

const VISIBLE_AGENT_MESSAGE: &str = "response";
const VISIBLE_PLAN: &str = "plan";
const VISIBLE_REASONING_SUMMARY: &str = "reasoning summary";
const VISIBLE_REASONING_TEXT: &str = "reasoning text";

#[tokio::test]
async fn turn_timing_state_records_ttft_only_once_per_turn() {
    let state = TurnTimingState::default();
    assert_eq!(
        state
            .record_ttft_for_response_event(&ResponseEvent::OutputTextDelta("hi".to_string()))
            .await,
        None
    );

    state.mark_turn_started(Instant::now()).await;
    assert_eq!(
        state
            .record_ttft_for_response_event(&ResponseEvent::Created)
            .await,
        None
    );
    assert!(
        state
            .record_ttft_for_response_event(&ResponseEvent::OutputTextDelta("hi".to_string()))
            .await
            .is_some()
    );
    assert_eq!(
        state
            .record_ttft_for_response_event(&ResponseEvent::OutputTextDelta("again".to_string()))
            .await,
        None
    );
}

#[tokio::test]
async fn turn_timing_state_records_ttfm_independently_of_ttft() {
    let state = TurnTimingState::default();
    state.mark_turn_started(Instant::now()).await;

    assert!(
        state
            .record_ttft_for_response_event(&ResponseEvent::OutputTextDelta("hi".to_string()))
            .await
            .is_some()
    );
    assert!(
        state
            .record_ttfm_for_turn_item(&TurnItem::AgentMessage(AgentMessageItem {
                id: "msg-1".to_string(),
                content: Vec::new(),
                phase: None,
                memory_citation: None,
            }))
            .await
            .is_some()
    );
    assert_eq!(
        state
            .record_ttfm_for_turn_item(&TurnItem::AgentMessage(AgentMessageItem {
                id: "msg-2".to_string(),
                content: Vec::new(),
                phase: None,
                memory_citation: None,
            }))
            .await,
        None
    );
}

#[tokio::test]
async fn turn_timing_state_records_turn_started_epoch_millis() {
    let state = TurnTimingState::default();
    let before = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time should be after unix epoch")
        .as_millis();

    let started_at_unix_ms = state.mark_turn_started(Instant::now()).await;

    let after = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time should be after unix epoch")
        .as_millis();
    assert!(u128::try_from(started_at_unix_ms).is_ok_and(|ms| before <= ms && ms <= after));
    assert_eq!(
        state.started_at_unix_secs().await,
        Some(started_at_unix_ms / 1000)
    );
}

#[test]
fn response_item_records_turn_ttft_for_first_output_signals() {
    assert!(response_item_records_turn_ttft(
        &ResponseItem::FunctionCall {
            id: None,
            name: "shell".to_string(),
            namespace: None,
            arguments: "{}".to_string(),
            call_id: "call-1".to_string(),
            internal_chat_message_metadata_passthrough: None,
        }
    ));
    assert!(response_item_records_turn_ttft(
        &ResponseItem::CustomToolCall {
            id: None,
            status: None,
            call_id: "call-2".to_string(),
            name: "custom".to_string(),
            namespace: None,
            input: "echo hi".to_string(),
            internal_chat_message_metadata_passthrough: None,
        }
    ));
    assert!(response_item_records_turn_ttft(&ResponseItem::Message {
        id: None,
        role: "assistant".to_string(),
        content: vec![ContentItem::OutputText {
            text: "hello".to_string(),
        }],
        phase: None,
        internal_chat_message_metadata_passthrough: None,
    }));
}

#[test]
fn response_item_records_turn_ttft_ignores_empty_non_output_items() {
    assert!(!response_item_records_turn_ttft(&ResponseItem::Message {
        id: None,
        role: "assistant".to_string(),
        content: vec![ContentItem::OutputText {
            text: String::new(),
        }],
        phase: None,
        internal_chat_message_metadata_passthrough: None,
    }));
    assert!(!response_item_records_turn_ttft(
        &ResponseItem::FunctionCallOutput {
            id: None,
            call_id: "call-1".to_string(),
            output: FunctionCallOutputPayload::from_text("ok".to_string()),
            internal_chat_message_metadata_passthrough: None,
        }
    ));
}

#[test]
fn turn_profile_breaks_down_sampling_blocking_and_retry_overhead() {
    let started_at = Instant::now();
    let mut state = TurnProfileState::default();
    state.start(started_at);

    let _ = state.begin_sampling(started_at + Duration::from_millis(100));
    state.end_phase(
        started_at + Duration::from_millis(600),
        TurnProfilePhase::Sampling,
    );
    let _ = state.begin_tool_blocking(started_at + Duration::from_millis(600));
    state.end_phase(
        started_at + Duration::from_millis(900),
        TurnProfilePhase::ToolBlocking,
    );
    state.record_sampling_retry();
    let _ = state.begin_sampling(started_at + Duration::from_millis(1_000));
    state.end_phase(
        started_at + Duration::from_millis(1_200),
        TurnProfilePhase::Sampling,
    );

    assert_eq!(
        state.complete(started_at + Duration::from_millis(1_300)),
        TurnProfile {
            before_first_sampling_ms: 100,
            sampling_ms: 700,
            between_sampling_overhead_ms: 100,
            tool_blocking_ms: 300,
            after_last_sampling_ms: 100,
            sampling_request_count: 2,
            sampling_retry_count: 1,
        }
    );
}

fn token_usage(output_tokens: i64) -> TokenUsage {
    TokenUsage {
        output_tokens,
        ..TokenUsage::default()
    }
}

#[test]
fn output_throughput_reports_a_completed_sample() {
    let state = TurnTimingState::default();
    let started_at = Instant::now();

    let started = state.begin_output_throughput_sample();
    assert_eq!(started.active, true);
    assert_eq!(started.output_tokens, None);
    assert_eq!(started.active_duration_ms, None);
    assert_eq!(started.tokens_per_second, None);

    state.record_output_throughput_first_visible_output(
        OutputThroughputVisibleDelta::AgentMessageText(VISIBLE_AGENT_MESSAGE),
        started_at + Duration::from_millis(100),
    );
    let completed = state
        .complete_output_throughput_sample(
            Some(&token_usage(50)),
            started_at + Duration::from_millis(1_100),
        )
        .expect("active sample should complete");

    assert_eq!(completed.active, false);
    assert_eq!(completed.output_tokens, Some(50));
    assert_eq!(completed.active_duration_ms, Some(1_000));
    assert_eq!(completed.tokens_per_second, Some(50.0));
}

#[test]
fn output_throughput_reports_a_plan_only_completed_sample() {
    let state = TurnTimingState::default();
    let started_at = Instant::now();

    state.begin_output_throughput_sample();
    state.record_output_throughput_first_visible_output(
        OutputThroughputVisibleDelta::PlanText(VISIBLE_PLAN),
        started_at + Duration::from_millis(100),
    );
    let completed = state
        .complete_output_throughput_sample(
            Some(&token_usage(50)),
            started_at + Duration::from_millis(1_100),
        )
        .expect("active sample should complete");

    assert_eq!(completed.active, false);
    assert_eq!(completed.output_tokens, Some(50));
    assert_eq!(completed.active_duration_ms, Some(1_000));
    assert_eq!(completed.tokens_per_second, Some(50.0));
}

#[test]
fn output_throughput_ignores_empty_visible_deltas() {
    let state = TurnTimingState::default();
    let started_at = Instant::now();

    state.begin_output_throughput_sample();
    state.record_output_throughput_first_visible_output(
        OutputThroughputVisibleDelta::AgentMessageText(""),
        started_at + Duration::from_millis(100),
    );
    state.record_output_throughput_first_visible_output(
        OutputThroughputVisibleDelta::PlanText(""),
        started_at + Duration::from_millis(150),
    );
    state.record_output_throughput_first_visible_output(
        OutputThroughputVisibleDelta::ReasoningSummaryText(""),
        started_at + Duration::from_millis(200),
    );
    state.record_output_throughput_first_visible_output(
        OutputThroughputVisibleDelta::ReasoningText(""),
        started_at + Duration::from_millis(300),
    );
    state.record_output_throughput_first_visible_output(
        OutputThroughputVisibleDelta::ReasoningText(VISIBLE_REASONING_TEXT),
        started_at + Duration::from_millis(400),
    );
    let completed = state
        .complete_output_throughput_sample(
            Some(&token_usage(10)),
            started_at + Duration::from_millis(1_400),
        )
        .expect("active sample should complete");

    assert_eq!(completed.output_tokens, Some(10));
    assert_eq!(completed.active_duration_ms, Some(1_000));
    assert_eq!(completed.tokens_per_second, Some(10.0));
}

#[test]
fn output_throughput_aggregates_completed_samples() {
    let state = TurnTimingState::default();
    let started_at = Instant::now();

    state.begin_output_throughput_sample();
    state.record_output_throughput_first_visible_output(
        OutputThroughputVisibleDelta::AgentMessageText(VISIBLE_AGENT_MESSAGE),
        started_at + Duration::from_millis(100),
    );
    state
        .complete_output_throughput_sample(
            Some(&token_usage(30)),
            started_at + Duration::from_millis(600),
        )
        .expect("first active sample should complete");

    state.begin_output_throughput_sample();
    state.record_output_throughput_first_visible_output(
        OutputThroughputVisibleDelta::ReasoningSummaryText(VISIBLE_REASONING_SUMMARY),
        started_at + Duration::from_millis(800),
    );
    let completed = state
        .complete_output_throughput_sample(
            Some(&token_usage(20)),
            started_at + Duration::from_millis(1_300),
        )
        .expect("second active sample should complete");

    assert_eq!(completed.active, false);
    assert_eq!(completed.output_tokens, Some(50));
    assert_eq!(completed.active_duration_ms, Some(1_000));
    assert_eq!(completed.tokens_per_second, Some(50.0));
}

#[test]
fn output_throughput_becomes_unknown_after_missing_sample_data() {
    let state = TurnTimingState::default();
    let started_at = Instant::now();

    state.begin_output_throughput_sample();
    let missing_first_output = state
        .complete_output_throughput_sample(
            Some(&token_usage(30)),
            started_at + Duration::from_millis(500),
        )
        .expect("active sample should complete");
    assert_eq!(missing_first_output.output_tokens, None);
    assert_eq!(missing_first_output.active_duration_ms, None);
    assert_eq!(missing_first_output.tokens_per_second, None);

    state.begin_output_throughput_sample();
    state.record_output_throughput_first_visible_output(
        OutputThroughputVisibleDelta::AgentMessageText(VISIBLE_AGENT_MESSAGE),
        started_at + Duration::from_millis(700),
    );
    let permanently_unknown = state
        .complete_output_throughput_sample(
            Some(&token_usage(20)),
            started_at + Duration::from_millis(1_200),
        )
        .expect("active sample should complete");
    assert_eq!(permanently_unknown.active, false);
    assert_eq!(permanently_unknown.output_tokens, None);
    assert_eq!(permanently_unknown.active_duration_ms, None);
    assert_eq!(permanently_unknown.tokens_per_second, None);

    let missing_usage_state = TurnTimingState::default();
    missing_usage_state.begin_output_throughput_sample();
    missing_usage_state.record_output_throughput_first_visible_output(
        OutputThroughputVisibleDelta::ReasoningText(VISIBLE_REASONING_TEXT),
        started_at + Duration::from_millis(100),
    );
    let missing_usage = missing_usage_state
        .complete_output_throughput_sample(None, started_at + Duration::from_millis(1_200))
        .expect("active sample should complete");
    assert_eq!(missing_usage.active, false);
    assert_eq!(missing_usage.output_tokens, None);
    assert_eq!(missing_usage.active_duration_ms, None);
    assert_eq!(missing_usage.tokens_per_second, None);
}

#[test]
fn output_throughput_deactivates_once_after_stream_failure() {
    let state = TurnTimingState::default();

    state.begin_output_throughput_sample();
    let deactivated = state
        .abandon_output_throughput_sample()
        .expect("active sample should deactivate");
    assert_eq!(deactivated.active, false);
    assert_eq!(deactivated.output_tokens, None);
    assert_eq!(deactivated.active_duration_ms, None);
    assert_eq!(deactivated.tokens_per_second, None);
    assert_eq!(state.abandon_output_throughput_sample(), None);

    let restarted = state.begin_output_throughput_sample();
    assert_eq!(restarted.active, true);
}
