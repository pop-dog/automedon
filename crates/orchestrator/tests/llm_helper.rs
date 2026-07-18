//! Exercises the shipped `examples/lib/llm.sh` example helper directly, mirroring
//! the style of the other shell-script integration tests (`install_script.rs`).
//!
//! The helper is forkable example code, not a crate, so it is tested the way a
//! user would consume it: source the file in a `/bin/sh` shell, set
//! `$AUTOMEDON_GATES` (the routing contract the engine injects), and assert on
//! `llm_prompt`'s menu and `llm_parse`'s exit code. `llm_parse` must be
//! stdout-silent — the Gate key rides the exit code — so the tests also assert an
//! empty stdout.

use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

fn repo_root() -> PathBuf {
    PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
}

fn lib_llm() -> PathBuf {
    repo_root().join("examples/lib/llm.sh")
}

/// The standard three-Gate review contract: 0 approve, 1 revise, `*` escalate.
const REVIEW_GATES: &str =
    r#"[{"key":"0","when":"approve"},{"key":"1","when":"revise"},{"key":"*","when":"escalate"}]"#;

/// Source `examples/lib/llm.sh` and run `body` under it with `$AUTOMEDON_GATES`
/// set to `gates` and `stdin` piped in. Returns (exit_code, stdout, stderr).
fn run_llm(gates: &str, stdin: &str, body: &str) -> (i32, String, String) {
    let script = format!(". \"{}\"\n{}", lib_llm().display(), body);
    let mut child = Command::new("sh")
        .arg("-c")
        .arg(script)
        .env("AUTOMEDON_GATES", gates)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to spawn sh");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(stdin.as_bytes())
        .unwrap();
    let out = child.wait_with_output().expect("failed to wait on sh");
    (
        out.status.code().expect("sh killed by signal"),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

/// A throwaway template file under the system temp dir, removed on Drop.
struct TempTemplate(PathBuf);

impl TempTemplate {
    fn new(tag: &str, content: &str) -> Self {
        let path = std::env::temp_dir().join(format!("ao-llm-{tag}-{}.md", uuid::Uuid::now_v7()));
        std::fs::write(&path, content).expect("failed to write template");
        TempTemplate(path)
    }
}

impl Drop for TempTemplate {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

#[test]
fn render_substitutes_a_single_pair() {
    let tpl = TempTemplate::new("single", "Implement the task at {{TASK_FILE}} now.\n");
    let body = format!("llm_render \"{}\" TASK_FILE=/work/TASK.md", tpl.0.display());
    let (code, stdout, stderr) = run_llm(REVIEW_GATES, "", &body);
    assert_eq!(code, 0, "render should succeed; stderr:\n{stderr}");
    // Exact equality doubles as the stdout-hygiene check: nothing but the
    // rendered prompt may appear on stdout.
    assert_eq!(stdout, "Implement the task at /work/TASK.md now.\n");
}

#[test]
fn render_substitutes_multiple_pairs() {
    // Values may span lines (llm_prompt's menu fills {{DECISION_MENU}}) and a
    // placeholder may legitimately expand to empty (a no-op {{REVISE}}).
    let tpl = TempTemplate::new(
        "multi",
        "Review {{TASK_FILE}}. {{REVISE}}Choose:\n{{DECISION_MENU}}\n",
    );
    let body = format!(
        "llm_render \"{}\" TASK_FILE=/work/TASK.md REVISE= 'DECISION_MENU=  DECISION: 0\n  DECISION: 1'",
        tpl.0.display()
    );
    let (code, stdout, stderr) = run_llm(REVIEW_GATES, "", &body);
    assert_eq!(code, 0, "render should succeed; stderr:\n{stderr}");
    assert_eq!(
        stdout,
        "Review /work/TASK.md. Choose:\n  DECISION: 0\n  DECISION: 1\n"
    );
}

#[test]
fn render_errors_on_an_unresolved_placeholder() {
    // A placeholder no pair fills means the prompt would ship with literal
    // `{{...}}` text; the render must refuse rather than emit a broken prompt.
    let tpl = TempTemplate::new("unresolved", "Fix {{TASK_FILE}} and {{FINDINGS_FILE}}.\n");
    let body = format!("llm_render \"{}\" TASK_FILE=/work/TASK.md", tpl.0.display());
    let (code, stdout, stderr) = run_llm(REVIEW_GATES, "", &body);
    assert_ne!(code, 0, "an unresolved placeholder must be a hard error");
    assert_eq!(stdout, "", "no partial prompt may leak to stdout");
    assert!(
        stderr.contains("FINDINGS_FILE"),
        "stderr should name the unresolved placeholder; got:\n{stderr}"
    );
}

#[test]
fn render_errors_on_an_unused_pair() {
    // A pair matching no placeholder means the caller believes text it supplied
    // reaches the prompt when it silently would not; the render must refuse.
    let tpl = TempTemplate::new("unused", "Implement {{TASK_FILE}}.\n");
    let body = format!(
        "llm_render \"{}\" TASK_FILE=/work/TASK.md REVISE='fix it'",
        tpl.0.display()
    );
    let (code, stdout, stderr) = run_llm(REVIEW_GATES, "", &body);
    assert_ne!(code, 0, "an unused pair must be a hard error");
    assert_eq!(stdout, "", "no prompt may leak to stdout");
    assert!(
        stderr.contains("REVISE"),
        "stderr should name the unused pair; got:\n{stderr}"
    );
}

#[test]
fn parse_exits_with_the_chosen_code_key() {
    // A valid `DECISION: <key>` naming a Code gate exits with that integer, and
    // emits nothing on stdout (the key rides the exit code, not the Message).
    let (code, stdout, _stderr) = run_llm(REVIEW_GATES, "DECISION: 1\n", "llm_parse");
    assert_eq!(code, 1, "DECISION: 1 should exit 1");
    assert_eq!(stdout, "", "llm_parse must be stdout-silent");
}

#[test]
fn review_sources_the_helper_and_drops_the_hardcoded_vocabulary() {
    // review.sh must drive the shared helper rather than its old bespoke parse:
    // the verdict vocabulary now derives from the Step's own Gates, so the
    // hardcoded VERDICT/CLEAN/BLOCKING tokens are gone.
    let review = std::fs::read_to_string(repo_root().join("examples/coder/review.sh"))
        .expect("review.sh exists");
    assert!(
        review.contains("lib/llm.sh"),
        "review.sh should source the lib/llm.sh helper"
    );
    for token in ["VERDICT:", "CLEAN", "BLOCKING"] {
        assert!(
            !review.contains(token),
            "review.sh should no longer hardcode the {token} vocabulary"
        );
    }
}

#[test]
fn prompt_lists_code_gates_and_excludes_the_default() {
    // The menu offers one entry per Code gate, each naming its key and `when`
    // rationale, and omits the "*" Default — it is the implicit fallback, not a
    // choice the model selects.
    let (code, stdout, _stderr) = run_llm(REVIEW_GATES, "", "llm_prompt");
    assert_eq!(code, 0, "llm_prompt should succeed");
    for (key, when) in [("0", "approve"), ("1", "revise")] {
        assert!(
            stdout.contains(key) && stdout.contains(when),
            "menu should show gate {key} ({when}); got:\n{stdout}"
        );
    }
    assert!(
        !stdout.contains("escalate"),
        "menu must not list the Default gate; got:\n{stdout}"
    );
}

#[test]
fn prompt_instructs_a_final_decision_line() {
    // The menu ends with the contract the parser relies on: a single final
    // `DECISION: <key>` line.
    let (_code, stdout, _stderr) = run_llm(REVIEW_GATES, "", "llm_prompt");
    assert!(
        stdout.contains("DECISION:"),
        "menu should instruct ending with a DECISION line; got:\n{stdout}"
    );
}

#[test]
fn parse_takes_the_last_decision_line() {
    // A reply may reason aloud and restate its choice; the final DECISION wins.
    let reply = "DECISION: 0\non reflection, blocking issues remain\nDECISION: 1\n";
    let (code, _stdout, _stderr) = run_llm(REVIEW_GATES, reply, "llm_parse");
    assert_eq!(code, 1, "the last DECISION line should win");
}

#[test]
fn parse_falls_through_on_an_unlisted_decision() {
    // A well-formed DECISION naming a key no gate offers is still unusable: it must
    // fail closed to the Default, never approve.
    let (code, _stdout, _stderr) = run_llm(REVIEW_GATES, "DECISION: 7\n", "llm_parse");
    assert!(
        ![0, 1].contains(&code),
        "an unlisted key {code} must not match a Code gate"
    );
}

#[test]
fn parse_falls_through_to_default_on_a_missing_decision() {
    // No decision line means no usable result; llm_parse must fail closed with a
    // code that is *not* any Code gate so routing hits the Default. The gate set
    // deliberately claims 0, 1 and 2, so a hardcoded `exit 2` would not pass.
    let gates = r#"[{"key":"0","when":"a"},{"key":"1","when":"b"},{"key":"2","when":"c"},{"key":"*","when":"d"}]"#;
    let (code, stdout, _stderr) = run_llm(gates, "I have no opinion\n", "llm_parse");
    assert!(code != 0, "a missing decision must not approve");
    assert!(
        ![1, 2].contains(&code),
        "fallthrough code {code} must not collide with a Code gate"
    );
    assert_eq!(stdout, "", "llm_parse must be stdout-silent");
}
