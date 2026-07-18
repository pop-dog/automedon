//! The Event stream (a side output) and the Fault sum type.

use serde::Serialize;

use crate::ir::{GateKey, GateTarget};

/// An immutable record of one Kernel transition. Emitted to Sinks; routing runs
/// on separate working state (this is event *logging*, not sourcing).
///
/// `Serialize` lets a persistence Sink render the stream (e.g. as JSON); the
/// Kernel still chooses no concrete format.
#[derive(Debug, Clone, Serialize)]
pub enum Event {
    RunStarted,
    StepEntered { step: String },
    StepExited { step: String, code: i32 },
    BudgetConsumed { step: String, remaining: u32 },
    Exhausted { step: String },
    /// A Composite Step pushed a Frame for its child `workflow`; `depth` is the
    /// pushed Frame's Depth. Nested inside the Step's `StepEntered`/`StepExited`
    /// bracket.
    FramePushed { step: String, workflow: String, depth: u32 },
    /// The child Frame entered at `step` has been popped (it reached an Exit Gate
    /// or unwound a Fault). The surfaced exit code, if any, follows in the
    /// Composite Step's `StepExited`.
    FramePopped { step: String, workflow: String },
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
    /// Entering a Composite Step would exceed the Run's max Depth. Carries the
    /// child `workflow` that could not be pushed. The one non-catchable Fault: it
    /// is never offered to a `FAULT` Gate and aborts the Run unconditionally.
    DepthOverflow { workflow: String },
}
