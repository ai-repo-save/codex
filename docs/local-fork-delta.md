# Local Fork Capability Contract

This repository carries a maintained Codex Rust fork on top of upstream stable release tags. The
current integration baseline is `rust-v0.144.1`. Future stable syncs preserve the capabilities in
this document as behavioral contracts rather than reproducing a historical commit range.

## Stable Sync Invariants

- `main` integrates stable upstream tags through merge commits so upstream ancestry remains
  inspectable.
- Fork behavior is identified from capability ownership, configuration, schemas, and focused tests;
  local commit hashes are not part of the contract.
- Upstream API changes may replace an implementation, but they must not silently remove a fork
  capability, weaken its gates, or change its persistence and model-visible behavior.
- Generated config and app-server schemas move with the Rust types that own their wire shapes.
- CLI or agent-runtime changes are complete only after remote validation and local standalone
  installation.

## Remote Build, Validation, and Installation

All routine Codex Rust compilation, execution, testing, code generation, and standalone packaging
run on `192.168.50.8` through `scripts/remote/`. The scripts own repository synchronization,
sccache, fast-linker configuration, diagnostics, artifact transfer, and local installation.

- `scripts/remote/just.py` is the only supported path for `codex-rs` `just` recipes.
- `scripts/remote/build_sync.py` performs the remote compile-and-execute smoke workflow.
- `scripts/remote/install_local_standalone.py` builds remotely, transfers a compressed standalone
  package, installs it locally, and reports timing and diagnostic information.
- `scripts/remote/doctor.py` checks the remote Git, network, and toolchain prerequisites.
- Repository hooks guard against accidental local Rust build, test, and code-generation commands.
- The local checkout remains the source of truth. Generated files retained from a remote workflow
  are copied back before review and commit.

## Skills

- `use_skill` is a first-party model tool. A successful load returns the canonical `SKILL.md` body
  and records the skill load in conversation and rollout history.
- Skill discovery merges system, user, repository, plugin, orchestrator, and executor sources under
  their runtime gates.
- Model-visible skill metadata has a bounded context budget. Skill bodies are loaded on demand
  rather than injected in full during initial context construction.
- Skill prompt guidance remains present after upstream prompt and extension refactors.
- App-server history conversion, analytics, and resume behavior preserve skill-load items.

## Multi-Agent V2

`multi_agent_v2` is an under-development feature and remains disabled unless
`features.multi_agent_v2.enabled` is true. Enabling the feature installs the v2 spawn,
communication, inspection, follow-up, waiting, and agent-list tools and injects the applicable root
or subagent usage guidance.

- The default Responses namespace is `agents`.
- `collaboration`, `functions`, `mcp`, `mcp__*`, and other Responses-owned namespaces are reserved
  and rejected by configuration validation. This prevents model-reserved tool schema collisions,
  including GPT-5.6 rejecting `collaboration.followup_task`.
- `tool_namespace` may select another validated namespace without changing the underlying agent
  protocol.
- `MultiAgentMode` supports `none`, `explicitRequestOnly`, and `proactive`. Explicit config takes
  precedence over effort-derived defaults; a per-turn app-server override applies only to that
  turn.
- Thread and turn app-server settings preserve the effective multi-agent mode across resume and
  history reconstruction.
- Root and child sessions receive distinct configurable usage hints. Spawned children inherit the
  relevant developer context and receive a subagent identity fragment.
- Completion notifications remain agent messages, including plaintext v2 completions. Encrypted
  inter-agent messaging follows the same lifecycle when enabled.
- Concurrency limits count the root thread, and inspect/list results identify the calling agent.
- In code-mode configurations, `non_code_mode_only` controls whether collaboration tools remain
  direct model tools instead of being routed through the code executor.

## Goals and Collaboration Modes

Goals are thread state with first-party `create_goal`, `get_goal`, and `update_goal` tools,
configurable prompts, usage accounting, and app-server representation.

