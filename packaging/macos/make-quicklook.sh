#!/bin/zsh
# QCView's Quick Look extension (canon rows 605-611): apps/qcv-quicklook (the Rust library) + its Swift shells, built by
# hand (swiftc, no Xcode project) into a WALK bundle - target/dist/quicklook-walk/QCView Quick Look Walk.app - signed with
# the Developer ID identity under the walk identifier and registered with the system, so that Finder's space bar on an
# .exr shows it. (Finder refuses an ad hoc extension; an extension's container belongs to the signature that made it, so
# the walk never uses the release identifier - docs/spikes/S20-quicklook.md, S21-preview-view.md.) Putting the extension
# into qcview.app is make-app.sh's, at the packaging step.
#   zsh packaging/macos/make-quicklook.sh            build, sign, register (needs packaging/macos/make-app.sh run once:
#                                                    the extension loads the staged bundle's OpenEXR)
#   zsh packaging/macos/make-quicklook.sh --remove   take the walk bundle off the system again
set -euo pipefail
ROOT="$PWD"; SRC="$ROOT/apps/qcv-quicklook"; OUT="$ROOT/target/dist/quicklook-walk"; APP="$OUT/QCView Quick Look Walk.app"; C="$APP/Contents"; X="$C/PlugIns/QCViewPreview.appex"
BID=app.qcview.walk
LSREG=/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister
reset_ql() { qlmanage -r > /dev/null 2>&1 || true; killall QuickLookUIService QuickLookSatellite 2> /dev/null || true; }
if [ "${1:-}" = "--remove" ]; then pluginkit -r "$X" 2> /dev/null || true; [ -d "$APP" ] && $LSREG -u "$APP" || true; rm -rf "$APP"; reset_ql; echo removed; exit 0; fi
ID="$(security find-identity -v -p codesigning | sed -n 's/.*"\(Developer ID Application: .*\)".*/\1/p' | head -1)"; [ -n "$ID" ] || { echo "no Developer ID Application identity in the keychain" >&2; exit 1; }
FWSRC="$ROOT/target/dist/qcview.app/Contents/Frameworks"; [ -f "$FWSRC/libOpenEXRCore-3_5.34.dylib" ] || { echo "no staged bundle: run packaging/macos/make-app.sh once" >&2; exit 1; }
VERSION="$(sed -n 's/^version *= *"\(.*\)"/\1/p' "$ROOT/apps/qcv/Cargo.toml" | head -1)"
# alone: with other packages in the same build the colour crate would take their OCIO feature and its library
MACOSX_DEPLOYMENT_TARGET=13.0 cargo build --release -p qcv-quicklook
pluginkit -r "$X" 2> /dev/null || true; rm -rf "$APP"; mkdir -p "$C/MacOS" "$C/Frameworks" "$X/Contents/MacOS" "$OUT/obj"
cp "$FWSRC"/libOpenEXRCore-*.dylib "$FWSRC"/libImath-*.dylib "$C/Frameworks/"
print 'print("QCView Quick Look walk: this application only carries the extension")' > "$OUT/obj/Host.swift"; xcrun swiftc -O -target arm64-apple-macos13.0 "$OUT/obj/Host.swift" -o "$C/MacOS/QCViewQuickLookWalk"
python3 - "$C" "$BID" "$VERSION" <<'PY'
import plistlib, sys, pathlib
c, bid, ver = pathlib.Path(sys.argv[1]), sys.argv[2], sys.argv[3]
base = {"CFBundleInfoDictionaryVersion": "6.0", "CFBundleShortVersionString": ver, "CFBundleVersion": ver, "LSMinimumSystemVersion": "13.0"}
plistlib.dump(dict(base, CFBundleExecutable="QCViewQuickLookWalk", CFBundleIdentifier=bid, CFBundleName="QCView Quick Look Walk", CFBundlePackageType="APPL", LSUIElement=True), open(c / "Info.plist", "wb"))
plistlib.dump(dict(base, CFBundleExecutable="QCViewPreview", CFBundleIdentifier=bid + ".preview", CFBundleName="QCViewPreview", CFBundleDisplayName="QCView", CFBundlePackageType="XPC!",
    NSExtension={"NSExtensionPointIdentifier": "com.apple.quicklook.preview", "NSExtensionPrincipalClass": "QCVPreview",
                 "NSExtensionAttributes": {"QLSupportedContentTypes": ["com.ilm.openexr-image"], "QLIsDataBasedPreview": False, "QLSupportsSearchableItems": False}}), open(c / "PlugIns/QCViewPreview.appex/Contents/Info.plist", "wb"))
PY
xcrun swiftc -O -target arm64-apple-macos13.0 -parse-as-library -application-extension -module-name QCViewPreview "$SRC/swift/Preview.swift" "$ROOT/target/release/libqcv_quicklook.a" "$C"/Frameworks/libOpenEXRCore-*.dylib \
  -lz -lc++ -liconv -framework ImageIO -framework CoreData -framework CoreText -framework CoreImage -framework CloudKit -framework QuartzCore -framework CoreVideo -framework Metal \
  -Xlinker -rpath -Xlinker @loader_path/../../../../Frameworks -Xlinker -e -Xlinker _NSExtensionMain -o "$X/Contents/MacOS/QCViewPreview"
strip -S -x "$X/Contents/MacOS/QCViewPreview" 2> /dev/null
ENT="$OUT/obj/extension.entitlements"
print '<?xml version="1.0" encoding="UTF-8"?><plist version="1.0"><dict><key>com.apple.security.app-sandbox</key><true/><key>com.apple.security.files.user-selected.read-only</key><true/></dict></plist>' > "$ENT"
sign() { codesign --force --options runtime --timestamp=none --sign "$ID" "$@" 2>&1 | grep -v "replacing existing signature" || true; }
sign "$C"/Frameworks/*.dylib; sign --entitlements "$ENT" "$X"; sign "$APP"   # inside out; never --deep (it would drop the extension's entitlements)
$LSREG -f -R -trusted "$APP"; pluginkit -a "$X"; pluginkit -e use -i $BID.preview; reset_ql; sleep 1
pluginkit -m -v -i $BID.preview | head -1
echo "built $APP - extension $(du -sh "$X" | cut -f1), QCView $VERSION"
