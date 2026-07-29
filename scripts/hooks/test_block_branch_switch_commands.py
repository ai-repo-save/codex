#!/usr/bin/env python3

import importlib.util
import unittest
from pathlib import Path


SCRIPT_PATH = Path(__file__).with_name("block-branch-switch-commands.py")
SPEC = importlib.util.spec_from_file_location("block_branch_switch_commands", SCRIPT_PATH)
assert SPEC is not None
assert SPEC.loader is not None
block_branch_switch_commands = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(block_branch_switch_commands)


class BlockBranchSwitchCommandsTest(unittest.TestCase):
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
                    block_branch_switch_commands.blocked_command_reason(command),
                    block_branch_switch_commands.BRANCH_SWITCH_BLOCK_REASON,
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
                self.assertFalse(
                    block_branch_switch_commands.blocked_command_reason(command)
                    is not None
                )


if __name__ == "__main__":
    unittest.main()
