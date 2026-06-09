//! The Kernel — the Workflow engine and *only* the engine (ADR-0003 microkernel).
//! It invokes Steps, routes Gates, manages the Frame/Budget, raises Faults, and
//! emits Events. It is LLM- and data-agnostic. Modules (Sinks, an LLM adapter)
//! live outside and depend on the Kernel, never the reverse.
//!
//! Current scope: a single flat Frame, subprocess Steps, `integer | * |
//! EXHAUSTED | EXIT` routing, per-Step Budget + Exhaustion, and unhandled-outcome
//! Faults that abort the Run. Not yet implemented: the Frame stack, Composite
//! Steps, Depth, and catching Faults at a Frame boundary.

mod event;
mod ir;
mod run;

pub use event::{Event, Fault};
pub use ir::{Gate, GateKey, GateTarget, Step, Workflow, DEFAULT_BUDGET};
pub use run::run;

/// A Module that consumes the Kernel's Event stream (Observer). Durability is a
/// Sink's choice; the Kernel never persists anything itself.
pub trait Sink {
    fn emit(&mut self, event: &Event);
}

/// Where a [`Workflow`] comes from — the IR-as-interface boundary. The Kernel
/// owns the IR types; this trait abstracts the source. YAML is the first impl
/// (in `orchestrator`); JSON or a code-builder could be added without touching
/// the Kernel.
pub trait WorkflowSource {
    fn load(&self) -> Result<Workflow, Box<dyn std::error::Error>>;
}
