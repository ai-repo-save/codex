#!/usr/bin/env -S uv run python
from __future__ import annotations

import argparse
import sys
from dataclasses import dataclass
from pathlib import Path
from pathlib import PurePosixPath
from typing import Sequence


if __package__ in (None, ""):
    sys.path.insert(0, str(Path(__file__).resolve().parent))
    from _sync import DEFAULT_HOST
    from _sync import RemoteWorkflow
    from _sync import require_command
    from _sync import run
    from _sync import shell_quote
    from _sync import ssh_command
else:
    from ._sync import DEFAULT_HOST
    from ._sync import RemoteWorkflow
    from ._sync import require_command
    from ._sync import run
    from ._sync import shell_quote
    from ._sync import ssh_command


DOTSLASH_VERSION = "0.5.8"
DEFAULT_INSTALL_DIR = "/usr/local/bin"


@dataclass(frozen=True)
class InstallDotslashConfig:
    host: str
    install_dir: PurePosixPath


def parse_args(argv: Sequence[str]) -> InstallDotslashConfig:
    parser = argparse.ArgumentParser(
        description=(
            "Install DotSlash on the remote execution host when the remote "
            "toolchain is missing it."
        ),
        epilog=(
            "Side effects: connects to the remote host over SSH, downloads "
            f"DotSlash {DOTSLASH_VERSION} from GitHub releases, and writes "
            "the binary into the remote install directory. Example: "
            "uv run --project scripts python scripts/remote/install_dotslash.py"
        ),
    )
    parser.add_argument(
        "--host",
        default=DEFAULT_HOST,
        help=f"remote host to configure (default: {DEFAULT_HOST})",
    )
    parser.add_argument(
        "--install-dir",
        default=DEFAULT_INSTALL_DIR,
        help=f"remote directory for the dotslash binary (default: {DEFAULT_INSTALL_DIR})",
    )
    args = parser.parse_args(argv)
    install_dir = PurePosixPath(args.install_dir)
    if not install_dir.is_absolute():
        parser.error("--install-dir must be an absolute remote path")
    return InstallDotslashConfig(host=args.host, install_dir=install_dir)


def install_command(config: InstallDotslashConfig) -> str:
    install_dir = shell_quote(str(config.install_dir))
    return (
        "set -euo pipefail; "
        "if command -v dotslash >/dev/null 2>&1; then "
        'printf "dotslash already installed at %s\\n" "$(command -v dotslash)"; '
        "dotslash --version; "
        "exit 0; "
        "fi; "
        'arch="$(uname -m)"; '
        'case "$arch" in x86_64|aarch64) ;; *) '
        'printf "unsupported remote architecture for dotslash: %s\\n" "$arch" >&2; '
        "exit 2; "
        ";; esac; "
        'tmpdir="$(mktemp -d)"; '
        'trap \'rm -rf "$tmpdir"\' EXIT; '
        f"url=https://github.com/facebook/dotslash/releases/download/v{DOTSLASH_VERSION}/dotslash-ubuntu-22.04.$arch.tar.gz; "
        'printf "installing dotslash from %s\\n" "$url"; '
        'timeout 120 curl -LSfs "$url" -o "$tmpdir/dotslash.tar.gz"; '
        f"mkdir -p {install_dir}; "
        f"tar -xzf \"$tmpdir/dotslash.tar.gz\" -C {install_dir}; "
        f"{install_dir}/dotslash --version; "
        f'printf "dotslash installed at %s/dotslash\\n" {install_dir}'
    )


def main(argv: Sequence[str] | None = None) -> int:
    require_command("ssh")
    config = parse_args(tuple(argv if argv is not None else sys.argv[1:]))
    workflow = RemoteWorkflow(
        host=config.host,
        branch="main",
        remote_path="/",
        command=(),
    )
    run(ssh_command(workflow, install_command(config)))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
