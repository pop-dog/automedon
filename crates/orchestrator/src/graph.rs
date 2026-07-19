//! The `automedon graph` Mermaid export: a pure function over an
//! already-loaded [`Registry`] that renders its full topology as one Mermaid
//! flowchart, so it needs no Kernel Run to unit test — the same shape as
//! `plan::describe`.

use kernel::{GateKey, GateTarget, Registry, StepBody};

use crate::display_id::display_id;

/// Render `registry` as a single Mermaid `flowchart` with one `subgraph` per
/// Workflow. Subgraph titles and node ids use each Workflow's display id (see
/// [`crate::display_id`]), never the loader's absolute-path-bearing id, so no
/// machine-specific path reaches a README or PR pasted from this output.
pub fn render(registry: &Registry) -> String {
    let mut out = String::from("flowchart TD\n");
    let mut ids: Vec<&String> = registry.workflows.keys().collect();
    ids.sort();
    for id in ids {
        let workflow = &registry.workflows[id];
        let display = display_id(id, &registry.root);
        out.push_str(&format!(
            "  subgraph {}[\"{}\"]\n",
            subgraph_id(&display),
            escape(&display)
        ));
        let mut names: Vec<&String> = workflow.steps.keys().collect();
        names.sort();
        for name in &names {
            out.push_str(&format!(
                "    {}[\"{}\"]\n",
                step_node_id(&display, name),
                escape(name)
            ));
        }
        let mut exit_codes: Vec<i32> = names
            .iter()
            .flat_map(|name| &workflow.steps[*name].gates)
            .filter_map(|gate| match &gate.target {
                GateTarget::Exit(code) => Some(*code),
                GateTarget::Step(_) => None,
            })
            .collect();
        exit_codes.sort_unstable();
        exit_codes.dedup();
        for code in &exit_codes {
            out.push_str(&format!(
                "    {}((\"exit {code}\"))\n",
                exit_node_id(&display, *code)
            ));
        }
        out.push_str("  end\n");
        for name in names {
            let step = &workflow.steps[name];
            for gate in &step.gates {
                let (target, label) = match &gate.target {
                    GateTarget::Step(target) => (step_node_id(&display, target), edge_label(gate)),
                    GateTarget::Exit(code) => (exit_node_id(&display, *code), edge_label(gate)),
                };
                out.push_str(&format!(
                    "  {} -->|\"{}\"| {}\n",
                    step_node_id(&display, name),
                    escape(&label),
                    target,
                ));
            }
            // A Composite Step hands off to a child Workflow's own entry
            // Step; a dashed edge distinguishes that handoff from the
            // Gate-driven edges above, whose Gates already show how the
            // child's surfaced codes route back in the parent.
            if let StepBody::Workflow(child_id) = &step.body {
                let child_display = display_id(child_id, &registry.root);
                let child_entry = &registry.workflows[child_id].entry;
                out.push_str(&format!(
                    "  {} -.-> {}\n",
                    step_node_id(&display, name),
                    step_node_id(&child_display, child_entry),
                ));
            }
        }
    }
    out
}

/// The text an edge's Mermaid label carries: the Gate key in the YAML
/// vocabulary, plus the full `when:` annotation when present. No truncation —
/// the payoff surface is a README/PR where that annotation text is the point.
fn edge_label(gate: &kernel::Gate) -> String {
    let key = gate_key_str(&gate.key);
    match &gate.when {
        Some(when) => format!("{key}: {when}"),
        None => key,
    }
}

/// The Gate key rendered as it appears in Workflow YAML, not its Rust debug
/// name (`GateKey::Code(0)` -> `0`, `Default` -> `*`, etc.), so the diagram
/// reads in the vocabulary an operator already knows from the source file.
/// Shared with the `run --dry-run` plan, which speaks to the same operator.
pub(crate) fn gate_key_str(key: &GateKey) -> String {
    match key {
        GateKey::Code(n) => n.to_string(),
        GateKey::Default => "*".to_string(),
        GateKey::Exhausted => "EXHAUSTED".to_string(),
        GateKey::Fault => "FAULT".to_string(),
    }
}

/// A Mermaid-safe identifier for the subgraph representing Workflow `id`.
/// Mermaid ids must avoid characters like `-` that a bare identifier can't
/// contain, so non-alphanumeric characters are collapsed to `_`.
fn subgraph_id(id: &str) -> String {
    format!("wf_{}", sanitize(id))
}

/// A Mermaid-safe identifier for the node representing Step `step` in
/// Workflow `workflow_id`. Namespaced by Workflow id because Step names are
/// local to their Workflow and can repeat across the registry.
fn step_node_id(workflow_id: &str, step: &str) -> String {
    format!("wf_{}_{}", sanitize(workflow_id), sanitize(step))
}

