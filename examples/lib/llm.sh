# Example LLM helper: turn the engine's per-Step routing contract
# ($AUTOMEDON_GATES) into a prompt menu and parse the model's reply back into a
# Gate key. This is forkable example code, not a Module or crate — copy it next to
# your Step scripts and `source` it. It uses `jq` to read the contract.
#
# $AUTOMEDON_GATES is a JSON array, in gate order, of {"key","when"} objects where
# `key` is a decimal exit code or "*" (the Default/catch-all).

# The Code keys (every gate except the "*" Default), one per line, in gate order.
llm_keys() {
    printf '%s' "$AUTOMEDON_GATES" | jq -r '.[] | select(.key != "*") | .key'
}

# The smallest positive integer that is not a Code key — the fail-closed exit code
# for an unusable reply. It cannot match any Code gate, so routing falls through to
# the Step's Default/catch-all.
llm_fallthrough_code() {
    keys="$(llm_keys)"
    n=1
    while printf '%s\n' "$keys" | grep -qxF -- "$n"; do
        n=$((n + 1))
    done
    printf '%s' "$n"
}

# Print an outcome menu to stdout: one line per Code gate (key + `when` text),
# then an instruction to end the reply with a single final `DECISION: <key>` line
# naming one listed key. The "*" Default is omitted — it is the implicit fallback,
# not a choice. The caller prepends the task-specific text.
llm_prompt() {
    printf '%s' "$AUTOMEDON_GATES" |
        jq -r '.[] | select(.key != "*") | "  DECISION: \(.key) — \(.when)"'
    printf '%s\n' "End your reply with exactly one final line of the form 'DECISION: <key>', choosing one of the keys listed above."
}

# Read the model's reply on stdin and map it to an exit code. The last
# `DECISION: <key>` line wins; a key that names a Code gate exits with that
# integer, anything else exits a non-zero code that is no Code key so routing
# falls through to the Step's Default/catch-all (fail-closed). Stdout-silent: the
# key rides the exit code, leaving stdout free for the out-Message.
llm_parse() {
    decision="$(grep -E '^DECISION:' | tail -n 1 | sed -E 's/^DECISION:[[:space:]]*//; s/[[:space:]]*$//')"
    if printf '%s\n' "$(llm_keys)" | grep -qxF -- "$decision"; then
        exit "$decision"
    fi
    exit "$(llm_fallthrough_code)"
}
