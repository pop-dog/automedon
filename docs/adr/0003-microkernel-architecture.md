# Microkernel architecture — the Kernel routes exit codes and nothing more

The tool is an *LLM agent orchestrator*, so the obvious design is to make LLM invocation a first-class concept. We deliberately do the opposite: the **Kernel** has zero awareness of LLMs, and treats all data as **opaque**. It is a small, stable engine, and every domain concern is an opt-in **Module** on top of it.

## Decision

- **The Kernel does only:** invoke a Step, read its exit code, route through Gates, manage Frames / Budget / Depth, and raise/propagate Faults.
- **The Step ABI is the entire Kernel↔Step contract:** *a Step is any process that exits with an integer.* The Kernel does not know or care whether that process is a shell script, a compiled binary, or an LLM agent.
- **Domain concerns are Modules.** LLM support is a Module providing two pure functions over a Step's Gate table — a Gates→prompt generator and an output→integer parser. Dataflow will likewise be a Module. The Kernel never depends on a Module.
- **The Kernel transports data and metadata without interpreting either.** A Gate's optional `when` description is kernel-opaque, and a Step's Message (stdout piped to the successor's stdin) is moved opaquely — the Kernel relays the bytes but never parses them (structure/schema is a Module/convention concern). Carrying data is not coupling; *interpreting* it would be. (See [[Message]] and ADR-0004 for why single-token execution makes this merge-free.)

## Consequences

- The graph layer stays LLM-independent, as the original concept required — the same engine orchestrates plain scripts and LLM agents identically.
- Prompt-engineering and provider/SDK churn (the fastest-moving part of the stack) is quarantined in Modules and cannot destabilise the core.
- There is exactly one kind of Step, preserving Step uniformity and the Composite substitutability of Workflows.
- The Step ABI is embodied in one seam: a `StepExecutor` trait. The routing core (`run`) decides *which* Gate to take and calls the executor to run a Step, so routing is testable with canned outcomes while the subprocess plumbing (`sh -c`, the deadlock-safe pipe threads, the signal→`-1` mapping) lives behind a single swappable adapter — a future executor is then an additive change, not a Kernel one.

## Considered and rejected

- **A first-class `LLM Step` type in the Kernel.** Most convenient for authors, but it couples the Kernel to LLM providers, SDKs, and prompt formats, and introduces a second Step kind that breaks Step uniformity and Composite. Rejected in favour of keeping the Step an ordinary command, with any LLM helpers living Step-side (outside the engine) rather than in the Kernel. See ADR-0002 for how malformed helper output (an unparseable LLM decision) re-uses the Fault channel rather than a bespoke error path.
