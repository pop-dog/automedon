//! End-to-end checks that `automedon validate` surfaces the two per-Step `env:`
//! load errors (#58): `env:` on a Composite Step, and an `AUTOMEDON_`-prefixed
//! author key. Both are rejected inside `loader::load`, which `validate` calls
//! before running its own graph checks, so this exercises the CLI boundary
//! rather than re-testing the loader unit tests.

use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Output};

fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_automedon"))
        .args(args)
        .output()
        .expect("failed to run orchestrator")
}

fn write_workflow(tag: &str, body: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("ao-validate-env-{tag}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("wf.yaml");
    let mut file = std::fs::File::create(&path).unwrap();
    file.write_all(body.as_bytes()).unwrap();
    path
}

#[test]
fn env_on_a_composite_step_is_reported() {
    let path = write_workflow(
        "composite",
        r#"
root: main
workflows:
  main:
    entry: call
    steps:
      call:
        workflow: child
        env:
          FOO: bar
        gates:
          - { key: 0, target: { exit: 0 } }
  child:
    entry: work
    steps:
      work:
        command: "exit 0"
        gates:
          - { key: 0, target: { exit: 0 } }
"#,
    );
    let out = run(&["validate", path.to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(stderr.contains("workflow:"), "{stderr}");
}

#[test]
fn a_reserved_automedon_prefixed_key_is_reported() {
    let path = write_workflow(
        "prefix",
        r#"
root: main
workflows:
  main:
    entry: a
    steps:
      a:
        command: "exit 0"
        env:
          AUTOMEDON_FOO: bar
        gates:
          - { key: 0, target: { exit: 0 } }
"#,
    );
    let out = run(&["validate", path.to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(stderr.contains("AUTOMEDON_"), "{stderr}");
}
