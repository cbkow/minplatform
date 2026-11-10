#!/bin/sh
# run.sh <script> — one scripted screenshot run (apps/qcv/src/shots.rs): an ISOLATED QCView (its own settings, no single-
# instance hand-off, no inputs card) on the generated media; the PNGs land in fixtures/docs-shots/out. A window appears and
# closes by itself. `MEDIA` in a script stands for the media folder.
set -e
cd "$(dirname "$0")/../.."
S=fixtures/docs-shots; name=$(basename "$1" .shots)
rm -rf "$S/session/$name"; mkdir -p "$S/session/$name/settings" "$S/session/$name/inputs" "$S/session/$name/logs" "$S/out"
sed "s#MEDIA#$PWD/$S/media#g" "$1" > "$S/session/$name/script.shots"
export QCV_MULTI_INSTANCE=1 QCV_SETTINGS_DIR="$PWD/$S/session/$name/settings" QCV_INPUTS_DIR="$PWD/$S/session/$name/inputs" QCV_LOG_DIR="$PWD/$S/session/$name/logs" RUST_LOG=info
./target/release/QCView --shots "$S/session/$name/script.shots" --shots-out "$PWD/$S/out" > "$S/session/$name/run.log" 2>&1 || true
grep -E "^shots: (wrote|no|unknown|this)" "$S/session/$name/run.log" || tail -5 "$S/session/$name/run.log"
