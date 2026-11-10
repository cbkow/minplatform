#!/bin/zsh
# The macOS release artifact (canon row 248): target/dist/qcview.app (make-app.sh) → a COPY signed with the Developer ID
# identity (hardened runtime, timestamp, inside-out: Sparkle's XPC services, Updater.app, Autoupdate, the framework; every
# dylib; the app last), notarized, stapled → target/dist/release/QCView-MacOS.dmg (the name the feed and the site link to),
# signed, notarized and stapled too. QCView-Player's sign-and-notarize.sh without the Qt steps. No entitlements: the port
# has no script engine and loads only libraries signed by the same team (2.x's four were Qt's).
# The identity and the notary profile live in the login keychain; nothing secret is in this file.
# Run from the repo root:  zsh packaging/macos/sign-macos.sh [--skip-notarize]
set -euo pipefail
ROOT="$PWD"; SRC="$ROOT/target/dist/qcview.app"; OUT="$ROOT/target/dist/release"; APP="$OUT/qcview.app"; DMG="$OUT/QCView-MacOS.dmg"
SKIP=0; [ "${1:-}" = "--skip-notarize" ] && SKIP=1
PROFILE="${NOTARY_PROFILE:-QCView}"; TEAM="${CODESIGN_TEAM_ID:-5Z4S9VHV56}"
IDENTITY="${CODESIGN_IDENTITY:-$(security find-identity -v -p codesigning | sed -n "s/.*\"\(Developer ID Application: .*(${TEAM})\)\".*/\1/p" | head -1)}"
[ -n "$IDENTITY" ] || { echo "no Developer ID Application identity for team $TEAM in the keychain" >&2; exit 1; }
[ -d "$SRC" ] || { echo "no bundle at $SRC — run packaging/macos/make-app.sh" >&2; exit 1; }
VERSION="$(/usr/libexec/PlistBuddy -c 'Print :CFBundleShortVersionString' "$SRC/Contents/Info.plist")"
echo "==> QCView $VERSION"
rm -rf "$OUT"; mkdir -p "$OUT"; ditto "$SRC" "$APP"
sign() { codesign --force --options runtime --timestamp --sign "$IDENTITY" "$@" 2>&1 | grep -v "replacing existing signature" || true; }
SPV="$APP/Contents/Frameworks/Sparkle.framework/Versions/B"
for nested in "$SPV/XPCServices/Downloader.xpc" "$SPV/XPCServices/Installer.xpc" "$SPV/Updater.app" "$SPV/Autoupdate"; do [ -e "$nested" ] && sign "$nested"; done
sign "$APP/Contents/Frameworks/Sparkle.framework"
for f in "$APP"/Contents/Frameworks/*.dylib; do sign "$f"; done
sign "$APP"
codesign --verify --deep --strict "$APP"
notarize() { xcrun notarytool submit "$1" --keychain-profile "$PROFILE" --wait 2>&1 | grep -E "^\s*(id|status):" | tail -2; }
if [ $SKIP = 0 ]; then
  echo "==> notarize the app (waits for Apple)"
  ditto -c -k --keepParent "$APP" "$OUT/notarize.zip"; notarize "$OUT/notarize.zip"; rm -f "$OUT/notarize.zip"
  xcrun stapler staple -q "$APP"; spctl --assess --type exec -vv "$APP"
fi
echo "==> DMG"
mkdir -p "$OUT/stage"; ditto "$APP" "$OUT/stage/qcview.app"; ln -s /Applications "$OUT/stage/Applications"
hdiutil create -volname "QCView $VERSION" -srcfolder "$OUT/stage" -ov -format UDZO -quiet "$DMG"; rm -rf "$OUT/stage"
codesign --force --timestamp --sign "$IDENTITY" "$DMG"
if [ $SKIP = 0 ]; then
  echo "==> notarize the DMG"
  notarize "$DMG"; xcrun stapler staple -q "$DMG"; spctl --assess --type open --context context:primary-signature -vv "$DMG"
fi
echo "==> $DMG ($(du -h "$DMG" | cut -f1)) sha256 $(shasum -a 256 "$DMG" | awk '{print $1}')"
