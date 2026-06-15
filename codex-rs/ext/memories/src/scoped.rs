use std::fs::OpenOptions;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;

use chrono::Utc;
use codex_context_fragments::ContextualUserFragment;
use codex_context_fragments::ScopedMemoryContextFragment;
use codex_utils_absolute_path::AbsolutePathBuf;
use codex_utils_output_truncation::TruncationPolicy;
use codex_utils_output_truncation::truncate_text;
use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;
use sha2::Digest;
use sha2::Sha256;

use crate::backend::ListMemoriesRequest;
use crate::backend::ListMemoriesResponse;
use crate::backend::MemoriesBackend;
use crate::backend::MemoriesBackendError;
use crate::backend::AddAdHocMemoryNoteRequest;
use crate::backend::AddAdHocMemoryNoteResponse;
use crate::backend::ReadMemoryRequest;
use crate::backend::ReadMemoryResponse;
use crate::backend::SearchMemoriesRequest;
use crate::backend::SearchMemoriesResponse;
use crate::local::LocalMemoriesBackend;

const SCOPED_MEMORIES_DIR: &str = "scoped-memories";
const NOTES_DIR: &str = "notes";
const PROJECT_METADATA_FILENAME: &str = "metadata.toml";
const SESSION_CONTEXT_TOKEN_LIMIT: usize = 10_000;
const PROJECT_CONTEXT_TOKEN_LIMIT: usize = 15_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum MemoryScope {
    Global,
    Session,
    Project,
}

impl MemoryScope {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Global => "global",
            Self::Session => "session",
            Self::Project => "project",
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct MemoryToolBackends {
    global: Option<LocalMemoriesBackend>,
    session: Option<ScopedMemoryStore>,
    project: Option<ScopedMemoryStore>,
}

impl MemoryToolBackends {
    pub(crate) fn new(
        codex_home: &AbsolutePathBuf,
        global_enabled: bool,
        scoped_enabled: bool,
        thread_id: &str,
        project_root: &AbsolutePathBuf,
    ) -> Self {
        let global = global_enabled.then(|| LocalMemoriesBackend::from_codex_home(codex_home));
        let session = scoped_enabled.then(|| ScopedMemoryStore::session(codex_home, thread_id));
        let project =
            scoped_enabled.then(|| ScopedMemoryStore::project(codex_home, project_root.clone()));
        Self {
            global,
            session,
            project,
        }
    }

    pub(crate) fn from_global_memory_root(root: impl Into<PathBuf>) -> Self {
        Self {
            global: Some(LocalMemoriesBackend::from_memory_root(root)),
            session: None,
            project: None,
        }
    }

    pub(crate) fn has_global(&self) -> bool {
        self.global.is_some()
    }

    pub(crate) fn has_scoped(&self) -> bool {
        self.session.is_some() || self.project.is_some()
    }

    pub(crate) async fn list(
        &self,
        scope: Option<MemoryScope>,
        request: ListMemoriesRequest,
    ) -> Result<ListMemoriesResponse, MemoriesBackendError> {
        self.backend_for_read(scope)?
            .list(request)
            .await
    }

    pub(crate) async fn add_global_ad_hoc_note(
        &self,
        request: AddAdHocMemoryNoteRequest,
    ) -> Result<AddAdHocMemoryNoteResponse, MemoriesBackendError> {
        let Some(global) = self.global.as_ref() else {
            return Err(MemoriesBackendError::invalid_path(
                MemoryScope::Global.as_str(),
                "memory scope is not enabled",
            ));
        };
        global.add_ad_hoc_note(request).await
    }

    pub(crate) async fn read(
        &self,
        scope: Option<MemoryScope>,
        request: ReadMemoryRequest,
    ) -> Result<ReadMemoryResponse, MemoriesBackendError> {
        self.backend_for_read(scope)?
            .read(request)
            .await
    }

    pub(crate) async fn search(
        &self,
        scope: Option<MemoryScope>,
        request: SearchMemoriesRequest,
    ) -> Result<SearchMemoriesResponse, MemoriesBackendError> {
        self.backend_for_read(scope)?
            .search(request)
            .await
    }

