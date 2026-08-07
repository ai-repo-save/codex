use std::collections::HashMap;

use codex_config::HookHandlerConfig;
use codex_config::HookStateToml;
use codex_config::MatcherGroup;
use codex_config::PromptHookFilterConfig;
use codex_protocol::openai_models::ReasoningEffort;
use codex_protocol::protocol::HookEventName;
use codex_protocol::protocol::HookHandlerType;
use codex_protocol::protocol::HookSource;
use codex_protocol::protocol::HookTrustStatus;
use codex_utils_absolute_path::AbsolutePathBuf;
use codex_utils_absolute_path::test_support::PathBufExt;
use codex_utils_absolute_path::test_support::test_path_buf;
use pretty_assertions::assert_eq;

use super::super::super::ConfiguredHandler;
use super::super::super::ConfiguredHandlerKind;
use super::super::super::ConfiguredPromptFilter;
use super::super::super::HookListEntry;
use super::super::HookHandlerSource;
use super::super::append_matcher_groups;

fn source_path() -> AbsolutePathBuf {
    test_path_buf("/tmp/hooks.json").abs()
}

fn hook_source() -> HookSource {
    HookSource::System
}

fn hook_handler_source<'a>(
    path: &'a AbsolutePathBuf,
    hook_states: &'a HashMap<String, HookStateToml>,
) -> HookHandlerSource<'a> {
    HookHandlerSource {
        path,
        key_source: path.display().to_string(),
        source: hook_source(),
        is_managed: true,
        bypass_hook_trust: false,
        hook_states,
        env: HashMap::new(),
        plugin_id: None,
    }
}

fn prompt_handler(fail_closed: bool) -> HookHandlerConfig {
    HookHandlerConfig::Prompt {
        id: None,
        prompt: "Review $$ARGUMENTS".to_string(),
        filter: None,
        model: Some("gpt-test".to_string()),
        reasoning_effort: Some(ReasoningEffort::High),
        timeout_sec: None,
        status_message: Some("Reviewing tool use".to_string()),
        fail_closed,
    }
}

fn prompt_handler_with_filter(filter: PromptHookFilterConfig) -> HookHandlerConfig {
    HookHandlerConfig::Prompt {
        id: None,
        prompt: "Review $$ARGUMENTS".to_string(),
        filter: Some(filter),
        model: None,
        reasoning_effort: None,
        timeout_sec: None,
        status_message: None,
        fail_closed: false,
    }
}

#[test]
fn supported_prompt_hooks_enter_runtime_and_list_metadata() {
    let mut handlers = Vec::new();
    let mut hook_entries = Vec::new();
    let mut warnings = Vec::new();
    let mut display_order = 0;
    let source_path = source_path();
    let hook_states = std::collections::HashMap::new();

    for event_name in [
        HookEventName::PreToolUse,
        HookEventName::PermissionRequest,
        HookEventName::PreCompact,
        HookEventName::SessionStart,
        HookEventName::UserPromptSubmit,
        HookEventName::SubagentStart,
    ] {
        append_matcher_groups(
            &mut handlers,
            &mut hook_entries,
            &mut warnings,
            &mut display_order,
            &hook_handler_source(&source_path, &hook_states),
            event_name,
            vec![MatcherGroup {
                matcher: Some("^Bash$".to_string()),
                hooks: vec![prompt_handler(/*fail_closed*/ true)],
            }],
        );
    }

    assert_eq!(warnings, Vec::<String>::new());
    assert_eq!(handlers.len(), 6);
    assert_eq!(hook_entries.len(), 6);
    for (handler, hook_entry) in handlers.iter().zip(&hook_entries) {
        assert_eq!(handler.event_name, hook_entry.event_name);
        assert_eq!(
            handler.kind,
            ConfiguredHandlerKind::Prompt {
                prompt: "Review $$ARGUMENTS".to_string(),
                filter: None,
                model: Some("gpt-test".to_string()),
                reasoning_effort: Some(ReasoningEffort::High),
                timeout_sec: 30,
                fail_closed: true,
            }
        );
        assert_eq!(hook_entry.handler_type, HookHandlerType::Prompt);
        assert_eq!(hook_entry.command, None);
        assert_eq!(hook_entry.prompt.as_deref(), Some("Review $$ARGUMENTS"));
        assert_eq!(hook_entry.model.as_deref(), Some("gpt-test"));
        assert_eq!(hook_entry.reasoning_effort, Some(ReasoningEffort::High));
        assert_eq!(hook_entry.timeout_sec, 30);
        assert_eq!(hook_entry.fail_closed, Some(true));
        assert_eq!(hook_entry.trust_status, HookTrustStatus::Managed);
    }
    assert_ne!(hook_entries[0].current_hash, hook_entries[1].current_hash);
    assert_ne!(hook_entries[1].current_hash, hook_entries[2].current_hash);
}

