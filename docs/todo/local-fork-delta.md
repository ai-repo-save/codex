# Local Fork Delta

This fork carries local Codex Rust changes on top of upstream release tags. Stable release syncs
must identify the local side from first-parent commits rather than from ad hoc feature memory.

## Baseline

- Local branch: `main`
- Upstream remote: `upstream`
- Last merged stable tag before this document: `rust-v0.141.0`
- Local delta query:
  `git log --first-parent --reverse --format='%h%x09%s' upstream/main..HEAD`
- Touched-path query:
  `git log --first-parent --name-only --format='' upstream/main..HEAD | sed '/^$/d' | sort | uniq -c | sort -nr`

The local delta includes merge commits for upstream stable tags. Conflict resolution should treat
the non-merge first-parent commits as the fork-owned behavior and the merge-fix commits after each
stable tag as release-sync repair work.

## Fork-Owned Behavior

### Remote build, validation, and install

- Remote execution is mandatory for Codex Rust builds, tests, code generation, and local standalone
  installs.
- Remote workflows are owned by repository scripts instead of ad hoc `ssh`, `scp`, or local `just`
  invocations.
- The remote host is reached through the local NetworkManager WireGuard connection `wg0`.
- Local standalone install supports diagnose and auto-build flows, defaults to the dev-small
  profile, records timing information, and uses the configured remote build path.
- Local build guard hooks block accidental local Rust build/test/codegen commands while allowing
  approved remote validation commands.

### Skills and tool context

- `use_skill` is a first-party tool and loaded skills are visible in conversation history.
- Skill prompt rendering and injection are fork-owned behavior, including restoration of
  `use_skill` prompt guidance after upstream merges.
- The Codex Rust architecture skill is part of the local repository guidance for internal changes.

### Multi-agent behavior

- Multi-agent tools include inspect/list behavior that identifies the calling agent and supports
  v2 spawn/message flows.
- Subagent identity context, encrypted-message handling, spawn initial task delivery, notification
  tests, and thread id fixtures are fork-owned behavior.
- Side-thread replay and side-stack repro commits were intentionally reverted; do not resurrect
  reverted repro-only behavior during release syncs.

### Goal state and prompts

- Goal prompts are configurable and exposed through the core API.
- Completed goals are cleared or replaced according to local turn-state rules.
- Goal continuation context guidance, paused goal edit tooling, generated goal schemas, and goal
  thread id sync fixes are fork-owned behavior.

### Reset-context and TUI flow

- Reset-context is exposed through app-server protocol types, generated schema, core handlers, and
  TUI commands.
- Reset-context forks live context without unnecessary compaction and remains responsive in the TUI.
- TUI command splitting, remote TUI smoke coverage, source-build snapshot guidance, and state-DB
  resume discovery are fork-owned behavior.

### Context compaction and inspection

- Context usage and context compaction request tools are fork-owned behavior.
- Context anchors can be saved and rewound, with compaction visibility preserved.
- Compact inspect, cancellable history compaction, PostCompact supplements, and compaction phase
  hook input are fork-owned behavior.
- Mid-turn compaction continuation supplements were reverted; do not restore that reverted behavior
  unless a new implementation explicitly replaces it.

### Scoped memories and context reminders

- Session and project scoped memories are fork-owned behavior, including generated files and test
  helper warning cleanup.
- Root context reminders and configurable context reminder messages are fork-owned behavior.
- `ContextReminderConfig` is exported through the core API and used by the thread-manager sample.

### Hooks, approval routing, and auto-review

- PostToolUse output rewrite support, blocking hook behavior, and updated PostToolUse fixtures are
  fork-owned behavior.
- Approval review route hooks, hook schemas, config schemas, analytics, and shell approval route
  tests are fork-owned behavior.
- Auto-review prompt and scope config are fork-owned behavior.

### App-server, schema, and merge repair work

- Local release-sync commits may regenerate app-server schemas, config schemas, Cargo locks, and
  Bazel locks.
- Schema generation must distinguish regular and experimental outputs; experimental generation must
  not overwrite regular fixtures unless the generated regular fixture is expected.
- Release-sync repairs after `rust-v0.141.0` include the app-server `ClientRequest` schema,
  `ResponseItem.metadata` handling, PostToolUse data migration, TUI/core response item fixtures,
  thread-manager sample defaults, remote DotSlash installation, and WireGuard documentation.

## Stable Sync Procedure

1. Verify the worktree is clean and create a backup branch before fetching or merging a stable tag.
2. Fetch the target upstream tag and merge it into `main` with a merge commit.
3. Resolve conflicts by preserving the fork-owned behavior above and accepting upstream changes only
   where they do not remove or silently weaken that behavior.
4. Regenerate affected schemas and lockfiles through the remote scripts.
5. Run focused remote tests for every conflicted subsystem and every fork-owned behavior touched by
   the merge.
6. Run the complete remote test suite for broad stable merges, classify known environment-sensitive
   failures separately, and fix merge regressions before local install.
7. Install the validated standalone build locally when CLI or agent behavior changed.

