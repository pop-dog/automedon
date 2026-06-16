//! Drives the shipped `examples/coder.yaml` end-to-end through the orchestrator
//! binary with the LLM Steps in stub mode (`CODER_STUB=1`). This exercises the
//! Workflow's routing and totality — the kernel-orchestrated guarantees — while
//! the agents stay inert, so no LLM is invoked and the repo is never touched.

use std::path::{Path, PathBuf};
use std::process::Command;

/// A throwaway directory under the system temp dir, removed on Drop. Mirrors the
/// isolation helper in `durable_logging.rs`.
struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> Self {
        let path = std::env::temp_dir().join(format!("ao-it-{tag}-{}", uuid::Uuid::now_v7()));
        std::fs::create_dir_all(&path).unwrap();
        TempDir(path)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Run the coder Workflow with stubbed Steps, returning the orchestrator's exit
/// code. `extra_env` scripts the stubs (e.g. the review outcome or a failing
/// build) on top of the `CODER_STUB=1` switch. Run logs go to a per-test temp
/// dir cleaned on Drop, so the suite never leaks into the real state dir.
fn run_coder(extra_env: &[(&str, &str)]) -> i32 {
    let log_dir = TempDir::new("coder-log");
    run_coder_logging_to(&log_dir.0, extra_env)
}

/// As `run_coder`, but logs Runs to `log_dir` so the caller can inspect (and own
/// the lifetime of) the resulting Run directories.
fn run_coder_logging_to(log_dir: &Path, extra_env: &[(&str, &str)]) -> i32 {
    // The Steps locate their scripts via $WORKFLOW_DIR (set by the orchestrator
    // from the yaml's path), so the child needs no particular cwd to find them.
    // We run from the repo root because that is the workspace the Steps read
    // (./TASK.md) and write (FINDINGS.md).
    let repo_root = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_orchestrator"));
    cmd.current_dir(repo_root)
        .env("CODER_STUB", "1")
        .arg("--log-dir")
        .arg(log_dir);
    for (key, value) in extra_env {
        cmd.env(key, value);
    }
    let output = cmd
        .arg("examples/coder.yaml")
        .arg("--message")
        .arg("./TASK.md")
        .output()
        .expect("failed to run orchestrator");
    output.status.code().expect("orchestrator killed by signal")
}

#[test]
fn clean_review_routes_through_commit_to_exit_zero() {
    assert_eq!(run_coder(&[("CODER_STUB_REVIEW", "clean")]), 0);
}

#[test]
fn run_is_logged_to_an_isolated_directory() {
    // Spawning the binary must not leak Run directories into the developer's
    // real state dir: the Run is recorded in a per-test temp dir instead.
    let log_dir = TempDir::new("coder-log");
    assert_eq!(
        run_coder_logging_to(&log_dir.0, &[("CODER_STUB_REVIEW", "clean")]),
        0
    );
    let runs = std::fs::read_dir(&log_dir.0)
        .expect("log dir exists")
        .filter(|e| e.as_ref().unwrap().path().is_dir())
        .count();
    assert!(runs >= 1, "the Run was logged into the isolated dir");
}

#[test]
fn persistent_blocking_exhausts_the_loop_and_escalates() {
    // Review never clears, so code's Budget is spent and the EXHAUSTED Gate
    // escalates with EXIT 90 rather than ever reaching commit.
    assert_eq!(run_coder(&[("CODER_STUB_REVIEW", "blocking")]), 90);
}

#[test]
fn code_that_cannot_build_escalates_with_exit_90() {
    // A non-zero code Step (a build that never goes green) takes its catch-all
    // Gate straight to escalation, never reaching review or commit.
    assert_eq!(run_coder(&[("CODER_STUB_CODE", "1")]), 90);
}
