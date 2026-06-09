//! Driver: wires a YAML WorkflowSource and a console Sink into the Kernel.
//! Usage: `orchestrator <workflow.yaml>`.

use std::path::PathBuf;

use kernel::{Event, Fault, GateKey, GateTarget, Sink, Workflow, WorkflowSource};

/// A `WorkflowSource` that parses a Workflow IR from YAML.
struct YamlSource {
    path: PathBuf,
}

impl WorkflowSource for YamlSource {
    fn load(&self) -> Result<Workflow, Box<dyn std::error::Error>> {
        let text = std::fs::read_to_string(&self.path)?;
        let workflow: Workflow = serde_yaml::from_str(&text)?;
        Ok(workflow)
    }
}

// ANSI helpers, to avoid a styling-crate dependency.
const B: &str = "\x1b[1m"; // bold
const D: &str = "\x1b[2m"; // dim
const R: &str = "\x1b[0m"; // reset

/// A trace Sink: one line per Kernel transition.
struct ConsoleSink;

impl Sink for ConsoleSink {
    fn emit(&mut self, event: &Event) {
        println!("{}", render(event));
    }
}

fn render(event: &Event) -> String {
    match event {
        Event::RunStarted => format!("{B}● RUN started{R}"),
        Event::StepEntered { step } => format!("  {B}▶ enter{R} {step}"),
        Event::StepExited { step, code } => format!("  {D}■ exit{R}  {step} {D}->{R} code {B}{code}{R}"),
        Event::BudgetConsumed { step, remaining } => {
            format!("  {D}· budget {step}: {remaining} left{R}")
        }
        Event::Exhausted { step } => format!("  {B}⊘ EXHAUSTED{R} {step} {D}(budget spent){R}"),
        Event::GateTaken { step, key, target } => {
            format!("  {D}↳ gate{R} {step} {D}[{}]{R} {D}->{R} {}", fmt_key(key), fmt_target(target))
        }
        Event::MessagePassed { from, to, bytes } => {
            format!("  {D}✉ message {from} -> {to} ({bytes} bytes){R}")
        }
        Event::FaultRaised { fault } => format!("  {B}✗ FAULT{R} {}", fmt_fault(fault)),
        Event::RunEnded { code } => format!("{B}◆ RUN ended -> exit {code}{R}"),
    }
}

fn fmt_key(key: &GateKey) -> String {
    match key {
        GateKey::Code(n) => n.to_string(),
        GateKey::Default => "*".to_string(),
        GateKey::Exhausted => "EXHAUSTED".to_string(),
        GateKey::Fault => "FAULT".to_string(),
    }
}

fn fmt_target(target: &GateTarget) -> String {
    match target {
        GateTarget::Step(s) => s.clone(),
        GateTarget::Exit(code) => format!("EXIT {code}"),
    }
}

fn fmt_fault(fault: &Fault) -> String {
    match fault {
        Fault::UnhandledOutcome { step, code } => {
            format!("unhandled outcome: {step} exited {code} with no matching Gate")
        }
        Fault::UnhandledExhaustion { step } => {
            format!("unhandled exhaustion: {step} spent its Budget with no EXHAUSTED Gate")
        }
        Fault::DepthOverflow => "depth overflow".to_string(),
    }
}

fn main() {
    let path = match std::env::args().nth(1) {
        Some(p) => PathBuf::from(p),
        None => {
            eprintln!("usage: orchestrator <workflow.yaml>");
            std::process::exit(2);
        }
    };

    let source = YamlSource { path };
    let workflow = source.load().unwrap_or_else(|e| {
        eprintln!("failed to load workflow: {e}");
        std::process::exit(2);
    });

    let mut sink = ConsoleSink;
    match kernel::run(&workflow, &mut sink) {
        Ok(code) => std::process::exit(code),
        // A Fault is not an exit code; surface it on a distinct status (sysexits EX_SOFTWARE).
        Err(_) => std::process::exit(70),
    }
}

// Tests for the YAML front-end live here (where the format dependency lives), not
// in the format-agnostic Kernel.
#[cfg(test)]
mod tests {
    use kernel::{GateKey, GateTarget, Workflow};

    #[test]
    fn parses_every_gate_key_form() {
        assert_eq!(serde_yaml::from_str::<GateKey>("0").unwrap(), GateKey::Code(0));
        assert_eq!(serde_yaml::from_str::<GateKey>("-1").unwrap(), GateKey::Code(-1));
        assert_eq!(serde_yaml::from_str::<GateKey>("'*'").unwrap(), GateKey::Default);
        assert_eq!(serde_yaml::from_str::<GateKey>("EXHAUSTED").unwrap(), GateKey::Exhausted);
        assert_eq!(serde_yaml::from_str::<GateKey>("FAULT").unwrap(), GateKey::Fault);
    }

    #[test]
    fn rejects_unknown_gate_key_string() {
        assert!(serde_yaml::from_str::<GateKey>("banana").is_err());
    }

    #[test]
    fn parses_both_gate_target_forms() {
        match serde_yaml::from_str::<GateTarget>("{ step: retry }").unwrap() {
            GateTarget::Step(s) => assert_eq!(s, "retry"),
            other => panic!("expected Step, got {other:?}"),
        }
        match serde_yaml::from_str::<GateTarget>("{ exit: 0 }").unwrap() {
            GateTarget::Exit(c) => assert_eq!(c, 0),
            other => panic!("expected Exit, got {other:?}"),
        }
    }

    #[test]
    fn rejects_ambiguous_or_empty_gate_target() {
        // Both keys set is ambiguous; neither set is empty. Both must fail.
        assert!(serde_yaml::from_str::<GateTarget>("{ step: a, exit: 0 }").is_err());
        assert!(serde_yaml::from_str::<GateTarget>("{}").is_err());
    }

    #[test]
    fn parses_a_whole_workflow() {
        let yaml = r#"
entry: a
default_budget: 4
steps:
  a:
    command: "exit 0"
    budget: 2
    gates:
      - { key: 0, target: { exit: 0 } }
      - { key: "*", target: { step: a } }
"#;
        let wf: Workflow = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(wf.entry, "a");
        assert_eq!(wf.default_budget, Some(4));
        let a = &wf.steps["a"];
        assert_eq!(a.budget, Some(2));
        assert_eq!(a.gates.len(), 2);
    }
}
