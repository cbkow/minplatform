#!/bin/zsh
# The macOS bundle of minNotes (canon row 347): target/dist/minNotes.app with minNotes 2.x's identity (bundle
# `minNotes.app`, executable `minNotes`, id app.minnotes), so 3.0 replaces 2.x in place. QCView's make-app.sh is the
# model, step for step: the release binary on the vendored libraries, Info.plist and the icons, every non-system dylib
# copied into Contents/Frameworks (@rpath ids; a Homebrew library is REFUSED), FFmpeg's program in Contents/Helpers
# (the web folder's videos are made with it - `transcode.rs`), and an ad hoc signature so the bundle runs on this
# machine. The Developer ID signature, notarization and the DMG are the user's (sign-macos.sh's way).
# NOT in the bundle yet: Sparkle (the application has no updater of its own yet) and the third-party notices.
# Run from the repo root:  zsh packaging/macos/make-app-minnotes.sh
set -euo pipefail
ROOT="$PWD"; APP="$ROOT/target/dist/minNotes.app"; C="$APP/Contents"; ICONS="$ROOT/apps/mn/assets/icons"
[ -f "$ROOT/external/ffmpeg/macos-arm64/bin/ffmpeg" ] || { echo "run tools/ffmpeg/build-macos.sh first: the release carries the family's FFmpeg" >&2; exit 1; }
MACOSX_DEPLOYMENT_TARGET=13.0 cargo build --release -p mn
rm -rf "$APP"; mkdir -p "$C/MacOS" "$C/Resources" "$C/Frameworks" "$C/Helpers"
cp "$ROOT/target/release/minNotes" "$C/MacOS/minNotes"
cp "$ROOT/LICENSE" "$C/Resources/LICENSE" 2> /dev/null || echo "note: no LICENSE at the repo root"

# the icons: minnotes.icns from the 1024 master (macOS 13-25), Assets.car from the layered .icon (macOS 26; needs full Xcode)
SET="$ROOT/target/dist/minnotes.iconset"; rm -rf "$SET"; mkdir -p "$SET"
for s in 16 32 128 256 512; do
  sips -z $s $s "$ICONS/minNotesMacOS.png" --out "$SET/icon_${s}x${s}.png" > /dev/null
  sips -z $((s*2)) $((s*2)) "$ICONS/minNotesMacOS.png" --out "$SET/icon_${s}x${s}@2x.png" > /dev/null
done
iconutil -c icns "$SET" -o "$C/Resources/minnotes.icns"; rm -rf "$SET"
ICON_ARG=()
if xcrun --find actool > /dev/null 2>&1; then
  CAR="$ROOT/target/dist/mn-icon-assets"; rm -rf "$CAR"; mkdir -p "$CAR"
  xcrun actool "$ICONS/minNotesMacOS.icon" --compile "$CAR" --output-format human-readable-text --errors \
    --output-partial-info-plist "$CAR/partial.plist" --app-icon minNotesMacOS --include-all-app-icons \
    --enable-on-demand-resources NO --development-region en --target-device mac --minimum-deployment-target 13.0 --platform macosx > /dev/null || true
  [ -f "$CAR/Assets.car" ] && { cp "$CAR/Assets.car" "$C/Resources/Assets.car"; ICON_ARG=(--icon-name minNotesMacOS); }
  rm -rf "$CAR"
else echo "note: no actool (full Xcode) - the bundle carries the .icns only"; fi
python3 "$ROOT/packaging/macos/make_plist_minnotes.py" "$ROOT" "$C/Info.plist" "${ICON_ARG[@]}"
plutil -lint "$C/Info.plist" > /dev/null

# the dylibs
EXE="$C/MacOS/minNotes"; FW="$C/Frameworks"; typeset -A done_libs
deps_of() { otool -L "$1" | tail -n +2 | awk '{print $1}' | grep -vE '^(/usr/lib/|/System/|@rpath/|@executable_path/|@loader_path/)' || true; }
copy_dep() {
  local src="$1" base="${1:t}" dep
  [ -n "${done_libs[$base]:-}" ] && return 0
  case "$src" in /opt/homebrew/*|/usr/local/*) echo "$base comes from Homebrew ($src) - refusing to bundle it" >&2; exit 1 ;; esac
  cp -f "$src" "$FW/$base"; chmod u+w "$FW/$base"; install_name_tool -id "@rpath/$base" "$FW/$base" 2> /dev/null; done_libs[$base]=1
  for dep in $(deps_of "$src"); do
    [ "${dep:t}" = "$base" ] && continue
    copy_dep "$dep"; install_name_tool -change "$dep" "@rpath/${dep:t}" "$FW/$base" 2> /dev/null
  done
}
cp "$ROOT/external/ffmpeg/macos-arm64/bin/ffmpeg" "$C/Helpers/"; chmod u+w "$C/Helpers"/*
for bin in "$EXE" "$C/Helpers/ffmpeg"; do
  for dep in $(deps_of "$bin"); do copy_dep "$dep"; install_name_tool -change "$dep" "@rpath/${dep:t}" "$bin" 2> /dev/null; done
  for rp in $(otool -l "$bin" | awk '/LC_RPATH/{f=1} f&&/ path /{print $2; f=0}'); do install_name_tool -delete_rpath "$rp" "$bin" 2> /dev/null; done   # the build machine's paths
  install_name_tool -add_rpath "@executable_path/../Frameworks" "$bin" 2> /dev/null
done
leaks=0
for f in "$EXE" "$C/Helpers/ffmpeg" "$FW"/*.dylib; do
  for dep in $(deps_of "$f"); do echo "LEAK ${f:t} -> $dep" >&2; leaks=1; done
  m="$(vtool -show-build "$f" | awk '$1=="minos"{print $2}')"; [ "$m" = "13.0" ] || { echo "minos $m: ${f:t}" >&2; leaks=1; }
done
[ $leaks = 0 ] || exit 1
codesign --force -s - "$FW"/*.dylib "$C/Helpers"/* 2> /dev/null; codesign --force -s - "$APP" 2> /dev/null
echo "built $APP - $(ls "$FW" | wc -l | tr -d ' ') dylibs, $(du -sh "$APP" | cut -f1)"
