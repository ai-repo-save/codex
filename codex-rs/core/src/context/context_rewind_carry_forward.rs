use super::ContextualUserFragment;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ContextRewindCarryForward {
    anchor_id: String,
    dropped_turns: u32,
    response_items_reclaimed: u64,
    approx_tokens_reclaimed: u64,
    reclaim_threshold_percent: u32,
    reclaim_threshold_tokens: Option<u64>,
    reclaim_threshold_met: Option<bool>,
    note: String,
}

impl ContextRewindCarryForward {
    pub(crate) fn new(
        anchor_id: impl Into<String>,
        dropped_turns: u32,
        response_items_reclaimed: u64,
        approx_tokens_reclaimed: u64,
        reclaim_threshold_percent: u32,
        reclaim_threshold_tokens: Option<u64>,
        reclaim_threshold_met: Option<bool>,
        note: impl Into<String>,
    ) -> Self {
        Self {
            anchor_id: anchor_id.into(),
            dropped_turns,
            response_items_reclaimed,
            approx_tokens_reclaimed,
            reclaim_threshold_percent,
            reclaim_threshold_tokens,
            reclaim_threshold_met,
            note: note.into(),
        }
    }
}

impl ContextualUserFragment for ContextRewindCarryForward {
    fn role(&self) -> &'static str {
        "user"
    }

    fn markers(&self) -> (&'static str, &'static str) {
        Self::type_markers()
    }

    fn type_markers() -> (&'static str, &'static str) {
        (
            "<context_rewind_carry_forward>",
            "</context_rewind_carry_forward>",
        )
    }

    fn body(&self) -> String {
        format!(
            "\n{}\n",
            serde_json::json!({
                "anchor_id": &self.anchor_id,
                "note": &self.note,
                "rewind_benefit": {
                    "dropped_user_turns": self.dropped_turns,
                    "response_items_reclaimed": self.response_items_reclaimed,
                    "approx_tokens_reclaimed": self.approx_tokens_reclaimed,
                    "significance_threshold": {
                        "percent": self.reclaim_threshold_percent,
                        "tokens": self.reclaim_threshold_tokens,
                        "met": self.reclaim_threshold_met,
                    },
                    "guidance": "If the reclaim benefit is unexpectedly small, call list_context_anchors before the next rewind and choose an earlier active anchor.",
                },
            })
        )
    }
}
