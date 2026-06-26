# Name the binary `automedon` — the charioteer, not the doer

The project's working name, "agent-orchestrator," and its binary, `orchestrator`,
were placeholders. The remote installer (a one-line `curl | bash`) puts a command
on every user's `PATH`, which makes the command name suddenly load-bearing —
costly to change once people and scripts depend on it. So the name had to be
settled before the installer ships. `orchestrator` is also generic on a shared
`PATH` and conceptually crowded: the workflow space is full of tools calling
themselves "orchestrator," so the word neither reads as a product name nor stays
collision-free once published.

## Decision

- **The binary installs as `automedon`.** The Cargo `[[bin]]` is renamed
  `orchestrator` → `automedon` so the dev install (`scripts/dev-install.sh`) and
  the remote install agree on a single command.
- **The rename is scoped to the binary/command for now.** Crate directories, the
  GitHub repo, and the README/`CONTEXT.md` titles keep the "agent-orchestrator"
  name pending a full rebrand (its own issue). The installed command and the
  project label diverge in the interim; accepted as the cost of keeping the
  installer change small.
- **Why `automedon`.** Automedon is Achilles' charioteer in the *Iliad* — the
  skilled driver who holds the reins and steers the team while the fighting
  happens up front. The engine is exactly that: under the microkernel boundary
  (ADR-0003) it does no domain work itself — it only invokes Steps, reads exit
  codes, and routes through Gates — while driving a team of Steps and agents along
  a controlled, bounded course. The name fixes the engine's role as the *driver*,
  not the *doer*.

## Consequences

- A single, distinctive command on `PATH` with effectively no collision in the
  workflow/CLI space, unlike `orchestrator`.
- Skill docs and examples that hard-code `orchestrator` must be updated; that work
  lives in the separate skills issue, so a transient mismatch exists between this
  rename and those docs until it lands.
- The repo, crates, and titles still read "agent-orchestrator," so newcomers meet
  two names until the rebrand issue closes; this ADR is the bridge that explains
  the relationship.
- Naming the engine after a charioteer rather than a conductor or "orchestrator"
  reinforces the microkernel framing (ADR-0003): the thing steers; the Steps do
  the work.

## Considered and rejected

- **Keep `orchestrator`.** Rejected: generic on a shared `PATH` and conceptually
  crowded, so it reads as neither a product name nor a collision-free command once
  the installer publishes it.
- **Rebrand the whole project now** (crates, repo, docs, paths). Rejected for this
  issue: a large, churny change that dwarfs the installer and ripples into open
  issues; deferred to a dedicated rebrand issue. The binary rename is the minimum
  the installer forces.
- **A verb-style name** (e.g. `gait`, `stride`). Considered: reads well as
  `<verb> workflow.yaml`, but the framing settled on "a thing that orchestrates,"
  which favours an agent/character noun over an action word.
- **Other mythic/SF orchestrator names** (e.g. `wintermute`, `maestro`). Rejected:
  longer to type or carrying heavier outside connotations and collisions;
  `automedon` is short, mythically precise to the "driver, not doer" role, and
  unencumbered.
