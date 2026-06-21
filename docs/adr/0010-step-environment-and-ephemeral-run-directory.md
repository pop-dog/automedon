# The Step environment: an ephemeral per-Run directory the engine provides

A Step often needs somewhere to put bulk bookkeeping — review findings, build
logs, future run-level state — that is *not* part of the Repository it operates
on. Today a Step that needs scratch writes into the target repo's working tree
and leans on that repo's `.gitignore` to stay clean: a leaky coupling. The engine
should *provide* that place, and provide it through a channel that is neither the
control signal (the exit code) nor the Message.

## Decision

- **A fourth channel — the Step environment.** Alongside control (exit code,
  Step→Kernel), data (the Message, predecessor→successor), and output (Step→Sink,
  ADR-0009), the engine establishes an ambient, read-only context for every Step
  before it runs. It is constant across the whole Run and identical for every
  Step, so it is *broadcast*, never passed Step-to-Step — there is no predecessor,
  no merge, none of the Message's fan-in reasoning (ADR-0004). Its members are
  `$WORKFLOW_DIR` (where the Workflow's scripts live) and `$RUN_DIR` (the Run
  Directory).
- **The Executor owns the injection, not the Kernel.** The Step environment is
  carried by the `StepExecutor` adapter and applied per-spawn (`SubprocessExecutor`
  layers it onto the inherited env via `Command::envs`). The routing core (`run`)
  and the `execute` trait signature are unchanged, so ADR-0003's boundary holds:
  the *router* stays env-agnostic; the *adapter* — already the home of all
  subprocess plumbing — gains the injection. A future non-subprocess Executor
  receives the same context object and propagates it its own way. This retires the
  previous global `std::env::set_var("WORKFLOW_DIR")`, which made the channel a
  side-effect on the driver's own process state rather than a value the engine
  provides.
- **The Run Directory is ephemeral.** `$RUN_DIR` is
  `<os-temp>/agent-orchestrator/runs/<run-id>/`, derived by the driver from the
  same UUIDv7 it already mints for the durable log (so the two correlate). It is
  reaped by the OS (e.g. on restart); the engine creates it but promises no
  cleanup, and nothing retains it. The OS temp root is `std::env::temp_dir()` —
  the per-user temp on macOS, `/tmp` on Linux — rather than a hardcoded,
  world-writable `/tmp`.
- **The Run Directory is distinct from the durable run log.** The log
  (`events.jsonl` + sidecars, ADR-0009) lives under XDG *state* and is pruned by
  `--keep`; it is observability written *about* a Run. The Run Directory lives
  under *temp* and is scratch the Steps operate *on*. Same run-id, different roots,
  different lifecycles.
- **The engine surfaces the Step environment, so consumers need no discoverability
  logic.** The orchestrator records the populated Step environment **once** at
  startup as run metadata (orchestrator-owned, like Run identity — not a Kernel
  Event, ADR-0003/0009), and prints the Run Directory path when a Run ends in
  failure (a non-zero exit or a Fault). The single startup record serves the
  post-hoc log reader; the failure pointer serves the operator at the terminal at
  the moment of need. An ephemeral Run Directory is therefore discoverable without
  any Step echoing where it wrote — discoverability is an engine property, not each
  Workflow's burden.

## Consequences

- A Workflow's bookkeeping never needs to touch the Repository or its
  `.gitignore`; the Repository stays the pure deliverable.
- `$WORKFLOW_DIR` becomes a documented member of a named channel rather than an
  undocumented global side-effect.
- The driver renames its existing `run_dir` (the durable-log directory handed to
  the file Sink) to `log_dir`, freeing `run_dir` for the new scratch dir — a
  same-name/different-directory trap otherwise.
- A long-lived host accumulates `<temp>/agent-orchestrator/runs/<id>/` directories
  until reboot; accepted as the cost of "no engine cleanup." If buildup bites, the
  existing `retention::prune` can later be pointed at the temp runs dir for a count
  cap — an additive change.
- A Workflow that escalates need not leave a human-facing artifact in the
  Repository: the engine's startup record plus its failure pointer make the
  ephemeral Run Directory discoverable, so a non-converged Run's scratch is findable
  without reintroducing repo writes.

## Considered and rejected

- **`$RUN_DIR` = the durable log dir.** Rejected: it fuses two lifecycles —
  durable, retained observability vs ephemeral scratch — onto one directory, and
  would subject scratch to `--keep` pruning. Splitting them by root (state vs temp)
  is cleaner and makes each lifecycle independently obvious.
- **A second `std::env::set_var` in the driver.** Rejected: it adds another
  instance of the global-mutation pattern, leaves the Step environment a notional
  concept (an implicit global, not a value), is unsafe for two Runs in one process,
  and hands a future Executor nothing to receive.
- **Per-Step or per-task `$RUN_DIR` injected by the engine.** Rejected for now:
  re-injecting a per-activation directory would push env knowledge into the Kernel
  (ADR-0003 forbids) or the routing loop. The engine provides one per-Run
  directory; a consumer that needs per-task isolation (a future fork-join Composite
  Step, ADR-0004) subdivides it (`$RUN_DIR/tasks/task-<n>/`) itself. Per-Run does
  not foreclose per-task.
- **Best-effort cleanup on `Drop`.** Rejected: it would delete a *clean* Run's
  scratch while a *crashed* Run's scratch — the one worth inspecting — survives to
  reboot, which is backwards for debugging. OS-reaping treats both alike.
- **Duplicating the Run Directory path into every escalation or record.** Rejected:
  redundant given the single startup record. One durable record plus a live console
  pointer at Run failure covers both the post-hoc log reader and the operator at the
  terminal, without repeating the env across the log.
