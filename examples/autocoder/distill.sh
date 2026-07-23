#!/bin/sh
# `distill` Step: read a GitHub issue reference (number or URL) from stdin,
# distill it into a concrete TASK.md staged in the ephemeral Run Directory,
# and print the full branch name the agent chose as the out-Message.
# `checkout` (the next Step) creates the worktree and moves the staged
# TASK.md into it.
#
# Recovery: if a worktree for this issue already exists (a
# `../automedon.worktrees` sibling directory whose slug starts with the
# issue number and already holds a TASK.md), the existing branch name is
# re-emitted instead of asking the agent for a new one — but the issue is
# still fetched and re-distilled, because refining the GitHub issue and
# re-running is the documented escalation loop: the refreshed spec must
# reach the agents, not the stale one.
#
# Knobs: CODER_STUB=1 keeps the Step inert (CODER_STUB_DISTILL_CODE sets its
# exit code; the stub emits the fixed branch name `stub/0-task`, not its
# in-Message, since there is no real issue to distill); model picks the
# `claude` model — opus by default, since turning a raw issue into a concrete
# spec is a judgement call worth the strongest model.
set -u

: "${AUTOMEDON_RUN_DIR:?must be set by the orchestrator (the ephemeral Run Directory)}"

. "$AUTOMEDON_WORKFLOW_DIR/lib/llm.sh"

issue_ref="$(cat)"

if [ "${CODER_STUB:-}" = "1" ]; then
    printf 'stub/0-task'
    exit "${CODER_STUB_DISTILL_CODE:-0}"
fi

# Ask gh for the canonical issue number rather than parsing issue_ref
# ourselves — it accepts a bare number or a URL verbatim — then look for a
# worktree already staged for it.
number="$(gh issue view "$issue_ref" --json number -q .number)" || exit 1

repo_root="$(git rev-parse --show-toplevel)" || exit 1
worktrees_root="$(cd "$repo_root/.." 2>/dev/null && pwd)/automedon.worktrees"
existing="$(find "$worktrees_root" -mindepth 1 -maxdepth 1 -type d -name "${number}-*" 2>/dev/null | head -n 1)"
existing_branch=""
if [ -n "$existing" ]; then
    slug="$(basename "$existing")"
    task_md="$existing/.workflows/$slug/TASK.md"
    branch="$(git -C "$repo_root" worktree list --porcelain | awk -v d="$existing" '
        /^worktree /{p=substr($0,10)}
        /^branch /{if (p==d) print substr($2,12)}
    ')"
    if [ -f "$task_md" ] && [ -n "$branch" ]; then
        existing_branch="$branch"
    fi
fi

prompt="$(llm_render "${0%/*}/prompts/distill.md" \
    ISSUE_REF="$issue_ref" RUN_DIR="$AUTOMEDON_RUN_DIR")" || exit 1

# Run the distilling agent unattended. It only reads the issue and writes
# scratch under $AUTOMEDON_RUN_DIR, so --dangerously-skip-permissions (as
# code.sh uses) is simpler here than a policy file scoped to a path that
# does not exist until run time.
reply="$(claude --dangerously-skip-permissions --model "${model:-opus}" -p "$prompt" 2>/dev/null)"

# On recovery the branch already exists, so the agent's BRANCH suggestion is
# ignored in favor of the one the worktree is on — the spec is refreshed, the
# branch identity is not.
if [ -n "$existing_branch" ]; then
    printf '%s' "$existing_branch"
    exit 0
fi

branch="$(printf '%s\n' "$reply" | grep -E '^BRANCH:' | tail -n 1 | sed -E 's/^BRANCH:[[:space:]]*//; s/[[:space:]]*$//')"
if [ -z "$branch" ]; then
    printf 'distill: agent reply did not include a BRANCH: line\n' >&2
    exit 1
fi

printf '%s' "$branch"
