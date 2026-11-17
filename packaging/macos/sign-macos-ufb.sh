#!/bin/zsh
# The signed macOS artifact of UFB (canon rows 457, 519, 538; minNotes' sign-macos-minnotes.sh with UFB's names), in the
# two kinds make-app-ufb.sh makes:
#   test      target/dist/UFB Test.app -> a COPY signed with the Developer ID identity (hardened runtime, timestamp,
#             inside-out: every dylib, FFmpeg's program, the app last), notarized, stapled ->
#             target/dist/release-ufb/UFB-Test-<version>-macOS.dmg, signed, notarized and stapled too. It installs
#             beside a real UFB 1.4.x (its own bundle id, data and mesh).
#   release   target/dist/UFB.app (Sparkle's parts signed first) -> the same, as UFB-<version>-macOS.dmg, AND the one-time
#             installer package UFB-<version>-arm64.pkg (row 457: an installed 1.4.x is updated by a package, and what
#             it laid down - /Applications/UFB/ with its agent and its Finder extension - must go; see below).
# No entitlements, as QCView 3: nothing here runs a script engine or loads a library signed by another team.
# The identity and the notary profile live in the login keychain; nothing secret is in this file.
# SINCE THE RELEASE AFTER 2.0.3 (row 546) the installer package is made only when asked for (`--with-installer`):
# 2.0.3 was the last version an installed 1.4.x is led to (the old feed, appcast-mac.xml, ends with it); from then
# on an update is the disk image, installed in place by Sparkle from the new feed (appcast.xml).
# Run from the repo root:  zsh packaging/macos/sign-macos-ufb.sh [test|release] [--skip-notarize] [--with-installer]
set -euo pipefail
MODE="${1:-test}"; SKIP=0; INSTALLER_PKG=0; for a in "$@"; do case "$a" in --skip-notarize) SKIP=1 ;; --with-installer) INSTALLER_PKG=1 ;; esac; done
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
# The release's ONE-TIME INSTALLER PACKAGE (rows 457, 539; UFB 1.4.x's build-mac-pkg.sh as the model): an installed 1.4.x
# is updated by a package (its Sparkle runs a guided install), and what it laid down - /Applications/UFB/ with UFB.app,
# ufb-agent.app and the uninstaller - must go. The payload is /Applications/UFB.app alone; the preinstall script stops
# the old processes and removes the old folder; the package id is 1.4.x's, so its receipt is replaced. Signed with the
# Developer ID INSTALLER identity (not the application's: Installer refuses that), notarized and stapled.
# UFB-<version>-arm64.pkg is the name 1.4.x's feed entries link to.
if [ "$MODE" = release ] && [ $INSTALLER_PKG = 0 ]; then rm -rf "$APP"; fi   # (the signed copy of the release's bundle is not left lying)
if [ "$MODE" = release ] && [ $INSTALLER_PKG = 1 ]; then
  echo "==> the installer package"
  INSTALLER="${INSTALLER_IDENTITY:-$(security find-identity -v | sed -n "s/.*\"\(Developer ID Installer: .*(${TEAM})\)\".*/\1/p" | head -1)}"
  [ -n "$INSTALLER" ] || { echo "no Developer ID Installer identity for team $TEAM in the keychain" >&2; exit 1; }
  PKG="$OUT/UFB-$VERSION-arm64.pkg"; ST="$OUT/pkg-stage"; rm -rf "$ST" "$PKG"; mkdir -p "$ST/root/Applications" "$ST/pkgs" "$ST/scripts"
  ditto "$APP" "$ST/root/Applications/UFB.app"
  cp "$ROOT/packaging/macos/pkg-ufb/preinstall" "$ST/scripts/preinstall"; chmod 755 "$ST/scripts/preinstall"
  pkgbuild --analyze --root "$ST/root" "$ST/components.plist" > /dev/null
  i=0; while /usr/libexec/PlistBuddy -c "Print :$i" "$ST/components.plist" > /dev/null 2>&1; do
    /usr/libexec/PlistBuddy -c "Set :$i:BundleIsRelocatable false" "$ST/components.plist" 2> /dev/null || /usr/libexec/PlistBuddy -c "Add :$i:BundleIsRelocatable bool false" "$ST/components.plist"
    i=$((i + 1)); done
  pkgbuild --root "$ST/root" --component-plist "$ST/components.plist" --identifier dev.ufb.UFB --version "$VERSION" --install-location / --scripts "$ST/scripts" "$ST/pkgs/UFB-core.pkg" > /dev/null
  sed "s/@VERSION@/$VERSION/g" "$ROOT/packaging/macos/pkg-ufb/distribution.xml" > "$ST/distribution.xml"
  productbuild --distribution "$ST/distribution.xml" --package-path "$ST/pkgs" --sign "$INSTALLER" --timestamp "$PKG" > /dev/null
  rm -rf "$ST"; pkgutil --check-signature "$PKG" | sed 's/^/  /' | head -3
  if [ $SKIP = 0 ]; then echo "==> notarize the package"; notarize "$PKG"; xcrun stapler staple -q "$PKG"; spctl --assess --type install -vv "$PKG"; fi
  echo "==> $PKG ($(du -h "$PKG" | cut -f1)) sha256 $(shasum -a 256 "$PKG" | awk '{print $1}')"
  # (the signed copy of the release's bundle carries 1.4.x's identity and link schemes: it is not left lying on this machine)
  rm -rf "$APP"
fi
echo "==> $DMG ($(du -h "$DMG" | cut -f1)) sha256 $(shasum -a 256 "$DMG" | awk '{print $1}')"
