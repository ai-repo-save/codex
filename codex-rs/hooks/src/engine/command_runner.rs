use std::collections::HashMap;
#[cfg(not(windows))]
use std::ffi::OsStr;
use std::ffi::OsString;
use std::io::ErrorKind;
use std::path::Path;
use std::process::Stdio;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::MutexGuard;
use std::time::Duration;
use std::time::Instant;

use async_channel::Sender;
use codex_protocol::shell_environment::scrub_non_inheritable_env_vars;
#[cfg(windows)]
use codex_utils_pty::JobObject;
use tokio::io::AsyncRead;
use tokio::io::AsyncReadExt;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;
use tokio::sync::Semaphore;
use tokio::task::JoinSet;
use tokio::time::timeout;
use tracing::Span;

use super::CommandShell;
use super::ConfiguredHandler;
use super::ConfiguredHandlerKind;
use super::HandlerRunResult;
use super::dispatcher::ParsedHandler;
use super::dispatcher::hook_event_name_label;
use super::dispatcher::hook_execution_mode_label;
use super::dispatcher::hook_handler_type_label;
use super::dispatcher::hook_scope_label;
use super::dispatcher::hook_source_label;
use super::dispatcher::scope_for_event;
use crate::output_spill::AdditionalContext;
use crate::output_spill::HookOutputSpiller;
use codex_protocol::ThreadId;
use codex_protocol::protocol::HookCompletedEvent;
use codex_protocol::protocol::HookHandlerType;
use codex_protocol::protocol::HookOutputEntry;
use codex_protocol::protocol::HookOutputEntryKind;

const MAX_CONCURRENT_ASYNC_HOOKS: usize = 8;

/// Owns command execution and bounded asynchronous work for one session.
#[derive(Clone)]
pub(crate) struct CommandHookRuntime {
    shell: CommandShell,
    environment: Arc<Vec<(OsString, OsString)>>,
    result_sender: Sender<HookCompletedEvent>,
    state: Arc<Mutex<CommandHookRuntimeState>>,
    output_spiller: HookOutputSpiller,
}

struct CommandHookRuntimeState {
    concurrency_limit: Arc<Semaphore>,
    tasks: JoinSet<()>,
}

impl Default for CommandHookRuntimeState {
    fn default() -> Self {
        Self {
            concurrency_limit: Arc::new(Semaphore::new(MAX_CONCURRENT_ASYNC_HOOKS)),
            tasks: JoinSet::new(),
        }
    }
}

impl CommandHookRuntime {
    pub(crate) fn new(
        shell: CommandShell,
        environment: Arc<Vec<(OsString, OsString)>>,
        thread_id: ThreadId,
        result_sender: Sender<HookCompletedEvent>,
    ) -> Self {
        Self {
            shell,
            environment,
            result_sender,
            state: Arc::new(Mutex::new(CommandHookRuntimeState::default())),
            output_spiller: HookOutputSpiller::new(thread_id),
        }
    }

