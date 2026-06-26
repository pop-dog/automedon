//! End-to-end checks of cross-file sub-Workflow references through the
//! orchestrator binary: a Composite Step naming `{ path: … }` loads the
//! referenced file, runs its root Workflow to its Exit Gate, and surfaces the
//! child's exit code in the parent — and a path cycle is bounded at run time by
//! the Depth cap, not refused at load. These drive the real subprocess executor
//! over the path loader, so path resolution and exit-code surfacing are exercised
//! together as the single-file `composite_workflow` tests do for the by-name form.

use std::path::{Path, PathBuf};
use std::process::Command;

/// A throwaway directory under the system temp dir, removed on Drop.
struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> Self {
        let path = std::env::temp_dir().join(format!("ao-xfile-{tag}-{}", uuid::Uuid::now_v7()));
        std::fs::create_dir_all(&path).unwrap();
        TempDir(path)
    }

    fn write(&self, name: &str, body: &str) -> PathBuf {
        let path = self.0.join(name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(&path, body).unwrap();
        path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Run a root workflow file through the binary with logging isolated to a temp
/// dir, returning its exit code. Extra args (e.g. `--max-depth`) are appended.
fn run_root(root: &Path, extra_args: &[&str]) -> i32 {
    let log_dir = TempDir::new("log");
    let status = Command::new(env!("CARGO_BIN_EXE_automedon"))
        .arg("--log-dir")
        .arg(&log_dir.0)
        .arg("--quiet")
        .args(extra_args)
        .arg(root)
        .status()
        .expect("failed to run orchestrator");
    status.code().expect("orchestrator killed by signal")
}

#[test]
fn path_reference_runs_the_child_and_surfaces_its_exit_code() {
    // The child file's root Workflow exits 7 at its Exit Gate; the parent's
    // Composite Step routes that surfaced code to its own Exit code 100.
    let dir = TempDir::new("surface");
    dir.write(
        "child.yaml",
        r#"
root: child
workflows:
  child:
    entry: work
    steps:
      work:
        command: "exit 7"
        gates:
          - { key: 7, target: { exit: 7 } }
"#,
    );
    let parent = dir.write(
        "parent.yaml",
        r#"
root: main
workflows:
  main:
    entry: call
    steps:
      call:
        workflow: { path: ./child.yaml }
        gates:
          - { key: 7, target: { exit: 100 } }
"#,
    );

    assert_eq!(run_root(&parent, &[]), 100);
}

#[test]
fn a_path_cycle_is_bounded_by_the_depth_cap_not_refused_at_load() {
    // A file referencing its own path loads once, then recurses at run time until
    // `--max-depth`; the uncatchable DepthOverflow aborts the Run, which the
    // binary reports as the Fault status (70, not a routable exit code).
    let dir = TempDir::new("cycle");
    let deep = dir.write(
        "deep.yaml",
        r#"
root: deep
workflows:
  deep:
    entry: recurse
    steps:
      recurse:
        workflow: { path: ./deep.yaml }
        gates:
          - { key: 0, target: { exit: 0 } }
"#,
    );

    assert_eq!(run_root(&deep, &["--max-depth", "3"]), 70);
}
