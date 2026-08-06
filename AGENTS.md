# Rust/codex-rs

## Fork conflict budget (local fork)

This checkout tracks upstream OpenAI Codex and carries local capabilities documented in
`docs/local-fork-delta.md`. Structural rules for reducing stable-sync merge cost live in
`docs/plans/fork-conflict-budget.md`.

- Prefer additive modules under `codex-rs/ext/*`, dedicated session/tool modules, or protocol
  feature modules. Do not grow business logic inside hot files listed in the conflict budget.
- A change that edits a hot file must keep that edit to thin wiring (target: fewer than about
  20 new logic lines per capability) and must explain why an existing extension point is
  insufficient.
- Never hand-merge generated app-server or config JSON schemas; regenerate with remote `just.py`.
- After a `sync/rust-v*` merge, run
  `uv run --project scripts python scripts/fork/conflict_budget.py` and refresh the baseline when
  the budget intentionally moves.

## Context anchor rewind notes

When using `rewind_context_to_anchor`, the note must start by naming the current task that should
continue after the rewind. If the current task changed since the target anchor was saved, the first
sentence must explicitly say that the older task is complete or abandoned and must not be resumed.

- Treat the note as task-control state, not just a summary.
- State any filesystem changes that remain after rewind, because rewinding only changes model
  context and never rolls back files.
- Include the next concrete action after rewind; do not end with a completed-task summary unless
  the task is actually done.
- If the target anchor belongs to an older task, prefer saving a fresh anchor instead of rewinding
  to it.

## Remote build and execution

Local performance is insufficient for routine compile and execution work in this repository.
When a task requires building, running, testing, or generating files from repository code, use
`192.168.50.8` as the execution host.

- The remote execution host is reached through the local WireGuard connection `wg0`. If
  `192.168.50.8` is unreachable or SSH times out, check whether `wg0` is active before treating the
  remote host as down. Use `nmcli connection up wg0` to start it when the NetworkManager connection
  exists, then retry the project remote script.
- Never run `just` on the local machine for this repository. This includes `just fmt`, `just fix`,
  `just test`, `just write-config-schema`, `just bazel-lock-update`, `just bazel-lock-check`,
  `just argument-comment-lint`, and every other `just` recipe. All `just` invocations must run on
  `192.168.50.8`.
- Treat code generation as remote execution. Commands such as `just write-config-schema` compile
  and run repository code and therefore must be executed on `192.168.50.8`, not locally.
- Commit local source changes before remote execution, then push `main` to `origin`.
- Use the project remote scripts for every remote build, test, codegen, install, and smoke
  workflow. Do not hand-write `ssh 192.168.50.8 '... just ...'`, `scp` bundle syncs, or ad hoc
  remote checkout/reset commands. If a needed remote workflow has no script, add or extend a
  `scripts/remote/` script first.
- `uv run --project scripts python scripts/remote/just.py <recipe> [args...]` runs `codex-rs`
  `just` recipes remotely with the shared sync, sccache, and fast-linker setup. For example:
  `uv run --project scripts python scripts/remote/just.py test -p codex-app-server`.
- Remote Rust workflows print sccache statistics before and after the command. Use those
  statistics to verify compiler-cache requests and hits instead of assuming that the configured
  `RUSTC_WRAPPER` is effective. Run
  `uv run --project scripts python scripts/remote/sccache_probe.py` when an isolated two-build
  cache-reuse measurement is required.
- Use `uv run --project scripts python scripts/remote/cleanup_build_cache.py --dry-run` to inspect
  stale incremental generations when the remote target cache exceeds its configured size limit.
  Use `--execute` only after reviewing the reclaimable count and size. The cleanup retains recent
  generations and refuses to run while the shared target lock or a Cargo process is active.
- The configured remote host runs as `root`, has DNS that may rewrite public test domains to local
  addresses, and may have ambient state under `/tmp`. Run full-workspace validation with
  `uv run --project scripts python scripts/remote/just.py --remote-full test`. This mode uses an
  isolated temporary directory, limits nextest to four threads, and prints every test excluded
  because its contract cannot be exercised on that host.
- Treat the `--remote-full` exclusion list as the authority for remote-host incompatibilities.
  Do not change production code or upstream-identical tests solely to make an excluded test pass
  on `192.168.50.8`. Validate an excluded contract in a compatible unprivileged or controlled-DNS
  environment when that contract is relevant to the task.
