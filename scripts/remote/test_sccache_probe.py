#!/usr/bin/env python3

import sys
import unittest
from pathlib import Path
from unittest import mock


SCRIPT_DIR = Path(__file__).resolve().parent
sys.path.insert(0, str(SCRIPT_DIR))

import sccache_probe  # noqa: E402


class SccacheProbeTest(unittest.TestCase):
    def test_runs_two_isolated_library_builds_and_cleans_them(self) -> None:
        probe_command = sccache_probe.probe_command("codex-utils-fuzzy-match")
        self.assertEqual(
            probe_command.count('CARGO_TARGET_DIR="$probe_root/target"'), 2
        )
        self.assertIn('rm -rf -- "$probe_root/target"', probe_command)
        self.assertEqual(probe_command.count("cargo build --locked"), 2)
        self.assertIn(
            "cargo build --locked -p 'codex-utils-fuzzy-match' --lib", probe_command
        )
        self.assertIn("trap 'rm -rf --", probe_command)

        with mock.patch.object(
            sccache_probe, "run_remote_workflow", return_value=0
        ) as run:
            exit_code = sccache_probe.main(("--package", "codex-utils-fuzzy-match"))

        self.assertEqual(exit_code, 0)
        workflow = run.call_args.args[0]
        self.assertEqual(workflow.branch, sccache_probe.DEFAULT_BRANCH)
        shell_command = workflow.command[2]
        self.assertIn("sccache --zero-stats", shell_command)
        self.assertIn("remote build: sccache stats after", shell_command)


if __name__ == "__main__":
    unittest.main()
