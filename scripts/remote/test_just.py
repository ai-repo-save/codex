#!/usr/bin/env python3

import sys
import unittest
from unittest import mock
from pathlib import Path


SCRIPT_DIR = Path(__file__).resolve().parent
sys.path.insert(0, str(SCRIPT_DIR))

import just  # noqa: E402


class RemoteJustTest(unittest.TestCase):
    def test_forwards_unfiltered_codex_tui_test(self) -> None:
        args = ("test", "-p", "codex-tui")
        with mock.patch.object(just, "run_remote_workflow", return_value=0) as run:
            exit_code = just.main(args)

        self.assertEqual(exit_code, 0)
        run.assert_called_once_with(
            just.RemoteWorkflow(
                host=just.DEFAULT_HOST,
                branch=just.DEFAULT_BRANCH,
                remote_path=just.DEFAULT_REMOTE_PATH,
                command=just.remote_codex_rs_just_command(args),
            )
        )


if __name__ == "__main__":
    unittest.main()
