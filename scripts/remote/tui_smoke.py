#!/usr/bin/env -S uv run python

import argparse
import sys
from pathlib import Path
from typing import Sequence


if __package__ in (None, ""):
    sys.path.insert(0, str(Path(__file__).resolve().parent))
    from _sync import DEFAULT_BRANCH
    from _sync import DEFAULT_HOST
    from _sync import DEFAULT_REMOTE_PATH
    from _sync import RemoteWorkflow
    from _sync import remote_codex_rs_just_command
    from _sync import run_remote_workflow
else:
    from ._sync import DEFAULT_BRANCH
    from ._sync import DEFAULT_HOST
    from ._sync import DEFAULT_REMOTE_PATH
    from ._sync import RemoteWorkflow
    from ._sync import remote_codex_rs_just_command
    from ._sync import run_remote_workflow


# This fixed compile/RPC smoke check verifies the remote TUI test graph. Routine
# TUI validation uses a test filter that matches the behavior being changed. The
# unfiltered `just test -p codex-tui` command runs the entire crate, including
# its platform-sensitive snapshot set; version-bearing snapshots use stable
# fixtures.
TUI_SMOKE_JUST_ARGS = (
    "test",
    "-p",
    "codex-tui",
    "embedded_app_server_supports_thread_start_rpc",
)


def argument_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        description=(
            "Compile the remote TUI test graph and run the fixed embedded "
            "app-server RPC smoke check."
        ),
        epilog=("Example: scripts/remote/tui_smoke.py --branch sync/rust-v0.146.0"),
    )
    parser.add_argument(
        "--branch",
        default=DEFAULT_BRANCH,
        help=f"local and remote Git branch to synchronize (default: {DEFAULT_BRANCH})",
    )
    return parser


def main(argv: Sequence[str] | None = None) -> int:
    args = argument_parser().parse_args(argv)

    return run_remote_workflow(
        RemoteWorkflow(
            host=DEFAULT_HOST,
            branch=args.branch,
            remote_path=DEFAULT_REMOTE_PATH,
            command=remote_codex_rs_just_command(TUI_SMOKE_JUST_ARGS),
        )
    )


if __name__ == "__main__":
    raise SystemExit(main())
