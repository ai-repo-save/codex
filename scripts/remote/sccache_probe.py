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
    from _sync import DEFAULT_TARGET
    from _sync import RemoteWorkflow
    from _sync import remote_build_shell_command
    from _sync import run_remote_workflow
    from _sync import shell_quote
else:
    from ._sync import DEFAULT_BRANCH
    from ._sync import DEFAULT_HOST
    from ._sync import DEFAULT_REMOTE_PATH
    from ._sync import DEFAULT_TARGET
    from ._sync import RemoteWorkflow
    from ._sync import remote_build_shell_command
    from ._sync import run_remote_workflow
    from ._sync import shell_quote


DEFAULT_PACKAGE = "codex-utils-fuzzy-match"


def argument_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        description=(
            "Verify remote Rust compiler-cache reuse with two clean builds "
            "of the same library."
        ),
        epilog=(
            "The probe resets sccache counters, creates two temporary Cargo target "
            "states at the same path, builds the selected library once in each "
            "state, prints statistics after both builds, and removes the temporary "
            "directory.\n\n"
            "Example:\n"
            "  uv run --project scripts python scripts/remote/sccache_probe.py "
            "--package codex-utils-fuzzy-match"
        ),
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    parser.add_argument(
        "--branch",
        default=DEFAULT_BRANCH,
        help=f"local and remote Git branch to synchronize (default: {DEFAULT_BRANCH})",
    )
    parser.add_argument(
        "--package",
        default=DEFAULT_PACKAGE,
        help=f"workspace library package to build twice (default: {DEFAULT_PACKAGE})",
    )
    return parser


def probe_command(package: str) -> str:
    quoted_package = shell_quote(package)
    return (
        "set -euo pipefail; "
        "if ! command -v sccache >/dev/null 2>&1; then "
        'echo "sccache probe: sccache is required" >&2; exit 2; '
        "fi; "
        'probe_root="$(mktemp -d /var/tmp/codex-sccache-probe.XXXXXX)"; '
        "trap 'rm -rf -- \"$probe_root\"' EXIT; "
        "sccache --zero-stats >/dev/null; "
        "cd codex-rs; "
        'echo "sccache probe: first clean build"; '
        f'CARGO_TARGET_DIR="$probe_root/target" cargo build --locked -p {quoted_package} --lib; '
        'echo "sccache probe: stats after first build"; '
        "sccache --show-stats; "
        'rm -rf -- "$probe_root/target"; '
        'echo "sccache probe: second clean build at the same path"; '
        f'CARGO_TARGET_DIR="$probe_root/target" cargo build --locked -p {quoted_package} --lib'
    )


def main(argv: Sequence[str] | None = None) -> int:
    args = argument_parser().parse_args(argv)
    command = remote_build_shell_command(DEFAULT_TARGET, probe_command(args.package))
    return run_remote_workflow(
        RemoteWorkflow(
            host=DEFAULT_HOST,
            branch=args.branch,
            remote_path=DEFAULT_REMOTE_PATH,
            command=("bash", "-lc", command),
        )
    )


if __name__ == "__main__":
    raise SystemExit(main())
