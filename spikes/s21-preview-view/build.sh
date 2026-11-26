#!/bin/zsh
# S21: builds target/s21/S21Host.app with one preview extension for EXR (the Swift shell + this spike's Rust library on the
# staged QCView bundle's OpenEXR), signs it with the Developer ID identity and registers it. From the repo root:
#   zsh spikes/s21-preview-view/build.sh            (needs packaging/macos/make-app.sh run once)
#   zsh spikes/s21-preview-view/build.sh --adhoc    an ad hoc signature under its own identifier: this shell can then read the
#                                                   extension's log (a Developer ID container is closed to other programs)
#   zsh spikes/s21-preview-view/build.sh --remove   takes it off the system again
set -euo pipefail
ROOT="$PWD"; SRC="$ROOT/spikes/s21-preview-view"; APP="$ROOT/target/s21/S21Host.app"; C="$APP/Contents"; X="$C/PlugIns/S21Preview.appex"
LSREG=/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister
if [ "${1:-}" = "--remove" ]; then pluginkit -r "$X" 2> /dev/null || true; [ -d "$APP" ] && $LSREG -u "$APP" || true; rm -rf "$APP"; qlmanage -r > /dev/null 2>&1; echo removed; exit 0; fi
BID=app.qcview.spike.s21; ADHOC=0; [ "${1:-}" = "--adhoc" ] && { ADHOC=1; BID=app.qcview.spike.s21a; }
ID="$(security find-identity -v -p codesigning | sed -n 's/.*"\(Developer ID Application: .*\)".*/\1/p' | head -1)"; [ -n "$ID" ] || { echo "no Developer ID identity" >&2; exit 1; }
FWSRC="$ROOT/target/dist/qcview.app/Contents/Frameworks"; [ -f "$FWSRC/libOpenEXRCore-3_5.34.dylib" ] || { echo "no staged bundle: run packaging/macos/make-app.sh once" >&2; exit 1; }
cargo build --release -p s21-preview-view
pluginkit -r "$X" 2> /dev/null || true; rm -rf "$APP"; mkdir -p "$C/MacOS" "$C/Frameworks" "$X/Contents/MacOS" "$ROOT/target/s21/obj"
cp "$FWSRC"/libOpenEXRCore-*.dylib "$FWSRC"/libImath-*.dylib "$C/Frameworks/"
print 'print("S21 host: nothing to do")' > "$ROOT/target/s21/obj/Host.swift"; xcrun swiftc -O "$ROOT/target/s21/obj/Host.swift" -o "$C/MacOS/S21Host"
python3 - "$C" "$BID" <<'PY'
import plistlib, sys, pathlib
c = pathlib.Path(sys.argv[1]); base = {"CFBundleInfoDictionaryVersion": "6.0", "CFBundleShortVersionString": "0.1", "CFBundleVersion": "1", "LSMinimumSystemVersion": "13.0"}
plistlib.dump(dict(base, CFBundleExecutable="S21Host", CFBundleIdentifier=sys.argv[2], CFBundleName="S21Host", CFBundlePackageType="APPL", LSUIElement=True), open(c / "Info.plist", "wb"))
plistlib.dump(dict(base, CFBundleExecutable="S21Preview", CFBundleIdentifier=sys.argv[2] + ".preview", CFBundleName="S21Preview", CFBundleDisplayName="S21 Preview", CFBundlePackageType="XPC!",
    NSExtension={"NSExtensionPointIdentifier": "com.apple.quicklook.preview", "NSExtensionPrincipalClass": "S21Preview",
                 "NSExtensionAttributes": {"QLSupportedContentTypes": ["com.ilm.openexr-image"], "QLIsDataBasedPreview": False, "QLSupportsSearchableItems": False}}), open(c / "PlugIns/S21Preview.appex/Contents/Info.plist", "wb"))
PY
xcrun swiftc -O -parse-as-library -application-extension -module-name S21Preview "$SRC/Preview.swift" "$ROOT/target/release/libs21_preview_view.a" "$C"/Frameworks/libOpenEXRCore-*.dylib \
  -lz -liconv -framework ImageIO -framework CoreData -framework CoreText -framework CoreImage -framework CloudKit -framework QuartzCore -framework CoreVideo -framework Metal \
  -Xlinker -rpath -Xlinker @loader_path/../../../../Frameworks -Xlinker -e -Xlinker _NSExtensionMain -o "$X/Contents/MacOS/S21Preview"
ENT="$ROOT/target/s21/obj/ext.entitlements"
print '<?xml version="1.0" encoding="UTF-8"?><plist version="1.0"><dict><key>com.apple.security.app-sandbox</key><true/><key>com.apple.security.files.user-selected.read-only</key><true/></dict></plist>' > "$ENT"
S=(codesign --force --options runtime --timestamp=none --sign "$ID"); [ $ADHOC = 1 ] && S=(codesign --force --sign -)
"${S[@]}" "$C"/Frameworks/*.dylib; "${S[@]}" --entitlements "$ENT" "$X"; "${S[@]}" "$APP"
$LSREG -f -R -trusted "$APP"; pluginkit -a "$X"; sleep 1; pluginkit -m -v -i $BID.preview; echo "log: ~/Library/Containers/$BID.preview/Data/tmp/s21.log"
echo "built $APP ($(du -sh "$X" | cut -f1) extension)"
