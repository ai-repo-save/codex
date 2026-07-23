use pretty_assertions::assert_eq;

use std::collections::BTreeMap;

use codex_protocol::openai_models::ReasoningEffort;

use super::HookEventsToml;
use super::HookHandlerConfig;
use super::HooksFile;
use super::HooksToml;
use super::ManagedHooksRequirementsToml;
use super::MatcherGroup;
use super::PromptHookFilterConfig;

#[test]
fn hooks_file_deserializes_existing_json_shape() {
    let parsed: HooksFile = serde_json::from_str(
        r#"{
  "description": "Optional stop-time review gate for Codex Companion.",
  "hooks": {
    "PreToolUse": [
      {
        "matcher": "^Bash$",
        "hooks": [
          {
            "type": "command",
            "command": "python3 /tmp/pre.py",
            "timeout": 10,
            "statusMessage": "checking",
            "additionalContextLimit": 4096
          }
        ]
      }
    ]
  }
}"#,
    )
    .expect("hooks.json should deserialize");

    assert_eq!(
        parsed,
        HooksFile {
            description: Some("Optional stop-time review gate for Codex Companion.".to_string()),
            hooks: HookEventsToml {
                pre_tool_use: vec![MatcherGroup {
                    matcher: Some("^Bash$".to_string()),
                    hooks: vec![HookHandlerConfig::Command {
                        command: "python3 /tmp/pre.py".to_string(),
                        command_windows: None,
                        timeout_sec: Some(10),
                        r#async: false,
                        status_message: Some("checking".to_string()),
                        additional_context_limit: Some(4096),
                    }],
                }],
                ..Default::default()
            },
        }
    );
}

#[test]
fn prompt_hook_deserializes_optional_reasoning_effort_from_toml_and_json() {
    let expected = HookEventsToml {
        pre_tool_use: vec![MatcherGroup {
            matcher: None,
            hooks: vec![
                HookHandlerConfig::Prompt {
                    prompt: "Review $$ARGUMENTS".to_string(),
                    filter: None,
                    model: Some("gpt-test".to_string()),
                    reasoning_effort: Some(ReasoningEffort::High),
                    timeout_sec: None,
                    status_message: None,
                    fail_closed: false,
                },
                HookHandlerConfig::Prompt {
                    prompt: "Review without override".to_string(),
                    filter: None,
                    model: None,
                    reasoning_effort: None,
                    timeout_sec: None,
                    status_message: None,
                    fail_closed: false,
                },
            ],
        }],
        ..Default::default()
    };
    let from_toml: HookEventsToml = toml::from_str(
        r#"
[[PreToolUse]]

[[PreToolUse.hooks]]
type = "prompt"
prompt = "Review $$ARGUMENTS"
model = "gpt-test"
reasoningEffort = "high"

[[PreToolUse.hooks]]
type = "prompt"
prompt = "Review without override"
"#,
    )
    .expect("prompt hook TOML should deserialize");
    let from_json: HooksFile = serde_json::from_str(
        r#"{
  "hooks": {
    "PreToolUse": [{
      "hooks": [{
        "type": "prompt",
        "prompt": "Review $$ARGUMENTS",
        "model": "gpt-test",
        "reasoningEffort": "high"
      }, {
        "type": "prompt",
        "prompt": "Review without override"
      }]
    }]
  }
}"#,
    )
    .expect("prompt hook JSON should deserialize");

    assert_eq!(from_toml, expected);
    assert_eq!(from_json.hooks, expected);
}

