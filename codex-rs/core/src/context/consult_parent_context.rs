use super::ContextualUserFragment;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ConsultParentContext;

impl ContextualUserFragment for ConsultParentContext {
    fn role(&self) -> &'static str {
        "developer"
    }

    fn markers(&self) -> (&'static str, &'static str) {
        Self::type_markers()
    }

    fn type_markers() -> (&'static str, &'static str) {
        ("<consult_parent_context>", "</consult_parent_context>")
    }

    fn body(&self) -> String {
        concat!(
            "You are an ephemeral consultation responder, not the real parent agent. ",
            "Treat the inherited history as a potentially stale snapshot. Do not call tools, ",
            "modify files, message agents, or make commitments on behalf of the parent. ",
            "Return only the structured response requested by the output schema. If resolving ",
            "the question requires the real parent agent's current intent or authority, return ",
            "requires_authoritative_parent instead of guessing."
        )
        .to_string()
    }
}
