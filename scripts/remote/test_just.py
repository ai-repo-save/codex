#!/usr/bin/env python3

import contextlib
import io
import sys
import unittest
from unittest import mock
from pathlib import Path


SCRIPT_DIR = Path(__file__).resolve().parent
sys.path.insert(0, str(SCRIPT_DIR))

import just  # noqa: E402


class RemoteJustTest(unittest.TestCase):
    def test_forwards_unfiltered_codex_tui_test_on_default_branch(self) -> None:
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

    def test_forwards_recipe_to_selected_sync_branch(self) -> None:
        recipe_args = ("test", "-p", "codex-core", "context_anchor")
        with mock.patch.object(just, "run_remote_workflow", return_value=0) as run:
            exit_code = just.main(("--branch", "sync/rust-v0.146.0", *recipe_args))

        self.assertEqual(exit_code, 0)
        run.assert_called_once_with(
            just.RemoteWorkflow(
                host=just.DEFAULT_HOST,
                branch="sync/rust-v0.146.0",
                remote_path=just.DEFAULT_REMOTE_PATH,
                command=just.remote_codex_rs_just_command(recipe_args),
            )
        )

    def test_recipe_arguments_named_branch_are_forwarded(self) -> None:
        recipe_args = ("example-recipe", "--branch", "recipe-branch")
        with mock.patch.object(just, "run_remote_workflow", return_value=0) as run:
            exit_code = just.main(recipe_args)

        self.assertEqual(exit_code, 0)
        run.assert_called_once_with(
            just.RemoteWorkflow(
                host=just.DEFAULT_HOST,
                branch=just.DEFAULT_BRANCH,
                remote_path=just.DEFAULT_REMOTE_PATH,
                command=just.remote_codex_rs_just_command(recipe_args),
            )
        )

    def test_remote_full_uses_bounded_isolated_policy(self) -> None:
        stderr = io.StringIO()
        with (
            contextlib.redirect_stderr(stderr),
            mock.patch.object(just, "run_remote_workflow", return_value=0) as run,
        ):
            exit_code = just.main(("--remote-full", "test"))

        self.assertEqual(exit_code, 0)
        workflow = run.call_args.args[0]
        self.assertEqual(workflow.host, just.DEFAULT_HOST)
        self.assertEqual(workflow.branch, just.DEFAULT_BRANCH)
        self.assertEqual(workflow.remote_path, just.DEFAULT_REMOTE_PATH)
        shell_command = workflow.command[2]
        self.assertTrue(shell_command.startswith("set -euo pipefail; "))
        self.assertIn("/var/tmp/codex-remote-tests.XXXXXX", shell_command)
        self.assertIn('export TMPDIR="$remote_test_tmpdir"', shell_command)
        self.assertIn("just test --test-threads=4 -E", shell_command)
        self.assertIn(
            "package(codex-apply-patch)",
            shell_command,
        )
        self.assertIn(
            "test(test_apply_patch_fails_on_write_error)",
            shell_command,
        )
        self.assertIn(
            "test(host_blocked_requires_allowlist_match)",
            shell_command,
        )
        self.assertIn(
            "test(policy_resolution_retries_after_auth_refresh)",
            shell_command,
        )
        output = stderr.getvalue()
        self.assertIn("isolated TMPDIR", output)
        self.assertIn(
            "codex-apply-patch::test_apply_patch_fails_on_write_error",
            output,
        )
        self.assertIn(
            "codex-network-proxy::host_blocked_requires_allowlist_match",
            output,
        )
        self.assertIn(
            "codex-git-attribution::policy_resolution_retries_after_auth_refresh",
            output,
        )

    def test_remote_full_rejects_additional_recipe_arguments(self) -> None:
        with (
            contextlib.redirect_stderr(io.StringIO()),
            self.assertRaises(SystemExit) as error,
        ):
            just.main(("--remote-full", "test", "-p", "codex-core"))

        self.assertEqual(error.exception.code, 2)


if __name__ == "__main__":
    unittest.main()
