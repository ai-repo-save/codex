# Testing, Remote Validation, and Local Install

## Default validation model

- Local performance is insufficient for routine compile/test/codegen in this repository.
- Commit local source changes, push `main`, update `/root/codex` on `192.168.50.8`, then run build/test/fix commands remotely.
- The local checkout remains the source of truth. If remote commands produce tracked changes, copy them back, inspect the local diff, and commit locally.
- Remote Rust commands disable Cargo incremental compilation and report sccache statistics before
  and after execution.
- `scripts/remote/sccache_probe.py` measures compiler-cache reuse by rebuilding one selected
  library from a clean temporary target at the same path.
- `scripts/remote/cleanup_build_cache.py` previews or removes stale incremental generations while
  retaining recent entries and coordinating with remote Rust commands through a shared lock.

## Rust validation

- Run local `just fmt` / `just fmt-check` after source changes (requires `dotslash` on `PATH`).
- Before commit/push, run focused local typecheck with
  `uv run --project scripts python scripts/local/rust_check.py -p <crate>` (or `cargo check -p`)
  so missing imports and obvious type errors are caught without a remote round-trip. Artifacts use
  an out-of-tree `CARGO_TARGET_DIR`. This does not replace remote tests.
- Run focused tests remotely with `scripts/remote/just.py test -p <crate>`.
- Use `just.py test-diagnostic -p <crate> <filter>` for deterministic failure diagnosis without
  nextest retries. Normal `just.py test` retains the configured retry.
- Run `just.py fix -p <crate>` only for crates with handwritten Rust source changes. Its default
  target scope is `--lib --bins`; select test targets explicitly only when test source changed. Do
  not include transitively affected crates. Do not rerun tests after `fix` or `fmt` unless a new
  code edit follows.
- For core/common/protocol changes, ask before running the complete remote `just.py test` suite.

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
