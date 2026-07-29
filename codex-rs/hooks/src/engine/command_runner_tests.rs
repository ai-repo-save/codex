use std::collections::HashMap;
#[cfg(unix)]
use std::path::Path;
#[cfg(unix)]
use std::process::Command;
#[cfg(unix)]
use std::time::Duration;
#[cfg(windows)]
use std::fs;

use codex_protocol::protocol::HookEventName;
use codex_protocol::protocol::HookSource;
use codex_utils_absolute_path::AbsolutePathBuf;
use pretty_assertions::assert_eq;
use tempfile::tempdir;

use super::CommandShell;
use super::ConfiguredHandler;
#[cfg(unix)]
use super::ShellCommandRequest;
use super::run_command;
use super::run_command;
#[cfg(unix)]
use super::run_shell_command;

#[cfg(unix)]
const GRANDCHILD_PID_ENV: &str = "CODEX_HOOK_TEST_GRANDCHILD_PID_FILE";
#[cfg(unix)]
const TIMEOUT_ERROR: &str = "test hook timed out";
#[cfg(unix)]
const TIMEOUT_OUTCOME: &str = "timeout";

#[cfg(unix)]
#[tokio::test]
async fn timeout_kills_grandchild_process() {
    let temp_dir = tempfile::tempdir().expect("create temporary directory");
    let pid_file = temp_dir.path().join("grandchild.pid");
    let env = HashMap::from([(
        GRANDCHILD_PID_ENV.to_string(),
        pid_file.to_string_lossy().into_owned(),
    )]);
    let shell = CommandShell {
        program: "/bin/sh".to_string(),
        args: vec!["-c".to_string()],
    };
    let completion = run_shell_command(ShellCommandRequest {
        shell: &shell,
        command_text: "sleep 60 & printf '%s' \"$!\" > \"$CODEX_HOOK_TEST_GRANDCHILD_PID_FILE\"; wait",
        env: &env,
        input_json: "",
        cwd: Path::new("."),
        timeout_sec: 1,
        output_limit: None,
        timeout_error: TIMEOUT_ERROR.to_string(),
    })
    .await;

    assert_eq!(completion.outcome, TIMEOUT_OUTCOME);
    let grandchild_pid = std::fs::read_to_string(pid_file)
        .expect("read grandchild pid")
        .parse::<u32>()
        .expect("parse grandchild pid");
    let exited = wait_for_process_exit(grandchild_pid).await;
    if !exited {
        let _ = Command::new("kill")
            .args(["-KILL", &grandchild_pid.to_string()])
            .status();
    }
    assert!(exited, "grandchild process survived command timeout");
}

#[cfg(unix)]
async fn wait_for_process_exit(process_id: u32) -> bool {
    for _ in 0..20 {
        if !process_exists(process_id) {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    false
}

#[cfg(unix)]
fn process_exists(process_id: u32) -> bool {
    Command::new("kill")
        .args(["-0", &process_id.to_string()])
        .status()
        .is_ok_and(|status| status.success())
}

#[cfg(windows)]
#[tokio::test]
async fn cmd_shell_runs_quoted_hook_command_path() {
    use std::fs;

    use crate::engine::ConfiguredHandlerKind;

    let temp = tempdir().expect("create temp dir");
    let hook_dir = temp.path().join("hook with spaces");
    fs::create_dir(&hook_dir).expect("create hook dir");
    let hook_path = hook_dir.join("hook.cmd");
    fs::write(
        &hook_path,
        "@echo off\r\nif not \"%~1\"==\"notify\" exit /B 7\r\necho hook-ran\r\n",
    )
    .expect("write hook command");
    let source_path =
        AbsolutePathBuf::try_from(hook_path.clone()).expect("absolute hook command path");
    let handler = ConfiguredHandler {
        event_name: HookEventName::SessionStart,
        matcher: None,
        kind: ConfiguredHandlerKind::Command {
            command: format!(r#""{}" notify"#, hook_path.display()),
            timeout_sec: 10,
        },
        status_message: None,
        additional_context_limit: Default::default(),
        source_path,
        source: HookSource::User,
        display_order: 0,
        env: HashMap::new(),
    };
    let shells = [
        CommandShell {
            program: String::new(),
            args: Vec::new(),
        },
        CommandShell {
            program: std::env::var("COMSPEC").unwrap_or_else(|_| "cmd.exe".to_string()),
            args: vec!["/c".to_string()],
        },
    ];

    for shell in shells {
        let result = run_command(
            &shell,
            &handler,
            /*configured_order*/ 0,
            "{}",
            temp.path(),
        )
        .await;

        assert_eq!(result.exit_code, Some(0), "stderr: {}", result.stderr);
        assert_eq!(result.stdout.trim(), "hook-ran");
        assert!(result.error.is_none());
    }
}

#[tokio::test]
async fn fast_exiting_hook_preserves_stdout_when_stdin_is_not_consumed() {
    let temp = tempdir().expect("create temp dir");
    let source_path = AbsolutePathBuf::try_from(temp.path().join("hooks.json"))
        .expect("absolute hook configuration path");
    let handler = ConfiguredHandler {
        event_name: HookEventName::SessionStart,
        matcher: None,
        kind: super::ConfiguredHandlerKind::Command {
            command: "echo hook-ran".to_string(),
            timeout_sec: 10,
        },
        status_message: None,
        additional_context_limit: Default::default(),
        source_path,
        source: HookSource::User,
        display_order: 0,
        env: HashMap::new(),
    };
    let shell = CommandShell {
        program: String::new(),
        args: Vec::new(),
    };
    let input_json = format!(r#"{{"padding":"{}"}}"#, "x".repeat(1024 * 1024));

    let result = run_command(
        &shell,
        &handler,
        /*configured_order*/ 0,
        &input_json,
        temp.path(),
    )
    .await;

    assert_eq!(result.exit_code, Some(0), "stderr: {}", result.stderr);
    assert_eq!(result.stdout.trim(), "hook-ran");
    assert_eq!(result.error, None);
}
