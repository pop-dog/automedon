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
set -u

task_path="$(cat)"

# Stub mode keeps the Step inert (no agent, no edits) so the Workflow's routing
# and totality can be tested without invoking an LLM.
if [ "${CODER_STUB:-}" = "1" ]; then
    printf '%s' "$task_path"
    exit "${CODER_STUB_CODE:-0}"
fi

# On a loop-back the agent is re-entered to fix what the deterministic Steps
# caught: a review's Blocking findings and/or a failing build/test run, each left
# in its own file in the working directory.
revise=""
if [ -f FINDINGS.md ]; then
    revise="${revise}A previous review left Blocking findings in FINDINGS.md; address them. "
fi
if [ -f BUILD_FAILURE.md ]; then
    revise="${revise}A previous build/test run failed; its output is in BUILD_FAILURE.md; fix the cause. "
fi

prompt="Use the /tdd skill to implement the task described in the file at
${task_path}. ${revise}Work test-first: write a failing test, make it pass, then
refactor, iterating until the build and tests are green before you exit. Leave
all changes unstaged and do not commit."

# Run the coding agent unattended: a Workflow Step is non-interactive, so there
# is no human to answer permission prompts. Unlike the narrow commit and review
# Steps, this Step edits source and drives the /tdd skill, so its toolset is too
# broad to allowlist; it is isolated by running against a throwaway branch/working
# copy instead.
claude --dangerously-skip-permissions -p "$prompt" 1>&2
code=$?

# A non-zero exit is the agent/CLI failing to run, not a build result (the
# build-test Step owns that), so surface it for the catch-all Gate to escalate.
printf '%s' "$task_path"
exit "$code"
