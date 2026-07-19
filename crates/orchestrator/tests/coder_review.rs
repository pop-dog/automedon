//! Guards the coder Workflow's review Step against the three escape modes
//! documented in issue #56: no spec input (`{{TASK_FILE}}` never reached the
//! prompt), no exercise of the deliverable's actual output, and no
//! verification of behavioral claims in comments/docs against the code and
//! task. Static content checks, mirroring `skill_docs.rs`'s style: these
//! guardrails are prompt text, not executable behavior, so the tests assert on
//! the shipped strings rather than driving an LLM.

use std::path::PathBuf;

fn repo_root() -> PathBuf {
    PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
}

fn read(rel: &str) -> String {
    let path = repo_root().join(rel);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

#[test]
fn review_sh_passes_task_file_to_llm_render() {
    // review.md's {{TASK_FILE}} placeholder needs a matching pair from the
    // caller or llm_render fails closed (see llm_helper.rs). review.sh already
    // holds task_path (it read it from stdin) — this asserts it forwards that
    // value under the TASK_FILE key.
    let review_sh = read("examples/coder/review.sh");
    assert!(
        review_sh.contains("TASK_FILE=\"$task_path\""),
        "review.sh should pass the task file path as TASK_FILE to llm_render; got:\n{review_sh}"
    );
}

#[test]
fn review_md_contains_task_file_placeholder() {
    let review_md = read("examples/coder/prompts/review.md");
    assert!(
        review_md.contains("{{TASK_FILE}}"),
        "review.md should reference {{{{TASK_FILE}}}} so the review has spec input; got:\n{review_md}"
    );
}

#[test]
fn review_md_instructs_reviewing_against_task_intent() {
    // Escape #51: the review Step faithfully implemented an
    // intent-contradicting decision because nothing pointed it at the task.
    let review_md = read("examples/coder/prompts/review.md");
    assert!(
        review_md.contains("intent"),
        "review.md should instruct reviewing the changes against the task's \
         intent; got:\n{review_md}"
    );
}

#[test]
fn review_md_instructs_exercising_the_artifact() {
    // Escape #55: the Mermaid `graph` output embedded absolute filesystem
    // paths, invisible in the diff because nothing ran the deliverable.
    let review_md = read("examples/coder/prompts/review.md");
    let lower = review_md.to_lowercase();
    assert!(
        lower.contains("run it") || lower.contains("exercise"),
        "review.md should instruct running/exercising a runnable deliverable \
         and judging its actual output, not just the diff; got:\n{review_md}"
    );
}

#[test]
fn review_md_instructs_claim_verification_with_truth_hierarchy() {
    // Escape #52: a comment claimed a loop "spins forever" while the kernel's
    // own tests show the opposite. The prompt must state the truth hierarchy
    // (code > task > comments) so the reviewer verifies claims rather than
    // trusting them.
    let review_md = read("examples/coder/prompts/review.md");
    let lower = review_md.to_lowercase();
    assert!(
        lower.contains("source of truth"),
        "review.md should state a truth hierarchy for verifying claims; got:\n{review_md}"
    );
    assert!(
        lower.contains("comment"),
        "review.md should call out comments/docs as claims to verify, not \
         trust; got:\n{review_md}"
    );
}

#[test]
fn skill_outcome_instructs_conversation_only_first_pass_review() {
    // The skill-driving agent holds provenance (the originating issue's
    // discussion) the in-Workflow reviewer lacks, so it performs its own
    // first pass on success — conversation only, no PR comments or fixes.
    let path = repo_root().join("skills/autocoder/SKILL.md");
    let skill = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));

    let zero_branch = skill
        .split("- `90`")
        .next()
        .expect("SKILL.md should have a `0` outcome branch before the `90` branch");
    assert!(
        zero_branch.contains("first-pass") || zero_branch.contains("first pass"),
        "the `0` outcome branch should instruct a first-pass review; got:\n{zero_branch}"
    );
    assert!(
        zero_branch.to_lowercase().contains("conversation"),
        "the first-pass review should be conversation-only; got:\n{zero_branch}"
    );
    assert!(
        zero_branch.contains("no PR comments") || zero_branch.to_lowercase().contains("no pr comment"),
        "the first-pass review must not post PR comments; got:\n{zero_branch}"
    );
}
