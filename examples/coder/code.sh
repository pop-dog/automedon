#!/bin/sh
# `code` Step: read the TASK.md path from stdin (the in-Message), drive a coding
# agent to implement it, and re-emit the path on stdout (the out-Message). All
# agent output is sent to stderr so stdout carries only the Message.
#
# The exit code is the objective build result, not the agent's self-report: 0
# when the test suite passes (route to review), non-zero otherwise (escalate).
set -u

task_path="$(cat)"

# Stub mode keeps the Step inert (no agent, no edits) so the Workflow's routing
# and totality can be tested without invoking an LLM.
if [ "${CODER_STUB:-}" = "1" ]; then
    printf '%s' "$task_path"
    exit "${CODER_STUB_CODE:-0}"
fi

# On a loop-back the previous review left its Blocking findings here.
revise=""
if [ -f FINDINGS.md ]; then
    revise="A previous review left Blocking findings in FINDINGS.md; address them. "
fi

prompt="Use the /tdd skill to implement the task described in the file at
${task_path}. ${revise}Work test-first: write a failing test, make it pass, then
refactor, iterating until the build and tests are green. Leave all changes
unstaged and do not commit."

# Least-privilege permissions for this Step (edit + cargo only; no git). The
# policy lives beside the script so it is cwd-independent and reviewable.
claude --settings "${0%/*}/code.permissions.json" -p "$prompt" 1>&2

# The kernel orchestrates the meaningful checkpoint: an objective green build.
if cargo build 1>&2 && cargo test 1>&2; then
    printf '%s' "$task_path"
    exit 0
fi
printf '%s' "$task_path"
exit 1
