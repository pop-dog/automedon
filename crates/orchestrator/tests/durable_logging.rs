//! End-to-end check of Issue 12's headline guarantee: after the orchestrator
//! exits, a failed Step's stderr is recoverable from its Run directory, and
//! events.jsonl carries Sink-assigned `seq`/`ts` and points at the sidecar.

use std::path::{Path, PathBuf};
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

#[test]
fn failed_step_stderr_is_recoverable_from_the_run_directory() {
    let work = TempDir::new("wf");
    let log_dir = TempDir::new("log");

    // A one-Step Workflow that prints to stderr and exits non-zero, routing that
    // code straight to an Exit Gate so the Run ends deterministically.
    let wf = r#"
root: main
workflows:
  main:
    entry: boom
    steps:
      boom:
        command: "echo 'diagnostic detail' >&2; exit 3"
        gates:
          - { key: 3, target: { exit: 3 } }
"#;
    let wf_path = work.0.join("wf.yaml");
    std::fs::write(&wf_path, wf).unwrap();

    let status = Command::new(env!("CARGO_BIN_EXE_automedon"))
        .arg("run")
        .arg("--log-dir")
        .arg(&log_dir.0)
        .arg("--quiet")
        .arg(&wf_path)
        .status()
        .expect("failed to run orchestrator");
    assert_eq!(status.code(), Some(3));

    let run_dir = sole_run_dir(&log_dir.0);

    // The Step's stderr survived to disk and is recoverable verbatim.
    let stderr = std::fs::read(run_dir.join("boom.0.stderr")).unwrap();
    assert_eq!(stderr, b"diagnostic detail\n");

    // events.jsonl exists, every record carries seq + ts, and one record points
    // at the sidecar file holding the bulk output.
    let log = std::fs::read_to_string(run_dir.join("events.jsonl")).unwrap();
    let records: Vec<serde_json::Value> =
        log.lines().map(|l| serde_json::from_str(l).unwrap()).collect();
    assert!(!records.is_empty());
    for (i, record) in records.iter().enumerate() {
        assert_eq!(record["seq"].as_u64(), Some(i as u64), "seq is monotonic from zero");
        assert!(record["ts"].as_u64().unwrap() > 0, "ts stamped on receipt");
    }
    let references_sidecar = records
        .iter()
        .filter_map(|r| r.get("output"))
        .any(|o| o["file"] == "boom.0.stderr");
    assert!(references_sidecar, "events.jsonl references the raw sidecar output");
}

#[test]
fn step_environment_is_recorded_as_run_metadata_and_run_dir_is_ephemeral() {
    let work = TempDir::new("wf");
    let log_dir = TempDir::new("log");

    let wf = r#"
root: main
workflows:
  main:
    entry: ok
    steps:
      ok:
        command: "true"
        gates:
          - { key: 0, target: { exit: 0 } }
"#;
    let wf_path = work.0.join("wf.yaml");
    std::fs::write(&wf_path, wf).unwrap();

    let status = Command::new(env!("CARGO_BIN_EXE_automedon"))
        .arg("run")
        .arg("--log-dir")
        .arg(&log_dir.0)
        .arg("--quiet")
        .arg(&wf_path)
        .status()
        .expect("failed to run orchestrator");
    assert_eq!(status.code(), Some(0));

    let run_dir = sole_run_dir(&log_dir.0);
    let run_id = run_dir.file_name().unwrap().to_str().unwrap();

    // The Step environment is recorded once as orchestrator-owned metadata.
    let meta: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(run_dir.join("meta.json")).unwrap()).unwrap();
    let scratch = meta["environment"]["AUTOMEDON_RUN_DIR"].as_str().expect("AUTOMEDON_RUN_DIR recorded");
    assert!(meta["environment"]["AUTOMEDON_WORKFLOW_DIR"].is_string(), "AUTOMEDON_WORKFLOW_DIR recorded");

    // The Run Directory is ephemeral (under the OS temp root) and shares its
    // run-id with the durable log dir, yet is a distinct directory.
    let scratch = PathBuf::from(scratch);
    assert!(scratch.starts_with(std::env::temp_dir()), "AUTOMEDON_RUN_DIR under the OS temp root");
    assert_eq!(scratch.file_name().unwrap().to_str().unwrap(), run_id, "shares the run-id");
    assert!(scratch.exists(), "the Run Directory exists after the Run");
    let _ = std::fs::remove_dir_all(&scratch);

    // The env values are not duplicated into the Kernel's control-plane log.
    let log = std::fs::read_to_string(run_dir.join("events.jsonl")).unwrap();
    assert!(!log.contains("AUTOMEDON_RUN_DIR"), "the Step environment stays out of events.jsonl");
}

