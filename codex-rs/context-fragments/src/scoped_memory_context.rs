use crate::ContextualUserFragment;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScopedMemoryContextFragment {
    body: String,
}

impl ScopedMemoryContextFragment {
    pub fn new(body: impl Into<String>) -> Self {
        Self { body: body.into() }
    }
}

impl ContextualUserFragment for ScopedMemoryContextFragment {
    fn role(&self) -> &'static str {
        "user"
    }

    fn markers(&self) -> (&'static str, &'static str) {
        Self::type_markers()
    }

    fn type_markers() -> (&'static str, &'static str) {
        ("<scoped_memory_context>", "</scoped_memory_context>")
    }

    fn body(&self) -> String {
        self.body.clone()
    }
}
