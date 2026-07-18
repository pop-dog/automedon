Push the current branch with `git push -u origin HEAD`, then use `gh pr
create` to open a ready-for-review pull request (do not pass `--base`; let
it default to the repository's default branch).

Write the description with `--body` inline, following this repository's
pull request standards in `.claude/CLAUDE.md` (Background, Why, Approach,
Testing) for structure and tone. Use the task at {{TASK_FILE}} and the
commits on this branch for the content. End the body with a `Fixes #<n>`
line naming the issue the task file is titled with, so the merge closes it.
