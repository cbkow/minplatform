#!/bin/zsh
# The macOS release artifact of minNotes (canon row 379; QCView's sign-macos.sh, row 248, with minNotes' names):
# target/dist/minNotes.app (make-app-minnotes.sh) -> a COPY signed with the Developer ID identity (hardened runtime,
# timestamp, inside-out: Sparkle's XPC services, Updater.app, Autoupdate, the framework; every dylib; the helpers; the
# app last), notarized, stapled -> target/dist/release-minnotes/minNotes-<version>-macOS.dmg (2.x's asset name: the
# feed's entries link to it), signed, notarized and stapled too. No entitlements, as QCView 3: nothing here runs a
# script engine or loads a library signed by another team (2.x's four entitlements were Qt's and the sandbox's).
# The identity and the notary profile live in the login keychain; nothing secret is in this file.
# Run from the repo root:  zsh packaging/macos/sign-macos-minnotes.sh [--skip-notarize]
set -euo pipefail
ROOT="$PWD"; SRC="$ROOT/target/dist/minNotes.app"; OUT="$ROOT/target/dist/release-minnotes"; APP="$OUT/minNotes.app"
SKIP=0; [ "${1:-}" = "--skip-notarize" ] && SKIP=1
PROFILE="${NOTARY_PROFILE:-QCView}"; TEAM="${CODESIGN_TEAM_ID:-5Z4S9VHV56}"
IDENTITY="${CODESIGN_IDENTITY:-$(security find-identity -v -p codesigning | sed -n "s/.*\"\(Developer ID Application: .*(${TEAM})\)\".*/\1/p" | head -1)}"
[ -n "$IDENTITY" ] || { echo "no Developer ID Application identity for team $TEAM in the keychain" >&2; exit 1; }
[ -d "$SRC" ] || { echo "no bundle at $SRC — run packaging/macos/make-app-minnotes.sh" >&2; exit 1; }
VERSION="$(/usr/libexec/PlistBuddy -c 'Print :CFBundleShortVersionString' "$SRC/Contents/Info.plist")"
DMG="$OUT/minNotes-$VERSION-macOS.dmg"
echo "==> minNotes $VERSION"
rm -rf "$OUT"; mkdir -p "$OUT"; ditto "$SRC" "$APP"
sign() { codesign --force --options runtime --timestamp --sign "$IDENTITY" "$@" 2>&1 | grep -v "replacing existing signature" || true; }
SPV="$APP/Contents/Frameworks/Sparkle.framework/Versions/B"
for nested in "$SPV/XPCServices/Downloader.xpc" "$SPV/XPCServices/Installer.xpc" "$SPV/Updater.app" "$SPV/Autoupdate"; do [ -e "$nested" ] && sign "$nested"; done
sign "$APP/Contents/Frameworks/Sparkle.framework"
for f in "$APP"/Contents/Frameworks/*.dylib; do sign "$f"; done
for f in "$APP"/Contents/Helpers/*; do sign "$f"; done
sign "$APP"
codesign --verify --deep --strict "$APP"
notarize() { xcrun notarytool submit "$1" --keychain-profile "$PROFILE" --wait 2>&1 | grep -E "^\s*(id|status):" | tail -2; }
if [ $SKIP = 0 ]; then
  echo "==> notarize the app (waits for Apple)"
  ditto -c -k --keepParent "$APP" "$OUT/notarize.zip"; notarize "$OUT/notarize.zip"; rm -f "$OUT/notarize.zip"
  xcrun stapler staple -q "$APP"; spctl --assess --type exec -vv "$APP"
fi
echo "==> DMG"
mkdir -p "$OUT/stage"; ditto "$APP" "$OUT/stage/minNotes.app"; ln -s /Applications "$OUT/stage/Applications"
hdiutil create -volname "minNotes $VERSION" -srcfolder "$OUT/stage" -ov -format UDZO -quiet "$DMG"; rm -rf "$OUT/stage"
codesign --force --timestamp --sign "$IDENTITY" "$DMG"
if [ $SKIP = 0 ]; then
  echo "==> notarize the DMG"
  notarize "$DMG"; xcrun stapler staple -q "$DMG"; spctl --assess --type open --context context:primary-signature -vv "$DMG"
fi
echo "==> $DMG ($(du -h "$DMG" | cut -f1)) sha256 $(shasum -a 256 "$DMG" | awk '{print $1}')"
