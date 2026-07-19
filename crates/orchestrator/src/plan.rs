//! The `run --dry-run` execution plan: the topology `automedon run --dry-run`
//! prints, and the worst-case Step activation count it implies. Pure
//! functions over an already-loaded [`Registry`] so they need no Kernel Run to
//! unit test.

use kernel::{GateTarget, Registry, StepBody, Workflow, DEFAULT_BUDGET};

/// The effective Budget for `step` in `workflow`, after the cascade (Step
/// value -> Workflow `default_budget` -> [`DEFAULT_BUDGET`]).
fn effective_budget(workflow: &Workflow, step: &kernel::Step) -> u32 {
    step.budget.unwrap_or_else(|| workflow.default_budget.unwrap_or(DEFAULT_BUDGET))
}

/// The worst-case total Step activation count for a Run of the registry's
/// root Workflow. A Step contributes its effective Budget per Frame — but a
/// Composite Step pushes a *fresh* Frame (fresh child Budgets) on every
/// activation, so budgets multiply along the Composite tree rather than sum.
/// `None` means unbounded: a Composite cycle re-enters a Workflow already on
/// the path, a recursion the run-time Depth cap bounds, not any Budget.
pub fn worst_case_activations(registry: &Registry) -> Option<u64> {
    worst_case_of(registry, &registry.root, &mut Vec::new())
}

/// The worst case for one Workflow, with `path` holding the Workflow ids of
/// the Frames above it to detect Composite cycles. Saturating arithmetic: an
/// astronomically-budgeted plan should print `u64::MAX`, not wrap.
fn worst_case_of(registry: &Registry, id: &str, path: &mut Vec<String>) -> Option<u64> {
    if path.iter().any(|above| above == id) {
        return None;
    }
    path.push(id.to_string());
    let workflow = &registry.workflows[id];
    let mut total: u64 = 0;
    for step in workflow.steps.values() {
        let per_activation = match &step.body {
            StepBody::Command(_) => 1,
            StepBody::Workflow(child) => {
                let Some(child_worst) = worst_case_of(registry, child, path) else {
                    path.pop();
                    return None;
                };
                1u64.saturating_add(child_worst)
            }
        };
        let budget = u64::from(effective_budget(workflow, step));
        total = total.saturating_add(budget.saturating_mul(per_activation));
    }
    path.pop();
    Some(total)
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
    match worst_case_activations(registry) {
        Some(n) => out.push_str(&format!("worst-case activations: {n}\n")),
        None => out.push_str(
            "worst-case activations: unbounded (recursive Composite; the run-time Depth cap bounds it)\n",
        ),
    }
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
        assert_eq!(worst_case_activations(&reg), Some(7));
    }

    #[test]
    fn worst_case_multiplies_budgets_through_a_composite() {
        // The composite `outer` step may activate twice, and each activation
        // runs the whole child Workflow afresh: 2 * (1 + (3 + 4)) = 16.
        let reg = registry(
            r#"
root: main
workflows:
  main:
    entry: outer
    steps:
      outer:
        workflow: child
        budget: 2
        gates:
          - { key: 0, target: { exit: 0 } }
  child:
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
        assert_eq!(worst_case_activations(&reg), Some(16));
    }

    #[test]
    fn worst_case_is_unbounded_when_composites_recurse() {
        // `main` re-enters itself through a Composite: no Budget bounds that
        // (only the run-time Depth cap does), so there is no finite worst case.
        let reg = registry(
            r#"
root: main
workflows:
  main:
    entry: again
    steps:
      again:
        workflow: main
        gates:
          - { key: 0, target: { exit: 0 } }
"#,
        );
        assert_eq!(worst_case_activations(&reg), None);
        assert!(
            describe(&reg).contains("unbounded"),
            "describe should say the worst case is unbounded"
        );
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
        assert_eq!(worst_case_activations(&reg), Some(u64::from(kernel::DEFAULT_BUDGET)));
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
