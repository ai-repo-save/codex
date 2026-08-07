mod config_rules;
mod declarations;
mod engine;
pub(crate) mod events;
mod legacy_notify;
mod output_spill;
mod registry;
mod schema;
mod types;

use codex_protocol::protocol::HookEventName;

pub use config_rules::hook_states_from_stack;
pub use declarations::PluginHookDeclaration;
pub use declarations::plugin_hook_declarations;
pub use engine::HookListEntry;
pub use engine::PromptHookRequest;
pub use engine::PromptHookRunner;
pub use events::common::SubagentHookContext;
/// Hook event names as they appear in hooks JSON and config files.
pub const HOOK_EVENT_NAMES: [&str; 11] = [
    "PreToolUse",
    "PermissionRequest",
    "PostToolUse",
    "PreCompact",
    "PostCompact",
    "SessionStart",
    "SessionEnd",
    "UserPromptSubmit",
    "SubagentStart",
    "SubagentStop",
    "Stop",
];

/// Hook event names whose matcher fields are meaningful during dispatch.
///
/// Other events can appear in hooks JSON, but Codex ignores their matcher
/// fields because those events do not dispatch against a tool, compaction
/// trigger, session-start source, or session-end reason.
pub const HOOK_EVENT_NAMES_WITH_MATCHERS: [&str; 9] = [
    "PreToolUse",
    "PermissionRequest",
    "PostToolUse",
    "PreCompact",
    "PostCompact",
    "SessionStart",
    "SessionEnd",
    "SubagentStart",
    "SubagentStop",
];

pub use events::compact::PostCompactRequest;
pub use events::compact::PreCompactOutcome;
pub use events::compact::PreCompactRequest;
pub use events::compact::StatelessHookOutcome;
pub use events::permission_request::PermissionRequestDecision;
pub use events::permission_request::PermissionRequestOutcome;
pub use events::permission_request::PermissionRequestRequest;
pub use events::post_tool_use::PostToolUseOutcome;
pub use events::post_tool_use::PostToolUseRequest;
pub use events::pre_tool_use::PreToolUseOutcome;
pub use events::pre_tool_use::PreToolUseRequest;
pub use events::session_end::SessionEndOutcome;
pub use events::session_end::SessionEndRequest;
pub use events::session_start::SessionStartOutcome;
pub use events::session_start::SessionStartRequest;
pub use events::session_start::SessionStartSource;
pub use events::session_start::StartHookTarget;
pub use events::stop::StopHookTarget;
pub use events::stop::StopOutcome;
pub use events::stop::StopRequest;
pub use events::user_prompt_submit::UserPromptSubmitOutcome;
pub use events::user_prompt_submit::UserPromptSubmitRequest;
pub use legacy_notify::legacy_notify_json;
pub use legacy_notify::notify_hook;
pub use registry::HookListOutcome;
pub use registry::Hooks;
pub use registry::HooksConfig;
pub use registry::command_from_argv;
pub use registry::list_hooks;
pub use schema::write_schema_fixtures;
pub use types::Hook;
pub use types::HookEvent;
pub use types::HookEventAfterAgent;
pub use types::HookPayload;
pub use types::HookResponse;
pub use types::HookResult;

/// Returns the hook event label used in persisted hook-state keys.
pub fn hook_event_key_label(event_name: HookEventName) -> &'static str {
    match event_name {
        HookEventName::PreToolUse => "pre_tool_use",
        HookEventName::PermissionRequest => "permission_request",
        HookEventName::PostToolUse => "post_tool_use",
        HookEventName::PreCompact => "pre_compact",
        HookEventName::PostCompact => "post_compact",
        HookEventName::SessionStart => "session_start",
        HookEventName::SessionEnd => "session_end",
        HookEventName::UserPromptSubmit => "user_prompt_submit",
        HookEventName::SubagentStart => "subagent_start",
        HookEventName::SubagentStop => "subagent_stop",
        HookEventName::Stop => "stop",
    }
}

/// Builds the legacy positional config-state key for one discovered hook handler.
pub fn hook_key(
    key_source: &str,
    event_name: HookEventName,
    group_index: usize,
    handler_index: usize,
) -> String {
    format!(
        "{key_source}:{}:{group_index}:{handler_index}",
        hook_event_key_label(event_name)
    )
}

