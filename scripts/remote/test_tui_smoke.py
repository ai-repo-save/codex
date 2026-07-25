#!/usr/bin/env python3
from __future__ import annotations

import contextlib
import io
import sys
import unittest
from unittest import mock
from pathlib import Path


SCRIPT_DIR = Path(__file__).resolve().parent
sys.path.insert(0, str(SCRIPT_DIR))

import tui_smoke  # noqa: E402


class RemoteTuiSmokeTest(unittest.TestCase):
    def test_rejects_arguments(self) -> None:
        stderr = io.StringIO()

        with (
            contextlib.redirect_stderr(stderr),
            self.assertRaises(SystemExit) as raised,
        ):
            tui_smoke.main(("test",))

        self.assertEqual(raised.exception.code, 2)
        self.assertIn("unrecognized arguments", stderr.getvalue())

    def test_runs_fixed_codex_tui_smoke_command(self) -> None:
        with mock.patch.object(tui_smoke, "run_remote_workflow", return_value=0) as run:
            exit_code = tui_smoke.main(())

        self.assertEqual(exit_code, 0)
        config = run.call_args.args[0]
        self.assertEqual(config.branch, tui_smoke.DEFAULT_BRANCH)
        shell_command = config.command[2]
        self.assertIn(
            "cd codex-rs && just test -p codex-tui "
            "embedded_app_server_supports_thread_start_rpc",
            shell_command,
        )

    def test_runs_smoke_command_on_selected_sync_branch(self) -> None:
        with mock.patch.object(tui_smoke, "run_remote_workflow", return_value=0) as run:
            exit_code = tui_smoke.main(("--branch", "sync/rust-v0.146.0"))

        self.assertEqual(exit_code, 0)
        self.assertEqual(run.call_args.args[0].branch, "sync/rust-v0.146.0")


if __name__ == "__main__":
    unittest.main()
