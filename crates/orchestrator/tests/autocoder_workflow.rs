//! Drives the shipped `examples/autocoder.yaml` end-to-end through the
//! orchestrator binary with every agent and git-mutating Step in stub mode
//! (`CODER_STUB=1`), mirroring `coder_workflow.rs`. This exercises the
//! wrapper's routing and totality — `setup (distill -> checkout) -> coder ->
//! create-pr`, and the inner `coder.yaml` Composite it reuses unmodified —
//! while no LLM, `gh`, or git worktree is ever touched.

use std::path::PathBuf;
use std::process::Command;

/// A throwaway directory under the system temp dir, removed on Drop.
struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> Self {
        let path = std::env::temp_dir().join(format!("ao-ac-{tag}-{}", uuid::Uuid::now_v7()));
        std::fs::create_dir_all(&path).unwrap();
        TempDir(path)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Run the autocoder Workflow with stubbed Steps, returning the
/// orchestrator's exit code. `extra_env` scripts the stubs (distill,
/// checkout, the inner coder Workflow, and create-pr) on top of the
/// `CODER_STUB=1` switch.
fn run_autocoder(extra_env: &[(&str, &str)]) -> i32 {
    let log_dir = TempDir::new("log");
    // The Steps locate their scripts via $AUTOMEDON_WORKFLOW_DIR (set by the
    // orchestrator from the yaml's path), so the child needs no particular cwd
    // to find them; the repo root is used only because it is a valid git
    // repository (some stubbed Steps still guard on $AUTOMEDON_RUN_DIR/basic
    // preconditions, none of which touch this repo in stub mode).
    let repo_root = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_automedon"));
    cmd.current_dir(repo_root)
        .env("CODER_STUB", "1")
        .arg("run")
        .arg("--log-dir")
        .arg(&log_dir.0)
        .arg("--quiet");
    for (key, value) in extra_env {
        cmd.env(key, value);
    }
    let output = cmd
        .arg("examples/autocoder.yaml")
        .arg("--message")
        .arg("1")
        .output()
        .expect("failed to run orchestrator");
    output.status.code().expect("orchestrator killed by signal")
}

#[test]
fn stubbed_happy_path_publishes_and_exits_zero() {
    assert_eq!(run_autocoder(&[("CODER_STUB_REVIEW", "clean")]), 0);
}

#[test]
fn a_distill_failure_escalates() {
    assert_eq!(
        run_autocoder(&[("CODER_STUB_DISTILL_CODE", "1"), ("CODER_STUB_REVIEW", "clean")]),
        90
    );
}

#[test]
fn a_checkout_failure_escalates() {
    assert_eq!(
        run_autocoder(&[("CODER_STUB_CHECKOUT_CODE", "1"), ("CODER_STUB_REVIEW", "clean")]),
        90
    );
}

#[test]
fn the_inner_coder_workflow_exhausting_its_loop_escalates() {
    // Review never clears, so the inner coder.yaml's `code` Budget is spent
    // and its EXHAUSTED Gate exits 90; that surfaces as the `coder` Composite
    // Step's own exit code here, which this Workflow's catch-all Gate also
    // routes to 90 rather than ever reaching create-pr.
    assert_eq!(run_autocoder(&[("CODER_STUB_REVIEW", "blocking")]), 90);
}

#[test]
fn a_create_pr_failure_escalates() {
    assert_eq!(
        run_autocoder(&[("CODER_STUB_REVIEW", "clean"), ("CODER_STUB_PR_CODE", "1")]),
        90
    );
}
