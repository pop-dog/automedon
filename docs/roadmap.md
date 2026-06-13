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

Status legend: ✅ done · 📋 tracked as an issue · ◻ planned.

> **Note on ordering.** Slices 2–3 (initial Message, then a real agentic-coder
> example) were pulled ahead of Composite/Faults so dogfooding starts early: a
> *flat* `code ⇄ review → commit` Workflow already runs on the Slice 1 kernel and
> only needs an input Message and bespoke LLM Steps. Composite, Faults, and the
> capstone follow.

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

## Slice 2 — Initial Message (Workflow arguments) ✅

> Done — [issue #1](https://github.com/pop-dog/agent-orchestrator/issues/1) (PR #3).

**Goal.** Let a Run be invoked *with* an input Message, so a Workflow can take
arguments. (Today the entry Step always receives an empty Message.)

**Exercises.** The orchestrator reads an initial Message — from a `--message`
flag and/or stdin — and seeds it as the entry Step's in-Message. The run loop
already pipes Messages between Steps; this just supplies the first one. Symmetric
with the existing "an Exit Gate's out-Message is the Workflow's return value":
the entry Step's in-Message is the Workflow's arguments.

**Key design work.** The CLI surface for the initial Message (flag vs. stdin vs.
both); whether an absent Message stays the empty default (yes).

**Done when.** `orchestrator <wf.yaml> --message "<text>"` (or piped stdin)
delivers that text to the entry Step's stdin; a test asserts the entry Step
receives it; omitting it preserves today's empty-Message behaviour.

---

## Slice 3 — Agentic coder example Workflow ✅

> Done — [issue #2](https://github.com/pop-dog/agent-orchestrator/issues/2) (PR #4).
> Each LLM Step runs under a least-privilege scoped permission policy
> (`examples/coder/*.permissions.json` via `--settings`), verified end-to-end.

**Goal.** The flagship example and first dogfood: a Workflow that writes code for
this repo. `code ⇄ review → commit`, all three Steps bespoke LLM agents
(`claude -p …`). Runs on the Slice 1 kernel + Slice 2's initial Message — no
Composite or Faults needed.

**The Workflow.**
- **Task in.** The entry Message is the *path* to a `TASK.md` file; the path
  propagates through all three Steps (each echoes it as its out-Message). The
  small structured value (the path) rides the **Message** channel; the bulk (the
  task text, the code, the findings) lives in the **working directory** — both
  channels, used as `CONTEXT.md` intends.
- **`code`** reads `TASK.md` (and, on loop-back, the review findings file), edits
  files, and runs its own internal build + `cargo test` debug loop until green,
  then exits `0` → `review`. The test/debug loop is *intra-Step* by design (the
  Kernel orchestrates the meaningful checkpoints, not the agent's own
  iteration — ADR-0004's opacity trade-off, accepted here). Leaves changes
  **unstaged**.
- **`review`** is an agent wrapping the `/review` skill. It categorises findings
  as **Blocking** or **Suggestion** and writes them to a shared findings file in
  the working directory. Any Blocking → exit `1` → Gate back to `code`; none →
  exit `0` → Gate to `commit`. Leaves changes unstaged.
- **`commit`** runs `git diff`, reads `TASK.md`, writes a commit message, and
  commits on the **current branch**. It **never pushes** (no LLM commit reaches a
  remote unattended; the human is responsible for being on a sensible branch).
- **Totality.** The kernel-bounded **code ⇄ review** loop carries a Budget. On
  non-convergence (Blocking findings remain when the Budget is spent), the
  EXHAUSTED Gate routes to `EXIT 90` (escalate — see Conventions), leaving the
  unstaged changes and findings for a human. Un-approved code is never committed.

**Bespoke now, Module later.** Each LLM Step is a hand-written script: it builds
its own prompt, runs `claude -p`, and parses the reply into an exit code itself.
This is the deliberate precursor to the LLM Module (Slice 6), which factors out
the two pure functions.

**Key design work.** The bespoke script shape (how each script turns the model's
reply into an exit code, and re-emits the path as its out-Message); the
`TASK.md` format; the findings-file format and location.

**Done when.** Given a `TASK.md`, the Workflow edits the repo, iterates code ⇄
review until approval, and commits on the current branch — or escalates with
`EXIT 90` on non-convergence — all visible on the Event stream.

---

## Slice 4 — Composite Steps and the Frame stack ◻

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
  Composite substitutability.
- **Depth:** each push increments Depth; a Run-level max Depth cap raises a
  `DepthOverflow` Fault that aborts the Run (not routed through Exhaustion).
- **Budget resets per Frame:** re-invoking a sub-Workflow starts with fresh
  Budgets, since Budget is tracked in the Frame.

**Key design work.** Generalise the run loop from a single mutable Frame into a
stack machine; decide the IR surface for a sub-Workflow reference (one
multi-Workflow file vs. path/import); make max Depth configurable.

**Done when.** A Workflow with a Composite Step runs the child to its Exit Gate
and routes the surfaced code in the parent; a deep recursion trips the Depth cap;
re-invoking a sub-Workflow resets its Budgets; tests cover nesting, exit-code
surfacing, Depth overflow, and per-Frame Budget reset.

---

## Slice 5 — Faults as structured exceptions ◻

**Goal.** Make Faults catchable and propagating, completing ADR-0002's
structured-exception model. Depends on Slice 4's Frame stack.

**Exercises.**
- **FAULT Gate catching:** when a child sub-Workflow surfaces a Fault, the parent
  Composite Step's `FAULT` Gate catches it, routing to a handler Step or an Exit
  Gate (the `catch` of the model).
- **Propagation / unwinding:** absent a `FAULT` Gate, the Fault unwinds — pop the
  Frame and re-offer it to the next parent, until caught or the root is reached
  (an uncaught Fault aborts the Run).
- **Uncatchable Depth overflow:** `DepthOverflow` continues to abort
  unconditionally.

**Done when.** A `FAULT` Gate catches a child's Fault and routes to recovery; an
uncaught Fault unwinds multiple Frames and aborts only at the root; a Depth
overflow is *not* caught by an enclosing `FAULT` Gate; tests cover catch,
multi-Frame unwind, and the uncatchable case.

---

## Slice 6 — Capstone: the LLM Module and Modules as crates ◻

**Goal.** Harden the dogfood and the microkernel boundary (ADR-0003) into
physical crate separation, and factor the bespoke LLM Steps into a reusable
Module.

**Exercises.**
- **Extract the LLM Module** as its own crate — the two pure functions over a
  Step's Gate table: a **Gates → prompt** generator (using each Gate's optional
  `when`) and an **output → exit code** parser. It builds a prompt a developer or
  Step feeds to their LLM and maps the reply back to a Gate signal; it does *not*
  call the LLM itself. Malformed output reuses the Fault channel. The
  bespoke Slice 3 scripts collapse onto it.
  - *Open dependency:* the Module needs each Step's own Gate table (keys +
    `when`). Resolve then whether the Kernel injects it via the Step's
    environment (a small, generic, LLM-agnostic ABI extension) or the author
    bakes it into the command args.
- **Sink graduates to its own crate** (console trace + a durable JSONL Sink —
  durability is a Sink's choice, ADR-0005).
- **Upgrade the coder** to the nested form once Composite exists: a code ⇄ review
  inner loop inside a build ⇄ e2e outer loop.

**Done when.** The coder runs on the extracted LLM Module; Sinks and the Module
live in their own crates; the dependency arrows still point only at `kernel`.

---

## Conventions

**Reserved exit-code band `9x` — framework escalation / control.** Example
Workflows reserve the `90–99` range for orchestration outcomes that are neither
ordinary success nor an author's domain failure. The Kernel is oblivious — these
are ordinary integers an Exit Gate declares; the band is a *convention* so
example Workflows read consistently.

- **`90` — escalate to user.** The automated Workflow could not complete and is
  handing off to a human (e.g. a code ⇄ review loop that exhausted its Budget
  without converging). State is left in place (unstaged changes, findings files)
  for the human to inspect.

(Distinct from the Kernel's own `70`, which the orchestrator bin returns when a
**Fault** aborts the Run — a different layer.)

---

## Cross-cutting work (not yet scheduled)

These are deliberate deferrals, tracked here so they are not forgotten. None
blocks the slices above.

- **Production hardening.** Replace the Slice 1 `panic!` paths (missing-Step
  reference, spawn failure) with proper load-time validation and runtime errors;
  consider `deny_unknown_fields` so authoring typos fail loudly; a
  cycle-detection lint that warns when a loop relies only on default Budgets.
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
