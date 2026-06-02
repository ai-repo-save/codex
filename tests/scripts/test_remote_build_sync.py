import sys
from pathlib import Path
from typing import Any

import pytest

sys.path.insert(0, str(Path(__file__).resolve().parents[2]))

from scripts.remote import build_sync
from scripts.remote.build_sync import Config
from scripts.remote.build_sync import StatusPlan
from scripts.remote.build_sync import parse_porcelain_status
from scripts.remote.build_sync import run_remote_command


def test_parse_porcelain_status_maps_git_visible_changes() -> None:
    raw_status = (
        b" M AGENTS.md\0"
        b"A  scripts/remote/build_sync.py\0"
        b"?? tests/scripts/test_remote_build_sync.py\0"
        b"D  stale.txt\0"
        b"R  new-name.txt\0old-name.txt\0"
    )

    assert parse_porcelain_status(raw_status) == StatusPlan(
        copy_paths=(
            "AGENTS.md",
            "scripts/remote/build_sync.py",
            "tests/scripts/test_remote_build_sync.py",
            "new-name.txt",
        ),
        delete_paths=("stale.txt", "old-name.txt"),
    )


@pytest.mark.parametrize("raw_status", [b" M ../outside.txt\0", b" M /tmp/outside.txt\0"])
def test_parse_porcelain_status_rejects_paths_outside_checkout(raw_status: bytes) -> None:
    with pytest.raises(ValueError):
        parse_porcelain_status(raw_status)


def test_run_remote_command_logs_and_executes_shell_quoted_command(
    monkeypatch: pytest.MonkeyPatch,
    caplog: pytest.LogCaptureFixture,
) -> None:
    calls: list[tuple[tuple[str, ...], dict[str, Any]]] = []

    def fake_run(command: tuple[str, ...], **kwargs: Any) -> None:
        calls.append((command, kwargs))

    monkeypatch.setattr(build_sync, "run", fake_run)
    caplog.set_level("INFO", logger="remote_build_sync")

    run_remote_command(
        Config(
            host="builder",
            branch="main",
            remote_path="/root/codex",
            command=(
                "bash",
                "-lc",
                'cd codex-rs && cargo build -p codex-cli --bin codex | rg "Build ID"',
            ),
        )
    )

    assert calls == [
        (
                (
                    "ssh",
                    "builder",
                    "set -euo pipefail; cd '/root/codex'; "
                    "bash -lc 'cd codex-rs && cargo build -p codex-cli --bin codex | rg \"Build ID\"'",
                ),
                {"heartbeat_interval_seconds": build_sync.REMOTE_COMMAND_HEARTBEAT_SECONDS},
            )
        ]
    assert [
        record.message for record in caplog.records if record.name == "remote_build_sync"
    ] == [
        "running remote command: "
        "bash -lc 'cd codex-rs && cargo build -p codex-cli --bin codex | rg \"Build ID\"'"
    ]
