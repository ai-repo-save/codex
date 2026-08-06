#!/usr/bin/env python3
"""Run focused local `cargo check` for obvious Rust errors before remote validation.

Uses an out-of-tree ``CARGO_TARGET_DIR`` so the workspace is not filled with a local
``target/``. This is a preflight for missing imports and type errors, not a substitute
for remote ``just.py test`` / ``fix``.

Examples:
  uv run --project scripts python scripts/local/rust_check.py -p codex-hooks
  uv run --project scripts python scripts/local/rust_check.py
  uv run --project scripts python scripts/local/rust_check.py --from-git
"""

from __future__ import annotations

import argparse
import logging
import os
import subprocess
import sys
import tomllib
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]
CODEX_RS = REPO_ROOT / "codex-rs"
DEFAULT_TARGET_DIR = (
    Path(os.environ.get("XDG_CACHE_HOME", Path.home() / ".cache"))
    / "codex-local-cargo-target"
)

logger = logging.getLogger("rust_check")


def parse_args(argv: list[str]) -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description=(
            "Run focused local cargo check for touched or selected crates. "
            "Writes build artifacts under an out-of-tree CARGO_TARGET_DIR."
        )
    )
    parser.add_argument(
        "-p",
        "--package",
        action="append",
        default=[],
        help="Cargo package name to check (repeatable). Default: discover from git changes.",
    )
    parser.add_argument(
        "--from-git",
        action="store_true",
        help="Discover packages from dirty/staged Rust paths under codex-rs (default when -p omitted).",
    )
    parser.add_argument(
        "--target-dir",
        type=Path,
        default=DEFAULT_TARGET_DIR,
        help=f"CARGO_TARGET_DIR (default: {DEFAULT_TARGET_DIR})",
    )
    parser.add_argument(
        "--allow-core",
        action="store_true",
        help="Allow checking codex-core (slow on this machine; off by default when auto-discovered).",
    )
    return parser.parse_args(argv)


def package_name_for_path(path: Path) -> str | None:
    current = path if path.is_dir() else path.parent
    while True:
        cargo_toml = current / "Cargo.toml"
        if cargo_toml.is_file():
            data = tomllib.loads(cargo_toml.read_text(encoding="utf-8"))
            package = data.get("package")
            if isinstance(package, dict):
                name = package.get("name")
                if isinstance(name, str) and name:
                    return name
            if "workspace" in data and current == CODEX_RS:
                return None
        if current == CODEX_RS or current == current.parent:
            return None
        current = current.parent


def git_changed_codex_rs_paths() -> list[Path]:
    commands = (
        ["git", "diff", "--name-only", "--diff-filter=ACMR", "HEAD", "--", "codex-rs"],
        [
            "git",
            "diff",
            "--name-only",
            "--diff-filter=ACMR",
            "--cached",
            "--",
            "codex-rs",
        ],
        ["git", "ls-files", "--others", "--exclude-standard", "--", "codex-rs"],
    )
    paths: set[Path] = set()
    for command in commands:
        completed = subprocess.run(
            command,
            cwd=REPO_ROOT,
            check=False,
            capture_output=True,
            text=True,
        )
        if completed.returncode != 0:
            stderr = (
                completed.stderr.strip() or completed.stdout.strip() or "git failed"
            )
            raise RuntimeError(stderr)
        for line in completed.stdout.splitlines():
            if not line.strip():
                continue
            path = REPO_ROOT / line
            if path.suffix == ".rs" or path.name == "Cargo.toml":
                paths.add(path)
    return sorted(paths)


def discover_packages(*, allow_core: bool) -> list[str]:
    packages: list[str] = []
    seen: set[str] = set()
    for path in git_changed_codex_rs_paths():
        name = package_name_for_path(path)
        if name is None or name in seen:
            continue
        if name == "codex-core" and not allow_core:
            logger.warning(
                "skipping auto-discovered codex-core (slow locally); "
                "pass -p codex-core or --allow-core to check it"
            )
            continue
        seen.add(name)
        packages.append(name)
    return packages


def run_cargo_check(packages: list[str], target_dir: Path) -> int:
    if not packages:
        logger.error(
            "no packages to check; pass -p <crate> or change Rust files under codex-rs"
        )
        return 2

    target_dir.mkdir(parents=True, exist_ok=True)
    env = os.environ.copy()
    env["CARGO_TARGET_DIR"] = str(target_dir.resolve())

    failed = 0
    for package in packages:
        command = ["cargo", "check", "-p", package, "--message-format=short"]
        logger.info(
            "running %s (CARGO_TARGET_DIR=%s)",
            " ".join(command),
            env["CARGO_TARGET_DIR"],
        )
        completed = subprocess.run(command, cwd=CODEX_RS, env=env, check=False)
        if completed.returncode != 0:
            failed = completed.returncode or 1
            logger.error("cargo check failed for %s", package)
        else:
            logger.info("cargo check ok for %s", package)
    return failed


def main(argv: list[str]) -> int:
    logging.basicConfig(
        level=logging.INFO, format="%(levelname)s: %(message)s", stream=sys.stderr
    )
    args = parse_args(argv)
    try:
        if args.package:
            packages = list(dict.fromkeys(args.package))
        else:
            packages = discover_packages(allow_core=args.allow_core)
    except RuntimeError as exc:
        logger.error("%s", exc)
        return 2

    return run_cargo_check(packages, args.target_dir)


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
