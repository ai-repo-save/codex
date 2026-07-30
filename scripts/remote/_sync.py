#!/usr/bin/env -S uv run python

import logging
import os
import shlex
import shutil
import subprocess
import time
from dataclasses import dataclass
from pathlib import Path, PurePosixPath
from typing import Sequence


LOGGER = logging.getLogger("remote_build_sync")
DEFAULT_HOST = "192.168.50.8"
DEFAULT_BRANCH = "main"
DEFAULT_REMOTE_PATH = "/root/codex"
DEFAULT_TARGET = "x86_64-unknown-linux-gnu"
REMOTE_TARGET_LOCK_PATH = "/var/tmp/codex-remote-target.lock"
DEFAULT_REMOTE_COMMAND_HEARTBEAT_SECONDS = 60.0
REMOTE_COMMAND_HEARTBEAT_SECONDS = float(
    os.environ.get(
        "CODEX_REMOTE_COMMAND_HEARTBEAT_SECONDS",
        str(DEFAULT_REMOTE_COMMAND_HEARTBEAT_SECONDS),
    )
)
MAX_HEARTBEAT_STATUS_CHARS = 6000
REMOTE_FETCH_ATTEMPTS = 3


@dataclass(frozen=True)
class StatusPlan:
    copy_paths: tuple[str, ...]
    delete_paths: tuple[str, ...]


@dataclass(frozen=True)
class RemoteWorkflow:
    host: str
    branch: str
    remote_path: str
    command: tuple[str, ...]


def run_remote_workflow(config: RemoteWorkflow) -> int:
    configure_logging()
    config = normalize_config(config)

    repo_root = git_repo_root()
    require_command("git")
    require_command("ssh")
    require_command("rsync")

    ensure_clean_local_worktree(repo_root)
    ensure_current_branch(repo_root, config.branch)

    local_head = git_output(repo_root, "rev-parse", "HEAD")
    LOGGER.info("pushing %s to origin/%s", local_head[:12], config.branch)
    run(("git", "push", "origin", config.branch), cwd=repo_root)

    sync_remote_checkout(repo_root, config, local_head)
    remote_command_error: subprocess.CalledProcessError | None = None
    try:
        run_remote_command(config)
    except subprocess.CalledProcessError as error:
        remote_command_error = error

    ensure_clean_local_worktree(repo_root)
    ensure_local_head(repo_root, local_head)

    status_plan = remote_status_plan(config)
    if not status_plan.copy_paths and not status_plan.delete_paths:
        LOGGER.info("remote command produced no Git-visible file changes")
    else:
        apply_remote_changes(repo_root, config, status_plan)
        LOGGER.info("local checkout updated from remote Git-visible changes")
        run(("git", "status", "--short"), cwd=repo_root)

    if remote_command_error is not None:
        raise remote_command_error
    return 0


def normalize_config(config: RemoteWorkflow) -> RemoteWorkflow:
    return RemoteWorkflow(
        host=config.host,
        branch=config.branch,
        remote_path=config.remote_path.rstrip("/"),
        command=config.command,
    )


def configure_logging() -> None:
    logging.basicConfig(
        format="%(levelname)s %(name)s: %(message)s",
        level=logging.INFO,
    )


def git_repo_root() -> Path:
    result = subprocess.run(
        ("git", "rev-parse", "--show-toplevel"),
        check=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
    )
    return Path(result.stdout.strip())


def require_command(command: str) -> None:
    if shutil.which(command) is None:
        raise SystemExit(f"{command} is required")


def ensure_clean_local_worktree(repo_root: Path) -> None:
    status = git_output(repo_root, "status", "--porcelain")
    if status:
        raise SystemExit(
            "local checkout has uncommitted changes; commit or clean them before remote sync"
        )


def ensure_current_branch(repo_root: Path, branch: str) -> None:
    current = git_output(repo_root, "branch", "--show-current")
    if current != branch:
        raise SystemExit(f"current branch is {current!r}; expected {branch!r}")


def ensure_local_head(repo_root: Path, expected_head: str) -> None:
    current_head = git_output(repo_root, "rev-parse", "HEAD")
    if current_head != expected_head:
        raise SystemExit(
            "local HEAD changed while remote command was running; refusing to copy files back"
        )


def sync_remote_checkout(
    repo_root: Path, config: RemoteWorkflow, expected_head: str
) -> None:
    LOGGER.info("updating remote checkout %s:%s", config.host, config.remote_path)
    try:
        run(ssh_command(config, remote_checkout_sync_command(config, expected_head)))
    except subprocess.CalledProcessError:
        LOGGER.warning("remote Git sync failed; falling back to a local Git bundle")
        sync_remote_checkout_from_bundle(repo_root, config, expected_head)


