#!/bin/zsh
# P0 fixtures: ONE real source per resolution, every other codec DERIVED from it with ffmpeg so the
# content (entropy) is identical across codecs. Sources are the user's own masters (not in the repo).
# Output: minplatform/fixtures/ (gitignored — the fixtures are derived from CLIENT footage and never leave this machine).
set -e
ROOT=${0:A:h:h:h}; F=$ROOT/fixtures; mkdir -p $F
# The source masters are PRIVATE (client work): their paths live in fixtures/sources.env (gitignored), never here.
[ -f $F/sources.env ] || { echo "missing $F/sources.env (SRC1080, SRC4K, SRCLONG = real ProRes masters: 1080p ~30 s, 4K ~30 s, 1080p ~5 min)"; exit 1; }
source $F/sources.env
FF="ffmpeg -hide_banner -loglevel error -y -nostdin"
t0=$(date +%s)
[ -f $F/prores_1080.mov ] || cp "$SRC1080" $F/prores_1080.mov
[ -f $F/prores_4k.mov ]   || cp "$SRC4K"   $F/prores_4k.mov
[ -e $F/prores_long.mov ] || ln -s "$SRCLONG" $F/prores_long.mov      # 306 s, 5.8 GB: referenced, not copied
# H.264 High 4:2:0 8-bit, VideoToolbox, CBR-ish bitrates typical of delivery
[ -f $F/h264_1080.mp4 ] || eval $FF -i \"$F/prores_1080.mov\" -an -c:v h264_videotoolbox -profile:v high -b:v 20M -pix_fmt yuv420p -g 48 $F/h264_1080.mp4
[ -f $F/h264_4k.mp4 ]   || eval $FF -i \"$F/prores_4k.mov\"   -an -c:v h264_videotoolbox -profile:v high -b:v 60M -pix_fmt yuv420p -g 48 $F/h264_4k.mp4
# HEVC Main10 tagged PQ/BT.2020 (content is SDR pushed through the tags — a DECODE fixture, not an HDR truth)
[ -f $F/hevc10_pq_4k.mp4 ] || eval $FF -i \"$F/prores_4k.mov\" -an -c:v hevc_videotoolbox -profile:v main10 -b:v 50M -pix_fmt p010le -g 48 \
   -color_primaries bt2020 -color_trc smpte2084 -colorspace bt2020nc -tag:v hvc1 $F/hevc10_pq_4k.mp4
# EXR sequence: 72 frames (3 s) of the 4K master, half float, ZIP16 — the common VFX plate shape
if [ ! -f $F/exr_4k/frame_0072.exr ]; then mkdir -p $F/exr_4k
  eval $FF -i \"$F/prores_4k.mov\" -frames:v 72 -pix_fmt gbrpf32le -c:v exr -compression zip16 -format half $F/exr_4k/frame_%04d.exr; fi
echo "fixtures done in $(( $(date +%s) - t0 )) s"; ls -la $F; du -sh $F/exr_4k
for f in $F/*.mov $F/*.mp4; do printf "%-18s " $(basename $f); ffprobe -v error -select_streams v:0 -show_entries stream=codec_name,profile,width,height,pix_fmt,r_frame_rate,color_transfer:format=duration -of compact=p=0:nk=1 "$f" | tr '\n' ' '; echo; done
