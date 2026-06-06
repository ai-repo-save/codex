#!/usr/bin/env python3
from __future__ import annotations

import contextlib
import io
import sys
import unittest
from pathlib import Path


SCRIPT_DIR = Path(__file__).resolve().parent
sys.path.insert(0, str(SCRIPT_DIR))

import tui_smoke  # noqa: E402


class RemoteTuiSmokeTest(unittest.TestCase):
    def test_rejects_arguments(self) -> None:
        stderr = io.StringIO()

        with contextlib.redirect_stderr(stderr):
            exit_code = tui_smoke.main(("test",))

        self.assertEqual(exit_code, 2)
        self.assertIn("does not accept arguments", stderr.getvalue())

    def test_runs_fixed_codex_tui_smoke_command(self) -> None:
        captured_commands: list[tuple[str, ...]] = []
        original_run_remote_workflow = tui_smoke.run_remote_workflow

        def capture_run_remote_workflow(config: tui_smoke.RemoteWorkflow) -> int:
            captured_commands.append(config.command)
            return 0

        tui_smoke.run_remote_workflow = capture_run_remote_workflow
        try:
            exit_code = tui_smoke.main(())
        finally:
            tui_smoke.run_remote_workflow = original_run_remote_workflow

        self.assertEqual(exit_code, 0)
        self.assertEqual(len(captured_commands), 1)
        shell_command = captured_commands[0][2]
        self.assertIn(
            "cd codex-rs && just test -p codex-tui "
            "embedded_app_server_supports_thread_start_rpc",
            shell_command,
        )


if __name__ == "__main__":
    unittest.main()
