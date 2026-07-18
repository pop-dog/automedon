# New capabilities live outside the Kernel

The Kernel does exactly five things: invoke a Step, read its exit code, route
through Gates, manage Frames/Budget/Depth, and raise Faults. Its entire
contract with a Step is the Step ABI — *a process that exits with an integer* —
and it transports all data (Messages, `when` annotations) without interpreting
it. Everything else belongs in one of three homes, and never in the Kernel:

- **A Module** — an opt-in layer the orchestrator wires in (a Sink is the
  archetype). The Kernel never depends on a Module; the Cargo workspace
  enforces this physically, because dependency arrows only ever point *at*
  `kernel`. A change that makes `kernel` depend on another crate of this
  workspace is a design error, not a build problem to solve.
- **Step-side user-space code** — anything that interprets a Step's inputs or
  outputs, LLM interfacing above all. The engine never learns what an LLM is:
  a Step's own command reads the Kernel-owned routing contract
  (`$AUTOMEDON_GATES`) and does its own prompting and parsing. The repo's LLM
  helpers are forkable examples, not a layer the engine provides.
- **An authoring front-end** — a `WorkflowSource` that produces the
  Kernel-owned IR. The Kernel never parses an authoring format; YAML support
  lives in `orchestrator`, and new surfaces (JSON, a code DSL) are additive
  front-ends.

Execution is single-token: one current Step per Frame, one active path per Run.
The Kernel will never run Steps concurrently itself — that would forfeit
totality, determinism, and merge-free Message passing. Concurrency, if it
comes, is a Composite Step that internally fans out and presents one exit code
and one Message through the standard Step ABI. Two invariants keep that door
open: no Kernel logic may assume a Step's children run sequentially in a way a
parallel region could not satisfy, and workspace scoping must be able to
isolate concurrent branches.