    pub(crate) async fn write_note(
        &self,
        scope: MemoryScope,
        title: String,
        note: String,
    ) -> Result<WriteScopedMemoryNoteResponse, MemoriesBackendError> {
        let store = match scope {
            MemoryScope::Session => self.session.as_ref(),
            MemoryScope::Project => self.project.as_ref(),
            MemoryScope::Global => {
                return Err(MemoriesBackendError::invalid_path(
                    scope.as_str(),
                    "does not support scoped note writes",
                ));
            }
        }
        .ok_or_else(|| {
            MemoriesBackendError::invalid_path(scope.as_str(), "memory scope is not enabled")
        })?;
        store.write_note(title, note).await
    }

    pub(crate) async fn scoped_context_fragment(&self) -> Option<String> {
        let mut sections = Vec::new();
        if let Some(session) = self.session.as_ref()
            && let Some(section) = session
                .context_section("Session memory", SESSION_CONTEXT_TOKEN_LIMIT)
                .await
        {
            sections.push(section);
        }
        if let Some(project) = self.project.as_ref()
            && let Some(section) = project
                .context_section("Project memory", PROJECT_CONTEXT_TOKEN_LIMIT)
                .await
        {
            sections.push(section);
        }
        if sections.is_empty() {
            return None;
        }

        let body = format!(
            "\n## Scoped Memory\n{}\n\nUse `memories.write_note` with `scope: \"session\"` or `scope: \"project\"` when the user explicitly asks Codex to remember something for the current session or project.\n",
            sections.join("\n\n")
        );
        Some(ScopedMemoryContextFragment::new(body).render())
    }

    fn backend_for_read(
        &self,
        scope: Option<MemoryScope>,
    ) -> Result<&LocalMemoriesBackend, MemoriesBackendError> {
        match scope {
            Some(MemoryScope::Global) => self.global.as_ref(),
            Some(MemoryScope::Session) => self.session.as_ref().map(|store| &store.backend),
            Some(MemoryScope::Project) => self.project.as_ref().map(|store| &store.backend),
            None => self.global.as_ref(),
        }
        .ok_or_else(|| {
            MemoriesBackendError::invalid_path(
                scope.map(MemoryScope::as_str).unwrap_or("scope"),
                if scope.is_none() {
                    "must be set to session or project because global memories are disabled"
                } else {
                    "memory scope is not enabled"
                },
            )
        })
    }
}

#[derive(Debug, Clone)]
struct ScopedMemoryStore {
    scope: MemoryScope,
    root: PathBuf,
    backend: LocalMemoriesBackend,
    project_root: Option<AbsolutePathBuf>,
}

impl ScopedMemoryStore {
    fn session(codex_home: &AbsolutePathBuf, thread_id: &str) -> Self {
        let root = codex_home
            .join(SCOPED_MEMORIES_DIR)
            .join("sessions")
            .join(thread_id);
        Self {
            scope: MemoryScope::Session,
            backend: LocalMemoriesBackend::from_memory_root(root.to_path_buf()),
            root: root.to_path_buf(),
            project_root: None,
        }
    }

    fn project(codex_home: &AbsolutePathBuf, project_root: AbsolutePathBuf) -> Self {
        let project_key = project_key(&project_root);
        let root = codex_home
            .join(SCOPED_MEMORIES_DIR)
            .join("projects")
            .join(project_key);
        Self {
            scope: MemoryScope::Project,
            backend: LocalMemoriesBackend::from_memory_root(root.to_path_buf()),
            root: root.to_path_buf(),
            project_root: Some(project_root),
        }
    }

    async fn write_note(
        &self,
        title: String,
        note: String,
    ) -> Result<WriteScopedMemoryNoteResponse, MemoriesBackendError> {
        if note.trim().is_empty() {
            return Err(MemoriesBackendError::EmptyAdHocNote);
        }
        self.ensure_root().await?;
        self.write_project_metadata().await?;
        let notes_dir = self.root.join(NOTES_DIR);
        ensure_directory(&notes_dir).await?;

        let timestamp = Utc::now().format("%Y-%m-%dT%H-%M-%S");
        let slug = slugify(&title);
        for attempt in 0..100 {
            let filename = if attempt == 0 {
                format!("{timestamp}-{slug}.md")
            } else {
                format!("{timestamp}-{slug}-{attempt}.md")
            };
            let path = notes_dir.join(&filename);
            let mut file = match OpenOptions::new().write(true).create_new(true).open(&path) {
                Ok(file) => file,
                Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(err) => return Err(err.into()),
            };
            file.write_all(note.as_bytes())?;
            return Ok(WriteScopedMemoryNoteResponse {
                scope: self.scope,
                path: format!("{NOTES_DIR}/{filename}"),
            });
        }

        Err(MemoriesBackendError::AdHocNoteAlreadyExists {
            filename: format!("{timestamp}-{slug}.md"),
        })
    }

