#!/bin/sh
# make-media.sh — the generated test media for the manual's screenshots: colour bars and test patterns only (nothing of a
# client's, nothing with a name that means anything). Written to fixtures/docs-shots/media (gitignored). Needs ffmpeg.
set -e
cd "$(dirname "$0")/../.."
M=fixtures/docs-shots/media; mkdir -p "$M" "$M/Render_v001"
FF="ffmpeg -nostdin -hide_banner -loglevel error -y"
TONE="-f lavfi -i sine=frequency=1000:sample_rate=48000"
A="-c:a pcm_s16le -ac 2 -af volume=-18dB"
[ -f "$M/Color Bars 1080p.mov" ]   || $FF -f lavfi -i "smptehdbars=size=1920x1080:rate=24000/1001" $TONE -t 20 -c:v prores_ks -profile:v 2 -pix_fmt yuv422p10le -color_primaries bt709 -color_trc bt709 -colorspace bt709 -timecode 01:00:00:00 $A "$M/Color Bars 1080p.mov"
[ -f "$M/Test Pattern A.mov" ]     || $FF -f lavfi -i "testsrc2=size=1920x1080:rate=24000/1001" $TONE -t 24 -c:v prores_ks -profile:v 2 -pix_fmt yuv422p10le -color_primaries bt709 -color_trc bt709 -colorspace bt709 -timecode 01:00:00:00 $A "$M/Test Pattern A.mov"
[ -f "$M/Test Pattern B.mov" ]     || $FF -f lavfi -i "testsrc2=size=1920x1080:rate=24000/1001,hue=h=40:s=0.8,eq=brightness=0.04" $TONE -t 24 -c:v prores_ks -profile:v 2 -pix_fmt yuv422p10le -color_primaries bt709 -color_trc bt709 -colorspace bt709 -timecode 01:00:00:00 $A "$M/Test Pattern B.mov"
[ -f "$M/Color Bars UHD.mov" ]     || $FF -f lavfi -i "smptehdbars=size=3840x2160:rate=24000/1001" $TONE -t 10 -c:v prores_ks -profile:v 2 -pix_fmt yuv422p10le -color_primaries bt709 -color_trc bt709 -colorspace bt709 $A "$M/Color Bars UHD.mov"
# a PQ-tagged ramp: grey and colour gradients written as 10-bit HEVC with the HDR10 tags (the code values are the ramp's)
[ -f "$M/Gradient PQ UHD.mp4" ]    || $FF -f lavfi -i "gradients=size=3840x2160:rate=24000/1001:c0=black:c1=white:x0=0:y0=0:x1=3840:y1=0:nb_colors=2" -t 10 -an -c:v hevc_videotoolbox -profile:v main10 -b:v 30M -pix_fmt p010le -color_primaries bt2020 -color_trc smpte2084 -colorspace bt2020nc -tag:v hvc1 "$M/Gradient PQ UHD.mp4"
[ -f "$M/Render_v001/Render_v001.0001.exr" ] || $FF -f lavfi -i "testsrc2=size=1920x1080:rate=24" -frames:v 48 -start_number 1 -c:v exr -pix_fmt gbrpf32le "$M/Render_v001/Render_v001.%04d.exr"
[ -f "$M/Tone 1 kHz.wav" ]         || $FF $TONE -t 20 $A "$M/Tone 1 kHz.wav"
ls -la "$M" | sed 's/^/  /'
