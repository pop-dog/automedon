# Operating the Automedon engine

A self-contained operating reference for the `automedon` skill: how to
run a Workflow, read its trace, interpret exit codes, and find its logs. It
covers the engine mechanics only — it is Workflow-agnostic.

> This is the operating subset of the project's root `README.md`, kept here so it
> ships self-contained with the skill. Keep the two in sync. For architecture,
> the domain glossary, and design decisions, see the repository:
> <https://github.com/pop-dog/automedon>.

## Running a Workflow

A Workflow is a `.yaml` template — a directed graph of Steps the engine executes.
Run it with the installed binary:

```sh
automedon <workflow.yaml> --message "<text>"
echo '<text>' | automedon <workflow.yaml>   # Message piped on stdin
```

- **Working directory.** The engine runs each Step in the current directory and
  leaves it alone, so the cwd is the repo (or tree) the Workflow operates on. A
  Workflow file and the repo it acts on need not be the same place.
- **The Message.** `--message` seeds the entry Step (the Workflow's argument); it
  wins over piped stdin. With neither, the Message is empty. A Step emits its
  out-Message on stdout, piped to its successor's stdin — small structured values
  by convention (JSON), bulk goes in the Run Directory (below).

## The Step environment

Before running a Step, the engine injects an ambient, Run-constant context every
Step inherits, distinct from the Message:

- **`$WORKFLOW_DIR`** — the directory of the Workflow file, so a Step can name its
  scripts (`command: "$WORKFLOW_DIR/build.sh"`) independently of the working
  directory.
- **`$RUN_DIR`** — an ephemeral, per-Run scratch directory under the OS temp dir
  (`<temp>/automedon/runs/<run-id>/`), for bulk bookkeeping a Step must
  keep out of the working repository. It is created before the first Step runs and
  reaped by the OS (no retention), shares its `<run-id>` with the durable log dir,
  is recorded in the log's `meta.json`, and is **printed to stderr when a Run
  fails**.

Design: [ADR-0010](https://github.com/pop-dog/automedon/blob/main/docs/adr/0010-step-environment-and-ephemeral-run-directory.md).

## Reading the trace

The engine streams one line per transition; the relevant control lines:

```
● RUN started
  ▶ enter <step>                       a Step began
  ╆ push <step> -> <workflow>          entered a sub-Workflow (Composite Step)
  ■ exit  <step> -> code <n>           the Step exited with code <n>
  ↳ gate <step> [<key>] -> <target>    routed through a Gate to a successor or EXIT
  ╄ pop  <step> <- <workflow>          a sub-Workflow returned
  ✉ message <from> -> <to> (<n> bytes) the out-Message was piped onward
  ⊘ EXHAUSTED <step>                   the Step's Budget was spent
  ✗ FAULT <...>                        an unhandled outcome / exhaustion / depth overflow
◆ RUN ended -> exit <code>             the Workflow's final exit code
```

The **outcome is the final `◆ RUN ended -> exit <code>` line**, not the shell or
`tee` exit code.

## Exit codes are each Workflow's contract

The engine routes integers and surfaces whatever code a Workflow's Exit Gate
declares; it attaches no global meaning. `0` is conventionally success; any other
code means whatever that Workflow documents. (The coder example, for instance,
uses `90` to escalate a non-converged Run.) A `FAULT` is different — it is a
framework-detected condition (no matching Gate, spent Budget with no handler, or
depth overflow), not an author-chosen code; the driver surfaces it on its own
process status.

## Run logs

A file Sink persists every Run to its own directory under
`$XDG_STATE_HOME/automedon/runs/<run-id>/` (falling back to
`~/.local/state/...`), where `<run-id>` is a time-sortable UUIDv7 (newest sorts
last). Each directory holds:

- **`events.jsonl`** — one JSON record per transition (the Step/Gate trace), each
  stamped with a monotonic `seq` and wall-clock `ts`.
- **`<step>.<activation>.<stream>`** — the raw stdout/stderr a Step produced, one
  sidecar per stream per activation, referenced from `events.jsonl`. To see *why*
  a Step failed, read its `.stderr` sidecar.
- **`meta.json`** — orchestrator-owned Run metadata (currently the Step
  environment, including `$RUN_DIR`), kept out of the Kernel's `events.jsonl`.

The ephemeral `$RUN_DIR` scratch is *separate* from this durable log: same
`<run-id>`, but under the OS temp dir and reaped by the OS, not retained here.

## Flags

| Flag | Env | Effect |
| --- | --- | --- |
| `--message <text>` | — | Seed the entry Step (wins over piped stdin). |
| `--log-dir <dir>` | `AGENT_ORCHESTRATOR_LOG_DIR` | Write Run log directories under `<dir>` instead of the default. |
| `--keep <n>` | `AGENT_ORCHESTRATOR_KEEP` | Retain the newest `n` Runs, pruning oldest first at startup (default 100, minimum 1). |
| `--max-depth <n>` | `AGENT_ORCHESTRATOR_MAX_DEPTH` | Cap the Frame stack depth (recursion guard; minimum 1). |
| `-q`, `--quiet` | — | Suppress the live tee of Step output; the control trace still prints. |
