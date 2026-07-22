use codex_extension_items::memory_mutation::MemoryMutation as CoreMemoryMutation;
use codex_extension_items::memory_mutation::MemoryMutationAction as CoreMemoryMutationAction;
use codex_extension_items::memory_mutation::MemoryMutationScope as CoreMemoryMutationScope;
use codex_extension_items::memory_mutation::MemoryMutationStatus as CoreMemoryMutationStatus;
use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;
use ts_rs::TS;

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct MemoryMutation {
    pub id: String,
    pub action: MemoryMutationAction,
    pub scope: MemoryMutationScope,
    pub status: MemoryMutationStatus,
    pub title: Option<String>,
    pub path: Option<String>,
    pub preview: Option<String>,
}

impl From<CoreMemoryMutation> for MemoryMutation {
    fn from(value: CoreMemoryMutation) -> Self {
        Self {
            id: value.id().to_string(),
            action: value.action().into(),
            scope: value.scope().into(),
            status: value.status().into(),
            title: value.title().map(String::from),
            path: value.path().map(String::from),
            preview: value.preview().map(String::from),
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase", export_to = "v2/")]
pub enum MemoryMutationAction {
    Write,
    Delete,
}

impl From<CoreMemoryMutationAction> for MemoryMutationAction {
    fn from(value: CoreMemoryMutationAction) -> Self {
        match value {
            CoreMemoryMutationAction::Write => Self::Write,
            CoreMemoryMutationAction::Delete => Self::Delete,
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase", export_to = "v2/")]
pub enum MemoryMutationScope {
    Global,
    Session,
    Project,
}

impl From<CoreMemoryMutationScope> for MemoryMutationScope {
    fn from(value: CoreMemoryMutationScope) -> Self {
        match value {
            CoreMemoryMutationScope::Global => Self::Global,
            CoreMemoryMutationScope::Session => Self::Session,
            CoreMemoryMutationScope::Project => Self::Project,
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase", export_to = "v2/")]
pub enum MemoryMutationStatus {
    InProgress,
    Succeeded,
    Failed,
}

impl From<CoreMemoryMutationStatus> for MemoryMutationStatus {
    fn from(value: CoreMemoryMutationStatus) -> Self {
        match value {
            CoreMemoryMutationStatus::InProgress => Self::InProgress,
            CoreMemoryMutationStatus::Succeeded => Self::Succeeded,
            CoreMemoryMutationStatus::Failed => Self::Failed,
        }
    }
}
