#!/usr/bin/env python3
from __future__ import annotations

import sys
import unittest
from pathlib import Path


SCRIPT_DIR = Path(__file__).resolve().parent
sys.path.insert(0, str(SCRIPT_DIR))

import _sync  # noqa: E402


class RemoteSyncTest(unittest.TestCase):
    def test_remote_codex_rs_test_command_uses_nextest_without_bench_smoke(
        self,
    ) -> None:
        command = _sync.remote_codex_rs_just_command(("test", "-p", "codex-app-server"))

        self.assertEqual(command[0:2], ("bash", "-lc"))
        shell_command = command[2]
        self.assertIn("export RUSTC_WRAPPER=", shell_command)
        self.assertIn(
            "export CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER=clang",
            shell_command,
        )
        self.assertIn("-C link-arg=-fuse-ld=$(command -v mold)", shell_command)
        self.assertIn(
            "cd codex-rs && RUST_MIN_STACK=8388608 cargo nextest run --no-fail-fast -p codex-app-server",
            shell_command,
        )
        self.assertNotIn("just test", shell_command)
        self.assertNotIn("bench-smoke", shell_command)

    def test_remote_codex_rs_non_test_command_uses_just_recipe(self) -> None:
        command = _sync.remote_codex_rs_just_command(("fmt",))

        self.assertEqual(command[0:2], ("bash", "-lc"))
        self.assertIn("cd codex-rs && just fmt", command[2])

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

    def test_remote_checkout_sync_command_retries_fetch_before_reset(self) -> None:
        config = _sync.RemoteWorkflow(
            host="builder",
            branch="main",
            remote_path="/root/codex",
            command=(),
        )

        command = _sync.remote_checkout_sync_command(config)

        self.assertIn("for attempt in $(seq 1 3)", command)
        self.assertIn("if git fetch origin; then break; fi", command)
        self.assertIn("remote sync: git fetch failed; retrying", command)
        self.assertIn("git reset --hard origin/'main'", command)
        self.assertIn("git clean -fd", command)


if __name__ == "__main__":
    unittest.main()
