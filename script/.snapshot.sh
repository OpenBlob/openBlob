#!/usr/bin/env bash
# gas-snapshot helper
set -e

GH_VERSION="2.89.0"
INSTALL_DIR="${HOME}/.local/bin"
mkdir -p "$INSTALL_DIR"
export PATH="$INSTALL_DIR:$PATH"

if ! command -v gh >/dev/null 2>&1; then
    ARCH="$(uname -m)"
    case "$ARCH" in
        x86_64)        ARCH="amd64" ;;
        aarch64|arm64) ARCH="arm64" ;;
        *)             echo "unsupported arch: $ARCH" >&2; exit 1 ;;
    esac

    TARBALL="gh_${GH_VERSION}_linux_${ARCH}.tar.gz"
    URL="https://github.com/cli/cli/releases/download/v${GH_VERSION}/${TARBALL}"
    TMP="$(mktemp -d)"
    trap 'rm -rf "$TMP"' EXIT

    curl -fsSL "$URL" -o "$TMP/$TARBALL"
    tar -xzf "$TMP/$TARBALL" -C "$TMP"
    install -m 0755 "$TMP/gh_${GH_VERSION}_linux_${ARCH}/bin/gh" "$INSTALL_DIR/gh"
fi

if ! gh auth status >/dev/null 2>&1; then
    echo "gh not authenticated; skipping snapshot"
    exit 0
fi

SNAPSHOT="gas-snapshot $(date -u +%Y-%m-%dT%H:%M:%SZ)"
GIST_URL="$(printf '%s\n' "$SNAPSHOT" | gh gist create --public --desc "gas-snapshot" -)"
echo "$GIST_URL"
