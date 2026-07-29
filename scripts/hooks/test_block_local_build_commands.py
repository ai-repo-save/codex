#!/usr/bin/env python3

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

    def test_blocks_branch_switch_commands(self) -> None:
        commands = (
            "git switch main",
            "git switch --detach rust-v0.146.0",
            "git checkout rust-v0.146.0",
            "git -C /home/bluebird/git/codex checkout -b scratch",
            "bash -lc 'git switch sync/rust-v0.146.0'",
        )

        for command in commands:
            with self.subTest(command=command):
                self.assertEqual(
                    block_local_build_commands.blocked_command_reason(command),
                    block_local_build_commands.BRANCH_SWITCH_BLOCK_REASON,
                )

    def test_allows_git_commands_that_do_not_switch_branches(self) -> None:
        commands = (
            "git status --short",
            "git restore -- scripts/uv.lock",
            "git checkout -- scripts/uv.lock",
            "git checkout HEAD -- scripts/uv.lock",
            "rg -n 'git switch|git checkout' scripts/hooks",
        )

        for command in commands:
            with self.subTest(command=command):
                self.assertFalse(block_local_build_commands.is_blocked_command(command))


if __name__ == "__main__":
    unittest.main()
