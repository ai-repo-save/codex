---
name: code-review-context
description: Model visible context
---

Codex maintains a context (history of messages) that is sent to the model in inference requests.

1. No history rewrite - the context must be built up incrementally.
2. Avoid frequent changes to context that cause cache misses.
3. Trace the complete path from the content producer through contribution, `ResponseItem`, context
   history, and request assembly before adding a model-visible limit.
4. Reuse the budget owner for that path:
   - The content producer owns semantic and collection budgets and validates the final rendered
     fragment, including wrappers and truncation markers.
   - Shared tool-output serialization owns generic tool-result truncation.
   - Session and request assembly own the full context-window budget.
5. Adapters, persistence, replay, and protocol projection preserve already-budgeted content. Flag
   any intermediate layer that introduces another limit or re-truncates the payload.
6. Treat 10K tokens as the maximum review boundary for one model-visible item, not as a default
   budget for each layer. A truncation marker counts toward the final rendered item.
7. Highlight new individual items that can cross 1K tokens as P0 for additional manual review.
8. Define injected fragment types in `codex-rs/context-fragments` with
   `ContextualUserFragment`; `codex-core` registers and assembles them.
