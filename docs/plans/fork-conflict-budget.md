# Fork Conflict Budget

This plan governs how local fork capabilities touch upstream-shared files so stable syncs stay
reviewable as the fork grows.

Baseline snapshot: [`fork-conflict-baseline.tsv`](fork-conflict-baseline.tsv)
(`HEAD=9e9ec69cda` vs `upstream/main=57f42a8113`).

## Rules for new fork work

1. Prefer **A** (new files / `codex-rs/ext/*` / capability hosts) over **B** (patches to hot files).
2. A PR that edits a hot file must state why an extension point is insufficient and keep the hot-file
   delta to wiring only (target: fewer than about 20 lines of new logic per capability).
3. Generated schemas and snapshots are never hand-merged. Delete conflicts and regenerate with the
   remote `just.py` recipes.
4. After each `sync/rust-v*` merge, rerun
   `uv run --project scripts python scripts/fork/conflict_budget.py` and refresh the baseline table
   when the budget intentionally moves.

## Hot files (no new business logic)

| Path | Role |
|---|---|
| `codex-rs/core/src/session/turn.rs` | Turn loop; call lifecycle helpers only |
| `codex-rs/core/src/session/mod.rs` | Session orchestration; registration / thin calls |
| `codex-rs/core/src/session/session.rs` | Session construction |
| `codex-rs/core/src/config/mod.rs` | Config assembly wiring only |
| `codex-rs/core/src/config/config_tests.rs` | Prefer capability-local tests |
| `codex-rs/core/src/tools/spec_plan.rs` | Apply plan deltas only |
| `codex-rs/core/src/tools/registry.rs` | Thin hook_runtime calls |
| `codex-rs/core/src/agent/control.rs` | Thin facade over `agent-control` |
| `codex-rs/hooks/src/engine/discovery.rs` | Minimal handler-kind wiring |
| `codex-rs/hooks/src/engine/dispatcher.rs` | Minimal dispatch wiring |
| `codex-rs/hooks/src/events/*.rs` | Minimal match arms for fork handlers |
| `codex-rs/tui/src/multi_agents.rs` | Orchestration only |
| `codex-rs/tui/src/chatwidget.rs` | Registration only |
| `codex-rs/tui/src/app.rs` | Registration only |
| `codex-rs/app-server-protocol/src/protocol/thread_history.rs` | Center dispatch only |
| `codex-rs/app-server-protocol/src/protocol/event_mapping.rs` | Center dispatch only |

## Capability placement (A / B / C)

| Capability | Ownership | Class | Allowed hot-file wiring |
|---|---|---|---|
| Remote build / install | `scripts/remote/`, `scripts/install/` | A | none |
| Skills / `use_skill` | `core-skills`, skills handlers | A/B | tool registration |
| Multi-agent v2 | `tools/handlers/multi_agents_v2/`, `agent-control`, TUI modules | A/B | `agent/control` facade, plan delta |
| Goals / collaboration | `ext/goal`, `collaboration_plan`, protocol collaboration modules | A/B | plan apply, history dispatch |
| Context anchors / rewind | `session/context_anchor*`, `session/context_lifecycle.rs` | A/B | `turn.rs` drain call |
| Context reminder | `session/context_reminder.rs` | A/B | token-usage call site |
| Scoped memories | `ext/memories` | A | fragment / tool registration |
| Prompt / filter hooks | `hooks/engine/prompt_*`, `filter_*`, `core/hook_prompt.rs` | A/B | discovery match arms |
| Tool search / MCP | handlers + MCP runtime | A/B | registry / plan wiring |
| App-server fork items | `app-server-protocol` feature modules | A/B | history/event dispatch |
| Daemon auto-update | `app-server-daemon` | B | settings sync only |
| Model provider / tool_mode | provider + `ToolPlanDelta` path | B | `spec_plan` apply |

## Stable sync order

1. Skip / regenerate generated schemas and snapshots (never hand-merge JSON).
2. Resolve `codex-rs/hooks/` additive modules, then remaining shared hooks files.
3. Resolve `codex-rs/ext/*` and other A modules.
4. Resolve hand-written `app-server-protocol` feature modules, then center dispatch files.
5. Resolve `core` hot files last (`turn` / `mod` / `config` / `spec_plan` / `agent/control`).
6. Run focused remote validation from the capability filter menu in
   [`docs/local-fork-delta.md`](../local-fork-delta.md) — only filters for touched capabilities,
   not the whole menu.
7. Refresh conflict budget report and update this document's baseline when the budget changes.

## New capability template

1. Add implementation under `codex-rs/ext/<feature>/` or a dedicated core/session module.
2. Add protocol shapes in a feature module under `app-server-protocol` when wire-visible.
3. Register through existing extension / plan / turn lifecycle entry points.
4. Do not open `turn.rs`, `config/mod.rs`, or `hooks/.../discovery.rs` except for the minimum
   wiring lines required by those entry points.
5. Add focused remote tests and ownership notes in `docs/local-fork-delta.md`.