/// A Mermaid-safe identifier for the terminal node representing exit code
/// `code` within Workflow `workflow_id`. Exit nodes render per subgraph — the
/// same numeric code exiting two different Workflows earns two distinct
/// nodes, since they terminate different Runs of the graph.
fn exit_node_id(workflow_id: &str, code: i32) -> String {
    format!("wf_{}_exit_{}", sanitize(workflow_id), sanitize(&code.to_string()))
}

fn sanitize(s: &str) -> String {
    s.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '_' }).collect()
}

/// Escape text destined for a quoted Mermaid label. Mermaid labels are
/// wrapped in `"..."`; a literal `"` inside must be replaced with its HTML
/// entity or it would terminate the label early.
fn escape(s: &str) -> String {
    s.replace('"', "#quot;")
}

#[cfg(test)]
mod tests {
    use super::render;
    use kernel::Registry;

    fn registry(yaml: &str) -> Registry {
        serde_yaml::from_str(yaml).unwrap()
    }

    #[test]
    fn render_escapes_quotes_in_edge_labels() {
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
          - { key: 0, target: { exit: 0 }, when: "the \"build\" step -> failed" }
"#,
        );
        let text = render(&reg);
        assert!(
            text.contains("|\"0: the #quot;build#quot; step -> failed\"|"),
            "{text}"
        );
    }

    #[test]
    fn render_uses_the_yaml_vocabulary_for_every_gate_key() {
        let reg = registry(
            r#"
root: main
workflows:
  main:
    entry: a
    steps:
      a:
        command: "exit 0"
        budget: 1
        gates:
          - { key: 0, target: { exit: 0 } }
          - { key: -1, target: { exit: 1 } }
          - { key: "*", target: { exit: 2 } }
          - { key: "EXHAUSTED", target: { exit: 3 } }
          - { key: "FAULT", target: { exit: 4 } }
"#,
        );
        let text = render(&reg);
        assert!(text.contains("|\"0\"|"), "{text}");
        assert!(text.contains("|\"-1\"|"), "{text}");
        assert!(text.contains("|\"*\"|"), "{text}");
        assert!(text.contains("|\"EXHAUSTED\"|"), "{text}");
        assert!(text.contains("|\"FAULT\"|"), "{text}");
    }

    #[test]
    fn render_dashes_a_composite_step_into_the_childs_entry_step() {
        let reg = registry(
            r#"
root: main
workflows:
  main:
    entry: call
    steps:
      call:
        workflow: child
        gates:
          - { key: 0, target: { exit: 0 } }
  child:
    entry: work
    steps:
      work:
        command: "exit 0"
        gates:
          - { key: 0, target: { exit: 0 } }
"#,
        );
        let text = render(&reg);
        assert!(text.contains("wf_main_call -.-> wf_child_work"), "{text}");
    }

    #[test]
    fn render_emits_a_terminal_node_per_exit_code_and_an_edge_into_it() {
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
        let text = render(&reg);
        assert!(text.contains("wf_main_exit_0((\"exit 0\"))"), "{text}");
        assert!(text.contains("wf_main_a -->|\"0\"| wf_main_exit_0"), "{text}");
    }

    #[test]
    fn render_emits_an_edge_per_step_gate_with_the_key_and_when_text() {
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
          - { key: 0, target: { step: b }, when: "build succeeded" }
      b:
        command: "exit 0"
        gates:
          - { key: 0, target: { exit: 0 } }
"#,
        );
        let text = render(&reg);
        assert!(
            text.contains("wf_main_a -->|\"0: build succeeded\"| wf_main_b"),
            "{text}"
        );
    }

    #[test]
    fn render_emits_a_node_per_step() {
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
        let text = render(&reg);
        assert!(text.contains("wf_main_a[\"a\"]"), "{text}");
    }

    #[test]
    fn render_emits_one_subgraph_per_workflow() {
        let reg = registry(
            r#"
root: main
workflows:
  main:
    entry: call
    steps:
      call:
        workflow: child
        gates:
          - { key: 0, target: { exit: 0 } }
  child:
    entry: work
    steps:
      work:
        command: "exit 0"
        gates:
          - { key: 0, target: { exit: 0 } }
"#,
        );
        let text = render(&reg);
        assert!(text.starts_with("flowchart TD\n"), "{text}");
        assert!(text.contains("subgraph wf_main[\"main\"]"), "{text}");
        assert!(text.contains("subgraph wf_child[\"child\"]"), "{text}");
    }
}
