#!/usr/bin/env python3
from __future__ import annotations

import importlib.util
import unittest
from pathlib import Path


SCRIPT_PATH = Path(__file__).with_name("block-local-build-commands.py")
SPEC = importlib.util.spec_from_file_location("block_local_build_commands", SCRIPT_PATH)
assert SPEC is not None
assert SPEC.loader is not None
block_local_build_commands = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(block_local_build_commands)


class BlockLocalBuildCommandsTest(unittest.TestCase):
    def test_allows_read_only_search_containing_blocked_words(self) -> None:
        self.assertFalse(
            block_local_build_commands.is_blocked_command(
                "rg -n 'just test|cargo build' AGENTS.md"
            )
        )

    def test_blocks_local_build_command(self) -> None:
        self.assertTrue(block_local_build_commands.is_blocked_command("just test"))
        self.assertEqual(
            block_local_build_commands.blocked_command_reason("just test"),
            block_local_build_commands.BLOCK_REASON,
        )

    def test_blocks_ad_hoc_remote_just_command(self) -> None:
        self.assertTrue(
            block_local_build_commands.is_blocked_command(
                "ssh 192.168.50.8 'cd /root/codex/codex-rs && just test'"
            )
        )
        reason = block_local_build_commands.blocked_command_reason(
            "ssh 192.168.50.8 'cd /root/codex/codex-rs && just test'"
        )
        self.assertEqual(reason, block_local_build_commands.REMOTE_BLOCK_REASON)
        self.assertIn("scripts/remote/just.py", reason)

    def test_allows_remote_diagnostic_command(self) -> None:
        self.assertFalse(
            block_local_build_commands.is_blocked_command(
                "ssh 192.168.50.8 'cd /root/codex && git status --short'"
            )
        )


if __name__ == "__main__":
    unittest.main()
