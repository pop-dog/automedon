#!/bin/sh
# `commit` Step: read the TASK.md path from stdin, write a commit message from
# the diff and the task, and commit on the current branch. Never pushes — a
# human is responsible for being on a sensible branch. Re-emits the path.
#
# Knobs: CODER_STUB=1 keeps the Step inert; CODER_COMMIT_MODEL picks the
# `claude` model — haiku by default, enough for writing a commit message.
set -u

# Orchestration scratch lives in the ephemeral Run Directory the engine provides,
# never in the Repository this Step operates on. Fail loud if it is missing: a cwd
# fallback would pollute the deliverable and a per-script mktemp would break the
# cross-Step handoff (review writes the findings that the next code pass reads).
: "${AUTOMEDON_RUN_DIR:?must be set by the orchestrator (the ephemeral Run Directory)}"

# The forkable example LLM helper: llm_render fills the prompt template that
# lives next to this script under prompts/.
. "$AUTOMEDON_WORKFLOW_DIR/lib/llm.sh"

# The forkable example repo-location helper: task_repo_cd puts this Step in
# the repository that actually holds the task file, which may be a sibling
# git worktree the orchestrator was not started in (see the autocoder
# wrapper's checkout.sh).
. "$AUTOMEDON_WORKFLOW_DIR/lib/repo.sh"

task_path="$(cat)"

if [ "${CODER_STUB:-}" = "1" ]; then
    printf '%s' "$task_path"
    exit 0
fi

task_repo_cd "$task_path" || exit 1

prompt="$(llm_render "${0%/*}/prompts/commit.md" TASK_FILE="$task_path")" || exit 1

# Run the commit agent unattended under a scoped permission policy. A Workflow
# Step is non-interactive, so the policy uses bypassPermissions (no prompts to
# hang on) while its deny rules still enforce the "never pushes" invariant — a
# denied tool cannot be re-allowed by anything the agent does. The narrow git
# toolset here keeps that allowlist short; the broad code Step cannot.
claude --settings "${0%/*}/commit.permissions.json" --model "${CODER_COMMIT_MODEL:-haiku}" -p "$prompt" 1>&2
code=$?

# On a successful commit the review's findings have been addressed and approved,
# so the shared findings file is discarded. The escalation path never reaches
# this Step, so a spent-Budget run instead leaves the findings in place.
if [ "$code" -eq 0 ]; then
    rm -f "$AUTOMEDON_RUN_DIR/FINDINGS.md"
fi

printf '%s' "$task_path"
exit "$code"
