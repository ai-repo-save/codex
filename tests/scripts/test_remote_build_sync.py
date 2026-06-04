import json
import sys
from pathlib import Path
from pathlib import PurePosixPath

import pytest

sys.path.insert(0, str(Path(__file__).resolve().parents[2]))

from scripts.codex_package.zsh import ZSH_RESOURCE_PATH
from scripts.remote import _sync
from scripts.remote import build_sync
from scripts.remote import install_local_standalone
from scripts.remote._sync import RemoteWorkflow
from scripts.remote._sync import StatusPlan
from scripts.remote._sync import parse_porcelain_status
from scripts.remote.install_local_standalone import CommitInfo
from scripts.remote.install_local_standalone import StandaloneInstallConfig


def test_parse_porcelain_status_maps_git_visible_changes() -> None:
    raw_status = (
        b" M AGENTS.md\0"
        b"A  scripts/remote/_sync.py\0"
        b"?? tests/scripts/test_remote_build_sync.py\0"
        b"D  stale.txt\0"
        b"R  new-name.txt\0old-name.txt\0"
    )

    assert parse_porcelain_status(raw_status) == StatusPlan(
        copy_paths=(
            "AGENTS.md",
            "scripts/remote/_sync.py",
            "tests/scripts/test_remote_build_sync.py",
            "new-name.txt",
        ),
        delete_paths=("stale.txt", "old-name.txt"),
    )


@pytest.mark.parametrize(
    "raw_status", [b" M ../outside.txt\0", b" M /tmp/outside.txt\0"]
)
def test_parse_porcelain_status_rejects_paths_outside_checkout(
    raw_status: bytes,
) -> None:
    with pytest.raises(ValueError):
        parse_porcelain_status(raw_status)


