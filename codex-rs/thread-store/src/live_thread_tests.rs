use super::*;
use crate::ArchiveThreadParams;
use crate::DeleteThreadParams;
use crate::ListThreadsParams;
use crate::ReadThreadByRolloutPathParams;
use crate::ThreadPage;
use crate::ThreadPersistenceMetadata;
use codex_protocol::models::BaseInstructions;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::SessionSource;
use codex_protocol::protocol::UserMessageEvent;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use tempfile::TempDir;

struct MetadataFailingStore {
    inner: LocalThreadStore,
    fail_next_metadata_update: AtomicBool,
}

impl MetadataFailingStore {
    fn new(inner: LocalThreadStore) -> Self {
        Self {
            inner,
            fail_next_metadata_update: AtomicBool::new(true),
        }
    }
}

impl ThreadStore for MetadataFailingStore {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn default_history_mode(&self) -> ThreadHistoryMode {
        self.inner.default_history_mode()
    }

    fn create_thread(&self, params: CreateThreadParams) -> crate::ThreadStoreFuture<'_, ()> {
        self.inner.create_thread(params)
    }

    fn resume_thread(&self, params: ResumeThreadParams) -> crate::ThreadStoreFuture<'_, ()> {
        self.inner.resume_thread(params)
    }

    fn append_items(&self, params: AppendThreadItemsParams) -> crate::ThreadStoreFuture<'_, ()> {
        self.inner.append_items(params)
    }

    fn persist_thread(&self, thread_id: ThreadId) -> crate::ThreadStoreFuture<'_, ()> {
        self.inner.persist_thread(thread_id)
    }

    fn flush_thread(&self, thread_id: ThreadId) -> crate::ThreadStoreFuture<'_, ()> {
        self.inner.flush_thread(thread_id)
    }

    fn shutdown_thread(&self, thread_id: ThreadId) -> crate::ThreadStoreFuture<'_, ()> {
        self.inner.shutdown_thread(thread_id)
    }

    fn discard_thread(&self, thread_id: ThreadId) -> crate::ThreadStoreFuture<'_, ()> {
        self.inner.discard_thread(thread_id)
    }

    fn load_history(
        &self,
        params: LoadThreadHistoryParams,
    ) -> crate::ThreadStoreFuture<'_, StoredThreadHistory> {
        self.inner.load_history(params)
    }

    fn load_canonical_history(
        &self,
        params: LoadThreadHistoryParams,
    ) -> crate::ThreadStoreFuture<'_, StoredThreadHistory> {
        self.inner.load_canonical_history(params)
    }

    fn read_thread(
        &self,
        params: ReadThreadParams,
    ) -> crate::ThreadStoreFuture<'_, StoredThread> {
        self.inner.read_thread(params)
    }

    fn read_thread_by_rollout_path(
        &self,
        params: ReadThreadByRolloutPathParams,
    ) -> crate::ThreadStoreFuture<'_, StoredThread> {
        self.inner.read_thread_by_rollout_path(params)
    }

    fn list_threads(
        &self,
        params: ListThreadsParams,
    ) -> crate::ThreadStoreFuture<'_, ThreadPage> {
        self.inner.list_threads(params)
    }

    fn update_thread_metadata(
        &self,
        params: UpdateThreadMetadataParams,
    ) -> crate::ThreadStoreFuture<'_, StoredThread> {
        if self.fail_next_metadata_update.swap(false, Ordering::SeqCst) {
            return Box::pin(async {
                Err(ThreadStoreError::Internal {
                    message: "injected metadata projection failure".to_string(),
                })
            });
        }
        self.inner.update_thread_metadata(params)
    }

    fn archive_thread(&self, params: ArchiveThreadParams) -> crate::ThreadStoreFuture<'_, ()> {
        self.inner.archive_thread(params)
    }

    fn unarchive_thread(
        &self,
        params: ArchiveThreadParams,
    ) -> crate::ThreadStoreFuture<'_, StoredThread> {
        self.inner.unarchive_thread(params)
    }

    fn delete_thread(&self, params: DeleteThreadParams) -> crate::ThreadStoreFuture<'_, ()> {
        self.inner.delete_thread(params)
    }
}

#[tokio::test]
async fn append_reports_canonical_commit_when_metadata_projection_fails() {
    let codex_home = TempDir::new().expect("temp codex home");
    let local_store = LocalThreadStore::new(
        crate::LocalThreadStoreConfig {
            codex_home: codex_home.path().to_path_buf(),
            sqlite_home: codex_home.path().to_path_buf(),
            default_model_provider_id: "test-provider".to_string(),
        },
        None,
    );
    let failing_store = Arc::new(MetadataFailingStore::new(local_store.clone()));
    let thread_id = ThreadId::default();
    let live_thread = LiveThread::create(
        failing_store,
        CreateThreadParams {
            session_id: thread_id.into(),
            thread_id,
            extra_config: None,
            forked_from_id: None,
            parent_thread_id: None,
            source: SessionSource::Exec,
            thread_source: None,
            originator: "test_originator".to_string(),
            base_instructions: BaseInstructions::default(),
            dynamic_tools: Vec::new(),
            selected_capability_roots: Vec::new(),
            multi_agent_version: None,
            history_mode: ThreadHistoryMode::Legacy,
            subagent_history_start_ordinal: None,
            initial_window_id: uuid::Uuid::now_v7().to_string(),
            metadata: ThreadPersistenceMetadata {
                cwd: Some(
                    codex_utils_absolute_path::AbsolutePathBuf::from_absolute_path(
                        codex_home.path(),
                    )
                    .expect("temp codex home should be absolute"),
                ),
                model_provider: "test-provider".to_string(),
                memory_mode: ThreadMemoryMode::Enabled,
            },
        },
    )
    .await
    .expect("create live thread");
    let appended_item = RolloutItem::EventMsg(EventMsg::UserMessage(UserMessageEvent {
        client_id: None,
        message: "durable before metadata failure".to_string(),
        images: None,
        local_images: Vec::new(),
        text_elements: Vec::new(),
        ..Default::default()
    }));

    live_thread
        .append_items(std::slice::from_ref(&appended_item))
        .await
        .expect_err("metadata projection should fail after canonical append");

    let history = live_thread
        .load_canonical_history(/*include_archived*/ false)
        .await
        .expect("canonical JSONL history should remain readable");
    assert_eq!(history.items.last(), Some(&appended_item));
}
