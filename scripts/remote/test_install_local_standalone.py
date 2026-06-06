#!/usr/bin/env python3
from __future__ import annotations

from pathlib import Path
from pathlib import PurePosixPath
import sys
import tempfile
import unittest

SCRIPT_DIR = Path(__file__).resolve().parent
sys.path.insert(0, str(SCRIPT_DIR))
sys.path.insert(0, str(SCRIPT_DIR.parents[0]))

import install_local_standalone  # noqa: E402


class RemoteInstallLocalStandaloneTest(unittest.TestCase):
    def test_rsync_remote_package_uses_compressed_delete_sync(self) -> None:
        commands: list[tuple[str, ...]] = []

        def capture_run(command: tuple[str, ...]) -> None:
            commands.append(command)

        original_run = install_local_standalone.run
        install_local_standalone.run = capture_run
        try:
            install_local_standalone.rsync_remote_package(
                "builder", PurePosixPath("/tmp/codex-package"), Path("/tmp/staging")
            )
        finally:
            install_local_standalone.run = original_run

        self.assertEqual(
            commands,
            [
                (
                    "rsync",
                    "--archive",
                    "--delete",
                    "--compress",
                    "--compress-choice=zstd",
                    "--compress-level=1",
                    "builder:/tmp/codex-package/",
                    "/tmp/staging/",
                )
            ],
        )

    def test_prefill_staging_dir_hardlinks_current_release_files(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            release_dir = root / "release"
            release_dir.mkdir()
            source_file = release_dir / "bin" / "codex"
            source_file.parent.mkdir()
            source_file.write_text("entrypoint", encoding="utf-8")
            current_link = root / "current"
            current_link.symlink_to(release_dir)
            staging_dir = root / "staging"

            install_local_standalone.prefill_staging_dir(current_link, staging_dir)

            staged_file = staging_dir / "bin" / "codex"
            self.assertEqual(staged_file.read_text(encoding="utf-8"), "entrypoint")
            self.assertEqual(staged_file.stat().st_ino, source_file.stat().st_ino)

    def test_remote_build_command_configures_cache_and_fast_linker(self) -> None:
        command = install_local_standalone.remote_build_command(
            install_local_standalone.StandaloneInstallConfig(),
            PurePosixPath("/tmp/codex-package"),
        )

        self.assertEqual(command[0:2], ("bash", "-lc"))
        shell_command = command[2]
        self.assertIn("export RUSTC_WRAPPER=", shell_command)
        self.assertIn(
            "export CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER=clang",
            shell_command,
        )
        self.assertIn("-C link-arg=-fuse-ld=$(command -v mold)", shell_command)
        self.assertIn("-C link-arg=-fuse-ld=lld", shell_command)


if __name__ == "__main__":
    unittest.main()
