---
name: agent-orchestrator
description: Run a Workflow on the agent-orchestrator engine — invoke the orchestrator binary with a Workflow template file, then read its trace, exit code, and logs. Use when asked to run, drive, or debug any agent-orchestrator Workflow (a `.yaml` template), or to operate the orchestrator engine in general. For the bundled coder example specifically, use the `autocoder` skill.
---

## Agent Orchestrator — run a Workflow

Drive the `agent-orchestrator` engine: run a **Workflow template** (a `.yaml`
file) by invoking the `orchestrator` binary, then read its trace and logs. This
skill is the *engine mechanics* and is Workflow-agnostic. For the bundled
`code → review → commit` coder example, use the **`autocoder`** skill, which
builds on this one.

The full operating reference — Step environment, trace anatomy, exit codes, run
logs, and flags — is in [`./README.md`](./README.md), installed alongside this
file.

### Prerequisites

- `orchestrator` on `PATH` — build and install it with the repo's `./install.sh`
  (`cargo install --path crates/orchestrator`). The binary is a build snapshot;
  re-run `./install.sh` after changing the engine.
- Whatever a Workflow's Steps need on `PATH` (the coder example needs `claude`
  and `cargo`).

### Run a Workflow

Run from the directory the Workflow should operate on — its working directory,
e.g. the repo a Step reads, edits, and commits:

```sh
orchestrator <workflow.yaml> --message "<text>"
# or pipe the Message on stdin:
echo '<text>' | orchestrator <workflow.yaml>
```

`--message` is the entry Step's input (the Workflow's argument); the flag wins
over piped stdin, and with neither the Message is empty.

### Read the outcome

The result is the **trace's final line**, not a shell exit code:

```
◆ RUN ended -> exit <code>
```

Exit codes are **each Workflow's own contract** — the engine surfaces whatever
code the Workflow's Exit Gate declares. `0` is conventionally success; other
codes mean whatever that Workflow documents. `./README.md` explains the rest of
the trace vocabulary (Step/Gate/Frame lines).

### Find the logs

Every Run writes a durable log directory under
`~/.local/state/agent-orchestrator/runs/<run-id>/` (newest sorts last by
UUIDv7): `events.jsonl` (the control trace) plus per-Step `.stderr`/`.stdout`
sidecars. To see *why* a Step failed, read its `.stderr` sidecar. A failed Run
also prints its ephemeral scratch directory (`$RUN_DIR`) to stderr. The full
layout and the `--log-dir` / `--keep` / `-q` / `--max-depth` flags are in
`./README.md`.
