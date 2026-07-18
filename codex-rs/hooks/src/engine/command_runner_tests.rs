use std::collections::HashMap;
use std::path::Path;
use std::process::Command;
use std::time::Duration;

use pretty_assertions::assert_eq;

use super::ShellCommandRequest;
use super::run_shell_command;
use crate::engine::CommandShell;

const GRANDCHILD_PID_ENV: &str = "CODEX_HOOK_TEST_GRANDCHILD_PID_FILE";
const TIMEOUT_ERROR: &str = "test hook timed out";
const TIMEOUT_OUTCOME: &str = "timeout";

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

async fn wait_for_process_exit(process_id: u32) -> bool {
    for _ in 0..20 {
        if !process_exists(process_id) {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    false
}

fn process_exists(process_id: u32) -> bool {
    Command::new("kill")
        .args(["-0", &process_id.to_string()])
        .status()
        .is_ok_and(|status| status.success())
}
