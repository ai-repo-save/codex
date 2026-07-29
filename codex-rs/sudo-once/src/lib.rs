//! In-process, single-use authorization for local sudo execution.
//!
//! This crate deliberately has no serializable request or response types. A
//! local UI receives prompts through [`SudoOncePromptReceiver`] and resolves
//! them with opaque one-shot responders. Dropping either endpoint denies the
//! pending operation.

use std::fmt;
use std::sync::Arc;

use codex_protocol::ThreadId;
use codex_utils_absolute_path::AbsolutePathBuf;
use tokio::sync::mpsc;
use tokio::sync::oneshot;
use zeroize::Zeroizing;

const PROMPT_CHANNEL_CAPACITY: usize = 16;

#[cfg(target_os = "linux")]
mod linux;

#[cfg(target_os = "linux")]
pub use linux::SealedSudoExecutable;
#[cfg(target_os = "linux")]
pub use linux::sudo_once_available;

#[cfg(not(target_os = "linux"))]
pub fn sudo_once_available() -> bool {
    false
}

/// Immutable execution details the user authorizes for exactly one sudo run.
#[derive(Debug)]
pub struct SudoOnceCommand {
    thread_id: ThreadId,
    argv: Arc<[String]>,
    cwd: AbsolutePathBuf,
    reason: Option<String>,
}

impl SudoOnceCommand {
    pub fn new(
        thread_id: ThreadId,
        argv: Arc<[String]>,
        cwd: AbsolutePathBuf,
        reason: Option<String>,
    ) -> Self {
        Self {
            thread_id,
            argv,
            cwd,
            reason,
        }
    }

    pub fn thread_id(&self) -> ThreadId {
        self.thread_id
    }

    pub fn argv(&self) -> &[String] {
        &self.argv
    }

    pub fn cwd(&self) -> &AbsolutePathBuf {
        &self.cwd
    }

    pub fn reason(&self) -> Option<&str> {
        self.reason.as_deref()
    }
}

/// A one-use approval bound to the exact shared [`SudoOnceCommand`] snapshot.
pub struct SudoOnceGrant {
    command: Arc<SudoOnceCommand>,
}

impl SudoOnceGrant {
    pub fn command(&self) -> &Arc<SudoOnceCommand> {
        &self.command
    }

    pub fn into_command(self) -> Arc<SudoOnceCommand> {
        self.command
    }
}

impl fmt::Debug for SudoOnceGrant {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("SudoOnceGrant([REDACTED])")
    }
}

/// In-memory sudo credential that zeroizes its allocation when dropped.
pub struct SudoOnceCredential(Zeroizing<String>);

impl SudoOnceCredential {
    pub fn new(credential: String) -> Self {
        Self(Zeroizing::new(credential))
    }

    pub fn expose_secret(&self) -> &str {
        self.0.as_str()
    }
}

impl From<String> for SudoOnceCredential {
    fn from(credential: String) -> Self {
        Self::new(credential)
    }
}

/// A concrete local capability for presenting sudo prompts to the trusted UI.
#[derive(Clone)]
pub struct LocalSudoOnceBroker {
    prompts: mpsc::Sender<SudoOncePrompt>,
}

impl LocalSudoOnceBroker {
    pub fn new() -> (Self, SudoOncePromptReceiver) {
        let (prompts, receiver) = mpsc::channel(PROMPT_CHANNEL_CAPACITY);
        (Self { prompts }, SudoOncePromptReceiver { receiver })
    }

    /// Requests authorization of one immutable command snapshot.
    pub async fn request_approval(&self, command: Arc<SudoOnceCommand>) -> Option<SudoOnceGrant> {
        let (response, receiver) = oneshot::channel();
        let prompt = SudoOncePrompt::Approval(SudoOnceApprovalPrompt {
            command: Arc::clone(&command),
            responder: SudoOnceApprovalResponder {
                command,
                response: Some(response),
            },
        });
        self.prompts.send(prompt).await.ok()?;
        receiver.await.ok().flatten()
    }