- An unfinished goal blocks replacement. Completed goals can be cleared or replaced on a later
  user turn according to the goal lifecycle.
- Token and elapsed-time accounting continues through turn completion and is exposed by the goal
  tools and protocol types.
- Reset-context and resume flows retain the goal/thread relationship rather than orphaning active
  state.
- Collaboration instructions resolve from configured `collaboration_modes` entries before built-in
  presets.
- Research is a first-class collaboration mode with wire value `research`, TUI selection and status
  rendering, app-server listing and settings support, analytics classification, and a built-in
  read-only investigation prompt.
- `request_user_input` is available in Research mode. Goal idle turns are permitted there, while
  Plan-only stream parsing and Default-only plan nudges keep their original mode boundaries.

## Context Lifecycle

### Reset Context

`thread/reset-context` forks the live thread context without running a preparatory compaction. The
source thread must be loaded and idle. The app-server response, goal migration, runtime workspace
roots, permissions, history reconstruction, and TUI state all follow the newly forked thread.

### Context Usage and Manual Compaction

- `get_context_usage` returns the current known context-window usage without estimating missing
  usage data.
- `request_context_compaction` accepts a bounded carry-forward note and requests normal compaction
  lifecycle processing.
- Manual compaction preserves its own turn identity and compaction phase, uses the configured local
  or remote compaction path, and retains the request tool output needed for coherent history.

### Context Anchors

- `save_context_anchor` creates a thread-local anchor at committed model context and optionally
  records a bounded label.
- `list_context_anchors` flushes persistence before reconstructing the bounded active anchor list.
- `rewind_context_to_anchor` discards later context, reports approximate reclaimed items and tokens,
  and atomically creates a replacement anchor. Its bounded carry-forward fragment identifies both
  the consumed anchor and the active replacement so reconstructed model context does not reuse a
  stale ID.
- Successful rewinds consume obsolete anchors. Reusing a consumed or otherwise unknown anchor
  returns a structured soft rejection and includes its still-active replacement when the persisted
  rewind chain can resolve one. Rewinds below `context_rewind.min_reclaim_percent` use the same
  non-mutating soft-rejection path.
- Rewind eligibility follows collaboration-mode guards, and a rewind call must be the only tool
  call in its model response.
- Anchor save and rewind events persist through rollout history, app-server thread items, analytics,
  TUI replay, and resume.

### Session-Control Tool Execution

`get_context_usage` completes inside its handler and may run through the code executor. Manual
compaction and anchor tools depend on turn-level post-processing, so they use
`ToolExposure::DirectModelOnly` and remain top-level model tools when the effective tool mode is
`CodeModeOnly`. They must not be nested under the code executor's `functions` namespace.

The handlers validate arguments and return typed outputs or requests. The session turn loop applies
the actual compaction, anchor persistence, listing, and rewind side effects after the model response
has been collected. This separation is required for correct response retention, tool-call ordering,
history mutation, and event delivery.

### Context Reminder

When `context_reminder.enabled` is true, every root and subagent session evaluates its own context
usage after token accounting. The reminder triggers when either the remaining context percentage
is at or below `context_reminder.remaining_percent` or the used token count is at or above the
optional `context_reminder.used_tokens` threshold. Omitting `used_tokens` preserves the
percentage-only default behavior; setting `enabled` to false disables both threshold checks.

Crossing either active threshold appends a hidden developer update containing the rendered
`ContextReminder` fragment. It becomes model-visible on the next inference rather than
interrupting the response that produced the usage data. The two thresholds share one crossing
state: a session receives one reminder while either condition remains active, and another reminder
is permitted only after both conditions return to their safe side. The default message directs the
agent to rewind to a suitable context anchor first and to request compaction when rewind cannot
reclaim enough context. A configured custom message replaces that guidance. Neither message
automatically mutates context or blocks the current task.

## Scoped Memories

