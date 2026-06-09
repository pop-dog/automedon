# Fault propagation as structured exception handling over Gates

Routing in a Workflow is control-flow on **exit codes**: a Step's integer exit code unlocks a Gate. But some terminations have *no* exit code the author chose — a Step exits with a code no Gate covers, a Budget is spent with no `EXHAUSTED` Gate, or Depth is exceeded. We call these **Faults**, and we model their propagation as **structured exception handling** layered onto the existing Gate mechanism, rather than encoding them as exit codes.

## Decision

- **A Fault is framework-detected, never author-chosen.** A raw exit code (even a crash, which the OS reports as `128+signal`) is a normal Step outcome and a Gate key. A Fault is the framework's inability to reach an Exit Gate. Three kinds: *unhandled outcome*, *unhandled Exhaustion*, *Depth overflow*.
- **Faults travel out-of-band, not as exit codes.** A faulting Workflow never reached an Exit Gate, so it has no legitimate code in `0–255` to surface. A Fault carries a diagnostic payload (kind, originating Step, unwind trace) used for logging, not routing.
- **The Gate key space gains two non-integer keys.** A Gate key is `integer | * | EXHAUSTED | FAULT`. `EXHAUSTED` is local recovery (taken before a budget-spent Step runs). `FAULT` is the `catch` clause (taken when a child sub-Workflow surfaces a Fault). The Default Gate `*` matches unmatched **integers only** — it never catches `EXHAUSTED` or `FAULT`, so faults are always explicit/opt-in.
- **Unwinding is frame-by-frame.** An uncaught Fault pops its Frame and is presented to the parent Step's `FAULT` Gate; if absent, the parent Frame faults and it bubbles further, up to the Run (which then fails). This is stack unwinding to the nearest handler.
- **Depth overflow is the one non-catchable Fault.** It aborts the Run unconditionally and is never offered to a `FAULT` Gate — it signals a structural mistake, and there is no frame it would be safe to resume in.

## Considered and rejected

- **Reserved exit-code band** (encode faults as integers, e.g. Unix-style `128+N`). Rejected: it pollutes the author's exit-code space (a script could legitimately emit the reserved code), and it erases the graceful-vs-fault distinction — a child that did `EXIT 1` and a child that faulted would be indistinguishable integers.
- **Unconditional propagation to the Run** (any Fault aborts immediately). Rejected for catchable faults: it makes supervisor/retry Workflows impossible — a parent could never recover from a child's internal failure (e.g. "code review failed 3×, so give up *here* and try a different branch"). Retained only for Depth overflow.
- **A bespoke `on_exhausted` Step property** (predating the Gate-key channel). Folded into the `EXHAUSTED` Gate key so there is one routing mechanism, not two.
