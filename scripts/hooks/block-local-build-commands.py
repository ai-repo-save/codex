#!/usr/bin/env -S uv run python
"""Block local build/test/codegen commands from Codex PreToolUse hooks."""

import json
import shlex
import sys
from dataclasses import dataclass
from pathlib import PurePath
from typing import Any


BLOCK_REASON = (
    "Local build/test/codegen commands are forbidden in this repository. "
    "Run them through the repository remote wrappers instead."
)
REMOTE_BLOCK_REASON = (
    "Direct remote build/test/codegen commands are forbidden in this repository, "
    "even when invoked through ssh. Use the repository remote wrappers instead. "
    "For codex-rs just recipes, run: "
    "uv run --project scripts python scripts/remote/just.py <recipe> [args...]"
)
BRANCH_SWITCH_BLOCK_REASON = (
    "Changing the current Git branch is forbidden while Codex is running. "
    "Keep the checkout on the branch where the session started."
)

BLOCKED_EXECUTABLE_NAMES = frozenset(
    {
        "j" + "ust",
        "car" + "go",
        "bazel",
        "bazelisk",
        "rustc",
        "rustfmt",
        "car" + "go-insta",
        "car" + "go-nextest",
        "ma" + "ke",
        "ninja",
        "cmake",
    }
)

BLOCKED_SCRIPT_PATHS = frozenset(
    {
        "scripts/format.py",
        "scripts/build_codex_package.py",
        "scripts/install-local-standalone.sh",
        "scripts/install/install_local_standalone.py",
    }
)
SHELL_EXECUTABLES = frozenset({"bash", "sh", "zsh", "fish"})
REMOTE_EXECUTABLES = frozenset({"ssh", "scp", "rsync"})
COMMAND_SEPARATORS = frozenset({";", "&&", "||", "|", "(", ")"})
GIT_OPTIONS_WITH_VALUE = frozenset(
    {
        "-C",
        "-c",
        "--config-env",
        "--exec-path",
        "--git-dir",
        "--namespace",
        "--super-prefix",
        "--work-tree",
    }
)


@dataclass(frozen=True)
class HookRequest:
    tool_name: str
    command: str


def main() -> int:
    payload = json.load(sys.stdin)
    request = parse_hook_request(payload)
    if request is None:
        return 0

    if reason := blocked_command_reason(request.command):
        print(json.dumps(block_decision(request.command, reason), ensure_ascii=False))
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
    return blocked_command_reason(command) is not None


def blocked_command_reason(command: str) -> str | None:
    try:
        words = shlex.split(command, comments=False, posix=True)
    except ValueError:
        return None

    words = strip_env_prefix(words)
    if not words:
        return None

    executable = PurePath(words[0]).name
    if executable in REMOTE_EXECUTABLES:
        if remote_command_is_blocked(words):
            return REMOTE_BLOCK_REASON
        return None
    if shell_words_switch_branch(words):
        return BRANCH_SWITCH_BLOCK_REASON
    if shell_words_are_blocked(words):
        return BLOCK_REASON
    return None


def strip_env_prefix(words: list[str]) -> list[str]:
    if words[:2] == ["env", "--"]:
        words = words[2:]
    while words and words[0] == "env":
        words = words[1:]
        while words and "=" in words[0]:
            words = words[1:]
    return words


def shell_words_are_blocked(words: list[str]) -> bool:
    index = 0
    command_position = True
    while index < len(words):
        word = words[index]
        if word in COMMAND_SEPARATORS:
            command_position = True
            index += 1
            continue

        if command_position:
            executable = PurePath(word).name
            if executable in SHELL_EXECUTABLES:
                script = shell_inline_script(words[index + 1 :])
                return script is not None and is_blocked_command(script)
            if executable in BLOCKED_EXECUTABLE_NAMES:
                return True
            if normalized_script_path(word) in BLOCKED_SCRIPT_PATHS:
                return True
            command_position = False
        index += 1
    return False


def shell_words_switch_branch(words: list[str]) -> bool:
    index = 0
    command_position = True
    while index < len(words):
        word = words[index]
        if word in COMMAND_SEPARATORS:
            command_position = True
            index += 1
            continue

        if command_position:
            executable = PurePath(word).name
            if executable in SHELL_EXECUTABLES:
                script = shell_inline_script(words[index + 1 :])
                return script is not None and blocked_branch_switch(script)
            if executable == "git" and git_command_switches_branch(words[index:]):
                return True
            command_position = False
        index += 1
    return False


def blocked_branch_switch(command: str) -> bool:
    try:
        words = shlex.split(command, comments=False, posix=True)
    except ValueError:
        return False
    return shell_words_switch_branch(strip_env_prefix(words))


def git_command_switches_branch(words: list[str]) -> bool:
    subcommand_index = git_subcommand_index(words)
    if subcommand_index is None:
        return False

    subcommand = words[subcommand_index]
    if subcommand == "switch":
        return True
    if subcommand != "checkout":
        return False
    return "--" not in words[subcommand_index + 1 :]


def git_subcommand_index(words: list[str]) -> int | None:
    index = 1
    while index < len(words):
        word = words[index]
        if word == "--":
            return index + 1 if index + 1 < len(words) else None
        if word in GIT_OPTIONS_WITH_VALUE:
            index += 2
            continue
        if any(
            word.startswith(f"{option}=")
            for option in GIT_OPTIONS_WITH_VALUE
            if option.startswith("--")
        ):
            index += 1
            continue
        if word.startswith("-"):
            index += 1
            continue
        return index
    return None


def remote_command_is_blocked(words: list[str]) -> bool:
    if PurePath(words[0]).name != "ssh":
        return False
    if len(words) < 3:
        return False
    return is_blocked_command(words[-1])


def shell_inline_script(words: list[str]) -> str | None:
    index = 0
    while index < len(words):
        word = words[index]
        if word in ("-c", "-lc") and index + 1 < len(words):
            return words[index + 1]
        if word.startswith("-") and "c" in word and index + 1 < len(words):
            return words[index + 1]
        index += 1
    return None


def normalized_script_path(word: str) -> str:
    path = word.removeprefix("./")
    while path.startswith("../"):
        path = path[3:]
    return path


def block_decision(command: str, reason: str) -> dict[str, object]:
    return {
        "hookSpecificOutput": {
            "hookEventName": "PreToolUse",
            "permissionDecision": "deny",
            "permissionDecisionReason": f"{reason} Blocked command: {command}",
        }
    }


if __name__ == "__main__":
    raise SystemExit(main())
