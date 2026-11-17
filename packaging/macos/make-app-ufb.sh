#!/bin/zsh
# The macOS bundle of UFB (canon rows 457, 519, 521, 536), in two kinds:
#   test (the default)  target/dist/UFB Test.app - built with UFB_IDENTITY=test (its own data folders, the test mesh, no
#                       safety belt: apps/ufb/src/identity.rs), its own bundle id and link scheme, no updater: it
#                       installs beside a real UFB 1.4.2 and shares nothing with it.
#   release             target/dist/UFB.app - UFB_IDENTITY=release: 1.4.x's bundle id, link schemes, update feed and
#                       key, its data folder; with Sparkle. It takes 1.4.x's place: NOT to be started on a machine
#                       whose 1.4.x data matters until the owner says so (it opens that data in place).
# minNotes' make-app-minnotes.sh is the model, step for step: the release binary on the vendored libraries, Info.plist
# and the icon, every non-system dylib copied into Contents/Frameworks (@rpath ids; a Homebrew library is REFUSED),
# FFmpeg's program in Contents/Helpers (Transcode to MP4), the built-in project type's skeletons and the third-party
# notices in Contents/Resources, and an ad hoc signature so the bundle runs on this machine. The Developer ID
# signature, notarization and the package are sign-macos-ufb.sh's.
# Run from the repo root:  zsh packaging/macos/make-app-ufb.sh [test|release]
set -euo pipefail
MODE="${1:-test}"
case "$MODE" in test) NAME="UFB Test"; OUT_NAME="ufb-testpkg" ;; release) NAME="UFB"; OUT_NAME="ufb-release" ;; *) echo "usage: make-app-ufb.sh [test|release]" >&2; exit 1 ;; esac
ROOT="$PWD"; APP="$ROOT/target/dist/$NAME.app"; C="$APP/Contents"; ICONS="$ROOT/apps/ufb/assets/icons"; OUT="$ROOT/target/$OUT_NAME"
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
# the release's updater: Sparkle (tools/sparkle/fetch.sh), the framework whole, symlinks kept; loaded at run time
if [ "$MODE" = release ]; then
  [ -d "$ROOT/external/sparkle/Sparkle.framework" ] || { echo "run tools/sparkle/fetch.sh first: the release carries its updater" >&2; exit 1; }
  ditto "$ROOT/external/sparkle/Sparkle.framework" "$FW/Sparkle.framework"
fi
# the third-party notices: generated for this bundle; it FAILS if a shipped binary has no entry or a crate's licence is
# not an accepted one. Settings > About > Licence Notices opens it.
python3 "$ROOT/packaging/notices/make_notices.py" macos --app ufb --out "$C/Resources/THIRD-PARTY-NOTICES.html" --stage "$APP"
leaks=0
for f in "$EXE" "$C/Helpers/ffmpeg" "$FW"/*.dylib; do
  for dep in $(deps_of "$f"); do echo "LEAK ${f:t} -> $dep" >&2; leaks=1; done
  m="$(vtool -show-build "$f" | awk '$1=="minos"{print $2}')"; [ "$m" = "13.0" ] || { echo "minos $m: ${f:t}" >&2; leaks=1; }
done
[ $leaks = 0 ] || exit 1
codesign --force -s - "$FW"/*.dylib "$C/Helpers"/* 2> /dev/null; [ -d "$FW/Sparkle.framework" ] && codesign --force --deep -s - "$FW/Sparkle.framework" 2> /dev/null; codesign --force -s - "$APP" 2> /dev/null
echo "built $APP - $(ls "$FW" | wc -l | tr -d ' ') dylibs, $(du -sh "$APP" | cut -f1)"
