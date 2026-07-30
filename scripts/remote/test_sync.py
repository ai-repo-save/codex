#!/usr/bin/env python3
import os
import signal
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch


SCRIPT_DIR = Path(__file__).resolve().parent
sys.path.insert(0, str(SCRIPT_DIR))

import _sync  # noqa: E402


class RemoteSyncTest(unittest.TestCase):
    def test_failed_remote_command_copies_git_visible_changes_before_raising(
        self,
    ) -> None:
        config = _sync.RemoteWorkflow(
            host="builder",
            branch="main",
            remote_path="/root/codex",
            command=("false",),
        )
        command_error = subprocess.CalledProcessError(1, config.command)
        status_plan = _sync.StatusPlan(
            copy_paths=("snapshot.snap.new",),
            delete_paths=(),
        )

        with (
            patch.object(_sync, "git_repo_root", return_value=Path("/repo")),
            patch.object(_sync, "require_command"),
            patch.object(_sync, "ensure_clean_local_worktree"),
            patch.object(_sync, "ensure_current_branch"),
            patch.object(_sync, "git_output", return_value="abc123"),
            patch.object(_sync, "run"),
            patch.object(_sync, "sync_remote_checkout"),
            patch.object(_sync, "run_remote_command", side_effect=command_error),
            patch.object(_sync, "ensure_local_head"),
            patch.object(_sync, "remote_status_plan", return_value=status_plan),
            patch.object(_sync, "apply_remote_changes") as apply_remote_changes,
            self.assertRaises(subprocess.CalledProcessError),
        ):
            _sync.run_remote_workflow(config)

        apply_remote_changes.assert_called_once_with(Path("/repo"), config, status_plan)

    def test_remote_codex_rs_test_command_uses_canonical_just_recipe(self) -> None:
        command = _sync.remote_codex_rs_just_command(("test", "-p", "codex-app-server"))

        self.assertEqual(command[0:2], ("bash", "-lc"))
        shell_command = command[2]
        self.assertIn("export CARGO_INCREMENTAL=0", shell_command)
        self.assertIn("remote build: sccache stats before", shell_command)
        self.assertIn("remote build: sccache stats after", shell_command)
        self.assertIn("compiler-cache metrics unavailable", shell_command)
        self.assertIn("flock --shared --close", shell_command)
        self.assertIn("export RUSTC_WRAPPER=", shell_command)
        self.assertIn(
            "export CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER=clang",
            shell_command,
        )
        self.assertIn("-C link-arg=-fuse-ld=$(command -v mold)", shell_command)
        self.assertIn(
            "cd codex-rs && just test -p codex-app-server",
            shell_command,
        )

    def test_remote_codex_rs_non_test_command_uses_just_recipe(self) -> None:
        command = _sync.remote_codex_rs_just_command(("fmt",))

        self.assertEqual(command[0:2], ("bash", "-lc"))
        self.assertIn("cd codex-rs && just fmt", command[2])

    def test_remote_build_shell_command_stops_before_followup_after_failure(
        self,
    ) -> None:
        with tempfile.TemporaryDirectory(dir="/tmp") as temp_dir:
            root = Path(temp_dir)
            sccache = root / "sccache"
            daemon_pids = root / "daemon-pids"
            lock_path = root / "target.lock"
            sccache.write_text(
                "#!/bin/sh\n"
                "printf 'fake sccache stats\\n'\n"
                'sleep 30 >/dev/null 2>&1 & echo "$!" >> "$SCCACHE_DAEMON_PIDS"\n'
            )
            sccache.chmod(0o755)
            marker = root / "unexpected-followup"
            environment = os.environ | {
                "PATH": f"{root}{os.pathsep}{os.environ['PATH']}",
                "SCCACHE_DAEMON_PIDS": str(daemon_pids),
            }
            try:
                with patch.object(_sync, "REMOTE_TARGET_LOCK_PATH", str(lock_path)):
                    command = _sync.remote_build_shell_command(
                        _sync.DEFAULT_TARGET,
                        f"false; touch {_sync.shell_quote(str(marker))}",
                    )
                    result = subprocess.run(
                        ("bash", "-lc", command),
                        env=environment,
                        stdout=subprocess.PIPE,
                        stderr=subprocess.PIPE,
                        text=True,
                    )
                lock_result = subprocess.run(
                    ("flock", "--exclusive", "--nonblock", str(lock_path), "true"),
                    stdout=subprocess.PIPE,
                    stderr=subprocess.PIPE,
                    text=True,
                )
            finally:
                if daemon_pids.exists():
                    for pid in daemon_pids.read_text().splitlines():
                        os.kill(int(pid), signal.SIGTERM)

            self.assertFalse(marker.exists())

        self.assertEqual(result.returncode, 1)
        self.assertEqual(result.stdout.count("remote build: sccache stats"), 2)
        self.assertEqual(lock_result.returncode, 0)

    def test_remote_build_shell_command_supports_an_exclusive_lock(self) -> None:
        command = _sync.remote_build_shell_command(
            _sync.DEFAULT_TARGET,
            "true",
            lock_mode=_sync.RemoteTargetLockMode.EXCLUSIVE,
        )

        self.assertIn("flock --exclusive --close", command)
        self.assertIn("acquired exclusive target cache lock", command)

    def test_ssh_command_builds_plain_remote_command(self) -> None:
        config = _sync.RemoteWorkflow(
            host="builder",
            branch="main",
            remote_path="/root/codex",
            command=(),
        )

        self.assertEqual(
            _sync.ssh_command(config, "git status"),
            ("ssh", "builder", "git status"),
        )

    def test_remote_checkout_sync_command_retries_fetch_before_reset(self) -> None:
        config = _sync.RemoteWorkflow(
            host="builder",
            branch="main",
            remote_path="/root/codex",
            command=(),
        )

        command = _sync.remote_checkout_sync_command(config, "abc123")

        self.assertIn("for attempt in $(seq 1 3)", command)
        self.assertIn("if git fetch origin; then fetched=true; break; fi", command)
        self.assertIn("remote sync: git fetch failed; retrying", command)
        self.assertIn("git reset --hard origin/'main'", command)
        self.assertIn("current_branch=$(git branch --show-current)", command)
        self.assertIn("[ \"$current_head\" != 'abc123' ]", command)
        self.assertIn("reusing matching clean checkout", command)
        self.assertIn("git clean -fd", command)

    def test_local_bundle_advertises_the_requested_branch_head(self) -> None:
        config = _sync.RemoteWorkflow(
            host="builder",
            branch="main",
            remote_path="/root/codex",
            command=(),
        )
        with tempfile.TemporaryDirectory(dir="/tmp") as temp_dir:
            repo = Path(temp_dir) / "repo"
            repo.mkdir()
            subprocess.run(("git", "init", "-b", "main"), cwd=repo, check=True)
            subprocess.run(
                ("git", "config", "user.email", "codex@example.invalid"),
                cwd=repo,
                check=True,
            )
            subprocess.run(
                ("git", "config", "user.name", "Codex Test"),
                cwd=repo,
                check=True,
            )
            (repo / "tracked.txt").write_text("content\n", encoding="utf-8")
            subprocess.run(("git", "add", "tracked.txt"), cwd=repo, check=True)
            subprocess.run(("git", "commit", "-m", "fixture"), cwd=repo, check=True)
            expected_head = subprocess.run(
                ("git", "rev-parse", "HEAD"),
                cwd=repo,
                check=True,
                stdout=subprocess.PIPE,
                text=True,
            ).stdout.strip()
            bundle = Path(temp_dir) / "sync.bundle"

            subprocess.run(
                _sync.local_bundle_create_command(config, bundle),
                cwd=repo,
                check=True,
            )
            advertised = subprocess.run(
                ("git", "bundle", "list-heads", str(bundle)),
                cwd=repo,
                check=True,
                stdout=subprocess.PIPE,
                text=True,
            ).stdout.strip()

            self.assertEqual(advertised, f"{expected_head} refs/heads/main")

    def test_remote_bundle_sync_command_resets_to_the_requested_head(self) -> None:
        config = _sync.RemoteWorkflow(
            host="builder",
            branch="main",
            remote_path="/root/codex",
            command=(),
        )

        command = _sync.remote_bundle_sync_command(
            config, "abc123", "/tmp/codex-sync.bundle"
        )

        self.assertIn("trap 'rm -f '/tmp/codex-sync.bundle'' EXIT", command)
        self.assertIn("git fetch '/tmp/codex-sync.bundle' 'refs/heads/main'", command)
        self.assertIn("test \"$(git rev-parse FETCH_HEAD)\" = 'abc123'", command)
        self.assertIn("git checkout 'main'", command)
        self.assertIn("git reset --hard FETCH_HEAD", command)
        self.assertIn("git clean -fd", command)


if __name__ == "__main__":
    unittest.main()
