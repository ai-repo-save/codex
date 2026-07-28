use codex_utils_absolute_path::AbsolutePathBuf;
use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;
use std::fmt;
use ts_rs::TS;
use zeroize::Zeroizing;

/// Requests a privileged execution mode for one command.
#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export_to = "protocol/")]
pub enum ExecPrivilege {
    SudoOnce,
}

/// User decision for a single sudo execution request.
#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export_to = "protocol/")]
pub enum SudoOnceApprovalDecision {
    Accept,
    Abort,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "protocol/")]
pub struct SudoOnceApprovalRequestEvent {
    pub call_id: String,
    pub turn_id: String,
    pub command: Vec<String>,
    pub cwd: AbsolutePathBuf,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "protocol/")]
pub struct SudoOnceApprovalResponse {
    pub decision: SudoOnceApprovalDecision,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "protocol/")]
pub struct SudoOnceCredentialRequestEvent {
    pub call_id: String,
    pub turn_id: String,
    pub attempt: u32,
}

/// In-memory sudo credential that clears its allocation when dropped.
///
/// This type deliberately does not implement `Clone`, `Serialize`, or `TS`.
pub struct SudoOnceCredential(Zeroizing<String>);

impl SudoOnceCredential {
    pub fn new(credential: String) -> Self {
        Self(Zeroizing::new(credential))
    }

    pub fn expose_secret(&self) -> &str {
        self.0.as_str()
    }
}

impl fmt::Debug for SudoOnceCredential {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("SudoOnceCredential([REDACTED])")
    }
}

impl From<String> for SudoOnceCredential {
    fn from(credential: String) -> Self {
        Self::new(credential)
    }
}

/// Resolution of one sudo credential prompt.
///
/// `None` means the user cancelled the command.
#[derive(Debug)]
pub struct SudoOnceCredentialResponse {
    pub credential: Option<SudoOnceCredential>,
}
