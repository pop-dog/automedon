#!/bin/sh
# Remote installer for the `automedon` Claude skill.
#
#   curl -fsSL https://raw.githubusercontent.com/pop-dog/automedon/main/install-skill.sh | bash
#
# Installs the bundled `automedon` skill into your Claude skills directory without
# cloning the repo. No Rust toolchain is needed. The binary installer is
# install.sh; the in-repo `autocoder` skill is repo-only (it reads repo files via
# $AUTOMEDON_WORKFLOW_DIR) and has no remote installer.
#
# A whole-repo source archive is downloaded rather than individual files so new
# skill files are picked up without editing this installer.
#
# Idempotent: safe to re-run; a fresh download replaces the installed skill.
set -eu

REPO="pop-dog/automedon"
# Override point for mirrors and offline testing. The default points at the
# project's GitHub source archives.
: "${AUTOMEDON_SKILL_BASE_URL:=https://github.com/$REPO/archive}"

die() {
    echo "install-skill.sh: $*" >&2
    exit 1
}

# --- Arguments and overrides -------------------------------------------------

ref="${AUTOMEDON_SKILL_REF:-main}"
skills_dir="${AUTOMEDON_SKILLS_DIR:-$HOME/.claude/skills}"

while [ $# -gt 0 ]; do
    case "$1" in
        --ref)
            shift
            [ $# -gt 0 ] || die "--ref needs an argument"
            ref="$1"
            ;;
        --ref=*) ref="${1#*=}" ;;
        --skills-dir)
            shift
            [ $# -gt 0 ] || die "--skills-dir needs an argument"
            skills_dir="$1"
            ;;
        --skills-dir=*) skills_dir="${1#*=}" ;;
        *) die "unknown argument: $1" ;;
    esac
    shift
done

# --- Downloader (curl, falling back to wget) ---------------------------------

if command -v curl >/dev/null 2>&1; then
    download() { curl -fsSL "$1" -o "$2"; }
elif command -v wget >/dev/null 2>&1; then
    download() { wget -qO "$2" "$1"; }
else
    die "need curl or wget to download the skill"
fi

# --- Download and extract ----------------------------------------------------

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT INT TERM

archive="$tmp/source.tar.gz"
download "$AUTOMEDON_SKILL_BASE_URL/$ref.tar.gz" "$archive" ||
    die "failed to download source archive for ref $ref"

tar -xzf "$archive" -C "$tmp" || die "failed to extract source archive for ref $ref"

# GitHub archives unpack into a single `<repo>-<ref>/` top-level directory, so the
# skill lives at `*/skills/automedon`. Locate it without assuming that prefix so a
# plain `skills/automedon` layout (e.g. an offline mirror) also works.
src=""
for candidate in "$tmp"/*/skills/automedon "$tmp"/skills/automedon; do
    if [ -d "$candidate" ]; then
        src="$candidate"
        break
    fi
done
[ -n "$src" ] || die "archive for ref $ref has no skills/automedon directory"

# --- Install -----------------------------------------------------------------

mkdir -p "$skills_dir"
dest="$skills_dir/automedon"
# Replace any prior install cleanly so files removed upstream do not linger.
rm -rf "$dest"
cp -R "$src" "$dest"

# --- Skills-dir guidance (never edits Claude config) -------------------------

case "$skills_dir" in
    "$HOME/.claude/skills") ;;
    *)
        echo ""
        echo "Note: installed outside the default skills dir."
        echo "Ensure Claude loads skills from: $skills_dir"
        ;;
esac

echo "Installed automedon skill (ref $ref) to $dest"
