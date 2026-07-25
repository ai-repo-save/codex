#!/usr/bin/env python3

import contextlib
import io
from pathlib import Path
import sys
import unittest
from unittest import mock


SCRIPT_DIR = Path(__file__).resolve().parent
sys.path.insert(0, str(SCRIPT_DIR))

import doctor  # noqa: E402


class RemoteDoctorTest(unittest.TestCase):
    def test_checks_default_branch(self) -> None:
        with mock.patch.object(doctor, "run") as run:
            exit_code = doctor.main(())

        self.assertEqual(exit_code, 0)
        command = run.call_args.args[0]
        self.assertEqual(command[0:2], ("ssh", doctor.DEFAULT_HOST))
        self.assertIn(
            f"git ls-remote --exit-code --heads origin '{doctor.DEFAULT_BRANCH}'",
            command[2],
        )

    def test_checks_selected_sync_branch(self) -> None:
        with mock.patch.object(doctor, "run") as run:
            exit_code = doctor.main(("--branch", "sync/rust-v0.146.0"))

        self.assertEqual(exit_code, 0)
        self.assertIn(
            "git ls-remote --exit-code --heads origin 'sync/rust-v0.146.0'",
            run.call_args.args[0][2],
        )

    def test_rejects_positional_arguments(self) -> None:
        stderr = io.StringIO()

        with (
            contextlib.redirect_stderr(stderr),
            self.assertRaises(SystemExit) as raised,
        ):
            doctor.main(("unexpected",))

        self.assertEqual(raised.exception.code, 2)
        self.assertIn("unrecognized arguments", stderr.getvalue())


if __name__ == "__main__":
    unittest.main()
