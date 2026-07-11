use codex_protocol::ThreadId;
use std::collections::HashMap;
use std::sync::Mutex;
use tokio::sync::oneshot;
use uuid::Uuid;

#[derive(Debug)]
pub(crate) enum ParentRequestOutcome {
    Answered(String),
    ParentUnavailable,
}

struct PendingParentRequest {
    child_thread_id: ThreadId,
    parent_thread_id: ThreadId,
    sender: oneshot::Sender<ParentRequestOutcome>,
}

#[derive(Default)]
pub(super) struct ParentRequestBroker {
    pending: Mutex<HashMap<String, PendingParentRequest>>,
}

impl ParentRequestBroker {
    pub(super) fn register(
        &self,
        child_thread_id: ThreadId,
        parent_thread_id: ThreadId,
    ) -> (String, oneshot::Receiver<ParentRequestOutcome>) {
        let request_id = Uuid::now_v7().to_string();
        let (sender, receiver) = oneshot::channel();
        self.pending
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .insert(
                request_id.clone(),
                PendingParentRequest {
                    child_thread_id,
                    parent_thread_id,
                    sender,
                },
            );
        (request_id, receiver)
    }

    pub(super) fn answer(
        &self,
        request_id: &str,
        parent_thread_id: ThreadId,
        child_thread_id: ThreadId,
        answer: String,
    ) -> Result<(), String> {
        let mut pending = self.pending.lock().unwrap_or_else(|err| err.into_inner());
        let request = pending.get(request_id).ok_or_else(|| {
            format!("parent request `{request_id}` is unknown, expired, or already answered")
        })?;
        if request.parent_thread_id != parent_thread_id {
            return Err(format!(
                "only parent thread {} may answer parent request `{request_id}`",
                request.parent_thread_id
            ));
        }
        if request.child_thread_id != child_thread_id {
            return Err(format!(
                "parent request `{request_id}` must target child thread {}",
                request.child_thread_id
            ));
        }
        let request = pending.remove(request_id).expect("request exists");
        drop(pending);
        request
            .sender
            .send(ParentRequestOutcome::Answered(answer))
            .map_err(|_| format!("parent request `{request_id}` is no longer waiting"))
    }

    pub(super) fn cancel(&self, request_id: &str) {
        self.pending
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .remove(request_id);
    }

    pub(super) fn cancel_for_thread(&self, thread_id: ThreadId) {
        let mut pending = self.pending.lock().unwrap_or_else(|err| err.into_inner());
        let request_ids = pending
            .iter()
            .filter_map(|(request_id, request)| {
                (request.child_thread_id == thread_id || request.parent_thread_id == thread_id)
                    .then(|| request_id.clone())
            })
            .collect::<Vec<_>>();
        for request_id in request_ids {
            if let Some(request) = pending.remove(&request_id) {
                let _ = request.sender.send(ParentRequestOutcome::ParentUnavailable);
            }
        }
    }
}
