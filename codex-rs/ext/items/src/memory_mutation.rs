use schemars::JsonSchema;
use serde::Deserialize;
use serde::Deserializer;
use serde::Serialize;
use ts_rs::TS;
use unicode_segmentation::UnicodeSegmentation;

pub const MEMORY_MUTATION_TITLE_MAX_GRAPHEMES: usize = 80;
pub const MEMORY_MUTATION_PREVIEW_MAX_GRAPHEMES: usize = 160;
pub const MEMORY_MUTATION_PATH_MAX_GRAPHEMES: usize = 240;

/// A visible mutation of a Codex memory store.
#[derive(Debug, Clone, Serialize, TS, JsonSchema, PartialEq)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct MemoryMutation {
    id: String,
    action: MemoryMutationAction,
    scope: MemoryMutationScope,
    status: MemoryMutationStatus,
    title: Option<String>,
    path: Option<String>,
    preview: Option<String>,
}

impl MemoryMutation {
    /// Stable identifier for protocol and UI mappings.
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Mutation action as an enum value.
    pub fn action(&self) -> MemoryMutationAction {
        self.action
    }

    /// Mutation scope as an enum value.
    pub fn scope(&self) -> MemoryMutationScope {
        self.scope
    }

    /// Mutation status as an enum value.
    pub fn status(&self) -> MemoryMutationStatus {
        self.status
    }

    /// Optional title describing the mutation target.
    pub fn title(&self) -> Option<&str> {
        self.title.as_deref()
    }

    /// Optional path for delete mutations.
    pub fn path(&self) -> Option<&str> {
        self.path.as_deref()
    }

    /// Optional normalized preview snippet for write mutations.
    pub fn preview(&self) -> Option<&str> {
        self.preview.as_deref()
    }

    pub fn write(
        id: String,
        scope: MemoryMutationScope,
        title: Option<String>,
        note: &str,
    ) -> Self {
        Self::new(
            id,
            MemoryMutationAction::Write,
            scope,
            MemoryMutationStatus::InProgress,
            title,
            None,
            first_non_empty_line_preview(note),
        )
    }

    pub fn delete(id: String, scope: MemoryMutationScope, path: String) -> Self {
        Self::new(
            id,
            MemoryMutationAction::Delete,
            scope,
            MemoryMutationStatus::InProgress,
            None,
            Some(path),
            None,
        )
    }

    pub fn with_status(mut self, status: MemoryMutationStatus) -> Self {
        self.status = status;
        self
    }

    pub fn with_path(mut self, path: String) -> Self {
        self.path = Some(truncate_graphemes(
            &path,
            MEMORY_MUTATION_PATH_MAX_GRAPHEMES,
        ));
        self
    }

    fn new(
        id: String,
        action: MemoryMutationAction,
        scope: MemoryMutationScope,
        status: MemoryMutationStatus,
        title: Option<String>,
        path: Option<String>,
        preview: Option<String>,
    ) -> Self {
        Self {
            id,
            action,
            scope,
            status,
            title: title.map(|title| truncate_graphemes(&title, MEMORY_MUTATION_TITLE_MAX_GRAPHEMES)),
            path: path.map(|path| truncate_graphemes(&path, MEMORY_MUTATION_PATH_MAX_GRAPHEMES)),
            preview: preview.map(|preview| normalize_preview(&preview)),
        }
    }
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, TS, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub enum MemoryMutationAction {
    Write,
    Delete,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, TS, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub enum MemoryMutationScope {
    Global,
    Session,
    Project,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, TS, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub enum MemoryMutationStatus {
    InProgress,
    Succeeded,
    Failed,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SerializedMemoryMutation {
    id: String,
    action: MemoryMutationAction,
    scope: MemoryMutationScope,
    status: MemoryMutationStatus,
    title: Option<String>,
    path: Option<String>,
    preview: Option<String>,
}

impl<'de> Deserialize<'de> for MemoryMutation {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let item = SerializedMemoryMutation::deserialize(deserializer)?;
        Ok(Self::new(
            item.id,
            item.action,
            item.scope,
            item.status,
            item.title,
            item.path,
            item.preview
                .and_then(|preview| first_non_empty_line_preview(&preview)),
        ))
    }
}

fn first_non_empty_line_preview(note: &str) -> Option<String> {
    note.lines()
        .find(|line| !line.trim().is_empty())
        .map(normalize_preview)
}

fn normalize_preview(preview: &str) -> String {
    let collapsed = preview.split_whitespace().collect::<Vec<_>>().join(" ");
    truncate_graphemes(&collapsed, MEMORY_MUTATION_PREVIEW_MAX_GRAPHEMES)
}

fn truncate_graphemes(value: &str, max_graphemes: usize) -> String {
    value.graphemes(true).take(max_graphemes).collect()
}