#[test]
fn a_failed_run_prints_the_run_directory_to_stderr() {
    let work = TempDir::new("wf");
    let log_dir = TempDir::new("log");

    // A Step that fails and routes straight to a non-zero Exit Gate.
    let wf = r#"
root: main
workflows:
  main:
    entry: boom
    steps:
      boom:
        command: "exit 3"
        gates:
          - { key: 3, target: { exit: 3 } }
"#;
    let wf_path = work.0.join("wf.yaml");
    std::fs::write(&wf_path, wf).unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_automedon"))
        .arg("run")
        .arg("--log-dir")
        .arg(&log_dir.0)
        .arg("--quiet")
        .arg(&wf_path)
        .output()
        .expect("failed to run orchestrator");
    assert_eq!(output.status.code(), Some(3));

    let run_dir = sole_run_dir(&log_dir.0);
    let run_id = run_dir.file_name().unwrap().to_str().unwrap();

    // The failure pointer names the ephemeral Run Directory so an operator can
    // find the scratch the engine provided, without any Step echoing it.
    let stderr = String::from_utf8_lossy(&output.stderr);
    let scratch = config_run_scratch_dir(run_id);
    assert!(
        stderr.contains(scratch.to_str().unwrap()),
        "stderr should point at the Run Directory on failure; got: {stderr}"
    );
    let _ = std::fs::remove_dir_all(&scratch);
}

/// The expected `$AUTOMEDON_RUN_DIR` for a given run-id, mirroring the engine's resolver.
fn config_run_scratch_dir(run_id: &str) -> PathBuf {
    std::env::temp_dir().join("automedon").join("runs").join(run_id)
}

#[test]
fn runs_directory_is_pruned_to_the_retention_cap() {
    let work = TempDir::new("wf");
    let log_dir = TempDir::new("retain");

    // A trivial Workflow that exits cleanly, so each invocation mints one Run
    // directory under the shared log dir.
    let wf = r#"
root: main
workflows:
  main:
    entry: ok
    steps:
      ok:
        command: "true"
        gates:
          - { key: 0, target: { exit: 0 } }
"#;
    let wf_path = work.0.join("wf.yaml");
    std::fs::write(&wf_path, wf).unwrap();

    // Run more times than the cap; startup pruning (after this Run's directory
    // exists) must hold the directory to the newest `--keep` Runs.
    let keep = 2;
    for _ in 0..5 {
        let status = Command::new(env!("CARGO_BIN_EXE_automedon"))
            .arg("run")
            .arg("--log-dir")
            .arg(&log_dir.0)
            .arg("--keep")
            .arg(keep.to_string())
            .arg("--quiet")
            .arg(&wf_path)
            .status()
            .expect("failed to run orchestrator");
        assert_eq!(status.code(), Some(0));
    }

    let surviving = std::fs::read_dir(&log_dir.0)
        .expect("log dir exists")
        .filter(|e| e.as_ref().unwrap().path().is_dir())
        .count();
    assert_eq!(surviving, keep, "expected the runs dir pruned to the cap");
}