- Add an exclusion only after an isolated rerun proves the host prerequisite is unavailable and
  the relevant implementation and test still match the upstream baseline. Revalidate and remove
  stale exclusions whenever the upstream baseline or remote host changes.
- `uv run --project scripts python scripts/remote/build_sync.py` performs only the remote
  compile-and-execute smoke test. `uv run --project scripts python
  scripts/remote/install_local_standalone.py` builds a standalone package remotely and installs
  it as the local Codex CLI. `uv run --project scripts python scripts/remote/doctor.py` checks
  remote Git/network/toolchain readiness.
- The remote scripts own `/root/codex` checkout synchronization. Do not manually replace that with
  bundle transfer unless the script has first diagnosed remote Git as unavailable and the fallback
  is added to the script rather than performed by hand.
- Run compile, test, codegen, and execution commands on the remote host, not on the local machine,
  unless the command is a small local inspection that does not meaningfully depend on machine
  performance.
- After a Codex CLI or agent-behavior code change has passed the relevant remote validation and no
  blocking regression remains, install the updated build for local use by default with
  `uv run --project scripts python scripts/remote/install_local_standalone.py`. A code fix that is
  not made available to the local Codex CLI is not a complete handoff. Skip this install step only
  when the user explicitly asks not to install it, the change is not intended to affect the local
  CLI/agent runtime, or validation has not reached a stable state; report the reason when skipping.
- If a remote command changes tracked files or produces artifacts that must be kept in the local
  checkout, copy those files back to `/home/bluebird/git/codex` after the remote command finishes,
  then inspect the local diff before continuing.
- Do not leave remote-only source changes in `/root/codex`. The local checkout remains the source
  of truth for editing and commits.

In the codex-rs folder where the rust code lives:

- Crate names are prefixed with `codex-`. For example, the `core` folder's crate is named `codex-core`
- When using format! and you can inline variables into {}, always do that.
- Install any commands the repo relies on (for example `just`, `rg`, or `cargo-insta`) if they aren't already available before running instructions here.
- Never add or modify any code related to `CODEX_SANDBOX_NETWORK_DISABLED_ENV_VAR` or `CODEX_SANDBOX_ENV_VAR`.
  - You operate in a sandbox where `CODEX_SANDBOX_NETWORK_DISABLED=1` will be set whenever you use the `shell` tool. Any existing code that uses `CODEX_SANDBOX_NETWORK_DISABLED_ENV_VAR` was authored with this fact in mind. It is often used to early exit out of tests that the author knew you would not be able to run given your sandbox limitations.
  - Similarly, when you spawn a process using Seatbelt (`/usr/bin/sandbox-exec`), `CODEX_SANDBOX=seatbelt` will be set on the child process. Integration tests that want to run Seatbelt themselves cannot be run under Seatbelt, so checks for `CODEX_SANDBOX=seatbelt` are also often used to early exit out of tests, as appropriate.
- Always collapse if statements per https://rust-lang.github.io/rust-clippy/master/index.html#collapsible_if
- Always inline format! args when possible per https://rust-lang.github.io/rust-clippy/master/index.html#uninlined_format_args
- Use method references over closures when possible per https://rust-lang.github.io/rust-clippy/master/index.html#redundant_closure_for_method_calls
- Avoid bool or ambiguous `Option` parameters that force callers to write hard-to-read code such as `foo(false)` or `bar(None)`. Prefer enums, named methods, newtypes, or other idiomatic Rust API shapes when they keep the callsite self-documenting.
- When you cannot make that API change and still need a small positional-literal callsite in Rust, follow the `argument_comment_lint` convention:
  - Use an exact `/*param_name*/` comment before opaque literal arguments such as `None`, booleans, and numeric literals when passing them by position.
  - A method's sole non-self argument is exempt when the method and parameter names match, such as `.enabled(false)` for `fn enabled(&self, enabled: bool)`.
  - Do not add these comments for string or char literals unless the comment adds real clarity; those literals are intentionally exempt from the lint.
  - The parameter name in the comment must exactly match the callee signature.
  - Run `uv run --project scripts python scripts/remote/just.py argument-comment-lint` only through the remote script. This is powered by Bazel, so running it the first time can be slow if Bazel is not warmed up, though incremental invocations should take <15s. Most of the time, it is best to update the PR and let CI take responsibility for checking this (or run it asynchronously in the background after submitting the PR). Note CI checks all three platforms, which the remote run does not.
