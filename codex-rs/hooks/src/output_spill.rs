use codex_protocol::ThreadId;
use codex_protocol::items::HookPromptFragment;
use codex_utils_absolute_path::AbsolutePathBuf;
use codex_utils_output_truncation::TruncationPolicy;
use codex_utils_output_truncation::approx_bytes_for_tokens;
use codex_utils_output_truncation::approx_token_count;
use codex_utils_output_truncation::formatted_truncate_text;
use tokio::fs;
use tracing::warn;
use uuid::Uuid;

const HOOK_OUTPUTS_DIR: &str = "hook_outputs";
pub(crate) const DEFAULT_HOOK_OUTPUT_TOKEN_LIMIT: usize = 2_500;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct AdditionalContextLimit {
    token_limit: usize,
}

impl AdditionalContextLimit {
    pub(crate) fn from_config(value: Option<usize>) -> Self {
        Self {
            token_limit: value.unwrap_or(DEFAULT_HOOK_OUTPUT_TOKEN_LIMIT),
        }
    }
}

impl Default for AdditionalContextLimit {
    fn default() -> Self {
        Self::from_config(/*value*/ None)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AdditionalContext {
    pub text: String,
    pub limit: AdditionalContextLimit,
}

#[derive(Clone)]
pub(crate) struct HookOutputSpiller {
    output_dir: AbsolutePathBuf,
}

impl HookOutputSpiller {
    pub(crate) fn new() -> Self {
        Self {
            output_dir: AbsolutePathBuf::resolve_path_against_base(std::env::temp_dir(), "/")
                .join(HOOK_OUTPUTS_DIR),
        }
    }

    /// Keeps hook text within the model-visible hook-output budget.
    ///
    /// Oversized text is written in full under the OS temp directory at
    /// `<temp_dir>/hook_outputs/<thread_id>/`
    /// and replaced with the same head/tail preview style used for other truncated
    /// output, plus a path back to the preserved full text when the configured
    /// budget can contain it.
    pub(crate) async fn maybe_spill_text(&self, thread_id: ThreadId, text: String) -> String {
        self.maybe_spill_text_with_limit(thread_id, text, AdditionalContextLimit::default())
            .await
    }

    async fn maybe_spill_text_with_limit(
        &self,
        thread_id: ThreadId,
        text: String,
        limit: AdditionalContextLimit,
    ) -> String {
        let token_limit = limit.token_limit;
        if token_limit == 0 || approx_token_count(&text) <= token_limit {
            return text;
        }

        let path = hook_output_path(&self.output_dir, thread_id);
        if let Some(parent) = path.parent()
            && let Err(err) = fs::create_dir_all(parent.as_ref()).await
        {
            warn!(
                "failed to create hook output directory {}: {err}",
                parent.display()
            );
            return bounded_formatted_preview(&text, "", token_limit);
        }

        if let Err(err) = fs::write(path.as_ref(), &text).await {
            warn!("failed to write hook output {}: {err}", path.display());
            return bounded_formatted_preview(&text, "", token_limit);
        }

        spilled_hook_output_preview(&text, &path, token_limit)
    }

    pub(crate) async fn maybe_spill_additional_contexts(
        &self,
        thread_id: ThreadId,
        contexts: Vec<AdditionalContext>,
    ) -> Vec<String> {
        let mut spilled = Vec::with_capacity(contexts.len());
        for context in contexts {
            spilled.push(
                self.maybe_spill_text_with_limit(thread_id, context.text, context.limit)
                    .await,
            );
        }
        spilled
    }

    pub(crate) async fn maybe_spill_prompt_fragments(
        &self,
        thread_id: ThreadId,
        fragments: Vec<HookPromptFragment>,
    ) -> Vec<HookPromptFragment> {
        let mut spilled = Vec::with_capacity(fragments.len());
        for fragment in fragments {
            spilled.push(HookPromptFragment {
                text: self.maybe_spill_text(thread_id, fragment.text).await,
                hook_run_id: fragment.hook_run_id,
            });
        }
        spilled
    }
}

fn hook_output_path(output_dir: &AbsolutePathBuf, thread_id: ThreadId) -> AbsolutePathBuf {
    output_dir
        .join(thread_id.to_string())
        .join(format!("{}.txt", Uuid::new_v4()))
}

/// Builds the model-visible replacement for a spilled hook output.
fn spilled_hook_output_preview(text: &str, path: &AbsolutePathBuf, token_limit: usize) -> String {
    let footer = format!("\n\nFull hook output saved to: {}", path.display());
    bounded_formatted_preview(text, &footer, token_limit)
}

fn bounded_formatted_preview(text: &str, suffix: &str, token_limit: usize) -> String {
    if !suffix.is_empty() && approx_token_count(suffix) >= token_limit {
        return suffix.trim_start().to_string();
    }

    let mut minimum_body_tokens = 0;
    let mut maximum_body_tokens = token_limit;
    let mut best_preview = None;

    while minimum_body_tokens <= maximum_body_tokens {
        let body_tokens = minimum_body_tokens + (maximum_body_tokens - minimum_body_tokens) / 2;
        let preview = format!(
            "{}{suffix}",
            formatted_truncate_text(text, TruncationPolicy::Tokens(body_tokens))
        );
        if approx_token_count(&preview) <= token_limit {
            best_preview = Some(preview);
            minimum_body_tokens = body_tokens.saturating_add(1);
        } else if body_tokens == 0 {
            break;
        } else {
            maximum_body_tokens = body_tokens - 1;
        }
    }

    best_preview.unwrap_or_else(|| {
        let fallback = if suffix.is_empty() {
            formatted_truncate_text(text, TruncationPolicy::Tokens(/*tokens*/ 0))
        } else {
            suffix.trim_start().to_string()
        };
        truncate_to_approx_token_limit(&fallback, token_limit)
    })
}

fn truncate_to_approx_token_limit(text: &str, token_limit: usize) -> String {
    let byte_limit = approx_bytes_for_tokens(token_limit);
    if text.len() <= byte_limit {
        return text.to_string();
    }

    let boundary = (0..=byte_limit)
        .rev()
        .find(|index| text.is_char_boundary(*index))
        .unwrap_or(0);
    text[..boundary].to_string()
}

#[cfg(test)]
#[path = "output_spill_tests.rs"]
mod tests;
