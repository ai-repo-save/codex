//! Fork-owned prompt/filter discovery normalization.
//!
//! Keeps the large Prompt-handler validation and metadata wiring out of the
//! upstream-shared `discovery.rs` match loop so sync conflicts stay at a thin
//! call site.

use codex_config::HookHandlerConfig;
use codex_config::MatcherGroup;
use codex_config::PromptHookFilterConfig;
use codex_protocol::openai_models::ReasoningEffort;
use codex_protocol::protocol::HookEventName;
use codex_protocol::protocol::HookHandlerType;
use codex_protocol::protocol::HookTrustStatus;

use super::super::ConfiguredHandler;
use super::super::ConfiguredHandlerKind;
use super::super::ConfiguredPromptFilter;
use super::super::HookListEntry;
use super::super::dispatcher;
use super::super::prompt_runner;
use super::HookHandlerSource;
use super::hook_enabled;
use super::hook_hash;
use super::hook_trust_status;
use super::hook_trusted_hash;

/// Validates, normalizes, and records one prompt hook handler.
///
/// Returns `true` when the handler was accepted into metadata (and possibly the
/// runtime list). Returns `false` when the handler was skipped after a warning.
pub(super) fn append_prompt_handler(
    handlers: &mut Vec<ConfiguredHandler>,
    hook_entries: &mut Vec<HookListEntry>,
    warnings: &mut Vec<String>,
    display_order: &mut i64,
    source: &HookHandlerSource<'_>,
    event_name: HookEventName,
    group: &MatcherGroup,
    group_index: usize,
    handler_index: usize,
    matcher: Option<&str>,
    prompt: String,
    filter: Option<PromptHookFilterConfig>,
    model: Option<String>,
    reasoning_effort: Option<ReasoningEffort>,
    timeout_sec: Option<u64>,
    status_message: Option<String>,
    fail_closed: bool,
) -> bool {
    if !prompt_runner::supports_event(event_name) {
        warnings.push(format!(
            "skipping prompt hook in {}: prompt hooks are not supported for {}",
            source.path.display(),
            dispatcher::hook_event_name_label(event_name)
        ));
        return false;
    }
    if prompt.trim().is_empty() || prompt.len() > 16 * 1024 {
        warnings.push(format!(
            "skipping invalid hook prompt in {}",
            source.path.display()
        ));
        return false;
    }
    if model
        .as_deref()
        .is_some_and(|model| model.trim().is_empty() || model.trim() != model)
    {
        warnings.push(format!(
            "skipping invalid prompt hook model in {}",
            source.path.display()
        ));
        return false;
    }
    let filter = match filter {
        Some(filter) => {
            let Some(filter) = normalize_prompt_filter(filter, source, warnings) else {
                return false;
            };
            Some(filter)
        }
        None => None,
    };
    if timeout_sec.is_some_and(|timeout_sec| !(1..=600).contains(&timeout_sec)) {
        warnings.push(format!(
            "skipping invalid prompt hook timeout in {}",
            source.path.display()
        ));
        return false;
    }
    let timeout_sec = timeout_sec.unwrap_or(30);
    let status_message = status_message.filter(|status_message| !status_message.trim().is_empty());
    let normalized_handler = HookHandlerConfig::Prompt {
        prompt: prompt.clone(),
        filter: filter.as_ref().map(|filter| PromptHookFilterConfig {
            command: filter.command.clone(),
            command_windows: None,
            timeout_sec: Some(filter.timeout_sec),
        }),
        model: model.clone(),
        reasoning_effort: reasoning_effort.clone(),
        timeout_sec: Some(timeout_sec),
        status_message: status_message.clone(),
        fail_closed,
    };
    let current_hash = hook_hash(event_name, matcher, group, normalized_handler);
    let effective_filter = filter.map(|filter| ConfiguredPromptFilter {
        command: source
            .env
            .iter()
            .fold(filter.command, |command, (key, value)| {
                command.replace(&format!("${{{key}}}"), value)
            }),
        timeout_sec: filter.timeout_sec,
    });
    let key = crate::hook_key(&source.key_source, event_name, group_index, handler_index);
    let state = source.hook_states.get(&key);
    let enabled = hook_enabled(source.is_managed, state);
    let trusted_hash = hook_trusted_hash(source.is_managed, state);
    let trust_status = hook_trust_status(source.is_managed, &current_hash, trusted_hash);
    hook_entries.push(HookListEntry {
        key,
        event_name,
        handler_type: HookHandlerType::Prompt,
        matcher: matcher.map(ToOwned::to_owned),
        command: None,
        prompt: Some(prompt.clone()),
        model: model.clone(),
        reasoning_effort: reasoning_effort.clone(),
        filter: effective_filter
            .as_ref()
            .map(|filter| PromptHookFilterConfig {
                command: filter.command.clone(),
                command_windows: None,
                timeout_sec: Some(filter.timeout_sec),
            }),
        fail_closed: Some(fail_closed),
        timeout_sec,
        status_message: status_message.clone(),
        additional_context_limit: None,
        source_path: source.path.clone(),
        source: source.source,
        plugin_id: source.plugin_id.clone(),
        display_order: *display_order,
        enabled,
        is_managed: source.is_managed,
        current_hash,
        trust_status,
    });
    if enabled
        && (source.bypass_hook_trust
            || matches!(
                trust_status,
                HookTrustStatus::Managed | HookTrustStatus::Trusted
            ))
    {
        handlers.push(ConfiguredHandler {
            event_name,
            matcher: matcher.map(ToOwned::to_owned),
            kind: ConfiguredHandlerKind::Prompt {
                prompt,
                filter: effective_filter,
                model,
                reasoning_effort,
                timeout_sec,
                fail_closed,
            },
            status_message,
            additional_context_limit: Default::default(),
            source_path: source.path.clone(),
            source: source.source,
            display_order: *display_order,
            env: source.env.clone(),
        });
    }
    *display_order += 1;
    true
}

fn normalize_prompt_filter(
    filter: PromptHookFilterConfig,
    source: &HookHandlerSource<'_>,
    warnings: &mut Vec<String>,
) -> Option<ConfiguredPromptFilter> {
    let command = if cfg!(windows) {
        filter.command_windows.unwrap_or(filter.command)
    } else {
        filter.command
    };
    if command.trim().is_empty() {
        warnings.push(format!(
            "skipping prompt hook with empty filter command in {}",
            source.path.display()
        ));
        return None;
    }
    if filter
        .timeout_sec
        .is_some_and(|timeout_sec| !(1..=60).contains(&timeout_sec))
    {
        warnings.push(format!(
            "skipping prompt hook with invalid filter timeout in {}",
            source.path.display()
        ));
        return None;
    }
    Some(ConfiguredPromptFilter {
        command,
        timeout_sec: filter.timeout_sec.unwrap_or(5),
    })
}

#[cfg(test)]
#[path = "prompt_handler_discovery_tests.rs"]
mod tests;
