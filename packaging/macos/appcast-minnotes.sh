#!/bin/zsh
# A release's entry in minNotes' Sparkle feed (canon row 379; 2.x's scripts/update_appcast.sh for the new tree). The feed is
# the minNotes repository's docs/appcast.xml (https://minnotes.app/appcast.xml): an installed 2.x reads it and trusts
# only archives signed with its EdDSA key, which lives in the login keychain - so this is run by the key's owner.
#   zsh packaging/macos/appcast-minnotes.sh <appcast.xml> [notes.html] [dmg] [download URL]
# Defaults: the notarized DMG sign-macos-minnotes.sh made; the URL of the minNotes repository's release v<version>.
# For the update proof against a local feed, pass a COPY of the feed and the local URL of the DMG.
set -euo pipefail
ROOT="$PWD"; APPCAST="${1:?the feed's file (docs/appcast.xml of the minNotes repository, or a copy)}"; NOTES="${2:-}"
VERSION="$(sed -n 's/^version *= *"\(.*\)"/\1/p' "$ROOT/apps/mn/Cargo.toml" | head -1)"
DMG="${3:-$ROOT/target/dist/release-minnotes/minNotes-$VERSION-macOS.dmg}"
URL="${4:-https://github.com/cbkow/minNotes/releases/download/v$VERSION/minNotes-$VERSION-macOS.dmg}"
SIGN="$ROOT/external/sparkle/bin/sign_update"; MARK="<!-- @@APPCAST_INSERT@@ -->"
[ -f "$DMG" ] || { echo "no DMG at $DMG - run packaging/macos/sign-macos-minnotes.sh" >&2; exit 1; }
[ -x "$SIGN" ] || { echo "run tools/sparkle/fetch.sh first" >&2; exit 1; }
grep -q "$MARK" "$APPCAST" || { echo "the insertion mark is missing in $APPCAST" >&2; exit 1; }
if grep -q "<sparkle:version>$VERSION</sparkle:version>" "$APPCAST"; then echo "the feed already has $VERSION"; exit 0; fi
LINE="$("$SIGN" "$DMG")"
SIG="$(printf '%s' "$LINE" | sed -n 's/.*sparkle:edSignature="\([^"]*\)".*/\1/p')"; LEN="$(printf '%s' "$LINE" | sed -n 's/.*length="\([^"]*\)".*/\1/p')"
[ -n "$SIG" ] && [ -n "$LEN" ] || { echo "sign_update said: $LINE" >&2; exit 1; }
if [ -n "$NOTES" ]; then BODY="$(cat "$NOTES")"; else BODY="<p>Notes: https://github.com/cbkow/minNotes/releases/tag/v$VERSION</p>"; fi
ITEM="$(mktemp)"; cat > "$ITEM" <<XML
    <item>
      <title>Version $VERSION</title>
      <link>https://minnotes.app/</link>
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
