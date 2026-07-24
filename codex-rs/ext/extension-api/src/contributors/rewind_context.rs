use codex_protocol::items::TurnItem;

use crate::ExtensionData;

/// Host context available while extensions contribute context after a rewind.
#[derive(Clone, Copy)]
pub struct RewindContextContributionInput<'a> {
    /// Store scoped to the host session runtime.
    pub session_store: &'a ExtensionData,
    /// Store scoped to this thread runtime.
    pub thread_store: &'a ExtensionData,
    /// Turn items completed after the restored context anchor.
    pub completed_items: &'a [TurnItem],
}
