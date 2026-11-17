#!/bin/sh
# After the jobs were made by UFB itself (shots/00-stage.demo): the made-up media goes into their folders.
#   sh tools/ufb-shots/fill-jobs.sh <folder>
set -e
ROOT="$1"; M="$ROOT/Media"; J="$ROOT/Jobs/2611_Harbor_Lights"
[ -d "$J/3d" ] || { echo "no job at $J - run the staging steps first" >&2; exit 1; }
n=1; for sh in sh010 sh020 sh030 sh040 sh050 sh060; do
  r="$J/3d/$sh/renders"; mkdir -p "$r" "$J/3d/$sh/playblasts"
  cp "$M/Plates/${sh}_plate_"*.mp4 "$J/3d/$sh/playblasts/"
  k=0; for f in "$M"/Stills/*.jpg "$M"/Stills/*.png; do k=$((k + 1)); [ $(( (k + n) % 3 )) = 0 ] || continue; b=$(basename "$f"); cp "$f" "$r/${sh}_${b}"; done
  n=$((n + 1)); done
cp "$M/Plates/sh020_comp_v007.mov" "$J/3d/sh020/renders/"
cp -R "$M/Sequences/sh020_comp" "$J/3d/sh020/renders/"
for d in "$J"/postings/*review*; do cp "$M/Deliveries/harbor_lights_review_v001.mp4" "$M/Documents/review_notes.txt" "$d/"; done
cp "$M/Documents/delivery_notes.md" "$M/Documents/shot_list.csv" "$J/docs/"
echo "filled $J"
