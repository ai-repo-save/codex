#!/usr/bin/env -S uv run python

import argparse
import sys
from dataclasses import dataclass
from pathlib import Path
from typing import Sequence


if __package__ in (None, ""):
    sys.path.insert(0, str(Path(__file__).resolve().parent))
    from _sync import DEFAULT_HOST
    from _sync import DEFAULT_REMOTE_PATH
    from _sync import REMOTE_TARGET_LOCK_PATH
    from _sync import RemoteWorkflow
    from _sync import require_command
    from _sync import run
    from _sync import shell_quote
    from _sync import ssh_command
else:
    from ._sync import DEFAULT_HOST
    from ._sync import DEFAULT_REMOTE_PATH
    from ._sync import REMOTE_TARGET_LOCK_PATH
    from ._sync import RemoteWorkflow
    from ._sync import require_command
    from ._sync import run
    from ._sync import shell_quote
    from ._sync import ssh_command


DEFAULT_MAX_AGE_DAYS = 14
DEFAULT_MAX_SIZE_GIB = 80
TARGET_RELATIVE_PATH = "codex-rs/target"


@dataclass(frozen=True)
class CleanupConfig:
    host: str
    remote_path: str
    max_age_days: int
    max_size_gib: int
    execute: bool


def positive_integer(value: str) -> int:
    parsed = int(value)
    if parsed < 1:
        raise argparse.ArgumentTypeError("must be at least 1")
    return parsed


def argument_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        description=(
            "Safely remove stale, rebuildable Cargo incremental cache generations "
            "from the remote Codex checkout."
        ),
        epilog=(
            "The command never synchronizes Git, so it remains usable when the "
            "remote disk is full. It is a dry run by default. Deletion requires "
            "the target cache exceeds the configured capacity limit, a candidate "
            "generation exceeds the age limit, no active Cargo or rustc process "
            "exists, and the shared target cache lock is available.\n\n"
            "Examples:\n"
            "  uv run --project scripts python "
            "scripts/remote/cleanup_build_cache.py --dry-run\n"
            "  uv run --project scripts python "
            "scripts/remote/cleanup_build_cache.py --execute "
            "--max-size-gib 100 --max-age-days 21"
        ),
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    parser.add_argument(
        "--max-age-days",
        type=positive_integer,
        default=DEFAULT_MAX_AGE_DAYS,
        help=(
            "minimum age of a generation's newest artifact before it is eligible "
            f"for deletion (default: {DEFAULT_MAX_AGE_DAYS})"
        ),
    )
    parser.add_argument(
        "--max-size-gib",
        type=positive_integer,
        default=DEFAULT_MAX_SIZE_GIB,
        help=(
            "minimum total target size before generation cleanup is eligible "
            f"(default: {DEFAULT_MAX_SIZE_GIB})"
        ),
    )
    mode = parser.add_mutually_exclusive_group()
    mode.add_argument(
        "--dry-run",
        action="store_true",
        help="preview the eligibility decision without deleting files (default)",
    )
    mode.add_argument(
        "--execute",
        action="store_true",
        help="delete eligible stale incremental generations after all safeguards pass",
    )
    return parser


def parse_args(argv: Sequence[str]) -> CleanupConfig:
    args = argument_parser().parse_args(argv)
    return CleanupConfig(
        host=DEFAULT_HOST,
        remote_path=DEFAULT_REMOTE_PATH,
        max_age_days=args.max_age_days,
        max_size_gib=args.max_size_gib,
        execute=args.execute,
    )