#[test]
fn prompt_reasoning_effort_changes_normalized_hook_hash() {
    let mut handlers = Vec::new();
    let mut hook_entries = Vec::new();
    let mut warnings = Vec::new();
    let mut display_order = 0;
    let source_path = source_path();
    let hook_states = std::collections::HashMap::new();
    let mut low_effort = prompt_handler(/*fail_closed*/ false);
    let HookHandlerConfig::Prompt {
        reasoning_effort, ..
    } = &mut low_effort
    else {
        unreachable!("prompt handler helper must return a prompt")
    };
    *reasoning_effort = Some(ReasoningEffort::Low);

    append_matcher_groups(
        &mut handlers,
        &mut hook_entries,
        &mut warnings,
        &mut display_order,
        &hook_handler_source(&source_path, &hook_states),
        HookEventName::PreToolUse,
        vec![
            MatcherGroup {
                matcher: None,
                hooks: vec![prompt_handler(/*fail_closed*/ false)],
            },
            MatcherGroup {
                matcher: None,
                hooks: vec![low_effort],
            },
        ],
    );

    assert_eq!(warnings, Vec::<String>::new());
    assert_ne!(hook_entries[0].current_hash, hook_entries[1].current_hash);
}

#[test]
fn prompt_filter_normalization_uses_default_timeout_and_hashes_normalized_values() {
    let mut handlers = Vec::new();
    let mut hook_entries = Vec::new();
    let mut warnings = Vec::new();
    let mut display_order = 0;
    let source_path = source_path();
    let hook_states = std::collections::HashMap::new();

    append_matcher_groups(
        &mut handlers,
        &mut hook_entries,
        &mut warnings,
        &mut display_order,
        &hook_handler_source(&source_path, &hook_states),
        HookEventName::PreToolUse,
        vec![MatcherGroup {
            matcher: None,
            hooks: vec![
                prompt_handler_with_filter(PromptHookFilterConfig {
                    command: "echo filter".to_string(),
                    command_windows: None,
                    timeout_sec: None,
                }),
                prompt_handler_with_filter(PromptHookFilterConfig {
                    command: "echo filter".to_string(),
                    command_windows: None,
                    timeout_sec: Some(5),
                }),
                prompt_handler_with_filter(PromptHookFilterConfig {
                    command: "echo another-filter".to_string(),
                    command_windows: None,
                    timeout_sec: Some(5),
                }),
                prompt_handler_with_filter(PromptHookFilterConfig {
                    command: "echo filter".to_string(),
                    command_windows: None,
                    timeout_sec: Some(6),
                }),
            ],
        }],
    );

    assert_eq!(warnings, Vec::<String>::new());
    assert_eq!(handlers.len(), 4);
    assert_eq!(hook_entries.len(), 4);
    for handler in &handlers[..2] {
        assert_eq!(
            handler.kind,
            ConfiguredHandlerKind::Prompt {
                prompt: "Review $$ARGUMENTS".to_string(),
                filter: Some(ConfiguredPromptFilter {
                    command: "echo filter".to_string(),
                    timeout_sec: 5,
                }),
                model: None,
                reasoning_effort: None,
                timeout_sec: 30,
                fail_closed: false,
            }
        );
    }
    assert_eq!(hook_entries[0].current_hash, hook_entries[1].current_hash);
    assert_ne!(hook_entries[0].current_hash, hook_entries[2].current_hash);
    assert_ne!(hook_entries[0].current_hash, hook_entries[3].current_hash);
}