def sync_remote_checkout_from_bundle(
    repo_root: Path, config: RemoteWorkflow, expected_head: str
) -> None:
    bundle_name = f"codex-sync-{os.getpid()}-{expected_head[:12]}.bundle"
    local_bundle = Path("/tmp") / bundle_name
    remote_bundle = f"/tmp/{bundle_name}"
    try:
        run(
            local_bundle_create_command(config, local_bundle),
            cwd=repo_root,
        )
        run(("rsync", "--archive", str(local_bundle), f"{config.host}:{remote_bundle}"))
        run(
            ssh_command(
                config,
                remote_bundle_sync_command(config, expected_head, remote_bundle),
            )
        )
    finally:
        local_bundle.unlink(missing_ok=True)


def local_bundle_create_command(
    config: RemoteWorkflow, local_bundle: Path
) -> tuple[str, ...]:
    return (
        "git",
        "bundle",
        "create",
        str(local_bundle),
        f"refs/heads/{config.branch}",
    )


def remote_bundle_sync_command(
    config: RemoteWorkflow, expected_head: str, remote_bundle: str
) -> str:
    return (
        "set -euo pipefail; "
        f"trap 'rm -f {shell_quote(remote_bundle)}' EXIT; "
        f"cd {shell_quote(config.remote_path)}; "
        f"git fetch {shell_quote(remote_bundle)} "
        f"{shell_quote(f'refs/heads/{config.branch}')}; "
        f'test "$(git rev-parse FETCH_HEAD)" = {shell_quote(expected_head)}; '
        f"git checkout {shell_quote(config.branch)}; "
        "git reset --hard FETCH_HEAD; "
        "git clean -fd"
    )


def remote_checkout_sync_command(config: RemoteWorkflow, expected_head: str) -> str:
    return (
        "set -euo pipefail; "
        f"cd {shell_quote(config.remote_path)}; "
        "fetched=false; "
        f"for attempt in $(seq 1 {REMOTE_FETCH_ATTEMPTS}); do "
        "if git fetch origin; then fetched=true; break; fi; "
        f'if [ "$attempt" -eq {REMOTE_FETCH_ATTEMPTS} ]; then break; fi; '
        'echo "remote sync: git fetch failed; retrying" >&2; '
        "sleep $((attempt * 2)); "
        "done; "
        'if [ "$fetched" = true ]; then '
        f"git checkout {shell_quote(config.branch)}; "
        f"git reset --hard origin/{shell_quote(config.branch)}; "
        "else "
        "current_branch=$(git branch --show-current); "
        "current_head=$(git rev-parse HEAD); "
        f'if [ "$current_branch" != {shell_quote(config.branch)} ] '
        f'|| [ "$current_head" != {shell_quote(expected_head)} ] '
        "|| ! git diff --quiet || ! git diff --cached --quiet; then "
        'echo "remote sync: fetch failed and checkout does not match the requested clean HEAD" >&2; '
        "exit 1; "
        "fi; "
        'echo "remote sync: fetch failed; reusing matching clean checkout" >&2; '
        "fi; "
        "git clean -fd"
    )


def run_remote_command(config: RemoteWorkflow) -> None:
    remote_command = shlex.join(config.command)
    remote_pid_file = (
        f"/tmp/codex-remote-command-{os.getpid()}-{int(time.time() * 1000)}.pid"
    )
    wrapped_remote_command = (
        f"printf '%s\\n' $$ > {shell_quote(remote_pid_file)}; "
        f"trap 'rm -f {shell_quote(remote_pid_file)}' EXIT; "
        f"set -euo pipefail; cd {shell_quote(config.remote_path)}; {remote_command}"
    )
    LOGGER.info("running remote command: %s", remote_command)
    run(
        ssh_command(config, wrapped_remote_command),
        heartbeat_interval_seconds=REMOTE_COMMAND_HEARTBEAT_SECONDS,
        heartbeat_status_command=ssh_command(
            config,
            remote_process_status_command(remote_pid_file),
        ),
    )


