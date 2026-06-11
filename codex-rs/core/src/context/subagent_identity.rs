use codex_protocol::AgentPath;
use codex_protocol::protocol::SessionSource;
use codex_protocol::protocol::SubAgentSource;

use super::ContextualUserFragment;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SubagentIdentity {
    pub(crate) agent_path: AgentPath,
    pub(crate) parent_agent_path: AgentPath,
}

impl SubagentIdentity {
    pub(crate) fn new(agent_path: AgentPath, parent_agent_path: AgentPath) -> Self {
        Self {
            agent_path,
            parent_agent_path,
        }
    }

    pub(crate) fn from_session_source(session_source: &SessionSource) -> Option<Self> {
        let SessionSource::SubAgent(SubAgentSource::ThreadSpawn {
            agent_path: Some(agent_path),
            ..
        }) = session_source
        else {
            return None;
        };
        let parent_agent_path = agent_path
            .as_str()
            .rsplit_once('/')
            .and_then(|(parent, _)| AgentPath::try_from(parent).ok())?;
        Some(Self::new(agent_path.clone(), parent_agent_path))
    }
}

impl ContextualUserFragment for SubagentIdentity {
    fn role(&self) -> &'static str {
        "developer"
    }

    fn markers(&self) -> (&'static str, &'static str) {
        Self::type_markers()
    }

    fn type_markers() -> (&'static str, &'static str) {
        ("<subagent_identity>", "</subagent_identity>")
    }

    fn body(&self) -> String {
        format!(
            "\n{}\n",
            serde_json::json!({
                "agent_path": &self.agent_path,
                "parent_agent_path": &self.parent_agent_path,
                "is_root_agent": false,
            })
        )
    }
}