def cleanup_command(config: CleanupConfig) -> str:
    mode = "execute" if config.execute else "dry-run"
    max_size_kib = config.max_size_gib * 1024 * 1024
    remove_command = (
        'rm -rf -- "$candidate_path"; '
        'removed_generations="$((removed_generations + 1))"; '
        'target_size_kib="$((target_size_kib - candidate_size_kib))"; '
        'printf "removed stale incremental generation: %s (%s KiB remaining target: %s KiB)\\n" '
        '"$candidate_path" "$candidate_size_kib" "$target_size_kib"; '
        'if [ "$target_size_kib" -lt "$max_size_kib" ]; then break 2; fi'
        if config.execute
        else 'printf "dry run: would remove stale incremental generation: %s (%s KiB)\\n" '
        '"$candidate_path" "$candidate_size_kib"'
    )
    return (
        "set -euo pipefail; "
        f"cd {shell_quote(config.remote_path)}; "
        f"target_path={shell_quote(TARGET_RELATIVE_PATH)}; "
        f"lock_path={shell_quote(REMOTE_TARGET_LOCK_PATH)}; "
        f"max_age_days={config.max_age_days}; "
        f"max_size_kib={max_size_kib}; "
        f"mode={shell_quote(mode)}; "
        'printf "remote target cache cleanup: mode=%s, max-age-days=%s, max-size-gib=%s\\n" '
        f'"$mode" "$max_age_days" {config.max_size_gib}; '
        'if [ ! -d "$target_path" ]; then '
        'printf "%s\\n" "target cache is already absent: $target_path"; '
        "exit 0; "
        "fi; "
        "if ! command -v flock >/dev/null 2>&1; then "
        'printf "%s\\n" "refusing cleanup: flock is required for target cache coordination" >&2; '
        "exit 2; "
        "fi; "
        'exec 9>"$lock_path"; '
        "if ! flock -n -x 9; then "
        'printf "%s\\n" "refusing cleanup: a remote build holds the target cache lock" >&2; '
        "exit 1; "
        "fi; "
        'active_processes="$(pgrep -a -x cargo || true; pgrep -a -x rustc || true)"; '
        'if [ -n "$active_processes" ]; then '
        'printf "%s\\n%s\\n" "refusing cleanup: active Cargo or rustc process detected" "$active_processes" >&2; '
        "exit 1; "
        "fi; "
        'cargo_lock="$(find "$target_path" -name .cargo-lock -print -quit)"; '
        'if [ -n "$cargo_lock" ]; then '
        'printf "%s\\n" "refusing cleanup: Cargo lock detected at $cargo_lock" >&2; '
        "exit 1; "
        "fi; "
        'target_size_kib="$(du -sk -- "$target_path" | cut -f1)"; '
        'printf "target cache size: %s KiB (threshold: %s KiB)\\n" "$target_size_kib" "$max_size_kib"; '
        'if [ "$target_size_kib" -lt "$max_size_kib" ]; then '
        'printf "%s\\n" "target cache is below the configured capacity threshold"; '
        "exit 0; "
        "fi; "
        'age_seconds="$((max_age_days * 24 * 60 * 60))"; '
        'now_epoch="$(date +%s)"; '
        "removed_generations=0; "
        "eligible_generations=0; "
        'while IFS= read -r -d "" incremental_path; do '
        'while IFS= read -r -d "" candidate_path; do '
        'newest_epoch="$(find "$candidate_path" -type f -printf "%T@\\n" | sort -nr | head -n 1)"; '
        'if [ -z "$newest_epoch" ]; then newest_epoch="$(stat -c %Y -- "$candidate_path")"; fi; '
        'if ! awk -v now="$now_epoch" -v newest="$newest_epoch" -v age="$age_seconds" '
        "'BEGIN { exit (now - newest >= age) ? 0 : 1 }'; then "
        "continue; "
        "fi; "
        'candidate_size_kib="$(du -sk -- "$candidate_path" | cut -f1)"; '
        'eligible_generations="$((eligible_generations + 1))"; '
        f"{remove_command}; "
        'done < <(find "$incremental_path" -mindepth 1 -maxdepth 1 -type d -print0); '
        'done < <(find "$target_path" -type d -name incremental -print0); '
        'if [ "$eligible_generations" -eq 0 ]; then '
        'printf "%s\\n" "no stale incremental generation exceeds the configured age threshold"; '
        'elif [ "$mode" = dry-run ]; then '
        'printf "dry run: %s stale incremental generation(s) are eligible\\n" "$eligible_generations"; '
        "else "
        'printf "removed %s stale incremental generation(s); target cache now uses %s KiB\\n" '
        '"$removed_generations" "$target_size_kib"; '
        "fi"
    )


def main(argv: Sequence[str] | None = None) -> int:
    require_command("ssh")
    config = parse_args(tuple(argv if argv is not None else sys.argv[1:]))
    workflow = RemoteWorkflow(
        host=config.host,
        branch="main",
        remote_path=config.remote_path,
        command=(),
    )
    run(ssh_command(workflow, cleanup_command(config)))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
