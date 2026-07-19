//! End-to-end checks of `automedon graph` (#53): a valid Mermaid flowchart on
//! stdout for a real, loaded root Workflow file, with no absolute paths
//! anywhere in the output — the same usage/exit-code contract as `validate`.

use std::path::PathBuf;
use std::process::{Command, Output};

fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_automedon"))
        .args(args)
        .output()
        .expect("failed to run orchestrator")
}

fn examples_dir() -> PathBuf {
    PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../examples"))
}

#[test]
fn graph_help_prints_a_usage_line() {
    let out = run(&["graph", "--help"]);
    assert_eq!(out.status.code(), Some(0));
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(
        stdout.contains("usage: automedon graph <workflow.yaml>"),
        "stdout should show the graph usage: {stdout}"
    );
}

#[test]
fn graph_without_a_positional_is_a_usage_error() {
    let out = run(&["graph"]);
    assert_ne!(out.status.code(), Some(0));
    assert!(out.stdout.is_empty());
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(stderr.contains("usage: automedon graph"), "{stderr}");
}

#[test]
fn a_load_failure_exits_2() {
    let out = run(&["graph", "/no/such/workflow.yaml"]);
    assert_eq!(out.status.code(), Some(2));
    assert!(out.stdout.is_empty());
}

#[test]
fn coder_example_emits_a_valid_mermaid_flowchart_with_no_absolute_paths() {
    let path = examples_dir().join("coder.yaml");
    let out = run(&["graph", path.to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(0));
    let stdout = String::from_utf8(out.stdout).unwrap();

    assert!(stdout.starts_with("flowchart TD\n"), "{stdout}");
    // Root-file Workflows render as their bare name.
    assert!(stdout.contains("subgraph wf_coder[\"coder\"]"), "{stdout}");
    assert!(stdout.contains("subgraph wf_develop[\"develop\"]"), "{stdout}");
    // The Composite `develop` Step hands off to the child Workflow's entry Step.
    assert!(stdout.contains("wf_coder_develop -.-> wf_develop_code"), "{stdout}");
    assert!(
        !stdout.contains('/'),
        "no absolute (or any) path should appear in output: {stdout}"
    );
}

#[test]
fn autocoder_example_labels_the_cross_file_composite_with_a_relative_path() {
    let path = examples_dir().join("autocoder.yaml");
    let out = run(&["graph", path.to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(0));
    let stdout = String::from_utf8(out.stdout).unwrap();

    assert!(stdout.starts_with("flowchart TD\n"), "{stdout}");
    // The root file's own Workflow renders bare.
    assert!(stdout.contains("subgraph wf_autocoder[\"autocoder\"]"), "{stdout}");
    // The cross-file `coder.yaml` Workflow renders as `coder.yaml#coder`, not an
    // absolute path.
    assert!(
        stdout.contains("subgraph wf_coder_yaml_coder[\"coder.yaml#coder\"]"),
        "{stdout}"
    );
    assert!(
        !stdout.lines().any(|line| line.contains(examples_dir().to_str().unwrap())),
        "no absolute example-dir path should appear in output: {stdout}"
    );
}
