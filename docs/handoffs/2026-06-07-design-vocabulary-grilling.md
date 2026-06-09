# Handoff — Agent Orchestrator: design complete, prototype next

**Date:** 2026-06-07
**Status:** The domain model and vocabulary are essentially complete after a long `/grill-with-docs` session. **Next step: build a throwaway Python prototype of the Kernel.**

## Read these first — they are authoritative, don't re-derive

- **`CONTEXT.md`** — the glossary (23 terms, Structural + Runtime). Source of truth for vocabulary; every term has an `_Avoid_` list of rejected synonyms. Keep it a glossary only.
- **`docs/adr/0001`–`0005`** — the five architectural decisions, each with rejected alternatives and *why*.
- **Project memory `kernel-language-plan`** — why Python is a throwaway prototype and the production language is deferred.

## The model in one screen (mental model only — details are in CONTEXT.md)

A **Workflow** is a directed control-flow graph of **Steps** connected by **Gates**. A Step runs a command and exits with an integer; the exit code unlocks a **Gate** `(key → target)` routing to a successor Step or an **Exit Gate** (`EXIT <code>`, the only way a Workflow ends). A Step may *be* a sub-Workflow (Composite) — substitutable because its only external contract is its exit code.

- **Gate keys:** `integer | * | EXHAUSTED | FAULT`. `*` catches unmatched integers only.
- **Data:** a Step may emit a **Message** (small structured value on stdout) piped to its successor's stdin — args/return-value semantics. Bulk artifacts live in the inherited working directory (the framework is a shell-like propagator; no workspace concept). The Kernel transports Messages opaquely.
- **Termination (ADR-0001):** every Step has a **Budget** (cascade Step→Workflow→10, counted per-**Frame**, resets per invocation) bounding loop breadth; a per-**Run** **Depth** cap bounds recursion. Together they make the orchestrator *total*.
- **Faults (ADR-0002):** framework-detected inability to reach an Exit Gate (unhandled outcome / unhandled Exhaustion / Depth overflow). Out-of-band, caught by a `FAULT` Gate, else unwinds the Frame stack (Depth overflow aborts the Run).
- **Architecture (ADR-0003):** **microkernel.** Kernel = invoke Steps, route Gates, manage Frames/Budget/Depth, raise Faults, emit Events. Everything else (LLM adapter, persistence) is an opt-in **Module**; the Kernel is LLM- and data-agnostic (transports, never interprets).
- **Execution (ADR-0004):** single-token/sequential. Concurrency deferred but reversible (parallel = a Composite Step).
- **Observability (ADR-0005):** Kernel emits an **Event** stream to **Sink** Modules; durability is a Sink's choice (event *logging*, not sourcing).

## LLM angle (settled, lives outside the Kernel)

An LLM Step is an ordinary command Step. A future **LLM Module** provides two pure functions over the Step's Gate table: (1) a prompt generator enumerating the valid exit codes (using each Gate's optional `when` description), (2) a minimal output→integer parser. Malformed output → unmapped code → reuses the Fault channel. The Kernel never learns what an LLM is.

## Next step: the prototype

Build a **throwaway Python Kernel** to validate the execution model (NOT production — see memory). Minimum to exercise the core loop:
- Load a small declarative Workflow (YAML) — a few Steps with Gates.
- Invoke Steps as subprocesses (cwd inherited; capture exit code; pipe stdout→stdin as the Message).
- Route exit code through Gates (`integer | * | EXHAUSTED | FAULT`); manage a Frame stack with Budget (default 10) and Depth.
- Raise/propagate Faults; honor Exit Gates.
- Emit Events to a simple console Sink.
- Good first scenario to model: the **code ⇄ review** loop (Budget 3) nested in **build ⇄ e2e** (Budget 3) — it exercises cycles, nested resetting budgets, Faults, and Composite.

## Suggested skills

- **`prototype`** — this is the explicit next action; use it to build the throwaway Python Kernel and play with the state machine.
- **`grill-with-docs`** — to resume design on the deferred branches below (it keeps CONTEXT.md/ADRs updated inline).

## Deferred / open (all deliberate)

- **Authoring surface** — leaning declarative YAML; the code-vs-declarative fork is open. A *common IR* targeted by both a code API and a YAML parser would preserve both without losing graph-as-data.
- **Resumability (event-sourcing "2b")** — the Event stream could later power replay; out of scope now (Step-boundary granularity; LLM re-runs non-deterministic).
- **Concurrency** — deferred, reversible via parallel-as-Composite-Step; hard part is join semantics (N exit codes → 1, N Messages → 1).
- **Production kernel language** — ADR-0006 pending; leaning compiled single-binary (Rust for correctness / Go for simplicity).

## Process notes (the user values this)

One question at a time, always with a *recommended* answer grounded in named CS paradigms (Composite, control-flow graphs, activation records, structured programming, structured exceptions, microkernel, event sourcing, totality). Challenge loose terms against `CONTEXT.md`'s `_Avoid_` lists. The user pushes back hard and is often right (they overturned the budget design twice, and reduced Workflow-budget to per-Step budget) — concede and find the genuinely missing piece rather than defending.
