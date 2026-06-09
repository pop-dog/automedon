# Termination via per-Step Budgets and per-Run Depth

A Workflow is a directed control-flow graph with cycles, and its Steps run arbitrary programs — so whether a Run halts is, in general, undecidable. We deliberately constrain the *orchestration layer* to be **total** (guaranteed to terminate) while leaving the Steps themselves Turing-complete. Termination is enforced by two independent bounds on the Run's tree of Frames:

- **Budget (bounds breadth).** Every Step has a Budget — the maximum times it may be activated within one Frame — resolved by a cascade: explicit Step value → Workflow-wide default → hardcoded default (10). Counts live in the Frame, so a Budget resets on every fresh invocation of its Workflow (nested loops get independent, resetting budgets for free). Because every Step is budgeted, each Frame's total activations are finite.
- **Depth (bounds height).** Each Frame has a Depth (root = 0, incremented per sub-Workflow invocation), capped by a Run-level max Depth (hardcoded default, configurable) — a recursion / stack-overflow guard.

Finite branching (Budget) × finite height (Depth) ⇒ a finite Frame tree ⇒ the Run halts.

When a Step's Budget is spent, control follows the Step's `EXHAUSTED` Gate (a Step or an Exit Gate); if there is no `EXHAUSTED` Gate, Exhaustion raises a Fault (see ADR-0002). Depth overflow is always a hard Run failure (like `RecursionError`) and is *not* routable, since it almost always signals a structural mistake.

## Considered and rejected

- **Global Run-level fuel** (one counter for the whole Run). Cannot express nested *resetting* budgets — e.g. an inner code↔review budget that resets each time an outer build↔test loop re-enters.
- **Workflow-scoped budgets with a structured-loop restriction** (only back-edges to a Workflow's Entry Step; inner loops must be nested). Correct but less flexible — it forces authors to nest every non-entry loop (`A → B ⇄ C`) into a sub-Workflow purely to attach a budget. Superseded once we saw a Workflow budget is just a per-Step Budget on the Entry Step.
- **A required static SCC (cycle-detection) check** to prove every loop is bounded. Made unnecessary by default Budgets, which guarantee termination structurally. Demoted to an optional future lint that warns when a cycle relies only on default budgets.
- **Forbidding recursive Workflow composition.** Would have kept the termination proof simple, but the Depth bound subsumes it: recursion is allowed and simply capped, which also catches unexpectedly deep *non-recursive* nesting.