- When possible, make `match` statements exhaustive and avoid wildcard arms.
- Newly added traits should include doc comments that explain their role and how implementations are expected to use them.
- Discourage both `#[async_trait]` and `#[allow(async_fn_in_trait)]` in Rust traits.
  - Prefer native RPITIT trait methods with explicit `Send` bounds on the returned future, as in `3c7f013f9735` / `#16630`.
  - Preferred trait shape:
    `fn foo(&self, ...) -> impl std::future::Future<Output = T> + Send;`
  - Implementations may still use `async fn foo(&self, ...) -> T` when they satisfy that contract.
  - Do not use `#[allow(async_fn_in_trait)]` as a shortcut around spelling the future contract explicitly.
- When writing tests, prefer comparing the equality of entire objects over fields one by one.
- Do not add tests for values that are statically defined.
- Do not add negative tests for logic that was removed.
- Do not add general product or user-facing documentation to the `docs/` folder. The official Codex documentation lives elsewhere. The exception is app-server API documentation, which is covered by the app-server guidance below.
- Prefer private modules and explicitly exported public crate API.
- If you change `ConfigToml` or nested config types, run `uv run --project scripts python scripts/remote/just.py write-config-schema` to update `codex-rs/core/config.schema.json`.
- When working with MCP tool calls, prefer using `codex-rs/codex-mcp/src/mcp_connection_manager.rs` to handle mutation of tools and tool calls. Aim to minimize the footprint of changes and leverage existing abstractions rather than plumbing code through multiple levels of function calls.
- Do not call `reset_client_session` unnecessarily; let the incremental check logic decide whether to reuse the previous request.
- If you change Rust dependencies (`Cargo.toml` or `Cargo.lock`), run `uv run --project scripts python scripts/remote/just.py bazel-lock-update` to refresh `MODULE.bazel.lock`, and include that lockfile update in the same change.
- After dependency changes, run `uv run --project scripts python scripts/remote/just.py bazel-lock-check` so lockfile drift is caught before CI.
- Bazel does not automatically make source-tree files available to compile-time Rust file access. If
  you add `include_str!`, `include_bytes!`, `sqlx::migrate!`, or similar build-time file or
  directory reads, update the crate's `BUILD.bazel` (`compile_data`, `build_script_data`, or test
  data) or Bazel may fail even when Cargo passes.
- Do not create small helper methods that are referenced only once.
- For tracing async work, instrument the function or method definition with
  `#[tracing::instrument(...)]` instead of attaching spans to futures with
  `.instrument(...)` at call sites. Before adding instrumentation, check whether the callee—or
  the implementation method it immediately delegates to—is already instrumented.
- Avoid large modules:
  - Prefer adding new modules instead of growing existing ones.
  - Target Rust modules under 500 LoC, excluding tests.
  - If a file exceeds roughly 800 LoC, add new functionality in a new module instead of extending
    the existing file unless there is a strong documented reason not to.
  - This rule applies especially to high-touch files that already attract unrelated changes, such
    as `codex-rs/tui/src/app.rs`, `codex-rs/tui/src/bottom_pane/chat_composer.rs`,
    `codex-rs/tui/src/bottom_pane/footer.rs`, `codex-rs/tui/src/chatwidget.rs`,
    `codex-rs/tui/src/bottom_pane/mod.rs`, and similarly central orchestration modules.
  - When extracting code from a large module, move the related tests and module/type docs toward
    the new implementation so the invariants stay close to the code that owns them.
  - Avoid adding new standalone methods to `codex-rs/tui/src/chatwidget.rs` unless the change is
    trivial; prefer new modules/files and keep `chatwidget.rs` focused on orchestration.
- When running Rust commands through the remote scripts (e.g. `just.py fix` or `just.py test`),
  prefer graceful cancellation first and avoid killing a healthy command just because Rust compile
  or lock contention is slow. PID/process-group cleanup is appropriate when the command was started
  by mistake, exceeds the intended validation scope, has already been interrupted by the user, is
  blocking newer work, or has left stale remote processes. After cleanup, check for residual
  `cargo`/`rustc`/`nextest` processes and `.git/index.lock` before starting another remote workflow.

