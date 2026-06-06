#!/usr/bin/env -S uv run python
from __future__ import annotations

import logging
import os
import shutil
import subprocess
import sys
import time
from dataclasses import dataclass
from pathlib import Path, PurePosixPath
from typing import Sequence


SCRIPT_DIR = Path(__file__).resolve().parent
REPO_ROOT = SCRIPT_DIR.parents[1]
SCRIPTS_DIR = REPO_ROOT / "scripts"
sys.path.insert(0, str(SCRIPTS_DIR))

if __package__ in (None, ""):
    sys.path.insert(0, str(SCRIPT_DIR))
    from _sync import DEFAULT_BRANCH
    from _sync import DEFAULT_HOST
    from _sync import DEFAULT_REMOTE_PATH
    from _sync import RemoteWorkflow
    from _sync import git_output
    from _sync import require_command
    from _sync import run
    from _sync import run_remote_workflow
    from _sync import shell_quote
else:
    from ._sync import DEFAULT_BRANCH
    from ._sync import DEFAULT_HOST
    from ._sync import DEFAULT_REMOTE_PATH
    from ._sync import RemoteWorkflow
    from ._sync import git_output
    from ._sync import require_command
    from ._sync import run
    from ._sync import run_remote_workflow
    from ._sync import shell_quote

from codex_package.layout import validate_package_dir  # noqa: E402
from codex_package.targets import PACKAGE_VARIANTS  # noqa: E402
from codex_package.targets import TARGET_SPECS  # noqa: E402
from codex_package.timing import timed_step  # noqa: E402
from install.install_local_standalone import DEFAULT_CARGO_PROFILE  # noqa: E402
from install.install_local_standalone import DEFAULT_VARIANT  # noqa: E402
from install.install_local_standalone import InstallPaths  # noqa: E402
from install.install_local_standalone import finalize_package_layout  # noqa: E402
from install.install_local_standalone import update_install_links  # noqa: E402
from install.install_local_standalone import verify_install  # noqa: E402


LOGGER = logging.getLogger("remote_install_local_standalone")
NO_ARGUMENTS_MESSAGE = (
    "scripts/remote/install_local_standalone.py does not accept arguments; it "
    "only builds and installs the fixed local standalone Codex package."
)


@dataclass(frozen=True)
class StandaloneInstallConfig:
    host: str = DEFAULT_HOST
    branch: str = DEFAULT_BRANCH
    remote_path: str = DEFAULT_REMOTE_PATH
    target: str = "x86_64-unknown-linux-gnu"
    variant: str = DEFAULT_VARIANT
    cargo_profile: str = DEFAULT_CARGO_PROFILE
    codex_home: Path | None = None
    bin_dir: Path | None = None


@dataclass(frozen=True)
class CommitInfo:
    full_hash: str
    short_hash: str
    timestamp: int


def main(argv: Sequence[str] | None = None) -> int:
    configure_logging()
    args = tuple(argv if argv is not None else sys.argv[1:])
    if args:
        print(NO_ARGUMENTS_MESSAGE, file=sys.stderr)
        return 2

    install_local_standalone(StandaloneInstallConfig())
    return 0


def configure_logging() -> None:
    logging.basicConfig(
        format="%(levelname)s %(name)s: %(message)s",
        level=logging.INFO,
    )


def install_local_standalone(config: StandaloneInstallConfig) -> None:
    require_command("rsync")
    repo_root = git_repo_root()
    commit = current_commit(repo_root)
    remote_package_dir = remote_package_path(commit)
    paths = InstallPaths.resolve(
        codex_home=config.codex_home,
        bin_dir=config.bin_dir,
        target=config.target,
        variant=config.variant,
        cargo_profile=config.cargo_profile,
    )

    LOGGER.info("remote package directory: %s", remote_package_dir)
    with timed_step(LOGGER, "remote package build workflow"):
        run_remote_workflow(
            RemoteWorkflow(
                host=config.host,
                branch=config.branch,
                remote_path=config.remote_path,
                command=remote_build_command(config, remote_package_dir),
            )
        )

    staging_dir = paths.releases_dir / (
        f".staging.{paths.release_name}.{commit.short_hash}.{os.getpid()}"
    )
    install_remote_package(
        config=config,
        paths=paths,
        remote_package_dir=remote_package_dir,
        staging_dir=staging_dir,
        commit=commit,
    )


def git_repo_root() -> Path:
    return Path(
        subprocess.run(
            ("git", "rev-parse", "--show-toplevel"),
            check=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
        ).stdout.strip()
    )


def current_commit(repo_root: Path) -> CommitInfo:
    full_hash = git_output(repo_root, "rev-parse", "HEAD")
    timestamp = int(git_output(repo_root, "log", "-1", "--format=%ct"))
    return CommitInfo(
        full_hash=full_hash,
        short_hash=full_hash[:12],
        timestamp=timestamp,
    )


def remote_package_path(commit: CommitInfo) -> PurePosixPath:
    return PurePosixPath("/tmp") / f"codex-standalone-{commit.short_hash}"


def remote_build_command(
    config: StandaloneInstallConfig, remote_package_dir: PurePosixPath
) -> tuple[str, ...]:
    command = (
        f"rm -rf {shell_quote(str(remote_package_dir))} && "
        f"{remote_build_env_command(config.target)} && "
        "uv run --project scripts python scripts/build_codex_package.py "
        f"--target {shell_quote(config.target)} "
        f"--variant {shell_quote(config.variant)} "
        f"--cargo-profile {shell_quote(config.cargo_profile)} "
        f"--package-dir {shell_quote(str(remote_package_dir))} "
        "--force"
    )
    return ("bash", "-lc", command)


