# Handoff — Execute Slice 2 (Initial Message), then Slice 3 (agentic coder)

**Date:** 2026-06-08
**Status:** Slice 1 (flat run loop) is built, tested, committed. The roadmap was
reordered to pull dogfooding forward. **Next: implement Slice 2 — Initial
Message** (a small prerequisite), which unblocks **Slice 3 — the agentic-coder
example Workflow** (the real goal).

## ⚠️ First: commit the uncommitted design changes

The working tree has **uncommitted** doc changes from a design (`/grill-with-docs`)
session that defined Slices 2–3:
- `docs/roadmap.md` (reordered to 6 slices; new Conventions section)
- `CONTEXT.md` (Entry Step refinement: its in-Message is the Workflow's arguments)
- `README.md` ("What's next" synced)
- this handoff file itself (untracked)

Commit these first (suggested subject: `Reorder roadmap to dogfood an agentic
coder early`) so the design is checkpointed before code starts. Branch is
`master`; HEAD is `67e6aa7`.

## Read these first (authoritative — don't re-derive)

- **`docs/roadmap.md`** — Slices 2, 3, and 6 are fully fleshed out (goal /
  exercises / key design work / done-when), plus the **Conventions** section
  (reserved `9x` escalation band; `90` = escalate to user). **This is the spec.**
- **`CONTEXT.md`** — glossary; use terms exactly (Message, Entry Step, Gate,
  Budget, …).
- **`docs/adr/0001`–`0007`** — architectural decisions.

## The task: Slice 2 — Initial Message

**Goal (see roadmap for the full done-definition).** Let a Run be invoked *with*
an input Message so a Workflow takes arguments. Today the entry Step always gets
an empty Message.

**Where the change lands:**
- `crates/kernel/src/run.rs` — `pub fn run(workflow, sink)` currently seeds
  `let mut message: Vec<u8> = Vec::new();`. Give `run` an initial-Message
  parameter (e.g. `run(workflow, initial_message: &[u8], sink)`) and seed from
  it. Update the call sites: the 7 existing tests in that file and `main.rs`.
- `crates/orchestrator/src/main.rs` — resolve the initial Message from a
  `--message <text>` flag and/or piped stdin (flag takes precedence; absent
  both, empty — preserving today's behaviour). Factor the resolution into a small
  pure fn so it is unit-testable without spawning a process.

**Tests (keep the kernel format-free; see ADR-0007 split):**
- In `kernel`: a Workflow whose entry Step consumes stdin and routes on it (the
  existing `message_is_piped_to_successor_stdin` test shows the
  `read n; exit "$n"` trick) — assert the entry Step receives the seeded Message.
- In `orchestrator`: unit-test the initial-Message resolution fn (flag wins;
  stdin fallback; empty default).
- Maintain the **60% coverage floor** (project standard in `.claude/CLAUDE.md`;
  currently ~83% total). `cargo llvm-cov` is installed.

**Done when.** `orchestrator <wf.yaml> --message "<text>"` (or piped stdin)
delivers that text to the entry Step's stdin; a test asserts it; omitting it
preserves the empty-Message behaviour.

## After Slice 2: Slice 3 (agentic coder) — design is settled

Slice 3 is fully specified in `docs/roadmap.md` (don't re-grill it). In brief: a
flat `code ⇄ review → commit` Workflow, all three Steps **bespoke** LLM agents
(`claude -p`); the task is a Message holding the **path to `TASK.md`** that
propagates through all three; `code` runs its own internal build+test loop; an
LLM `review` wraps the `/review` skill (Blocking → exit 1 loop, none → exit 0);
`commit` does `git diff` + `TASK.md` → message, commits on the **current branch,
never pushes**; non-convergence escalates via **`EXIT 90`**. The LLM Module
(prompt-gen + output-parser pure functions) is deferred to Slice 6 — Slice 3 is
deliberately bespoke. Open implementation details (deferred to build time, not
open design questions): `TASK.md` format, findings-file format, the bespoke
script shape.

## Environment notes (not in any artifact)

- **cargo is not on PATH** — run `. "$HOME/.cargo/env"` before any `cargo`.
  Toolchain: stable 1.96.
- Coverage: `cargo-llvm-cov` + `llvm-tools-preview` installed (`cargo llvm-cov`).
- `clippy` is **not** installed (`rustup component add clippy` to add it).
- Git identity is configured; never push.

## Conventions to honor (`.claude/CLAUDE.md` + `~/.claude/CLAUDE.md`)

- **60% test-coverage floor** (in-repo `.claude/CLAUDE.md`).
- **Comments** are for future readers and must **never reference the current
  task** — no "Slice N", "scrappy", "tracer bullet" in code comments; phrase
  incomplete-implementation markers as durable statements about the code.
- **Commit messages:** imperative subject ≤50 chars, no trailing period, body
  wrapped at 72 explaining what/why, end with
  `Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>`. No during-the-change
  detail (no "this session", no discarded choices).
- **Test placement mirrors ADR-0007:** run-loop tests in `kernel` (no format
  dep, struct-literal Workflows + a `MockSink`); format/CLI tests in
  `orchestrator`.
- **Stage specific paths, not `git add -A`** — it once swept an unrelated file
  into a commit.

## Working style the user expects

One question at a time, each with a *recommended* answer grounded in named CS
paradigms. The user often asks for plain-language definitions before assuming
shared terms. They push back hard and are often right; concede and find the
genuinely missing piece. **They commit explicitly** — do the work and offer,
don't auto-commit unless told.

## Suggested skills

- **`tdd`** — build Slice 2 test-first (red-green-refactor); the project has a
  suite and a coverage floor.
- **`code-review`** / **`review`** — review the slice before committing (a
  `general-purpose` subagent ran `/review` successfully in a prior session).
- **`run`** — to exercise `orchestrator` against an example and confirm the
  initial Message arrives.
- (Slice 3 only) **`prototype`** is *not* needed — the design is settled in the
  roadmap; go straight to implementation.
