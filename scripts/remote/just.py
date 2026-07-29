#!/usr/bin/env -S uv run python

import argparse
import sys
from dataclasses import dataclass
from pathlib import Path
from typing import Sequence


if __package__ in (None, ""):
    sys.path.insert(0, str(Path(__file__).resolve().parent))
    from _sync import DEFAULT_BRANCH
    from _sync import DEFAULT_HOST
    from _sync import DEFAULT_REMOTE_PATH
    from _sync import RemoteWorkflow
    from _sync import remote_codex_rs_just_command
    from _sync import run_remote_workflow
else:
    from ._sync import DEFAULT_BRANCH
    from ._sync import DEFAULT_HOST
    from ._sync import DEFAULT_REMOTE_PATH
    from ._sync import RemoteWorkflow
    from ._sync import remote_codex_rs_just_command
    from ._sync import run_remote_workflow


REMOTE_FULL_TEST_THREADS = 4


@dataclass(frozen=True)
class RemoteTestExclusion:
    package: str
    tests: tuple[str, ...]
    reason: str


REMOTE_FULL_TEST_EXCLUSIONS: tuple[RemoteTestExclusion, ...] = (
    RemoteTestExclusion(
        package="codex-apply-patch",
        tests=(
            "test_apply_patch_fails_on_write_error",
            "test_failed_move_returns_committed_destination_delta",
        ),
        reason="requires Unix mode bits to deny writes, which uid 0 bypasses",
    ),
    RemoteTestExclusion(
        package="codex-app-server",
        tests=("plugin_installed_hook_trust_write_failure_stays_untrusted",),
        reason="requires Unix mode bits to make a tracked config file unwritable",
    ),
    RemoteTestExclusion(
        package="codex-cli",
        tests=("read_probe_file_rejects_unreadable_file",),
        reason="requires Unix mode bits to deny reads, which uid 0 bypasses",
    ),
    RemoteTestExclusion(
        package="codex-core",
        tests=(
            "extension_tool_receives_turn_environment_sandbox",
            "shell_command_enforces_glob_deny_read_policy",
            "view_image_tool_applies_local_sandbox_read_denies",
        ),
        reason="requires an unprivileged sandbox to enforce read-deny permissions",
    ),
    RemoteTestExclusion(
        package="codex-core",
        tests=("managed_network_proxy_decider_survives_full_access_start",),
        reason="uses a public hostname that the configured remote DNS rewrites locally",
    ),
    RemoteTestExclusion(
        package="codex-network-proxy",
        tests=(
            "add_allowed_domain_removes_matching_deny_entry",
            "evaluate_host_policy_emits_domain_event_for_decider_allow_override",
            "evaluate_host_policy_emits_domain_event_for_decider_ask",
            "evaluate_host_policy_emits_execution_id_for_baseline_allow",
            "handle_socks5_tcp_blocks_hooked_non_https_host_in_full_mode",
            "handle_socks5_tcp_blocks_limited_mode_without_mitm_state",
            "handle_socks5_tcp_detects_tls_for_brokered_nonstandard_port_in_full_mode",
            "handle_socks5_tcp_uses_mitm_for_hooked_host_in_full_mode",
            "handle_socks5_tcp_uses_mitm_in_limited_mode",
            "host_blocked_global_wildcard_allowlist_allows_public_hosts_except_denylist",
            "host_blocked_requires_allowlist_match",
            "host_blocked_subdomain_wildcards_exclude_apex",
            "http_connect_accept_blocks_hooked_host_in_full_mode_without_mitm_state",
            "http_connect_accept_blocks_in_limited_mode",
            "http_connect_accept_defers_brokered_host_mitm_until_protocol_detection",
            "http_connect_accept_passes_environment_id_to_decider",
            "mitm_policy_allows_matching_hooked_write_in_full_mode",
            "mitm_policy_blocks_disallowed_method_and_records_telemetry",
            "mitm_policy_blocks_hook_miss_for_hooked_host_and_records_telemetry_in_full_mode",
            "mitm_policy_blocks_matching_hooked_write_in_limited_mode",
        ),
        reason="uses public hostnames that the configured remote DNS rewrites to local addresses",
    ),
)


