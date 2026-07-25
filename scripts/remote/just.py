#!/usr/bin/env -S uv run python

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


USAGE = """usage: scripts/remote/just.py <recipe> [recipe args...]

Runs a codex-rs just recipe on the remote execution host after syncing the
remote checkout. The command is executed with the remote sccache and fast-linker
environment used by standalone builds.
"""


def main(argv: Sequence[str] | None = None) -> int:
    args = tuple(argv if argv is not None else sys.argv[1:])
    if not args:
        print(USAGE, file=sys.stderr)
        return 2

    return run_remote_workflow(
        RemoteWorkflow(
            host=DEFAULT_HOST,
            branch=DEFAULT_BRANCH,
            remote_path=DEFAULT_REMOTE_PATH,
            command=remote_codex_rs_just_command(args),
        )
    )


if __name__ == "__main__":
    raise SystemExit(main())
