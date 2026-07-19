Use the /code-review skill to review the unstaged changes against the intent
of the task at {{TASK_FILE}}. Write the findings to a file at
{{FINDINGS_FILE}}, grouping any Critical and Major findings under a
"## Blocking" heading and any Minor and Nit findings under a "## Suggestion"
heading.

If the change has a runnable surface — a new or changed subcommand, output
format, or script — run it and judge the actual output, not just the diff.
Keep the exercise cheap and inert: prefer `--dry-run`, stub mode
(`CODER_STUB=1`), or read-only invocations, and never launch a live agent
Run from inside this review.

Verify behavioral claims rather than trusting them, using this hierarchy: the
code is the source of truth for what happens; the task is the source of truth
for what should happen; comments and docs are claims about both. Check any
comment or doc claim against the code and its tests (including neighboring
crates), and check the code against the task's intent. Report any
disagreement as a finding, attributed to whichever layer contradicts the one
above it.

After writing the file, decide how to route this review:
{{DECISION_MENU}}
