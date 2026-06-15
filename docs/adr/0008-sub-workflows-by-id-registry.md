---
status: accepted
---

# Sub-Workflows referenced by id through a registry

A Composite Step *is* a sub-Workflow (Composite pattern), so the Kernel must reach every sub-Workflow a Run can enter. Rather than have a Composite Step own its child inline, the Kernel runs against a **registry** — a set of Workflows keyed by a `WorkflowId`, plus the id of the root. A Composite Step holds a `WorkflowId`; entering it looks the child up in the registry and pushes a Frame. This replaces the single-`Workflow` `run` signature with `run(registry, root, …)`.

## Decision

- **The Kernel addresses sub-Workflows by id, never by value or by path.** A Composite Step's body is a `WorkflowId` into the registry. The Kernel is therefore agnostic to *how* the registry was assembled — it cannot tell a within-file reference from a cross-file one, because by run time both are just a Step holding an id.
- **Resolution is a `WorkflowSource` concern, not a Kernel one (ADR-0007).** How a reference binds to an id, and how the registry is populated, lives entirely in the source layer. This keeps the file-format question out of the engine.
- **Slice 4 ships one resolution strategy: the multi-Workflow file, referenced by name.** The top-level YAML becomes `root:` + `workflows:` (a map of named Workflows); a reference is a name lookup in that map. No filesystem, no path resolution, no load-time cycle handling. The existing `Workflow` struct is unchanged — it becomes a *value* in the map instead of the document root.
- **Cross-file references (path/import) are deferred to a follow-up, by construction a no-Kernel-change extension.** Because the Kernel already takes a registry keyed by id, a path-based `WorkflowSource` is purely additive: it canonicalizes paths to ids and transitively loads files into the same registry. The engine — stack machine, Depth, Budget reset, exit-code surfacing, FAULT propagation — is untouched.

## Why this matters

Reuse is the point of composition, and reuse has two scopes. *Within* a system — one sub-Workflow referenced by several parents — the multi-Workflow file already delivers. *Across* systems — a sub-Workflow defined once and reused by Workflows in other files or projects — needs path/import, because a self-contained file is a reuse boundary. The registry-by-id boundary is precisely what lets the smaller scope ship now and the larger scope land later without re-opening the Kernel.

## Considered and rejected

- **Inlined child (`Box<Workflow>` in the Composite Step).** Simplest to parse, but it cannot express recursion (the type is infinite / expansion is eager), and it forces every reused sub-Workflow to be copied into each consumer — defeating reuse. Recursion is required: the Depth cap (ADR-0001) is tested by a Workflow that references itself, which a registry expresses as an id appearing in its own map.
- **Kernel resolves paths itself.** Would let a Composite Step name a file directly, but it makes the Kernel format- and IO-aware, violating ADR-0007's front-end/back-end split.
- **Path/import as the Slice 4 surface.** The destination for cross-file reuse, but it pulls relative-path resolution and load-time cycle dedup into the slice whose actual subject is the stack machine. Deferred — the registry boundary guarantees it costs nothing later.
