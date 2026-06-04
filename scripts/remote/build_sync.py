#!/usr/bin/env -S uv run python
from __future__ import annotations

import sys
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


SMOKE_COMMAND = ("just", "codex", "--version")
NO_ARGUMENTS_MESSAGE = (
    "scripts/remote/build_sync.py does not accept arguments; it only runs the "
    "fixed remote smoke workflow."
)


def main(argv: Sequence[str] | None = None) -> int:
    args = tuple(argv if argv is not None else sys.argv[1:])
    if args:
        print(NO_ARGUMENTS_MESSAGE, file=sys.stderr)
        return 2

    return run_remote_workflow(
        RemoteWorkflow(
            host=DEFAULT_HOST,
            branch=DEFAULT_BRANCH,
            remote_path=DEFAULT_REMOTE_PATH,
            command=SMOKE_COMMAND,
        )
    )


if __name__ == "__main__":
    raise SystemExit(main())