The memories extension supports global memories and independent session/project scoped stores.
Scoped behavior is active only when the memory feature and `memories.use_scoped_memories` are both
enabled; dedicated tools also require `memories.dedicated_tools`.

- Session storage is keyed by thread. Project storage is keyed by the canonical project root.
- Initial context injection is bounded to 10,000 tokens for session memory and 15,000 tokens for
  project memory.
- `memories.list`, `memories.read`, and `memories.search` accept explicit scopes. When global
  memories are disabled, callers must select `session` or `project`.
- `memories.write_note` writes append-only Markdown notes only to session or project scope and is
  intended for explicit user requests to remember or update information.
- `memories.delete` removes one exact memory file. Directories, globs, hidden paths, and path
  traversal are rejected.
- Scoped context is contributed as a bounded contextual-user fragment with the write/delete gate
  stated to the model.
- Memory tool handlers execute backend reads and writes inside the handler and return the completed
  result. Unlike session-control tools, their filesystem side effects are not deferred to turn
  post-processing.

## Hooks, Approval Routing, and Auto-Review

- PostToolUse hooks can rewrite tool output. Blocking hook results remain blocking in both direct
  and code-mode execution.
- Hook discovery and schemas preserve configured trust behavior and supported metadata.
- Shell and command approvals carry the selected approval-review route through hook input,
  analytics, and approval handling.
- Auto-review prompt, scope, model selection, and configured approval behavior remain fork-owned
  runtime policy.
- Review sessions isolate the context they should not inherit, including skills and memories, while
  preserving the explicit review inputs.

## Tool Search and MCP

- `tool_search` accepts either a text query or an MCP prefix selector.
- Prefix expansion resolves MCP namespaces and tool names deterministically and supports calls that
  contain only the prefix selector.
- Deferred MCP and extension tools remain registered for discovery while absent from the initial
  visible tool list.
- Tool payload logging preserves text-query and prefix forms for function, namespace, search, and
  custom payloads.
- MCP runtime selection, status inventory, authentication, turn metadata, and app-server reporting
  retain their configured provider and environment boundaries.

## App Server, Schemas, and TUI

- Fork-owned protocol shapes live in app-server v2 and keep Rust, TypeScript, JSON schema, legacy
  event conversion, and thread-history reconstruction aligned.
- Context anchors, reset-context, Research mode, multi-agent mode, goals, permissions, and tool
  items are represented in the generated regular or experimental schema according to their API
  gate.
- Regular schema generation and experimental schema generation are separate workflows; generating
  experimental fixtures must not replace regular fixtures.
- TUI replay renders persisted fork items rather than reconstructing behavior from transient state.
- User-visible anchor, collaboration, goal, approval, or reset-context changes carry focused
  snapshot coverage.
- Remote TUI smoke validates compilation and one app-server/TUI RPC path but does not replace
  behavior-specific tests.

## Model Provider and Tool Mode

`model_provider` selects transport, authentication, and provider-specific request behavior from the
configured provider map. Model availability comes from the provider/catalog path; changing a
provider can therefore change the model list without changing the compiled fork capability set.

The selected model's catalog metadata may provide `tool_mode`. A recognized remote selector
(`direct`, `code_mode`, or `code_mode_only`) takes precedence over local feature-derived fallback.
Unknown selectors are ignored. Tool planning must then preserve fork tools according to their
exposure: `DirectModelOnly` controls remain top-level model tools, while code-routable tools follow
the effective code-mode plan.

Provider aliases such as a no-WebSocket OpenAI provider may alter transport behavior, but they do
not define an alternate fork tool registry. A missing tool must be diagnosed through feature gates,
effective `tool_mode`, auth/model metadata, extension gates, and namespace validation rather than
attributed to the provider name alone.

## App-Server Daemon Auto-Update

The app-server daemon reads `app_server_auto_update` from config and defaults to enabled when the
setting is absent. Bootstrap options and daemon settings carry the resolved value. Disabling the
setting prevents updater startup and stops an existing update loop after settings synchronization;
re-enabling it permits normal update-loop operation.