Run `uv run --project scripts python scripts/remote/just.py fmt` automatically after you have finished making code changes anywhere in this repository; do not ask for approval to run it. Additionally, run the tests on the remote execution host through the remote scripts:

1. Do not run `cargo test` directly. Use `uv run --project scripts python scripts/remote/just.py test` so test execution follows the repo defaults.
2. Run a test filter that matches the behavior changed. For example, for a TUI slash-command change, run a focused `codex-tui` filter such as `uv run --project scripts python scripts/remote/just.py test -p codex-tui chatwidget::tests::slash_commands`.
   Use the `test-diagnostic` recipe instead of `test` when reproducing a deterministic failure
   where retrying would only duplicate the result. Normal validation keeps the configured retry.
3. For TUI compile/RPC smoke only, use `uv run --project scripts python scripts/remote/tui_smoke.py`. This is not a substitute for behavior-specific tests; it only verifies that the remote TUI test graph still builds and one app-server/TUI RPC path runs.
4. Do not use unfiltered `uv run --project scripts python scripts/remote/just.py test -p codex-tui` as a routine development check. It runs the entire `codex-tui` crate, including platform-sensitive snapshots, and is substantially broader than a behavior-specific test. Version-bearing snapshots use stable fixtures.
5. Once focused tests pass, run the complete test suite with `uv run --project scripts python scripts/remote/just.py --remote-full test` only when the change is broad enough that focused coverage does not bound the risk, when a shared common/core/protocol change affects many independent call paths, or when the user/PR explicitly requires full validation. This configured-host suite reports and excludes only the tests whose root, DNS, or temporary-directory prerequisites are unavailable there. The remaining workspace suite includes all snapshot tests. Do not use full-workspace tests as routine finalization for a narrow behavior change that already has focused coverage. Avoid `--all-features` for routine local runs because it expands the build matrix and can significantly increase `target/` disk usage; use it only when you specifically need full feature coverage.

Before finalizing a large change to `codex-rs`, run `uv run --project scripts python scripts/remote/just.py fix -p <project>` only for crates whose handwritten Rust source changed. Do not include crates merely because they are transitively affected. The default `fix` scope is production library and binary targets; pass `--tests`, `--test <name>`, or another Cargo target selector only when that target's source changed. Do not run `uv run --project scripts python scripts/remote/just.py fix -p codex-core` as a routine finalization step: the crate's production graph alone can spend many minutes in a single `clippy-driver` process and has poor signal-to-cost for ordinary changes. For `codex-core`, rely on focused remote tests, formatting, schema/codegen checks when relevant, and CI for full Clippy coverage unless the user explicitly asks to run the slow fix. Do not re-run tests after running `fix` or `fmt`.

## The `codex-core` crate

Over time, the `codex-core` crate (defined in `codex-rs/core/`) has become bloated because it is the largest crate, so it is often easier to add something new to `codex-core` rather than refactor out the library code you need so your new code neither takes a dependency on, nor contributes to the size of, `codex-core`.

To that end: **resist adding code to codex-core**!

Particularly when introducing a new concept/feature/API, before adding to `codex-core`, consider whether:

- There is an existing crate other than `codex-core` that is an appropriate place for your new code to live.
- It is time to introduce a new crate to the Cargo workspace for your new functionality. Refactor existing code as necessary to make this happen.

Likewise, when reviewing code, do not hesitate to push back on PRs that would unnecessarily add code to `codex-core`.

## Code Review Rules

### Crate API surface

Keep crate API surfaces as small as possible. Avoid proliferating test-only helpers.

### Model visible context

Codex maintains a context (history of messages) that is sent to the model in inference requests.

1. No history rewrite - the context must be built up incrementally.
2. Avoid frequent changes to context that cause cache misses.
3. Before adding a model-visible limit, trace the complete path from the content producer through
   contribution, `ResponseItem`, context history, and request assembly, then reuse the existing
   budget owner for that path.
4. The content producer owns semantic and collection budgets and validates the final rendered
   fragment, including wrappers and truncation markers. Shared tool-output serialization owns
   generic tool-result truncation. Session and request assembly own the full context-window budget.
