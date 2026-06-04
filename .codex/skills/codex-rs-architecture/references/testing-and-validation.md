# Testing, Remote Validation, and Local Install

## Default validation model

- Local performance is insufficient for routine compile/test/codegen in this repository.
- Commit local source changes, push `main`, update `/root/codex` on `192.168.50.8`, then run build/test/fix commands remotely.
- The local checkout remains the source of truth. If remote commands produce tracked changes, copy them back, inspect the local diff, and commit locally.

## Rust validation

- Run `just fmt` in `codex-rs` after Rust changes.
- Run focused tests with `just test -p <crate>`.
- Run `just fix -p <crate>` before finalizing large or Rust code changes; do not rerun tests after `fix` or `fmt` unless a new code edit follows.
- For core/common/protocol changes, ask before running the complete `just test` suite.

## Special validation

- TUI visible changes require snapshot tests and snapshot review.
- Config type changes require `just write-config-schema`.
- App-server protocol changes require schema generation and `just test -p codex-app-server-protocol`.
- Rust dependency changes require Bazel lock update and lock check.

## Local installation

- After Codex CLI or agent-behavior changes pass relevant remote validation and no blocking regression remains, run:

```bash
uv run --project scripts python scripts/remote/install_local_standalone.py
```

- This builds the standalone package remotely, copies it locally, updates the active local Codex command, and verifies the install.
- Skip local install only when the user explicitly excludes it, the change does not affect local CLI/agent runtime, or validation has not reached a stable state. Report the reason when skipping.
