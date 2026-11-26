#!/bin/zsh
# QCView's Quick Look extension (canon rows 605-611): apps/qcv-quicklook (the Rust library) + its Swift shells, built by
# hand (swiftc, no Xcode project) into a WALK bundle - target/dist/quicklook-walk/QCView Quick Look Walk.app - signed with
# the Developer ID identity under the walk identifier and registered with the system, so that Finder's space bar on an
# .exr shows it and Finder's icons of .exr and .mxf files are its thumbnails. (Finder refuses an ad hoc extension; an extension's container belongs to the signature that made it, so
# the walk never uses the release identifier - docs/spikes/S20-quicklook.md, S21-preview-view.md.) Putting the extension
# into qcview.app is make-app.sh's, at the packaging step.
#   zsh packaging/macos/make-quicklook.sh            build, sign, register (needs packaging/macos/make-app.sh run once:
#                                                    the extension loads the staged bundle's OpenEXR)
#   zsh packaging/macos/make-quicklook.sh --readout  the same, with the player's readout along the top of the preview
#   zsh packaging/macos/make-quicklook.sh --remove   take the walk bundle off the system again
#   zsh packaging/macos/make-quicklook.sh --into <qcview.app> --id <identifier prefix>
#                                                    make-app.sh's: the two extensions built into that bundle's
#                                                    Contents/PlugIns on the libraries already in its Frameworks; nothing
#                                                    is signed or registered (the caller signs)
set -euo pipefail
ROOT="$PWD"; SRC="$ROOT/apps/qcv-quicklook"; OUT="$ROOT/target/dist/quicklook-walk"; APP="$OUT/QCView Quick Look Walk.app"; C="$APP/Contents"; X="$C/PlugIns/QCViewPreview.appex"; T="$C/PlugIns/QCViewThumbnail.appex"
BID=app.qcview.walk; INTO=""; READOUT=""; [ "${1:-}" = "--readout" ] && READOUT=1
if [ "${1:-}" = "--into" ]; then INTO="$2"; [ "${3:-}" = "--id" ] && BID="$4"; APP="$INTO"; C="$APP/Contents"; X="$C/PlugIns/QCViewPreview.appex"; T="$C/PlugIns/QCViewThumbnail.appex"; OUT="$ROOT/target/dist/quicklook-obj"
  [ -f "$C/Frameworks/libavcodec.63.dylib" ] || { echo "$INTO has no Frameworks yet: build the extensions after the libraries are copied" >&2; exit 1; }; fi
LSREG=/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister
reset_ql() { qlmanage -r > /dev/null 2>&1 || true; qlmanage -r cache > /dev/null 2>&1 || true; killall QuickLookUIService QuickLookSatellite 2> /dev/null || true; }
if [ "${1:-}" = "--remove" ]; then pluginkit -r "$X" 2> /dev/null || true; pluginkit -r "$T" 2> /dev/null || true; [ -d "$APP" ] && $LSREG -u "$APP" || true; rm -rf "$APP"; reset_ql; echo removed; exit 0; fi
[ -n "$INTO" ] || ID="$(security find-identity -v -p codesigning | sed -n 's/.*"\(Developer ID Application: .*\)".*/\1/p' | head -1)"; [ -n "$INTO" ] || [ -n "$ID" ] || { echo "no Developer ID Application identity in the keychain" >&2; exit 1; }
FWSRC="$ROOT/target/dist/qcview.app/Contents/Frameworks"; [ -n "$INTO" ] || [ -f "$FWSRC/libOpenEXRCore-3_5.34.dylib" ] || { echo "no staged bundle: run packaging/macos/make-app.sh once" >&2; exit 1; }
VERSION="$(sed -n 's/^version *= *"\(.*\)"/\1/p' "$ROOT/apps/qcv/Cargo.toml" | head -1)"
# alone: with other packages in the same build the colour crate would take their OCIO feature and its library
MACOSX_DEPLOYMENT_TARGET=13.0 cargo build --release -p qcv-quicklook
if [ -z "$INTO" ]; then
  pluginkit -r "$X" 2> /dev/null || true; pluginkit -r "$T" 2> /dev/null || true; rm -rf "$APP"; mkdir -p "$C/MacOS" "$C/Frameworks"
  cp "$FWSRC"/libOpenEXRCore-*.dylib "$FWSRC"/libImath-*.dylib "$FWSRC"/lib{av,sw}*.dylib "$C/Frameworks/"
  mkdir -p "$OUT/obj"; print 'print("QCView Quick Look walk: this application only carries the extension")' > "$OUT/obj/Host.swift"; xcrun swiftc -O -target arm64-apple-macos13.0 "$OUT/obj/Host.swift" -o "$C/MacOS/QCViewQuickLookWalk"
