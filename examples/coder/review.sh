#!/bin/sh
# `review` Step: read the TASK.md path from stdin, run a review agent over the
# unstaged changes, and re-emit the path on stdout. The findings (bulk) are
# written to $RUN_DIR/FINDINGS.md for the next `code` pass; the verdict (control)
# is surfaced as the exit code via the shared LLM helper, which derives the
# choices from this Step's own Gates ($AUTOMEDON_GATES) rather than hardcoding a
# verdict vocabulary here. The gate fails closed — only an explicit, valid
# decision can approve, so a review agent that crashes or emits no decision routes
# through the Default (escalate) instead of advancing un-reviewed code.
set -u

# Orchestration scratch lives in the ephemeral Run Directory the engine provides,
# never in the Repository this Step operates on. Fail loud if it is missing: a cwd
# fallback would pollute the deliverable and a per-script mktemp would break the
# cross-Step handoff (review writes the findings that the next code pass reads).
: "${RUN_DIR:?must be set by the orchestrator (the ephemeral Run Directory)}"

# The forkable example LLM helper: llm_prompt turns the routing contract into an
# outcome menu; llm_parse maps the reply back to a Gate key on the exit code.
. "$WORKFLOW_DIR/lib/llm.sh"

task_path="$(cat)"

if [ "${CODER_STUB:-}" = "1" ]; then
    printf '%s' "$task_path"
    case "${CODER_STUB_REVIEW:-clean}" in
        blocking) exit 1 ;;
        *) exit 0 ;;
    esac
fi

# Build the review task: drive the /code-review skill and write the findings to
# $RUN_DIR/FINDINGS.md, grouped Blocking vs Suggestion for the next code pass.
# Appending the menu from llm_prompt lets the model choose its outcome from this
# Step's Gates, closing the prompt/parse-vs-routing drift (ADR-0012).
task='Use the /code-review skill to review the unstaged changes. Write the
findings to a file at '"$RUN_DIR"'/FINDINGS.md, grouping any
Critical and Major findings under a "## Blocking" heading and any Minor and Nit
findings under a "## Suggestion" heading.

After writing the file, decide how to route this review:
'"$(llm_prompt)"

# Run the review agent unattended under a scoped permission policy. The
# /code-review skill drives many read tools, so the policy uses bypassPermissions
# (no prompts to hang a non-interactive Step) rather than a narrow allowlist that
# would block the skill. Its deny rules still hold the line that matters: review
# may read freely and write its findings, but cannot edit crate source or stage,
# commit, or push — so no review-introduced change can advance un-reviewed.
reply="$(claude --settings "${0%/*}/review.permissions.json" -p "$task" 2>/dev/null)"

# Re-emit the path (the out-Message) before mapping the verdict. llm_parse is
# stdout-silent and exits with the chosen Gate key, failing closed to the Default
# on any missing or unusable decision.
printf '%s' "$task_path"
printf '%s' "$reply" | llm_parse