    /// Requests a fresh credential for a previously approved command.
    pub async fn request_credential(
        &self,
        grant: &SudoOnceGrant,
        attempt: u32,
    ) -> Option<SudoOnceCredential> {
        let (response, receiver) = oneshot::channel();
        let prompt = SudoOncePrompt::Credential(SudoOnceCredentialPrompt {
            command: Arc::clone(&grant.command),
            attempt,
            responder: SudoOnceCredentialResponder {
                response: Some(response),
            },
        });
        self.prompts.send(prompt).await.ok()?;
        receiver.await.ok().flatten()
    }
}

/// The unique receiving endpoint for local sudo prompts.
pub struct SudoOncePromptReceiver {
    receiver: mpsc::Receiver<SudoOncePrompt>,
}

impl SudoOncePromptReceiver {
    pub async fn recv(&mut self) -> Option<SudoOncePrompt> {
        self.receiver.recv().await
    }
}

/// A non-serializable local prompt.
pub enum SudoOncePrompt {
    Approval(SudoOnceApprovalPrompt),
    Credential(SudoOnceCredentialPrompt),
}

impl fmt::Debug for SudoOncePrompt {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Approval(_) => formatter.write_str("SudoOncePrompt::Approval([REDACTED])"),
            Self::Credential(_) => formatter.write_str("SudoOncePrompt::Credential([REDACTED])"),
        }
    }
}

/// Local UI request to authorize one command.
pub struct SudoOnceApprovalPrompt {
    command: Arc<SudoOnceCommand>,
    responder: SudoOnceApprovalResponder,
}

impl SudoOnceApprovalPrompt {
    pub fn command(&self) -> &Arc<SudoOnceCommand> {
        &self.command
    }

    pub fn into_parts(self) -> (Arc<SudoOnceCommand>, SudoOnceApprovalResponder) {
        (self.command, self.responder)
    }
}

impl fmt::Debug for SudoOnceApprovalPrompt {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("SudoOnceApprovalPrompt([REDACTED])")
    }
}

/// Single-use approval responder. Dropping it aborts the request.
pub struct SudoOnceApprovalResponder {
    command: Arc<SudoOnceCommand>,
    response: Option<oneshot::Sender<Option<SudoOnceGrant>>>,
}

impl SudoOnceApprovalResponder {
    pub fn approve(mut self) -> bool {
        self.response.take().is_some_and(|response| {
            response
                .send(Some(SudoOnceGrant {
                    command: Arc::clone(&self.command),
                }))
                .is_ok()
        })
    }

    pub fn abort(mut self) -> bool {
        self.response
            .take()
            .is_some_and(|response| response.send(None).is_ok())
    }

    pub fn is_closed(&self) -> bool {
        self.response
            .as_ref()
            .is_none_or(oneshot::Sender::is_closed)
    }
}

impl fmt::Debug for SudoOnceApprovalResponder {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("SudoOnceApprovalResponder([REDACTED])")
    }
}

/// Local UI request for a fresh credential attempt.
pub struct SudoOnceCredentialPrompt {
    command: Arc<SudoOnceCommand>,
    attempt: u32,
    responder: SudoOnceCredentialResponder,
}

impl SudoOnceCredentialPrompt {
    pub fn command(&self) -> &Arc<SudoOnceCommand> {
        &self.command
    }

    pub fn attempt(&self) -> u32 {
        self.attempt
    }

    pub fn into_parts(self) -> (Arc<SudoOnceCommand>, u32, SudoOnceCredentialResponder) {
        (self.command, self.attempt, self.responder)
    }
}

impl fmt::Debug for SudoOnceCredentialPrompt {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("SudoOnceCredentialPrompt([REDACTED])")
    }
}

/// Single-use credential responder. Dropping it cancels the request.
pub struct SudoOnceCredentialResponder {
    response: Option<oneshot::Sender<Option<SudoOnceCredential>>>,
}

impl SudoOnceCredentialResponder {
    pub fn submit(mut self, credential: SudoOnceCredential) -> bool {
        self.response
            .take()
            .is_some_and(|response| response.send(Some(credential)).is_ok())
    }

    pub fn cancel(mut self) -> bool {
        self.response
            .take()
            .is_some_and(|response| response.send(None).is_ok())
    }

    pub fn is_closed(&self) -> bool {
        self.response
            .as_ref()
            .is_none_or(oneshot::Sender::is_closed)
    }
}

impl fmt::Debug for SudoOnceCredentialResponder {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("SudoOnceCredentialResponder([REDACTED])")
    }
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
