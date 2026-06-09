//! The IR: a Workflow as plain data. The Kernel runs against these types; how
//! they are *produced* is abstracted by [`crate::WorkflowSource`] (YAML is the
//! first implementation, in the `orchestrator` crate).

use std::collections::HashMap;

use serde::{Deserialize, Deserializer};

/// A directed control-flow graph of Steps connected by Gates.
#[derive(Debug, Deserialize)]
pub struct Workflow {
    /// The single Entry Step where the Frame begins execution.
    pub entry: String,
    /// Workflow-wide Budget default (cascade: Step value -> this -> [`DEFAULT_BUDGET`]).
    #[serde(default)]
    pub default_budget: Option<u32>,
    /// Step names are local to this Workflow (the namespace).
    pub steps: HashMap<String, Step>,
}

/// A single executable unit: runs a command, terminates with an exit code.
#[derive(Debug, Deserialize)]
pub struct Step {
    /// The command, run via `sh -c` with the working directory inherited. A Step
    /// is currently only a command; Composite (sub-Workflow) Steps are not yet
    /// represented.
    pub command: String,
    /// Max activations of this Step within one Frame. `None` -> cascade default.
    #[serde(default)]
    pub budget: Option<u32>,
    /// Outgoing Gates. Each key unlocks at most one Gate.
    #[serde(default)]
    pub gates: Vec<Gate>,
}

/// An outgoing exit from a Step: `(key -> target)`.
#[derive(Debug, Clone, Deserialize)]
pub struct Gate {
    pub key: GateKey,
    pub target: GateTarget,
    /// Kernel-opaque description of what the key means. Transported, never routed on.
    #[serde(default)]
    pub when: Option<String>,
}

/// What unlocks a Gate. Routing matches an exit code against these in order:
/// an exact `Code`, then `Default`; the special keys handle the cases where a
/// Step produced no routable exit code.
#[derive(Debug, Clone, PartialEq)]
pub enum GateKey {
    /// An exact integer exit code.
    Code(i32),
    /// The Default Gate `*` — matches unmatched *integers only*.
    Default,
    /// Taken instead of entering a Step whose Budget is spent.
    Exhausted,
    /// Taken when a child sub-Workflow surfaces a Fault. Not yet routed on:
    /// catching a Fault at a Frame boundary is not implemented.
    Fault,
}

/// Where a Gate leads: a successor Step, or termination with an exit code.
#[derive(Debug, Clone)]
pub enum GateTarget {
    /// Internal Gate -> a successor Step (by local name).
    Step(String),
    /// Exit Gate (`EXIT <code>`) -> termination carrying the Workflow exit code.
    Exit(i32),
}

/// Hardcoded Budget default, last in the cascade.
pub const DEFAULT_BUDGET: u32 = 10;

// Written in YAML as a single-key map: `{ step: name }` or `{ exit: 0 }`.
impl<'de> Deserialize<'de> for GateTarget {
    fn deserialize<D>(d: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct Raw {
            #[serde(default)]
            step: Option<String>,
            #[serde(default)]
            exit: Option<i32>,
        }
        let raw = Raw::deserialize(d)?;
        match (raw.step, raw.exit) {
            (Some(s), None) => Ok(GateTarget::Step(s)),
            (None, Some(c)) => Ok(GateTarget::Exit(c)),
            _ => Err(serde::de::Error::custom(
                "gate target must be exactly one of `{ step: <name> }` or `{ exit: <code> }`",
            )),
        }
    }
}

// YAML writes keys as `0` (int) or `"*"`/`"EXHAUSTED"`/`"FAULT"` (string), so we
// deserialize via an untagged shim and map onto the sum type.
impl<'de> Deserialize<'de> for GateKey {
    fn deserialize<D>(d: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Raw {
            Int(i32),
            Str(String),
        }
        match Raw::deserialize(d)? {
            Raw::Int(n) => Ok(GateKey::Code(n)),
            Raw::Str(s) => match s.as_str() {
                "*" => Ok(GateKey::Default),
                "EXHAUSTED" => Ok(GateKey::Exhausted),
                "FAULT" => Ok(GateKey::Fault),
                other => Err(serde::de::Error::custom(format!(
                    "invalid gate key {other:?} (want an integer, \"*\", \"EXHAUSTED\", or \"FAULT\")"
                ))),
            },
        }
    }
}
