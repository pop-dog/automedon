//! End-to-end checks of `run --dry-run` (#53): the loaded root path appears
//! exactly once, as a header, and per-workflow lines use display ids rather
//! than the loader's absolute-path-bearing ids.

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
fn dry_run_prints_the_root_path_exactly_once_and_display_ids_per_workflow() {
    let path = examples_dir().join("coder.yaml");
    let canonical = std::fs::canonicalize(&path).unwrap();
    let out = run(&["run", path.to_str().unwrap(), "--dry-run"]);
    assert_eq!(out.status.code(), Some(0));
    let stdout = String::from_utf8(out.stdout).unwrap();

    let canonical_str = canonical.to_str().unwrap();
    let occurrences = stdout.matches(canonical_str).count();
    assert_eq!(occurrences, 1, "root path should appear exactly once: {stdout}");
    assert!(stdout.contains("workflow coder (entry: develop)"), "{stdout}");
    assert!(stdout.contains("workflow develop (entry: code)"), "{stdout}");
}

#[test]
fn dry_run_of_a_cross_file_registry_labels_the_child_with_a_relative_path() {
    let path = examples_dir().join("autocoder.yaml");
    let out = run(&["run", path.to_str().unwrap(), "--dry-run"]);
    assert_eq!(out.status.code(), Some(0));
    let stdout = String::from_utf8(out.stdout).unwrap();

    assert!(stdout.contains("workflow autocoder (entry: setup)"), "{stdout}");
    assert!(stdout.contains("workflow coder.yaml#coder (entry: develop)"), "{stdout}");
}
