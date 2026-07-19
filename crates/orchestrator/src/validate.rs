//! Static analysis of a loaded [`Registry`]: every check `automedon validate`
//! reports without executing a Step. Runs after `loader::load`, so dangling
//! Composite references, a missing `root`, and a missing `entry` already
//! surfaced as load errors; this module covers the checks that need the fully
//! assembled graph.

use std::collections::{HashMap, HashSet};

use kernel::{Gate, GateKey, GateTarget, Registry, Workflow};

/// Run every static check against `registry`, returning one message per
/// problem found. Collects every problem in one pass rather than stopping at
/// the first, so an operator fixes a Workflow in one round trip.
pub fn check(registry: &Registry) -> Vec<String> {
    let mut problems = Vec::new();
    for (id, workflow) in &registry.workflows {
        dangling_gate_targets(id, workflow, &mut problems);
        duplicate_gate_keys(id, workflow, &mut problems);
        unreachable_steps(id, workflow, &mut problems);
        unescaped_budgeted_loops(id, workflow, &mut problems);
    }
    problems
}

/// A Gate `target: { step: … }` naming a Step absent from this Workflow.
fn dangling_gate_targets(id: &str, workflow: &Workflow, problems: &mut Vec<String>) {
    for (step_name, step) in &workflow.steps {
        for gate in &step.gates {
            if let GateTarget::Step(target) = &gate.target {
                if !workflow.steps.contains_key(target) {
                    problems.push(format!(
                        "{id}: step `{step_name}` has a gate targeting undefined step `{target}`"
                    ));
                }
            }
        }
    }
}

/// Two Gates on the same Step sharing a `GateKey`. Each key unlocks at most one
/// Gate, so a duplicate is dead or ambiguous routing. `GateKey` has no `Eq`
/// (only `PartialEq`), so this compares pairwise rather than hashing.
fn duplicate_gate_keys(id: &str, workflow: &Workflow, problems: &mut Vec<String>) {
    for (step_name, step) in &workflow.steps {
        for (i, gate) in step.gates.iter().enumerate() {
            if step.gates[..i].iter().any(|g| g.key == gate.key) {
                problems.push(format!(
                    "{id}: step `{step_name}` has duplicate gates for key {:?}",
                    gate.key
                ));
            }
        }
    }
}

/// Steps not reachable from `entry` by following `Step` gate targets. An
/// `Exit` target terminates the Workflow rather than naming a Step, so it
/// contributes no edge.
fn unreachable_steps(id: &str, workflow: &Workflow, problems: &mut Vec<String>) {
    let reachable = reachable_from(workflow, &workflow.entry);
    let mut names: Vec<&String> = workflow
        .steps
        .keys()
        .filter(|name| !reachable.contains(name.as_str()))
        .collect();
    names.sort();
    for name in names {
        problems.push(format!("{id}: step `{name}` is unreachable from entry `{}`", workflow.entry));
    }
}

/// Every Step reachable from `start` (inclusive), following `Step` gate
/// targets. Dangling targets are skipped here; `dangling_gate_targets` reports
/// them separately.
fn reachable_from(workflow: &Workflow, start: &str) -> HashSet<String> {
    let mut seen = HashSet::new();
    let mut stack = vec![start.to_string()];
    while let Some(name) = stack.pop() {
        if !seen.insert(name.clone()) {
            continue;
        }
        if let Some(step) = workflow.steps.get(&name) {
            for gate in &step.gates {
                if let GateTarget::Step(target) = &gate.target {
                    if workflow.steps.contains_key(target) {
                        stack.push(target.clone());
                    }
                }
            }
        }
    }
    seen
}

/// A cycle among Steps (via `Step` gate targets) none of whose members has an
/// `EXHAUSTED` Gate. Every Step's Budget is finite (the cascade always
/// bottoms out at the default), so the loop cannot spin forever — the Kernel
/// instead raises a Fault when a spent Budget has no `EXHAUSTED` Gate to
/// take. This check surfaces at validate time the loop that would otherwise
/// fault mid-Run.
fn unescaped_budgeted_loops(id: &str, workflow: &Workflow, problems: &mut Vec<String>) {
    for scc in strongly_connected_components(workflow) {
        if scc.len() < 2 && !self_loop(workflow, &scc[0]) {
            continue;
        }
        let escapes = scc.iter().any(|name| {
            workflow.steps[name]
                .gates
                .iter()
                .any(|g: &Gate| g.key == GateKey::Exhausted)
        });
        if !escapes {
            let mut members = scc.clone();
            members.sort();
            problems.push(format!(
                "{id}: budgeted loop {{{}}} has no reachable EXHAUSTED gate",
                members.join(", ")
            ));
        }
    }
}

fn self_loop(workflow: &Workflow, name: &str) -> bool {
    workflow.steps[name].gates.iter().any(|g| matches!(&g.target, GateTarget::Step(t) if t == name))
}

