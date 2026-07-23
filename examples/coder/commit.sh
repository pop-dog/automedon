#!/bin/sh
# `commit` Step: read the TASK.md path from stdin, stage and commit the tree
# the `code` Step produced onto the current branch, and re-emit the path on
# stdout. Never pushes — a human is responsible for being on a sensible
# branch.
#
# The script stages and commits; the agent only writes the message. Keeping
# staging in the script is what makes new files as reliable as tracked edits.
#
# Knobs: CODER_STUB=1 keeps the Step inert; model picks the `claude` model —
# haiku by default, enough for writing a commit message.
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

# Resolve to absolute: task_repo_cd changes cwd below, after which a relative
# path would resolve wrong and misfire the git reset. The out-Message re-emits
# the original, so a caller sees back exactly what it sent.
task_path_abs="$task_path"
case "$task_path_abs" in
    /*) ;;
    *) task_path_abs="$(pwd)/$task_path_abs" ;;
esac

task_repo_cd "$task_path_abs" || exit 1

# Stage everything, so files code created but never re-touched ride along, not
# just tracked edits.
git add -A 1>&2 || exit 1
# Drop the task file: in standalone mode it is a tracked file in the repo, and
# is orchestration input, not a deliverable to commit.
git reset -- "$task_path_abs" 1>&2 || exit 1

# Review already approved changes, so an empty tree is an upstream fault, not a
# no-op — escalate instead of letting git commit fail opaquely.
if git diff --cached --quiet; then
    printf 'commit: nothing staged after git add -A; review approved changes but none were found\n' >&2
    exit 1
fi

prompt="$(llm_render "${0%/*}/prompts/commit.md" TASK_FILE="$task_path_abs")" || exit 1

# bypassPermissions lets this non-interactive Step run without prompts it could
# never answer; the policy still denies the agent any git or file write, so
# only this script mutates the repo.
reply="$(claude --settings "${0%/*}/commit.permissions.json" --model "${model:-haiku}" -p "$prompt" 2>/dev/null)"
claude_status=$?

# On failure, unstage: never commit under a garbage message, never leave a
# staged tree behind for a human to trip over.
if [ "$claude_status" -ne 0 ] || [ -z "$reply" ]; then
    printf 'commit: agent failed to produce a commit message (exit %s)\n' "$claude_status" >&2
    git reset 1>&2
    exit 1
fi

if ! printf '%s' "$reply" | git commit -F - 1>&2; then
    git reset 1>&2
    exit 1
fi

# Reaching here means the commit succeeded — every failure exits earlier — so
# the review findings are consumed and the shared file can go.
rm -f "$AUTOMEDON_RUN_DIR/FINDINGS.md"

printf '%s' "$task_path"
exit 0
