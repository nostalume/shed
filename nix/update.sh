#!/usr/bin/env bash
# nix/update.sh — update version and SRI hashes in nix/shed.nix
#
# Usage:
#   ./nix/update.sh            # auto-detect latest release from GitHub
#   ./nix/update.sh 0.2.0      # specific version (leading v optional)
#
# Requirements: nix, curl, sed

set -euo pipefail

REPO="nostalume/shed"
NIX_FILE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/shed.nix"

# ── Resolve version ────────────────────────────────────────────────────────────
if [[ $# -ge 1 ]]; then
  VERSION="${1#v}"
else
  echo "Fetching latest release tag from GitHub..."
  VERSION=$(curl -fsSL "https://api.github.com/repos/${REPO}/releases/latest" \
    | grep '"tag_name"' \
    | sed 's/.*"v\([^"]*\)".*/\1/')
fi

echo "Updating nix/shed.nix to v${VERSION}"

# ── Update version line ────────────────────────────────────────────────────────
sed -i.bak "s/version = \"[^\"]*\"/version = \"${VERSION}\"/" "${NIX_FILE}"
rm -f "${NIX_FILE}.bak"
echo "  version -> ${VERSION}"

# ── Fetch hash and rewrite in one sed pass per platform ───────────────────────
# Each hash line carries an inline comment with the platform name, e.g.:
#   hash = "sha256-..."; # x86_64-linux
# This makes it uniquely addressable with a single sed address pattern.

fetch_sri() {
  local url="$1"
  local base32
  base32=$(nix-prefetch-url --type sha256 "${url}" 2>/dev/null)
  nix hash convert --hash-algo sha256 --to sri "${base32}" 2>/dev/null \
    || nix hash to-sri --type sha256 "${base32}" 2>/dev/null
}

declare -A ARTIFACTS=(
  [x86_64-linux]="shed-linux-x86_64"
  [aarch64-linux]="shed-linux-aarch64"
  [x86_64-darwin]="shed-macos-x86_64"
  [aarch64-darwin]="shed-macos-aarch64"
)

for PLATFORM in "${!ARTIFACTS[@]}"; do
  URL="https://github.com/${REPO}/releases/download/v${VERSION}/${ARTIFACTS[$PLATFORM]}"
  printf "  %-20s ... " "${PLATFORM}"
  SRI=$(fetch_sri "${URL}")
  echo "${SRI}"
  # Target only the line that ends with the platform comment tag
  sed -i.bak "/${PLATFORM}/s|hash = \"[^\"]*\"|hash = \"${SRI}\"|" "${NIX_FILE}"
  rm -f "${NIX_FILE}.bak"
done

echo ""
echo "Done. nix/shed.nix is now at v${VERSION}."
echo "Verify with: nix build .#shed"
