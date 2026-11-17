#!/bin/zsh
# The signed macOS artifact of UFB (canon rows 457, 519, 538; minNotes' sign-macos-minnotes.sh with UFB's names), in the
# two kinds make-app-ufb.sh makes:
#   test      target/dist/UFB Test.app -> a COPY signed with the Developer ID identity (hardened runtime, timestamp,
#             inside-out: every dylib, FFmpeg's program, the app last), notarized, stapled ->
#             target/dist/release-ufb/UFB-Test-<version>-macOS.dmg, signed, notarized and stapled too. It installs
#             beside a real UFB 1.4.x (its own bundle id, data and mesh).
#   release   target/dist/UFB.app (Sparkle's parts signed first) -> the same, as UFB-<version>-macOS.dmg. THE RELEASE'S
#             FIRST VERSION ALSO NEEDS THE ONE-TIME INSTALLER PACKAGE (row 457: an installed 1.4.x expects a package,
#             and what it leaves behind - its agent, its Finder extension, its mount folders, /Applications/UFB/ - must
#             be cleared): that package is NOT made here yet.
# No entitlements, as QCView 3: nothing here runs a script engine or loads a library signed by another team.
# The identity and the notary profile live in the login keychain; nothing secret is in this file.
# Run from the repo root:  zsh packaging/macos/sign-macos-ufb.sh [test|release] [--skip-notarize]
set -euo pipefail
MODE="${1:-test}"; SKIP=0; [ "${2:-}" = "--skip-notarize" ] && SKIP=1
case "$MODE" in test) NAME="UFB Test"; ASSET="UFB-Test" ;; release) NAME="UFB"; ASSET="UFB" ;; *) echo "usage: sign-macos-ufb.sh [test|release] [--skip-notarize]" >&2; exit 1 ;; esac
ROOT="$PWD"; SRC="$ROOT/target/dist/$NAME.app"; OUT="$ROOT/target/dist/release-ufb"; APP="$OUT/$NAME.app"
PROFILE="${NOTARY_PROFILE:-QCView}"; TEAM="${CODESIGN_TEAM_ID:-5Z4S9VHV56}"
IDENTITY="${CODESIGN_IDENTITY:-$(security find-identity -v -p codesigning | sed -n "s/.*\"\(Developer ID Application: .*(${TEAM})\)\".*/\1/p" | head -1)}"
[ -n "$IDENTITY" ] || { echo "no Developer ID Application identity for team $TEAM in the keychain" >&2; exit 1; }
[ -d "$SRC" ] || { echo "no bundle at $SRC - run packaging/macos/make-app-ufb.sh $MODE" >&2; exit 1; }
VERSION="$(/usr/libexec/PlistBuddy -c 'Print :CFBundleShortVersionString' "$SRC/Contents/Info.plist")"
DMG="$OUT/$ASSET-$VERSION-macOS.dmg"
echo "==> $NAME $VERSION"
mkdir -p "$OUT"; rm -rf "$APP" "$DMG"; ditto "$SRC" "$APP"
sign() { codesign --force --options runtime --timestamp --sign "$IDENTITY" "$@" 2>&1 | grep -v "replacing existing signature" || true; }
SPV="$APP/Contents/Frameworks/Sparkle.framework/Versions/B"
for nested in "$SPV/XPCServices/Downloader.xpc" "$SPV/XPCServices/Installer.xpc" "$SPV/Updater.app" "$SPV/Autoupdate"; do [ -e "$nested" ] && sign "$nested"; done
[ -d "$APP/Contents/Frameworks/Sparkle.framework" ] && sign "$APP/Contents/Frameworks/Sparkle.framework"
for f in "$APP"/Contents/Frameworks/*.dylib(N); do sign "$f"; done
for f in "$APP"/Contents/Helpers/*(N); do sign "$f"; done
sign "$APP"
codesign --verify --deep --strict "$APP"
notarize() { xcrun notarytool submit "$1" --keychain-profile "$PROFILE" --wait 2>&1 | grep -E "^\s*(id|status):" | tail -2; }
if [ $SKIP = 0 ]; then
  echo "==> notarize the app (waits for Apple)"
  ditto -c -k --keepParent "$APP" "$OUT/notarize.zip"; notarize "$OUT/notarize.zip"; rm -f "$OUT/notarize.zip"
  xcrun stapler staple -q "$APP"; spctl --assess --type exec -vv "$APP"
fi
echo "==> DMG"
rm -rf "$OUT/stage"; mkdir -p "$OUT/stage"; ditto "$APP" "$OUT/stage/$NAME.app"; ln -s /Applications "$OUT/stage/Applications"
hdiutil create -volname "$NAME $VERSION" -srcfolder "$OUT/stage" -ov -format UDZO -quiet "$DMG"; rm -rf "$OUT/stage"
codesign --force --timestamp --sign "$IDENTITY" "$DMG"
if [ $SKIP = 0 ]; then
  echo "==> notarize the DMG"
  notarize "$DMG"; xcrun stapler staple -q "$DMG"; spctl --assess --type open --context context:primary-signature -vv "$DMG"
fi
# (the signed copy of the release's bundle carries 1.4.x's identity and link schemes: it is not left lying on this machine)
[ "$MODE" = release ] && rm -rf "$APP"
echo "==> $DMG ($(du -h "$DMG" | cut -f1)) sha256 $(shasum -a 256 "$DMG" | awk '{print $1}')"
