#!/bin/sh
# `create-pr` Step: read the TASK.md path from stdin, push the current branch
# and open a ready-for-review PR describing the change, then re-emit the
# path. The wrapper's final Step — its Exit Gate surfaces 0 (published) to
# the caller.
#
# Knobs: CODER_STUB=1 keeps the Step inert (CODER_STUB_PR_CODE sets its exit
# code); CODER_PR_MODEL picks the `claude` model — sonnet by default, enough
# for pushing and writing a PR description.
set -u

: "${AUTOMEDON_RUN_DIR:?must be set by the orchestrator (the ephemeral Run Directory)}"

. "$AUTOMEDON_WORKFLOW_DIR/lib/llm.sh"

# The forkable example repo-location helper: task_repo_cd puts this Step in
# the worktree `checkout` created for this issue, which is a sibling of the
# repo the orchestrator was started in.
. "$AUTOMEDON_WORKFLOW_DIR/lib/repo.sh"

task_path="$(cat)"

if [ "${CODER_STUB:-}" = "1" ]; then
    printf '%s' "$task_path"
    exit "${CODER_STUB_PR_CODE:-0}"
fi

task_repo_cd "$task_path" || exit 1

prompt="$(llm_render "${0%/*}/prompts/create-pr.md" TASK_FILE="$task_path")" || exit 1

# Run the PR agent unattended under a scoped permission policy. Its deny
# rules hold the line that matters: it may push the current branch and open
# or edit a PR, but cannot force-push, delete a remote branch, or merge,
# close, or review the PR it just opened.
claude --settings "${0%/*}/create-pr.permissions.json" --model "${CODER_PR_MODEL:-sonnet}" -p "$prompt" 1>&2
code=$?

printf '%s' "$task_path"
exit "$code"
