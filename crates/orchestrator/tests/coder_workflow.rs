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
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_automedon"));
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

/// Absolute path to a coder Step script shipped under `examples/coder/`.
fn coder_script(name: &str) -> PathBuf {
    PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../examples/coder")).join(name)
}

/// The single per-Run directory inside a freshly-used log directory.
fn sole_run_dir(log_dir: &Path) -> PathBuf {
    let mut runs: Vec<PathBuf> = std::fs::read_dir(log_dir)
        .expect("log dir exists")
        .map(|e| e.unwrap().path())
        .filter(|p| p.is_dir())
        .collect();
    assert_eq!(runs.len(), 1, "expected exactly one run dir, got {runs:?}");
    runs.remove(0)
}

/// The expected ephemeral `$RUN_DIR` for a Run logged to `log_dir`, mirroring the
/// engine's resolver (`<temp_root>/agent-orchestrator/runs/<run-id>`).
fn run_scratch_dir(log_dir: &Path) -> PathBuf {
    let run_id = sole_run_dir(log_dir);
    let run_id = run_id.file_name().unwrap().to_str().unwrap();
    std::env::temp_dir().join("agent-orchestrator").join("runs").join(run_id)
}

/// The repo working tree the Steps operate on (cwd of a Run, per `run_coder`).
fn repo_root() -> PathBuf {
    PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
}

#[test]
fn coder_scripts_fail_loud_when_run_dir_is_unset() {
    // The Steps are orchestrator Steps; the engine always provides $RUN_DIR. With
    // it unset, each script must error with a clear message rather than silently
    // falling back to cwd (which would reintroduce the repo coupling) or mktemp
    // (which would break the cross-Step file handoff). CODER_STUB=1 keeps the
    // guard ahead of any agent invocation, so the check is what trips, not claude.
    for script in ["review.sh", "build-test.sh", "code.sh", "commit.sh"] {
        let output = Command::new("/bin/sh")
            .arg(coder_script(script))
            .env_remove("RUN_DIR")
            .env("CODER_STUB", "1")
            .stdin(std::process::Stdio::null())
            .output()
            .expect("failed to run coder script");
        assert!(
            !output.status.success(),
            "{script} should fail when RUN_DIR is unset"
        );
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains("RUN_DIR"),
            "{script} should name RUN_DIR in its error; got: {stderr}"
        );
    }
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
fn persistent_build_failure_exhausts_the_loop_and_escalates() {
    // The deterministic build-test Step never goes green, so it loops back to code
    // until code's Budget is spent and the EXHAUSTED Gate escalates with EXIT 90,
    // never reaching review or commit.
    assert_eq!(run_coder(&[("CODER_STUB_BUILD", "fail")]), 90);
}

#[test]
fn transient_build_failure_is_retried_then_succeeds() {
    // build-test fails on its first activation then passes, so code is re-entered
    // once to fix the build and the Run still reaches commit and exits 0 — the
    // build result is a feedback loop, not an automatic escalation.
    let log_dir = TempDir::new("coder-log");
    assert_eq!(
        run_coder_logging_to(
            &log_dir.0,
            &[("CODER_STUB_BUILD", "fail-once"), ("CODER_STUB_REVIEW", "clean")],
        ),
        0
    );

    // The stub's "failed once" marker is orchestration scratch: it must land in
    // the ephemeral Run Directory and never in the Repository working tree. The
    // marker survives the Run (the second build-test reads but does not remove
    // it), so it is the observable proof that scratch is repointed at $RUN_DIR.
    let scratch = run_scratch_dir(&log_dir.0);
    assert!(
        scratch.join(".build-stub-marker").exists(),
        "stub scratch should land in $RUN_DIR: {}",
        scratch.display()
    );
    for artifact in [".build-stub-marker", "FINDINGS.md", "BUILD_FAILURE.md"] {
        assert!(
            !repo_root().join(artifact).exists(),
            "{artifact} must not appear in the Repository working tree"
        );
    }
    let _ = std::fs::remove_dir_all(&scratch);
}
