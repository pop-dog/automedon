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

## Slice ladder (next)

- **Slice 2** — Composite Steps → the Frame *stack*, Depth cap, Exit Gate
  surfacing the child exit code to the parent.
- **Slice 3** — `FAULT` Gate catching + Fault propagation up the Frame stack;
  Depth-overflow abort.
- **Slice 4** — the full scenario (code⇄review nested in build⇄e2e), with the
  LLM adapter and Sink graduated to their own crates.
