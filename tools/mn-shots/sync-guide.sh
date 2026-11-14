#!/bin/sh
# sync-guide.sh - the User Guide that travels inside minNotes (row 359) is a copy of the manual's pages: the website's
# Markdown (the minNotes repository's docs/, on its 3.0-release branch or main) and its pictures at half size.
#   sh tools/mn-shots/sync-guide.sh [path to the site's docs folder]     (default: ../minNotes-3.0/docs)
# Writes apps/mn/guide/ (pages, images/, STAMP). The application rebuilds its guide whenever STAMP changes.
set -e
cd "$(dirname "$0")/../.."
SRC="${1:-../minNotes-3.0/docs}"; G=apps/mn/guide
[ -f "$SRC/index.md" ] || { echo "no manual at $SRC"; exit 1; }
rm -rf "$G"; mkdir -p "$G/images"
cp "$SRC"/*.md "$G/"
# only the pictures the pages name, at 1280 across (the pictures are taken at 2560)
for n in $(grep -oh "images/mn3-[a-z0-9-]*\.png" "$G"/*.md | sort -u); do
  [ -f "$SRC/$n" ] && ffmpeg -nostdin -hide_banner -loglevel error -y -i "$SRC/$n" -vf "scale='min(1280,iw)':-2" "$G/$n"
done
cat "$G"/*.md "$G"/images/* 2>/dev/null | shasum | cut -c1-16 > "$G/STAMP"
echo "guide: $(ls "$G"/*.md | wc -l | tr -d ' ') pages, $(ls "$G/images" | wc -l | tr -d ' ') pictures, $(du -sh "$G" | cut -f1), stamp $(cat "$G/STAMP")"