def remote_build_env_command(target: str) -> str:
    linker_env = cargo_target_linker_env_key(target)
    return (
        "if command -v sccache >/dev/null 2>&1; then "
        "export RUSTC_WRAPPER=\"$(command -v sccache)\"; "
        "echo \"remote build: using RUSTC_WRAPPER=$RUSTC_WRAPPER\"; "
        "else echo \"remote build: sccache not found\"; fi; "
        "if command -v clang >/dev/null 2>&1 && command -v mold >/dev/null 2>&1; then "
        f"export {linker_env}=clang; "
        "export RUSTFLAGS=\"${RUSTFLAGS:+$RUSTFLAGS }-C link-arg=-fuse-ld=mold\"; "
        "echo \"remote build: using clang with mold\"; "
        "elif command -v clang >/dev/null 2>&1 && command -v ld.lld >/dev/null 2>&1; then "
        f"export {linker_env}=clang; "
        "export RUSTFLAGS=\"${RUSTFLAGS:+$RUSTFLAGS }-C link-arg=-fuse-ld=lld\"; "
        "echo \"remote build: using clang with lld\"; "
        "else echo \"remote build: no fast linker configured\"; fi"
    )


def cargo_target_linker_env_key(target: str) -> str:
    return f"CARGO_TARGET_{target.upper().replace('-', '_')}_LINKER"


def install_remote_package(
    *,
    config: StandaloneInstallConfig,
    paths: InstallPaths,
    remote_package_dir: PurePosixPath,
    staging_dir: Path,
    commit: CommitInfo,
) -> None:
    if staging_dir.exists() or staging_dir.is_symlink():
        raise RuntimeError(f"staging directory already exists: {staging_dir}")
    staging_dir.parent.mkdir(parents=True, exist_ok=True)
    prefill_staging_dir(paths.current_link, staging_dir)
    with timed_step(
        LOGGER, f"copying remote package to local staging directory {staging_dir}"
    ):
        rsync_remote_package(config.host, remote_package_dir, staging_dir)

    try:
        with timed_step(LOGGER, f"preparing local staging package {staging_dir}"):
            finalize_package_layout(staging_dir)
            validate_package_dir(
                staging_dir,
                PACKAGE_VARIANTS[config.variant],
                TARGET_SPECS[config.target],
                include_zsh=True,
            )
        with timed_step(LOGGER, f"activating standalone release {paths.release_dir}"):
            activate_release_with_backup(staging_dir, paths.release_dir, commit)
            update_install_links(paths)
        with timed_step(LOGGER, "verifying installed standalone package"):
            verify_install(paths, variant=config.variant)
            verify_entrypoint_mtime(paths.bin_path.resolve(), commit)
    except Exception:
        if staging_dir.exists():
            failed_staging = move_to_backup(
                staging_dir,
                paths.releases_dir / "backups",
                f"failed-staging.{paths.release_name}",
                commit,
            )
            LOGGER.error("moved failed staging directory to %s", failed_staging)
        raise

    LOGGER.info("installed standalone Codex from %s", commit.short_hash)
    LOGGER.info("active command: %s", paths.bin_path)


def rsync_remote_package(
    host: str, remote_package_dir: PurePosixPath, staging_dir: Path
) -> None:
    run(
        (
            "rsync",
            "--archive",
            "--delete",
            "--compress",
            "--compress-choice=zstd",
            "--compress-level=1",
            f"{host}:{remote_package_dir}/",
            f"{staging_dir}/",
        )
    )


def prefill_staging_dir(current_link: Path, staging_dir: Path) -> None:
    if not current_link.is_symlink() and not current_link.exists():
        LOGGER.info("no current standalone release to prefill staging directory")
        return

    current_release = current_link.resolve()
    if not current_release.is_dir():
        LOGGER.info("current standalone release is not a directory: %s", current_release)
        return

    with timed_step(LOGGER, f"prefilling staging directory from {current_release}"):
        shutil.copytree(
            current_release,
            staging_dir,
            copy_function=os.link,
            symlinks=True,
        )


def activate_release_with_backup(
    staging_dir: Path, release_dir: Path, commit: CommitInfo
) -> None:
    if release_dir.exists() or release_dir.is_symlink():
        backup_dir = move_to_backup(
            release_dir,
            release_dir.parent / "backups",
            release_dir.name,
            commit,
        )
        LOGGER.info("moved previous release to %s", backup_dir)
    staging_dir.replace(release_dir)


def move_to_backup(
    path: Path, backup_root: Path, name_prefix: str, commit: CommitInfo
) -> Path:
    backup_root.mkdir(parents=True, exist_ok=True)
    timestamp = time.strftime("%Y%m%dT%H%M%S", time.localtime())
    backup_dir = backup_root / f"{name_prefix}.{timestamp}.{commit.short_hash}"
    if backup_dir.exists():
        raise RuntimeError(f"backup path already exists: {backup_dir}")
    shutil.move(str(path), str(backup_dir))
    return backup_dir


def verify_entrypoint_mtime(entrypoint: Path, commit: CommitInfo) -> None:
    if not entrypoint.is_file():
        raise RuntimeError(f"installed codex entrypoint does not exist: {entrypoint}")
    mtime = int(entrypoint.stat().st_mtime)
    if mtime <= commit.timestamp:
        raise RuntimeError(
            f"installed codex is older than commit {commit.short_hash}: "
            f"entrypoint mtime={mtime}, commit timestamp={commit.timestamp}"
        )


if __name__ == "__main__":
    raise SystemExit(main())
