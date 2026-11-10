#!/bin/zsh
# The macOS bundle (canon row 248): target/dist/qcview.app — QCView 2.x's identity (bundle `qcview.app`, executable `qcview`,
# id app.qcview.player), so 3.0 replaces 2.x in place. Builds the release binary on the vendored libraries
# (tools/deps/build-macos.sh, tools/ffmpeg/build-macos.sh), stages it, generates Info.plist and the icons, copies every
# non-system dylib into Contents/Frameworks (QCView's bundle_dylibs.sh: @rpath ids, and it REFUSES a Homebrew library), and
# signs ad hoc so the bundle runs on this machine. The Developer ID signature, notarization and the DMG are sign-macos.sh's.
# Run from the repo root:  zsh packaging/macos/make-app.sh
set -euo pipefail
ROOT="$PWD"; APP="$ROOT/target/dist/qcview.app"; C="$APP/Contents"; ICONS="$ROOT/apps/qcv/assets/icons"
[ -f "$ROOT/external/deps/macos-arm64/lib/libOpenColorIO.dylib" ] || { echo "run tools/deps/build-macos.sh first: the release never links Homebrew" >&2; exit 1; }
MACOSX_DEPLOYMENT_TARGET=13.0 cargo build --release -p qcv
rm -rf "$APP"; mkdir -p "$C/MacOS" "$C/Resources/assets" "$C/Frameworks"
cp "$ROOT/target/release/QCView" "$C/MacOS/qcview"
cp -R "$ROOT/apps/qcv/assets/OCIO" "$C/Resources/assets/OCIO"   # color.rs looks in ../Resources/assets/OCIO
cp "$ROOT/LICENSE" "$C/Resources/LICENSE" 2> /dev/null || echo "note: no LICENSE at the repo root yet (the notices step)"

# the icons: qcview.icns from the 512 master (macOS 13–25), Assets.car from the layered .icon (macOS 26; needs full Xcode)
SET="$ROOT/target/dist/qcview.iconset"; rm -rf "$SET"; mkdir -p "$SET"
for s in 16 32 128 256; do
  sips -z $s $s "$ICONS/qcviewMac.png" --out "$SET/icon_${s}x${s}.png" > /dev/null
  sips -z $((s*2)) $((s*2)) "$ICONS/qcviewMac.png" --out "$SET/icon_${s}x${s}@2x.png" > /dev/null
done
cp "$ICONS/qcviewMac.png" "$SET/icon_512x512.png"
iconutil -c icns "$SET" -o "$C/Resources/qcview.icns"; rm -rf "$SET"
ICON_ARG=()
if xcrun --find actool > /dev/null 2>&1; then
  CAR="$ROOT/target/dist/icon-assets"; rm -rf "$CAR"; mkdir -p "$CAR"
  xcrun actool "$ICONS/qcviewMac.icon" --compile "$CAR" --output-format human-readable-text --errors \
    --output-partial-info-plist "$CAR/partial.plist" --app-icon qcviewMac --include-all-app-icons \
    --enable-on-demand-resources NO --development-region en --target-device mac --minimum-deployment-target 13.0 --platform macosx > /dev/null
  [ -f "$CAR/Assets.car" ] && { cp "$CAR/Assets.car" "$C/Resources/Assets.car"; ICON_ARG=(--icon-name qcviewMac); }
  rm -rf "$CAR"
else echo "note: no actool (full Xcode) — the bundle carries the .icns only"; fi
python3 "$ROOT/packaging/macos/make_plist.py" "$ROOT" "$C/Info.plist" "${ICON_ARG[@]}"
plutil -lint "$C/Info.plist" > /dev/null

# the dylibs
EXE="$C/MacOS/qcview"; FW="$C/Frameworks"; typeset -A done_libs
deps_of() { otool -L "$1" | tail -n +2 | awk '{print $1}' | grep -vE '^(/usr/lib/|/System/|@rpath/|@executable_path/|@loader_path/)' || true; }
copy_dep() {
  local src="$1" base="${1:t}" dep
  [ -n "${done_libs[$base]:-}" ] && return 0
  case "$src" in /opt/homebrew/*|/usr/local/*) echo "$base comes from Homebrew ($src) — refusing to bundle it" >&2; exit 1 ;; esac
  cp -f "$src" "$FW/$base"; chmod u+w "$FW/$base"; install_name_tool -id "@rpath/$base" "$FW/$base" 2> /dev/null; done_libs[$base]=1
  for dep in $(deps_of "$src"); do
    [ "${dep:t}" = "$base" ] && continue
    copy_dep "$dep"; install_name_tool -change "$dep" "@rpath/${dep:t}" "$FW/$base" 2> /dev/null
  done
}
for dep in $(deps_of "$EXE"); do copy_dep "$dep"; install_name_tool -change "$dep" "@rpath/${dep:t}" "$EXE" 2> /dev/null; done
for rp in $(otool -l "$EXE" | awk '/LC_RPATH/{f=1} f&&/ path /{print $2; f=0}'); do install_name_tool -delete_rpath "$rp" "$EXE" 2> /dev/null; done   # the build machine's paths
install_name_tool -add_rpath "@executable_path/../Frameworks" "$EXE" 2> /dev/null
leaks=0
for f in "$EXE" "$FW"/*.dylib; do
  for dep in $(deps_of "$f"); do echo "LEAK ${f:t} -> $dep" >&2; leaks=1; done
  m="$(vtool -show-build "$f" | awk '$1=="minos"{print $2}')"; [ "$m" = "13.0" ] || { echo "minos $m: ${f:t}" >&2; leaks=1; }
done
[ $leaks = 0 ] || exit 1
codesign --force -s - "$FW"/*.dylib 2> /dev/null; codesign --force -s - "$APP" 2> /dev/null
echo "built $APP — $(ls "$FW" | wc -l | tr -d ' ') dylibs, $(du -sh "$APP" | cut -f1)"
