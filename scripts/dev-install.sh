#!/bin/sh
# Source install for contributors working on the engine.
#
#   ./scripts/dev-install.sh
#
# Use this when you have the repo cloned and a Rust toolchain. End users without
# either want the remote installer (install.sh) instead.
#
# - Builds and installs the `automedon` binary to ~/.cargo/bin (on PATH via
#   rustup). The binary is a build snapshot: re-run this script after changing
#   the engine to pick the changes up.
# - Symlinks the bundled skills into ~/.claude/skills so Claude can use them.
#   The links are live — edits to a skill in this repo take effect immediately,
#   no re-install needed.
#
# Idempotent: safe to re-run. Runnable from any directory (paths resolve to the
# script's own location).
set -eu

script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
repo_root=$(CDPATH= cd -- "$script_dir/.." && pwd)
skills_dir="$HOME/.claude/skills"

# The automedon binary (a build snapshot — re-run to pick up engine changes).
cargo install --path "$repo_root/crates/orchestrator"

# The Claude skills, symlinked live. `rm -rf` clears a prior symlink or copied
# directory of the same name without following the link into this repo, so the
# fresh symlink replaces it cleanly instead of nesting inside it.
mkdir -p "$skills_dir"
for skill in agent-orchestrator autocoder; do
    rm -rf "$skills_dir/$skill"
    ln -s "$repo_root/skills/$skill" "$skills_dir/$skill"
    echo "linked $skills_dir/$skill -> $repo_root/skills/$skill"
done

echo "Installed: automedon (binary) and skills (agent-orchestrator, autocoder)."
