# Agent Orchestrator

A framework for defining and executing developer workflows as a directed control-flow graph of executable Steps. The graph is LLM-independent, but is designed so that Steps can invoke LLM agents (e.g. `claude -p ...`).

## Language

**Workflow**:
A directed control-flow graph of Steps connected by Gates. The unit a developer authors and executes.
_Avoid_: State machine (a Step does work; it is not a passive automaton state), pipeline.

**Step**:
A single executable unit within a Workflow — runs a command and terminates with an exit code. May itself be another Workflow (Composite pattern).
_Avoid_: Activity, Node, State, Task, Block, Job, Process, Stage.

**Gate**:
An outgoing exit from a Step — a pair `(key → target)`. The Step's result *unlocks* the matching Gate, selecting the next target. A Gate is either *internal* (target is a successor Step) or an *Exit Gate* (target is termination). Replaces the separate notions of edge and routing-condition. A Gate may carry an optional **`when`** — a kernel-opaque description of what its key means; the Kernel transports it but never routes on it, while Modules (LLM prompt generation), documentation, and observability consume it.
_Avoid_: Transition, edge, guard, case, branch, arrow.

**Gate key**:
What unlocks a Gate — one of: an *integer* exit code, `*` (the Default Gate), `EXHAUSTED`, or `FAULT`. Unique per Step (each key unlocks at most one Gate — the precondition for deterministic routing). Integer/`*` Gates fire *after* a Step runs; the special keys handle conditions where the Step produced no routable exit code (see [[EXHAUSTED Gate]], [[FAULT Gate]]).

**Default Gate**:
A wildcard Gate (`*`) unlocked when a Step's exit code matches no other Gate key. Matches unmatched *integers only* — never the special keys `EXHAUSTED` or `FAULT`. Optional; absent it, an unmapped exit code raises a Fault.
_Avoid_: Else, fallback, catch-all.

**EXHAUSTED Gate**:
A Gate keyed `EXHAUSTED`, taken *instead of* entering a Step whose Budget is spent (so it fires *before* the Step runs). Its target is a successor Step or an Exit Gate — local recovery within the Workflow. Absent, a spent Budget raises a Fault. Replaces the former `on_exhausted` Step property.
_Avoid_: on_exhausted, retry handler.

**FAULT Gate**:
A Gate keyed `FAULT`, taken when a child sub-Workflow Step surfaces a Fault instead of an exit code. Catches the Fault (routing to a handler Step or Exit Gate); absent, the Fault propagates up the Frame stack. The structured-exception `catch` of the model.
_Avoid_: catch, error handler, on_error.

**Exit Gate**:
A terminal kind of Gate whose target is termination rather than a successor Step: it halts the Workflow and declares its exit code. Surface syntax `EXIT <code>`. A Workflow ends only by traversing an Exit Gate — never by absence of outgoing Gates. (Note `EXIT <code>` carries the Workflow exit code, which may differ from the Gate key that unlocked it.)
_Avoid_: Final node, terminal Step, exit node, sink, done/accept state.

**Workflow exit code**:
The exit code declared by the Exit Gate a Workflow traverses. This is the Workflow's external contract, making it substitutable as a Step within a parent Workflow (Composite pattern).
_Avoid_: Return value, result.