fi
rm -rf "$X" "$T"; mkdir -p "$X/Contents/MacOS" "$T/Contents/MacOS" "$OUT/obj"
python3 - "$C" "$BID" "$VERSION" "$INTO" "$READOUT" <<'PY'
import plistlib, sys, pathlib
c, bid, ver = pathlib.Path(sys.argv[1]), sys.argv[2], sys.argv[3]
base = {"CFBundleInfoDictionaryVersion": "6.0", "CFBundleShortVersionString": ver, "CFBundleVersion": ver, "LSMinimumSystemVersion": "13.0"}
if not sys.argv[4]: plistlib.dump(dict(base, CFBundleExecutable="QCViewQuickLookWalk", CFBundleIdentifier=bid, CFBundleName="QCView Quick Look Walk", CFBundlePackageType="APPL", LSUIElement=True), open(c / "Info.plist", "wb"))
def ext(name, cls, point, attrs):
    extra = {"QCVQuickLookReadout": True} if cls == "preview" and len(sys.argv) > 5 and sys.argv[5] else {}
    plistlib.dump(dict(base, **extra, CFBundleExecutable=name, CFBundleIdentifier=bid + "." + cls, CFBundleName=name, CFBundleDisplayName="QCView", CFBundlePackageType="XPC!",
        NSExtension={"NSExtensionPointIdentifier": point, "NSExtensionPrincipalClass": "QCV" + cls.capitalize(), "NSExtensionAttributes": attrs}), open(c / f"PlugIns/{name}.appex/Contents/Info.plist", "wb"))
# canon row 606: MXF and EXR only, for the preview and for the thumbnails.
ext("QCViewPreview", "preview", "com.apple.quicklook.preview", {"QLSupportedContentTypes": ["com.ilm.openexr-image", "org.smpte.mxf"], "QLIsDataBasedPreview": False, "QLSupportsSearchableItems": False})
ext("QCViewThumbnail", "thumbnail", "com.apple.quicklook.thumbnail", {"QLSupportedContentTypes": ["com.ilm.openexr-image", "org.smpte.mxf"], "QLThumbnailMinimumDimension": 0})
PY
# one library for both; what an extension does not call is stripped, and so are the libraries it then does not need
LIBS=("$ROOT/target/release/libqcv_quicklook.a" "$C"/Frameworks/libOpenEXRCore-*.dylib "$C"/Frameworks/lib{avutil,avformat,avfilter,swscale,swresample,avcodec}.*.dylib
  -lz -lc++ -liconv -framework ImageIO -framework AudioToolbox -framework CoreAudio -framework AudioUnit -framework CoreData -framework CoreText -framework CoreImage -framework CloudKit -framework QuartzCore -framework CoreVideo -framework Metal
  -Xlinker -dead_strip -Xlinker -dead_strip_dylibs -Xlinker -rpath -Xlinker @loader_path/../../../../Frameworks -Xlinker -e -Xlinker _NSExtensionMain)
build_ext() { xcrun swiftc -O -target arm64-apple-macos13.0 -parse-as-library -application-extension -module-name "$1" "$SRC/swift/$2" "${LIBS[@]}" -o "$C/PlugIns/$1.appex/Contents/MacOS/$1"; strip -S -x "$C/PlugIns/$1.appex/Contents/MacOS/$1" 2> /dev/null; }
build_ext QCViewPreview Preview.swift; build_ext QCViewThumbnail Thumbnail.swift
for f in "$X/Contents/MacOS/QCViewPreview" "$T/Contents/MacOS/QCViewThumbnail"; do [ "$(vtool -show-build "$f" | awk '$1=="minos"{print $2}')" = "13.0" ] || { echo "minos is not 13.0: $f" >&2; exit 1; }; done   # canon row 614
[ -n "$INTO" ] && { echo "built into $APP - preview $(du -sh "$X" | cut -f1), thumbnail $(du -sh "$T" | cut -f1)"; exit 0; }
ENT="$ROOT/packaging/macos/quicklook.entitlements"
sign() { codesign --force --options runtime --timestamp=none --sign "$ID" "$@" 2>&1 | grep -v "replacing existing signature" || true; }
sign "$C"/Frameworks/*.dylib; sign --entitlements "$ENT" "$X"; sign --entitlements "$ENT" "$T"; sign "$APP"   # inside out; never --deep (it would drop the extension's entitlements)
$LSREG -f -R -trusted "$APP"; pluginkit -a "$X"; pluginkit -a "$T"; pluginkit -e use -i $BID.preview; pluginkit -e use -i $BID.thumbnail; reset_ql; sleep 1
pluginkit -m -v -i $BID.preview | head -1 | cut -c1-70; pluginkit -m -v -i $BID.thumbnail | head -1 | cut -c1-70
echo "built $APP - preview $(du -sh "$X" | cut -f1), thumbnail $(du -sh "$T" | cut -f1), QCView $VERSION"
