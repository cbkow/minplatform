#!/bin/sh
# make-media.sh — the generated test media for the manual's screenshots: colour bars and test patterns only (nothing of a
# client's, nothing with a name that means anything). Written to "/Users/Shared/QCView Demo/Media" (or $QCV_DEMO_MEDIA): a
# neutral folder, because the Inspector shows a file's folder and the app resolves symlinks. Needs ffmpeg.
set -e
cd "$(dirname "$0")/../.."
M="${QCV_DEMO_MEDIA:-/Users/Shared/QCView Demo/Media}"; mkdir -p "$M" "$M/Render_v001"
FF="ffmpeg -nostdin -hide_banner -loglevel error -y"
TONE="-f lavfi -i sine=frequency=1000:sample_rate=48000"
A="-c:a pcm_s16le -ac 2 -af volume=-18dB"
[ -f "$M/Color Bars 1080p.mov" ]   || $FF -f lavfi -i "smptehdbars=size=1920x1080:rate=24000/1001" $TONE -t 20 -c:v prores_ks -profile:v 2 -pix_fmt yuv422p10le -color_primaries bt709 -color_trc bt709 -colorspace bt709 -timecode 01:00:00:00 $A "$M/Color Bars 1080p.mov"
[ -f "$M/Test Pattern A.mov" ]     || $FF -f lavfi -i "testsrc2=size=1920x1080:rate=24000/1001" $TONE -t 24 -c:v prores_ks -profile:v 2 -pix_fmt yuv422p10le -color_primaries bt709 -color_trc bt709 -colorspace bt709 -timecode 01:00:00:00 $A "$M/Test Pattern A.mov"
[ -f "$M/Test Pattern B.mov" ]     || $FF -f lavfi -i "testsrc2=size=1920x1080:rate=24000/1001,hue=h=40:s=0.8,eq=brightness=0.04" $TONE -t 24 -c:v prores_ks -profile:v 2 -pix_fmt yuv422p10le -color_primaries bt709 -color_trc bt709 -colorspace bt709 -timecode 01:00:00:00 $A "$M/Test Pattern B.mov"
# a "second version" of pattern A with small changes (a patch moved in, a slight lift), for the Difference mode
[ -f "$M/Test Pattern A v2.mov" ]  || $FF -f lavfi -i "testsrc2=size=1920x1080:rate=24000/1001,drawbox=x=420:y=300:w=260:h=150:color=0x808080@0.35:t=fill,eq=gamma=1.02" $TONE -t 24 -c:v prores_ks -profile:v 2 -pix_fmt yuv422p10le -color_primaries bt709 -color_trc bt709 -colorspace bt709 -timecode 01:00:00:00 $A "$M/Test Pattern A v2.mov"
[ -f "$M/Color Bars UHD.mov" ]     || $FF -f lavfi -i "smptehdbars=size=3840x2160:rate=24000/1001" $TONE -t 10 -c:v prores_ks -profile:v 2 -pix_fmt yuv422p10le -color_primaries bt709 -color_trc bt709 -colorspace bt709 $A "$M/Color Bars UHD.mov"
# a PQ-tagged ramp: grey and colour gradients written as 10-bit HEVC with the HDR10 tags (the code values are the ramp's)
[ -f "$M/Gradient PQ UHD.mp4" ]    || $FF -f lavfi -i "gradients=size=3840x2160:rate=24000/1001:c0=black:c1=white:x0=0:y0=0:x1=3840:y1=0:nb_colors=2" -t 10 -an -c:v hevc_videotoolbox -profile:v main10 -b:v 30M -pix_fmt p010le -color_primaries bt2020 -color_trc smpte2084 -colorspace bt2020nc -bsf:v hevc_metadata=colour_primaries=9:transfer_characteristics=16:matrix_coefficients=9 -tag:v hvc1 "$M/Gradient PQ UHD.mp4"
# a multi-layer EXR sequence: four generated "passes" combined into one multi-part file per frame (the parts are the layers)
if [ ! -f "$M/Render_v001/Render_v001.0001.exr" ]; then
  T="$(mktemp -d)"
  $FF -f lavfi -i "testsrc2=size=1920x1080:rate=24" -frames:v 48 -start_number 1 -c:v exr -pix_fmt gbrpf32le "$T/beauty.%04d.exr"
  $FF -f lavfi -i "testsrc2=size=1920x1080:rate=24" -vf "hue=s=0.35,eq=brightness=-0.08" -frames:v 48 -start_number 1 -c:v exr -pix_fmt gbrpf32le "$T/diffuse.%04d.exr"
  $FF -f lavfi -i "testsrc2=size=1920x1080:rate=24" -vf "hue=s=0,eq=contrast=2.6:brightness=-0.35" -frames:v 48 -start_number 1 -c:v exr -pix_fmt gbrpf32le "$T/specular.%04d.exr"
  $FF -f lavfi -i "gradients=size=1920x1080:rate=24:c0=black:c1=white:x0=0:y0=0:x1=1920:y1=1080:speed=0.02" -frames:v 48 -start_number 1 -c:v exr -pix_fmt gbrpf32le "$T/depth.%04d.exr"
  for n in $(seq -f %04g 1 48); do
    exrmultipart -combine -i "$T/beauty.$n.exr::beauty" "$T/diffuse.$n.exr::diffuse" "$T/specular.$n.exr::specular" "$T/depth.$n.exr::depth" -o "$M/Render_v001/Render_v001.$n.exr" >/dev/null
  done
  rm -rf "$T"
fi
# a title with ALPHA (ProRes 4444): bars keyed out to transparent around a centre block, for the background tiles
[ -f "$M/Title Alpha.mov" ]        || $FF -f lavfi -i "smptehdbars=size=1920x1080:rate=24000/1001,format=yuva444p10le,geq=lum='lum(X,Y)':cb='cb(X,Y)':cr='cr(X,Y)':a='if(between(X,480,1440)*between(Y,270,810),65535,0)'" -t 10 -an -c:v prores_ks -profile:v 4444 -pix_fmt yuva444p10le -alpha_bits 16 "$M/Title Alpha.mov"
# six channels of tone at different levels (5.1), for the Inspector's Mix chips and per-channel meters
[ -f "$M/Surround 5.1.mov" ]       || $FF -f lavfi -i "smptehdbars=size=1920x1080:rate=24000/1001" -f lavfi -i "aevalsrc=0.25*sin(2*PI*440*t)|0.2*sin(2*PI*554*t)|0.3*sin(2*PI*330*t)|0.1*sin(2*PI*60*t)|0.12*sin(2*PI*660*t)|0.08*sin(2*PI*880*t):c=5.1:s=48000" -t 20 -c:v prores_ks -profile:v 2 -pix_fmt yuv422p10le -color_primaries bt709 -color_trc bt709 -colorspace bt709 -timecode 10:00:00:00 -c:a pcm_s24le "$M/Surround 5.1.mov"
[ -f "$M/Tone 1 kHz.wav" ]         || $FF $TONE -t 20 $A "$M/Tone 1 kHz.wav"
ls -la "$M" | sed 's/^/  /'
