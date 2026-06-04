# UI and Entrypoints

## CLI and runtime entrypoints

- `codex-rs/cli` owns the `codex` binary and top-level command dispatch.
- `codex-rs/exec` owns non-interactive execution behavior.
- `codex-rs/tui` owns terminal interaction and visible rendering.
- App-server startup and service behavior belong in `app-server` crates, not in TUI.

## TUI ownership

- TUI code is under `codex-rs/tui/src`.
- High-touch orchestration modules include `app.rs`, `chatwidget.rs`, `bottom_pane/mod.rs`, `bottom_pane/chat_composer.rs`, and `bottom_pane/footer.rs`; avoid growing these files for non-trivial new logic.
- Status display, bottom pane behavior, chat widget rendering, event rendering, progress messages, and visible tool status should be changed in TUI modules, not core, unless the underlying event data is wrong.
- Follow `codex-rs/tui/styles.md` and local `ratatui` `Stylize` conventions.

## TUI testing

- User-visible UI changes require `insta` snapshot coverage.
- TUI snapshots are in the `codex-tui` test area. Run the focused `codex-tui` tests remotely after updating snapshots.
- Review generated `*.snap.new` before accepting snapshots.
- Prefer rendering-level assertions for visual behavior and core-level assertions for event payload semantics.

## Event and status debugging

- If the displayed text is wrong but event payloads are correct, inspect TUI rendering.
- If TUI has no data or gets the wrong phase/status, inspect core event emission and protocol types.
- For progress/status prompts, distinguish actual agent state from UI-only presentation; fix the source layer that owns the incorrect fact.
