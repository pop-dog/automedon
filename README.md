# Agent Orchestrator

A framework for defining and executing developer workflows as a directed
control-flow graph of executable Steps — LLM-independent, total, and observable.

![language: Rust](https://img.shields.io/badge/language-Rust-orange)
![edition: 2021](https://img.shields.io/badge/edition-2021-blue)
![built with: cargo](https://img.shields.io/badge/built%20with-cargo-brightgreen)
![status: pre--1.0](https://img.shields.io/badge/status-pre--1.0-yellow)

## Contents

- [Overview](#overview)
- [Key Features](#key-features)
- [Installation / Quick Start](#installation--quick-start)
- [Usage](#usage)
- [Crate layout](#crate-layout)
- [Testing](#testing)
- [Roadmap](#roadmap)
- [Configuration](#configuration)
- [Contributing](#contributing)
- [License](#license)

## Overview

Agent Orchestrator runs developer workflows expressed as a graph of Steps, where
each Step can invoke anything that exits with an integer — a shell script, a
binary, or an LLM agent. The graph is LLM-independent: control flow is driven by
exit codes routed through Gates, not by any model's reasoning. Its defining
property is **totality** — every Run terminates, because every loop is bounded by
a Budget and the engine routes a spent Budget through an explicit `EXHAUSTED`
Gate rather than spinning forever. The Kernel is a microkernel implemented in
Rust ([ADR-0006](docs/adr/0006-kernel-implemented-in-rust.md)); the domain
vocabulary lives in [`CONTEXT.md`](CONTEXT.md) and the architectural decisions in
[`docs/adr/`](docs/adr/).

## Key Features

- **Language-agnostic Steps.** A Step is any subprocess; the engine pipes one
  Step's stdout into the next as the Message and routes its exit code.
- **Budget-bounded looping (totality).** Every loop carries a Budget; a spent
  Budget takes the `EXHAUSTED` Gate, so a Run can never hang.
- **Exit-code routing through Gates.** Outcomes are matched against
  `integer | * | EXHAUSTED | EXIT` Gates to decide the next Step or return value.
- **Workflow arguments and return values.** A Run is seeded with an initial
  Message (`--message` or stdin); an Exit Gate's out-Message is the return value.
- **Faults as diagnostics.** An unhandled outcome raises a Fault that aborts the
  Run with a clear diagnostic rather than failing silently.
- **Observability as an Event stream.** The run loop emits Events to a Sink (a
  console trace today), keeping the engine decoupled from output.
- **Flagship agentic coder.** `examples/coder.yaml` is a runnable `code → review
  → commit` loop of bespoke `claude -p` LLM Steps that writes code for this repo.

## Installation / Quick Start

Requires a [Rust toolchain](https://rustup.rs/) (edition 2021) with `cargo`.

```sh
git clone https://github.com/pop-dog/agent-orchestrator.git
cd agent-orchestrator
cargo build
```

Run the totality demo — a Workflow that loops a failing Step 3 times (its
Budget), then takes the `EXHAUSTED` Gate to `EXIT 42`, proving the engine is
total:

```sh
cargo run -p orchestrator -- examples/loop.yaml ; echo "exit=$?"
```

## Usage

Point the `orchestrator` binary at a Workflow YAML file and, optionally, seed the
entry Step with an initial Message:

```sh
cargo run -p orchestrator -- <workflow.yaml> --message "<text>"
```

The flagship example, [`examples/coder.yaml`](examples/coder.yaml), is an
agentic coder: a flat `code → review → commit` Workflow whose three Steps are
bespoke `claude -p` LLM agents. The entry Message is the path to a `TASK.md`
file; the `code ⇄ review` loop is Budget-bounded, and on non-convergence the
`EXHAUSTED` Gate escalates with `EXIT 90`, leaving the unstaged changes and
findings for a human. Run it from the repo root:

```sh
cargo run -p orchestrator -- examples/coder.yaml --message ./TASK.md
```

Each LLM Step runs its agent unattended, since a Workflow Step is
non-interactive. The narrow `commit` and `review` Steps run under scoped
permission policies (`examples/coder/*.permissions.json`) whose deny rules
enforce the invariants that matter — `commit` never pushes, `review` never edits
crate source or stages a commit — while skipping prompts so the Step does not
hang. The broad `code` Step edits source and drives the `/tdd` skill, so its
toolset is too wide to allowlist; it uses `--dangerously-skip-permissions` and
relies on running against a throwaway branch a human reviews before pushing. The
Steps expect `claude` (and `cargo`, for the build check) on `PATH`.

### Run logs

A file Sink persists every Run to its own directory under
`$XDG_STATE_HOME/agent-orchestrator/runs/<run-id>/` (falling back to
`~/.local/state/...`), where `<run-id>` is a time-sortable UUIDv7. Each directory
holds:

- `events.jsonl` — one JSON record per Kernel transition (the Step/Gate trace),
  each stamped with a Sink-assigned monotonic `seq` and wall-clock `ts`.
- `<step>.<activation>.<stream>` — the raw stdout/stderr a Step produced, one
  sidecar per stream per activation, referenced from `events.jsonl`. To see *why*
  a Step failed, read its `.stderr` sidecar.

This separation of a lean control-plane log from bulk output is
[ADR-0009](docs/adr/0009-step-output-on-a-dedicated-sink-channel.md); the Kernel
emits, the Sink persists ([ADR-0005](docs/adr/0005-observability-as-emitted-event-stream.md)).

Flags (each with an environment fallback):

| Flag | Env | Effect |
| --- | --- | --- |
| `--log-dir <dir>` | `AGENT_ORCHESTRATOR_LOG_DIR` | Write Run directories under `<dir>` instead of the default. |
| `--keep <n>` | `AGENT_ORCHESTRATOR_KEEP` | Retain the newest `n` Runs, pruning oldest first at startup (default 100, minimum 1). |
| `-q`, `--quiet` | — | Suppress the live tee of Step output; the control trace still prints. |

## Crate layout

This is a Cargo workspace (edition 2021). Dependency arrows point only at
`kernel` — it is depended on but never depends back
([ADR-0003](docs/adr/0003-microkernel-architecture.md)).

```text
crates/kernel/         lib: IR types, WorkflowSource + Sink traits, the run loop.
crates/orchestrator/   bin: serde_yaml loader + console Sink + main().
examples/              example Workflows (loop.yaml, coder.yaml).
docs/                  ADRs, conventions, and developer docs.
```

## Testing

```sh
cargo test
```

For line-coverage instructions and what the engine covers, see
[docs/coverage.md](docs/coverage.md).

## Roadmap

The plan and the current status of each vertical slice are tracked as GitHub
issues grouped under the [`v0.1` milestone](https://github.com/pop-dog/agent-orchestrator/milestones).

## Configuration

Workflows are authored as YAML — see the annotated
[`examples/loop.yaml`](examples/loop.yaml) and
[`examples/coder.yaml`](examples/coder.yaml) for the Step, Gate, and Budget
schema. The authoritative vocabulary for every term (Step, Gate, Budget, Frame,
Message, Fault, Sink) is defined in [`CONTEXT.md`](CONTEXT.md).

## Contributing

Contributions are welcome. The project follows Conventional Commits for commit
subjects and targets 60% test coverage; please read [`CONTEXT.md`](CONTEXT.md)
for the domain vocabulary and the [milestones](https://github.com/pop-dog/agent-orchestrator/milestones)
for where the work is headed before opening a pull request. New work lands as thin, end-to-end
vertical slices, each covered by tests.

## License

This project is not yet licensed. No license has been chosen, so all rights are
reserved by the authors pending a license decision; it is published for reference
and not yet offered under open-source terms.
