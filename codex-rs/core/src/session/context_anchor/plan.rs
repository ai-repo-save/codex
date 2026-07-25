use codex_protocol::config_types::ModeKind;
use codex_protocol::models::ResponseItem;
use codex_protocol::protocol::ContextAnchorSavedEvent;
use codex_protocol::protocol::ContextRewoundToAnchorEvent;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::RolloutItem;

#[derive(Clone, Debug)]
pub(super) struct RewindPlan {
    source_items: Vec<RolloutItem>,
    pub(super) rewind_event: ContextRewoundToAnchorEvent,
    contribution_items: Vec<ResponseItem>,
    replacement_anchor_id: String,
}

#[derive(Clone, Debug)]
pub(super) struct FinalizedRewindPlan {
    pub(super) rewind_event: ContextRewoundToAnchorEvent,
    pub(super) replacement_anchor: ContextAnchorSavedEvent,
    pub(super) replay_items: Vec<RolloutItem>,
    pub(super) persisted_items: Vec<RolloutItem>,
}

impl RewindPlan {
    pub(super) fn new(
        source_items: Vec<RolloutItem>,
        rewind_event: ContextRewoundToAnchorEvent,
        contribution_items: Vec<ResponseItem>,
        replacement_anchor_id: String,
    ) -> Self {
        Self {
            source_items,
            rewind_event,
            contribution_items,
            replacement_anchor_id,
        }
    }

    pub(super) fn replay_items_before_replacement(&self) -> Vec<RolloutItem> {
        self.source_items
            .iter()
            .cloned()
            .chain(std::iter::once(RolloutItem::EventMsg(
                EventMsg::ContextRewoundToAnchor(self.rewind_event.clone()),
            )))
            .chain(
                self.contribution_items
                    .iter()
                    .cloned()
                    .map(RolloutItem::ResponseItem),
            )
            .collect()
    }

    pub(super) fn finalize(
        self,
        history_boundary: u64,
        created_at: i64,
        collaboration_mode_kind: ModeKind,
    ) -> FinalizedRewindPlan {
        let replacement_anchor = ContextAnchorSavedEvent {
            anchor_id: self.replacement_anchor_id,
            label: Some(format!(
                "after rewind from {}",
                self.rewind_event.anchor_id
            )),
            history_boundary,
            created_at,
            collaboration_mode_kind: Some(collaboration_mode_kind),
        };
        let persisted_items = std::iter::once(RolloutItem::EventMsg(
            EventMsg::ContextRewoundToAnchor(self.rewind_event.clone()),
        ))
        .chain(
            self.contribution_items
                .iter()
                .cloned()
                .map(RolloutItem::ResponseItem),
        )
        .chain(std::iter::once(RolloutItem::EventMsg(
            EventMsg::ContextAnchorSaved(replacement_anchor.clone()),
        )))
        .collect();
        let replay_items = self
            .replay_items_before_replacement()
            .into_iter()
            .chain(std::iter::once(RolloutItem::EventMsg(
                EventMsg::ContextAnchorSaved(replacement_anchor.clone()),
            )))
            .collect();

        FinalizedRewindPlan {
            rewind_event: self.rewind_event,
            replacement_anchor,
            replay_items,
            persisted_items,
        }
    }
}
