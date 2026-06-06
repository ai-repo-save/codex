#!/usr/bin/env python3
from __future__ import annotations

import contextlib
import io
import sys
import unittest
from pathlib import Path


SCRIPT_DIR = Path(__file__).resolve().parent
sys.path.insert(0, str(SCRIPT_DIR))

import just  # noqa: E402


class RemoteJustTest(unittest.TestCase):
    def test_rejects_unfiltered_codex_tui_test(self) -> None:
        stderr = io.StringIO()

        with contextlib.redirect_stderr(stderr):
            exit_code = just.main(("test", "-p", "codex-tui"))

        self.assertEqual(exit_code, 2)
        self.assertIn("scripts/remote/tui_smoke.py", stderr.getvalue())


if __name__ == "__main__":
    unittest.main()
