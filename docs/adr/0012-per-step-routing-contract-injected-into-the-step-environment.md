# The Step's routing contract: a per-Step, kernel-owned ABI member

Interfacing a Step with an LLM means turning the Step's Gate table into a prompt
and parsing the reply back into a Gate key. For the Gate table to be the *single
source of truth* — so the prompt and the parse can never drift from what the
Kernel actually routes on — that Step-side logic must read the Step's own Gates
(keys + `when`) at run time. The open question (issue #9) was *how* that table
reaches the Step's subprocess.

## Decision

- **Inject it, generically.** The execution seam exposes each leaf Step its own
  routing contract — "here is how your exit code will be routed." The Kernel and
  Executor never learn what an LLM is; the LLM interpretation happens entirely
  Step-side, in the Step's own command. (Honest caveat: the contract's *shape* — `Code`/`Default`
  pairs with `when`, `Default` narrowed out by the consumer — is designed with
  prompt generation as the first and currently only consumer. It is provider-neutral
  and any Step may read it, but "generic, not LLM-shaped" is the intent, not an
  accident of a pre-existing need.)
- **A projected, kernel-owned type crosses the seam.** `StepExecutor::execute`
  takes a per-call `RoutingContract` — the Step's `Code` and `Default` Gates as
  `{ key, when }` pairs, projected once by the run loop from `step.gates`. No
  targets (Kernel-internal routing; leaking sibling Step names would erode the
  black-box / name-scope property), no `EXHAUSTED`/`FAULT` (not exit-code
  outcomes). The Module narrows `Default` out when generating the prompt.
- **The subprocess adapter serializes; the seam does not.** `SubprocessExecutor`
  serializes the `RoutingContract` to JSON in `$AUTOMEDON_GATES`. Serialization is
  the adapter's private choice, so a future in-process Executor (ADR-0004) can
  hand an in-process consumer the struct directly. The Kernel routing core is untouched.
- **The Kernel owns the `$AUTOMEDON_GATES` schema.** The wire format is a generic,
  Kernel-owned, public format that any Step may read; the consumer is a Step's own
  command (today, an example LLM helper script), never a crate the Kernel depends
  on. Keeping the schema stable and Kernel-owned is what lets a forkable,
  user-space consumer read it without the producer depending on it.
- **Brand the Step-facing env namespace `AUTOMEDON_*`.** Introducing
  `$AUTOMEDON_GATES` makes the unprefixed members inconsistent and collision-prone
  with a Step's ambient environment, so `$WORKFLOW_DIR` → `$AUTOMEDON_WORKFLOW_DIR`
  and `$RUN_DIR` → `$AUTOMEDON_RUN_DIR`. One namespace for the whole channel.

## Considered options

- **Bake the Gate table into the command args.** Rejected: the Gates would live in
  both `gates:` and the command, reintroducing exactly the drift the Module exists
  to eliminate.
- **Inject only the Step's identity; the consumer re-reads the Workflow file.**
  Rejected: still needs a per-Step ABI member (`$STEP_NAME`), *and* couples the
  consumer to the YAML surface syntax — an `orchestrator` concern, not the Kernel's —
  forcing every helper to re-implement the loader. Injecting the already-projected
  contract keeps the consumer a few lines of JSON-reading shell.

## Consequences

- The Step environment is no longer wholly *broadcast*. The routing contract is a
  **per-Step** member, distinct from the Run-constant broadcast members
  (`$AUTOMEDON_WORKFLOW_DIR`, `$AUTOMEDON_RUN_DIR`). This amends ADR-0010: the
  broadcast members stay a constant field on the Executor; the routing contract is
  a per-`execute()` argument.
- The env-var rename is a breaking ABI change touching the coder example scripts
  and the `automedon`/`autocoder` skill docs.
