#!/usr/bin/env -S uv run python

import contextlib
import io
import os
from pathlib import Path
from pathlib import PurePosixPath
import sys
import tempfile
import unittest
from unittest import mock


SCRIPT_DIR = Path(__file__).resolve().parent
sys.path.insert(0, str(SCRIPT_DIR))
sys.path.insert(0, str(SCRIPT_DIR.parents[0]))

import install_local_standalone  # noqa: E402


HOST = "builder"
REMOTE_PACKAGE_DIR = PurePosixPath("/tmp/codex-package")
REMOTE_ARCHIVE = PurePosixPath("/tmp/codex-package.tar.zst")
REMOTE_BUILD_ENV = "prepare-remote-build-environment"
TARGET = "x86_64-unknown-linux-gnu"
VARIANT = "codex"
CARGO_PROFILE = "release"
HELP_USAGE = (
    "uv run --project scripts python scripts/remote/install_local_standalone.py"
)
HELP_ARCHIVE_DESCRIPTION = (
    "Compresses the remote package into a .tar.zst archive before downloading it."
)
COMMIT = install_local_standalone.CommitInfo(
    full_hash="0123456789abcdef",
    short_hash="0123456789ab",
    timestamp=1_750_000_000,
)


class RemoteInstallLocalStandaloneTest(unittest.TestCase):
    def test_main_help_documents_usage_and_archive_transfer(self) -> None:
        stdout = io.StringIO()

        with contextlib.redirect_stdout(stdout):
            result = install_local_standalone.main(("--help",))

        self.assertEqual(result, 0)
        help_text = stdout.getvalue()
        self.assertIn(HELP_USAGE, help_text)
        self.assertIn(HELP_ARCHIVE_DESCRIPTION, help_text)

    def test_main_rejects_arguments_without_starting_install(self) -> None:
        stderr = io.StringIO()

        with (
            mock.patch.object(
                install_local_standalone, "install_local_standalone"
            ) as install,
            contextlib.redirect_stderr(stderr),
        ):
            result = install_local_standalone.main(("unexpected",))

        self.assertEqual(result, 2)
        self.assertEqual(
            stderr.getvalue(),
            f"{install_local_standalone.NO_ARGUMENTS_MESSAGE}\n",
        )
        install.assert_not_called()

    def test_rsync_remote_package_archive_downloads_archive(self) -> None:
        local_archive = Path("/tmp/local-package.tar.zst")

        with mock.patch.object(install_local_standalone, "run") as run:
            install_local_standalone.rsync_remote_package_archive(
                HOST, REMOTE_ARCHIVE, local_archive
            )

        run.assert_called_once_with(
            (
                "rsync",
                "--archive",
                f"{HOST}:{REMOTE_ARCHIVE}",
                str(local_archive),
            )
        )

    def test_extract_package_archive_creates_destination_and_extracts(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            local_archive = root / "package.tar.zst"
            staging_dir = root / "staging"

            with mock.patch.object(install_local_standalone, "run") as run:
                install_local_standalone.extract_package_archive(
                    local_archive, staging_dir
                )

            self.assertTrue(staging_dir.is_dir())
            run.assert_called_once_with(
                (
                    "tar",
                    "-I",
                    "zstd",
                    "-xf",
                    str(local_archive),
                    "-C",
                    str(staging_dir),
                )
            )

    def test_remote_build_command_builds_and_compresses_package(self) -> None:
        config = install_local_standalone.StandaloneInstallConfig(
            target=TARGET,
            variant=VARIANT,
            cargo_profile=CARGO_PROFILE,
        )
        quoted_target = install_local_standalone.shell_quote(TARGET)
        quoted_variant = install_local_standalone.shell_quote(VARIANT)
        quoted_cargo_profile = install_local_standalone.shell_quote(CARGO_PROFILE)
        quoted_package_dir = install_local_standalone.shell_quote(
            str(REMOTE_PACKAGE_DIR)
        )
        quoted_archive = install_local_standalone.shell_quote(str(REMOTE_ARCHIVE))

        with mock.patch.object(
            install_local_standalone,
            "remote_build_env_command",
            return_value=REMOTE_BUILD_ENV,
        ) as build_env:
            command = install_local_standalone.remote_build_command(
                config,
                REMOTE_PACKAGE_DIR,
                REMOTE_ARCHIVE,
            )

        self.assertEqual(
            command,
            (
                "bash",
                "-lc",
                f"rm -rf {quoted_package_dir} {quoted_archive} && "
                f"{REMOTE_BUILD_ENV} && "
                "uv run --project scripts python scripts/build_codex_package.py "
                f"--target {quoted_target} --variant {quoted_variant} "
                f"--cargo-profile {quoted_cargo_profile} "
                f"--package-dir {quoted_package_dir} --force && "
                f"tar -I 'zstd -T0 -3' -cf {quoted_archive} "
                f"-C {quoted_package_dir} . && du -h {quoted_archive}",
            ),
        )
        build_env.assert_called_once_with(TARGET)

    def test_activate_release_backs_up_previous_release(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            staging_dir = root / "staging"
            release_dir = root / "releases" / "current-release"
            staging_dir.mkdir()
            release_dir.mkdir(parents=True)
            staged_entrypoint = staging_dir / "codex"
            previous_entrypoint = release_dir / "codex"
            staged_entrypoint.write_text("new", encoding="utf-8")
            previous_entrypoint.write_text("old", encoding="utf-8")

            with mock.patch.object(
                install_local_standalone.time,
                "strftime",
                return_value="20260710T120000",
            ):
                install_local_standalone.activate_release_with_backup(
                    staging_dir, release_dir, COMMIT
                )

            backup_dir = (
                release_dir.parent
                / "backups"
                / f"{release_dir.name}.20260710T120000.{COMMIT.short_hash}"
            )
            self.assertEqual(
                release_dir.joinpath("codex").read_text(encoding="utf-8"), "new"
            )
            self.assertEqual(
                backup_dir.joinpath("codex").read_text(encoding="utf-8"), "old"
            )
            self.assertFalse(staging_dir.exists())

    def test_verify_entrypoint_mtime_rejects_non_fresh_install(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            entrypoint = Path(temp_dir) / "codex"
            entrypoint.write_text("entrypoint", encoding="utf-8")
            os.utime(entrypoint, (COMMIT.timestamp, COMMIT.timestamp))

            with self.assertRaises(RuntimeError):
                install_local_standalone.verify_entrypoint_mtime(entrypoint, COMMIT)

            fresh_timestamp = COMMIT.timestamp + 1
            os.utime(entrypoint, (fresh_timestamp, fresh_timestamp))
            install_local_standalone.verify_entrypoint_mtime(entrypoint, COMMIT)


if __name__ == "__main__":
    unittest.main()
