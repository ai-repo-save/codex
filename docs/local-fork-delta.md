# Local Fork Delta

This fork carries local Codex Rust changes on top of upstream release tags. Stable release syncs
derive the fork-owned side from first-parent history and preserve the behavior listed here when
merging a newer upstream tag.

## Baseline

- Local branch: `main`
- Fork remote: `origin`
- Upstream remote: `upstream`
- Last merged stable tag: `rust-v0.143.0`
- Upstream tag commit: `c4d748f586a84a3ed5b6aceb82e9a1db4abb1cda`
- Local merge commit for `rust-v0.143.0`: `722aa36f29136a489ab21885c038f7e67140175c`
- Completed 0.143 merge-repair boundary:
  `25742b1a057d0fa7bf2dfffa85f8fc1bc44e1f10`

`722aa36f29..HEAD` is the full local history after the 0.143 merge commit. It includes
merge-repair commits that adapted fork behavior to upstream API drift. `25742b1a05..HEAD` is the
post-repair feature delta and is the most useful range for identifying behavior newly added after
the completed 0.143 sync.

Useful local queries:

- Full post-0.143 history:
  `git log --first-parent --reverse --format='%h%x09%s' 722aa36f29..HEAD`
- New post-repair local behavior:
  `git log --first-parent --reverse --format='%h%x09%s' 25742b1a05..HEAD`
- Touched paths for the next stable conflict plan:
  `git diff --name-status 722aa36f29..HEAD`

For the next stable sync, fetch the target tag before merging so the local checkout records the
exact upstream tag object and commit being integrated.

## Fork-Owned Behavior

### Remote build, validation, and install

- Codex Rust builds, tests, code generation, and local standalone installs run through
  `scripts/remote/` workflows on the remote execution host.
- Remote workflows own checkout synchronization, sccache, linker setup, and command diagnostics;
  ad hoc `ssh`, `scp`, and local `just` invocations are outside the supported workflow.
- Local standalone install supports diagnose and auto-build modes, defaults to the dev-small
  profile, records timing information, and uses compressed transfer packaging.
- Local build guard hooks block accidental local Rust build/test/codegen commands while allowing
  approved remote validation commands.

### Skills and tool context

- `use_skill` is a first-party tool and loaded skills are visible in conversation history.
- Skill prompt rendering and injection are fork-owned behavior, including restored `use_skill`
  prompt guidance after upstream merges.
- The Codex Rust architecture skill is repository guidance for internal Codex changes.

### Multi-agent behavior

- Multi-agent tools include inspect/list behavior that identifies the calling agent and supports
  v2 spawn/message/follow-up flows.
- Subagent identity context, spawn initial task delivery, plaintext completion notifications, and
  notification tests are fork-owned behavior.
- `MultiAgentMode` is configurable. `none` keeps tools available without injected mode
  instructions, `explicitRequestOnly` requires user request, and `proactive` allows delegation
  when useful.
- App-server `turn/start.multiAgentMode` accepts `none`, `explicitRequestOnly`, and `proactive`;
  omission keeps the loaded session mode.
- Side-thread replay and side-stack repro commits were reverted. Repro-only behavior from those
  reverted commits is not part of the preserved fork surface.

### Goal state and collaboration prompts

- Goal prompts are configurable and exposed through the core API.
- Completed goals are cleared or replaced according to local turn-state rules.
- Goal continuation context guidance, paused goal edit tooling, generated goal schemas, and goal
  thread id sync fixes are fork-owned behavior.
- Collaboration mode prompts resolve from the configured `collaboration_modes` map before falling
  back to built-in presets.
- Config schema generation covers collaboration prompt configuration.

### Reset-context and TUI flow

- Reset-context is exposed through app-server protocol types, generated schema, core handlers, and
  TUI commands.
- Reset-context forks live context without unnecessary compaction and remains responsive in the TUI.
- TUI command splitting, remote TUI smoke coverage, source-build snapshot guidance, and state-DB
  resume discovery are fork-owned behavior.

### Context compaction, inspection, and anchors

- Context usage and context compaction request tools are fork-owned behavior.
- Context anchors can be saved, listed, and rewound from tool calls.
- Rewind reports approximate benefit, enforces `[context_rewind].min_reclaim_percent`, returns a
  soft rejection when the threshold is not met, and consumes obsolete anchors after a successful
  rewind.
- Anchor save/rewind activity is preserved in app-server history, TUI replay, analytics, and
  snapshots.
- Rewind behavior is guarded across collaboration modes, and rewind carry-forward notes are
  explicit context fragments.
- Compact inspect, cancellable history compaction, PostCompact supplements, and compaction phase
  hook input are fork-owned behavior.
- Mid-turn compaction continuation supplements were reverted. That reverted implementation is not
  part of the preserved fork surface.

### Scoped memories and context reminders

- Session and project scoped memories are fork-owned behavior.
- Scoped memories support exact-path deletion through the memories delete tool, local delete
  backend, and scoped request/response types.
