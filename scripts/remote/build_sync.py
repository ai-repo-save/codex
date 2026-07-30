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


SMOKE_JUST_ARGS = ("codex", "--version")


def argument_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        description=(
            "Compile and execute the Codex version smoke check on the remote "
            "execution host."
        ),
        epilog=(
            "Behavior: pushes the selected branch to origin, resets and cleans "
            "the shared remote checkout, then copies Git-visible remote changes "
            "back to the unchanged local checkout.\n\n"
            "Example: scripts/remote/build_sync.py --branch sync/rust-v0.146.0"
        ),
        formatter_class=argparse.RawDescriptionHelpFormatter,
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
            command=remote_codex_rs_just_command(SMOKE_JUST_ARGS),
        )
    )


if __name__ == "__main__":
    raise SystemExit(main())
