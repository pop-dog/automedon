#!/bin/sh
# Remote installer for the `automedon` binary.
#
#   curl -fsSL https://raw.githubusercontent.com/pop-dog/agent-orchestrator/main/install.sh | bash
#
# Downloads a prebuilt release binary for the host platform, verifies it against
# the release checksums, and installs it onto your PATH. No Rust toolchain and no
# repo clone are needed. Contributors building from source want
# scripts/dev-install.sh instead.
#
# Idempotent: safe to re-run; a fresh download replaces the installed binary.
set -eu

REPO="pop-dog/agent-orchestrator"
# Override points for release mirrors and offline testing. The defaults point at
# the project's GitHub releases.
: "${AUTOMEDON_BASE_URL:=https://github.com/$REPO/releases/download}"
: "${AUTOMEDON_API_URL:=https://api.github.com/repos/$REPO/releases/latest}"

die() {
    echo "install.sh: $*" >&2
    exit 1
}

# --- Arguments and overrides -------------------------------------------------

version="${VERSION:-}"
bin_dir="${AUTOMEDON_BIN_DIR:-$HOME/.local/bin}"

while [ $# -gt 0 ]; do
    case "$1" in
        --version)
            shift
            [ $# -gt 0 ] || die "--version needs an argument"
            version="$1"
            ;;
        --version=*) version="${1#*=}" ;;
        --bin-dir)
            shift
            [ $# -gt 0 ] || die "--bin-dir needs an argument"
            bin_dir="$1"
            ;;
        --bin-dir=*) bin_dir="${1#*=}" ;;
        *) die "unknown argument: $1" ;;
    esac
    shift
done

# --- Platform detection ------------------------------------------------------

os=$(uname -s)
case "$os" in
    Darwin) os="darwin" ;;
    Linux) os="linux" ;;
    *) die "unsupported OS: $os (need Darwin or Linux)" ;;
esac

arch=$(uname -m)
case "$arch" in
    x86_64 | amd64) arch="x86_64" ;;
    aarch64 | arm64) arch="aarch64" ;;
    *) die "unsupported architecture: $arch (need x86_64 or aarch64)" ;;
esac

# --- Downloader (curl, falling back to wget) ---------------------------------

if command -v curl >/dev/null 2>&1; then
    fetch() { curl -fsSL "$1"; }
    download() { curl -fsSL "$1" -o "$2"; }
elif command -v wget >/dev/null 2>&1; then
    fetch() { wget -qO - "$1"; }
    download() { wget -qO "$2" "$1"; }
else
    die "need curl or wget to download the release"
fi

# --- Version resolution ------------------------------------------------------

if [ -z "$version" ]; then
    # Resolve the latest release tag from the GitHub API by reading tag_name.
    tag=$(fetch "$AUTOMEDON_API_URL" |
        grep '"tag_name"' |
        head -n1 |
        sed -E 's/.*"tag_name"[[:space:]]*:[[:space:]]*"([^"]+)".*/\1/')
    [ -n "$tag" ] || die "could not resolve the latest release tag"
else
    tag="$version"
fi
# Tags are `vX.Y.Z`; the archive name uses the bare version.
case "$tag" in
    v*) ;;
    *) tag="v$tag" ;;
esac
version="${tag#v}"

# --- Download ----------------------------------------------------------------

archive="automedon-$version-$os-$arch.tar.gz"
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT INT TERM

download "$AUTOMEDON_BASE_URL/$tag/$archive" "$tmp/$archive" ||
    die "failed to download $archive"
download "$AUTOMEDON_BASE_URL/$tag/checksums.sha256" "$tmp/checksums.sha256" ||
    die "failed to download checksums.sha256"

# --- Verify ------------------------------------------------------------------

if command -v sha256sum >/dev/null 2>&1; then
    sha_check() { sha256sum -c -; }
elif command -v shasum >/dev/null 2>&1; then
    sha_check() { shasum -a 256 -c -; }
else
    die "need sha256sum or shasum to verify the download"
fi

# Check only our archive's line; the checksum filename is relative, so run from
# the directory holding the archive. `grep -F` matches the literal
# "<hash>  <archive>" line so the `.` in the name is not a regex wildcard.
if ! (cd "$tmp" && grep -F "  $archive" checksums.sha256 | sha_check) >/dev/null 2>&1; then
    die "checksum verification failed for $archive"
fi

# --- Install -----------------------------------------------------------------

tar -xzf "$tmp/$archive" -C "$tmp" || die "failed to extract $archive"
[ -f "$tmp/automedon" ] || die "$archive did not contain an automedon binary"

mkdir -p "$bin_dir"
install_path="$bin_dir/automedon"
cp "$tmp/automedon" "$install_path"
chmod +x "$install_path"

# --- PATH guidance (never edits shell rc files) ------------------------------

case ":$PATH:" in
    *":$bin_dir:"*) ;;
    *)
        echo ""
        echo "Note: $bin_dir is not on your PATH. Add it to your shell profile:"
        echo "    export PATH=\"$bin_dir:\$PATH\""
        ;;
esac

echo "Installed automedon $version to $install_path"
