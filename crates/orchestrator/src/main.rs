//! Driver: wires a YAML WorkflowSource and a console Sink into the Kernel.
//! Usage: `orchestrator <workflow.yaml> [--message <text>]`.
//!
//! The initial Message seeds the entry Step. It comes from `--message` or piped
//! stdin (the flag wins); with no `--message` and nothing piped, it is empty.

use std::io::Read;
use std::path::{Path, PathBuf};

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

/// Resolve the Run's initial Message from the two CLI input channels. The
/// `--message` flag wins over piped stdin; with neither present the Message is
/// empty, preserving a Run invoked without arguments. Kept pure (no argv/stdin
/// access) so the precedence rules are unit-testable.
fn resolve_initial_message(flag: Option<String>, stdin: Option<Vec<u8>>) -> Vec<u8> {
    match (flag, stdin) {
        (Some(text), _) => text.into_bytes(),
        (None, Some(bytes)) => bytes,
        (None, None) => Vec::new(),
    }
}

/// Pull the value of `--message <text>` out of the argument list, if present.
fn message_flag(args: &[String]) -> Option<String> {
    args.iter()
        .position(|a| a == "--message")
        .and_then(|i| args.get(i + 1).cloned())
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();

    let path = match args.first() {
        Some(p) => PathBuf::from(p),
        None => {
            eprintln!("usage: orchestrator <workflow.yaml> [--message <text>]");
            std::process::exit(2);
        }
    };

    // Expose the workflow definition's directory as $WORKFLOW_DIR so a Step can
    // locate the scripts it names (e.g. `command: "$WORKFLOW_DIR/fetch.sh"`)
    // independently of the working directory. This decouples *where the scripts
    // live* (the workflow repo) from *where the work happens*: the cwd is left as
    // the target project root — the repo a Step reads, edits, and commits — so a
    // workflow and the repo it operates on need not be the same directory. The
    // child Steps inherit this variable through the kernel's plain `sh -c` spawn.
    let dir = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let workflow_dir = std::fs::canonicalize(dir).unwrap_or_else(|_| dir.to_path_buf());
    std::env::set_var("WORKFLOW_DIR", &workflow_dir);

    let source = YamlSource { path };
    let workflow = source.load().unwrap_or_else(|e| {
        eprintln!("failed to load workflow: {e}");
        std::process::exit(2);
    });

    // Read stdin only when it is piped or redirected; a terminal stdin would
    // block on an end-of-file that never arrives.
    let piped_stdin = if std::io::IsTerminal::is_terminal(&std::io::stdin()) {
        None
    } else {
        let mut buf = Vec::new();
        std::io::stdin().read_to_end(&mut buf).ok().map(|_| buf)
    };

    let initial_message = resolve_initial_message(message_flag(&args), piped_stdin);

    let mut sink = ConsoleSink;
    match kernel::run(&workflow, &initial_message, &mut sink) {
        Ok(code) => std::process::exit(code),
        // A Fault is not an exit code; surface it on a distinct status (sysexits EX_SOFTWARE).
        Err(_) => std::process::exit(70),
    }
}

// Tests for the YAML front-end live here (where the format dependency lives), not
// in the format-agnostic Kernel.
#[cfg(test)]
mod tests {
    use super::{message_flag, resolve_initial_message};
    use kernel::{GateKey, GateTarget, Workflow};

    fn args(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn message_flag_reads_the_following_value() {
        let got = message_flag(&args(&["wf.yaml", "--message", "hello"]));
        assert_eq!(got.as_deref(), Some("hello"));
    }

    #[test]
    fn message_flag_absent_yields_none() {
        assert_eq!(message_flag(&args(&["wf.yaml"])), None);
    }

    #[test]
    fn dangling_message_flag_yields_none() {
        // A trailing `--message` with no value is treated as absent, falling back
        // to stdin or the empty default rather than erroring.
        assert_eq!(message_flag(&args(&["wf.yaml", "--message"])), None);
    }

    #[test]
    fn flag_wins_over_stdin() {
        let got = resolve_initial_message(Some("from-flag".into()), Some(b"from-stdin".to_vec()));
        assert_eq!(got, b"from-flag");
    }

    #[test]
    fn stdin_used_when_flag_absent() {
        let got = resolve_initial_message(None, Some(b"from-stdin".to_vec()));
        assert_eq!(got, b"from-stdin");
    }

    #[test]
    fn empty_when_both_absent() {
        let got = resolve_initial_message(None, None);
        assert!(got.is_empty());
    }

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

    // Guards the shipped coder example: parsing it and asserting its routing
    // keeps the file honest without invoking the LLM Steps it names.
    #[test]
    fn coder_example_wires_the_review_loop() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../examples/coder.yaml");
        let text = std::fs::read_to_string(path).unwrap();
        let wf: Workflow = serde_yaml::from_str(&text).unwrap();

        // Resolve the target a Step routes to for a given Gate key.
        let target = |step: &str, key: GateKey| -> GateTarget {
            wf.steps[step]
                .gates
                .iter()
                .find(|g| g.key == key)
                .unwrap_or_else(|| panic!("{step} has no gate for {key:?}"))
                .target
                .clone()
        };
        let routes_to_step = |t: GateTarget, name: &str| matches!(t, GateTarget::Step(s) if s == name);
        let exits_with = |t: GateTarget, code: i32| matches!(t, GateTarget::Exit(c) if c == code);

        assert_eq!(wf.entry, "code");
        // The loop is bounded by code's Budget; exhausting it escalates.
        assert_eq!(wf.steps["code"].budget, Some(3));
        assert!(routes_to_step(target("code", GateKey::Code(0)), "review"));
        assert!(exits_with(target("code", GateKey::Exhausted), 90));
        // Review approves forward to commit, or sends a Blocking verdict back to code.
        assert!(routes_to_step(target("review", GateKey::Code(0)), "commit"));
        assert!(routes_to_step(target("review", GateKey::Code(1)), "code"));
        // Commit terminates the Run.
        assert!(exits_with(target("commit", GateKey::Code(0)), 0));
    }
}