**Message**:
The structured value a Step emits on stdout alongside its exit code, piped to the successor Step (the taken Gate's target) as stdin. The data channel, parallel to the exit-code control channel. In the function analogy: a Step's *in-Message* is its arguments, its *out-Message* its return value, and an Exit Gate's out-Message is the Workflow's return value. Small structured values only (JSON by convention) — bulk artifacts belong in the workspace. The Kernel transports a Message opaquely: it moves the bytes but never interprets them. Because execution is single-token, exactly one predecessor delivers a Message, so there is never a merge.
_Avoid_: Actor message, mailbox, event, payload, pipe.

**Entry Step**:
The single start node of a Workflow, where a Frame begins execution. Its in-Message is the Workflow's arguments, supplied by the Run's initiator (empty if none) — the symmetric counterpart to an Exit Gate's out-Message being the Workflow's return value.
_Avoid_: Start state, root, head.

**Name scope**:
Step names are local to their Workflow — each Workflow is a self-contained namespace, so identically-named Steps in different Workflows never collide. A Workflow references another only as a whole (by reference — by name within a multi-Workflow file, or by path), never by reaching into its internal Steps. This makes every sub-Workflow a black box — the precondition for exit-code identity surfacing (a parent sees only the child's exit code, never its internals).
_Avoid_: Global names, qualified Step names.

## Runtime

**Kernel**:
The Workflow engine and *only* the engine: it invokes Steps, reads exit codes, routes through Gates, manages Frames/Budget/Depth, and raises Faults. It is LLM-agnostic and data-agnostic — its entire contract with a Step is "a command that exits with an integer." All LLM, dataflow, and other domain concerns live outside it as opt-in Modules (microkernel architecture).
_Avoid_: Engine, runtime, core, interpreter (acceptable informally), framework (the framework = Kernel + Modules).

**Step Executor**:
The seam between *deciding which Gate* (routing, in the run loop) and *actually running a Step's command*. The Kernel's routing core calls a `StepExecutor` to run one Step and report back its `(exit code, out-Message)`, streaming output to the Sink as it arrives; the production adapter runs the Step as a subprocess (`sh -c`). Isolating execution behind this trait lets the routing core (Budget cascade, Gate precedence, Exhaustion, Faults) be tested with canned outcomes — no shell, no I/O — and keeps the whole Step ABI ("a process that exits with an integer", ADR-0003) as one swappable adapter, leaving a future executor an additive change rather than a Kernel change.
_Avoid_: Runner, driver, invoker, spawner.

**Orchestrator**:
The composition root that assembles and runs a [[Run]] from the [[Kernel]] and
its [[Module]]s — the home of everything the Kernel is deliberately ignorant of.
It parses the Workflow file, constructs the [[Sink]]s, mints the Run's UUIDv7
identity (ADR-0009), establishes the [[Step environment]] (ADR-0010), then runs
the Kernel's routing loop. Realised as the `orchestrator` crate, which builds the
`automedon` binary: the crate keeps the descriptive role name while the product
and binary carry the brand (ADR-0011). Distinct from the [[Kernel]] (which only
routes) and a [[Module]] (an opt-in capability the orchestrator wires in).
_Avoid_: Engine (that is the Kernel), driver, runner, main, app, host.

**Module**:
An opt-in layer built on top of the Kernel, never part of it — e.g. an LLM adapter (Gates→prompt generator, output→integer parser). A Module may read kernel-opaque annotations (like a Gate's `when`) but the Kernel never depends on a Module.
_Avoid_: Plugin, extension, package.

**Run**:
One execution of the root Workflow. Holds a stack of Frames (a call stack). All execution state lives here, never in a Step (which is a pure definition).
_Avoid_: Execution, session, instance.

**Frame**:
The per-invocation context of one active Workflow: its current position (control state), its Steps' remaining Budgets, and its Depth. Pushed when a sub-Workflow Step is entered, popped when that Workflow traverses an Exit Gate (surfacing the exit code to the parent). An activation record.
_Avoid_: Scope, context, stack entry.

**Depth**:
A Frame's nesting level in the Run's Frame stack (root = 0; each sub-Workflow invocation increments it). Bounded by a Run-level max Depth (hardcoded default, configurable) — a recursion/stack-overflow guard. Exceeding it is a hard Run failure (not routed through Exhaustion). Depth bounds stack height; together with Budget (which bounds breadth), it makes every Run total — guaranteed to halt regardless of what Steps do internally.
_Avoid_: Level, nesting, recursion limit.

**Budget**:
A Step property: the maximum number of times that Step may be activated within one Frame. Tracked in the Frame, so it resets on every fresh invocation of the enclosing Workflow. *Every* Step has a Budget, resolved by a cascade: explicit Step value → Workflow-wide default → hardcoded default (10). Because every Step is budgeted, every Frame's total activations are finite — termination within a Frame is guaranteed without graph analysis (an optional cycle-detection lint may later warn when a loop relies only on defaults). Budget bounds breadth; see [[Depth]] for the complementary bound on stack height that together make the Run total.
_Avoid_: Fuel, gas, limit, quota, retries.

**Exhaustion**:
What happens when control would activate a Step whose Budget is spent. The Step does not run; control follows the Step's EXHAUSTED Gate (target: a Step or an Exit Gate). If there is no EXHAUSTED Gate, Exhaustion raises a Fault.
_Avoid_: Timeout, overflow.

**Fault**:
A framework-detected condition that prevents a Workflow from reaching an Exit Gate — never an exit code the author chose. Three kinds: *unhandled outcome* (a Step's exit code matches no Gate), *unhandled Exhaustion* (a spent Budget with no EXHAUSTED Gate), and *Depth overflow*. A Fault carries a diagnostic payload, not a routable exit code. Uncaught, it propagates up the Frame stack (Depth overflow is the exception — it aborts the Run unconditionally).
_Avoid_: Error, exception, crash, panic.

**Event**:
An immutable, never-mutated record of one Kernel transition (StepEntered, StepExited, GateTaken, FramePushed/Popped, MessagePassed, BudgetConsumed/Exhausted, FaultRaised/Caught, RunStarted/Ended). An Event records *what* transition happened, not *when*: timestamping and sequence-numbering are added by a persistence Sink, not intrinsic to an Event (ADR-0005). The Kernel emits Events as a *side output* of execution; routing runs on separate in-memory working state — so this is event *logging*, not event sourcing (nothing reads the stream back to drive execution). Single-token execution (ADR-0004) makes the stream a totally ordered linear sequence.
_Avoid_: Log line, record, Message (a Message is data passed between Steps, not an execution record).

**Sink**:
A Module that consumes the Kernel's Event stream — e.g. a persistence Sink (the only thing that makes a Run durable), a console trace, or a live monitor. The Kernel publishes Events to zero or more Sinks through a narrow interface (Observer pattern) and never persists anything itself: durability is a Sink's choice, not a Kernel property.
_Avoid_: Listener, handler, logger, observer (acceptable informally).

## Workspace

**Workspace**:
The filesystem context a Run operates in — the umbrella over the [[Repository]] and the [[Run Directory]]. It is where bulk artifacts live (the Steps' edits, the task text, review findings), referenced by the small values that ride the Message. The Workspace is *not* a Kernel concept: the Kernel is data- and IO-agnostic (ADR-0003), so the Workspace is a convention the driver and Steps share, not something the engine knows about.
_Avoid_: Sandbox, scratch (names only one region), Cargo workspace (an unrelated build concept).

**Repository**:
The target working tree a Run operates on — the repo whose source the Steps read, edit, build, and commit, and the Run's working directory. Its tracked contents are the deliverable, so orchestration bookkeeping must never land here; that belongs in the [[Run Directory]]. Distinct from where the Workflow's own scripts live (the [[Step environment]]'s `$WORKFLOW_DIR`): a Workflow and the Repository it operates on need not be the same directory.
_Avoid_: Working directory, target, project, codebase.

**Run Directory**:
The engine-provided, per-Run scratch directory — the second region of the [[Workspace]] and the home of a Run's bulk bookkeeping (review findings, build logs), kept out of the [[Repository]] so it never pollutes the deliverable and the Steps need not lean on the target repo's `.gitignore`. *Ephemeral*: it lives under the OS temp directory and is reaped by the OS (e.g. on restart); the engine provides it but promises no cleanup, and nothing retains it. Distinct from the **durable run log** (the persistence [[Sink]]'s Events plus Step-output sidecars, under XDG state and pruned by retention) — that log is observability written *about* a Run, not where Steps operate, so it is not part of the Workspace. The engine hands each Step the Run Directory through the [[Step environment]] as `$RUN_DIR`.
_Avoid_: Scratch dir (informal), temp dir, log dir (that is the separate durable run log).

**Step environment**:
The ambient, read-only context the [[Step Executor]] establishes for every Step before running it. Constant across the whole Run and identical for every Step, so the engine *broadcasts* it rather than passing it Step-to-Step like a [[Message]] — a fourth channel alongside control (the exit code), data (the Message), and output (Step output to the [[Sink]]). Its members are `$WORKFLOW_DIR` (where the Workflow's scripts live) and `$RUN_DIR` (the [[Run Directory]]). The subprocess Executor realises it as environment variables inherited by each `sh -c` child; the [[Kernel]] never sees it (ADR-0003) — it is the Executor adapter's concern, like Run identity is the orchestrator's (ADR-0009).
_Avoid_: Env, globals, config, ambient state.