def remote_process_status_command(pid_file: str) -> str:
    return (
        "set -euo pipefail; "
        f"pid_file={shell_quote(pid_file)}; "
        'if [ ! -s "$pid_file" ]; then '
        'echo "remote status: command pid file is missing"; '
        "exit 0; "
        "fi; "
        'root_pid="$(cat "$pid_file")"; '
        'if ! kill -0 "$root_pid" 2>/dev/null; then '
        'echo "remote status: command pid $root_pid is not running"; '
        "exit 0; "
        "fi; "
        'pids="$root_pid"; '
        'frontier="$root_pid"; '
        'while [ -n "$frontier" ]; do '
        'next=""; '
        "for pid in $frontier; do "
        'children="$(pgrep -P "$pid" 2>/dev/null || true)"; '
        'next="$next $children"; '
        "done; "
        'frontier="$(echo "$next" | xargs 2>/dev/null || true)"; '
        'if [ -n "$frontier" ]; then pids="$pids $frontier"; fi; '
        "done; "
        'pid_csv="$(echo "$pids" | tr " " "," | sed "s/,,*/,/g; s/^,//; s/,$//")"; '
        'echo "remote status: active process tree rooted at $root_pid"; '
        'ps -o pid,ppid,stat,etime,pcpu,pmem,comm,args -p "$pid_csv" --sort=-pcpu '
        "| head -n 12"
    )


def remote_status_plan(config: RemoteWorkflow) -> StatusPlan:
    result = run(
        ssh_command(
            config,
            f"cd {shell_quote(config.remote_path)} && git status --porcelain=v1 -z",
        ),
        stdout=subprocess.PIPE,
        text=False,
    )
    return parse_porcelain_status(result.stdout)


def parse_porcelain_status(raw_status: bytes) -> StatusPlan:
    copy_paths: list[str] = []
    delete_paths: list[str] = []
    entries = raw_status.split(b"\0")
    index = 0
    while index < len(entries):
        entry = entries[index]
        index += 1
        if not entry:
            continue
        if len(entry) < 4:
            raise ValueError(f"invalid porcelain status entry: {entry!r}")

        status = entry[:2].decode("ascii")
        path = validate_git_path(entry[3:].decode("utf-8"))
        old_path = ""
        if "R" in status or "C" in status:
            if index >= len(entries) or not entries[index]:
                raise ValueError(
                    f"missing source path for porcelain status entry: {entry!r}"
                )
            old_path = validate_git_path(entries[index].decode("utf-8"))
            index += 1

        if status == "??":
            copy_paths.append(path)
        elif "D" in status:
            delete_paths.append(path)
        else:
            copy_paths.append(path)

        if old_path and "R" in status:
            delete_paths.append(old_path)

    return StatusPlan(
        copy_paths=tuple(dict.fromkeys(copy_paths)),
        delete_paths=tuple(dict.fromkeys(delete_paths)),
    )


def validate_git_path(path: str) -> str:
    posix_path = PurePosixPath(path)
    if not path or posix_path.is_absolute() or ".." in posix_path.parts:
        raise ValueError(f"unsafe porcelain status path: {path!r}")
    return path


def apply_remote_changes(
    repo_root: Path, config: RemoteWorkflow, status_plan: StatusPlan
) -> None:
    for path in status_plan.delete_paths:
        delete_local_path(repo_root, path)

    if not status_plan.copy_paths:
        return

    LOGGER.info("copying %d changed file(s) from remote", len(status_plan.copy_paths))
    rsync = subprocess.Popen(
        (
            "rsync",
            "--archive",
            "--relative",
            "--files-from=-",
            "--from0",
            f"{config.host}:{config.remote_path}/",
            f"{repo_root}/",
        ),
        stdin=subprocess.PIPE,
    )
    assert rsync.stdin is not None
    with rsync.stdin:
        rsync.stdin.write(
            b"\0".join(path.encode("utf-8") for path in status_plan.copy_paths)
        )
        rsync.stdin.write(b"\0")
    exit_code = rsync.wait()
    if exit_code != 0:
        raise SystemExit(exit_code)


def delete_local_path(repo_root: Path, relative_path: str) -> None:
    path = (repo_root / relative_path).resolve()
    if repo_root not in path.parents and path != repo_root:
        raise SystemExit(f"refusing to delete path outside repo: {relative_path}")
    if path.is_dir():
        shutil.rmtree(path)
        LOGGER.info("deleted local directory removed on remote: %s", relative_path)
    elif path.exists():
        path.unlink()
        LOGGER.info("deleted local file removed on remote: %s", relative_path)


def git_output(repo_root: Path, *args: str) -> str:
    result = run(("git", *args), cwd=repo_root, stdout=subprocess.PIPE)
    return result.stdout.strip()


def remote_codex_rs_just_command(args: Sequence[str]) -> tuple[str, ...]:
    if not args:
        raise ValueError("just args must not be empty")
    command = f"cd codex-rs && just {shlex.join(args)}"
    return (
        "bash",
        "-lc",
        remote_build_shell_command(DEFAULT_TARGET, command),
    )


