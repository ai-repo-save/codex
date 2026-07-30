#!/usr/bin/env python3

import sys
import unittest
from pathlib import Path
from unittest import mock


SCRIPT_DIR = Path(__file__).resolve().parent
sys.path.insert(0, str(SCRIPT_DIR))

import cleanup_build_cache  # noqa: E402


class CleanupBuildCacheTest(unittest.TestCase):
    def test_dry_run_requires_capacity_and_generation_age_thresholds(self) -> None:
        command = cleanup_build_cache.cleanup_command(
            cleanup_build_cache.parse_args(("--dry-run",))
        )

        self.assertIn("mode='dry-run'", command)
        self.assertIn("active Cargo or rustc process", command)
        self.assertIn("remote build holds the target cache lock", command)
        self.assertIn("Cargo lock detected", command)
        self.assertIn(
            "target cache is below the configured capacity threshold", command
        )
        self.assertIn(
            "no stale incremental generation exceeds the configured age threshold",
            command,
        )
        self.assertIn("-name incremental", command)
        self.assertIn("-maxdepth 1 -type d", command)
        self.assertNotIn('rm -rf -- "$target_path"', command)
        self.assertNotIn('rm -rf -- "$candidate_path"', command)

    def test_execute_removes_only_eligible_incremental_generations(self) -> None:
        command = cleanup_build_cache.cleanup_command(
            cleanup_build_cache.parse_args(
                ("--execute", "--max-age-days", "21", "--max-size-gib", "100")
            )
        )

        self.assertIn("max_age_days=21", command)
        self.assertIn("max_size_kib=104857600", command)
        self.assertIn('rm -rf -- "$candidate_path"', command)
        self.assertIn("removed stale incremental generation", command)
        self.assertIn('if [ "$target_size_kib" -lt "$max_size_kib" ]', command)

    def test_main_runs_the_dry_run_over_ssh_without_git_sync(self) -> None:
        with (
            mock.patch.object(cleanup_build_cache, "require_command"),
            mock.patch.object(cleanup_build_cache, "run") as run,
        ):
            exit_code = cleanup_build_cache.main(("--dry-run",))

        self.assertEqual(exit_code, 0)
        command = run.call_args.args[0]
        self.assertEqual(command[0:2], ("ssh", cleanup_build_cache.DEFAULT_HOST))
        self.assertIn("mode='dry-run'", command[2])
        self.assertNotIn("git fetch", command[2])

    def test_invalid_retention_limit_is_rejected(self) -> None:
        with self.assertRaises(SystemExit) as error:
            cleanup_build_cache.parse_args(("--max-age-days", "0"))

        self.assertEqual(error.exception.code, 2)


if __name__ == "__main__":
    unittest.main()
