#!/bin/sh
# `build-test` Step: the deterministic, LLM-free checkpoint between `code` and
# `review`. Read the TASK.md path from stdin and re-emit it on stdout (the
# Message is just relayed). Build the project and run the whole test suite; exit
# 0 on green (route to review), non-zero on red (route back to code). On red the
# combined build/test output is written to $AUTOMEDON_RUN_DIR/BUILD_FAILURE.md so the next
# `code` pass can act on it; on green that file is removed.
set -u

# Orchestration scratch lives in the ephemeral Run Directory the engine provides,
# never in the Repository this Step operates on. Fail loud if it is missing: a cwd
# fallback would pollute the deliverable and a per-script mktemp would break the
# cross-Step handoff (review writes the findings that the next code pass reads).
: "${AUTOMEDON_RUN_DIR:?must be set by the orchestrator (the ephemeral Run Directory)}"

# The forkable example repo-location helper: task_repo_cd puts this Step in
# the repository that actually holds the task file, which may be a sibling
# git worktree the orchestrator was not started in (see the autocoder
# wrapper's checkout.sh).
. "$AUTOMEDON_WORKFLOW_DIR/lib/repo.sh"

task_path="$(cat)"

# Stub mode scripts the gate so the Workflow's routing can be tested without
# invoking cargo (or requiring the in-Message to be a real path): `pass`
# (default) is green, `fail` is always red, and `fail-once` is red on its
# first activation and green thereafter (so a retry can be exercised),
# recording that it has failed in a marker file.
if [ "${CODER_STUB:-}" = "1" ]; then
    printf '%s' "$task_path"
    case "${CODER_STUB_BUILD:-pass}" in
        fail) exit 1 ;;
        fail-once)
            marker="${CODER_STUB_BUILD_MARKER:-$AUTOMEDON_RUN_DIR/.build-stub-marker}"
            [ -f "$marker" ] && exit 0
            : > "$marker"
            exit 1
            ;;
        *) exit 0 ;;
    esac
fi

task_repo_cd "$task_path" || exit 1

# Capture build+test output so a failure can be handed back to the coding agent,
# while still streaming it to stderr for the live view.
log="$(mktemp)"
if { cargo build && cargo test; } > "$log" 2>&1; then
    cat "$log" 1>&2
    rm -f "$log" "$AUTOMEDON_RUN_DIR/BUILD_FAILURE.md"
    printf '%s' "$task_path"
    exit 0
fi

cat "$log" 1>&2
{
    printf '# Build/test failure\n\n'
    printf 'The project did not build or its tests did not pass. Output:\n\n'
    printf '```\n'
    cat "$log"
    printf '```\n'
} > "$AUTOMEDON_RUN_DIR/BUILD_FAILURE.md"
rm -f "$log"
printf '%s' "$task_path"
exit 1
