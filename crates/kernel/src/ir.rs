//! The IR: a Workflow as plain data. The Kernel runs against these types; how
//! they are *produced* is abstracted by [`crate::WorkflowSource`] (YAML is the
//! first implementation, in the `orchestrator` crate).

use std::collections::HashMap;

use serde::{de, Deserialize, Deserializer, Serialize};

/// Identifies a Workflow within a [`Registry`]. A Composite Step names its child
/// by this id; it is the `workflows:` map key, never a path (ADR-0008).
pub type WorkflowId = String;

/// The set of Workflows a Run can reach, plus the id of the root. The Kernel runs
/// against a registry rather than a single Workflow so a Composite Step can name
/// a child by id (ADR-0008); whether ids were assembled from one file or many is
/// a [`crate::WorkflowSource`] concern the engine never sees.
#[derive(Debug, Deserialize)]
pub struct Registry {
    /// The Workflow the Run begins in.
    pub root: WorkflowId,
    /// Every Workflow, keyed by id. References (`StepBody::Workflow`) resolve here.
    pub workflows: HashMap<WorkflowId, Workflow>,
}

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

/// A single routable unit: it terminates with an exit code that unlocks a Gate.
#[derive(Debug)]
pub struct Step {
    /// What the Step *is*: a leaf command, or a sub-Workflow (Composite).
    pub body: StepBody,
    /// Max activations of this Step within one Frame. `None` -> cascade default.
    pub budget: Option<u32>,
    /// Outgoing Gates. Each key unlocks at most one Gate.
    pub gates: Vec<Gate>,
}

/// What a Step does. A leaf `Command` runs via the executor; a `Workflow`
/// (Composite) runs the named child sub-Workflow to its Exit Gate and surfaces
/// its code. The two are mutually exclusive — illegal "two bodies" states are
/// unrepresentable (ADR-0008).
#[derive(Debug, Clone)]
pub enum StepBody {
    /// Run a command via `sh -c` with the working directory inherited.
    Command(String),
    /// Push a Frame for the named child Workflow (a Composite Step).
    Workflow(WorkflowId),
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
#[derive(Debug, Clone, PartialEq, Serialize)]
pub enum GateKey {
    /// An exact integer exit code.
    Code(i32),
    /// The Default Gate `*` — matches unmatched *integers only*.
    Default,
    /// Taken instead of entering a Step whose Budget is spent.
    Exhausted,
    /// Taken when a child sub-Workflow surfaces a Fault (the `catch` clause).
    Fault,
}

/// Where a Gate leads: a successor Step, or termination with an exit code.
#[derive(Debug, Clone, Serialize)]
pub enum GateTarget {
    /// Internal Gate -> a successor Step (by local name).
    Step(String),
    /// Exit Gate (`EXIT <code>`) -> termination carrying the Workflow exit code.
    Exit(i32),
}

/// Hardcoded Budget default, last in the cascade.
pub const DEFAULT_BUDGET: u32 = 10;

// A Step body is written as a flat `command:` *xor* `workflow:` key alongside
// `budget`/`gates`, so we deserialize via a raw shim and map onto `StepBody` —
// the same one-of pattern as `GateTarget`, keeping the illegal two-bodies state
// out of the type.
impl<'de> Deserialize<'de> for Step {
    fn deserialize<D>(d: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct Raw {
            #[serde(default)]
            command: Option<String>,
            #[serde(default)]
            workflow: Option<WorkflowId>,
            #[serde(default)]
            budget: Option<u32>,
            #[serde(default)]
            gates: Vec<Gate>,
        }
        let raw = Raw::deserialize(d)?;
        let body = match (raw.command, raw.workflow) {
            (Some(c), None) => StepBody::Command(c),
            (None, Some(w)) => StepBody::Workflow(w),
            _ => {
                return Err(de::Error::custom(
                    "step body must be exactly one of `command: <sh>` or `workflow: <id>`",
                ))
            }
        };
        Ok(Step { body, budget: raw.budget, gates: raw.gates })
    }
}

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
