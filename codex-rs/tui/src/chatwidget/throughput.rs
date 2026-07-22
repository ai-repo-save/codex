//! Chat-widget wiring for the status-line throughput tracker.

use super::*;

impl ChatWidget {
    pub(super) fn record_throughput_delta(&mut self, text: &str) {
        let now = Instant::now();
        self.throughput_tracker.record_utf8_bytes(text.len(), now);
        self.refresh_status_line();
        self.schedule_throughput_update(now);
    }

    pub(super) fn update_throughput_sampling(
        &mut self,
        active: bool,
        tokens_per_second: Option<f64>,
    ) {
        let now = Instant::now();
        if active {
            self.throughput_tracker.begin_sampling(now);
        } else {
            self.throughput_tracker.finish_sampling(tokens_per_second, now);
        }
        self.refresh_status_line();
        self.schedule_throughput_update(now);
        self.request_redraw();
    }

    pub(super) fn advance_throughput_tracker(&mut self, now: Instant) {
        if self.throughput_tracker.advance(now) {
            self.refresh_status_line();
        }
        self.schedule_throughput_update(now);
    }

    fn schedule_throughput_update(&self, now: Instant) {
        if let Some(deadline) = self.throughput_tracker.next_update_after(now) {
            self.frame_requester
                .schedule_frame_in(deadline.saturating_duration_since(now));
        }
    }
}
