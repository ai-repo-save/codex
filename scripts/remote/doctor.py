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
    from _sync import run
    from _sync import shell_quote
    from _sync import ssh_command
else:
    from ._sync import DEFAULT_BRANCH
    from ._sync import DEFAULT_HOST
    from ._sync import DEFAULT_REMOTE_PATH
    from ._sync import RemoteWorkflow
    from ._sync import run
    from ._sync import shell_quote
    from ._sync import ssh_command


def argument_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        description=(
            "Check remote Git, network, and toolchain prerequisites for a "
            "Codex workflow."
        ),
        epilog=("Example: scripts/remote/doctor.py --branch sync/rust-v0.146.0"),
    )
    parser.add_argument(
        "--branch",
        default=DEFAULT_BRANCH,
        help=f"origin branch whose availability is checked (default: {DEFAULT_BRANCH})",
    )
    return parser


def main(argv: Sequence[str] | None = None) -> int:
    args = argument_parser().parse_args(argv)

    config = RemoteWorkflow(
        host=DEFAULT_HOST,
        branch=args.branch,
        remote_path=DEFAULT_REMOTE_PATH,
        command=(),
    )
    command = (
        "set -euo pipefail; "
        'printf "%s\\n" "== remote toolchain =="; '
        "for tool in git sccache clang mold ld.lld; do "
        'if command -v "$tool" >/dev/null 2>&1; then '
        'printf "%s %s\\n" "$tool" "$(command -v "$tool")"; '
        'else printf "%s missing\\n" "$tool"; fi; '
        "done; "
        'printf "%s\\n" "== remote dns =="; '
        "getent hosts google.com github.com || true; "
        'printf "%s\\n" "== remote google https =="; '
        'timeout 15 curl -I -L --connect-timeout 5 https://www.google.com 2>&1 | sed -n "1,12p"; '
        'printf "%s\\n" "== remote git origin =="; '
        f"cd {shell_quote(config.remote_path)}; "
        "timeout 45 git ls-remote --exit-code --heads origin "
        f'{shell_quote(config.branch)} 2>&1 | sed -n "1,40p"'
    )
    run(ssh_command(config, command))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
