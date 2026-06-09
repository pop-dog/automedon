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
The single start node of a Workflow, where a Frame begins execution.
_Avoid_: Start state, root, head.

**Name scope**:
Step names are local to their Workflow — each Workflow is a self-contained namespace, so identically-named Steps in different Workflows never collide. A Workflow references another only as a whole (by path/import), never by reaching into its internal Steps. This makes every sub-Workflow a black box — the precondition for exit-code identity surfacing (a parent sees only the child's exit code, never its internals).
_Avoid_: Global names, qualified Step names.

## Runtime

**Kernel**:
The Workflow engine and *only* the engine: it invokes Steps, reads exit codes, routes through Gates, manages Frames/Budget/Depth, and raises Faults. It is LLM-agnostic and data-agnostic — its entire contract with a Step is "a command that exits with an integer." All LLM, dataflow, and other domain concerns live outside it as opt-in Modules (microkernel architecture).
_Avoid_: Engine, runtime, core, interpreter (acceptable informally), framework (the framework = Kernel + Modules).

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
An immutable record of one Kernel transition (StepEntered, StepExited, GateTaken, FramePushed/Popped, MessagePassed, BudgetConsumed/Exhausted, FaultRaised/Caught, RunStarted/Ended), timestamped and never mutated. The Kernel emits Events as a *side output* of execution; routing runs on separate in-memory working state — so this is event *logging*, not event sourcing (nothing reads the stream back to drive execution). Single-token execution (ADR-0004) makes the stream a totally ordered linear sequence.
_Avoid_: Log line, record, Message (a Message is data passed between Steps, not an execution record).

**Sink**:
A Module that consumes the Kernel's Event stream — e.g. a persistence Sink (the only thing that makes a Run durable), a console trace, or a live monitor. The Kernel publishes Events to zero or more Sinks through a narrow interface (Observer pattern) and never persists anything itself: durability is a Sink's choice, not a Kernel property.
_Avoid_: Listener, handler, logger, observer (acceptable informally).
