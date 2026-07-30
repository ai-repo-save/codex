import fcntl
import os
import subprocess
import sys
import tempfile
import time
import unittest
from pathlib import Path
from unittest import mock


SCRIPT_DIR = Path(__file__).resolve().parent
sys.path.insert(0, str(SCRIPT_DIR))

import cleanup_build_cache  # noqa: E402


class CleanupBuildCacheTest(unittest.TestCase):
    def create_generation(
        self,
        checkout: Path,
        name: str,
        age_days: int,
    ) -> Path:
        generation = (
            checkout / "codex-rs/target" / f"build-{name}" / "incremental" / name
        )
        generation.mkdir(parents=True)
        artifact = generation / "artifact"
        artifact.write_text("artifact", encoding="utf-8")
        modified_at = time.time() - age_days * 24 * 60 * 60
        os.utime(artifact, (modified_at, modified_at))
        return generation

    def run_cleanup(
        self,
        checkout: Path,
        lock_path: Path,
        *,
        execute: bool,
        target_size_kib: int,
        candidate_size_kib: int = 4096,
    ) -> subprocess.CompletedProcess[str]:
        bin_dir = checkout / "bin"
        bin_dir.mkdir()
        (bin_dir / "pgrep").write_text("#!/bin/sh\nexit 1\n", encoding="utf-8")
        (bin_dir / "du").write_text(
            "#!/bin/sh\n"
            'if [ "$3" = "$CLEANUP_TEST_TARGET_PATH" ]; then\n'
            '  printf "%s\\t%s\\n" "$CLEANUP_TEST_TARGET_SIZE_KIB" "$3"\n'
            "else\n"
            '  printf "%s\\t%s\\n" "$CLEANUP_TEST_CANDIDATE_SIZE_KIB" "$3"\n'
            "fi\n",
            encoding="utf-8",
        )
        for command in ("pgrep", "du"):
            (bin_dir / command).chmod(0o755)

        environment = os.environ | {
            "CLEANUP_TEST_TARGET_PATH": "codex-rs/target",
            "CLEANUP_TEST_TARGET_SIZE_KIB": str(target_size_kib),
            "CLEANUP_TEST_CANDIDATE_SIZE_KIB": str(candidate_size_kib),
            "PATH": f"{bin_dir}:{os.environ['PATH']}",
        }
        config = cleanup_build_cache.CleanupConfig(
            host="unused",
            remote_path=str(checkout),
            max_age_days=14,
            max_size_gib=1,
            execute=execute,
        )
        with mock.patch.object(
            cleanup_build_cache, "REMOTE_TARGET_LOCK_PATH", str(lock_path)
        ):
            return subprocess.run(
                ("/bin/bash", "-c", cleanup_build_cache.cleanup_command(config)),
                check=False,
                capture_output=True,
                cwd=checkout,
                env=environment,
                text=True,
            )

    def test_dry_run_preserves_old_generations(self) -> None:
        with tempfile.TemporaryDirectory() as temporary_directory:
            checkout = Path(temporary_directory)
            first_generation = self.create_generation(checkout, "first", age_days=30)
            second_generation = self.create_generation(checkout, "second", age_days=30)

            result = self.run_cleanup(
                checkout,
                checkout / "target.lock",
                execute=False,
                target_size_kib=2 * 1024 * 1024,
            )

            self.assertTrue(first_generation.exists())
            self.assertTrue(second_generation.exists())

        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("dry run: 2 stale incremental generation(s)", result.stdout)

    def test_execute_preserves_generations_younger_than_age_limit(self) -> None:
        with tempfile.TemporaryDirectory() as temporary_directory:
            checkout = Path(temporary_directory)
            generation = self.create_generation(checkout, "recent", age_days=1)

            result = self.run_cleanup(
                checkout,
                checkout / "target.lock",
                execute=True,
                target_size_kib=2 * 1024 * 1024,
            )

            self.assertTrue(generation.exists())

        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("no stale incremental generation", result.stdout)

    def test_execute_preserves_target_at_capacity_threshold(self) -> None:
        with tempfile.TemporaryDirectory() as temporary_directory:
            checkout = Path(temporary_directory)
            generation = self.create_generation(checkout, "old", age_days=30)

            result = self.run_cleanup(
                checkout,
                checkout / "target.lock",
                execute=True,
                target_size_kib=1024 * 1024,
            )

            self.assertTrue(generation.exists())

        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("at or below the configured capacity threshold", result.stdout)

    def test_execute_refuses_when_target_lock_is_held(self) -> None:
        with tempfile.TemporaryDirectory() as temporary_directory:
            checkout = Path(temporary_directory)
            self.create_generation(checkout, "old", age_days=30)
            lock_path = checkout / "target.lock"
            with lock_path.open("w", encoding="utf-8") as lock_file:
                fcntl.flock(lock_file.fileno(), fcntl.LOCK_EX | fcntl.LOCK_NB)
                result = self.run_cleanup(
                    checkout,
                    lock_path,
                    execute=True,
                    target_size_kib=2 * 1024 * 1024,
                )

        self.assertEqual(result.returncode, 1)
        self.assertIn("remote build holds the target cache lock", result.stderr)

    def test_execute_removes_old_generation_and_stops_at_capacity_threshold(
        self,
    ) -> None:
        with tempfile.TemporaryDirectory() as temporary_directory:
            checkout = Path(temporary_directory)
            first_generation = self.create_generation(checkout, "first", age_days=30)
            second_generation = self.create_generation(checkout, "second", age_days=30)

            result = self.run_cleanup(
                checkout,
                checkout / "target.lock",
                execute=True,
                target_size_kib=1024 * 1024 + 4096,
            )

            remaining_generations = [
                generation
                for generation in (first_generation, second_generation)
                if generation.exists()
            ]
            self.assertEqual(len(remaining_generations), 1)

        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("removed 1 stale incremental generation(s)", result.stdout)

    def test_help_exits_before_checking_ssh_availability(self) -> None:
        with mock.patch.object(
            cleanup_build_cache,
            "require_command",
            side_effect=AssertionError("ssh availability should not be checked"),
        ):
            with self.assertRaises(SystemExit) as error:
                cleanup_build_cache.main(("--help",))

        self.assertEqual(error.exception.code, 0)


if __name__ == "__main__":
    unittest.main()