def argument_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        description=(
            "Run a codex-rs just recipe on the remote execution host after "
            "syncing the selected local branch."
        ),
        epilog=(
            "Behavior: pushes the selected branch to origin, resets and cleans "
            "the shared remote checkout, then copies Git-visible remote changes "
            "back to the unchanged local checkout.\n\n"
            "Examples:\n"
            "  scripts/remote/just.py --branch sync/rust-v0.146.0 test "
            "-p codex-core context_anchor\n"
            "  scripts/remote/just.py --branch sync/rust-v0.146.0 "
            "--remote-full test"
        ),
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    parser.add_argument(
        "--remote-full",
        action="store_true",
        help=(
            "run the full workspace test suite with the configured remote-host "
            "isolation policy; valid only with the bare `test` recipe"
        ),
    )
    parser.add_argument(
        "--branch",
        default=DEFAULT_BRANCH,
        help=f"local and remote Git branch to synchronize (default: {DEFAULT_BRANCH})",
    )
    parser.add_argument("recipe", help="just recipe to run")
    parser.add_argument(
        "recipe_args",
        nargs=argparse.REMAINDER,
        help="arguments forwarded unchanged to the just recipe",
    )
    return parser


def remote_full_filter_expression() -> str:
    package_filters: list[str] = []
    for exclusion in REMOTE_FULL_TEST_EXCLUSIONS:
        test_filters = " | ".join(f"test({test})" for test in exclusion.tests)
        package_filters.append(f"(package({exclusion.package}) & ({test_filters}))")
    return f"not ({' | '.join(package_filters)})"


def remote_full_recipe_args() -> tuple[str, ...]:
    return (
        "test",
        f"--test-threads={REMOTE_FULL_TEST_THREADS}",
        "-E",
        remote_full_filter_expression(),
    )


def remote_full_command() -> tuple[str, ...]:
    command = remote_codex_rs_just_command(remote_full_recipe_args())
    shell_command = command[2]
    isolated_tmp = (
        'mkdir -p "$HOME/.cache"; '
        'remote_test_tmpdir="$(mktemp -d "$HOME/.cache/codex-remote-tests.XXXXXX")"; '
        "trap 'rm -rf -- \"$remote_test_tmpdir\"' EXIT; "
        'export TMPDIR="$remote_test_tmpdir"'
    )
    return (*command[:2], f"{isolated_tmp}; {shell_command}")


def print_remote_full_policy() -> None:
    excluded_count = sum(
        len(exclusion.tests) for exclusion in REMOTE_FULL_TEST_EXCLUSIONS
    )
    print(
        (
            "remote full validation: "
            f"{REMOTE_FULL_TEST_THREADS} test threads, isolated TMPDIR, "
            f"{excluded_count} environment-incompatible tests excluded:"
        ),
        file=sys.stderr,
    )
    for exclusion in REMOTE_FULL_TEST_EXCLUSIONS:
        for test in exclusion.tests:
            print(
                f"  - {exclusion.package}::{test}: {exclusion.reason}",
                file=sys.stderr,
            )


def main(argv: Sequence[str] | None = None) -> int:
    parser = argument_parser()
    args = parser.parse_args(argv)
    if args.remote_full:
        if args.recipe != "test" or args.recipe_args:
            parser.error("--remote-full requires the bare `test` recipe")
        print_remote_full_policy()
        command = remote_full_command()
    else:
        recipe_args = (args.recipe, *args.recipe_args)
        command = remote_codex_rs_just_command(recipe_args)

    return run_remote_workflow(
        RemoteWorkflow(
            host=DEFAULT_HOST,
            branch=args.branch,
            remote_path=DEFAULT_REMOTE_PATH,
            command=command,
        )
    )


if __name__ == "__main__":
    raise SystemExit(main())
