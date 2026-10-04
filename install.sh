#!/bin/sh
# shed installer — Linux and macOS
# Usage:
#   curl -fsSL https://raw.githubusercontent.com/nostalume/shed/main/install.sh | sh
#   curl -fsSL https://raw.githubusercontent.com/nostalume/shed/main/install.sh | sh -s v0.2.0
set -e

REPO="nostalume/shed"
VERSION="${1:-latest}"

OS=$(uname -s)
ARCH=$(uname -m)

case "${OS}-${ARCH}" in
  Linux-x86_64)           FILE="shed-linux-x86_64" ;;
  Linux-aarch64)          FILE="shed-linux-aarch64" ;;
  Linux-arm64)            FILE="shed-linux-aarch64" ;;
  Darwin-x86_64)          FILE="shed-macos-x86_64" ;;
  Darwin-arm64)           FILE="shed-macos-aarch64" ;;
  *)
    echo "shed: unsupported platform ${OS}-${ARCH}" >&2
    exit 1
    ;;
esac

if [ "$VERSION" = "latest" ]; then
  URL="https://github.com/${REPO}/releases/latest/download/${FILE}"
else
  URL="https://github.com/${REPO}/releases/download/${VERSION}/${FILE}"
fi

# Pick install directory: prefer ~/.local/bin, fall back to ~/bin
if [ -d "$HOME/.local/bin" ] || mkdir -p "$HOME/.local/bin" 2>/dev/null; then
  DEST="$HOME/.local/bin"
else
  DEST="$HOME/bin"
  mkdir -p "$DEST"
fi

TMP=$(mktemp)
SUM=$(mktemp)
trap 'rm -f "$TMP" "$SUM"' EXIT

echo "Downloading shed from $URL ..."
curl -fsSL "$URL" -o "$TMP"
curl -fsSL "${URL}.sha256" -o "$SUM"

EXPECTED=$(awk '{print $1}' "$SUM")
if command -v sha256sum >/dev/null 2>&1; then
  ACTUAL=$(sha256sum "$TMP" | awk '{print $1}')
elif command -v shasum >/dev/null 2>&1; then
  ACTUAL=$(shasum -a 256 "$TMP" | awk '{print $1}')
else
  echo "shed: no SHA-256 utility found; cannot verify download" >&2
  exit 1
fi
if [ "$ACTUAL" != "$EXPECTED" ]; then
  echo "shed: checksum verification failed" >&2
  exit 1
fi
chmod +x "$TMP"
mv "$TMP" "$DEST/shed"

echo "shed installed to $DEST/shed"

# Warn if the directory is not on PATH
case ":${PATH}:" in
  *":$DEST:"*) ;;
  *)
    echo ""
    echo "NOTE: $DEST is not on your PATH."
    echo "Add the following line to your shell rc file:"
    echo "  export PATH=\"$DEST:\$PATH\""
    ;;
esac