/// Builds a durable config-state key when the handler declares a stable `id`.
pub fn durable_hook_key(key_source: &str, event_name: HookEventName, id: &str) -> String {
    format!("{key_source}:{}:{id}", hook_event_key_label(event_name))
}

/// Returns true when `id` is a non-empty durable hook identifier.
///
/// Allowed characters: ASCII letters, digits, `.`, `_`, and `-`. Colon is rejected because it
/// separates key segments.
pub fn is_valid_hook_id(id: &str) -> bool {
    !id.is_empty()
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

/// Resolved persisted key plus the legacy positional key used for dual-read migration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedHookKey {
    pub key: String,
    pub legacy_key: String,
}

impl ResolvedHookKey {
    pub fn lookup<'a, V>(&self, states: &'a std::collections::HashMap<String, V>) -> Option<&'a V> {
        states.get(&self.key).or_else(|| {
            if self.key == self.legacy_key {
                None
            } else {
                states.get(&self.legacy_key)
            }
        })
    }
}

/// Chooses a durable or positional hook-state key and tracks per-source event id uniqueness.
pub fn resolve_hook_key(
    key_source: &str,
    event_name: HookEventName,
    group_index: usize,
    handler_index: usize,
    configured_id: Option<&str>,
    seen_ids: &mut std::collections::HashSet<String>,
) -> Result<ResolvedHookKey, String> {
    let legacy_key = hook_key(key_source, event_name, group_index, handler_index);
    let Some(raw_id) = configured_id.map(str::trim).filter(|id| !id.is_empty()) else {
        return Ok(ResolvedHookKey {
            key: legacy_key.clone(),
            legacy_key,
        });
    };
    if !is_valid_hook_id(raw_id) {
        return Err(format!(
            "invalid hook id {raw_id:?}: use letters, digits, '.', '_', or '-'"
        ));
    }
    if !seen_ids.insert(raw_id.to_string()) {
        return Err(format!(
            "duplicate hook id {raw_id:?} for event {}; falling back to positional key",
            hook_event_key_label(event_name)
        ));
    }
    Ok(ResolvedHookKey {
        key: durable_hook_key(key_source, event_name, raw_id),
        legacy_key,
    })
}

#[cfg(test)]
mod hook_key_tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use std::collections::HashMap;
    use std::collections::HashSet;

    #[test]
    fn resolve_hook_key_uses_durable_id_when_valid() {
        let mut seen = HashSet::new();
        let resolved = resolve_hook_key(
            "/tmp/config.toml",
            HookEventName::PreToolUse,
            1,
            0,
            Some("grok-build-0.1"),
            &mut seen,
        )
        .expect("id should resolve");
        assert_eq!(resolved.key, "/tmp/config.toml:pre_tool_use:grok-build-0.1");
        assert_eq!(resolved.legacy_key, "/tmp/config.toml:pre_tool_use:1:0");
    }

    #[test]
    fn resolve_hook_key_rejects_duplicate_ids() {
        let mut seen = HashSet::from(["dup".to_string()]);
        let err = resolve_hook_key(
            "/tmp/config.toml",
            HookEventName::PreToolUse,
            0,
            0,
            Some("dup"),
            &mut seen,
        )
        .expect_err("duplicate id should fail");
        assert!(err.contains("duplicate hook id"));
    }

    #[test]
    fn resolved_hook_key_prefers_durable_lookup() {
        let resolved = ResolvedHookKey {
            key: "/tmp/config.toml:pre_tool_use:named".to_string(),
            legacy_key: "/tmp/config.toml:pre_tool_use:0:0".to_string(),
        };
        let states = HashMap::from([
            (
                resolved.legacy_key.clone(),
                HookStateTomlProbe { enabled: true },
            ),
            (resolved.key.clone(), HookStateTomlProbe { enabled: false }),
        ]);
        assert_eq!(
            resolved.lookup(&states).map(|state| state.enabled),
            Some(false)
        );
    }

    #[derive(Debug, PartialEq, Eq)]
    struct HookStateTomlProbe {
        enabled: bool,
    }
}
