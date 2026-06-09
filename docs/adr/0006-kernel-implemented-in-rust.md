---
status: accepted
---

# Kernel implemented in Rust

The Kernel is the trust anchor for the totality and Fault guarantees (ADR-0001, ADR-0002), and its domain is almost entirely **sum types** — Gate keys (`integer | * | EXHAUSTED | FAULT`), the three Fault kinds, and the ~12 Event variants. We will implement the production Kernel in **Rust**, chosen for compiler-enforced **exhaustive matching** over those sum types (the compiler does the case-analysis a trust anchor needs) and for compiling to a **single self-contained static binary** (the `curl | bash` distribution goal). Because the Kernel is a microkernel (small, stable, rarely-changing — ADR-0003), Rust's up-front borrow-checker cost is paid once on a small surface and amortised, neutralising Go's usual iteration-speed advantage.

The Python Kernel remains a throwaway prototype, not the foundation (see project memory `kernel-language-plan`).

## Independent of the authoring surface

The Kernel consumes a **language-neutral IR** (YAML being the first authoring front-end, with the door open to others). The Kernel never imports author code, so its language is an isolated choice — and a code-authoring DSL may later be added in *any* language without touching the Kernel. This is the compiler front-end/back-end split: authoring is the front-end, the Kernel the back-end, the IR the contract between them.

## Considered and rejected

- **Go** — huge contributor pool, simplicity, fast iteration, static binary by default. Rejected because it has **no sum types and no exhaustiveness checking**: a Gate key or Fault kind would be an `interface{}` + type switch the compiler cannot verify is complete. For a trust anchor whose entire job is handling *every* routing/fault variant, trading compiler-enforced case coverage for test coverage was the decisive cost. The microkernel's small, stable surface also mutes Go's iteration-speed edge.
- **OCaml / Zig** — equal or better sum-type modelling and pattern matching, and both compile native binaries. Rejected for materially smaller ecosystems and contributor pools.
- **TypeScript (Bun-compiled single binary)** — its one distinctive draw was being the *authoring host language*; decoupling authoring via the IR removed that, leaving structural unions and a weaker correctness story than Rust.
- **Python** — the prototype language; rejected for production by the distribution problem (bundling an interpreter vs shipping one static binary) and the absence of compiler-enforced exhaustiveness.