    fn lock_state(&self) -> MutexGuard<'_, CommandHookRuntimeState> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    pub(crate) fn reconfigured(&self, shell: CommandShell) -> Self {
        Self {
            shell,
            environment: Arc::clone(&self.environment),
            result_sender: self.result_sender.clone(),
            state: Arc::clone(&self.state),
            output_spiller: self.output_spiller.clone(),
        }
    }

    pub(crate) fn output_spiller(&self) -> &HookOutputSpiller {
        &self.output_spiller
    }

    pub(crate) fn shell(&self) -> &CommandShell {
        &self.shell
    }

    pub(crate) fn schedule_async_hook<T: 'static>(
        &self,
        handler: ConfiguredHandler,
        input_json: String,
        cwd: std::path::PathBuf,
        turn_id: Option<String>,
        parse: fn(&ConfiguredHandler, HandlerRunResult, Option<String>) -> ParsedHandler<T>,
    ) {
        let mut state = self.lock_state();
        if self.result_sender.is_closed() || state.concurrency_limit.is_closed() {
            return;
        }

        while state.tasks.try_join_next().is_some() {}
        let result_sender = self.result_sender.clone();
        let concurrency_limit = Arc::clone(&state.concurrency_limit);
        let runtime = self.clone();
        state.tasks.spawn(async move {
            let Ok(_permit) = concurrency_limit.acquire_owned().await else {
                return;
            };
            let result = match &handler.kind {
                ConfiguredHandlerKind::Command { command, env, .. } => {
                    run_command(&runtime, &handler, command, env, &input_json, &cwd).await
                }
                ConfiguredHandlerKind::McpTool { .. } | ConfiguredHandlerKind::Prompt { .. } => {
                    return;
                }
            };
            let mut hook_result = parse(&handler, result, turn_id).completed;
            let mut entries = Vec::new();
            let mut warnings = Vec::new();

            for entry in std::mem::take(&mut hook_result.run.entries) {
                match entry.kind {
                    HookOutputEntryKind::Context => {
                        if let Some(text) = runtime
                            .output_spiller
                            .maybe_spill_additional_contexts(vec![AdditionalContext {
                                text: entry.text,
                                limit: handler.additional_context_limit,
                            }])
                            .await
                            .into_iter()
                            .next()
                        {
                            entries.push(HookOutputEntry {
                                kind: HookOutputEntryKind::Context,
                                text,
                            });
                        }
                    }
                    HookOutputEntryKind::Warning => warnings.push(entry),
                    HookOutputEntryKind::Error => entries.push(entry),
                    HookOutputEntryKind::Stop | HookOutputEntryKind::Feedback => {}
                }
            }

            entries.extend(warnings);
            hook_result.run.entries = entries;
            let _ = result_sender.try_send(hook_result);
        });
    }

    pub(crate) async fn shutdown(&self) {
        let mut tasks = {
            let mut state = self.lock_state();
            state.concurrency_limit.close();
            std::mem::take(&mut state.tasks)
        };
        tasks.abort_all();
        while tasks.join_next().await.is_some() {}
    }
}

#[tracing::instrument(
    name = "codex.hooks.command",
    level = "trace",
    skip_all,
    fields(
        hook.event_name = hook_event_name_label(handler.event_name),
        hook.handler_type = hook_handler_type_label(HookHandlerType::Command),
        hook.execution_mode = hook_execution_mode_label(handler.execution_mode()),
        hook.scope = hook_scope_label(scope_for_event(handler.event_name)),
        hook.source = hook_source_label(handler.source),
        hook.display_order = handler.display_order,
        hook.timeout_sec = handler.timeout_sec,
        hook.command_outcome = tracing::field::Empty,
    )
)]
pub(crate) async fn run_command(
    runtime: &CommandHookRuntime,
    handler: &ConfiguredHandler,
    command: &str,
    env: &HashMap<String, String>,
    input_json: &str,
    cwd: &Path,
) -> HandlerRunResult {
    let started_at = chrono::Utc::now().timestamp();
    let started = Instant::now();
    let timeout_sec = handler.timeout_sec;
    let completion = run_shell_command_with_environment(
        ShellCommandRequest {
            shell: &runtime.shell,
            command_text: command,
            env,
            input_json,
            cwd,
            timeout_sec,
            output_limit: None,
            timeout_error: format!("hook timed out after {timeout_sec}s"),
        },
        &runtime.environment,
    )
    .await;
    finish_command_run(started_at, started, completion)
}

pub(crate) struct ShellCommandRequest<'a> {
    pub shell: &'a CommandShell,
    pub command_text: &'a str,
    pub env: &'a HashMap<String, String>,
    pub input_json: &'a str,
    pub cwd: &'a Path,
    pub timeout_sec: u64,
    pub output_limit: Option<usize>,
    pub timeout_error: String,
}

pub(crate) async fn run_shell_command(request: ShellCommandRequest<'_>) -> CommandRunCompletion {
    let environment = std::env::vars_os().collect::<Vec<_>>();
    run_shell_command_with_environment(request, &environment).await
}

