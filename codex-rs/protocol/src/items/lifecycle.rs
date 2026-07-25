use crate::protocol::ThreadHistoryMode;

use super::TurnItem;
use codex_extension_items::ExtensionItem;

/// History modes in which a completed canonical turn item is durable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompletedItemPersistence {
    /// Persist the canonical completion only in paginated rollouts.
    PaginatedOnly,
    /// Persist the canonical completion in both legacy and paginated rollouts.
    AllHistoryModes,
}

impl CompletedItemPersistence {
    /// Returns whether the canonical completion is durable in `history_mode`.
    pub fn includes(self, history_mode: ThreadHistoryMode) -> bool {
        match self {
            Self::PaginatedOnly => matches!(history_mode, ThreadHistoryMode::Paginated),
            Self::AllHistoryModes => true,
        }
    }
}

/// How the legacy thread-history builder handles a canonical lifecycle item.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MaterializedItemHandling {
    /// Upsert the canonical item directly.
    CanonicalLifecycle,
    /// Reconstruct the item from its response item or legacy event projection.
    CompatibilityProjection,
}

/// Durable and legacy-history behavior for one canonical turn item.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TurnItemLifecyclePolicy {
    completed_item_persistence: CompletedItemPersistence,
    materialized_item_handling: MaterializedItemHandling,
}

impl TurnItemLifecyclePolicy {
    /// Returns the history modes that durably store the canonical completion.
    pub fn completed_item_persistence(self) -> CompletedItemPersistence {
        self.completed_item_persistence
    }

    /// Returns how legacy history reconstruction handles the canonical item.
    pub fn materialized_item_handling(self) -> MaterializedItemHandling {
        self.materialized_item_handling
    }
}

impl TurnItem {
    /// Returns the shared persistence and history-materialization policy for this item.
    pub fn lifecycle_policy(&self) -> TurnItemLifecyclePolicy {
        let completed_item_persistence = match self {
            Self::Plan(_)
            | Self::Extension(ExtensionItem::Sleep(_) | ExtensionItem::MemoryMutation(_)) => {
                CompletedItemPersistence::AllHistoryModes
            }
            Self::UserMessage(_)
            | Self::HookPrompt(_)
            | Self::AgentMessage(_)
            | Self::Reasoning(_)
            | Self::CommandExecution(_)
            | Self::DynamicToolCall(_)
            | Self::CollabAgentToolCall(_)
            | Self::SubAgentActivity(_)
            | Self::WebSearch(_)
            | Self::ImageView(_)
            | Self::Extension(
                ExtensionItem::ImageGeneration(_) | ExtensionItem::WebSearch(_),
            )
            | Self::ImageGeneration(_)
            | Self::EnteredReviewMode(_)
            | Self::ExitedReviewMode(_)
            | Self::FileChange(_)
            | Self::McpToolCall(_)
            | Self::SkillLoad(_)
            | Self::ContextCompaction(_) => CompletedItemPersistence::PaginatedOnly,
        };
        let materialized_item_handling = match self {
            Self::HookPrompt(_)
            | Self::Plan(_)
            | Self::CommandExecution(_)
            | Self::DynamicToolCall(_)
            | Self::CollabAgentToolCall(_)
            | Self::SubAgentActivity(_)
            | Self::Extension(_)
            | Self::EnteredReviewMode(_)
            | Self::ExitedReviewMode(_)
            | Self::SkillLoad(_) => MaterializedItemHandling::CanonicalLifecycle,
            Self::UserMessage(_)
            | Self::AgentMessage(_)
            | Self::Reasoning(_)
            | Self::WebSearch(_)
            | Self::ImageView(_)
            | Self::ImageGeneration(_)
            | Self::FileChange(_)
            | Self::McpToolCall(_)
            | Self::ContextCompaction(_) => MaterializedItemHandling::CompatibilityProjection,
        };

        TurnItemLifecyclePolicy {
            completed_item_persistence,
            materialized_item_handling,
        }
    }
}
