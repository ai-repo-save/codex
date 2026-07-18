use std::path::Path;
use std::process::Stdio;
use std::time::Duration;
use std::time::Instant;

use tokio::io::AsyncRead;
use tokio::io::AsyncReadExt;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;
use tokio::time::timeout;
use tracing::Span;

use super::CommandShell;
use super::ConfiguredHandler;
use super::ConfiguredHandlerKind;
use super::dispatcher::hook_event_name_label;
use super::dispatcher::hook_execution_mode_label;
use super::dispatcher::hook_handler_type_label;
use super::dispatcher::hook_scope_label;
use super::dispatcher::hook_source_label;
use super::dispatcher::scope_for_event;
use codex_protocol::protocol::HookExecutionMode;
use codex_protocol::protocol::HookHandlerType;

#[derive(Debug)]
pub(crate) struct CommandRunResult {
    pub started_at: i64,
    pub completed_at: i64,
    pub duration_ms: i64,
    pub exit_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    pub error: Option<String>,
}

#[tracing::instrument(
    name = "codex.hooks.command",
    level = "trace",
    skip_all,
    fields(
        hook.event_name = hook_event_name_label(handler.event_name),
        hook.handler_type = hook_handler_type_label(HookHandlerType::Command),
        hook.execution_mode = hook_execution_mode_label(HookExecutionMode::Sync),
        hook.scope = hook_scope_label(scope_for_event(handler.event_name)),
        hook.source = hook_source_label(handler.source),
        hook.display_order = handler.display_order,
        hook.configured_order = configured_order,
        hook.timeout_sec = handler.timeout_sec(),
        hook.command_outcome = tracing::field::Empty,
    )
)]
pub(crate) async fn run_command(
    shell: &CommandShell,
    handler: &ConfiguredHandler,
    configured_order: usize,
    input_json: &str,
    cwd: &Path,
) -> CommandRunResult {
    let started_at = chrono::Utc::now().timestamp();
    let started = Instant::now();
    let ConfiguredHandlerKind::Command {
        command: command_text,
        timeout_sec,
    } = &handler.kind
    else {
        panic!("prompt handler cannot run as a command hook");
    };
    let completion = run_shell_command(ShellCommandRequest {
        shell,
        command_text,
        env: &handler.env,
        input_json,
        cwd,
        timeout_sec: *timeout_sec,
        output_limit: None,
        timeout_error: format!("hook timed out after {timeout_sec}s"),
    })
    .await;
    finish_command_run(started_at, started, completion)
}

pub(crate) struct ShellCommandRequest<'a> {
    pub shell: &'a CommandShell,
    pub command_text: &'a str,
    pub env: &'a std::collections::HashMap<String, String>,
    pub input_json: &'a str,
    pub cwd: &'a Path,
    pub timeout_sec: u64,
    pub output_limit: Option<usize>,
    pub timeout_error: String,
}

