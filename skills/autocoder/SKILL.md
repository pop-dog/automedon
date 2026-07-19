---
name: autocoder
description: Turn a GitHub issue into a published, ready-for-review PR by driving the bundled autocoder Workflow (distill → checkout → code → review → commit → create-pr) instead of editing the repo directly. You give it an issue reference; the Workflow's agents do everything else. Use when asked to code or fix a GitHub issue in the Automedon project via Automedon.
---

## Autocoder — issue in, published PR out

Give this skill a GitHub issue and it runs the bundled `autocoder` Workflow:
`setup (distill → checkout) → coder → create-pr`. `distill` reads the issue and
writes a concrete `TASK.md`; `checkout` prepares a dedicated git worktree and
branch for it; `coder` is the existing `code → review → commit` loop, unmodified,
run over that worktree; `create-pr` pushes the branch and opens the PR. This
skill builds on the **`automedon`** skill (the engine mechanics — invocation,
traces, exit codes, logs); activate that skill for anything about running the
binary or reading a Run.

**This skill is repo-only.** It requires a checkout: it runs
`examples/autocoder.yaml`, whose Steps read repo files via
`$AUTOMEDON_WORKFLOW_DIR`. There is no remote installer for it — it is an
example/template for coding the Automedon project itself.

**Do not edit the project's source yourself.** Give it the issue; every code
change and every git operation (worktree, branch, push, PR) comes from the
Workflow.

### Prerequisites

- `automedon`, `claude`, `cargo`, and `gh` on `PATH` — the Workflow's Steps read
  the issue and open the PR with `gh`, drive `claude` agents, and run
  `cargo build && cargo test`.
- **Re-install after engine changes.** The `automedon` binary is a build
  snapshot. Because this Workflow can modify the engine itself (the
  `orchestrator`/`kernel` crates), re-run the repo's `scripts/dev-install.sh`
  whenever the engine changes, so the binary you run reflects them.

### Running it

From the root of the Automedon checkout, give the entry Message as an issue
number or URL:

```sh
automedon run examples/autocoder.yaml --message 42
```

### Reading the outcome

Read the trace's final line `◆ RUN ended -> exit <code>` (not a shell/`tee`
exit code):

- `0` — the PR is open and ready for review. Before handing the link to the
  user, do your own conversation-only first-pass review: read the PR diff and,
  where cheap, exercise the built artifact. Report findings in conversation
  only — no PR comments, no fixes. You hold provenance the in-Workflow
  reviewer lacks (the originating issue's discussion and related decisions),
  so this pass catches different things than that one did. All code changes
  still come only from the Workflow's agents; the user decides merge vs.
  refine-and-re-run from what you report.
- `90` — escalated (distilling/checking out the issue failed, the build failed,
  review did not converge, or pushing/opening the PR failed). Diagnose in the
  worktree the Run left in place:
  - The orchestrator prints the Run's ephemeral scratch directory
    (`$AUTOMEDON_RUN_DIR`) to stderr on a failed Run. The coder leaves
    `FINDINGS.md` (review findings) and `BUILD_FAILURE.md` (build/test output)
    there.
  - The durable log under `~/.local/state/automedon/runs/<run-id>/` (newest
    sorts last) holds each Step's `.stderr` sidecar; read the failing Step's to
    see why it failed. The `automedon` skill documents the log layout.
  - Refine the GitHub issue with whatever the failure revealed, then re-run the
    same command — recovery reuses the existing worktree and branch, and
    re-distills the TASK.md from the refined issue so the new spec reaches the
    agents.

### Post-merge cleanup

Once the PR merges, remove its worktree and branch:

```sh
git worktree remove ../automedon.worktrees/<slug> && git branch -d <branch>
```