#[test]
fn prompt_hook_deserializes_filter_from_toml_and_json() {
    let expected = HookHandlerConfig::Prompt {
        prompt: "Review $$ARGUMENTS".to_string(),
        filter: Some(PromptHookFilterConfig {
            command: "uv run --script /tmp/filter.py".to_string(),
            command_windows: Some("py C:\\hooks\\filter.py".to_string()),
            timeout_sec: Some(7),
        }),
        model: None,
        reasoning_effort: None,
        timeout_sec: None,
        status_message: None,
        fail_closed: false,
    };
    let from_toml: HookHandlerConfig = toml::from_str(
        r#"
type = "prompt"
prompt = "Review $$ARGUMENTS"
filter = { command = "uv run --script /tmp/filter.py", commandWindows = "py C:\\hooks\\filter.py", timeout = 7 }
"#,
    )
    .expect("prompt filter TOML should deserialize");
    let from_json: HookHandlerConfig = serde_json::from_value(serde_json::json!({
        "type": "prompt",
        "prompt": "Review $$ARGUMENTS",
        "filter": {
            "command": "uv run --script /tmp/filter.py",
            "commandWindows": "py C:\\hooks\\filter.py",
            "timeout": 7
        }
    }))
    .expect("prompt filter JSON should deserialize");

    assert_eq!(from_toml, expected);
    assert_eq!(from_json, expected);
}

#[test]
fn hooks_file_rejects_events_outside_hooks_object() {
    let error = serde_json::from_str::<HooksFile>(
        r#"{
  "SessionStart": [
    {
      "hooks": [
        {
          "type": "command",
          "command": "python3 /tmp/session_start.py"
        }
      ]
    }
  ]
}"#,
    )
    .expect_err("root-level hook events should be rejected");

    assert!(
        error.to_string().contains("unknown field `SessionStart`"),
        "unexpected parse error: {error}"
    );
}

#[test]
fn hook_events_deserialize_from_toml_arrays_of_tables() {
    let parsed: HookEventsToml = toml::from_str(
        r#"
[[PreToolUse]]
matcher = "^Bash$"

[[PreToolUse.hooks]]
type = "command"
command = "python3 /tmp/pre.py"
timeout = 10
statusMessage = "checking"
additionalContextLimit = 4096
"#,
    )
    .expect("hook events TOML should deserialize");

    assert_eq!(
        parsed,
        HookEventsToml {
            pre_tool_use: vec![MatcherGroup {
                matcher: Some("^Bash$".to_string()),
                hooks: vec![HookHandlerConfig::Command {
                    command: "python3 /tmp/pre.py".to_string(),
                    command_windows: None,
                    timeout_sec: Some(10),
                    r#async: false,
                    status_message: Some("checking".to_string()),
                    additional_context_limit: Some(4096),
                }],
            }],
            ..Default::default()
        }
    );
}

#[test]
fn hooks_toml_deserializes_inline_events_and_state_map() {
    let parsed: HooksToml = toml::from_str(
        r#"
[state."/tmp/hooks.json:pre_tool_use:0:0"]
enabled = false
trusted_hash = "sha256:abc123"

[[PreToolUse]]
matcher = "^Bash$"

[[PreToolUse.hooks]]
type = "command"
command = "python3 /tmp/pre.py"
"#,
    )
    .expect("hooks TOML should deserialize");

    assert_eq!(
        parsed,
        HooksToml {
            events: HookEventsToml {
                pre_tool_use: vec![MatcherGroup {
                    matcher: Some("^Bash$".to_string()),
                    hooks: vec![HookHandlerConfig::Command {
                        command: "python3 /tmp/pre.py".to_string(),
                        command_windows: None,
                        timeout_sec: None,
                        r#async: false,
                        status_message: None,
                        additional_context_limit: None,
                    }],
                }],
                ..Default::default()
            },
            state: BTreeMap::from([(
                "/tmp/hooks.json:pre_tool_use:0:0".to_string(),
                super::HookStateToml {
                    enabled: Some(false),
                    trusted_hash: Some("sha256:abc123".to_string()),
                },
            )]),
        }
    );
}

#[test]
fn managed_hooks_requirements_flatten_hook_events() {
    let parsed: ManagedHooksRequirementsToml = toml::from_str(
        r#"
managed_dir = "/enterprise/place"

[[PreToolUse]]
matcher = "^Bash$"

[[PreToolUse.hooks]]
type = "command"
command = "python3 /enterprise/place/pre.py"
"#,
    )
    .expect("requirements hooks TOML should deserialize");

    assert_eq!(
        parsed,
        ManagedHooksRequirementsToml {
            managed_dir: Some(std::path::PathBuf::from("/enterprise/place")),
            windows_managed_dir: None,
            hooks: HookEventsToml {
                pre_tool_use: vec![MatcherGroup {
                    matcher: Some("^Bash$".to_string()),
                    hooks: vec![HookHandlerConfig::Command {
                        command: "python3 /enterprise/place/pre.py".to_string(),
                        command_windows: None,
                        timeout_sec: None,
                        r#async: false,
                        status_message: None,
                        additional_context_limit: None,
                    }],
                }],
                ..Default::default()
            },
        }
    );
}

