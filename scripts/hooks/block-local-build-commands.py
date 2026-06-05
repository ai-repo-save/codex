#!/usr/bin/env -S uv run python
"""Block local build/test/codegen commands from Codex PreToolUse hooks."""

import json
import re
import shlex
import sys
from dataclasses import dataclass
from pathlib import PurePath
from typing import Any


BLOCK_REASON = (
    "Local build/test/codegen commands are forbidden in this repository. "
    "Run them on 192.168.50.8 instead."
)

BLOCKED_EXECUTABLE_RE = re.compile(
    r"(^|[\s;&|()])(?:\S*/)?("
    r"just|cargo|bazel|bazelisk|rustc|rustfmt|cargo-insta|cargo-nextest|"
    r"make|ninja|cmake"
    r")($|[\s;&|()])"
)

BLOCKED_PATH_RE = re.compile(
    r"(^|[\s;&|])(?:\./|\../)?("
    r"scripts/format\.py|"
    r"scripts/build_codex_package\.py|"
    r"scripts/install-local-standalone\.sh|"
    r"scripts/install/install_local_standalone\.py"
    r")($|[\s;&|])"
)

REMOTE_EXECUTABLES = frozenset({"ssh", "scp", "rsync"})


@dataclass(frozen=True)
class HookRequest:
    tool_name: str
    command: str


def main() -> int:
    payload = json.load(sys.stdin)
    request = parse_hook_request(payload)
    if request is None:
        return 0

    if is_blocked_command(request.command):
        print(json.dumps(block_decision(request.command), ensure_ascii=False))
    return 0


def parse_hook_request(payload: dict[str, Any]) -> HookRequest | None:
    tool_input = payload.get("tool_input")
    if not isinstance(tool_input, dict):
        return None

    command = first_string_value(tool_input, ("command", "cmd"))
    if command is None:
        return None

    tool_name = payload.get("tool_name")
    return HookRequest(
        tool_name=tool_name if isinstance(tool_name, str) else "",
        command=command,
    )


def first_string_value(mapping: dict[str, Any], keys: tuple[str, ...]) -> str | None:
    for key in keys:
        value = mapping.get(key)
        if isinstance(value, str):
            return value
    return None


def is_blocked_command(command: str) -> bool:
    if starts_with_remote_executable(command):
        return False

    return (
        BLOCKED_EXECUTABLE_RE.search(command) is not None
        or BLOCKED_PATH_RE.search(command) is not None
    )


def starts_with_remote_executable(command: str) -> bool:
    try:
        words = shlex.split(command, comments=False, posix=True)
    except ValueError:
        return False

    if words[:2] == ["env", "--"]:
        words = words[2:]
    while words and words[0] == "env":
        words = words[1:]
        while words and "=" in words[0]:
            words = words[1:]

    if not words:
        return False

    return PurePath(words[0]).name in REMOTE_EXECUTABLES


def block_decision(command: str) -> dict[str, object]:
    return {
        "hookSpecificOutput": {
            "hookEventName": "PreToolUse",
            "permissionDecision": "deny",
            "permissionDecisionReason": f"{BLOCK_REASON} Blocked command: {command}",
        }
    }


if __name__ == "__main__":
    raise SystemExit(main())
