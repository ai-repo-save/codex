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
    from _sync import remote_codex_rs_just_command
    from _sync import run_remote_workflow
else:
    from ._sync import DEFAULT_BRANCH
    from ._sync import DEFAULT_HOST
    from ._sync import DEFAULT_REMOTE_PATH
    from ._sync import RemoteWorkflow
    from ._sync import remote_codex_rs_just_command
    from ._sync import run_remote_workflow


# Do not use this as complete TUI validation. It is only a fixed compile/RPC
# smoke check for the remote TUI test graph. Routine TUI validation still needs
# a test filter that matches the behavior being changed. The unfiltered
# `just test -p codex-tui` command runs the entire crate, including
# environment-sensitive snapshots, and is not a routine development gate.
TUI_SMOKE_JUST_ARGS = (
    "test",
    "-p",
    "codex-tui",
    "embedded_app_server_supports_thread_start_rpc",
)
NO_ARGUMENTS_MESSAGE = (
    "scripts/remote/tui_smoke.py does not accept arguments; it runs the fixed "
    "codex-tui smoke validation command."
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
            command=remote_codex_rs_just_command(TUI_SMOKE_JUST_ARGS),
        )
    )


if __name__ == "__main__":
    raise SystemExit(main())
