#!/bin/zsh
# A release's entry in UFB's Sparkle feed (canon rows 536, 541; 1.4.x's scripts/make-appcast.sh for the new tree). The feed is
# the UFB repository's docs/appcast-mac.xml (https://ufbrowser.com/appcast-mac.xml): an installed 1.4.x reads it and trusts
# only archives signed with its EdDSA key, which lives in the login keychain - so this is run by the key's owner.
#   zsh packaging/macos/appcast-ufb.sh <feed.xml> [notes.html] [archive] [download URL]
# TWO FEEDS since 2.0.3 (row 546):
#   docs/appcast.xml      the feed UFB 2.0.3 and later read. Its entries are the DISK IMAGE (the default here): Sparkle
#                         replaces /Applications/UFB.app in place.
#   docs/appcast-mac.xml  the feed 1.4.x, 2.0.1 and 2.0.2 read. FROZEN: its last entry is 2.0.3's INSTALLER PACKAGE,
#                         which clears what 1.4.x laid down (row 539). Never add a later version to it - a 1.4.x
#                         would be led past the installer.
# Defaults: the notarized disk image sign-macos-ufb.sh made; the URL of the UFB repository's release v<version>.
# (2.0.3 itself went into both feeds as the package: pass it and its URL as the third and fourth arguments.)
set -euo pipefail
ROOT="$PWD"; APPCAST="${1:?the feed's file (docs/appcast.xml of the UFB repository, or a copy)}"; NOTES="${2:-}"
VERSION="$(sed -n 's/^version *= *"\(.*\)"/\1/p' "$ROOT/apps/ufb/Cargo.toml" | head -1)"
DMG="${3:-$ROOT/target/dist/release-ufb/UFB-$VERSION-macOS.dmg}"
URL="${4:-https://github.com/cbkow/ufb/releases/download/v$VERSION/UFB-$VERSION-macOS.dmg}"
SIGN="$ROOT/external/sparkle/bin/sign_update"; MARK="<!-- @@APPCAST_INSERT@@ -->"
[ -f "$DMG" ] || { echo "no archive at $DMG - run packaging/macos/sign-macos-ufb.sh release" >&2; exit 1; }
[ -x "$SIGN" ] || { echo "run tools/sparkle/fetch.sh first" >&2; exit 1; }
grep -q "$MARK" "$APPCAST" || { echo "the insertion mark is missing in $APPCAST" >&2; exit 1; }
if grep -q "<sparkle:version>$VERSION</sparkle:version>" "$APPCAST"; then echo "the feed already has $VERSION"; exit 0; fi
LINE="$("$SIGN" "$DMG")"
SIG="$(printf '%s' "$LINE" | sed -n 's/.*sparkle:edSignature="\([^"]*\)".*/\1/p')"; LEN="$(printf '%s' "$LINE" | sed -n 's/.*length="\([^"]*\)".*/\1/p')"
[ -n "$SIG" ] && [ -n "$LEN" ] || { echo "sign_update said: $LINE" >&2; exit 1; }
if [ -n "$NOTES" ]; then BODY="$(cat "$NOTES")"; else BODY="<p>Notes: https://github.com/cbkow/ufb/releases/tag/v$VERSION</p>"; fi
ITEM="$(mktemp)"; cat > "$ITEM" <<XML
    <item>
      <title>Version $VERSION</title>
      <sparkle:version>$VERSION</sparkle:version>
      <sparkle:shortVersionString>$VERSION</sparkle:shortVersionString>
      <sparkle:minimumSystemVersion>13.0</sparkle:minimumSystemVersion>
      <description><![CDATA[
$BODY
      ]]></description>
      <pubDate>$(date -R)</pubDate>
      <enclosure
        url="$URL"
        sparkle:edSignature="$SIG"
        length="$LEN"
        type="application/octet-stream" />
    </item>
XML
python3 - "$APPCAST" "$ITEM" "$MARK" <<'PY'
import sys
feed, item, mark = sys.argv[1:4]; s = open(feed, encoding="utf-8").read()
open(feed, "w", encoding="utf-8").write(s.replace(mark, mark + "\n" + open(item, encoding="utf-8").read().rstrip("\n"), 1))
PY
rm -f "$ITEM"; echo "added $VERSION to $APPCAST ($LEN bytes) - review, then commit it to publish"
