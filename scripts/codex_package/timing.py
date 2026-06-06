"""Timing helpers for package build scripts."""

from __future__ import annotations

import logging
import subprocess
import time
from collections.abc import Iterator
from contextlib import contextmanager
from pathlib import Path


@contextmanager
def timed_step(logger: logging.Logger, description: str) -> Iterator[None]:
    started_at = time.monotonic()
    logger.info("starting %s", description)
    try:
        yield
    except Exception:
        elapsed = time.monotonic() - started_at
        logger.exception("failed %s after %.1fs", description, elapsed)
        raise
    logger.info("finished %s in %.1fs", description, time.monotonic() - started_at)


def run_timed(
    logger: logging.Logger,
    description: str,
    command: list[str],
    *,
    cwd: Path | None = None,
    env: dict[str, str] | None = None,
) -> subprocess.CompletedProcess[str]:
    with timed_step(logger, description):
        return subprocess.run(
            command,
            cwd=cwd,
            check=True,
            env=env,
            text=True,
        )