async fn run_shell_command_with_environment(
    request: ShellCommandRequest<'_>,
    environment: &[(OsString, OsString)],
) -> CommandRunCompletion {
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
    let mut command = build_command(shell, command_text, environment, env);
    command
        .current_dir(cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);

    #[cfg(unix)]
    command.process_group(0);

    #[cfg(windows)]
    let mut process_tree_job = JobObject::create().ok();
    #[cfg(windows)]
    let child = match process_tree_job.as_ref() {
        Some(job) => match job.spawn_contained(&mut command) {
            Ok(child) => Ok(child),
            Err(_) => {
                process_tree_job = None;
                command.creation_flags(0);
                command.spawn()
            }
        },
        None => command.spawn(),
    };
    #[cfg(not(windows))]
    let child = command.spawn();

    let mut child = match child {
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
    let mut process_tree_guard = ProcessTreeGuard {
        process_id,
        #[cfg(windows)]
        job: process_tree_job,
    };

    let stdin = child.stdin.take();
    let (Some(stdout), Some(stderr)) = (child.stdout.take(), child.stderr.take()) else {
        kill_process_tree(&mut child, process_id).await;
        process_tree_guard.process_id = None;
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

    let result = timeout(Duration::from_secs(timeout_sec), async {
        let write_stdin = async move {
            if let Some(mut stdin) = stdin
                && let Err(err) = stdin.write_all(input_json.as_bytes()).await
                && err.kind() != ErrorKind::BrokenPipe
            {
                return Err(("stdin_error", format!("failed to write hook stdin: {err}")));
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
    .await;

    match result {
        Ok(Ok(((), (status, stdout, stderr)))) => {
            let output_exceeded = stdout.exceeded || stderr.exceeded;
            #[cfg(windows)]
            if let Some(job) = process_tree_guard.job.as_ref() {
                let _ = job.preserve_descendants();
            }
            process_tree_guard.process_id = None;
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
            process_tree_guard.process_id = None;
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
            process_tree_guard.process_id = None;
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

// Needed only until command hooks move to the exec server, which owns process-tree cleanup.
struct ProcessTreeGuard {
    process_id: Option<u32>,
    #[cfg(windows)]
    job: Option<JobObject>,
}

impl Drop for ProcessTreeGuard {
    fn drop(&mut self) {
        let Some(process_id) = self.process_id else {
            return;
        };

        #[cfg(unix)]
        {
            let _ = codex_utils_pty::process_group::kill_process_group(process_id);
        }

        #[cfg(windows)]
        {
            if let Some(job) = self.job.as_ref() {
                let _ = job.terminate();
            } else {
                let _ = std::process::Command::new("taskkill")
                    .args(["/PID", &process_id.to_string(), "/T", "/F"])
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .spawn();
            }
        }
    }
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
) -> HandlerRunResult {
    Span::current().record("hook.command_outcome", completion.outcome);
    HandlerRunResult {
        started_at,
        completed_at: chrono::Utc::now().timestamp(),
        duration_ms: started.elapsed().as_millis().try_into().unwrap_or(i64::MAX),
        exit_code: completion.exit_code,
        stdout: completion.stdout,
        stderr: completion.stderr,
        error: completion.error,
        prompt_filter_skipped: false,
    }
}

fn build_command(
    shell: &CommandShell,
    command_line: &str,
    environment: &[(OsString, OsString)],
    env: &HashMap<String, String>,
) -> Command {
    let mut command = if shell.program.is_empty() {
        default_shell_command(environment)
    } else {
        Command::new(&shell.program)
    };
    if shell.program.is_empty() {
        #[cfg(windows)]
        command.raw_arg(format!(r#""{command_line}""#));

        #[cfg(not(windows))]
        command.arg(command_line);
    } else {
        command.args(&shell.args);

        #[cfg(windows)]
        if shell.args.iter().any(|arg| arg.eq_ignore_ascii_case("/c")) {
            command.raw_arg(format!(r#""{command_line}""#));
        } else {
            command.arg(command_line);
        }

        #[cfg(not(windows))]
        command.arg(command_line);
    }
    // Replay the session snapshot instead of inheriting the live process environment.
    command.env_clear();
    command.envs(environment.iter().cloned());
    command.envs(env);
    scrub_non_inheritable_env_vars(command.as_std_mut());
    command
}

fn default_shell_command(environment: &[(OsString, OsString)]) -> Command {
    #[cfg(windows)]
    let (environment_variable, fallback_program, argument) = ("COMSPEC", "cmd.exe", "/C");

    #[cfg(not(windows))]
    let (environment_variable, fallback_program, argument) = ("SHELL", "/bin/sh", "-lc");

    let program = environment
        .iter()
        .find(|(key, _)| {
            #[cfg(windows)]
            {
                key.to_str()
                    .is_some_and(|key| key.eq_ignore_ascii_case(environment_variable))
            }

            #[cfg(not(windows))]
            {
                key == OsStr::new(environment_variable)
            }
        })
        .map(|(_, value)| value.clone())
        .unwrap_or_else(|| OsString::from(fallback_program));

    let mut command = Command::new(program);
    command.arg(argument);
    command
}

#[cfg(test)]
#[path = "command_runner_tests.rs"]
mod tests;
