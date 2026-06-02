import sys
from pathlib import Path

import pytest

sys.path.insert(0, str(Path(__file__).resolve().parents[2]))

from scripts.remote.build_sync import StatusPlan
from scripts.remote.build_sync import parse_porcelain_status


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