    async fn context_section(&self, title: &str, token_limit: usize) -> Option<String> {
        let notes_dir = self.root.join(NOTES_DIR);
        let mut entries = tokio::fs::read_dir(notes_dir).await.ok()?;
        let mut paths = Vec::new();
        while let Ok(Some(entry)) = entries.next_entry().await {
            let path = entry.path();
            if path
                .extension()
                .is_some_and(|extension| extension.to_string_lossy() == "md")
            {
                paths.push(path);
            }
        }
        paths.sort();
        if paths.is_empty() {
            return None;
        }

        let mut notes = Vec::new();
        for path in paths {
            let Ok(content) = tokio::fs::read_to_string(&path).await else {
                continue;
            };
            let Some(filename) = path.file_name().and_then(|filename| filename.to_str()) else {
                continue;
            };
            let content = content.trim();
            if !content.is_empty() {
                notes.push(format!("### {filename}\n{content}"));
            }
        }
        if notes.is_empty() {
            return None;
        }

        let content = truncate_text(
            &notes.join("\n\n"),
            TruncationPolicy::Tokens(token_limit),
        );
        Some(format!("### {title}\n{content}"))
    }

    async fn ensure_root(&self) -> Result<(), MemoriesBackendError> {
        ensure_directory(&self.root).await
    }

    async fn write_project_metadata(&self) -> Result<(), MemoriesBackendError> {
        let Some(project_root) = self.project_root.as_ref() else {
            return Ok(());
        };
        let metadata_path = self.root.join(PROJECT_METADATA_FILENAME);
        if metadata_path.exists() {
            return Ok(());
        }
        let escaped_project_root = project_root.to_string_lossy().replace('\\', "\\\\").replace('"', "\\\"");
        tokio::fs::write(
            metadata_path,
            format!("project_root = \"{escaped_project_root}\"\n"),
        )
        .await?;
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub(crate) struct WriteScopedMemoryNoteResponse {
    pub(crate) scope: MemoryScope,
    pub(crate) path: String,
}

fn project_key(project_root: &AbsolutePathBuf) -> String {
    let canonical = std::fs::canonicalize(project_root.as_path())
        .unwrap_or_else(|_| project_root.to_path_buf());
    let mut hasher = Sha256::new();
    hasher.update(canonical.to_string_lossy().as_bytes());
    let digest = hasher.finalize();
    digest
        .iter()
        .take(16)
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn slugify(title: &str) -> String {
    let mut slug = String::new();
    let mut previous_hyphen = false;
    for byte in title.bytes() {
        let next = if byte.is_ascii_alphanumeric() {
            previous_hyphen = false;
            Some(byte.to_ascii_lowercase() as char)
        } else if !previous_hyphen {
            previous_hyphen = true;
            Some('-')
        } else {
            None
        };
        if let Some(next) = next
            && slug.len() < 80
        {
            slug.push(next);
        }
    }
    let slug = slug.trim_matches('-').to_string();
    if slug.is_empty() {
        "note".to_string()
    } else {
        slug
    }
}

async fn ensure_directory(path: &Path) -> Result<(), MemoriesBackendError> {
    match tokio::fs::symlink_metadata(path).await {
        Ok(metadata) => {
            if metadata.file_type().is_symlink() {
                return Err(MemoriesBackendError::invalid_path(
                    path.display().to_string(),
                    "must not be a symlink",
                ));
            }
            if metadata.is_dir() {
                return Ok(());
            }
            Err(MemoriesBackendError::invalid_path(
                path.display().to_string(),
                "must be a directory",
            ))
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            tokio::fs::create_dir_all(path).await?;
            Ok(())
        }
        Err(err) => Err(err.into()),
    }
}
