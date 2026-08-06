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

# Formatting and focused local typecheck are allowed; build/test/codegen stay remote.
ALLOWED_JUST_RECIPES = frozenset({"fmt", "fmt-check"})
ALLOWED_SCRIPT_PATHS = frozenset(
    {
        "scripts/format.py",
        "scripts/local/rust_check.py",
    }
)

BLOCKED_SCRIPT_PATHS = frozenset(
    {
        "scripts/build_codex_package.py",
        "scripts/install-local-standalone.sh",
        "scripts/install/install_local_standalone.py",
    }
)
CARGO_CHECK_FORBIDDEN_FLAGS = frozenset(
    {
        "--workspace",
        "--all",
        "--all-targets",
        "--all-features",
        "--tests",
        "--benches",
        "--examples",
        "--bins",
        "--bench",
        "--example",
        "--test",
        "--bin",
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
            segment = command_segment(words, index)
            executable = PurePath(word).name
            if executable in SHELL_EXECUTABLES:
                script = shell_inline_script(words[index + 1 :])
                return script is not None and is_blocked_command(script)
            if local_command_is_allowed(segment):
                command_position = False
                index += len(segment)
                continue
            if executable in BLOCKED_EXECUTABLE_NAMES:
                return True
            if normalized_script_path(word) in BLOCKED_SCRIPT_PATHS:
                return True
            command_position = False
        index += 1
    return False


def command_segment(words: list[str], start: int) -> list[str]:
    end = start
    while end < len(words) and words[end] not in COMMAND_SEPARATORS:
        end += 1
    return words[start:end]


def local_command_is_allowed(words: list[str]) -> bool:
    if not words:
        return False
    executable = PurePath(words[0]).name
    script_path = normalized_script_path(words[0])
    if script_path in ALLOWED_SCRIPT_PATHS:
        return True
    if executable in {"python", "python3"}:
        return python_local_preflight_is_allowed(words)
    if executable == "uv":
        return uv_local_preflight_is_allowed(words)
    if executable == "just":
        return just_command_is_allowed(words)
    if executable == "cargo":
        return cargo_command_is_allowed(words)
    if executable == "rustfmt":
        return True
    return False


def python_local_preflight_is_allowed(words: list[str]) -> bool:
    for word in words[1:]:
        if word.startswith("-"):
            continue
        return normalized_script_path(word) in ALLOWED_SCRIPT_PATHS
    return False


def uv_local_preflight_is_allowed(words: list[str]) -> bool:
    if "run" not in words:
        return False
    for word in words:
        if normalized_script_path(word) in ALLOWED_SCRIPT_PATHS:
            return True
    return False


def just_command_is_allowed(words: list[str]) -> bool:
    args = words[1:]
    index = 0
    while index < len(args):
        arg = args[index]
        if arg == "--unstable":
            index += 1
            continue
        if arg == "--fmt":
            rest = args[index + 1 :]
            return all(item == "--check" for item in rest)
        if arg.startswith("-"):
            return False
        return arg in ALLOWED_JUST_RECIPES and index == len(args) - 1
    return False


def cargo_command_is_allowed(words: list[str]) -> bool:
    args = words[1:]
    index = 0
    if args and args[0].startswith("+"):
        index = 1
    if index >= len(args):
        return False
    subcommand = args[index]
    if subcommand == "fmt":
        return True
    if subcommand == "check":
        return cargo_check_is_allowed(args[index + 1 :])
    return False


def cargo_check_is_allowed(args: list[str]) -> bool:
    has_package = False
    index = 0
    while index < len(args):
        arg = args[index]
        if arg in CARGO_CHECK_FORBIDDEN_FLAGS or arg.startswith(
            ("--features=", "--all-features")
        ):
            return False
        if arg in {"-p", "--package"}:
            if index + 1 >= len(args):
                return False
            has_package = True
            index += 2
            continue
        if arg.startswith("-p=") or arg.startswith("--package="):
            has_package = True
            index += 1
            continue
        if arg in {
            "--lib",
            "--message-format",
            "--message-format=short",
            "--quiet",
            "-q",
        }:
            if arg == "--message-format":
                index += 2
                continue
            index += 1
            continue
        if arg.startswith("--message-format="):
            index += 1
            continue
        # Unknown flags or path args keep the allowlist narrow.
        return False
    return has_package


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
