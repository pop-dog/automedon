# Roadmap

The Kernel is built as a sequence of **vertical slices** — each one thin but
end-to-end (YAML → IR → run loop → Step ABI → routing → Events → Sink), and each
chosen to exercise the *next* architecturally novel mechanism rather than the
next convenient one. A slice is done when it runs end-to-end from a YAML file and
its new mechanism is covered by tests.

This file is the durable plan. `README.md` holds the *current* status; the
authoritative model lives in [`CONTEXT.md`](../CONTEXT.md) (vocabulary) and
[`docs/adr/`](adr/) (decisions). Where a slice cites a term (Frame, Budget,
Fault, …) it is using it exactly as `CONTEXT.md` defines it.

Status legend: ✅ done · ◻ planned.

---

## Slice 1 — Flat run loop ✅

**Goal.** Prove the totality mechanism (Budget-bounded looping) end-to-end in a
single Frame.

**Exercises.** Subprocess Step ABI (cwd inherited, stdout→stdin Message);
routing over `integer | * | EXHAUSTED | EXIT`; per-Step Budget with the
EXHAUSTED Gate; unhandled-outcome and unhandled-exhaustion Faults that abort the
Run; the Event stream to a console Sink; the IR loaded behind the
`WorkflowSource` interface (YAML first impl, ADR-0007).

**Done.** Implemented in `crates/kernel` + `crates/orchestrator`; the
exhaustion, success, and Fault paths are covered by tests; `examples/loop.yaml`
runs to `EXIT 42`.

---

## Slice 2 — Composite Steps and the Frame stack ◻

**Goal.** Let a Step *be* a sub-Workflow (Composite pattern), turning the single
Frame into a call stack.

**Exercises.**
- A Step body that is either a command *or* a reference to another Workflow — a
  new sum type (`Command` | `Workflow`). Sub-Workflows are referenced as a whole
  (Name scope: a parent never reaches into a child's Steps).
- **Frame push/pop:** entering a Composite Step pushes a Frame; traversing the
  child's Exit Gate pops it and surfaces the child's Workflow exit code.
- **Unified routing:** a Composite Step's surfaced exit code routes through the
  *parent* Step's Gates exactly as a leaf Step's exit code does — preserving
  Composite substitutability (a Workflow is substitutable for a Step because its
  only external contract is an exit code).
- **Depth:** each push increments Depth; a Run-level max Depth cap raises a
  `DepthOverflow` Fault that aborts the Run (not routed through Exhaustion).
- **Budget resets per Frame:** re-invoking a sub-Workflow starts with fresh
  Budgets, since Budget is tracked in the Frame.

**Key design work.** Generalise the run loop from a single mutable Frame into a
stack machine; decide the IR surface for a sub-Workflow reference (by local name
within a multi-Workflow file, or by file path/import); make max Depth
configurable with a hardcoded default.

**Done when.** A Workflow with a Composite Step runs the child to its Exit Gate
and routes the surfaced code in the parent; a deep recursion trips the Depth cap
and aborts; re-invoking a sub-Workflow demonstrably resets its Budgets; tests
cover nesting, exit-code surfacing, Depth overflow, and per-Frame Budget reset.

**Open.** The multi-Workflow authoring shape (one file vs. imports) is undecided
and may deserve its own note once the IR reference form is chosen.

---

## Slice 3 — Faults as structured exceptions ◻

**Goal.** Make Faults catchable and propagating, completing ADR-0002's
structured-exception model. Depends on slice 2's Frame stack.

**Exercises.**
- **FAULT Gate catching:** when a child sub-Workflow surfaces a Fault instead of
  an exit code, the parent Composite Step's `FAULT` Gate catches it, routing to a
  handler Step or an Exit Gate (the `catch` of the model).
- **Propagation / unwinding:** absent a `FAULT` Gate, the Fault unwinds — pop the
  Frame and re-offer the Fault to the next parent, repeating until caught or the
  root is reached (an uncaught Fault aborts the Run).
- **Uncatchable Depth overflow:** `DepthOverflow` continues to abort
  unconditionally, ignoring any `FAULT` Gate.

**Key design work.** Change the run loop's error path from "abort on Fault" to
"propagate up the Frame stack looking for a `FAULT` Gate at each Composite
boundary"; make `UnhandledOutcome` and `UnhandledExhaustion` catchable while
keeping `DepthOverflow` terminal; emit `FaultCaught` Events.

**Done when.** A `FAULT` Gate catches a child's Fault and routes to recovery; an
uncaught Fault unwinds multiple Frames and aborts only at the root; a Depth
overflow is *not* caught by an enclosing `FAULT` Gate; tests cover catch,
multi-Frame unwind, and the uncatchable case.

---

## Slice 4 — The real scenario, and Modules as crates ◻

**Goal.** Run the motivating scenario end-to-end and harden the microkernel
boundary into physical crate separation (ADR-0003).

**Exercises.**
- The **code⇄review loop (Budget 3) nested in a build⇄e2e loop (Budget 3)** —
  cycles, nested resetting Budgets, Composite, and Faults all at once.
- **Sink graduates to its own crate** (e.g. a `sinks` crate with the console
  trace and a durable JSONL Sink — durability is a Sink's choice, ADR-0005).
- **LLM Module scaffolding** as a separate crate: the two pure functions over a
  Step's Gate table — a Gates→prompt generator (using each Gate's optional
  `when`) and an output→integer parser — with malformed output reusing the Fault
  channel. An LLM Step remains an ordinary command Step; the Kernel never learns
  what an LLM is.

**Key design work.** Move `ConsoleSink` out of the bin; define the Module crate
layout; keep the dependency graph one-way (every Module depends on `kernel`,
never the reverse). The LLM call itself can be a real `claude -p …` Step or a
stub — the slice validates the *Module boundary*, not prompt quality.

**Done when.** The nested-loop example runs end-to-end demonstrating every
mechanism; Sinks and the LLM Module live in their own crates; the dependency
arrows still point only at `kernel`.

---

## Cross-cutting work (not yet scheduled)

These are deliberate deferrals, tracked here so they are not forgotten. None
blocks the slices above.

- **Production hardening.** Replace the slice-1 `panic!` paths (missing-Step
  reference, spawn failure) with proper load-time validation and runtime errors;
  consider `deny_unknown_fields` so authoring typos fail loudly; a cycle-detection
  lint that warns when a loop relies only on default Budgets.
- **Authoring surface beyond YAML.** The IR is already behind the
  `WorkflowSource` interface; a code-builder front-end and a shared common IR are
  open (the code-vs-declarative fork from the design phase).
- **Resumability.** The Event stream could later power replay (event sourcing);
  out of scope now (Step-boundary granularity; LLM re-runs are non-deterministic).
- **Concurrency.** Reversible via parallel-as-a-Composite-Step (ADR-0004); the
  hard part is join semantics (N exit codes → 1, N Messages → 1).
- **Production kernel packaging.** The `curl | bash` single-static-binary
  distribution goal (a musl build target) that motivated the Rust choice
  (ADR-0006).
