use std::fs;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;
use std::process::Output;
use std::thread::sleep;
use std::time::Duration;
use std::time::Instant;

use anyhow::Context;
use anyhow::Result;
use tempfile::tempdir;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires tmux, a locally built codex binary, and CODEX_SIDE_STACK_REPRO_ROLLOUT"]
async fn tmux_side_command_reproduces_stack_overflow_from_real_rollout() -> Result<()> {
    if cfg!(windows) {
        return Ok(());
    }
    let source_rollout = match std::env::var_os("CODEX_SIDE_STACK_REPRO_ROLLOUT") {
        Some(value) => PathBuf::from(value),
        None => {
            eprintln!("skipping side stack repro because CODEX_SIDE_STACK_REPRO_ROLLOUT is unset");
            return Ok(());
        }
    };
    let tmux_version = Command::new("tmux")
        .arg("-V")
        .output()
        .context("failed to run tmux -V")?;
    anyhow::ensure!(
        tmux_version.status.success(),
        "tmux -V failed with status {:?}\nstdout:\n{}\nstderr:\n{}",
        tmux_version.status.code(),
        String::from_utf8_lossy(&tmux_version.stdout),
        String::from_utf8_lossy(&tmux_version.stderr)
    );
    let thread_id = std::env::var("CODEX_SIDE_STACK_REPRO_THREAD_ID")
        .context("CODEX_SIDE_STACK_REPRO_THREAD_ID must be set")?;
    let resume_cwd = std::env::var_os("CODEX_SIDE_STACK_REPRO_CWD")
        .map(PathBuf::from)
        .unwrap_or(codex_utils_cargo_bin::repo_root()?);
    let repo_root = codex_utils_cargo_bin::repo_root()?;
    let codex = codex_binary(&repo_root)?;
    let codex_home = tempdir()?;
    let log_dir = tempdir()?;
    let status_path = codex_home.path().join("side-stack-repro.status");
    let process_log = codex_home.path().join("side-stack-repro.log");

    copy_rollout_into_codex_home(&source_rollout, codex_home.path())?;
    write_config(codex_home.path(), &resume_cwd)?;

    let session_name = format!("codex-side-stack-repro-{}", std::process::id());
    let _session = TmuxSession {
        name: session_name.clone(),
    };
    let command = format!(
        "set -o pipefail; \
         unset RUST_MIN_STACK; \
         export CODEX_HOME={codex_home}; \
         export OPENAI_API_KEY=dummy; \
         export RUST_LOG=trace; \
         {codex} resume {thread_id} --no-alt-screen -C {resume_cwd} \
             -c analytics.enabled=false -c log_dir={log_dir} 2>&1 | tee {process_log}; \
         status=${{PIPESTATUS[0]}}; \
         echo \"$status\" > {status_path}; \
         exit \"$status\"",
        codex_home = shell_quote(codex_home.path()),
        codex = shell_quote(&codex),
        thread_id = shell_quote(&thread_id),
        resume_cwd = shell_quote(&resume_cwd),
        log_dir = shell_quote(log_dir.path()),
        process_log = shell_quote(&process_log),
        status_path = shell_quote(&status_path),
    );

    let start_output = checked_output(
        Command::new("tmux")
            .arg("new-session")
            .arg("-d")
            .arg("-P")
            .arg("-F")
            .arg("#{pane_id}")
            .arg("-x")
            .arg("140")
            .arg("-y")
            .arg("40")
            .arg("-s")
            .arg(&session_name)
            .arg("--")
            .arg("bash")
            .arg("-lc")
            .arg(command),
    )?;
    let codex_pane = stdout_text(&start_output).trim().to_string();
    anyhow::ensure!(!codex_pane.is_empty(), "tmux did not report a pane id");

    wait_for_prompt_or_status(&codex_pane, &status_path, Duration::from_secs(/*secs*/ 30))?;
    answer_prompt_if_present(&codex_pane, "Choose working directory")?;
    answer_prompt_if_present(&codex_pane, "Resume paused goal?")?;
    sleep(Duration::from_millis(/*millis*/ 500));
    send_literal(&codex_pane, "/side")?;
    send_enter(&codex_pane)?;

    let status = wait_for_status_file(&status_path, Duration::from_secs(/*secs*/ 60))
        .with_context(|| failure_context(&codex_pane, &process_log))?;
    let log = fs::read_to_string(&process_log).unwrap_or_default();
    anyhow::ensure!(
        status != 0,
        "expected /side to abort, but codex exited successfully\n{}",
        failure_context(&codex_pane, &process_log)
    );
    anyhow::ensure!(
        log.contains("overflowed its stack")
            || log.contains("fatal runtime error: stack overflow")
            || log.contains("stack overflow"),
        "expected stack overflow evidence, got status {status}\n{}",
        failure_context(&codex_pane, &process_log)
    );

    Ok(())
}

struct TmuxSession {
    name: String,
}

impl Drop for TmuxSession {
    fn drop(&mut self) {
        let _ = Command::new("tmux")
            .arg("kill-session")
            .arg("-t")
            .arg(&self.name)
            .output();
    }
}

