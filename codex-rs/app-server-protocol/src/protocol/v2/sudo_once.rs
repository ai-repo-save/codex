use codex_protocol::sudo_once::SudoOnceApprovalDecision;
use codex_protocol::sudo_once::SudoOnceCredential as CoreSudoOnceCredential;
use codex_utils_path_uri::LegacyAppPathString;
use schemars::JsonSchema;
use serde::Deserialize;
use serde::Deserializer;
use serde::Serialize;
use serde::Serializer;
use std::fmt;
use ts_rs::TS;

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct SudoOnceRequestApprovalParams {
    pub thread_id: String,
    pub turn_id: String,
    pub item_id: String,
    pub command: String,
    pub cwd: LegacyAppPathString,
    #[ts(optional = nullable)]
    pub reason: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct SudoOnceRequestApprovalResponse {
    pub decision: SudoOnceApprovalDecision,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct SudoOnceRequestCredentialParams {
    pub thread_id: String,
    pub turn_id: String,
    pub item_id: String,
    pub attempt: u32,
}

#[derive(JsonSchema, TS)]
#[schemars(with = "String")]
#[ts(type = "string", export_to = "v2/")]
pub struct SudoOnceCredential(CoreSudoOnceCredential);

impl SudoOnceCredential {
    pub fn new(credential: String) -> Self {
        Self(CoreSudoOnceCredential::new(credential))
    }

    pub fn into_secret(self) -> CoreSudoOnceCredential {
        self.0
    }
}

impl Serialize for SudoOnceCredential {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.0.expose_secret())
    }
}

impl<'de> Deserialize<'de> for SudoOnceCredential {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        String::deserialize(deserializer).map(Self::new)
    }
}

impl fmt::Debug for SudoOnceCredential {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("SudoOnceCredential([REDACTED])")
    }
}

#[derive(Serialize, Deserialize, Debug, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct SudoOnceRequestCredentialResponse {
    /// The credential supplied for this prompt, or null when the user cancels.
    pub credential: Option<SudoOnceCredential>,
}
