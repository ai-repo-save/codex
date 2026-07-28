#!/usr/bin/env -S uv run python

import argparse
import sys
from enum import Enum
from pathlib import Path
from typing import Sequence


if __package__ in (None, ""):
    sys.path.insert(0, str(Path(__file__).resolve().parent))
    from _sync import DEFAULT_HOST
    from _sync import DEFAULT_REMOTE_PATH
    from _sync import RemoteWorkflow
    from _sync import run
    from _sync import shell_quote
    from _sync import ssh_command
else:
    from ._sync import DEFAULT_HOST
    from ._sync import DEFAULT_REMOTE_PATH
    from ._sync import RemoteWorkflow
    from ._sync import run
    from ._sync import shell_quote
    from ._sync import ssh_command


class CleanupScope(str, Enum):
    INCREMENTAL = "incremental"
    CARGO_TARGET = "cargo-target"


def argument_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        description="Remove rebuildable Cargo artifacts from the remote Codex checkout.",
        epilog=(
            "This maintenance command does not synchronize Git, so it remains usable "
            "when the remote disk is full. It previews disk usage by default; pass "
            "--execute to delete the selected cache scope.\n\n"
            "Examples:\n"
            "  scripts/remote/cleanup_build_cache.py --scope incremental\n"
            "  scripts/remote/cleanup_build_cache.py --scope cargo-target --execute"
        ),
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    parser.add_argument(
        "--scope",
        choices=tuple(scope.value for scope in CleanupScope),
        default=CleanupScope.INCREMENTAL.value,
        help="rebuildable Cargo artifacts to inspect or remove (default: incremental)",
    )
    parser.add_argument(
        "--execute",
        action="store_true",
        help="delete the selected artifacts after showing their size",
    )
    return parser


def cleanup_command(
    remote_path: str,
    scope: CleanupScope,
    execute: bool,
) -> str:
    relative_path = {
        CleanupScope.INCREMENTAL: "codex-rs/target/debug/incremental",
        CleanupScope.CARGO_TARGET: "codex-rs/target",
    }[scope]
    action = (
        'rm -rf -- "$path"; printf "%s\\n" "removed $path"'
        if execute
        else 'printf "%s\\n" "preview only; pass --execute to remove $path"'
    )
    script = (
        f"set -euo pipefail; cd {shell_quote(remote_path)}; "
        f"path={relative_path!r}; "
        "df -h .; "
        'if [ -e "$path" ]; then du -sh -- "$path"; '
        f"{action}; "
        'else printf "%s\\n" "$path is already absent"; fi; '
        "df -h ."
    )
    return script


def main(argv: Sequence[str] | None = None) -> int:
    args = argument_parser().parse_args(argv)
    scope = CleanupScope(args.scope)
    config = RemoteWorkflow(
        host=DEFAULT_HOST,
        branch="",
        remote_path=DEFAULT_REMOTE_PATH,
        command=(),
    )
    run(
        ssh_command(
            config,
            cleanup_command(config.remote_path, scope, args.execute),
        )
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