fn codex_binary(repo_root: &Path) -> Result<PathBuf> {
    if let Ok(path) = codex_utils_cargo_bin::cargo_bin("codex") {
        return Ok(path);
    }

    let fallback = repo_root.join("codex-rs/target/debug/codex");
    anyhow::ensure!(
        fallback.is_file(),
        "codex binary is unavailable; run `cargo build -p codex-cli` first"
    );
    Ok(fallback)
}

fn copy_rollout_into_codex_home(source_rollout: &Path, codex_home: &Path) -> Result<()> {
    let file_name = source_rollout
        .file_name()
        .and_then(|name| name.to_str())
        .context("rollout path must have a UTF-8 filename")?;
    let date = file_name
        .strip_prefix("rollout-")
        .and_then(|rest| rest.get(0..10))
        .context("rollout filename must start with rollout-YYYY-MM-DD")?;
    let year = date.get(0..4).context("rollout year missing")?;
    let month = date.get(5..7).context("rollout month missing")?;
    let day = date.get(8..10).context("rollout day missing")?;
    let destination_dir = codex_home.join("sessions").join(year).join(month).join(day);
    fs::create_dir_all(&destination_dir)?;
    fs::copy(source_rollout, destination_dir.join(file_name))?;
    Ok(())
}

fn write_config(codex_home: &Path, resume_cwd: &Path) -> Result<()> {
    let resume_cwd = resume_cwd.display();
    fs::write(
        codex_home.join("config.toml"),
        format!(
            r#"model = "gpt-5.4"
model_provider = "openai"
suppress_unstable_features_warning = true

[projects."{resume_cwd}"]
trust_level = "trusted"
"#
        ),
    )?;
    Ok(())
}

fn wait_for_prompt_or_status(pane: &str, status_path: &Path, timeout: Duration) -> Result<String> {
    let deadline = Instant::now() + timeout;
    let mut last_capture = String::new();
    while Instant::now() < deadline {
        if status_path.exists() {
            return Ok(last_capture);
        }
        last_capture = capture_pane(pane)?;
        if last_capture.contains("Choose working directory")
            || last_capture.contains("Resume paused goal?")
            || last_capture.contains('›')
        {
            return Ok(last_capture);
        }
        sleep(Duration::from_millis(/*millis*/ 100));
    }

    anyhow::bail!("timed out waiting for TUI prompt; last capture:\n{last_capture}");
}

fn answer_prompt_if_present(pane: &str, needle: &str) -> Result<()> {
    let capture = capture_pane(pane)?;
    if capture.contains(needle) {
        send_enter(pane)?;
        sleep(Duration::from_millis(/*millis*/ 500));
    }
    Ok(())
}

fn send_literal(pane: &str, text: &str) -> Result<()> {
    check(
        Command::new("tmux")
            .arg("send-keys")
            .arg("-t")
            .arg(pane)
            .arg("-l")
            .arg(text),
    )
}

fn send_enter(pane: &str) -> Result<()> {
    check(
        Command::new("tmux")
            .arg("send-keys")
            .arg("-t")
            .arg(pane)
            .arg("Enter"),
    )
}

fn wait_for_status_file(status_path: &Path, timeout: Duration) -> Result<i32> {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if let Ok(raw) = fs::read_to_string(status_path) {
            return raw
                .trim()
                .parse::<i32>()
                .with_context(|| format!("invalid status file contents: {raw:?}"));
        }
        sleep(Duration::from_millis(/*millis*/ 100));
    }

    anyhow::bail!("timed out waiting for codex process to exit");
}

fn failure_context(pane: &str, process_log: &Path) -> String {
    let capture = capture_pane(pane).unwrap_or_default();
    let log = fs::read_to_string(process_log).unwrap_or_default();
    format!(
        "tmux capture:\n{}\nprocess log tail:\n{}",
        tail_lines(&capture, 80),
        tail_lines(&log, 120)
    )
}

fn tail_lines(text: &str, limit: usize) -> String {
    let lines: Vec<&str> = text.lines().collect();
    let start = lines.len().saturating_sub(limit);
    lines[start..].join("\n")
}

fn capture_pane(pane: &str) -> Result<String> {
    let output = output(
        Command::new("tmux")
            .arg("capture-pane")
            .arg("-p")
            .arg("-t")
            .arg(pane),
    )?;
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

fn check(command: &mut Command) -> Result<()> {
    checked_output(command)?;
    Ok(())
}

fn checked_output(command: &mut Command) -> Result<Output> {
    let output = output(command)?;
    anyhow::ensure!(
        output.status.success(),
        "command failed with status {:?}\nstdout:\n{}\nstderr:\n{}",
        output.status.code(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(output)
}

fn output(command: &mut Command) -> Result<Output> {
    command
        .output()
        .with_context(|| format!("failed to run {command:?}"))
}

fn stdout_text(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).to_string()
}

fn shell_quote(value: impl AsRef<std::ffi::OsStr>) -> String {
    let value = value.as_ref().to_string_lossy();
    format!("'{}'", value.replace('\'', "'\\''"))
}
