#!/bin/zsh
# S22: builds target/s22/S22Host.app with one preview extension for MXF (the Swift shell + this spike's Rust library on
# the staged QCView bundle's FFmpeg), signs it with the Developer ID identity (Finder refuses an ad hoc extension, S21)
# and registers it. From the repo root:
#   zsh spikes/s22-preview-player/build.sh            (needs packaging/macos/make-app.sh run once)
#   zsh spikes/s22-preview-player/build.sh --remove   takes it off the system again
set -euo pipefail
ROOT="$PWD"; SRC="$ROOT/spikes/s22-preview-player"; APP="$ROOT/target/s22/S22Host.app"; C="$APP/Contents"; X="$C/PlugIns/S22Preview.appex"
LSREG=/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister
reset_ql() { qlmanage -r > /dev/null 2>&1 || true; killall QuickLookUIService QuickLookSatellite 2> /dev/null || true; }
if [ "${1:-}" = "--remove" ]; then pluginkit -r "$X" 2> /dev/null || true; [ -d "$APP" ] && $LSREG -u "$APP" || true; rm -rf "$APP"; reset_ql; echo removed; exit 0; fi
ID="$(security find-identity -v -p codesigning | sed -n 's/.*"\(Developer ID Application: .*\)".*/\1/p' | head -1)"; [ -n "$ID" ] || { echo "no Developer ID identity" >&2; exit 1; }
FWSRC="$ROOT/target/dist/qcview.app/Contents/Frameworks"; [ -f "$FWSRC/libavcodec.63.dylib" ] || { echo "no staged bundle: run packaging/macos/make-app.sh once" >&2; exit 1; }
cargo build --release -p s22-preview-player
pluginkit -r "$X" 2> /dev/null || true; rm -rf "$APP"; mkdir -p "$C/MacOS" "$C/Frameworks" "$X/Contents/MacOS" "$ROOT/target/s22/obj"
cp "$FWSRC"/libOpenEXRCore-*.dylib "$FWSRC"/libImath-*.dylib "$FWSRC"/lib{av,sw}*.dylib "$C/Frameworks/"
print 'print("S22 host: nothing to do")' > "$ROOT/target/s22/obj/Host.swift"; xcrun swiftc -O "$ROOT/target/s22/obj/Host.swift" -o "$C/MacOS/S22Host"
python3 - "$C" <<'PY'
import plistlib, sys, pathlib
c = pathlib.Path(sys.argv[1]); base = {"CFBundleInfoDictionaryVersion": "6.0", "CFBundleShortVersionString": "0.1", "CFBundleVersion": "1", "LSMinimumSystemVersion": "13.0"}
plistlib.dump(dict(base, CFBundleExecutable="S22Host", CFBundleIdentifier="app.qcview.spike.s22", CFBundleName="S22Host", CFBundlePackageType="APPL", LSUIElement=True), open(c / "Info.plist", "wb"))
plistlib.dump(dict(base, CFBundleExecutable="S22Preview", CFBundleIdentifier="app.qcview.spike.s22.preview", CFBundleName="S22Preview", CFBundleDisplayName="S22 Preview", CFBundlePackageType="XPC!",
    NSExtension={"NSExtensionPointIdentifier": "com.apple.quicklook.preview", "NSExtensionPrincipalClass": "S22Preview",
                 "NSExtensionAttributes": {"QLSupportedContentTypes": ["org.smpte.mxf"], "QLIsDataBasedPreview": False, "QLSupportsSearchableItems": False}}), open(c / "PlugIns/S22Preview.appex/Contents/Info.plist", "wb"))
PY
xcrun swiftc -O -parse-as-library -application-extension -module-name S22Preview "$SRC/Preview.swift" "$ROOT/target/release/libs22_preview_player.a" "$C"/Frameworks/libOpenEXRCore-*.dylib "$C"/Frameworks/lib{avutil,avformat,avfilter,swscale,swresample,avcodec}.*.dylib \
  -lz -lc++ -liconv -framework ImageIO -framework AudioToolbox -framework CoreAudio -framework AudioUnit -framework CoreData -framework CoreText -framework CoreImage -framework CloudKit -framework QuartzCore -framework CoreVideo -framework Metal \
  -Xlinker -dead_strip -Xlinker -rpath -Xlinker @loader_path/../../../../Frameworks -Xlinker -e -Xlinker _NSExtensionMain -o "$X/Contents/MacOS/S22Preview"
strip -S -x "$X/Contents/MacOS/S22Preview" 2> /dev/null
ENT="$ROOT/target/s22/obj/ext.entitlements"
print '<?xml version="1.0" encoding="UTF-8"?><plist version="1.0"><dict><key>com.apple.security.app-sandbox</key><true/><key>com.apple.security.files.user-selected.read-only</key><true/></dict></plist>' > "$ENT"
S=(codesign --force --options runtime --timestamp=none --sign "$ID")
"${S[@]}" "$C"/Frameworks/*.dylib 2>&1 | grep -v replacing || true; "${S[@]}" --entitlements "$ENT" "$X" 2>&1 | grep -v replacing || true; "${S[@]}" "$APP" 2>&1 | grep -v replacing || true
$LSREG -f -R -trusted "$APP"; pluginkit -a "$X"; pluginkit -e use -i app.qcview.spike.s22.preview; reset_ql; sleep 1; pluginkit -m -v -i app.qcview.spike.s22.preview | head -1 | cut -c1-70
echo "built $APP ($(du -sh "$X" | cut -f1) extension)"
