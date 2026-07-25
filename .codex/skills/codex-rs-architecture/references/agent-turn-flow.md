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

- Define typed model-visible fragments in `codex-rs/context-fragments` with
  `ContextualUserFragment`; core registers and assembles them.
- Trace producer → contribution → `ResponseItem` → context history → request assembly before
  changing a limit. The producer owns semantic and collection budgets and measures the final
  rendered fragment, including wrappers and truncation markers.
- Shared tool-output serialization owns generic tool-result truncation. Session and request
  assembly own the full context-window budget. Adapters, persistence, replay, and protocol
  projection preserve already-budgeted content rather than applying another budget.
- Treat 10K tokens as the maximum review boundary for one model-visible item, not as a default
  per-layer budget. Items that can cross 1K tokens require additional manual review.
- Represent large source material through its owning bounded summary, explicit references, or
  progressive loading rather than copying raw documents into requests.

## Persistence and history

- Rollout/session reconstruction and truncation live around `thread_manager`, `session`, `context_manager`, and `thread_rollout_truncation`.
- Forked/resumed history is not identical to a fresh user turn. Preserve role, phase, and content semantics when reconstructing model input.
- Tests should assert structured request bodies when possible instead of substring matches over serialized JSON.