pub(crate) async fn run_shell_command(request: ShellCommandRequest<'_>) -> CommandRunCompletion {
    let ShellCommandRequest {
        shell,
        command_text,
        env,
        input_json,
        cwd,
        timeout_sec,
        output_limit,
        timeout_error,
    } = request;
    let mut command = build_command(shell, command_text, env);
    command
        .current_dir(cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    #[cfg(unix)]
    command.process_group(0);

    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(err) => {
            return CommandRunCompletion {
                exit_code: None,
                stdout: String::new(),
                stderr: String::new(),
                stdout_len: 0,
                stderr_len: 0,
                error: Some(err.to_string()),
                outcome: "spawn_error",
            };
        }
    };
    let process_id = child.id();

    let stdin = child.stdin.take();
    let (Some(stdout), Some(stderr)) = (child.stdout.take(), child.stderr.take()) else {
        kill_process_tree(&mut child, process_id).await;
        return CommandRunCompletion {
            exit_code: None,
            stdout: String::new(),
            stderr: String::new(),
            stdout_len: 0,
            stderr_len: 0,
            error: Some("hook output pipes are unavailable".to_string()),
            outcome: "pipe_error",
        };
    };
    let timeout_duration = Duration::from_secs(timeout_sec);
    match timeout(timeout_duration, async {
        let write_stdin = async move {
            if let Some(mut stdin) = stdin {
                stdin
                    .write_all(input_json.as_bytes())
                    .await
                    .map_err(|err| ("stdin_error", format!("failed to write hook stdin: {err}")))?;
            }
            Ok::<_, (&'static str, String)>(())
        };
        let collect_process = async {
            tokio::try_join!(
                child.wait(),
                collect_output(stdout, output_limit),
                collect_output(stderr, output_limit),
            )
            .map_err(|err| ("wait_error", err.to_string()))
        };
        tokio::try_join!(write_stdin, collect_process)
    })
    .await
    {
        Ok(Ok(((), (status, stdout, stderr)))) => {
            let output_exceeded = stdout.exceeded || stderr.exceeded;
            CommandRunCompletion {
                exit_code: status.code(),
                stdout: String::from_utf8_lossy(&stdout.bytes).to_string(),
                stderr: String::from_utf8_lossy(&stderr.bytes).to_string(),
                stdout_len: stdout.total_len,
                stderr_len: stderr.total_len,
                error: match (output_exceeded, output_limit) {
                    (true, Some(limit)) => Some(format!(
                        "hook output exceeded {limit} bytes (stdout: {}, stderr: {})",
                        stdout.total_len, stderr.total_len
                    )),
                    (true, None) => Some(format!(
                        "hook output exceeded its limit (stdout: {}, stderr: {})",
                        stdout.total_len, stderr.total_len
                    )),
                    (false, _) => None,
                },
                outcome: if output_exceeded {
                    "output_limit"
                } else {
                    "completed"
                },
            }
        }
        Ok(Err((outcome, error))) => {
            kill_process_tree(&mut child, process_id).await;
            CommandRunCompletion {
                exit_code: None,
                stdout: String::new(),
                stderr: String::new(),
                stdout_len: 0,
                stderr_len: 0,
                error: Some(error),
                outcome,
            }
        }
        Err(_) => {
            kill_process_tree(&mut child, process_id).await;
            CommandRunCompletion {
                exit_code: None,
                stdout: String::new(),
                stderr: String::new(),
                stdout_len: 0,
                stderr_len: 0,
                error: Some(timeout_error),
                outcome: "timeout",
            }
        }
    }
}

#[cfg(unix)]
async fn kill_process_tree(child: &mut tokio::process::Child, process_id: Option<u32>) {
    let Some(process_group_id) = process_id else {
        let _ = child.kill().await;
        return;
    };
    if let Err(error) = codex_utils_pty::process_group::kill_process_group(process_group_id) {
        tracing::warn!("failed to kill hook command process group {process_group_id}: {error}");
        let _ = child.kill().await;
        return;
    }
    let _ = child.wait().await;
}

#[cfg(windows)]
async fn kill_process_tree(child: &mut tokio::process::Child, process_id: Option<u32>) {
    let Some(process_id) = process_id else {
        let _ = child.kill().await;
        return;
    };
    let process_id = process_id.to_string();
    let status = Command::new("taskkill")
        .args(["/PID", &process_id, "/T", "/F"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .await;
    if !status.is_ok_and(|status| status.success()) {
        let _ = child.kill().await;
        return;
    }
    let _ = child.wait().await;
}

#[cfg(not(any(unix, windows)))]
async fn kill_process_tree(child: &mut tokio::process::Child, _process_id: Option<u32>) {
    let _ = child.kill().await;
}

pub(crate) struct CommandRunCompletion {
    pub exit_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    pub stdout_len: usize,
    pub stderr_len: usize,
    pub error: Option<String>,
    pub outcome: &'static str,
}

struct CollectedOutput {
    bytes: Vec<u8>,
    total_len: usize,
    exceeded: bool,
}

async fn collect_output(
    mut reader: impl AsyncRead + Unpin,
    limit: Option<usize>,
) -> std::io::Result<CollectedOutput> {
    let mut bytes = Vec::new();
    let mut total_len = 0_usize;
    let mut buffer = [0_u8; 8 * 1024];
    loop {
        let read = reader.read(&mut buffer).await?;
        if read == 0 {
            break;
        }
        total_len = total_len.saturating_add(read);
        match limit {
            Some(limit) if bytes.len() < limit => {
                let remaining = limit - bytes.len();
                bytes.extend_from_slice(&buffer[..read.min(remaining)]);
            }
            Some(_) => {}
            None => bytes.extend_from_slice(&buffer[..read]),
        }
    }
    Ok(CollectedOutput {
        bytes,
        total_len,
        exceeded: limit.is_some_and(|limit| total_len > limit),
    })
}

fn finish_command_run(
    started_at: i64,
    started: Instant,
    completion: CommandRunCompletion,
) -> CommandRunResult {
    Span::current().record("hook.command_outcome", completion.outcome);
    CommandRunResult {
        started_at,
        completed_at: chrono::Utc::now().timestamp(),
        duration_ms: started.elapsed().as_millis().try_into().unwrap_or(i64::MAX),
        exit_code: completion.exit_code,
        stdout: completion.stdout,
        stderr: completion.stderr,
        error: completion.error,
    }
}

fn build_command(
    shell: &CommandShell,
    command_text: &str,
    env: &std::collections::HashMap<String, String>,
) -> Command {
    let mut process = if shell.program.is_empty() {
        default_shell_command()
    } else {
        Command::new(&shell.program)
    };
    if shell.program.is_empty() {
        process.arg(command_text);
    } else {
        process.args(&shell.args);
        process.arg(command_text);
    }
    process.envs(env);
    process
}

fn default_shell_command() -> Command {
    #[cfg(windows)]
    {
        let comspec = std::env::var("COMSPEC").unwrap_or_else(|_| "cmd.exe".to_string());
        let mut command = Command::new(comspec);
        command.arg("/C");
        command
    }

    #[cfg(not(windows))]
    {
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string());
        let mut command = Command::new(shell);
        command.arg("-lc");
        command
    }
}

#[cfg(all(test, unix))]
#[path = "command_runner_tests.rs"]
mod tests;
