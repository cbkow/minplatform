#!/bin/zsh
# The macOS bundle of UFB - so far the TEST package alone (canon rows 519 point 1, 521): target/dist/UFB Test.app,
# built with UFB_IDENTITY=test (its own data folders, the test mesh, NO safety belt - see apps/ufb/src/identity.rs),
# with its own bundle id and without link schemes or an updater, so that it installs beside a real UFB 1.4.2 and
# shares nothing with it. minNotes' make-app-minnotes.sh is the model, step for step: the release binary on the
# vendored libraries, Info.plist and the icon, every non-system dylib copied into Contents/Frameworks (@rpath ids; a
# Homebrew library is REFUSED), FFmpeg's program in Contents/Helpers (Transcode to MP4), the built-in project type's
# skeletons in Contents/Resources, and an ad hoc signature so the bundle runs on this machine.
# NOT here yet (the release's): Sparkle, the third-party notices, the Developer ID signature and notarization, the
# transition installer package.
# Run from the repo root:  zsh packaging/macos/make-app-ufb.sh
set -euo pipefail
MODE="test"; NAME="UFB Test"
ROOT="$PWD"; APP="$ROOT/target/dist/$NAME.app"; C="$APP/Contents"; ICONS="$ROOT/apps/ufb/assets/icons"; OUT="$ROOT/target/ufb-testpkg"
[ -f "$ROOT/external/ffmpeg/macos-arm64/bin/ffmpeg" ] || { echo "run tools/ffmpeg/build-macos.sh first: the bundle carries the family's FFmpeg" >&2; exit 1; }
SKEL="$ROOT/apps/ufb/resources/types/studio/skeletons"
[ -d "$SKEL" ] || { echo "the built-in project type's skeletons are not in apps/ufb/resources (they are not in the repository): Add would make empty shots" >&2; exit 1; }
# (its own target folder: the identity is compiled in, and the development build beside it must stay what it is)
UFB_IDENTITY=$MODE CARGO_TARGET_DIR="$OUT" MACOSX_DEPLOYMENT_TARGET=13.0 cargo build --release -p ufb
rm -rf "$APP"; mkdir -p "$C/MacOS" "$C/Resources" "$C/Frameworks" "$C/Helpers"
cp "$OUT/release/UFB" "$C/MacOS/UFB"
cp "$ROOT/LICENSE" "$C/Resources/LICENSE" 2> /dev/null || echo "note: no LICENSE at the repo root"
mkdir -p "$C/Resources/types/studio"; cp -R "$SKEL" "$C/Resources/types/studio/skeletons"

# the icon: ufb.icns from 1.4.2's master (512 px: the largest size is that picture enlarged)
SET="$ROOT/target/dist/ufb.iconset"; rm -rf "$SET"; mkdir -p "$SET"
for s in 16 32 128 256 512; do
  sips -z $s $s "$ICONS/ufb-macos.png" --out "$SET/icon_${s}x${s}.png" > /dev/null
  sips -z $((s*2)) $((s*2)) "$ICONS/ufb-macos.png" --out "$SET/icon_${s}x${s}@2x.png" > /dev/null
done
iconutil -c icns "$SET" -o "$C/Resources/ufb.icns"; rm -rf "$SET"
python3 "$ROOT/packaging/macos/make_plist_ufb.py" "$ROOT" "$C/Info.plist" "$MODE"
plutil -lint "$C/Info.plist" > /dev/null

# the dylibs
EXE="$C/MacOS/UFB"; FW="$C/Frameworks"; typeset -A done_libs
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
