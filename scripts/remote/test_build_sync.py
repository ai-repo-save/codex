#!/usr/bin/env python3

import contextlib
import io
from pathlib import Path
import sys
import unittest
from unittest import mock


SCRIPT_DIR = Path(__file__).resolve().parent
sys.path.insert(0, str(SCRIPT_DIR))

import build_sync  # noqa: E402


class RemoteBuildSyncTest(unittest.TestCase):
    def test_runs_fixed_smoke_command_on_default_branch(self) -> None:
        with mock.patch.object(
            build_sync, "run_remote_workflow", return_value=0
        ) as run:
            exit_code = build_sync.main(())

        self.assertEqual(exit_code, 0)
        run.assert_called_once_with(
            build_sync.RemoteWorkflow(
                host=build_sync.DEFAULT_HOST,
                branch=build_sync.DEFAULT_BRANCH,
                remote_path=build_sync.DEFAULT_REMOTE_PATH,
                command=build_sync.remote_codex_rs_just_command(
                    build_sync.SMOKE_JUST_ARGS
                ),
            )
        )

    def test_runs_fixed_smoke_command_on_selected_sync_branch(self) -> None:
        with mock.patch.object(
            build_sync, "run_remote_workflow", return_value=0
        ) as run:
            exit_code = build_sync.main(("--branch", "sync/rust-v0.146.0"))

        self.assertEqual(exit_code, 0)
        self.assertEqual(run.call_args.args[0].branch, "sync/rust-v0.146.0")

    def test_rejects_positional_arguments(self) -> None:
        stderr = io.StringIO()

        with (
            contextlib.redirect_stderr(stderr),
            self.assertRaises(SystemExit) as raised,
        ):
            build_sync.main(("unexpected",))

        self.assertEqual(raised.exception.code, 2)
        self.assertIn("unrecognized arguments", stderr.getvalue())


if __name__ == "__main__":
    unittest.main()