def test_build_sync_runs_only_fixed_smoke_workflow(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    calls: list[RemoteWorkflow] = []

    def fake_run_remote_workflow(config: RemoteWorkflow) -> int:
        calls.append(config)
        return 0

    monkeypatch.setattr(build_sync, "run_remote_workflow", fake_run_remote_workflow)

    assert build_sync.main([]) == 0
    assert calls == [
        RemoteWorkflow(
            host=_sync.DEFAULT_HOST,
            branch=_sync.DEFAULT_BRANCH,
            remote_path=_sync.DEFAULT_REMOTE_PATH,
            command=build_sync.SMOKE_COMMAND,
        )
    ]


def test_build_sync_rejects_arguments(capsys: pytest.CaptureFixture[str]) -> None:
    assert build_sync.main(["--", "just", "test"]) == 2
    assert capsys.readouterr().err == f"{build_sync.NO_ARGUMENTS_MESSAGE}\n"


def test_install_local_standalone_rejects_arguments(
    capsys: pytest.CaptureFixture[str],
) -> None:
    assert install_local_standalone.main(["--target", "x86_64-unknown-linux-gnu"]) == 2
    assert (
        capsys.readouterr().err == f"{install_local_standalone.NO_ARGUMENTS_MESSAGE}\n"
    )


def test_remote_build_command_uses_uv_and_fixed_package_inputs() -> None:
    command = install_local_standalone.remote_build_command(
        StandaloneInstallConfig(),
        PurePosixPath("/tmp/codex-standalone-abc123"),
    )

    assert command == (
        "bash",
        "-lc",
        "rm -rf '/tmp/codex-standalone-abc123' && "
        "uv run --project scripts python scripts/build_codex_package.py "
        "--target 'x86_64-unknown-linux-gnu' "
        "--variant 'codex' "
        "--cargo-profile 'dev-small' "
        "--package-dir '/tmp/codex-standalone-abc123' "
        "--force",
    )


def test_install_remote_package_moves_existing_release_to_backup_and_updates_links(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    config = StandaloneInstallConfig(
        codex_home=tmp_path / "codex-home",
        bin_dir=tmp_path / "bin",
    )
    paths = install_local_standalone.InstallPaths.resolve(
        codex_home=config.codex_home,
        bin_dir=config.bin_dir,
        target=config.target,
        variant=config.variant,
        cargo_profile=config.cargo_profile,
    )
    old_marker = paths.release_dir / "old-release"
    old_marker.parent.mkdir(parents=True)
    old_marker.write_text("old\n", encoding="utf-8")
    commit = CommitInfo(
        full_hash="abcdef1234567890",
        short_hash="abcdef123456",
        timestamp=1,
    )

    def fake_rsync_remote_package(
        host: str, remote_package_dir: PurePosixPath, staging_dir: Path
    ) -> None:
        assert host == _sync.DEFAULT_HOST
        assert remote_package_dir == PurePosixPath("/tmp/codex-standalone-abcdef123456")
        write_valid_package(staging_dir)

    monkeypatch.setattr(
        install_local_standalone,
        "rsync_remote_package",
        fake_rsync_remote_package,
    )

    install_local_standalone.install_remote_package(
        config=config,
        paths=paths,
        remote_package_dir=PurePosixPath("/tmp/codex-standalone-abcdef123456"),
        staging_dir=paths.releases_dir / ".staging.test",
        commit=commit,
    )

    assert paths.current_link.resolve() == paths.release_dir
    assert paths.bin_path.resolve() == paths.release_dir / "bin" / "codex"
    assert (paths.release_dir / "codex-package.json").is_file()
    backup_markers = list((paths.releases_dir / "backups").glob("*/old-release"))
    assert len(backup_markers) == 1
    assert backup_markers[0].read_text(encoding="utf-8") == "old\n"


def test_install_remote_package_keeps_current_release_when_staging_is_invalid(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    config = StandaloneInstallConfig(
        codex_home=tmp_path / "codex-home",
        bin_dir=tmp_path / "bin",
    )
    paths = install_local_standalone.InstallPaths.resolve(
        codex_home=config.codex_home,
        bin_dir=config.bin_dir,
        target=config.target,
        variant=config.variant,
        cargo_profile=config.cargo_profile,
    )
    write_valid_package(paths.release_dir)
    paths.current_link.parent.mkdir(parents=True, exist_ok=True)
    paths.current_link.symlink_to(paths.release_dir)
    paths.bin_path.parent.mkdir(parents=True, exist_ok=True)
    paths.bin_path.symlink_to(paths.current_link / "bin" / "codex")
    commit = CommitInfo(
        full_hash="abcdef1234567890",
        short_hash="abcdef123456",
        timestamp=1,
    )

    def fake_rsync_remote_package(
        host: str, remote_package_dir: PurePosixPath, staging_dir: Path
    ) -> None:
        del host, remote_package_dir
        staging_dir.mkdir(parents=True)

    monkeypatch.setattr(
        install_local_standalone,
        "rsync_remote_package",
        fake_rsync_remote_package,
    )

    with pytest.raises(RuntimeError):
        install_local_standalone.install_remote_package(
            config=config,
            paths=paths,
            remote_package_dir=PurePosixPath("/tmp/codex-standalone-abcdef123456"),
            staging_dir=paths.releases_dir / ".staging.invalid",
            commit=commit,
        )

    assert paths.current_link.resolve() == paths.release_dir
    assert paths.bin_path.resolve() == paths.release_dir / "bin" / "codex"


def write_valid_package(package_dir: Path) -> None:
    write_executable(package_dir / "bin" / "codex", "echo codex-cli 0.0.0\n")
    write_executable(package_dir / "codex-path" / "rg", "echo rg\n")
    write_executable(package_dir / "codex-resources" / "bwrap", "echo bwrap\n")
    write_executable(
        package_dir / "codex-resources" / ZSH_RESOURCE_PATH,
        "echo zsh\n",
    )
    metadata = {
        "layoutVersion": 1,
        "version": "0.0.0",
        "target": "x86_64-unknown-linux-gnu",
        "variant": "codex",
        "entrypoint": "bin/codex",
        "resourcesDir": "codex-resources",
        "pathDir": "codex-path",
    }
    (package_dir / "codex-package.json").write_text(
        json.dumps(metadata, indent=2) + "\n",
        encoding="utf-8",
    )


def write_executable(path: Path, body: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(f"#!/bin/sh\n{body}", encoding="utf-8")
    path.chmod(0o755)
