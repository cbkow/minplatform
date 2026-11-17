#!/bin/sh
# UFB's pictures, taken by the application itself (UFB_DEMO + UFB_SHOT), on made-up jobs and media:
#   sh tools/ufb-shots/run.sh <the UFB executable> <folder> <out folder> [stage]
# `stage` first makes the media, lets UFB make the jobs and their metadata, and fills them; without it the pictures
# alone are taken again. (`side:3` in each closes the Places section: a machine's own drives are not for a picture.) Settings, the database and the thumbnails live under <out folder>/state - never the user's.
set -e
EXE="$1"; ROOT="$2"; OUT="$3"; HERE="$(cd "$(dirname "$0")" && pwd)"
[ -n "$OUT" ] || { echo "usage: run.sh <UFB> <folder> <out> [stage]" >&2; exit 1; }
mkdir -p "$OUT"; export UFB_SETTINGS_DIR="$OUT/state/settings" UFB_THUMB_CACHE="$OUT/state/cache" UFB_SIZE="${UFB_SIZE:-1920x1080}" UFB_NO_SYNC=1
steps() { tr -d '\r' < "$1" | sed "s|@ROOT@|$ROOT|g" | tr '\n' ';'; }
fav() { "$EXE" "$ROOT/Jobs" --jobs "$ROOT/Jobs" --favorite "$ROOT/Media/Stills" --favorite "$ROOT/Media/Plates" --favorite "$ROOT/Media/Sequences" --favorite "$ROOT/Media/Documents" --favorite "$ROOT/Media/Deliveries" 2>> "$OUT/state/log.txt"; }
if [ "${4:-}" = stage ]; then
  rm -rf "$OUT/state" "$ROOT/Jobs" "$ROOT/Media/Deliveries/MP4" "$ROOT/Media/Deliveries/stills_2026-11-16"; mkdir -p "$ROOT/Jobs" "$OUT/state"
  [ -d "$ROOT/Media" ] || sh "$HERE/make-media.sh" "$ROOT"      # (slow: made once; delete the folder to make it anew)
  UFB_DEMO="$(steps "$HERE/shots/00-stage.demo")" UFB_SHOT="$OUT/state/stage.png" fav
  sh "$HERE/fill-jobs.sh" "$ROOT"
fi
rm -rf "$ROOT/Media/Deliveries/MP4" "$ROOT/Media/Deliveries/stills_2026-11-16"      # (what the tasks' picture made last time)
for f in "$HERE"/shots/[1-9]*.demo; do n="$(basename "$f" .demo)"; UFB_DEMO="$(steps "$f")" UFB_SHOT="$OUT/$n.png" fav; echo "$n"; done
