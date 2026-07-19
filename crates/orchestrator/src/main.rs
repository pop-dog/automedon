//! Driver: wires a YAML WorkflowSource and a console Sink into the Kernel.
//! Usage: `automedon run <workflow.yaml> [--message <text>]`.
//!
//! The initial Message seeds the entry Step. It comes from `--message` or piped
//! stdin (the flag wins); with no `--message` and nothing piped, it is empty.

use std::io::Read;
use std::path::{Path, PathBuf};

use kernel::{Registry, RunConfig, Sink, SubprocessExecutor, WorkflowSource};
use sinks::{ConsoleSink, FileSink, Tee};

mod display_id;
mod graph;
mod loader;
mod plan;
mod validate;

/// A `WorkflowSource` that parses a Workflow registry from a root YAML file
/// (`root:` + `workflows:`), transitively loading any files its Composite Steps
/// reference by `{ path: … }` and assembling them into one registry.
/// A single-file Workflow is just the degenerate case with no path references.
struct YamlSource {
    path: PathBuf,
}

impl WorkflowSource for YamlSource {
    fn load(&self) -> Result<Registry, Box<dyn std::error::Error>> {
        loader::load(&self.path)
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

/// The parsed command-line invocation. Holds raw flag values verbatim; the
/// precedence rules (flag vs piped stdin, flag vs env) stay in their own pure
/// resolvers (`resolve_initial_message`, `config::runs_dir`,
/// `config::resolve_keep`) so this stays a parse, not a policy.
#[derive(Debug, PartialEq)]
struct Cli {
    path: PathBuf,
    message: Option<String>,
    quiet: bool,
    log_dir: Option<String>,
    keep: Option<String>,
    max_depth: Option<String>,
    dry_run: bool,
}

/// The invocation is missing the positional Workflow path.
#[derive(Debug, PartialEq)]
struct UsageError;

/// Top-level usage: names the program, shows the command grammar, and lists the
/// available subcommands so the CLI is self-describing. Printed to stdout for an
/// explicit `help`, or to stderr for a missing/unknown subcommand.
const TOP_USAGE: &str = "\
automedon — run an agent Workflow

usage: automedon <command> [args]

commands:
  run       Run a Workflow from a YAML file
  validate  Statically check a Workflow for graph errors
  graph     Emit a Mermaid flowchart of a Workflow's graph
  help      Show this help

Run `automedon <command> --help` for command-specific usage.";

/// Usage for the `run` subcommand alone. Printed for `run --help` (stdout) or a
/// `run` invocation missing its Workflow path (stderr).
const RUN_USAGE: &str =
    "usage: automedon run <workflow.yaml> [--message <text>] [--dry-run]";

/// Usage for the `validate` subcommand alone. Printed for `validate --help`
/// (stdout) or a `validate` invocation missing its Workflow path (stderr).
const VALIDATE_USAGE: &str = "usage: automedon validate <workflow.yaml>";

/// Usage for the `graph` subcommand alone. Printed for `graph --help` (stdout)
/// or a `graph` invocation missing its Workflow path (stderr).
const GRAPH_USAGE: &str = "usage: automedon graph <workflow.yaml>";

/// The outcome of parsing argv: run a Workflow, show requested help (a success,
/// exit 0), or a usage error (exit non-zero). Help and usage errors carry the
/// text to print so each context can point at the most specific usage, while the
/// stream and exit code are decided by which variant it is — not by the parser.
#[derive(Debug, PartialEq)]
enum Invocation {
    Run(Box<Cli>),
    Validate(PathBuf),
    Graph(PathBuf),
    Help(&'static str),
    Usage(&'static str),
}

impl Cli {
    /// Parse an argument vector (argv minus the program name). The first token
    /// selects the subcommand; `run` executes a Workflow and `help` (or a
    /// top-level `--help`/`-h`) prints usage. A missing or unknown subcommand is
    /// a usage error. Defined over a `&[String]` with no argv/stdin access of its
    /// own so it is unit-testable.
    fn parse(args: &[String]) -> Invocation {
        match args.split_first() {
            Some((cmd, rest)) if cmd == "run" => {
                // A help flag anywhere in the run arguments asks for run's usage,
                // taking precedence over parsing a (possibly absent) Workflow path.
                if rest.iter().any(|a| a == "--help" || a == "-h") {
                    return Invocation::Help(RUN_USAGE);
                }
                match Cli::parse_run(rest) {
                    Ok(cli) => Invocation::Run(Box::new(cli)),
                    Err(UsageError) => Invocation::Usage(RUN_USAGE),
                }
            }
            Some((cmd, rest)) if cmd == "validate" => {
                if rest.iter().any(|a| a == "--help" || a == "-h") {
                    return Invocation::Help(VALIDATE_USAGE);
                }
                match rest.iter().find(|a| !a.starts_with('-')) {
                    Some(path) => Invocation::Validate(PathBuf::from(path)),
                    None => Invocation::Usage(VALIDATE_USAGE),
                }
            }
            Some((cmd, rest)) if cmd == "graph" => {
                if rest.iter().any(|a| a == "--help" || a == "-h") {
                    return Invocation::Help(GRAPH_USAGE);
                }
                match rest.iter().find(|a| !a.starts_with('-')) {
                    Some(path) => Invocation::Graph(PathBuf::from(path)),
                    None => Invocation::Usage(GRAPH_USAGE),
                }
            }
            Some((cmd, _)) if cmd == "help" || cmd == "--help" || cmd == "-h" => {
                Invocation::Help(TOP_USAGE)
            }
            // No arguments or an unknown subcommand: point the operator at the
            // fuller top-level usage on stderr.
            _ => Invocation::Usage(TOP_USAGE),
        }
    }

    /// Parse the arguments that follow the `run` subcommand: the workflow path
    /// plus its flags. This is the pre-subcommand invocation surface, shifted one
    /// token right.
    fn parse_run(args: &[String]) -> Result<Cli, UsageError> {
        let mut path: Option<PathBuf> = None;
        let mut message = None;
        let mut quiet = false;
        let mut log_dir = None;
        let mut keep = None;
        let mut max_depth = None;
        let mut dry_run = false;

        // Each flag's spelling and whether it consumes the next argument as its
        // value are defined only here — one source of truth for the flag vocabulary.
        let mut i = 0;
        while i < args.len() {
            let arg = args[i].as_str();
            let value = || args.get(i + 1).cloned();
            match arg {
                "--message" => {
                    message = value();
                    i += 2;
                }
                "--log-dir" => {
                    log_dir = value();
                    i += 2;
                }
                "--keep" => {
                    keep = value();
                    i += 2;
                }
                "--max-depth" => {
                    max_depth = value();
                    i += 2;
                }
                "-q" | "--quiet" => {
                    quiet = true;
                    i += 1;
                }
                "--dry-run" => {
                    dry_run = true;
                    i += 1;
                }
                _ if arg.starts_with('-') => i += 1,
                _ => {
                    path.get_or_insert_with(|| PathBuf::from(arg));
                    i += 1;
                }
            }
        }

        match path {
            Some(path) => Ok(Cli { path, message, quiet, log_dir, keep, max_depth, dry_run }),
            None => Err(UsageError),
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();

    let cli = match Cli::parse(&args) {
        Invocation::Run(cli) => *cli,
        Invocation::Validate(path) => {
            let registry = loader::load(&path).unwrap_or_else(|e| {
                eprintln!("failed to load workflow: {e}");
                std::process::exit(2);
            });
            let problems = validate::check(&registry);
            if problems.is_empty() {
                println!("{}: valid", path.display());
                std::process::exit(0);
            }
            for problem in &problems {
                eprintln!("{problem}");
            }
            std::process::exit(1);
        }
        Invocation::Graph(path) => {
            let registry = loader::load(&path).unwrap_or_else(|e| {
                eprintln!("failed to load workflow: {e}");
                std::process::exit(2);
            });
            print!("{}", graph::render(&registry));
            std::process::exit(0);
        }
        // Explicit help is a success: usage to stdout, exit 0.
        Invocation::Help(usage) => {
            println!("{usage}");
            std::process::exit(0);
        }
        // A usage error: usage to stderr, non-zero exit.
        Invocation::Usage(usage) => {
            eprintln!("{usage}");
            std::process::exit(2);
        }
    };
    let path = cli.path;

    // The workflow definition's directory becomes $AUTOMEDON_WORKFLOW_DIR (a Step
    // environment member, below) so a Step can locate the scripts it names (e.g.
    // `command: "$AUTOMEDON_WORKFLOW_DIR/fetch.sh"`) independently of the working directory.
    // This decouples *where the scripts live* (the workflow repo) from *where the
    // work happens*: the cwd is left as the target project root — the repo a Step
    // reads, edits, and commits — so a workflow and the repo it operates on need
    // not be the same directory.
    let dir = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let workflow_dir = std::fs::canonicalize(dir).unwrap_or_else(|_| dir.to_path_buf());

    let source = YamlSource { path };
    let registry = source.load().unwrap_or_else(|e| {
        eprintln!("failed to load workflow: {e}");
        std::process::exit(2);
    });

    // A dry run prints the plan the Kernel would execute and exits before any
    // Frame, Sink, or run/log directory is created — it must not produce a Run.
    // The loaded root path is diagnostic value worth keeping, but only once —
    // as this header — rather than repeated (and machine-specific) on every
    // per-workflow line below.
    if cli.dry_run {
        println!("root: {}", root_path(&registry));
        print!("{}", plan::describe(&registry));
        std::process::exit(0);
    }

    // Read stdin only when it is piped or redirected; a terminal stdin would
    // block on an end-of-file that never arrives.
    let piped_stdin = if std::io::IsTerminal::is_terminal(&std::io::stdin()) {
        None
    } else {
        let mut buf = Vec::new();
        std::io::stdin().read_to_end(&mut buf).ok().map(|_| buf)
    };

    let initial_message = resolve_initial_message(cli.message, piped_stdin);

    // Choose the runs directory, then mint this Run's UUIDv7 ID and hand its
    // directory to the file Sink. The Kernel stays unaware of Run identity.
    let log_override = cli.log_dir.or_else(|| env_var("AGENT_ORCHESTRATOR_LOG_DIR"));
    let runs_dir = sinks::runs_dir(
        log_override.as_deref(),
        env_var("XDG_STATE_HOME").as_deref(),
        env_var("HOME").as_deref(),
    );
    let keep = sinks::resolve_keep(
        cli.keep.as_deref(),
        env_var("AGENT_ORCHESTRATOR_KEEP").as_deref(),
    );

    let run_id = uuid::Uuid::now_v7();
    let log_dir = runs_dir.join(run_id.to_string());

    // The ephemeral Run Directory ($AUTOMEDON_RUN_DIR): per-Run scratch the engine provides
    // under the OS temp root, sharing this Run's id with the durable log dir but
    // with an independent (OS-reaped) lifecycle. Created best-effort
    // before the first Step runs; a Run whose scratch cannot be made still runs.
    let run_dir = sinks::run_scratch_dir(&std::env::temp_dir(), &run_id.to_string());
    if let Err(e) = std::fs::create_dir_all(&run_dir) {
        eprintln!("warning: cannot create run directory at {}: {e}", run_dir.display());
    }

    // The Step environment: the ambient, Run-constant context every Step
    // receives. The Executor injects it per-spawn, retiring the previous global
    // `set_var("AUTOMEDON_WORKFLOW_DIR")`. Logging whatever it holds covers future members.
    let environment: Vec<(String, PathBuf)> = vec![
        ("AUTOMEDON_WORKFLOW_DIR".to_string(), workflow_dir),
        ("AUTOMEDON_RUN_DIR".to_string(), run_dir.clone()),
    ];

    let mut sink_chain: Vec<Box<dyn Sink>> = vec![Box::new(ConsoleSink::new(cli.quiet))];
    match FileSink::create(log_dir.clone()) {
        Ok(file) => {
            // Record the populated Step environment once at startup, so the
            // ephemeral Run Directory is discoverable from the durable log.
            file.record_environment(&environment);
            sink_chain.push(Box::new(file));
        }
        // A Run that cannot be logged still runs; only durability is lost.
        Err(e) => eprintln!("warning: cannot open run log at {}: {e}", log_dir.display()),
    }

    // Prune once this Run's directory exists, so the newest (this) Run counts
    // toward the kept N and the oldest are dropped first.
    let _ = sinks::prune(&runs_dir, keep);

    let mut sink = Tee::new(sink_chain);
    let mut executor = SubprocessExecutor::with_env(environment);

    let max_depth = sinks::resolve_max_depth(
        cli.max_depth.as_deref(),
        env_var("AGENT_ORCHESTRATOR_MAX_DEPTH").as_deref(),
    );
    let run_config = RunConfig::default().with_max_depth(max_depth);

    let outcome = kernel::run(&registry, &initial_message, &run_config, &mut executor, &mut sink);

    // On a failed Run — a non-zero exit or a Fault — point the operator at the
    // ephemeral Run Directory, the engine's live counterpart to the startup
    // metadata record, so a Run's scratch is findable without any Step echoing it.
    let failed = !matches!(outcome, Ok(0));
    if failed {
        eprintln!("run directory: {}", run_dir.display());
    }
    match outcome {
        Ok(code) => std::process::exit(code),
        // A Fault is not an exit code; surface it on a distinct status (sysexits EX_SOFTWARE).
        Err(_) => std::process::exit(70),
    }
}

/// A non-empty environment variable, or `None`. An empty value is treated as
/// unset so a blank override falls through to the next source.
fn env_var(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|v| !v.is_empty())
}

/// The canonical file path the registry was loaded from, recovered from
/// `registry.root` (`<canonical path>#<name>`, see `loader::make_id`) rather
/// than threaded separately through the `WorkflowSource` trait.
fn root_path(registry: &Registry) -> &str {
    registry.root.rsplit_once('#').map_or(registry.root.as_str(), |(path, _)| path)
}

// Tests for the YAML front-end live here (where the format dependency lives), not
// in the format-agnostic Kernel.
#[cfg(test)]
mod tests {
    use super::{resolve_initial_message, Cli, Invocation};
    use kernel::{GateKey, GateTarget, Registry, Step, StepBody, Workflow};

    fn args(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    /// Unwrap a `Run` invocation, panicking on `Help`/`Usage`. Keeps the
    /// run-parsing tests reading about the parsed `Cli`, not the enum wrapper.
    fn run(items: &[&str]) -> Cli {
        match Cli::parse(&args(items)) {
            Invocation::Run(cli) => *cli,
            other => panic!("expected a run invocation, got {other:?}"),
        }
    }

    #[test]
    fn parse_takes_the_first_positional_after_run_as_the_path() {
        let cli = run(&["run", "wf.yaml"]);
        assert_eq!(cli.path, std::path::PathBuf::from("wf.yaml"));
    }

    #[test]
    fn parse_rejects_a_missing_subcommand() {
        assert_eq!(Cli::parse(&args(&[])), Invocation::Usage(super::TOP_USAGE));
    }

    #[test]
    fn parse_rejects_an_unknown_subcommand() {
        // The workflow path is no longer accepted as the bare first argument; it
        // reads as an unknown subcommand.
        assert_eq!(Cli::parse(&args(&["wf.yaml"])), Invocation::Usage(super::TOP_USAGE));
    }

    #[test]
    fn parse_without_a_positional_is_a_usage_error() {
        assert_eq!(
            Cli::parse(&args(&["run", "-q", "--keep", "5"])),
            Invocation::Usage(super::RUN_USAGE),
        );
    }

    #[test]
    fn parse_top_level_help_flags_request_top_usage() {
        for flag in ["help", "--help", "-h"] {
            assert_eq!(Cli::parse(&args(&[flag])), Invocation::Help(super::TOP_USAGE));
        }
    }

    #[test]
    fn parse_run_help_flags_request_run_usage() {
        for flag in ["--help", "-h"] {
            assert_eq!(Cli::parse(&args(&["run", flag])), Invocation::Help(super::RUN_USAGE));
        }
    }

    #[test]
    fn parse_recognizes_the_dry_run_flag() {
        let cli = run(&["run", "wf.yaml", "--dry-run"]);
        assert!(cli.dry_run);
    }

    #[test]
    fn parse_dry_run_defaults_to_false() {
        let cli = run(&["run", "wf.yaml"]);
        assert!(!cli.dry_run);
    }

    #[test]
    fn parse_graph_takes_the_workflow_path() {
        match Cli::parse(&args(&["graph", "wf.yaml"])) {
            Invocation::Graph(path) => assert_eq!(path, std::path::PathBuf::from("wf.yaml")),
            other => panic!("expected a graph invocation, got {other:?}"),
        }
    }

    #[test]
    fn parse_graph_without_a_positional_is_a_usage_error() {
        assert_eq!(Cli::parse(&args(&["graph"])), Invocation::Usage(super::GRAPH_USAGE));
    }

    #[test]
    fn parse_graph_help_flags_request_graph_usage() {
        for flag in ["--help", "-h"] {
            assert_eq!(Cli::parse(&args(&["graph", flag])), Invocation::Help(super::GRAPH_USAGE));
        }
    }

    #[test]
    fn parse_validate_takes_the_workflow_path() {
        match Cli::parse(&args(&["validate", "wf.yaml"])) {
            Invocation::Validate(path) => assert_eq!(path, std::path::PathBuf::from("wf.yaml")),
            other => panic!("expected a validate invocation, got {other:?}"),
        }
    }

    #[test]
    fn parse_validate_without_a_positional_is_a_usage_error() {
        assert_eq!(Cli::parse(&args(&["validate"])), Invocation::Usage(super::VALIDATE_USAGE));
    }

    #[test]
    fn parse_validate_help_flags_request_validate_usage() {
        for flag in ["--help", "-h"] {
            assert_eq!(
                Cli::parse(&args(&["validate", flag])),
                Invocation::Help(super::VALIDATE_USAGE)
            );
        }
    }

    #[test]
    fn parse_captures_flags_after_the_positional() {
        let cli = run(&["run", "wf.yaml", "--message", "hello"]);
        assert_eq!(cli.path, std::path::PathBuf::from("wf.yaml"));
        assert_eq!(cli.message.as_deref(), Some("hello"));
    }

    #[test]
    fn parse_treats_a_dangling_message_as_absent() {
        // A trailing `--message` with no value falls back to stdin or the empty
        // default rather than erroring.
        let cli = run(&["run", "wf.yaml", "--message"]);
        assert_eq!(cli.message, None);
    }

    #[test]
    fn parse_captures_flags_before_the_positional() {
        let cli = run(&["run", "-q", "--log-dir", "/tmp/runs", "--keep", "5", "wf.yaml"]);
        assert_eq!(cli.path, std::path::PathBuf::from("wf.yaml"));
        assert!(cli.quiet);
        assert_eq!(cli.log_dir.as_deref(), Some("/tmp/runs"));
        assert_eq!(cli.keep.as_deref(), Some("5"));
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

    #[test]
    fn parses_both_step_body_forms() {
        // A leaf names `command:`; a Composite names `workflow:`. The flat shim
        // maps each onto the StepBody sum type.
        match serde_yaml::from_str::<Step>("{ command: \"exit 0\" }").unwrap().body {
            StepBody::Command(c) => assert_eq!(c, "exit 0"),
            other => panic!("expected Command, got {other:?}"),
        }
        match serde_yaml::from_str::<Step>("{ workflow: reviewer }").unwrap().body {
            StepBody::Workflow(id) => assert_eq!(id, "reviewer"),
            other => panic!("expected Workflow, got {other:?}"),
        }
    }

    #[test]
    fn rejects_ambiguous_or_empty_step_body() {
        // Both keys set is ambiguous; neither set is empty. Both must fail, so the
        // illegal two-bodies state never reaches the engine.
        assert!(serde_yaml::from_str::<Step>("{ command: \"x\", workflow: w }").is_err());
        assert!(serde_yaml::from_str::<Step>("{ budget: 1 }").is_err());
    }

    #[test]
    fn parses_a_multi_workflow_file() {
        // The top-level surface is `root:` + `workflows:`; a Composite Step in one
        // references another by its map key.
        let yaml = r#"
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
"#;
        let reg: Registry = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(reg.root, "main");
        assert_eq!(reg.workflows.len(), 2);
        match &reg.workflows["main"].steps["call"].body {
            StepBody::Workflow(id) => assert_eq!(id, "child"),
            other => panic!("expected a Composite Step, got {other:?}"),
        }
    }

    // Guards the shipped coder example: parsing it and asserting its routing
    // keeps the file honest without invoking the LLM Steps it names.
    #[test]
    fn coder_example_wires_the_composite_develop_loop() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../examples/coder.yaml");
        let text = std::fs::read_to_string(path).unwrap();
        let reg: Registry = serde_yaml::from_str(&text).unwrap();
        assert_eq!(reg.root, "coder");

        // Resolve the target a Step in a given Workflow routes to for a Gate key.
        let target = |wf: &Workflow, step: &str, key: GateKey| -> GateTarget {
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

        // Top level: `develop` (a Composite Step over the `develop` sub-Workflow)
        // hands off to `commit`; any non-zero surfaced code escalates.
        let coder = &reg.workflows["coder"];
        assert_eq!(coder.entry, "develop");
        match &coder.steps["develop"].body {
            StepBody::Workflow(id) => assert_eq!(id, "develop"),
            other => panic!("expected develop to be a Composite Step, got {other:?}"),
        }
        assert!(routes_to_step(target(coder, "develop", GateKey::Code(0)), "commit"));
        assert!(exits_with(target(coder, "develop", GateKey::Default), 90));
        assert!(exits_with(target(coder, "commit", GateKey::Code(0)), 0));

        // The sub-Workflow is the code -> build-test -> review loop, bounded by
        // code's Budget — its single re-entry point. `code` hands off to the
        // deterministic build-test gate; a red build loops back to code, a green
        // build goes to review. Review approval exits 0 (surfacing to the parent);
        // a Blocking verdict loops back to code.
        let develop = &reg.workflows["develop"];
        assert_eq!(develop.entry, "code");
        assert_eq!(develop.steps["code"].budget, Some(3));
        assert!(routes_to_step(target(develop, "code", GateKey::Code(0)), "build-test"));
        assert!(exits_with(target(develop, "code", GateKey::Exhausted), 90));
        assert!(routes_to_step(target(develop, "build-test", GateKey::Code(0)), "review"));
        assert!(routes_to_step(target(develop, "build-test", GateKey::Default), "code"));
        assert!(exits_with(target(develop, "review", GateKey::Code(0)), 0));
        assert!(routes_to_step(target(develop, "review", GateKey::Code(1)), "code"));
    }
}
