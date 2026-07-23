//! The IR: a Workflow as plain data. The Kernel runs against these types; how
//! they are *produced* is abstracted by [`crate::WorkflowSource`] (YAML is the
//! first implementation, in the `orchestrator` crate).

use std::collections::HashMap;

use serde::{de, Deserialize, Deserializer, Serialize};

/// Identifies a Workflow within a [`Registry`]. A Composite Step names its child
/// by this id; it is the `workflows:` map key, never a path.
pub type WorkflowId = String;

/// The set of Workflows a Run can reach, plus the id of the root. The Kernel runs
/// against a registry rather than a single Workflow so a Composite Step can name
/// a child by id; whether ids were assembled from one file or many is
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
    /// Author-declared per-spawn environment, in declaration order. Leaf-only
    /// (rejected at load on a Composite Step) and distinct from the
    /// `PathBuf`-valued, Run-constant environment `SubprocessExecutor` carries —
    /// this is computed fresh for each spawn and never touches the orchestrator's
    /// own process environment.
    pub env: Vec<(String, String)>,
}

/// What a Step does. A leaf `Command` runs via the executor; a `Workflow`
/// (Composite) runs the named child sub-Workflow to its Exit Gate and surfaces
/// its code. The two are mutually exclusive — illegal "two bodies" states are
/// unrepresentable.
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
            #[serde(default)]
            env: Option<RawEnv>,
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
        let env = raw.env.map(|e| e.0).unwrap_or_default();
        // A Composite Step spawns no process, so `env:` there could only mean
        // implicit propagation to the child Frame — nothing needs that, and
        // silently ignoring the field would hide a likely author mistake.
        if matches!(body, StepBody::Workflow(_)) && !env.is_empty() {
            return Err(de::Error::custom(
                "`env:` is only valid on a `command:` step; a `workflow:` step spawns no process",
            ));
        }
        for (key, _) in &env {
            if key.starts_with("AUTOMEDON_") {
                return Err(de::Error::custom(format!(
                    "step env key {key:?} uses the reserved `AUTOMEDON_` prefix"
                )));
            }
        }
        Ok(Step { body, budget: raw.budget, gates: raw.gates, env })
    }
}

/// The raw `env:` map as written in the Workflow file: any YAML scalar value is
/// accepted and stringified in its canonical form; a sequence or map value is a
/// load-time error. A hand-written `Deserialize` (rather than
/// `HashMap<String, EnvScalar>`) so declaration order survives into the `Vec` —
/// the Kernel is format-agnostic, so this reads through serde's generic map
/// visitor rather than a YAML-specific `Value`.
struct RawEnv(Vec<(String, String)>);

impl<'de> Deserialize<'de> for RawEnv {
    fn deserialize<D>(d: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct EnvVisitor;

        impl<'de> de::Visitor<'de> for EnvVisitor {
            type Value = RawEnv;

            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("a map of environment variable names to scalar values")
            }

            fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
            where
                A: de::MapAccess<'de>,
            {
                let mut entries = Vec::new();
                while let Some((key, EnvScalar(value))) = map.next_entry()? {
                    entries.push((key, value));
                }
                Ok(RawEnv(entries))
            }
        }

        d.deserialize_map(EnvVisitor)
    }
}

/// One `env:` value, coerced to its canonical string form (`1` -> `"1"`,
/// `true` -> `"true"`). A sequence or map value fails to deserialize.
struct EnvScalar(String);

impl<'de> Deserialize<'de> for EnvScalar {
    fn deserialize<D>(d: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct ScalarVisitor;

        impl<'de> de::Visitor<'de> for ScalarVisitor {
            type Value = EnvScalar;

            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("a scalar (string, integer, float, or bool)")
            }

            fn visit_bool<E>(self, v: bool) -> Result<Self::Value, E> {
                Ok(EnvScalar(v.to_string()))
            }

            fn visit_i64<E>(self, v: i64) -> Result<Self::Value, E> {
                Ok(EnvScalar(v.to_string()))
            }

            fn visit_u64<E>(self, v: u64) -> Result<Self::Value, E> {
                Ok(EnvScalar(v.to_string()))
            }

            fn visit_f64<E>(self, v: f64) -> Result<Self::Value, E> {
                Ok(EnvScalar(v.to_string()))
            }

            fn visit_str<E>(self, v: &str) -> Result<Self::Value, E> {
                Ok(EnvScalar(v.to_string()))
            }

            fn visit_string<E>(self, v: String) -> Result<Self::Value, E> {
                Ok(EnvScalar(v))
            }
        }

        d.deserialize_any(ScalarVisitor)
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

#[cfg(test)]
mod tests {
    use super::{Step, StepBody};

    #[test]
    fn author_env_round_trips_in_declaration_order() {
        let step: Step = serde_json::from_str(
            r#"{"command": "noop", "gates": [], "env": {"FIRST": "a", "SECOND": "b"}}"#,
        )
        .unwrap();
        assert_eq!(
            step.env,
            vec![("FIRST".to_string(), "a".to_string()), ("SECOND".to_string(), "b".to_string())]
        );
    }

    #[test]
    fn integer_and_bool_scalars_are_stringified() {
        let step: Step = serde_json::from_str(
            r#"{"command": "noop", "gates": [], "env": {"N": 1, "B": true}}"#,
        )
        .unwrap();
        assert_eq!(
            step.env,
            vec![("N".to_string(), "1".to_string()), ("B".to_string(), "true".to_string())]
        );
    }

    #[test]
    fn a_sequence_env_value_is_a_load_error() {
        let err = serde_json::from_str::<Step>(
            r#"{"command": "noop", "gates": [], "env": {"BAD": [1, 2]}}"#,
        )
        .unwrap_err();
        assert!(err.to_string().contains("scalar"), "unexpected error: {err}");
    }

    #[test]
    fn a_map_env_value_is_a_load_error() {
        let err = serde_json::from_str::<Step>(
            r#"{"command": "noop", "gates": [], "env": {"BAD": {"nested": 1}}}"#,
        )
        .unwrap_err();
        assert!(err.to_string().contains("scalar"), "unexpected error: {err}");
    }

    #[test]
    fn env_on_a_workflow_step_is_a_load_error() {
        let err = serde_json::from_str::<Step>(
            r#"{"workflow": "child", "gates": [], "env": {"X": "1"}}"#,
        )
        .unwrap_err();
        assert!(
            err.to_string().contains("workflow"),
            "expected the error to explain the leaf-only rule: {err}"
        );
    }

    #[test]
    fn an_automedon_prefixed_key_is_a_load_error() {
        let err = serde_json::from_str::<Step>(
            r#"{"command": "noop", "gates": [], "env": {"AUTOMEDON_FOO": "1"}}"#,
        )
        .unwrap_err();
        assert!(
            err.to_string().contains("AUTOMEDON_"),
            "expected the error to name the reserved prefix: {err}"
        );
    }

    #[test]
    fn env_is_optional_and_defaults_to_empty() {
        let step: Step = serde_json::from_str(r#"{"command": "noop", "gates": []}"#).unwrap();
        assert!(step.env.is_empty());
        assert!(matches!(step.body, StepBody::Command(_)));
    }
}
