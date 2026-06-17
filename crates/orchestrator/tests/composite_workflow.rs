//! End-to-end checks of Slice 4 through the orchestrator binary: a Composite
//! Step runs a named child sub-Workflow to its Exit Gate and the surfaced code
//! routes in the parent, and a self-referencing Workflow trips the `--max-depth`
//! cap. These drive the real subprocess executor over the Frame stack, so the
//! CLI flag, the multi-Workflow file format, and exit-code surfacing are all
//! exercised together.

use std::path::PathBuf;
use std::process::Command;

/// A throwaway directory under the system temp dir, removed on Drop.
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

/// Run a workflow file through the binary with logging isolated to a temp dir,
/// returning its exit code. Extra args (e.g. `--max-depth`) are appended.
fn run_workflow(yaml: &str, extra_args: &[&str]) -> i32 {
    let work = TempDir::new("composite-wf");
    let log_dir = TempDir::new("composite-log");
    let wf_path = work.0.join("wf.yaml");
    std::fs::write(&wf_path, yaml).unwrap();

    let status = Command::new(env!("CARGO_BIN_EXE_orchestrator"))
        .arg("--log-dir")
        .arg(&log_dir.0)
        .arg("--quiet")
        .args(extra_args)
        .arg(&wf_path)
        .status()
        .expect("failed to run orchestrator");
    status.code().expect("orchestrator killed by signal")
}

#[test]
fn composite_step_surfaces_child_code_and_routes_in_parent() {
    // The child exits 7 at its own Exit Gate; the parent's Composite Step routes
    // that surfaced code to its own Exit code 100.
    let yaml = r#"
root: main
workflows:
  main:
    entry: call
    steps:
      call:
        workflow: child
        gates:
          - { key: 7, target: { exit: 100 } }
  child:
    entry: work
    steps:
      work:
        command: "exit 7"
        gates:
          - { key: 7, target: { exit: 7 } }
"#;
    assert_eq!(run_workflow(yaml, &[]), 100);
}

#[test]
fn self_reference_trips_the_depth_cap_and_fails_the_run() {
    // A Workflow that names itself recurses until `--max-depth`; the uncatchable
    // DepthOverflow aborts the Run, which the binary reports as the Fault status
    // (70, not a routable exit code).
    let yaml = r#"
root: deep
workflows:
  deep:
    entry: recurse
    steps:
      recurse:
        workflow: deep
        gates:
          - { key: 0, target: { exit: 0 } }
"#;
    assert_eq!(run_workflow(yaml, &["--max-depth", "3"]), 70);
}
