---
status: accepted
---

# IR behind a WorkflowSource interface; Kernel and Modules as separate crates

The Kernel runs against an **IR** — the Workflow as plain data (the `Workflow`/`Step`/`Gate`/`GateKey` types it owns) — and never parses an authoring format itself. A **`WorkflowSource` trait** abstracts where that IR comes from; **YAML is the first implementation** (in the `orchestrator` crate, via `serde_yaml`), and a JSON loader or a code-builder could be added later without touching the Kernel. This is the compiler front-end/back-end split: authoring is the front-end, the Kernel the back-end, the IR the contract between them — keeping the authoring surface open (ADR-pending) while letting the Kernel language be decided in isolation (ADR-0006).

The codebase is a **Cargo workspace**: `crates/kernel` (the engine) is a separate crate from the Modules (Sinks today; an LLM adapter later). The dependency arrows physically enforce ADR-0003: `orchestrator` → `kernel`, never the reverse, so the Kernel cannot accidentally depend on a Module.

## Notes

- The Kernel depends on `serde` for *derive only* — serde is format-agnostic; the concrete format (`serde_yaml`) lives in `orchestrator`. So "Kernel owns the IR types" does not leak a format dependency into the Kernel.
- The `Sink` trait (Observer, ADR-0005) lives in the Kernel; its implementations (the console trace today) live outside, the same way `WorkflowSource` does.
