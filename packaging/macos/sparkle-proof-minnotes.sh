#!/bin/zsh
# The update proof of row 379 (4): a COPY of an installed minNotes 2 is pointed at a local copy of the feed that
# carries the new version, and updates itself. Nothing is published and the real feed is not touched.
#   zsh packaging/macos/sparkle-proof-minnotes.sh start    the feed copy (with a signed entry for the DMG in
#       target/dist/release-minnotes), a web server on 127.0.0.1:8123, the feed override, the 2.x copy opened -
#       then, in that copy: minNotes > Check for Updates..., Install and Relaunch; it must come back as the new version
#   zsh packaging/macos/sparkle-proof-minnotes.sh stop     the override removed (ALWAYS run this), the server stopped
# The override is a preference of the identity `app.minnotes`: while it is set, the INSTALLED minNotes reads the local
# feed too. A file:// feed is refused by Sparkle, hence the server.
set -euo pipefail
ROOT="$PWD"; P="$ROOT/target/sparkle-proof"; PORT=8123
VERSION="$(sed -n 's/^version *= *"\(.*\)"/\1/p' "$ROOT/apps/mn/Cargo.toml" | head -1)"; DMG="minNotes-$VERSION-macOS.dmg"
case "${1:-}" in
  start)
    [ -f "$ROOT/target/dist/release-minnotes/$DMG" ] || { echo "no $DMG - run packaging/macos/sign-macos-minnotes.sh" >&2; exit 1; }
    SRC="${2:-/Applications/minNotes.app}"; OLD="$(/usr/libexec/PlistBuddy -c 'Print :CFBundleShortVersionString' "$SRC/Contents/Info.plist")"
    case "$OLD" in 2.*) ;; *) echo "$SRC is minNotes $OLD, not a 2.x - pass the path of a 2.x minNotes.app" >&2; exit 1 ;; esac
    rm -rf "$P"; mkdir -p "$P/feed" "$P/minNotes $OLD copy"
    cp "${FEED:-$ROOT/../minNotes-3.0/docs/appcast.xml}" "$P/feed/appcast.xml"; cp "$ROOT/target/dist/release-minnotes/$DMG" "$P/feed/"
    zsh "$ROOT/packaging/macos/appcast-minnotes.sh" "$P/feed/appcast.xml" "$ROOT/packaging/macos/minnotes-$VERSION-notes.html" "$P/feed/$DMG" "http://127.0.0.1:$PORT/$DMG"
    ditto "$SRC" "$P/minNotes $OLD copy/minNotes.app"
    (cd "$P/feed" && nohup python3 -m http.server $PORT --bind 127.0.0.1 > "$P/server.log" 2>&1 & echo $! > "$P/server.pid")
    defaults write app.minnotes SUFeedURL "http://127.0.0.1:$PORT/appcast.xml"
    open -n "$P/minNotes $OLD copy/minNotes.app"
    echo "minNotes $OLD (a copy) is open: choose minNotes > Check for Updates..., then Install and Relaunch."
    echo "When done:  zsh packaging/macos/sparkle-proof-minnotes.sh stop" ;;
  stop)
    defaults delete app.minnotes SUFeedURL 2> /dev/null || true
    [ -f "$P/server.pid" ] && kill "$(cat "$P/server.pid")" 2> /dev/null || true
    NOW="$(/usr/libexec/PlistBuddy -c 'Print :CFBundleShortVersionString' "$P"/minNotes*copy/minNotes.app/Contents/Info.plist 2> /dev/null || echo '?')"
    echo "the override is removed; the copy is now minNotes $NOW" ;;
  *) echo "usage: zsh packaging/macos/sparkle-proof-minnotes.sh start [path to a 2.x minNotes.app] | stop" >&2; exit 1 ;;
esac
