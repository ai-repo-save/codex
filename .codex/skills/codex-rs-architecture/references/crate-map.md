# Crate Ownership Map

## High-level crates

- `codex-rs/cli` owns the `codex` binary, command parsing, and startup handoff into TUI, exec, app-server, and other modes.
- `codex-rs/core` owns agent/session orchestration, turn execution, tool routing, model requests, approvals, config resolution at runtime, rollout persistence, and thread management.
- `codex-rs/tui` owns terminal UI rendering, bottom pane behavior, chat composer, status/event display, snapshots, and interactive TUI state.
- `codex-rs/protocol` owns shared wire/domain types used between core, UI, app-server, and clients.
- `codex-rs/app-server` and `codex-rs/app-server-protocol` own app-server behavior and v1/v2 RPC schemas.
- `codex-rs/exec`, `codex-rs/exec-server`, `codex-rs/sandboxing`, and `codex-rs/shell-command` own non-TUI command execution surfaces and sandbox/runtime command behavior.
- `codex-rs/codex-mcp` and `codex-rs/mcp-server` own MCP connection and server behavior. Core may orchestrate MCP tools, but shared MCP mutation logic belongs in the MCP crate when possible.
- `codex-rs/core-skills` owns skill discovery, metadata parsing, rendering, implicit invocation, and injection policy. `codex-rs/core/src/skills.rs` is mostly the core-facing adapter.

## Placement rules

- Add CLI flags and command wiring in `cli`; add agent behavior in `core`; add terminal visuals in `tui`; add wire types in `protocol`; add app RPC types in `app-server-protocol`.
- Keep `codex-core` from absorbing unrelated new concepts. Before editing core, check whether the behavior belongs in protocol, app-server, MCP, TUI, config, or a utility crate.
- Do not add large logic to already-large orchestration modules when a focused sibling module can own the behavior.
- Shared behavior crossing crates usually needs protocol or a narrow utility crate; avoid duplicating stringly typed contracts across call sites.

## Common ownership examples

- A tool call parses valid JSON but dispatches incorrectly: `core/src/tools` or the relevant handler module.
- A model-visible message has the wrong role/content/phase: `protocol` types plus `core` context/history/session conversion.
- A TUI event appears with wrong styling or ordering: `tui` event rendering modules and snapshots.
- An app client RPC shape changes: `app-server-protocol`, schema generation, and app-server docs/examples.
- A config key is added or renamed: `core/src/config`, schema generation, and config-lock behavior if exposed to sessions.