def remote_build_env_command(target: str) -> str:
    linker_env = cargo_target_linker_env_key(target)
    return (
        "export CARGO_INCREMENTAL=0; "
        'echo "remote build: CARGO_INCREMENTAL=$CARGO_INCREMENTAL"; '
        "if command -v flock >/dev/null 2>&1; then "
        f"exec 9>{shell_quote(REMOTE_TARGET_LOCK_PATH)}; "
        "flock -s 9; "
        'echo "remote build: acquired shared target cache lock"; '
        "else "
        'echo "remote build: flock not found; target cleanup cannot coordinate" >&2; '
        "fi; "
        "if command -v sccache >/dev/null 2>&1; then "
        'export RUSTC_WRAPPER="$(command -v sccache)"; '
        'echo "remote build: using RUSTC_WRAPPER=$RUSTC_WRAPPER"; '
        'printf "%s\\n" "remote build: sccache stats before"; '
        '"$RUSTC_WRAPPER" --show-stats || '
        'echo "remote build: unable to read sccache stats before" >&2; '
        "codex_remote_sccache_stats_after() { "
        'printf "%s\\n" "remote build: sccache stats after"; '
        '"$RUSTC_WRAPPER" --show-stats || '
        'echo "remote build: unable to read sccache stats after" >&2; '
        "}; "
        "else "
        'echo "remote build: sccache not found; compiler-cache metrics unavailable"; '
        "codex_remote_sccache_stats_after() { :; }; "
        "fi; "
        "if command -v clang >/dev/null 2>&1 && command -v mold >/dev/null 2>&1; then "
        f"export {linker_env}=clang; "
        'export RUSTFLAGS="${RUSTFLAGS:+$RUSTFLAGS }-C link-arg=-fuse-ld=$(command -v mold)"; '
        'echo "remote build: using clang with mold"; '
        "elif command -v clang >/dev/null 2>&1 && command -v ld.lld >/dev/null 2>&1; then "
        f"export {linker_env}=clang; "
        'export RUSTFLAGS="${RUSTFLAGS:+$RUSTFLAGS }-C link-arg=-fuse-ld=lld"; '
        'echo "remote build: using clang with lld"; '
        'else echo "remote build: no fast linker configured"; fi'
    )


def remote_build_shell_command(target: str, command: str) -> str:
    return (
        f"{remote_build_env_command(target)} && ({command}; "
        "build_status=$?; "
        "codex_remote_sccache_stats_after; "
        'exit "$build_status")'
    )


def cargo_target_linker_env_key(target: str) -> str:
    return f"CARGO_TARGET_{target.upper().replace('-', '_')}_LINKER"


def ssh_command(config: RemoteWorkflow, remote_command: str) -> tuple[str, ...]:
    return ("ssh", config.host, remote_command)


def run(
    command: Sequence[str],
    *,
    cwd: Path | None = None,
    stdout: int | None = None,
    text: bool = True,
    heartbeat_interval_seconds: float | None = None,
    heartbeat_status_command: Sequence[str] | None = None,
) -> subprocess.CompletedProcess[str] | subprocess.CompletedProcess[bytes]:
    LOGGER.debug("running: %s", shlex.join(command))
    if stdout is not None or heartbeat_interval_seconds is None:
        return subprocess.run(command, cwd=cwd, check=True, stdout=stdout, text=text)

    started_at = time.monotonic()
    next_heartbeat_at = started_at + heartbeat_interval_seconds
    process = subprocess.Popen(command, cwd=cwd, text=text)
    while True:
        exit_code = process.poll()
        if exit_code is not None:
            if exit_code != 0:
                raise subprocess.CalledProcessError(exit_code, command)
            return subprocess.CompletedProcess(command, exit_code)

        now = time.monotonic()
        if now >= next_heartbeat_at:
            LOGGER.info(
                "still running after %.0fs: %s",
                now - started_at,
                shlex.join(command),
            )
            if heartbeat_status_command is not None:
                log_heartbeat_status(heartbeat_status_command, cwd)
            next_heartbeat_at = now + heartbeat_interval_seconds
        time.sleep(min(1.0, heartbeat_interval_seconds))


def log_heartbeat_status(command: Sequence[str], cwd: Path | None) -> None:
    result = subprocess.run(
        command,
        cwd=cwd,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        text=True,
    )
    output = result.stdout.strip()
    if result.returncode != 0:
        LOGGER.info(
            "heartbeat status command failed with exit code %s: %s",
            result.returncode,
            shlex.join(command),
        )
    if output:
        LOGGER.info("%s", truncate_text(output, MAX_HEARTBEAT_STATUS_CHARS))


def truncate_text(value: str, max_chars: int) -> str:
    if len(value) <= max_chars:
        return value
    return f"{value[:max_chars]}... [truncated {len(value) - max_chars} chars]"


def shell_quote(value: str) -> str:
    return "'" + value.replace("'", "'\"'\"'") + "'"