#[test]
fn invalid_prompt_filter_timeout_skips_only_that_handler() {
    let mut handlers = Vec::new();
    let mut hook_entries = Vec::new();
    let mut warnings = Vec::new();
    let mut display_order = 0;
    let source_path = source_path();
    let hook_states = std::collections::HashMap::new();

    append_matcher_groups(
        &mut handlers,
        &mut hook_entries,
        &mut warnings,
        &mut display_order,
        &hook_handler_source(&source_path, &hook_states),
        HookEventName::PreToolUse,
        vec![MatcherGroup {
            matcher: None,
            hooks: vec![
                prompt_handler_with_filter(PromptHookFilterConfig {
                    command: "echo invalid".to_string(),
                    command_windows: None,
                    timeout_sec: Some(61),
                }),
                prompt_handler_with_filter(PromptHookFilterConfig {
                    command: "echo valid".to_string(),
                    command_windows: None,
                    timeout_sec: Some(1),
                }),
            ],
        }],
    );

    assert_eq!(handlers.len(), 1);
    assert_eq!(hook_entries.len(), 1);
    assert_eq!(
        handlers[0].kind,
        ConfiguredHandlerKind::Prompt {
            prompt: "Review $$ARGUMENTS".to_string(),
            filter: Some(ConfiguredPromptFilter {
                command: "echo valid".to_string(),
                timeout_sec: 1,
            }),
            model: None,
            reasoning_effort: None,
            timeout_sec: 30,
            fail_closed: false,
        }
    );
}

#[test]
fn prompt_filter_uses_platform_command_override() {
    let mut handlers = Vec::new();
    let mut warnings = Vec::new();
    let mut display_order = 0;
    let source_path = source_path();
    let hook_states = std::collections::HashMap::new();

    append_matcher_groups(
        &mut handlers,
        &mut Vec::new(),
        &mut warnings,
        &mut display_order,
        &hook_handler_source(&source_path, &hook_states),
        HookEventName::PreToolUse,
        vec![MatcherGroup {
            matcher: None,
            hooks: vec![prompt_handler_with_filter(PromptHookFilterConfig {
                command: "echo unix".to_string(),
                command_windows: Some("echo windows".to_string()),
                timeout_sec: None,
            })],
        }],
    );

    assert_eq!(warnings, Vec::<String>::new());
    let ConfiguredHandlerKind::Prompt {
        filter: Some(filter),
        ..
    } = &handlers[0].kind
    else {
        panic!("prompt filter handler")
    };
    assert_eq!(
        filter.command,
        if cfg!(windows) {
            "echo windows"
        } else {
            "echo unix"
        }
    );
    assert_eq!(filter.timeout_sec, 5);
}

