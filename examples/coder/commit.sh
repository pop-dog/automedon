#!/bin/sh
# `commit` Step: read the TASK.md path from stdin, write a commit message from
# the diff and the task, and commit on the current branch. Never pushes — a
# human is responsible for being on a sensible branch. Re-emits the path.
set -u

task_path="$(cat)"

if [ "${CODER_STUB:-}" = "1" ]; then
    printf '%s' "$task_path"
    exit 0
fi

prompt="Use the /commit skill to commit the current changes on the current
branch, using the task in the file at ${task_path} for intent. Do not push."

# Least-privilege permissions for this Step: git add/commit only, never push.
# The policy lives beside the script so the "never pushes" invariant is explicit.
claude --settings "${0%/*}/commit.permissions.json" -p "$prompt" 1>&2
code=$?

# On a successful commit the review's findings have been addressed and approved,
# so the shared findings file is discarded. The escalation path never reaches
# this Step, so a spent-Budget run instead leaves FINDINGS.md in place.
if [ "$code" -eq 0 ]; then
    rm -f FINDINGS.md
fi

printf '%s' "$task_path"
exit "$code"
