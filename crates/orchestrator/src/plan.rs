//! The `run --dry-run` execution plan: the topology `automedon run --dry-run`
//! prints, and the worst-case Step activation count it implies. Pure
//! functions over an already-loaded [`Registry`] so they need no Kernel Run to
//! unit test.

use kernel::{GateTarget, Registry, Workflow, DEFAULT_BUDGET};

/// The effective Budget for `step` in `workflow`, after the cascade (Step
/// value -> Workflow `default_budget` -> [`DEFAULT_BUDGET`]).
fn effective_budget(workflow: &Workflow, step: &kernel::Step) -> u32 {
    step.budget.unwrap_or_else(|| workflow.default_budget.unwrap_or(DEFAULT_BUDGET))
}

/// The worst-case total Step activation count across the registry: the sum,
/// over every Step in every Workflow, of that Step's effective Budget — the
/// most times it could run before its own Budget forces a Gate decision.
pub fn worst_case_activations(registry: &Registry) -> u64 {
    registry
        .workflows
        .values()
        .flat_map(|workflow| workflow.steps.values().map(|step| effective_budget(workflow, step) as u64))
        .sum()
}

/// A human-readable rendering of the registry's topology: every Workflow's
/// Steps, each Step's Gates with their `when` annotations, and effective
/// Budgets — everything `run --dry-run` shows in place of executing.
pub fn describe(registry: &Registry) -> String {
    let mut out = String::new();
    let mut ids: Vec<&String> = registry.workflows.keys().collect();
    ids.sort();
    for id in ids {
        let workflow = &registry.workflows[id];
        out.push_str(&format!("workflow {id} (entry: {})\n", workflow.entry));
        let mut names: Vec<&String> = workflow.steps.keys().collect();
        names.sort();
        for name in names {
            let step = &workflow.steps[name];
            out.push_str(&format!("  step {name} (budget: {})\n", effective_budget(workflow, step)));
            for gate in &step.gates {
                let target = match &gate.target {
                    GateTarget::Step(s) => format!("step {s}"),
                    GateTarget::Exit(c) => format!("exit {c}"),
                };
                match &gate.when {
                    Some(when) => out.push_str(&format!("    {:?} -> {target}  # {when}\n", gate.key)),
                    None => out.push_str(&format!("    {:?} -> {target}\n", gate.key)),
                }
            }
        }
    }
    out.push_str(&format!("worst-case activations: {}\n", worst_case_activations(registry)));
    out
}

#[cfg(test)]
mod tests {
    use super::{describe, worst_case_activations};
    use kernel::Registry;

    fn registry(yaml: &str) -> Registry {
        serde_yaml::from_str(yaml).unwrap()
    }

    #[test]
    fn worst_case_sums_effective_budgets_across_the_cascade() {
        // `a` has an explicit Budget of 3; `b` falls back to the Workflow
        // `default_budget` of 4. Worst case is their sum: 7.
        let reg = registry(
            r#"
root: main
workflows:
  main:
    entry: a
    default_budget: 4
    steps:
      a:
        command: "exit 0"
        budget: 3
        gates:
          - { key: 0, target: { step: b } }
      b:
        command: "exit 0"
        gates:
          - { key: 0, target: { exit: 0 } }
"#,
        );
        assert_eq!(worst_case_activations(&reg), 7);
    }

    #[test]
    fn worst_case_falls_back_to_the_hardcoded_default_with_no_workflow_override() {
        let reg = registry(
            r#"
root: main
workflows:
  main:
    entry: a
    steps:
      a:
        command: "exit 0"
        gates:
          - { key: 0, target: { exit: 0 } }
"#,
        );
        assert_eq!(worst_case_activations(&reg), u64::from(kernel::DEFAULT_BUDGET));
    }

    #[test]
    fn describe_names_every_step_gate_and_the_worst_case_total() {
        let reg = registry(
            r#"
root: main
workflows:
  main:
    entry: a
    steps:
      a:
        command: "exit 0"
        budget: 2
        gates:
          - { key: 0, target: { step: a }, when: "retry" }
          - { key: 1, target: { exit: 0 } }
"#,
        );
        let text = describe(&reg);
        assert!(text.contains("step a (budget: 2)"), "{text}");
        assert!(text.contains("retry"), "{text}");
        assert!(text.contains("worst-case activations: 2"), "{text}");
    }
}