5. Adapters, persistence, replay, and protocol projection preserve already-budgeted content. They
   do not introduce another limit or re-truncate it.
6. The 10K-token rule is the maximum review boundary for one model-visible item, not a default
   budget to recreate at every layer. A truncation marker is part of the measured final item.
7. Highlight new individual items that can cross >1K tokens as P0. These need an additional manual
   review.
8. Injected fragment types live in `codex-rs/context-fragments` and implement
   `ContextualUserFragment`; `codex-core` registers and assembles them.

### Breaking changes

Search for breaking changes in external integration surfaces:

- app-server APIs
- raw response item events (`rawResponseItem/*`), even while experimental
- CLI parameters
- configuration loading
- resuming sessions from existing rollouts

### Test authoring guidance

For agent changes prefer integration tests over unit tests. Integration tests are under `core/suite` and use `test_codex` to set up a test instance of codex.

Features that change the agent logic MUST add an integration test:

- Provide a list of major logic changes and user-facing behaviors that need to be tested.

If unit tests are needed, put them in a dedicated test file (\*\_tests.rs).
Avoid test-only functions in the main implementation.

Check whether there are existing helpers to make tests more streamlined and readable.

### Change size guidance (800 lines)

Unless the change is mechanical the total number of changed lines should not exceed 800 lines.
For complex logic changes the size should be under 500 lines.

If the change is larger, explore whether it can be split into reviewable stages and identify the smallest coherent stage to land first.
Base the staging suggestion on the actual diff, dependencies, and affected call sites.

## TUI style conventions

See `codex-rs/tui/styles.md`.

## TUI code conventions

- Use concise styling helpers from ratatui’s Stylize trait.
  - Basic spans: use "text".into()
  - Styled spans: use "text".red(), "text".green(), "text".magenta(), "text".dim(), etc.
  - Prefer these over constructing styles with `Span::styled` and `Style` directly.
  - Example: patch summary file lines
    - Desired: vec!["  └ ".into(), "M".red(), " ".dim(), "tui/src/app.rs".dim()]

### TUI Styling (ratatui)

- Prefer Stylize helpers: use "text".dim(), .bold(), .cyan(), .italic(), .underlined() instead of manual Style where possible.
- Prefer simple conversions: use "text".into() for spans and vec![…].into() for lines; when inference is ambiguous (e.g., Paragraph::new/Cell::from), use Line::from(spans) or Span::from(text).
- Computed styles: if the Style is computed at runtime, using `Span::styled` is OK (`Span::from(text).set_style(style)` is also acceptable).
- Avoid hardcoded white: do not use `.white()`; prefer the default foreground (no color).
- Chaining: combine helpers by chaining for readability (e.g., url.cyan().underlined()).
- Single items: prefer "text".into(); use Line::from(text) or Span::from(text) only when the target type isn’t obvious from context, or when using .into() would require extra type annotations.
- Building lines: use vec![…].into() to construct a Line when the target type is obvious and no extra type annotations are needed; otherwise use Line::from(vec![…]).
- Avoid churn: don’t refactor between equivalent forms (Span::styled ↔ set_style, Line::from ↔ .into()) without a clear readability or functional gain; follow file‑local conventions and do not introduce type annotations solely to satisfy .into().
- Compactness: prefer the form that stays on one line after rustfmt; if only one of Line::from(vec![…]) or vec![…].into() avoids wrapping, choose that. If both wrap, pick the one with fewer wrapped lines.

### Text wrapping

- Always use textwrap::wrap to wrap plain strings.
- If you have a ratatui Line and you want to wrap it, use the helpers in tui/src/wrapping.rs, e.g. word_wrap_lines / word_wrap_line.
- If you need to indent wrapped lines, use the initial_indent / subsequent_indent options from RtOptions if you can, rather than writing custom logic.
- If you have a list of lines and you need to prefix them all with some prefix (optionally different on the first vs subsequent lines), use the `prefix_lines` helper from line_utils.

## Tests

### Test module organization

- When adding a new test module, define its contents in a separate sibling file rather than inline in the implementation file.
- Use an explicit `#[path = "..._tests.rs"]` attribute so the test filename is descriptive and easy to locate:

  ```rust
  #[cfg(test)]
  #[path = "parser_tests.rs"]
  mod tests;
  ```

