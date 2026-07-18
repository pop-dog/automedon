//! The per-Step routing contract: a generic, Kernel-owned description of how a
//! leaf Step's exit code will be routed, projected from its Gates and handed
//! across the execution seam. It exposes *how your exit code routes* —
//! the `Code`/`Default` keys and their opaque `when` text — never targets
//! (Kernel-internal routing; leaking sibling Step names would erode the
//! black-box property) and never the non-exit-code `EXHAUSTED`/`FAULT` Gates.

use std::ops::Deref;

use serde::Serialize;

use crate::ir::{GateKey, Step};

/// One routable exit-code outcome in a Step's [`RoutingContract`]: the exit-code
/// `key` and the Gate's opaque `when` text.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RoutingEntry {
    /// The decimal exit code for a `Code(n)` Gate (e.g. `"0"`, `"1"`), or `"*"`
    /// for the `Default` Gate.
    pub key: String,
    /// The Gate's optional, Kernel-opaque description, `null` when absent.
    pub when: Option<String>,
}

/// A Step's routing contract: its `Code`/`Default` Gates as `{ key, when }`
/// pairs, in declaration order. Derefs to the entry slice so it reads as the
/// collection it is.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct RoutingContract(Vec<RoutingEntry>);

impl RoutingContract {
    /// Project the contract from a Step's Gates, keeping only the `Code` and
    /// `Default` Gates (the exit-code outcomes) in declaration order.
    pub fn from_step(step: &Step) -> Self {
        let entries = step
            .gates
            .iter()
            .filter_map(|gate| {
                let key = match gate.key {
                    GateKey::Code(code) => code.to_string(),
                    GateKey::Default => "*".to_string(),
                    GateKey::Exhausted | GateKey::Fault => return None,
                };
                Some(RoutingEntry { key, when: gate.when.clone() })
            })
            .collect();
        RoutingContract(entries)
    }
}

impl Deref for RoutingContract {
    type Target = [RoutingEntry];

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use super::RoutingContract;
    use crate::ir::{Gate, GateKey, GateTarget, Step, StepBody};

    fn gate(key: GateKey, when: Option<&str>) -> Gate {
        Gate { key, target: GateTarget::Exit(0), when: when.map(str::to_string) }
    }

    #[test]
    fn projection_keeps_only_code_and_default_gates_in_order() {
        // A Step carrying every Gate kind projects to just its exit-code
        // outcomes: the Code Gates by their decimal code and Default as "*",
        // each with its `when`, in declaration order. EXHAUSTED and FAULT are
        // not exit-code outcomes and are dropped.
        let step = Step {
            body: StepBody::Command("noop".into()),
            budget: None,
            gates: vec![
                gate(GateKey::Code(0), Some("approve")),
                gate(GateKey::Code(1), Some("revise")),
                gate(GateKey::Default, Some("escalate")),
                gate(GateKey::Exhausted, None),
                gate(GateKey::Fault, None),
            ],
        };

        let contract = RoutingContract::from_step(&step);

        let pairs: Vec<(&str, Option<&str>)> = contract
            .iter()
            .map(|entry| (entry.key.as_str(), entry.when.as_deref()))
            .collect();
        assert_eq!(
            pairs,
            vec![
                ("0", Some("approve")),
                ("1", Some("revise")),
                ("*", Some("escalate")),
            ]
        );
    }
}
