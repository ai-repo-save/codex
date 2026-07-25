use codex_extension_api::PromptFragment;
use codex_extension_api::PromptSlot;
use codex_protocol::models::ResponseItem;
use codex_utils_output_truncation::TruncationPolicy;
use codex_utils_output_truncation::approx_token_count;
use codex_utils_output_truncation::truncate_text;

const MAX_REWIND_CONTRIBUTION_FRAGMENTS: usize = 32;
const MAX_REWIND_CONTRIBUTION_TOKENS: usize = 10_000;

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
        let mut remaining_tokens = MAX_REWIND_CONTRIBUTION_TOKENS;
        let mut bounded_fragments = Vec::new();

        for fragment in fragments
            .into_iter()
            .take(MAX_REWIND_CONTRIBUTION_FRAGMENTS)
        {
            if remaining_tokens == 0 {
                break;
            }
            let text = fragment.text();
            let token_count = approx_token_count(text);
            let text = if token_count > remaining_tokens {
                truncate_text(text, TruncationPolicy::Tokens(remaining_tokens))
            } else {
                text.to_string()
            };
            remaining_tokens =
                remaining_tokens.saturating_sub(approx_token_count(text.as_str()));
            bounded_fragments.push(RewindContributionFragment {
                slot: fragment.slot(),
                text,
            });
        }

        Self {
            fragments: bounded_fragments,
        }
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
