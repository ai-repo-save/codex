#!/usr/bin/env python3
import sys
import subprocess
import tempfile
import unittest
from pathlib import Path


SCRIPT_DIR = Path(__file__).resolve().parent
sys.path.insert(0, str(SCRIPT_DIR))

import _sync  # noqa: E402


class RemoteSyncTest(unittest.TestCase):
    def test_remote_codex_rs_test_command_uses_nextest_without_bench_smoke(
        self,
    ) -> None:
        command = _sync.remote_codex_rs_just_command(("test", "-p", "codex-app-server"))

        self.assertEqual(command[0:2], ("bash", "-lc"))
        shell_command = command[2]
        self.assertIn("export RUSTC_WRAPPER=", shell_command)
        self.assertIn(
            "export CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER=clang",
            shell_command,
        )
        self.assertIn("-C link-arg=-fuse-ld=$(command -v mold)", shell_command)
        self.assertIn(
            "cd codex-rs && RUST_MIN_STACK=8388608 cargo nextest run --no-fail-fast -p codex-app-server",
            shell_command,
        )
        self.assertNotIn("just test", shell_command)
        self.assertNotIn("bench-smoke", shell_command)

    def test_remote_codex_rs_non_test_command_uses_just_recipe(self) -> None:
        command = _sync.remote_codex_rs_just_command(("fmt",))

        self.assertEqual(command[0:2], ("bash", "-lc"))
        self.assertIn("cd codex-rs && just fmt", command[2])

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
