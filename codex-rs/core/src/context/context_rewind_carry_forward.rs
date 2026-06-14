use super::ContextualUserFragment;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ContextRewindCarryForward {
    anchor_id: String,
    note: String,
}

impl ContextRewindCarryForward {
    pub(crate) fn new(anchor_id: impl Into<String>, note: impl Into<String>) -> Self {
        Self {
            anchor_id: anchor_id.into(),
            note: note.into(),
        }
    }
}

impl ContextualUserFragment for ContextRewindCarryForward {
    fn role(&self) -> &'static str {
        "user"
    }

    fn markers(&self) -> (&'static str, &'static str) {
        Self::type_markers()
    }

    fn type_markers() -> (&'static str, &'static str) {
        (
            "<context_rewind_carry_forward>",
            "</context_rewind_carry_forward>",
        )
    }

    fn body(&self) -> String {
        format!(
            "\n{}\n",
            serde_json::json!({
                "anchor_id": &self.anchor_id,
                "note": &self.note,
            })
        )
    }
}
