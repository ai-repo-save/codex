#!/usr/bin/env python3
from __future__ import annotations

import sys
import unittest
from pathlib import Path


SCRIPT_DIR = Path(__file__).resolve().parent
sys.path.insert(0, str(SCRIPT_DIR))

import _sync  # noqa: E402


class RemoteSyncTest(unittest.TestCase):
    def test_remote_codex_rs_just_command_uses_cache_and_fast_linker_env(self) -> None:
        command = _sync.remote_codex_rs_just_command(("test", "-p", "codex-app-server"))

        self.assertEqual(command[0:2], ("bash", "-lc"))
        shell_command = command[2]
        self.assertIn("export RUSTC_WRAPPER=", shell_command)
        self.assertIn(
            "export CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER=clang",
            shell_command,
        )
        self.assertIn("-C link-arg=-fuse-ld=$(command -v mold)", shell_command)
        self.assertIn("cd codex-rs && just test -p codex-app-server", shell_command)

    def test_ssh_command_builds_plain_remote_command(self) -> None:
        config = _sync.RemoteWorkflow(
            host="builder",
            branch="main",
            remote_path="/root/codex",
            command=(),
        )

        self.assertEqual(
            _sync.ssh_command(config, "git status"),
            ("ssh", "builder", "git status"),
        )


if __name__ == "__main__":
    unittest.main()