#[test]
fn hook_events_deserialize_windows_override_from_toml() {
    let parsed: HookEventsToml = toml::from_str(
        r#"
[[PreToolUse]]
matcher = "^Bash$"

[[PreToolUse.hooks]]
type = "command"
command = "bash /enterprise/hooks/pre.sh"
command_windows = "powershell -File C:\\enterprise\\hooks\\pre.ps1"
"#,
    )
    .expect("hook command Windows override TOML should deserialize");

    assert_eq!(
        parsed,
        HookEventsToml {
            pre_tool_use: vec![MatcherGroup {
                matcher: Some("^Bash$".to_string()),
                hooks: vec![HookHandlerConfig::Command {
                    command: "bash /enterprise/hooks/pre.sh".to_string(),
                    command_windows: Some(
                        r"powershell -File C:\enterprise\hooks\pre.ps1".to_string(),
                    ),
                    timeout_sec: None,
                    r#async: false,
                    status_message: None,
                    additional_context_limit: None,
                }],
            }],
            ..Default::default()
        }
    );
}

#[test]
fn hook_events_deserialize_camel_case_windows_override_from_toml() {
    let parsed: HookEventsToml = toml::from_str(
        r#"
[[PreToolUse]]
matcher = "^Bash$"

[[PreToolUse.hooks]]
type = "command"
command = "bash /enterprise/hooks/pre.sh"
commandWindows = "powershell -File C:\\enterprise\\hooks\\pre.ps1"
"#,
    )
    .expect("camelCase hook command Windows override TOML should deserialize");

    assert_eq!(
        parsed,
        HookEventsToml {
            pre_tool_use: vec![MatcherGroup {
                matcher: Some("^Bash$".to_string()),
                hooks: vec![HookHandlerConfig::Command {
                    command: "bash /enterprise/hooks/pre.sh".to_string(),
                    command_windows: Some(
                        r"powershell -File C:\enterprise\hooks\pre.ps1".to_string(),
                    ),
                    timeout_sec: None,
                    r#async: false,
                    status_message: None,
                    additional_context_limit: None,
                }],
            }],
            ..Default::default()
        }
    );
}

#[test]
fn prompt_hook_requires_prompt_text() {
    let error = serde_json::from_value::<HookHandlerConfig>(serde_json::json!({
        "type": "prompt",
        "statusMessage": "  "
    }))
    .expect_err("prompt hook should require prompt text");

    assert!(error.to_string().contains("missing field `prompt`"));
}

#[test]
fn prompt_hook_semantic_errors_do_not_reject_the_config_layer() {
    let oversized_prompt = "é".repeat(8 * 1024 + 1);
    for semantic_error in [
        serde_json::json!({"type":"prompt","prompt":" "}),
        serde_json::json!({"type":"prompt","prompt":oversized_prompt}),
        serde_json::json!({"type":"prompt","prompt":"review","model":" gpt-test"}),
        serde_json::json!({"type":"prompt","prompt":"review","model":""}),
        serde_json::json!({"type":"prompt","prompt":"review","timeout":0}),
        serde_json::json!({"type":"prompt","prompt":"review","timeout":601}),
    ] {
        serde_json::from_value::<HookHandlerConfig>(semantic_error)
            .expect("semantic validation belongs to per-handler discovery");
    }
}

#[test]
fn hook_handler_omits_unset_additional_context_limit() {
    let handler = HookHandlerConfig::Command {
        command: "python3 /tmp/pre.py".to_string(),
        command_windows: None,
        timeout_sec: None,
        r#async: false,
        status_message: None,
        additional_context_limit: None,
    };

    let serialized = serde_json::to_value(handler).expect("hook handler should serialize");

    assert_eq!(serialized.get("additionalContextLimit"), None);
}
