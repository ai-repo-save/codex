//! Live and final model-output throughput state for the status line.

use std::collections::VecDeque;
use std::time::Duration;
use std::time::Instant;

const BYTES_PER_ESTIMATED_TOKEN: f64 = 4.0;
const BUCKET_DURATION: Duration = Duration::from_millis(250);
const WINDOW_DURATION: Duration = Duration::from_secs(5);
const MINIMUM_SAMPLE_DURATION: Duration = Duration::from_millis(500);
const MINIMUM_ESTIMATED_TOKENS: f64 = 4.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum ThroughputDisplay {
    Waiting,
    Approximate(f64),
    Exact(f64),
}

#[derive(Debug)]
struct Bucket {
    started_at: Instant,
    estimated_tokens: f64,
}

/// Tracks the local live estimate while a sampling response is active, then retains the final
/// app-server supplied measurement when it becomes available.
#[derive(Debug, Default)]
pub(super) struct ThroughputTracker {
    sampling_started_at: Option<Instant>,
    first_delta_at: Option<Instant>,
    last_delta_at: Option<Instant>,
    buckets: VecDeque<Bucket>,
    display: Option<ThroughputDisplay>,
}

impl ThroughputTracker {
    pub(super) fn reset(&mut self) {
        self.sampling_started_at = None;
        self.first_delta_at = None;
        self.last_delta_at = None;
        self.buckets.clear();
        self.display = None;
    }

    pub(super) fn begin_sampling(&mut self, now: Instant) {
        if self.sampling_started_at.is_some() {
            return;
        }

        self.sampling_started_at = Some(now);
        self.first_delta_at = None;
        self.last_delta_at = None;
        self.buckets.clear();
        self.display = Some(ThroughputDisplay::Waiting);
    }

    pub(super) fn finish_sampling(&mut self, tokens_per_second: Option<f64>, now: Instant) {
        if let Some(tokens_per_second) = tokens_per_second {
            self.display = Some(ThroughputDisplay::Exact(tokens_per_second));
        } else {
            self.refresh_live_display(now);
        }
        self.sampling_started_at = None;
    }

    pub(super) fn freeze(&mut self, now: Instant) {
        self.refresh_live_display(now);
        self.sampling_started_at = None;
    }

    pub(super) fn record_utf8_bytes(&mut self, byte_count: usize, now: Instant) {
        if self.sampling_started_at.is_none() {
            return;
        }
        if byte_count == 0 {
            return;
        }

        let first_delta_at = *self.first_delta_at.get_or_insert(now);
        let elapsed = now.saturating_duration_since(first_delta_at);
        let bucket_count = elapsed.as_millis() / BUCKET_DURATION.as_millis();
        let bucket_offset = Duration::from_millis(
            u64::try_from(bucket_count)
                .unwrap_or(u64::MAX)
                .saturating_mul(BUCKET_DURATION.as_millis() as u64),
        );
        let bucket_started_at = first_delta_at + bucket_offset;
        let estimated_tokens = byte_count as f64 / BYTES_PER_ESTIMATED_TOKEN;

        match self.buckets.back_mut() {
            Some(bucket) if bucket.started_at == bucket_started_at => {
                bucket.estimated_tokens += estimated_tokens;
            }
            _ => self.buckets.push_back(Bucket {
                started_at: bucket_started_at,
                estimated_tokens,
            }),
        }
        self.last_delta_at = Some(now);
        self.refresh_live_display(now);
    }

    pub(super) fn display(&mut self, now: Instant) -> Option<ThroughputDisplay> {
        self.refresh_live_display(now);
        self.display
    }

    /// Advances time-dependent live state and reports whether a redraw is needed.
    pub(super) fn advance(&mut self, now: Instant) -> bool {
        let display_before = self.display;
        self.refresh_live_display(now);
        display_before != self.display
    }

    pub(super) fn next_update_after(&self, now: Instant) -> Option<Instant> {
        self.sampling_started_at?;
        let first_delta_at = self.first_delta_at?;
        let last_delta_at = self.last_delta_at?;
        let threshold_at = first_delta_at + MINIMUM_SAMPLE_DURATION;
        let stall_at = last_delta_at + WINDOW_DURATION;
        [threshold_at, stall_at]
            .into_iter()
            .filter(|deadline| *deadline > now)
            .min()
    }

    fn refresh_live_display(&mut self, now: Instant) {
        if self.sampling_started_at.is_none()
            || matches!(self.display, Some(ThroughputDisplay::Exact(_)))
        {
            return;
        }

        self.evict_expired_buckets(now);
        let Some(first_delta_at) = self.first_delta_at else {
            self.display = Some(ThroughputDisplay::Waiting);
            return;
        };
        if self.last_delta_at.is_some_and(|last_delta_at| {
            now.saturating_duration_since(last_delta_at) >= WINDOW_DURATION
        }) {
            self.display = Some(ThroughputDisplay::Approximate(0.0));
            return;
        }
        let elapsed = now.saturating_duration_since(first_delta_at);
        let estimated_tokens = self
            .buckets
            .iter()
            .map(|bucket| bucket.estimated_tokens)
            .sum::<f64>();
        if elapsed < MINIMUM_SAMPLE_DURATION || estimated_tokens < MINIMUM_ESTIMATED_TOKENS {
            self.display = Some(ThroughputDisplay::Waiting);
            return;
        }

        let denominator = elapsed.min(WINDOW_DURATION).as_secs_f64();
        self.display = Some(ThroughputDisplay::Approximate(if denominator == 0.0 {
            0.0
        } else {
            estimated_tokens / denominator
        }));
    }

    fn evict_expired_buckets(&mut self, now: Instant) {
        while self.buckets.front().is_some_and(|bucket| {
            now.saturating_duration_since(bucket.started_at) >= WINDOW_DURATION
        }) {
            self.buckets.pop_front();
        }
    }
}

pub(super) fn format_throughput_display(display: ThroughputDisplay) -> String {
    match display {
        ThroughputDisplay::Waiting => "— tok/s".to_string(),
        ThroughputDisplay::Approximate(tokens_per_second) => {
            format!("~{tokens_per_second:.1} tok/s")
        }
        ThroughputDisplay::Exact(tokens_per_second) => format!("{tokens_per_second:.1} tok/s"),
    }
}

#[cfg(test)]
#[path = "throughput_tracker_tests.rs"]
mod tests;
