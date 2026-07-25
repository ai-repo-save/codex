---
name: codex-rs-architecture
description: Reference map for the Codex Rust repository architecture and common modification entry points. Use when changing or investigating Codex CLI, TUI, exec, core agent/session behavior, multi-agent behavior, model context, config loading, approvals, sandboxing, MCP, tools, skills, app-server APIs, protocol types, rollout/session persistence, or when deciding which codex-rs crate owns a feature.
---

# Codex Rust Architecture

Use this skill before modifying Codex internals or when a task is unclear about which crate owns the behavior. Select the narrowest reference that matches the requested change; do not load every reference by default.

## Reference selection

- **Crate ownership and entrypoints**: read `references/crate-map.md`.
- **Agent, session, thread, turns, and model context**: read `references/agent-turn-flow.md`.
- **Configuration, approvals, sandboxing, and permissions**: read `references/config-and-permissions.md`.
- **Tools, MCP, skills, exec, and multi-agent behavior**: read `references/tools-mcp-and-skills.md`.
- **TUI, CLI, exec command, and visible output surfaces**: read `references/ui-and-entrypoints.md`.
- **App-server APIs and protocol surfaces**: read `references/app-server-api.md`.
- **Remote validation, tests, snapshots, and local install**: read `references/testing-and-validation.md`.

## Operating rules

- Locate the owning crate before editing. Avoid adding new concepts to `codex-core` when a narrower crate already owns the behavior.
- Prefer bottom-layer fixes. If a request parser, tool router, or transport contract is wrong, fix that layer rather than adding caller-side compatibility.
- Treat model-visible context as high risk. Follow the existing producer, tool-output, and
  context-window budget owners; intermediate adapters preserve typed fragments without
  re-budgeting them. Review new fragments for prompt-cache impact.
- For user-visible TUI changes, include snapshot coverage and inspect the rendered behavior path.
- For Codex CLI or agent-behavior fixes that pass remote validation, install the updated standalone build locally before handoff unless the user explicitly excludes local install.
