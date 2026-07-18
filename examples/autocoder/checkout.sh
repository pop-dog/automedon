#!/bin/sh
# `checkout` Step: read the branch name `distill` emitted from stdin, prepare
# a dedicated git worktree for this issue, move the staged TASK.md into it,
# and re-emit the worktree-absolute TASK.md path on stdout. The only
# git-mutating Step in the autocoder wrapper — deterministic and LLM-free, so
# nothing here needs a model.
#
# Fresh run: `git worktree add ../automedon.worktrees/<slug> -b <branch>` (a
# sibling of the repo's toplevel), then the staged TASK.md
# ($AUTOMEDON_RUN_DIR/TASK.md, written by `distill`) is moved to
# `.workflows/<slug>/TASK.md` inside the new worktree.
#
# Recovery: distill re-emits the branch name for an issue it already staged;
# this Step then reuses the existing worktree/branch instead of recreating
# it. Every other collision — the branch existing without this issue's
# TASK.md, an unregistered worktree directory already at the expected path,
# or the branch checked out somewhere else — fails closed (non-zero exit, an
# explanatory message on stderr) rather than guessing which state to trust.
#
# Knobs: CODER_STUB=1 keeps the Step inert (CODER_STUB_CHECKOUT_CODE sets its
# exit code); the stub re-emits its in-Message unchanged, since there is no
# worktree to create.
set -u

: "${AUTOMEDON_RUN_DIR:?must be set by the orchestrator (the ephemeral Run Directory)}"

branch="$(cat)"

if [ "${CODER_STUB:-}" = "1" ]; then
    printf '%s' "$branch"
    exit "${CODER_STUB_CHECKOUT_CODE:-0}"
fi

# The slug is the branch name after its conventional-commit type prefix
# (distill starts it with the issue number, e.g. `fix/14-prune-race` ->
# `14-prune-race`), giving each issue a stable worktree/task directory name
# independent of the type the model chose.
slug="${branch#*/}"

repo_root="$(git rev-parse --show-toplevel)" || {
    echo "checkout: not inside a git repository" >&2
    exit 1
}
worktrees_root="$(cd "$repo_root/.." && pwd)/automedon.worktrees"
worktree="$worktrees_root/$slug"
task_md="$worktree/.workflows/$slug/TASK.md"

# Prune stale registrations (e.g. a worktree whose directory was removed by
# hand) before reasoning about what already exists, so a merged/deleted PR's
# leftovers never look like a collision. Routed to stderr so stdout carries
# only the out-Message.
git worktree prune 1>&2

# Where refs/heads/$branch is currently checked out, if anywhere: parse the
# porcelain worktree list rather than trust the expected path, since that is
# exactly the fact a collision could contradict.
elsewhere="$(git worktree list --porcelain | awk -v b="refs/heads/$branch" '
    /^worktree /{p=substr($0,10)}
    /^branch /{if ($2==b) print p}
')"

if git show-ref --verify --quiet "refs/heads/$branch"; then
    if [ "$elsewhere" = "$worktree" ] && [ -f "$task_md" ]; then
        # Recovery: same branch, same expected worktree, this issue's staged
        # TASK.md already landed there in a previous run.
        printf '%s' "$task_md"
        exit 0
    fi
    if [ -n "$elsewhere" ] && [ "$elsewhere" != "$worktree" ]; then
        printf 'checkout: branch %s is checked out elsewhere: %s\n' "$branch" "$elsewhere" >&2
        exit 1
    fi
    printf 'checkout: branch %s exists without a TASK.md for this issue at %s\n' "$branch" "$task_md" >&2
    exit 1
fi

if [ -e "$worktree" ]; then
    if ! git worktree list --porcelain | grep -qxF "worktree $worktree"; then
        printf 'checkout: worktree directory exists and is not registered: %s\n' "$worktree" >&2
        exit 1
    fi
    printf 'checkout: worktree directory exists at %s but branch %s does not\n' "$worktree" "$branch" >&2
    exit 1
fi

mkdir -p "$worktrees_root"
git worktree add "$worktree" -b "$branch" 1>&2 || exit 1

mkdir -p "$(dirname "$task_md")"
mv "$AUTOMEDON_RUN_DIR/TASK.md" "$task_md" || exit 1

printf '%s' "$task_md"
