#!/usr/bin/env -S uv run python
from __future__ import annotations

import argparse
import logging
import shlex
import shutil
import subprocess
import sys
import time
from dataclasses import dataclass
from pathlib import Path, PurePosixPath
from typing import Sequence


LOGGER = logging.getLogger("remote_build_sync")
DEFAULT_HOST = "192.168.50.8"
DEFAULT_BRANCH = "main"
DEFAULT_REMOTE_PATH = "/root/codex"
DEFAULT_COMMAND = ("just", "codex", "--version")
REMOTE_COMMAND_HEARTBEAT_SECONDS = 60.0


@dataclass(frozen=True)
class StatusPlan:
    copy_paths: tuple[str, ...]
    delete_paths: tuple[str, ...]


@dataclass(frozen=True)
class Config:
    host: str
    branch: str
    remote_path: str
    command: tuple[str, ...]


def main(argv: Sequence[str] | None = None) -> int:
    configure_logging()
    args = parse_args(argv if argv is not None else sys.argv[1:])
    config = Config(
        host=args.host,
        branch=args.branch,
        remote_path=args.remote_path.rstrip("/"),
        command=tuple(args.command) if args.command else DEFAULT_COMMAND,
    )

    repo_root = git_repo_root()
    require_command("git")
    require_command("ssh")
    require_command("rsync")

    ensure_clean_local_worktree(repo_root)
    ensure_current_branch(repo_root, config.branch)

    local_head = git_output(repo_root, "rev-parse", "HEAD")
    LOGGER.info("pushing %s to origin/%s", local_head[:12], config.branch)
    run(("git", "push", "origin", config.branch), cwd=repo_root)

    sync_remote_checkout(config)
    run_remote_command(config)

    ensure_clean_local_worktree(repo_root)
    ensure_local_head(repo_root, local_head)

    status_plan = remote_status_plan(config)
    if not status_plan.copy_paths and not status_plan.delete_paths:
        LOGGER.info("remote command produced no Git-visible file changes")
        return 0

    apply_remote_changes(repo_root, config, status_plan)
    LOGGER.info("local checkout updated from remote Git-visible changes")
    run(("git", "status", "--short"), cwd=repo_root)
    return 0


def configure_logging() -> None:
    logging.basicConfig(
        format="%(levelname)s %(name)s: %(message)s",
        level=logging.INFO,
    )


def parse_args(argv: Sequence[str]) -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description=(
            "Push the local branch, run a repository command on the remote "
            "builder, then copy remote Git-visible changes back locally."
        )
    )
    parser.add_argument(
        "--host", default=DEFAULT_HOST, help="SSH host for remote execution"
    )
    parser.add_argument(
        "--branch", default=DEFAULT_BRANCH, help="Git branch to push and reset"
    )
    parser.add_argument(
        "--remote-path",
        default=DEFAULT_REMOTE_PATH,
        help="Remote checkout path updated from origin before running the command",
    )
    parser.add_argument(
        "command",
        nargs=argparse.REMAINDER,
        help="Remote command to run after '--'. Defaults to: just codex --version",
    )
    parsed = parser.parse_args(argv)
    if parsed.command and parsed.command[0] == "--":
        parsed.command = parsed.command[1:]
    return parsed


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


def sync_remote_checkout(config: Config) -> None:
    LOGGER.info("updating remote checkout %s:%s", config.host, config.remote_path)
    run(
        (
            "ssh",
            config.host,
            "set -euo pipefail; "
            f"cd {shell_quote(config.remote_path)}; "
            "git fetch origin; "
            f"git checkout {shell_quote(config.branch)}; "
            f"git reset --hard origin/{shell_quote(config.branch)}; "
            "git clean -fd",
        )
    )


def run_remote_command(config: Config) -> None:
    remote_command = shlex.join(config.command)
    LOGGER.info("running remote command: %s", remote_command)
    run(
        (
            "ssh",
            config.host,
            f"set -euo pipefail; cd {shell_quote(config.remote_path)}; {remote_command}",
        ),
        heartbeat_interval_seconds=REMOTE_COMMAND_HEARTBEAT_SECONDS,
    )


def remote_status_plan(config: Config) -> StatusPlan:
    result = run(
        (
            "ssh",
            config.host,
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
    repo_root: Path, config: Config, status_plan: StatusPlan
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


def run(
    command: Sequence[str],
    *,
    cwd: Path | None = None,
    stdout: int | None = None,
    text: bool = True,
    heartbeat_interval_seconds: float | None = None,
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
            next_heartbeat_at = now + heartbeat_interval_seconds
        time.sleep(min(1.0, heartbeat_interval_seconds))


def shell_quote(value: str) -> str:
    return "'" + value.replace("'", "'\"'\"'") + "'"


if __name__ == "__main__":
    raise SystemExit(main())
