Use `gh issue view {{ISSUE_REF}}` to read the GitHub issue (it accepts an
issue number or URL verbatim). Distill it into a concrete task specification
and write it as `{{RUN_DIR}}/TASK.md`.

Choose a full branch name for the fix: a conventional-commit type (feat,
fix, etc.) followed by a slash and a slug that starts with the issue number,
e.g. `fix/14-prune-race`.

End your reply with exactly one final line of the form `BRANCH: <branch-name>`.
