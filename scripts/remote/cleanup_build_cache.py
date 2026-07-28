#!/usr/bin/env -S uv run python

import argparse
import sys
from enum import Enum
from pathlib import Path
from typing import Sequence


if __package__ in (None, ""):
    sys.path.insert(0, str(Path(__file__).resolve().parent))
    from _sync import DEFAULT_BRANCH
    from _sync import DEFAULT_HOST
    from _sync import DEFAULT_REMOTE_PATH
    from _sync import RemoteWorkflow
    from _sync import run_remote_workflow
else:
    from ._sync import DEFAULT_BRANCH
    from ._sync import DEFAULT_HOST
    from ._sync import DEFAULT_REMOTE_PATH
    from ._sync import RemoteWorkflow
    from ._sync import run_remote_workflow


class CleanupScope(str, Enum):
    INCREMENTAL = "incremental"
    CARGO_TARGET = "cargo-target"


def argument_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        description="Remove rebuildable Cargo artifacts from the remote Codex checkout.",
        epilog=(
            "The command previews disk usage by default. Pass --execute to delete "
            "the selected cache scope.\n\n"
            "Examples:\n"
            "  scripts/remote/cleanup_build_cache.py --scope incremental\n"
            "  scripts/remote/cleanup_build_cache.py --scope cargo-target --execute"
        ),
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    parser.add_argument(
        "--branch",
        default=DEFAULT_BRANCH,
        help=f"local and remote Git branch to synchronize (default: {DEFAULT_BRANCH})",
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


def cleanup_command(scope: CleanupScope, execute: bool) -> tuple[str, ...]:
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
        "set -euo pipefail; "
        f"path={relative_path!r}; "
        "df -h .; "
        'if [ -e "$path" ]; then du -sh -- "$path"; '
        f"{action}; "
        'else printf "%s\\n" "$path is already absent"; fi; '
        "df -h ."
    )
    return ("bash", "-lc", script)


def main(argv: Sequence[str] | None = None) -> int:
    args = argument_parser().parse_args(argv)
    scope = CleanupScope(args.scope)
    return run_remote_workflow(
        RemoteWorkflow(
            host=DEFAULT_HOST,
            branch=args.branch,
            remote_path=DEFAULT_REMOTE_PATH,
            command=cleanup_command(scope, args.execute),
        )
    )


if __name__ == "__main__":
    raise SystemExit(main())
