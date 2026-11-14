#!/bin/zsh
# A WALK bundle of minNotes: the packaged application (make-app-minnotes.sh) made into a development copy that can be
# started beside an installed minNotes - its own identity (app.minplatform.dev.minnotes, not 2.x's), no updater, no
# document types - and put OUTSIDE the Documents folder. The earlier walk bundles linked the tree's libraries by their
# paths under ~/Documents, so every start waited on macOS's question about that folder and died when it went unanswered
# ("Library not loaded ... file system sandbox blocked open()", 2026-11-14). This one carries its libraries.
#   zsh packaging/macos/make-walk-minnotes.sh b108      ->  "/Users/Shared/minNotes Walk/b108/minNotes.app"
# Start it:  open -n "<that>" --args <collection.mnd>
set -euo pipefail
N="${1:?a name for the bundle, such as b108}"; W="${MN_WALK_DIR:-/Users/Shared/minNotes Walk}/$N"; A="$W/minNotes.app"
zsh packaging/macos/make-app-minnotes.sh
rm -rf "$W"; mkdir -p "$W"; ditto target/dist/minNotes.app "$A"
P="$A/Contents/Info.plist"; /usr/libexec/PlistBuddy -c "Set :CFBundleIdentifier app.minplatform.dev.minnotes" "$P"
for k in SUFeedURL SUPublicEDKey SUEnableAutomaticChecks SUScheduledCheckInterval CFBundleDocumentTypes CFBundleURLTypes UTExportedTypeDeclarations; do /usr/libexec/PlistBuddy -c "Delete :$k" "$P" 2> /dev/null || true; done
rm -rf "$A/Contents/Frameworks/Sparkle.framework"
codesign --force -s - "$A/Contents/Frameworks"/*.dylib "$A/Contents/Helpers"/* 2> /dev/null; codesign --force -s - "$A"
echo "walk bundle: $A"
