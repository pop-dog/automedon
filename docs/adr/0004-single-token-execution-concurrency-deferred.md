# Single-token execution; concurrency deferred but reversible

The Kernel executes a Workflow with a **single token**: within a Frame there is one current Step, Gates route to exactly one successor, and the Run is a stack of Frames (one active path). The system is therefore sequential. This is a deliberate choice, not an oversight — for an *agent orchestrator*, a reader will reasonably ask why it isn't parallel.

## Decision

- **The Kernel stays single-token.** This is what keeps the model simple, total (ADR-0001), deterministic, and analyzable, and what makes "Message fan-in is free" hold (exactly one predecessor transfers control, so there is never a merge to resolve).
- **The Kernel will never run multiple Steps concurrently itself.** That would shatter the single-token model, the totality argument, and the free-merge property.
- **Concurrency, if added, is encapsulated as a Composite Step.** A parallel region is *a Step that internally runs N children concurrently and exits with one aggregate code and one Message*. To the Kernel it is an ordinary Step (the standard Step ABI), so the outer control flow remains single-token and the join is owned inside the parallel Step. Sequential execution is simply the degree-1 case.

## Why this is reversible (not a one-way door)

Because parallelism presents outward as an ordinary Step, it can be added later **additively** — as a fork-join / map-over-list Composite Step or Module — without changing Kernel semantics. Two cheap invariants keep the door open:

1. No Kernel logic may assume a Step's children run sequentially in a way a future parallel region could not satisfy.
2. Workspace scoping must be able to isolate concurrent branches (the Frame-scoped-workspace direction already supports this).

## What is deferred

- **Intra-Step parallelism is available today** with zero framework work: a Step's command may spawn workers and wait. The trade-off is opacity — no per-task Gates, Budgets, Faults, retries, or observability.
- **First-class fork-join** (per-task Steps) is the future additive work. Its hard, self-contained problem is **join semantics**: how N child exit codes collapse to one Gate key (all-success / any-failure / a reduction) and how N Messages merge into one. Deterministic results will require branch independence (no shared mutable state).

## Considered and rejected

- **Kernel-level multi-token execution.** Rejected permanently: it destroys simplicity, totality, determinism, and the free-merge property — the properties the rest of the design depends on.
