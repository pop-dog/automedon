# Conventions

## Reserved exit-code band `9x` — framework escalation / control

Example Workflows reserve the `90–99` range for orchestration outcomes that are
neither ordinary success nor an author's domain failure. The Kernel is oblivious
— these are ordinary integers an Exit Gate declares; the band is a *convention*
so example Workflows read consistently.

- **`90` — escalate to user.** The automated Workflow could not complete and is
  handing off to a human (e.g. a code ⇄ review loop that exhausted its Budget
  without converging). State is left in place (unstaged changes, findings files)
  for the human to inspect.

(Distinct from the Kernel's own `70`, which the orchestrator bin returns when a
**Fault** aborts the Run — a different layer.)
