// Module declarations for the app-server protocol namespace.
// Exposes protocol pieces used by `lib.rs` via `pub use protocol::common::*;`.

mod collaboration_items;
pub mod common;
mod context_anchor_items;
pub mod event_mapping;
mod hook_prompt_items;
mod image_generation_items;
pub mod item_builders;
mod mappers;
mod serde_helpers;
pub mod thread_history;
pub mod thread_history_projection;
pub mod v1;
pub mod v2;