- This applies only when introducing a new test module. Do not move or rewrite existing inline `#[cfg(test)] mod tests { ... }` modules solely to follow this convention.

### Snapshot tests

This repo uses snapshot tests (via `insta`), especially in `codex-rs/tui`, to validate rendered output.

**Requirement:** any change that affects user-visible UI (including adding new UI) must include
corresponding `insta` snapshot coverage (add a new snapshot test if one doesn't exist yet, or
update the existing snapshot). Review and accept snapshot updates as part of the PR so UI impact
is easy to review and future diffs stay visual.

When UI or text output changes intentionally, update the snapshots as follows:

- Run the specific snapshot test or narrow snapshot family that covers the changed surface:
  - `uv run --project scripts python scripts/remote/just.py test -p codex-tui <snapshot-test-filter>`
- Check what’s pending:
  - `cargo insta pending-snapshots -p codex-tui`
- Review changes by reading the generated `*.snap.new` files directly in the repo, or preview a specific file:
  - `cargo insta show -p codex-tui path/to/file.snap.new`
- Only if you intend to accept all new snapshots in this crate, run:
  - `cargo insta accept -p codex-tui`

Version-bearing snapshots use stable fixtures. Run the unfiltered `codex-tui` crate only when complete crate coverage is needed: it includes the full platform-sensitive snapshot set. Snapshot updates should be intentional and scoped to the UI surface being changed.

If you don’t have the tool:

- `cargo install --locked cargo-insta`

### Benchmarks

cargo benchmarks can be run with `just bench`, use the divan crate to write new ones.

Use `just bench-smoke` to dry-run the benchmark for a single iteration to ensure it works.

### Test assertions

- Tests should use pretty_assertions::assert_eq for clearer diffs. Import this at the top of the test module if it isn't already.
- Prefer deep equals comparisons whenever possible. Perform `assert_eq!()` on entire objects, rather than individual fields.
- Avoid mutating process environment in tests; prefer passing environment-derived flags or dependencies from above.

### Spawning workspace binaries in tests (Cargo vs Bazel)

- Prefer `codex_utils_cargo_bin::cargo_bin("...")` over `assert_cmd::Command::cargo_bin(...)` or `escargot` when tests need to spawn first-party binaries.
  - Under Bazel, binaries and resources may live under runfiles; use `codex_utils_cargo_bin::cargo_bin` to resolve absolute paths that remain stable after `chdir`.
- When locating fixture files or test resources under Bazel, avoid `env!("CARGO_MANIFEST_DIR")`. Prefer `codex_utils_cargo_bin::find_resource!` so paths resolve correctly under both Cargo and Bazel runfiles.

### Integration tests (core)

- Prefer the utilities in `core_test_support::responses` when writing end-to-end Codex tests.

- All `mount_sse*` helpers return a `ResponseMock`; hold onto it so you can assert against outbound `/responses` POST bodies.
- Use `ResponseMock::single_request()` when a test should only issue one POST, or `ResponseMock::requests()` to inspect every captured `ResponsesRequest`.
- `ResponsesRequest` exposes helpers (`body_json`, `input`, `function_call_output`, `custom_tool_call_output`, `call_output`, `header`, `path`, `query_param`) so assertions can target structured payloads instead of manual JSON digging.
- Build SSE payloads with the provided `ev_*` constructors and the `sse(...)`.
- Prefer `wait_for_event` over `wait_for_event_with_timeout`.
- Prefer `mount_sse_once` over `mount_sse_once_match` or `mount_sse_sequence`

- Typical pattern:

  ```rust
  let mock = responses::mount_sse_once(&server, responses::sse(vec![
      responses::ev_response_created("resp-1"),
      responses::ev_function_call(call_id, "shell", &serde_json::to_string(&args)?),
      responses::ev_completed("resp-1"),
  ])).await;

  codex.submit(Op::UserTurn { ... }).await?;

  // Assert request body if needed.
  let request = mock.single_request();
  // assert using request.function_call_output(call_id) or request.json_body() or other helpers.
  ```

#### app-server integration testing

