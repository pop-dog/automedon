//! The Event stream (a side output) and the Fault sum type.

use serde::Serialize;

use crate::ir::{GateKey, GateTarget};

/// An immutable record of one Kernel transition. Emitted to Sinks; routing runs
/// on separate working state (this is event *logging*, not sourcing).
///
/// `Serialize` lets a persistence Sink render the stream (e.g. as JSON); the
/// Kernel still chooses no concrete format (ADR-0005).
#[derive(Debug, Clone, Serialize)]
pub enum Event {
    RunStarted,
    StepEntered { step: String },
    StepExited { step: String, code: i32 },
    BudgetConsumed { step: String, remaining: u32 },
    Exhausted { step: String },
    GateTaken { step: String, key: GateKey, target: GateTarget },
    MessagePassed { from: String, to: String, bytes: usize },
    FaultRaised { fault: Fault },
    RunEnded { code: i32 },
}

/// A framework-detected inability to reach an Exit Gate — never an author-chosen
/// exit code. A Fault currently aborts the Run; catching a Fault at a Frame
/// boundary is not yet implemented.
#[derive(Debug, Clone, Serialize)]
pub enum Fault {
    /// A Step's exit code matched no Gate (and there was no Default Gate).
    UnhandledOutcome { step: String, code: i32 },
    /// A spent Budget with no EXHAUSTED Gate.
    UnhandledExhaustion { step: String },
    /// The Run's max Depth was exceeded. Not yet constructed: the run loop
    /// executes a single Frame, so nesting cannot occur.
    DepthOverflow,
}
