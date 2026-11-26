#!/bin/zsh
# S20: builds target/s20/S20Host.app with its two extensions by hand (swiftc, no Xcode project), signs it and registers it
# with the system. Run from the repo root:  zsh spikes/s20-quicklook/build.sh [--siblings] [--identity "<name>"]
#   --siblings   adds the sandbox exception that lets the extensions read beside the file (read-only, the whole disk)
# zsh spikes/s20-quicklook/build.sh --remove   takes the registration and the bundle away again.
set -euo pipefail
ROOT="$PWD"; SRC="$ROOT/spikes/s20-quicklook"; OUT="$ROOT/target/s20"; APP="$OUT/S20Host.app"; C="$APP/Contents"
LSREG=/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister
if [ "${1:-}" = "--remove" ]; then
  for x in thumb preview; do pluginkit -r "$C/PlugIns/S20${(C)x}.appex" 2> /dev/null || true; done
  [ -d "$APP" ] && $LSREG -u "$APP" || true; rm -rf "$APP"; qlmanage -r > /dev/null 2>&1; qlmanage -r cache > /dev/null 2>&1; echo removed; exit 0
fi
SIB=0; ID="-"; while [ $# -gt 0 ]; do case "$1" in --siblings) SIB=1 ;; --identity) ID="$2"; shift ;; esac; shift; done
rm -rf "$APP"; mkdir -p "$C/MacOS" "$C/PlugIns" "$OUT/obj"
print 'import Foundation\nprint("S20 host: nothing to do")' > "$OUT/obj/Host.swift"
xcrun swiftc -O "$OUT/obj/Host.swift" -o "$C/MacOS/S20Host"
# A container belongs to the signature that made it: the same identifier signed another way waits for ever in the sandbox's
# start (the system asks the person whether it may read "another application's" data). So each way of signing has its own identifier.
BID=app.qcview.spike.s20; [ "$ID" != "-" ] && BID=app.qcview.spike.s20d
python3 - "$C" "$BID" <<'PY'
import plistlib, sys, pathlib
c = pathlib.Path(sys.argv[1])
def imp(i, desc, conf, ext): return {"UTTypeIdentifier": i, "UTTypeDescription": desc, "UTTypeConformsTo": conf, "UTTypeTagSpecification": {"public.filename-extension": [ext]}}
base = {"CFBundleInfoDictionaryVersion": "6.0", "CFBundleShortVersionString": "0.1", "CFBundleVersion": "1", "LSMinimumSystemVersion": "13.0"}
host = dict(base, CFBundleExecutable="S20Host", CFBundleIdentifier=sys.argv[2], CFBundleName="S20Host", CFBundlePackageType="APPL", LSUIElement=True,
    UTImportedTypeDeclarations=[imp("org.smpte.dpx", "DPX image", ["public.image"], "dpx"), imp("org.matroska.mkv", "Matroska video", ["public.movie"], "mkv")])
plistlib.dump(host, open(c / "Info.plist", "wb"))
types = ["org.smpte.mxf", "com.ilm.openexr-image", "org.smpte.dpx", "org.matroska.mkv"]
for name, point, attrs in [("S20Thumb", "com.apple.quicklook.thumbnail", {"QLSupportedContentTypes": types, "QLThumbnailMinimumDimension": 0}),
                           ("S20Preview", "com.apple.quicklook.preview", {"QLSupportedContentTypes": types, "QLIsDataBasedPreview": False, "QLSupportsSearchableItems": False})]:
    d = c / "PlugIns" / f"{name}.appex" / "Contents"; (d / "MacOS").mkdir(parents=True)
    plistlib.dump(dict(base, CFBundleExecutable=name, CFBundleIdentifier=f"{sys.argv[2]}.{name[3:].lower()}", CFBundleName=name, CFBundleDisplayName=f"S20 {name[3:]}",
        CFBundlePackageType="XPC!", NSExtension={"NSExtensionPointIdentifier": point, "NSExtensionPrincipalClass": name, "NSExtensionAttributes": attrs}), open(d / "Info.plist", "wb"))
PY
ext() { xcrun swiftc -O -parse-as-library -application-extension -module-name "$1" "${@:2}" -Xlinker -e -Xlinker _NSExtensionMain -o "$C/PlugIns/$1.appex/Contents/MacOS/$1"; }
# the decode: this spike's Rust library, on the libraries of a staged QCView bundle (already @rpath, no Homebrew), in the HOST's Frameworks
FWSRC="$ROOT/target/dist/qcview.app/Contents/Frameworks"; [ -f "$FWSRC/libavcodec.63.dylib" ] || { echo "no staged bundle: run packaging/macos/make-app.sh once" >&2; exit 1; }
cargo build --release -p s20-quicklook; mkdir -p "$C/Frameworks"; cp "$FWSRC"/lib{av,sw}*.dylib "$C/Frameworks/"
ext S20Thumb "$SRC/Thumb.swift" "$ROOT/target/release/libs20_quicklook.a" "$C"/Frameworks/lib{avcodec,avformat,avutil,swscale,swresample,avfilter}.*.dylib -Xlinker -rpath -Xlinker @loader_path/../../../../Frameworks
{ print 'import Foundation'; sed -n '/^func s20Neighbours/,/^}/p;/^func s20Note/,/^}/p' "$SRC/Thumb.swift"; } > "$OUT/obj/N.swift"
ext S20Preview "$SRC/Preview.swift" "$OUT/obj/N.swift"
ENT="$OUT/obj/ext.entitlements"
{ print '<?xml version="1.0" encoding="UTF-8"?><plist version="1.0"><dict><key>com.apple.security.app-sandbox</key><true/><key>com.apple.security.files.user-selected.read-only</key><true/>'
  [ $SIB = 1 ] && print '<key>com.apple.security.temporary-exception.files.absolute-path.read-only</key><array><string>/</string></array>'
  print '</dict></plist>'; } > "$ENT"
RT=(); [ "$ID" != "-" ] && RT=(--options runtime --timestamp=none)
codesign --force "${RT[@]}" --sign "$ID" "$C"/Frameworks/*.dylib
for a in "$C"/PlugIns/*.appex; do codesign --force "${RT[@]}" --sign "$ID" --entitlements "$ENT" "$a"; done
codesign --force "${RT[@]}" --sign "$ID" "$APP"
$LSREG -f -R -trusted "$APP"; for a in "$C"/PlugIns/*.appex; do pluginkit -a "$a"; done
sleep 1; pluginkit -m -v -i $BID.thumb; pluginkit -m -v -i $BID.preview
xcrun swiftc -O "$SRC/probe.swift" -o "$OUT/probe"; echo "built $APP (siblings exception: $SIB, identity: $ID)"