- Tests should exercise app-server's public JSON-RPC API.
- Use similar server mocking as for core integration tests.
- Use `TestAppServer::builder().build()` and `TestAppServer::send_thread_start_request_with_auto_env()`
  by default to ensure that new tests work with foreign app/exec OSes. See `$remote-tests` for
  details.

## App-server API Development Best Practices

These guidelines apply to app-server protocol work in `codex-rs`, especially:

- `app-server-protocol/src/protocol/common.rs`
- `app-server-protocol/src/protocol/v2.rs`
- `app-server/README.md`

### Core Rules

- All active API development should happen in app-server v2. Do not add new API surface area to v1.
- Follow payload naming consistently:
  `*Params` for request payloads, `*Response` for responses, and `*Notification` for notifications.
- Expose RPC methods as `<resource>/<method>` and keep `<resource>` singular (for example, `thread/read`, `app/list`).
- Always expose fields as camelCase on the wire with `#[serde(rename_all = "camelCase")]` unless a tagged union or explicit compatibility requirement needs a targeted rename.
- Always expose string enum values as camelCase on the wire with matching serde and TS `rename_all = "camelCase"` annotations unless an explicit compatibility requirement needs targeted renames.
- Exception: config RPC payloads are expected to use snake_case to mirror config.toml keys (see the config read/write/list APIs in `app-server-protocol/src/protocol/v2.rs`).
- Always set `#[ts(export_to = "v2/")]` on v2 request/response/notification types so generated TypeScript lands in the correct namespace.
- Never use `#[serde(skip_serializing_if = "Option::is_none")]` for v2 API payload fields.
  Exception: client->server requests that intentionally have no params may use:
  `params: #[ts(type = "undefined")] #[serde(skip_serializing_if = "Option::is_none")] Option<()>`.
- Keep Rust and TS wire renames aligned. If a field or variant uses `#[serde(rename = "...")]`, add matching `#[ts(rename = "...")]`.
- For discriminated unions, use explicit tagging in both serializers:
  `#[serde(tag = "type", ...)]` and `#[ts(tag = "type", ...)]`.
- Prefer plain `String` IDs at the API boundary (do UUID parsing/conversion internally if needed).
- Timestamps should be integer Unix seconds (`i64`) and named `*_at` (for example, `created_at`, `updated_at`, `resets_at`).
- For experimental API surface area:
  use `#[experimental("method/or/field")]`, derive `ExperimentalApi` when field-level gating is needed, and use `inspect_params: true` in `common.rs` when only some fields of a method are experimental.

### Client->server request payloads (`*Params`)

- Every optional field must be annotated with `#[ts(optional = nullable)]`. Do not use `#[ts(optional = nullable)]` outside client->server request payloads (`*Params`).
- Optional collection fields (for example `Vec`, `HashMap`) must use `Option<...>` + `#[ts(optional = nullable)]`. Do not use `#[serde(default)]` to model optional collections, and do not use `skip_serializing_if` on v2 payload fields.
- When you want omission to mean `false` for boolean fields, use `#[serde(default, skip_serializing_if = "std::ops::Not::not")] pub field: bool` over `Option<bool>`.
- For new list methods, implement cursor pagination by default:
  request fields `pub cursor: Option<String>` and `pub limit: Option<u32>`,
  response fields `pub data: Vec<...>` and `pub next_cursor: Option<String>`.

### Development Workflow

- Update app-server docs/examples when API behavior changes (at minimum `app-server/README.md`).
- Regenerate schema fixtures when API shapes change:
  `uv run --project scripts python scripts/remote/just.py write-app-server-schema`
  (and `uv run --project scripts python scripts/remote/just.py write-app-server-schema --experimental` when experimental API fixtures are affected).
- Validate with `uv run --project scripts python scripts/remote/just.py test -p codex-app-server-protocol`.
- Avoid boilerplate tests that only assert experimental field markers for individual
  request fields in `common.rs`; rely on schema generation/tests and behavioral coverage instead.

## Python Development Best Practices

### Ignore Python 2 compatibility

This project uses Python 3+. You should not use the `__future__` module.

If you need to worry about feature compatibility between different 3.xx point releases, check the
closest `pyproject.toml`'s `requires-python` field to see what minimum runtime version is supported.

## Platform Support

Tests and features must support Linux, macOS and Windows unless feature is explicitly OS-specific.
