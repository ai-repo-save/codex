use codex_extension_api::PromptFragment;
use codex_extension_api::PromptSlot;
use codex_protocol::models::ResponseItem;

#[derive(Clone, Debug)]
struct RewindContributionFragment {
    slot: PromptSlot,
    text: String,
}

#[derive(Clone, Debug, Default)]
pub(super) struct RewindContributions {
    fragments: Vec<RewindContributionFragment>,
}

impl RewindContributions {
    pub(super) fn from_prompt_fragments(fragments: impl IntoIterator<Item = PromptFragment>) -> Self {
        let fragments = fragments
            .into_iter()
            .map(|fragment| RewindContributionFragment {
                slot: fragment.slot(),
                text: fragment.text().to_string(),
            })
            .collect();
        Self { fragments }
    }

    pub(super) fn into_response_items(self) -> Vec<ResponseItem> {
        let mut developer_sections = Vec::new();
        let mut contextual_user_sections = Vec::new();
        let mut separate_developer_sections = Vec::new();

        for fragment in self.fragments {
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

        let mut contribution_items = Vec::with_capacity(3);
        if let Some(developer_message) =
            crate::context_manager::updates::build_developer_update_item(developer_sections)
        {
            contribution_items.push(developer_message);
        }
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
        contribution_items
    }
}
