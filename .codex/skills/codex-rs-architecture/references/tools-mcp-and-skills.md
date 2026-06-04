# Tools, MCP, Skills, Exec, and Multi-Agent

## Tool routing

- Core tool specs and handlers live under `codex-rs/core/src/tools`.
- Tool exposure and planning live in `codex-rs/core/src/tools/spec_plan.rs`.
- Handler tests commonly live in sibling `*_tests.rs` files under `core/src/tools/handlers`.
- Prefer structured argument parsing and protocol types over ad hoc string parsing.

## Multi-agent

- Multi-agent v1 and v2 handlers live under `core/src/tools/handlers/multi_agents*`.
- Agent lifecycle, spawn metadata, task paths, mailbox notifications, and completion propagation are in `core/src/agent/control.rs` and related modules.
- V2 `spawn_agent` initial task content must remain a `UserInput` operation. `send_message` and `followup_task` use inter-agent communication semantics.
- `list_agents` and `inspect_agent` should read live agent state/history without altering child execution.

## MCP

- MCP connection and mutation behavior should use `codex-rs/codex-mcp/src/mcp_connection_manager.rs` where possible.
- Avoid plumbing MCP mutations through unrelated layers when the connection manager already owns the operation.
- MCP server behavior belongs in `codex-rs/mcp-server`; core should orchestrate tool availability and call handling.

## Skills

- Skill discovery/rendering/injection is in `codex-rs/core-skills`.
- `codex-rs/core/src/skills.rs` adapts skill loading to core session/config state.
- Repo skills live in `.codex/skills/<skill-name>/SKILL.md`; frontmatter must include `name` and `description`.
- Use progressive disclosure: keep `SKILL.md` concise and put detailed maps in `references/`.

## Exec and shell tools

- Unified execution behavior spans core tool handlers, `exec`, `exec-server`, `sandboxing`, and `shell-command`.
- Fix request/response contract issues in the tool or execution layer, not in UI rendering.
- For tests, assert structured tool call outputs through existing response helpers when available.
