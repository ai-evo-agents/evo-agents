#!/usr/bin/env bash
# download-runner.sh — Download the correct evo-runner binary for the current platform.
#
# Usage:
#   ./download-runner.sh [version]
#
# The binary is placed in the same directory as this script as `evo-runner`
# (or `evo-runner.exe` on Windows).
#
# Requires: curl, tar (Linux/macOS) or unzip (Windows)

set -euo pipefail

REPO="ai-evo-agents/evo-agents"
BINARY="evo-runner"

# ── Determine version ─────────────────────────────────────────────────────────
VERSION="${1:-latest}"

if [[ "$VERSION" == "latest" ]]; then
  API_URL="https://api.github.com/repos/${REPO}/releases/latest"
  VERSION=$(curl -fsSL "$API_URL" | grep '"tag_name"' | head -1 | sed 's/.*"tag_name": "\(.*\)".*/\1/')
  if [[ -z "$VERSION" ]]; then
    echo "ERROR: Could not determine latest version from GitHub API." >&2
    exit 1
  fi
fi

echo "Downloading evo-runner ${VERSION} ..."

# ── Detect platform ───────────────────────────────────────────────────────────
OS=$(uname -s | tr '[:upper:]' '[:lower:]')
ARCH=$(uname -m)

case "$OS" in
  linux)
    case "$ARCH" in
      x86_64)  TARGET="x86_64-unknown-linux-gnu"  ;;
      aarch64) TARGET="aarch64-unknown-linux-gnu"  ;;
      arm64)   TARGET="aarch64-unknown-linux-gnu"  ;;
      *)
        echo "ERROR: Unsupported Linux architecture: $ARCH" >&2
        exit 1
        ;;
    esac
    ARCHIVE_EXT="tar.gz"
    ;;
  darwin)
    case "$ARCH" in
      x86_64) TARGET="x86_64-apple-darwin"  ;;
      arm64)  TARGET="aarch64-apple-darwin"  ;;
      *)
        echo "ERROR: Unsupported macOS architecture: $ARCH" >&2
        exit 1
        ;;
    esac
    ARCHIVE_EXT="tar.gz"
    ;;
  mingw*|cygwin*|msys*)
    TARGET="x86_64-pc-windows-msvc"
    ARCHIVE_EXT="zip"
    BINARY="evo-runner.exe"
    ;;
  *)
    echo "ERROR: Unsupported OS: $OS" >&2
    exit 1
    ;;
esac

# ── Download & extract ────────────────────────────────────────────────────────
ARCHIVE_NAME="evo-runner-${VERSION}-${TARGET}.${ARCHIVE_EXT}"
DOWNLOAD_URL="https://github.com/${REPO}/releases/download/${VERSION}/${ARCHIVE_NAME}"
DEST_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

TMP_DIR=$(mktemp -d)
trap 'rm -rf "$TMP_DIR"' EXIT

echo "Downloading: $DOWNLOAD_URL"
curl -fsSL --progress-bar -o "$TMP_DIR/$ARCHIVE_NAME" "$DOWNLOAD_URL"

echo "Extracting ..."
if [[ "$ARCHIVE_EXT" == "tar.gz" ]]; then
  tar -xzf "$TMP_DIR/$ARCHIVE_NAME" -C "$TMP_DIR"
else
  unzip -q "$TMP_DIR/$ARCHIVE_NAME" -d "$TMP_DIR"
fi

# Move the binary into the destination directory
mv "$TMP_DIR/$BINARY" "$DEST_DIR/$BINARY"
chmod +x "$DEST_DIR/$BINARY"

echo "✓ $BINARY ${VERSION} installed to ${DEST_DIR}/${BINARY}"