/// Tarjan's algorithm over the local Step graph (`Step` gate targets only).
/// Returns each strongly connected component (size >= 1); a size-1 component
/// is a cycle only if it is also a self-loop, checked separately by callers.
fn strongly_connected_components(workflow: &Workflow) -> Vec<Vec<String>> {
    struct State<'a> {
        workflow: &'a Workflow,
        index: HashMap<String, usize>,
        lowlink: HashMap<String, usize>,
        on_stack: HashSet<String>,
        stack: Vec<String>,
        next_index: usize,
        sccs: Vec<Vec<String>>,
    }

    fn strongconnect(name: &str, state: &mut State) {
        state.index.insert(name.to_string(), state.next_index);
        state.lowlink.insert(name.to_string(), state.next_index);
        state.next_index += 1;
        state.stack.push(name.to_string());
        state.on_stack.insert(name.to_string());

        if let Some(step) = state.workflow.steps.get(name) {
            let targets: Vec<String> = step
                .gates
                .iter()
                .filter_map(|g| match &g.target {
                    GateTarget::Step(t) if state.workflow.steps.contains_key(t) => Some(t.clone()),
                    _ => None,
                })
                .collect();
            for target in targets {
                if !state.index.contains_key(&target) {
                    strongconnect(&target, state);
                    let low = state.lowlink[&target].min(state.lowlink[name]);
                    state.lowlink.insert(name.to_string(), low);
                } else if state.on_stack.contains(&target) {
                    let low = state.index[&target].min(state.lowlink[name]);
                    state.lowlink.insert(name.to_string(), low);
                }
            }
        }

        if state.lowlink[name] == state.index[name] {
            let mut component = Vec::new();
            loop {
                let member = state.stack.pop().unwrap();
                state.on_stack.remove(&member);
                let done = member == name;
                component.push(member);
                if done {
                    break;
                }
            }
            state.sccs.push(component);
        }
    }

    let mut state = State {
        workflow,
        index: HashMap::new(),
        lowlink: HashMap::new(),
        on_stack: HashSet::new(),
        stack: Vec::new(),
        next_index: 0,
        sccs: Vec::new(),
    };
    let mut names: Vec<&String> = workflow.steps.keys().collect();
    names.sort();
    for name in names {
        if !state.index.contains_key(name) {
            strongconnect(name, &mut state);
        }
    }
    state.sccs
}

#[cfg(test)]
mod tests {
    use super::check;
    use kernel::Registry;

    fn registry(yaml: &str) -> Registry {
        serde_yaml::from_str(yaml).unwrap()
    }

    fn single_workflow(steps_yaml: &str) -> Registry {
        registry(&format!(
            "root: main\nworkflows:\n  main:\n    entry: a\n    steps:\n{steps_yaml}"
        ))
    }

    #[test]
    fn a_clean_workflow_has_no_problems() {
        let reg = single_workflow(
            r#"      a:
        command: "exit 0"
        gates:
          - { key: 0, target: { exit: 0 } }
"#,
        );
        assert!(check(&reg).is_empty());
    }

    #[test]
    fn a_gate_targeting_an_undefined_step_is_reported() {
        let reg = single_workflow(
            r#"      a:
        command: "exit 0"
        gates:
          - { key: 0, target: { step: ghost } }
"#,
        );
        let problems = check(&reg);
        assert!(
            problems.iter().any(|p| p.contains("ghost")),
            "expected a problem naming the dangling target: {problems:?}"
        );
    }

    #[test]
    fn duplicate_gate_keys_on_one_step_are_reported() {
        let reg = single_workflow(
            r#"      a:
        command: "exit 0"
        gates:
          - { key: 0, target: { exit: 0 } }
          - { key: 0, target: { exit: 1 } }
"#,
        );
        let problems = check(&reg);
        assert!(
            problems.iter().any(|p| p.contains("duplicate")),
            "expected a duplicate-key problem: {problems:?}"
        );
    }

    #[test]
    fn a_step_unreachable_from_entry_is_reported() {
        let reg = single_workflow(
            r#"      a:
        command: "exit 0"
        gates:
          - { key: 0, target: { exit: 0 } }
      orphan:
        command: "exit 0"
        gates:
          - { key: 0, target: { exit: 0 } }
"#,
        );
        let problems = check(&reg);
        assert!(
            problems.iter().any(|p| p.contains("orphan") && p.contains("unreachable")),
            "expected an unreachable-step problem: {problems:?}"
        );
    }

    #[test]
    fn a_budgeted_self_loop_with_no_exhausted_gate_is_reported() {
        let reg = single_workflow(
            r#"      a:
        command: "exit 0"
        budget: 3
        gates:
          - { key: 1, target: { step: a } }
          - { key: 0, target: { exit: 0 } }
"#,
        );
        let problems = check(&reg);
        assert!(
            problems.iter().any(|p| p.contains("EXHAUSTED")),
            "expected an unescaped-loop problem: {problems:?}"
        );
    }

    #[test]
    fn a_budgeted_loop_with_an_exhausted_gate_is_not_reported() {
        let reg = single_workflow(
            r#"      a:
        command: "exit 0"
        budget: 3
        gates:
          - { key: 1, target: { step: a } }
          - { key: 0, target: { exit: 0 } }
          - { key: "EXHAUSTED", target: { exit: 90 } }
"#,
        );
        assert!(check(&reg).is_empty());
    }

    #[test]
    fn a_two_step_cycle_without_an_exhausted_gate_is_reported() {
        let reg = single_workflow(
            r#"      a:
        command: "exit 0"
        budget: 3
        gates:
          - { key: 0, target: { step: b } }
      b:
        command: "exit 0"
        budget: 3
        gates:
          - { key: 1, target: { step: a } }
          - { key: 0, target: { exit: 0 } }
"#,
        );
        let problems = check(&reg);
        assert!(
            problems.iter().any(|p| p.contains("EXHAUSTED")),
            "expected an unescaped-loop problem naming the cycle: {problems:?}"
        );
    }
}
