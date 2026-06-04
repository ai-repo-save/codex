# Config, Approvals, Sandboxing, and Permissions

## Config ownership

- Core config types and `ConfigToml` live in `codex-rs/core/src/config`.
- Changes to `ConfigToml` or nested config types require `just write-config-schema` from `codex-rs`.
- Config lock/session exposure behavior is under `codex-rs/core/src/session/config_lock.rs`.
- Do not edit Cursor MCP config for Codex behavior. Codex MCP config lives in `/home/bluebird/.codex/config.toml`.

## Approval and permission flow

- Approval policy and sandbox decisions are core runtime behavior, not UI-only behavior.
- User-visible approval prompts and TUI presentation belong in `tui`, but the policy decision belongs in core/session/tool execution.
- Avoid caller-side bypasses for permission errors. Fix policy derivation, request metadata, or sandbox execution at the layer that owns the decision.

## Sandboxing and execution

- Sandboxing-related crates include `codex-rs/sandboxing`, `codex-rs/exec-server`, and core execution/tool paths.
- Do not add or modify code related to `CODEX_SANDBOX_NETWORK_DISABLED_ENV_VAR` or `CODEX_SANDBOX_ENV_VAR`.
- Integration tests may intentionally skip under sandbox constraints. Do not remove those skips without confirming the underlying execution environment.

## Config change checklist

- Update schema when config structs change.
- Add focused tests for parsing, defaults, validation, and precedence.
- Check app-server/config-lock exposure when the key is visible outside local CLI config.
- Treat config wire names as stable external behavior when exposed through app-server APIs.