## Focused Remote Validation

Run formatting after source changes:

```bash
uv run --project scripts python scripts/remote/just.py fmt
```

Validate remote tooling and standalone installation:

```bash
uv run --project scripts python scripts/remote/doctor.py
uv run --project scripts python scripts/remote/build_sync.py
uv run --project scripts python scripts/remote/install_local_standalone.py
```

Validate skills, multi-agent behavior, goals, and collaboration modes:

```bash
uv run --project scripts python scripts/remote/just.py test -p codex-core skills
uv run --project scripts python scripts/remote/just.py test -p codex-core multi_agent_v2
uv run --project scripts python scripts/remote/just.py test -p codex-core multi_agent_mode
uv run --project scripts python scripts/remote/just.py test -p codex-core subagent_notifications
uv run --project scripts python scripts/remote/just.py test -p codex-goal-extension
uv run --project scripts python scripts/remote/just.py test -p codex-core collaboration_instructions
```

Validate context lifecycle and scoped memories:

```bash
uv run --project scripts python scripts/remote/just.py test -p codex-core context_anchor
uv run --project scripts python scripts/remote/just.py test -p codex-core request_context_compaction
uv run --project scripts python scripts/remote/just.py test -p codex-core context_reminder
uv run --project scripts python scripts/remote/just.py test -p codex-core compact_remote
uv run --project scripts python scripts/remote/just.py test -p codex-core tool_harness
uv run --project scripts python scripts/remote/just.py test -p codex-memories-extension
uv run --project scripts python scripts/remote/just.py test -p codex-app-server thread_reset_context
```

Validate hooks, approvals, review, tool search, and MCP:

```bash
uv run --project scripts python scripts/remote/just.py test -p codex-core approvals
uv run --project scripts python scripts/remote/just.py test -p codex-core auto_review
uv run --project scripts python scripts/remote/just.py test -p codex-core tool_search
uv run --project scripts python scripts/remote/just.py test -p codex-core mcp_turn_metadata
uv run --project scripts python scripts/remote/just.py test -p codex-core mcp_tool_exposure
```

Validate provider/tool planning, app-server protocol, daemon updates, and TUI integration:

```bash
uv run --project scripts python scripts/remote/just.py test -p codex-core model_runtime_selectors
uv run --project scripts python scripts/remote/just.py test -p codex-core code_mode
uv run --project scripts python scripts/remote/just.py test -p codex-app-server-protocol
uv run --project scripts python scripts/remote/just.py test -p codex-app-server turn_start
uv run --project scripts python scripts/remote/just.py test -p codex-app-server-daemon auto_update
uv run --project scripts python scripts/remote/tui_smoke.py
```

Regenerate affected schemas through the remote scripts:

```bash
uv run --project scripts python scripts/remote/just.py write-config-schema
uv run --project scripts python scripts/remote/just.py write-app-server-schema
uv run --project scripts python scripts/remote/just.py write-app-server-schema --experimental
```

## Stable Sync Procedure

1. Start from a clean worktree and create a backup branch.
2. Fetch and verify the target stable tag, then merge it into `main` with a merge commit.
3. Resolve conflicts against the capability contracts in this document and the current focused
   tests, not against an obsolete implementation shape.
4. Regenerate every affected config, protocol, TypeScript, JSON schema, Cargo lock, and Bazel lock
   artifact through the remote workflows.
5. Run focused remote validation for every touched capability group. Use the complete remote test
   suite when the merge crosses shared core or protocol boundaries broadly enough that focused
   coverage cannot bound the risk.
6. Review model-visible tool schemas and runtime gates with at least one supported current model,
   including `CodeModeOnly` planning when session-control tools are affected.
7. Install the validated standalone build locally and smoke the resulting CLI before declaring the
   stable sync complete.