#[test]
fn invalid_prompt_handlers_are_skipped_without_dropping_valid_siblings() {
    let mut handlers = Vec::new();
    let mut hook_entries = Vec::new();
    let mut warnings = Vec::new();
    let mut display_order = 0;
    let source_path = source_path();
    let hook_states = std::collections::HashMap::new();
    let oversized_prompt = "é".repeat(8 * 1024 + 1);

    append_matcher_groups(
        &mut handlers,
        &mut hook_entries,
        &mut warnings,
        &mut display_order,
        &hook_handler_source(&source_path, &hook_states),
        HookEventName::PreToolUse,
        vec![MatcherGroup {
            matcher: None,
            hooks: vec![
                HookHandlerConfig::Prompt {
                    id: None,
                    prompt: String::new(),
                    filter: None,
                    model: None,
                    reasoning_effort: None,
                    timeout_sec: None,
                    status_message: None,
                    fail_closed: false,
                },
                HookHandlerConfig::Prompt {
                    id: None,
                    prompt: oversized_prompt,
                    filter: None,
                    model: None,
                    reasoning_effort: None,
                    timeout_sec: None,
                    status_message: None,
                    fail_closed: false,
                },
                HookHandlerConfig::Prompt {
                    id: None,
                    prompt: "Review $$ARGUMENTS".to_string(),
                    filter: None,
                    model: Some(" gpt-test".to_string()),
                    reasoning_effort: None,
                    timeout_sec: None,
                    status_message: None,
                    fail_closed: false,
                },
                HookHandlerConfig::Prompt {
                    id: None,
                    prompt: "Review $$ARGUMENTS".to_string(),
                    filter: None,
                    model: None,
                    reasoning_effort: None,
                    timeout_sec: Some(601),
                    status_message: None,
                    fail_closed: false,
                },
                HookHandlerConfig::Prompt {
                    id: None,
                    prompt: "Review $$ARGUMENTS".to_string(),
                    filter: None,
                    model: Some("gpt-test".to_string()),
                    reasoning_effort: Some(ReasoningEffort::High),
                    timeout_sec: Some(1),
                    status_message: Some("  ".to_string()),
                    fail_closed: true,
                },
            ],
        }],
    );

    assert_eq!(warnings.len(), 4);
    assert_eq!(handlers.len(), 1);
    assert_eq!(handlers[0].display_order, 0);
    assert_eq!(
        handlers[0].kind,
        ConfiguredHandlerKind::Prompt {
            prompt: "Review $$ARGUMENTS".to_string(),
            filter: None,
            model: Some("gpt-test".to_string()),
            reasoning_effort: Some(ReasoningEffort::High),
            timeout_sec: 1,
            fail_closed: true,
        }
    );
    assert_eq!(handlers[0].status_message, None);
    assert_eq!(hook_entries.len(), 1);
    assert_eq!(hook_entries[0].display_order, 0);
    assert_eq!(
        hook_entries[0].prompt.as_deref(),
        Some("Review $$ARGUMENTS")
    );
    assert_eq!(hook_entries[0].model.as_deref(), Some("gpt-test"));
    assert_eq!(
        hook_entries[0].reasoning_effort,
        Some(ReasoningEffort::High)
    );
    assert_eq!(hook_entries[0].status_message, None);
    assert_eq!(hook_entries[0].timeout_sec, 1);
    assert_eq!(hook_entries[0].fail_closed, Some(true));
}

#[test]
fn unsupported_events_skip_each_prompt_handler_without_lifecycle_or_metadata() {
    let mut handlers = Vec::new();
    let mut hook_entries = Vec::new();
    let mut warnings = Vec::new();
    let mut display_order = 0;
    let source_path = source_path();
    let hook_states = std::collections::HashMap::new();

    for event_name in [
        HookEventName::PostToolUse,
        HookEventName::PostCompact,
        HookEventName::SubagentStop,
        HookEventName::Stop,
    ] {
        append_matcher_groups(
            &mut handlers,
            &mut hook_entries,
            &mut warnings,
            &mut display_order,
            &hook_handler_source(&source_path, &hook_states),
            event_name,
            vec![MatcherGroup {
                matcher: None,
                hooks: vec![
                    prompt_handler(/*fail_closed*/ false),
                    prompt_handler(/*fail_closed*/ true),
                ],
            }],
        );
    }

    assert_eq!(handlers, Vec::<ConfiguredHandler>::new());
    assert_eq!(hook_entries, Vec::<HookListEntry>::new());
    assert_eq!(display_order, 0);
    assert_eq!(warnings.len(), 8);
    for event_name in ["PostToolUse", "PostCompact", "SubagentStop", "Stop"] {
        assert_eq!(
            warnings
                .iter()
                .filter(|warning| warning
                    .contains(&format!("prompt hooks are not supported for {event_name}")))
                .count(),
            2
        );
    }
}
