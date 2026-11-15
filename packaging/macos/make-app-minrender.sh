#!/bin/zsh
# The macOS bundle of minRender (canon row 427): target/dist/minRender.app with minRender 0.5.x's identity (bundle
# `minRender.app`, executable `minRender`, 0.5.x's bundle id, feed and update key), so 1.0 replaces 0.5.x in place and
# an installed 0.5.11 updates into it. minNotes' make-app-minnotes.sh is the model, step for step: the release binary
# on the vendored libraries, Info.plist and the icon, every non-system dylib copied into Contents/Frameworks (@rpath
# ids; a Homebrew library is REFUSED), Sparkle, the third-party notices, and an ad hoc signature so the bundle runs on
# this machine. The Developer ID signature, notarization and the DMG are sign-macos-minrender.sh's.
# Run from the bundle, with no arguments, the application keeps its data in ~/Library/Application Support/MinRender -
# 0.5.x's folder (row 425). -n NAME builds a TEST bundle "NAME.app" beside it (e.g. -n "minRender Test"): the same
# program, its data in …/MinRender-test, for a look at the bundle without touching the real node's folder.
# Run from the repo root:  zsh packaging/macos/make-app-minrender.sh [-n "minRender Test"]
set -euo pipefail
NAME="minRender"; [ "${1:-}" = "-n" ] && NAME="$2"
ROOT="$PWD"; APP="$ROOT/target/dist/$NAME.app"; C="$APP/Contents"; ICONS="$ROOT/apps/mr/assets/icons"
MACOSX_DEPLOYMENT_TARGET=13.0 cargo build --release -p mr
rm -rf "$APP"; mkdir -p "$C/MacOS" "$C/Resources" "$C/Frameworks"
cp "$ROOT/target/release/minRender" "$C/MacOS/minRender"
cp "$ROOT/LICENSE" "$C/Resources/LICENSE"
cp "$ICONS/minrender.icns" "$C/Resources/minrender.icns"
python3 "$ROOT/packaging/macos/make_plist_minrender.py" "$ROOT" "$C/Info.plist"
plutil -lint "$C/Info.plist" > /dev/null

# the dylibs
EXE="$C/MacOS/minRender"; FW="$C/Frameworks"; typeset -A done_libs
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
for dep in $(deps_of "$EXE"); do copy_dep "$dep"; install_name_tool -change "$dep" "@rpath/${dep:t}" "$EXE" 2> /dev/null; done
for rp in $(otool -l "$EXE" | awk '/LC_RPATH/{f=1} f&&/ path /{print $2; f=0}'); do install_name_tool -delete_rpath "$rp" "$EXE" 2> /dev/null; done   # the build machine's paths
install_name_tool -add_rpath "@executable_path/../Frameworks" "$EXE" 2> /dev/null
# Sparkle (tools/sparkle/fetch.sh): the framework whole, symlinks kept; the application loads it at run time
[ -d "$ROOT/external/sparkle/Sparkle.framework" ] || { echo "run tools/sparkle/fetch.sh first: the release carries its updater" >&2; exit 1; }
ditto "$ROOT/external/sparkle/Sparkle.framework" "$FW/Sparkle.framework"
# the third-party notices: generated for this bundle; it FAILS if a shipped binary has no entry or a crate's licence is
# not an accepted one
python3 "$ROOT/packaging/notices/make_notices.py" macos --app mr --out "$C/Resources/THIRD-PARTY-NOTICES.html" --stage "$APP"
leaks=0
for f in "$EXE" "$FW"/*.dylib(N); do
  for dep in $(deps_of "$f"); do echo "LEAK ${f:t} -> $dep" >&2; leaks=1; done
  m="$(vtool -show-build "$f" | awk '$1=="minos"{print $2}')"; [ "$m" = "13.0" ] || { echo "minos $m: ${f:t}" >&2; leaks=1; }
done
[ $leaks = 0 ] || exit 1
for f in "$FW"/*.dylib(N); do codesign --force -s - "$f" 2> /dev/null; done
codesign --force --deep -s - "$FW/Sparkle.framework" 2> /dev/null; codesign --force -s - "$APP" 2> /dev/null
echo "built $APP - $(ls "$FW" | wc -l | tr -d ' ') libraries, $(du -sh "$APP" | cut -f1)"
