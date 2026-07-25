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
        with mock.patch.object(just, "run_remote_workflow", return_value=0) as run:
            exit_code = just.main(("test", "-p", "codex-tui"))

        self.assertEqual(exit_code, 0)
        workflow = run.call_args.args[0]
        self.assertIn("just test -p codex-tui", workflow.command[-1])


if __name__ == "__main__":
    unittest.main()
