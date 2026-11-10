#!/bin/zsh
# Sparkle for the macOS bundle (canon row 248): the pinned release tarball, checked, unpacked into the gitignored
# external/sparkle (Sparkle.framework for the bundle; bin/sign_update and generate_keys for the appcast). QCView-Player's
# fetch_sparkle.sh — same version, same hash. Run from the repo root:  zsh tools/sparkle/fetch.sh [--force]
set -euo pipefail
VERSION="2.9.2"; SHA256="1cb340cbbef04c6c0d162078610c25e2221031d794a3449d89f2f56f4df77c95"
DEST="$PWD/external/sparkle"
if [ -d "$DEST/Sparkle.framework" ] && [ "${1:-}" != "--force" ]; then echo "Sparkle already in $DEST"; exit 0; fi
TMP="$(mktemp -d)"; trap 'rm -rf "$TMP"' EXIT
curl -fsSL -o "$TMP/sparkle.tar.xz" "https://github.com/sparkle-project/Sparkle/releases/download/$VERSION/Sparkle-$VERSION.tar.xz"
GOT="$(shasum -a 256 "$TMP/sparkle.tar.xz" | awk '{print $1}')"
[ "$GOT" = "$SHA256" ] || { echo "Sparkle $VERSION: SHA-256 mismatch (got $GOT) — do not use this download" >&2; exit 1; }
rm -rf "$DEST"; mkdir -p "$DEST"; tar -xJf "$TMP/sparkle.tar.xz" -C "$DEST"
echo "Sparkle $VERSION → $DEST"
