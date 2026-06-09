# Observability as an emitted Event stream (not event sourcing)

The Kernel produces a record of every transition in a Run. Rather than persist it itself, or make the Run's state derive from it, the Kernel **emits** an immutable, ordered stream of Events to subscribed Sinks; what (if anything) becomes of the stream is a Sink's concern.

## Decision

- **Event logging, not event sourcing.** The Kernel routes on ordinary in-memory working state and *additionally* emits an append-only stream of Events (StepEntered, StepExited, GateTaken, FramePushed/Popped, MessagePassed, BudgetConsumed/Exhausted, FaultRaised/Caught, RunStarted/Ended) as a side output. Nothing reads the stream back to drive execution.
- **The Kernel is the sole producer and is storage-agnostic.** It publishes Events through a narrow Sink interface (Observer / pub-sub). It owns the Event *vocabulary* (its observable surface, like a syscall trace) but never opens a file or chooses a format.
- **Sinks are Modules:** persistence, console trace, live monitor, or none. Therefore **durability is a Sink's decision, not a Kernel property** — the Kernel is neither ephemeral nor durable; it emits.
- **The stream is totally ordered** because execution is single-token (ADR-0004): no interleaving to reconcile.

## Consequences

- Observability, audit, and live monitoring are opt-in Sinks layered on one stream; the core stays microkernel-pure (ADR-0003).
- The stream is complete enough to *later* power resumable Runs via replay, but that capability is deliberately out of scope (see rejected).

## Considered and rejected

- **Event sourcing** (state = fold(events); replay to resume — "2b"). Would enable crash-resumability, but commits the Kernel to replay/restore logic and Step re-execution semantics: a Step is an opaque, side-effecting process, so resumption is only Step-boundary-granular and re-running an in-flight Step is non-deterministic for LLM agents. Deferred until long, expensive agent Runs justify the cost.
- **Kernel-owned persistence** (bake ephemeral-vs-durable into the core). Rejected: it couples the Kernel to storage. Making the Kernel *emit* and leaving durability to a Sink keeps the core storage-agnostic.