- Root context reminders and configurable context reminder messages are fork-owned behavior.
- `ContextReminderConfig` is exported through the core API and used by the thread-manager sample.

### Research collaboration mode

- `Research` is a collaboration mode with wire value `research` and display name `Research`.
- TUI exposes `/research`, includes Research in the visible collaboration mode cycle, and renders
  Research in footer/status surfaces.
- App-server `collaborationMode/list` returns the Research preset.
- App-server `thread/settings/update.collaborationMode` and `turn/start.collaborationMode` accept
  `mode: "research"`.
- `request_user_input` is available in Research mode.
- Goal idle turns are allowed in Research mode.
- Plan stream parsing remains limited to Plan mode, and the Plan nudge is hidden outside Default
  mode.
- Research mode uses the default reasoning effort and a built-in read-only investigation prompt
  unless a client supplies explicit developer instructions.

### Hooks, approval routing, and auto-review

- PostToolUse output rewrite support, blocking hook behavior, and updated PostToolUse fixtures are
  fork-owned behavior.
- Approval review route hooks, hook schemas, config schemas, analytics, and shell approval route
  tests are fork-owned behavior.
- Auto-review prompt and scope config are fork-owned behavior.

### Tool search and MCP prefixes

- `tool_search` accepts either a text query or an MCP prefix.
- MCP prefix expansion can resolve MCP namespace and tool names deterministically.
- Tool payload logging preserves query-or-prefix behavior for function, tool search, and custom
  payload shapes.

### App-server, schema, and merge repair work

- Local release-sync commits may regenerate app-server schemas, config schemas, Cargo locks, and
  Bazel locks.
- Schema generation distinguishes regular and experimental outputs; experimental generation must
  not overwrite regular fixtures unless the regular fixture change is expected.
- The 0.143 merge repair set preserved context anchors, reset-context, Research mode,
  configurable multi-agent mode, app-server schemas, stable schema fixtures, legacy event
  conversion, compaction code generation, and focused app-server/core/TUI test expectations.

## Post-0.143 Conflict Hotspots

### Core session, tools, and config

- Context anchor behavior is concentrated in `codex-rs/core/src/session/context_anchor.rs`,
  `codex-rs/core/src/tools/handlers/context_anchor.rs`, and the related integration tests.
- Rewind carry-forward context is owned by `codex-rs/core/src/context/`.
- Config resolution for context rewind, multi-agent mode, and collaboration prompts is owned by
  `codex-rs/core/src/config/mod.rs`, `codex-rs/config/src/config_toml.rs`, and generated
  `codex-rs/core/config.schema.json`.
- MCP prefix search behavior is owned by `codex-rs/core/src/tools/handlers/tool_search.rs` and
  `codex-rs/tools/src/tool_payload.rs`.

### App-server protocol and history

- Thread item variants for context anchor save/rewind are represented in
  `codex-rs/app-server-protocol/src/protocol/v2/item.rs`.
- Reset-context and multi-agent mode wire surfaces are represented in
  `codex-rs/app-server-protocol/src/protocol/v2/thread.rs`.
- Thread history reconstruction for anchor events is represented in
  `codex-rs/app-server-protocol/src/protocol/thread_history.rs` and app-server bespoke event
  handling.
- App-server schema fixtures under `codex-rs/app-server-protocol/schema/` are expected to move
  with protocol changes.

### TUI and visible history

- Context anchor display and replay are represented in `codex-rs/tui/src/context_anchor_display.rs`,
  `codex-rs/tui/src/thread_transcript.rs`, and `codex-rs/tui/src/chatwidget/replay.rs`.
- Research mode entry, cycling, and status rendering are represented in
  `codex-rs/tui/src/collaboration_modes.rs`, `codex-rs/tui/src/slash_command.rs`, TUI slash
  dispatch, and footer/status modules.
- Snapshot updates are expected when visible anchor or collaboration mode output changes.

### Remote install script

- Local standalone install compression is owned by `scripts/remote/install_local_standalone.py`.
- Keep the script single-purpose: it builds remotely, transfers the standalone artifact, installs
  it locally, and reports diagnose/timing data.

## Stable Sync Procedure

1. Verify the worktree is clean and create a backup branch before fetching or merging a stable tag.
2. Fetch the target upstream tag, record its tag object and peeled commit, and merge it into
   `main` with a merge commit.
3. Resolve conflicts by preserving the fork-owned behavior above and accepting upstream changes
   only where they do not remove or silently weaken that behavior.
4. Regenerate affected app-server schemas, config schemas, Cargo locks, and Bazel locks through
   the remote scripts.
5. Run focused remote tests for every conflicted subsystem and every fork-owned behavior touched by
   the merge.
6. Run the complete remote test suite for broad stable merges, classify known environment-sensitive
   failures separately, and fix merge regressions before local install.
7. Install the validated standalone build locally when CLI or agent behavior changed.
