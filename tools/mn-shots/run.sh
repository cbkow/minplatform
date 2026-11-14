#!/bin/sh
# run.sh - the minNotes manual's pictures, retaken by the application itself (`apps/mn/src/shots.rs`).
#   sh tools/mn-shots/run.sh build        make the media, then the demo collection (from nothing)
#   sh tools/mn-shots/run.sh [NAME...]    run scripts/NAME*.shots on a COPY of the demo collection (all of them without a name)
# Pictures go to $MN_SHOTS_OUT (default: target/mn-shots/out, which git ignores). A window appears while a script
# runs; nothing is sent to the desktop. Settings and sessions are throwaway ones under target/mn-shots/.
set -e
cd "$(dirname "$0")/../.."
export MN_DEMO_DIR="${MN_DEMO_DIR:-/Users/Shared/minNotes Demo}"
W="$PWD/target/mn-shots"; OUT="${MN_SHOTS_OUT:-$W/out}"; mkdir -p "$W" "$OUT"
BIN="${MN_BIN:-target/release/minNotes}"; [ -x "$BIN" ] || cargo build --release -p mn
export MN_NO_AUDIO=1 MN_NO_DIALOGS=1 MN_SESSIONS_DIR="$W/sessions"
if [ "$1" = build ]; then
  sh tools/mn-shots/make-media.sh
  # (the clips' notes are kept beside them, in .qcview: a build starts without them, or every build would add its notes again)
  rm -rf "$MN_DEMO_DIR/Tidewater.mnd" "$MN_DEMO_DIR/minNotes Media" "$MN_DEMO_DIR/Media/.qcview" "$W/sessions"
  "$BIN" "$MN_DEMO_DIR/Tidewater.mnd" --new --shots tools/mn-shots/scripts/000-build.shots --shots-out "$W/build" | grep -E "^(shots: (wrote|no |unknown)|imported|saved)" || true
  exit 0
fi
[ -f "$MN_DEMO_DIR/Tidewater.mnd" ] || { echo "no demo collection: run 'sh tools/mn-shots/run.sh build' first"; exit 1; }
for s in tools/mn-shots/scripts/*.shots; do
  n="$(basename "$s" .shots)"; [ "$n" = 000-build ] && continue
  if [ $# -gt 0 ]; then hit=; for want in "$@"; do case "$n" in "$want"*) hit=1;; esac; done; [ -n "$hit" ] || continue; fi
  # each script works on its own copy, in a neutral folder (the application shows paths)
  rm -rf "$MN_DEMO_DIR/Work" "$W/sessions"; mkdir -p "$MN_DEMO_DIR/Work"
  cp "$MN_DEMO_DIR/Tidewater.mnd" "$MN_DEMO_DIR/Work/Tidewater.mnd"; [ -d "$MN_DEMO_DIR/minNotes Media" ] && cp -R "$MN_DEMO_DIR/minNotes Media" "$MN_DEMO_DIR/Work/"
  echo "== $n"
  if head -1 "$s" | grep -q '^# blank'; then "$BIN" --shots "$s" --shots-out "$OUT" | grep -E "^shots: (wrote|no |unknown)" || true
  else "$BIN" "$MN_DEMO_DIR/Work/Tidewater.mnd" --shots "$s" --shots-out "$OUT" | grep -E "^shots: (wrote|no |unknown)" || true; fi
done
