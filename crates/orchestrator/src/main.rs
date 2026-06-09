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
