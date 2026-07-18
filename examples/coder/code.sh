#!/bin/sh
# `code` Step: read the TASK.md path from stdin (the in-Message), drive a coding
# agent to implement it, and re-emit the path on stdout (the out-Message). All
# agent output is sent to stderr so stdout carries only the Message.
#
# This Step is purely the coding agent. The objective build/test gate is a
# separate `build-test` Step, so `code` exits 0 once the agent finishes and
# non-zero only when the agent itself fails to run (an escalating error). The
# agent is still asked to build and test before exiting — the cheapest place to
# catch a failure — but the deterministic gate is what the kernel routes on.
#
# Knobs: CODER_STUB=1 keeps the Step inert (CODER_STUB_CODE sets its exit code);
# CODER_CODE_MODEL picks the `claude` model — sonnet by default, matched to the
# bulk coding work this Step does.
set -u

# Orchestration scratch lives in the ephemeral Run Directory the engine provides,
# never in the Repository this Step operates on. Fail loud if it is missing: a cwd
# fallback would pollute the deliverable and a per-script mktemp would break the
# cross-Step handoff (review writes the findings that the next code pass reads).
: "${AUTOMEDON_RUN_DIR:?must be set by the orchestrator (the ephemeral Run Directory)}"

# The forkable example LLM helper: llm_render fills the prompt templates that
# live next to this script under prompts/.
. "$AUTOMEDON_WORKFLOW_DIR/lib/llm.sh"

# The forkable example repo-location helper: task_repo_cd puts this Step in
# the repository that actually holds the task file, which may be a sibling
# git worktree the orchestrator was not started in (see the autocoder
# wrapper's checkout.sh).
. "$AUTOMEDON_WORKFLOW_DIR/lib/repo.sh"

task_path="$(cat)"

# Stub mode keeps the Step inert (no agent, no edits, no cd) so the
# Workflow's routing and totality can be tested without invoking an LLM or
# requiring the in-Message to be a real path.
if [ "${CODER_STUB:-}" = "1" ]; then
    printf '%s' "$task_path"
    exit "${CODER_STUB_CODE:-0}"
fi

task_repo_cd "$task_path" || exit 1

# On a loop-back the agent is re-entered to fix what the deterministic Steps
# caught: a review's Blocking findings and/or a failing build/test run, each left
# in its own file in the ephemeral Run Directory. Each applicable fragment is
# rendered and appended (with a separating space) into the {{REVISE}} slot;
# a first pass leaves it empty.
revise=""
if [ -f "$AUTOMEDON_RUN_DIR/FINDINGS.md" ]; then
    revise="${revise}$(llm_render "${0%/*}/prompts/code-revise-findings.md" \
        FINDINGS_FILE="$AUTOMEDON_RUN_DIR/FINDINGS.md") " || exit 1
fi
if [ -f "$AUTOMEDON_RUN_DIR/BUILD_FAILURE.md" ]; then
    revise="${revise}$(llm_render "${0%/*}/prompts/code-revise-build.md" \
        BUILD_FAILURE_FILE="$AUTOMEDON_RUN_DIR/BUILD_FAILURE.md") " || exit 1
fi

prompt="$(llm_render "${0%/*}/prompts/code.md" \
    TASK_FILE="$task_path" REVISE="$revise")" || exit 1

# Run the coding agent unattended: a Workflow Step is non-interactive, so there
# is no human to answer permission prompts. Unlike the narrow commit and review
# Steps, this Step edits source and drives the /tdd skill, so its toolset is too
# broad to allowlist; it is isolated by running against a throwaway branch/working
# copy instead.
claude --dangerously-skip-permissions --model "${CODER_CODE_MODEL:-sonnet}" -p "$prompt" 1>&2
code=$?

# A non-zero exit is the agent/CLI failing to run, not a build result (the
# build-test Step owns that), so surface it for the catch-all Gate to escalate.
printf '%s' "$task_path"
exit "$code"
