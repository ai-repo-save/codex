use crate::context::ContextualUserFragment;
use codex_extension_api::PromptFragment;
use codex_extension_api::PromptSlot;
use codex_protocol::models::ResponseItem;

#[derive(Clone, Debug)]
struct RewindContributionFragment {
    slot: PromptSlot,
    text: String,
}

#[derive(Default)]
pub(super) struct RewindContributions {
    prompt_fragments: Vec<RewindContributionFragment>,
    contextual_fragments: Vec<Box<dyn ContextualUserFragment + Send>>,
}

impl RewindContributions {
    pub(super) fn from_fragments(
        prompt_fragments: impl IntoIterator<Item = PromptFragment>,
        contextual_fragments: impl IntoIterator<Item = Box<dyn ContextualUserFragment + Send>>,
    ) -> Self {
        let prompt_fragments = prompt_fragments
            .into_iter()
            .map(|fragment| RewindContributionFragment {
                slot: fragment.slot(),
                text: fragment.text().to_string(),
            })
            .collect::<Vec<_>>();
        Self {
            prompt_fragments,
            contextual_fragments: contextual_fragments.into_iter().collect(),
        }
    }

    pub(super) fn into_response_items(self) -> Vec<ResponseItem> {
        let mut developer_sections = Vec::new();
        let mut contextual_user_sections = Vec::new();
        let mut separate_developer_sections = Vec::new();

        for fragment in self.prompt_fragments {
            match fragment.slot {
                PromptSlot::DeveloperPolicy | PromptSlot::DeveloperCapabilities => {
                    developer_sections.push(fragment.text);
                }
                PromptSlot::ContextualUser => contextual_user_sections.push(fragment.text),
                PromptSlot::SeparateDeveloper => {
                    separate_developer_sections.push(fragment.text);
                }
            }
        }

        let mut typed_developer_items = Vec::new();
        let mut typed_user_items = Vec::new();
        for fragment in self.contextual_fragments {
            match fragment.role() {
                "developer" => typed_developer_items.push(fragment.into_boxed_response_item()),
                "user" => typed_user_items.push(fragment.into_boxed_response_item()),
                role => {
                    tracing::warn!(
                        role,
                        "extension contributed unsupported rewind fragment role"
                    );
                }
            }
        }

        let mut contribution_items =
            Vec::with_capacity(3 + typed_developer_items.len() + typed_user_items.len());
        if let Some(developer_message) =
            crate::context_manager::updates::build_developer_update_item(developer_sections)
        {
            contribution_items.push(developer_message);
        }
        contribution_items.extend(typed_developer_items);
        for section in separate_developer_sections {
            if let Some(developer_message) =
                crate::context_manager::updates::build_developer_update_item(vec![section])
            {
                contribution_items.push(developer_message);
            }
        }
        if let Some(contextual_user_message) =
            crate::context_manager::updates::build_contextual_user_message(contextual_user_sections)
        {
            contribution_items.push(contextual_user_message);
        }
        contribution_items.extend(typed_user_items);
        contribution_items
    }
}
