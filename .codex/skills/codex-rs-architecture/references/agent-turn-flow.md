# Agent, Thread, Turn, and Context Flow

## Core flow

- Thread lifecycle is centered in `codex-rs/core/src/thread_manager.rs`, `codex_thread.rs`, and `session/`.
- Session runtime and turn handling live under `codex-rs/core/src/session/`. Start with `session/mod.rs`, `session/turn.rs`, and `session/handlers.rs` for user input, turn execution, and event handling.
- Agent control and multi-agent orchestration live under `codex-rs/core/src/agent/`, especially `control.rs` for spawn/send/follow-up/wait/completion behavior.
- Model request construction pulls from context fragments, history, instructions, tools, and config. Changes here can affect prompt caching and must stay bounded.

## User input versus inter-agent messages

- User-originated or initial task content should remain `Op::UserInput` unless the behavior is explicitly an agent-to-agent communication.
- `InterAgentCommunication` serializes as assistant/commentary output text. It is appropriate for `send_message` and agent mail, not for initial user tasks.
- When debugging "child agent got no concrete task", inspect whether initial task content was converted from `UserInput` to assistant/commentary before the child turn.

## Model-visible context

- Context fragments should be structured and bounded. Repo guidance requires injected fragments to be represented under `core/context` when adding new model-visible context.
- Avoid unbounded history or large raw documents in requests. Prefer capped summaries, explicit references, and progressive loading.
- Any new individual context item that can exceed roughly 1k tokens needs manual review; anything above 10k tokens should not be injected as a single item.

## Persistence and history

- Rollout/session reconstruction and truncation live around `thread_manager`, `session`, `context_manager`, and `thread_rollout_truncation`.
- Forked/resumed history is not identical to a fresh user turn. Preserve role, phase, and content semantics when reconstructing model input.
- Tests should assert structured request bodies when possible instead of substring matches over serialized JSON.
