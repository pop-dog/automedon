//! End-to-end checks of the self-describing help surface (#24). Explicit help is
//! a success (usage to stdout, exit 0); a missing or unknown subcommand is an
//! error (usage to stderr, non-zero). These drive the real binary and assert
//! both the exit code and which stream the usage text lands on.

use std::path::PathBuf;
use std::process::{Command, Output};

/// Invoke the binary with the given args, returning its captured output.
fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_automedon"))
        .args(args)
        .output()
        .expect("failed to run orchestrator")
}

/// A throwaway directory under the system temp dir, removed on Drop.
struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> Self {
        let path = std::env::temp_dir().join(format!("ao-help-{tag}-{}", uuid::Uuid::now_v7()));
        std::fs::create_dir_all(&path).unwrap();
        TempDir(path)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn help_lists_commands_on_stdout_and_exits_zero() {
    let out = run(&["help"]);
    assert_eq!(out.status.code(), Some(0));
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains("run"), "stdout should list the run command: {stdout}");
    assert!(stdout.contains("help"), "stdout should list the help command: {stdout}");
    assert!(out.stderr.is_empty(), "explicit help should write nothing to stderr");
}

#[test]
fn help_flags_match_the_help_command() {
    for flag in ["--help", "-h"] {
        let out = run(&[flag]);
        assert_eq!(out.status.code(), Some(0), "{flag} should exit 0");
        let stdout = String::from_utf8(out.stdout).unwrap();
        assert!(stdout.contains("run"), "{flag} stdout should list run: {stdout}");
        assert!(stdout.contains("help"), "{flag} stdout should list help: {stdout}");
    }
}

#[test]
fn run_help_prints_run_usage_on_stdout_and_exits_zero() {
    let out = run(&["run", "--help"]);
    assert_eq!(out.status.code(), Some(0));
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(
        stdout.contains("usage: automedon run <workflow.yaml>"),
        "stdout should show the run usage: {stdout}"
    );
    assert!(out.stderr.is_empty(), "explicit help should write nothing to stderr");
}

#[test]
fn no_args_and_unknown_subcommand_write_usage_to_stderr_and_fail() {
    for args in [&["bogus"][..], &[][..]] {
        let out = run(args);
        assert_ne!(out.status.code(), Some(0), "{args:?} should exit non-zero");
        assert!(out.stdout.is_empty(), "a usage error should write nothing to stdout: {args:?}");
        let stderr = String::from_utf8(out.stderr).unwrap();
        assert!(
            stderr.contains("usage: automedon"),
            "{args:?} stderr should carry the usage: {stderr}"
        );
    }
}

#[test]
fn a_normal_run_invocation_is_unchanged() {
    // Adding the help surface must not disturb dispatch of a real `run`: a
    // single-Step Workflow that exits 0 still runs and surfaces that code.
    let work = TempDir::new("work");
    let log_dir = TempDir::new("log");
    let wf_path = work.0.join("wf.yaml");
    let yaml = "\
root: main
workflows:
  main:
    entry: a
    steps:
      a:
        command: \"exit 0\"
        gates:
          - { key: 0, target: { exit: 0 } }
";
    std::fs::write(&wf_path, yaml).unwrap();

    let out = Command::new(env!("CARGO_BIN_EXE_automedon"))
        .arg("run")
        .arg("--log-dir")
        .arg(&log_dir.0)
        .arg("--quiet")
        .arg(&wf_path)
        .output()
        .expect("failed to run orchestrator");

    assert_eq!(out.status.code(), Some(0));
}
