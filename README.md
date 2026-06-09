# Agent Orchestrator

A framework for defining and executing developer workflows as a directed
control-flow graph of executable Steps. The graph is LLM-independent; Steps can
invoke anything that exits with an integer (a shell script, a binary, or an LLM
agent). See [`CONTEXT.md`](CONTEXT.md) for the vocabulary and
[`docs/adr/`](docs/adr/) for the architectural decisions.

The Kernel is implemented in **Rust** (ADR-0006).

## Status: slice 1 (tracer bullet)

The first vertical slice is a deliberately scrappy end-to-end path that proves
the novel part of the engine — **Budget-bounded looping (totality)** — wired all
the way from a YAML file through the run loop to a console trace.

What slice 1 does:

- Loads a Workflow from YAML (the first `WorkflowSource`, ADR-0007).
- Runs **one flat Frame** (Depth 0, no nesting yet).
- Invokes Steps as subprocesses (`sh -c`, cwd inherited), piping stdout→stdin as
  the Message.
- Routes the exit code through Gates: `integer | * | EXHAUSTED | EXIT`.
- Enforces a per-Step **Budget** and the **EXHAUSTED** Gate.
- Raises an **unhandled-outcome Fault** that aborts the Run with a diagnostic.
- Emits **Events** to a console Sink.

### Run it

```sh
cargo run -p orchestrator -- examples/loop.yaml ; echo "exit=$?"
```

`examples/loop.yaml` loops a failing Step 3 times (its Budget), then takes the
EXHAUSTED Gate to `EXIT 42` — demonstrating that the engine is total.

## Crate layout

```
crates/kernel/         lib: IR types, WorkflowSource + Sink traits, the run loop. (Modules depend on it, never the reverse.)
crates/orchestrator/   bin: serde_yaml loader + console Sink + main().
examples/              example Workflows.
```

## Testing

```sh
cargo test
```

The run-loop tests (gate routing, the Budget cascade, Exhaustion, Faults,
Message piping) live in `crates/kernel`; the YAML-parsing tests live in
`crates/orchestrator`, keeping the Kernel free of any format dependency.

Coverage uses [`cargo-llvm-cov`](https://github.com/taiki-e/cargo-llvm-cov):

```sh
rustup component add llvm-tools-preview   # one-time
cargo install cargo-llvm-cov              # one-time
cargo llvm-cov                            # summary table
cargo llvm-cov --html --open             # browsable line-by-line report
```

The `kernel` crate (the correctness-critical engine) is the coverage target;
the uncovered remainder is the deliberate `panic!` paths for malformed input and
the console Sink's rendering glue.

## What's next

The Kernel is built as a sequence of thin, end-to-end vertical slices. The next
ones, in order:

- **Slice 2** — Initial Message: invoke a Run with input, so a Workflow takes
  arguments (the entry Step's in-Message).
- **Slice 3** — Agentic coder example: a `code ⇄ review → commit` Workflow of
  bespoke LLM Steps that writes code for this repo (first dogfood).
- **Slice 4** — Composite Steps → the Frame *stack*, Depth cap, Exit Gate
  surfacing the child exit code to the parent.
- **Slice 5** — `FAULT` Gate catching + Fault propagation up the Frame stack;
  Depth-overflow abort.
- **Slice 6** — Capstone: extract the LLM Module, graduate Sinks to their own
  crate, upgrade the coder to the nested code⇄review-in-build⇄e2e form.

See [`docs/roadmap.md`](docs/roadmap.md) for each slice's goal, what it
exercises, and its done-definition, plus the example-Workflow conventions and
cross-cutting deferred work.
