//! Chat-widget wiring for app-server supplied status-line throughput.

use super::*;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum ThroughputDisplay {
    Waiting,
    Approximate(f64),
    Exact(f64),
}

impl ChatWidget {
    pub(super) fn update_throughput(&mut self, active: bool, tokens_per_second: Option<f64>) {
        match (active, tokens_per_second) {
            (true, None) => self.throughput = Some(ThroughputDisplay::Waiting),
            (true, Some(tokens_per_second)) => {
                self.throughput = Some(ThroughputDisplay::Approximate(tokens_per_second));
            }
            (false, Some(tokens_per_second)) => {
                self.throughput = Some(ThroughputDisplay::Exact(tokens_per_second));
            }
            (false, None) => {}
        }
        self.refresh_status_line();
        self.request_redraw();
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
