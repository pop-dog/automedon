# Test coverage

Unit tests run with `cargo test`. The run-loop tests (gate routing, the Budget
cascade, Exhaustion, Faults, Message piping) live in `crates/kernel`; the
YAML-parsing tests live in `crates/orchestrator`, keeping the Kernel free of any
format dependency.

Line coverage is measured with
[`cargo-llvm-cov`](https://github.com/taiki-e/cargo-llvm-cov):

```sh
rustup component add llvm-tools-preview   # one-time
cargo install cargo-llvm-cov              # one-time
cargo llvm-cov                            # summary table
cargo llvm-cov --html --open              # browsable line-by-line report
```

The `kernel` crate (the correctness-critical engine) is the coverage target.
The uncovered remainder is the deliberate `panic!` paths for malformed input
(a deferred production-hardening item, tracked under the [Cross-cutting work
milestone](https://github.com/pop-dog/automedon/milestones)) and the
console Sink's rendering glue. The project requires 60% test coverage.
