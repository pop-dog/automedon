#!/bin/sh
# `review` Step: read the TASK.md path from stdin, run a review agent over the
# unstaged changes, and re-emit the path on stdout. The findings (bulk) are
# written to FINDINGS.md in the working directory for the next `code` pass; the
# verdict (control) is surfaced as the exit code: 0 approves (route to commit),
# 1 means Blocking findings remain (route back to code), and any other code
# escalates. The gate fails closed — only an explicit CLEAN verdict approves, so
# a review agent that crashes or emits no verdict never advances un-reviewed
# code to commit.
set -u

task_path="$(cat)"

if [ "${CODER_STUB:-}" = "1" ]; then
    printf '%s' "$task_path"
    case "${CODER_STUB_REVIEW:-clean}" in
        blocking) exit 1 ;;
        *) exit 0 ;;
    esac
fi

# Wrap the /code-review skill. Critical and Major findings are Blocking; Minor
# and Nit are Suggestions. The agent prints a final sentinel line that maps the
# review onto the exit code — the bespoke output-to-exit-code parse that the LLM
# Module will later generalise.
prompt='Use the /code-review skill to review the unstaged changes. Write the
findings to a file named FINDINGS.md in the current directory, grouping any
Critical and Major findings under a "## Blocking" heading and any Minor and Nit
findings under a "## Suggestion" heading. After writing the file, print exactly
one final line on its own: "VERDICT: BLOCKING" if there are any Critical or
Major findings, otherwise "VERDICT: CLEAN".'

# Least-privilege permissions for this Step: write only FINDINGS.md and read the
# diff; it may not edit source or commit. The policy lives beside the script.
verdict="$(claude --settings "${0%/*}/review.permissions.json" -p "$prompt" 2>/dev/null | grep -E '^VERDICT: (BLOCKING|CLEAN)$' | tail -n 1)"

printf '%s' "$task_path"
case "$verdict" in
    "VERDICT: CLEAN") exit 0 ;;
    "VERDICT: BLOCKING") exit 1 ;;
    # A missing or malformed verdict means no usable review result; escalate
    # through the Step's catch-all Gate rather than approving by default.
    *) exit 2 ;;
esac
