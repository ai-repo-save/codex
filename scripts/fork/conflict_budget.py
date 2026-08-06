#!/usr/bin/env python3
"""Report fork vs upstream churn on hot files that drive merge cost.

Examples:
  uv run --project scripts python scripts/fork/conflict_budget.py
  uv run --project scripts python scripts/fork/conflict_budget.py --base upstream/main --fail-above 900
"""

from __future__ import annotations

import argparse
import subprocess
import sys
from dataclasses import dataclass
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]

HOT_FILES = (
    "codex-rs/core/src/session/turn.rs",
    "codex-rs/core/src/session/mod.rs",
    "codex-rs/core/src/session/session.rs",
    "codex-rs/core/src/config/mod.rs",
    "codex-rs/core/src/config/config_tests.rs",
    "codex-rs/core/src/tools/spec_plan.rs",
    "codex-rs/core/src/tools/registry.rs",
    "codex-rs/core/src/agent/control.rs",
    "codex-rs/hooks/src/engine/discovery.rs",
    "codex-rs/hooks/src/engine/dispatcher.rs",
    "codex-rs/hooks/src/engine/command_runner.rs",
    "codex-rs/tui/src/multi_agents.rs",
    "codex-rs/tui/src/chatwidget.rs",
    "codex-rs/tui/src/app.rs",
    "codex-rs/app-server-protocol/src/protocol/thread_history.rs",
    "codex-rs/app-server-protocol/src/protocol/event_mapping.rs",
)

HOOKS_EVENTS_GLOB = "codex-rs/hooks/src/events/*.rs"


@dataclass(frozen=True)
class FileChurn:
    path: str
    added: int
    deleted: int

    @property
    def total(self) -> int:
        return self.added + self.deleted


def _run_git(args: list[str]) -> str:
    completed = subprocess.run(
        ["git", *args],
        cwd=REPO_ROOT,
        check=False,
        capture_output=True,
        text=True,
    )
    if completed.returncode != 0:
        stderr = completed.stderr.strip() or completed.stdout.strip() or "git failed"
        raise RuntimeError(stderr)
    return completed.stdout


def _parse_numstat(stdout: str) -> list[FileChurn]:
    rows: list[FileChurn] = []
    for line in stdout.splitlines():
        if not line.strip():
            continue
        parts = line.split("\t")
        if len(parts) < 3:
            continue
        added_s, deleted_s, path = parts[0], parts[1], parts[2]
        if not added_s.isdigit() or not deleted_s.isdigit():
            continue
        rows.append(FileChurn(path=path, added=int(added_s), deleted=int(deleted_s)))
    return rows


def collect_churn(base: str, head: str) -> list[FileChurn]:
    rows: list[FileChurn] = []
    for path in HOT_FILES:
        out = _run_git(["diff", "--numstat", f"{base}...{head}", "--", path])
        parsed = _parse_numstat(out)
        if parsed:
            rows.extend(parsed)
        else:
            rows.append(FileChurn(path=path, added=0, deleted=0))
    events_out = _run_git(["diff", "--numstat", f"{base}...{head}", "--", HOOKS_EVENTS_GLOB])
    rows.extend(_parse_numstat(events_out))
    rows.sort(key=lambda row: (-row.total, row.path))
    return rows


def format_report(rows: list[FileChurn], base: str, head: str) -> str:
    head_sha = _run_git(["rev-parse", "--short", head]).strip()
    base_sha = _run_git(["rev-parse", "--short", base]).strip()
    lines = [
        f"fork conflict budget: {base}({base_sha})...{head}({head_sha})",
        f"{'sum':>6}  {'+added':>7}  {'-deleted':>8}  path",
    ]
    total = 0
    for row in rows:
        total += row.total
        lines.append(f"{row.total:6d}  {row.added:7d}  {row.deleted:8d}  {row.path}")
    lines.append(f"{total:6d}  {'':>7}  {'':>8}  TOTAL")
    return "\n".join(lines) + "\n"


def parse_args(argv: list[str]) -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description=(
            "Report numstat churn for fork hot files versus an upstream base. "
            "Use after stable syncs to track merge-cost trends."
        )
    )
    parser.add_argument(
        "--base",
        default="upstream/main",
        help="Git ref for the upstream side (default: upstream/main)",
    )
    parser.add_argument(
        "--head",
        default="HEAD",
        help="Git ref for the fork side (default: HEAD)",
    )
    parser.add_argument(
        "--fail-above",
        type=int,
        default=None,
        help="Exit 1 when TOTAL churn exceeds this threshold",
    )
    parser.add_argument(
        "--tsv",
        type=Path,
        default=None,
        help="Optional path to write path/added/deleted/sum TSV",
    )
    return parser.parse_args(argv)


def main(argv: list[str]) -> int:
    args = parse_args(argv)
    try:
        rows = collect_churn(args.base, args.head)
    except RuntimeError as exc:
        print(f"conflict_budget: {exc}", file=sys.stderr)
        return 2

    report = format_report(rows, args.base, args.head)
    sys.stdout.write(report)

    if args.tsv is not None:
        tsv_path = args.tsv if args.tsv.is_absolute() else REPO_ROOT / args.tsv
        lines = ["path\tadded\tdeleted\tsum"]
        lines.extend(f"{row.path}\t{row.added}\t{row.deleted}\t{row.total}" for row in rows)
        tsv_path.parent.mkdir(parents=True, exist_ok=True)
        tsv_path.write_text("\n".join(lines) + "\n", encoding="utf-8")
        print(f"wrote {tsv_path}", file=sys.stderr)

    total = sum(row.total for row in rows)
    if args.fail_above is not None and total > args.fail_above:
        print(
            f"conflict_budget: TOTAL {total} exceeds --fail-above {args.fail_above}",
            file=sys.stderr,
        )
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
