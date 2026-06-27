//! The Kernel — the Workflow engine and *only* the engine (ADR-0003 microkernel).
//! It invokes Steps, routes Gates, manages the Frame/Budget, raises Faults, and
//! emits Events. It is LLM- and data-agnostic. Modules (e.g. Sinks) live outside
//! and depend on the Kernel, never the reverse.
//!
//! Current scope: a Frame stack over a Workflow registry (Composite Steps run a
//! named child sub-Workflow and surface its code), subprocess leaf Steps,
//! `integer | * | EXHAUSTED | FAULT | EXIT` routing, per-Frame Budget +
//! Exhaustion, a Run-level Depth cap, and structured Fault propagation across
//! Frame boundaries.

mod contract;
mod event;
mod executor;
mod ir;
mod run;

pub use contract::{RoutingContract, RoutingEntry};
pub use event::{Event, Fault};
pub use executor::{StepExecutor, SubprocessExecutor};
pub use ir::{
    Gate, GateKey, GateTarget, Registry, Step, StepBody, Workflow, WorkflowId, DEFAULT_BUDGET,
};
pub use run::{run, RunConfig, DEFAULT_MAX_DEPTH};

use serde::Serialize;

/// Which of a Step's two output streams a chunk came from. Carried by
/// [`Sink::on_output`] so a Sink can keep stdout and stderr distinct.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Stream {
    Stdout,
    Stderr,
}

/// A Module that consumes the Kernel's Event stream (Observer). Durability is a
/// Sink's choice; the Kernel never persists anything itself.
pub trait Sink {
    /// Receive one control-plane transition.
    fn emit(&mut self, event: &Event);

    /// Receive a chunk of a Step's bulk output (ADR-0009). Bytes arrive as the
    /// Step produces them, bracketed between its `StepEntered` and `StepExited`
    /// Events; `activation` disambiguates repeated runs of the Step under its
    /// Budget. Defaults to a no-op so control-only Sinks need not implement it.
    fn on_output(&mut self, step: &str, activation: u32, stream: Stream, bytes: &[u8]) {
        let _ = (step, activation, stream, bytes);
    }
}

/// Where a [`Registry`] comes from — the IR-as-interface boundary. The Kernel
/// owns the IR types; this trait abstracts the source. YAML is the first impl
/// (in `orchestrator`); JSON or a code-builder could be added without touching
/// the Kernel. How references resolve to ids — within one file now, across files
/// later — is entirely a source concern (ADR-0008).
pub trait WorkflowSource {
    fn load(&self) -> Result<Registry, Box<dyn std::error::Error>>;
}
