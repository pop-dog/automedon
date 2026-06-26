---
name: autocoder
description: Implement a coding task in the agent-orchestrator project by driving its bundled coder Workflow (code → review → commit) instead of editing the repo directly. You write a TASK.md; the Workflow's agents write the code. Use when asked to code or fix an issue in the agent-orchestrator project via the orchestrator.
---

## Autocoder — code a task with the coder Workflow

Implement a task by running the bundled `code → review → commit` coder Workflow.
You write a `TASK.md`; the Workflow's agents do the coding. This skill builds on
the **`agent-orchestrator`** skill (the engine mechanics — invocation, traces,
exit codes, logs); activate that skill for anything about running the binary or
reading a Run.

**Do not edit the project's source yourself.** Your only output is a `TASK.md`.
Every code change comes from the orchestrator's agents.

### Prerequisites

- `automedon` on `PATH`, plus `claude` and `cargo` — the coder's Steps drive a
  `claude` agent and run `cargo build && cargo test`.
- **Re-install after engine changes.** The `automedon` binary is a build
  snapshot. Because this Workflow can modify the engine itself (the
  `orchestrator`/`kernel` crates), re-run the repo's `scripts/dev-install.sh`
  whenever the engine changes, so the binary you run reflects them. (The repo's
  `install.sh` downloads a prebuilt release and would not pick up your changes.)

### Process

Run from the root of the agent-orchestrator checkout — its working directory is
the repo the coder reads, edits, and commits.

1. **Read the issue:** `gh issue view <number>`.
2. **Branch** — the `commit` Step commits on the current branch, so never run on
   `main`: `git checkout -b feat/<slug>`.
3. **Write the task** at `.workflows/<slug>/TASK.md` (kebab-case `<slug>`, e.g.
   `14-fix-prune-race`). `.workflows/` is gitignored and is *this skill's*
   convention for organizing task specs — it is **not** part of the engine, which
   receives only the file's *path* as the entry Message. Keep the spec concrete:

   ```md
   # <issue #> — <title>
   **Goal.** What must be true afterward, and why.
   **Exercises.** The concrete changes / files involved.
   **Done when.** Observable acceptance criteria (e.g. the tests that pass).
   ```
4. **Run the coder:**
   ```sh
   automedon examples/coder.yaml --message .workflows/<slug>/TASK.md
   ```
5. **Read the outcome** from the trace's final line `◆ RUN ended -> exit <code>`
   (not a shell/`tee` exit code):
   - `0` — review approved; the change is committed on your branch.
   - `90` — escalated (the build failed or review did not converge); changes are
     left unstaged for you. Diagnose (below), refine the `TASK.md`, and re-run.

### Diagnosing an EXIT 90

- The orchestrator prints the Run's ephemeral scratch directory (`$RUN_DIR`) to
  stderr on a failed Run. The coder leaves `FINDINGS.md` (review findings) and
  `BUILD_FAILURE.md` (build/test output) there — read them while refining the
  `TASK.md`.
- The durable log under `~/.local/state/automedon/runs/<run-id>/` (newest
  sorts last) holds each Step's `.stderr` sidecar; read the failing Step's to see
  why it failed. The `agent-orchestrator` skill documents the log layout.

### On success

Review the coder's commit (`git show`), then push and open a PR yourself.
