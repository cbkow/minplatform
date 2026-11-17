#!/bin/sh
# Made-up media for UFB's pictures (the Store's screenshots, the site): everything is generated here by FFmpeg's own
# sources - no footage, no names of anyone's work. Run with the repository's ffmpeg on PATH (on Windows under Git's sh):
#   sh tools/ufb-shots/make-media.sh <folder>      ->  <folder>/Media/{Stills,Plates,Sequences,Documents,Deliveries}
set -e
ROOT="$1"; [ -n "$ROOT" ] || { echo "usage: make-media.sh <folder>" >&2; exit 1; }
FF="${FFMPEG:-ffmpeg}"; M="$ROOT/Media"; rm -rf "$M"; mkdir -p "$M/Stills" "$M/Plates" "$M/Sequences/sh020_comp" "$M/Documents" "$M/Deliveries"
ff() { "$FF" -v error -y "$@"; }
# a picture source: n = which, 1..24 - six palettes in four kinds of gradient, with a vignette and a little grain
pal() { case $(( ($1 - 1) % 6 )) in
  0) echo "c0=0x0b1d3a:c1=0x1f6f8b:c2=0xf2a65a:c3=0xf7e3af:c4=0x12343b:c5=0xe76f51" ;;
  1) echo "c0=0x2b0f3a:c1=0xd9376e:c2=0xffb86b:c3=0x1b1b3a:c4=0x7a4fd6:c5=0xffd6a5" ;;
  2) echo "c0=0x0f2a1d:c1=0x3fa34d:c2=0xd8f3a0:c3=0x12343b:c4=0x0a9396:c5=0xe9d8a6" ;;
  3) echo "c0=0x101820:c1=0x2e6fd9:c2=0x8fd3ff:c3=0xf5f7fa:c4=0x14213d:c5=0x7a4fd6" ;;
  4) echo "c0=0x3a1c0b:c1=0xc7591c:c2=0xf6c453:c3=0x2a1a12:c4=0xbb3e03:c5=0xffe8c2" ;;
  *) echo "c0=0x03071e:c1=0x9d0208:c2=0xffba08:c3=0x370617:c4=0xe85d04:c5=0x22223b" ;;
esac; }
kind() { case $(( ($1 - 1) / 6 % 4 )) in 0) echo linear ;; 1) echo radial ;; 2) echo circular ;; *) echo spiral ;; esac; }
src() { echo "gradients=s=${SIZE:-1920x1080}:nb_colors=6:$(pal $1):type=$(kind $1):seed=$(( $1 * 37 + 5 )):speed=${SPEED:-0.02},vignette=PI/4.5,noise=alls=6:allf=t"; }
# -- stills
i=1; for name in harbor_dawn_v003.jpg harbor_dusk_v002.jpg garden_moss_v001.jpg sky_plate_v004.png kiln_glow_v002.jpg neon_alley_v005.jpg coast_haze_v001.png deep_water_v002.tif tide_radial_v001.jpg paper_blue_v003.png ember_ring_v002.jpg slate_wash_v001.exr \
    kite_field_v002.jpg lantern_row_v001.jpg glass_leaf_v004.png market_steam_v002.jpg pier_fog_v001.jpg moss_ring_v003.jpg dune_spiral_v001.png reef_glow_v002.jpg copper_sun_v001.jpg night_bloom_v003.jpg orchard_v002.png signal_red_v001.jpg; do
  ff -f lavfi -i "$(src $i)" -ss 2 -frames:v 1 "$M/Stills/$name"; i=$((i + 1)); done
# -- plates (short clips)
enc() { if "$FF" -v error -encoders | grep -q libx264; then echo "-c:v libx264 -preset fast -crf 20"; else echo "-c:v h264_videotoolbox -b:v 8M"; fi; }
i=1; for name in sh010_plate_v001 sh020_plate_v001 sh030_plate_v002 sh040_plate_v001 sh050_plate_v003 sh060_plate_v001; do
  SPEED=0.08 ff -f lavfi -i "$(SPEED=0.08 src $((i * 5)))" -f lavfi -i "sine=frequency=$((180 + i * 40)):sample_rate=48000" -t 6 -r 24 $(enc) -pix_fmt yuv420p -c:a aac -b:a 128k -shortest "$M/Plates/$name.mp4"
  i=$((i + 1)); done
ff -f lavfi -i "$(SPEED=0.08 src 14)" -t 4 -r 24 -c:v prores_ks -profile:v 3 -pix_fmt yuv422p10le "$M/Plates/sh020_comp_v007.mov"
# -- an image sequence (shown as one entry)
ff -f lavfi -i "$(SPEED=0.1 src 9)" -frames:v 48 -start_number 1001 "$M/Sequences/sh020_comp/sh020_comp_v007.%04d.png"
# -- documents
printf '# Harbor Lights - delivery notes\n\n- Master: 3840x2160, 24 fps, Rec.709\n- Audio: stereo, 48 kHz\n- Slate: 2 seconds, then picture\n- Naming: <shot>_<task>_v###\n\n## Open\n\n- sh030: sky replacement, second pass\n- sh050: grade notes from the review\n' > "$M/Documents/delivery_notes.md"
printf 'shot,task,frames,status\nsh010,comp,96,approved\nsh020,comp,120,review\nsh030,comp,84,in progress\nsh040,comp,72,todo\n' > "$M/Documents/shot_list.csv"
printf 'Review 2026-11-16\n\nsh010  approved\nsh020  one more pass on the reflections\nsh030  hold for the new plate\n' > "$M/Documents/review_notes.txt"
printf '<!doctype html><meta charset="utf-8"><title>Call sheet</title><body style="font:16px sans-serif;margin:40px;background:#16181c;color:#e8e8e8"><h1>Call sheet</h1><p>Stage 2, 08:00. Bring the grey card.</p></body>' > "$M/Documents/call_sheet.html"
# -- deliveries (an archive to extract)
( cd "$M/Stills" && ( command -v zip > /dev/null 2>&1 && zip -q "../Deliveries/stills_2026-11-16.zip" harbor_dawn_v003.jpg harbor_dusk_v002.jpg garden_moss_v001.jpg || "${SYSTEMROOT:-/c/Windows}/System32/tar.exe" -a -c -f "../Deliveries/stills_2026-11-16.zip" harbor_dawn_v003.jpg harbor_dusk_v002.jpg garden_moss_v001.jpg ) )
cp "$M/Plates/sh010_plate_v001.mp4" "$M/Deliveries/harbor_lights_review_v001.mp4"
# (a large master: its transcode takes long enough to be seen at work)
ff -f lavfi -i "$(SIZE=3840x2160 SPEED=0.06 src 20)" -t 20 -r 24 -c:v prores_ks -profile:v 2 -pix_fmt yuv422p10le "$M/Deliveries/harbor_lights_master_v001.mov"
ff -f lavfi -i "$(SIZE=3840x2160 SPEED=0.06 src 3)" -t 20 -r 24 -c:v prores_ks -profile:v 2 -pix_fmt yuv422p10le "$M/Deliveries/paper_kites_master_v002.mov"
echo "made $M"
