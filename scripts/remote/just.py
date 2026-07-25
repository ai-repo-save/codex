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


def argument_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        description=(
            "Run a codex-rs just recipe on the remote execution host after "
            "syncing the selected local branch."
        ),
        epilog=(
            "Example: scripts/remote/just.py --branch sync/rust-v0.146.0 "
            "test -p codex-core context_anchor"
        ),
    )
    parser.add_argument(
        "--branch",
        default=DEFAULT_BRANCH,
        help=f"local and remote Git branch to synchronize (default: {DEFAULT_BRANCH})",
    )
    parser.add_argument("recipe", help="just recipe to run")
    parser.add_argument(
        "recipe_args",
        nargs=argparse.REMAINDER,
        help="arguments forwarded unchanged to the just recipe",
    )
    return parser


def main(argv: Sequence[str] | None = None) -> int:
    args = argument_parser().parse_args(argv)
    recipe_args = (args.recipe, *args.recipe_args)

    return run_remote_workflow(
        RemoteWorkflow(
            host=DEFAULT_HOST,
            branch=args.branch,
            remote_path=DEFAULT_REMOTE_PATH,
            command=remote_codex_rs_just_command(recipe_args),
        )
    )


if __name__ == "__main__":
    raise SystemExit(main())
