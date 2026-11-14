#!/bin/sh
# make-media.sh - the generated media for the minNotes manual's pictures: test patterns and computed images only
# (nothing of a client's, nothing with a name that means anything). Written to "/Users/Shared/minNotes Demo" (or
# $MN_DEMO_DIR): a neutral folder, because the application shows a file's path. Needs ffmpeg and python3.
set -e
cd "$(dirname "$0")"
M="${MN_DEMO_DIR:-/Users/Shared/minNotes Demo}"; mkdir -p "$M/Media" "$M/Notes"
FF="ffmpeg -nostdin -hide_banner -loglevel error -y"
still() { [ -f "$M/Media/$1" ] || $FF -f lavfi -i "$2" -frames:v 1 "$M/Media/$1"; }
still "Lantern study 01.png" "gradients=size=1600x900:c0=0x1b2a49:c1=0xf2a65a:c2=0xd95d39:nb_colors=3:x0=0:y0=0:x1=1600:y1=900:seed=7"
still "Lantern study 02.png" "gradients=size=1600x900:c0=0x0b3954:c1=0x087e8b:c2=0xbfd7ea:nb_colors=3:x0=1600:y0=0:x1=0:y1=900:seed=11"
[ -f "$M/Media/Harbour palette.png" ] || $FF -f lavfi -i "color=c=0x1b2a49:s=240x600" -f lavfi -i "color=c=0x087e8b:s=240x600" -f lavfi -i "color=c=0xbfd7ea:s=240x600" -f lavfi -i "color=c=0xf2a65a:s=240x600" -f lavfi -i "color=c=0xd95d39:s=240x600" -filter_complex hstack=inputs=5 -frames:v 1 "$M/Media/Harbour palette.png"
still "Texture pass.png"     "mandelbrot=size=1600x900:start_scale=0.012:start_x=-0.743643887:start_y=-0.131825904:maxiter=400,hue=h=200:s=0.7"
still "Layout sketch.png"    "cellauto=size=1200x800:rule=90:random_seed=5,negate,format=gray"
still "Night key.jpg"        "gradients=size=1920x1080:c0=0x03071e:c1=0x370617:c2=0xe85d04:nb_colors=3:x0=0:y0=1080:x1=1920:y1=0:seed=21"
# the review clip: a moving gradient with a counter, 12 s at 24 fps, H.264 with a tone
[ -f "$M/Media/Reel 2 - harbour v003.mp4" ] || $FF -f lavfi -i "gradients=size=1920x1080:rate=24:c0=0x0b3954:c1=0xf2a65a:c2=0x1b2a49:c3=0xd95d39:nb_colors=4:speed=0.012:seed=4" \
  -f lavfi -i "sine=frequency=220:sample_rate=48000" -t 12 -vf "drawbox=x=120:y=760:w=1680:h=4:color=white@0.5:t=fill" \
  -c:v libx264 -preset medium -crf 18 -pix_fmt yuv420p -color_primaries bt709 -color_trc bt709 -colorspace bt709 -c:a aac -b:a 128k -af volume=-24dB "$M/Media/Reel 2 - harbour v003.mp4"
[ -f "$M/Media/Reel 2 - harbour v002.mp4" ] || $FF -f lavfi -i "gradients=size=1920x1080:rate=24:c0=0x087e8b:c1=0xbfd7ea:c2=0x0b3954:nb_colors=3:speed=0.01:seed=9" -t 8 \
  -c:v libx264 -preset medium -crf 18 -pix_fmt yuv420p -color_primaries bt709 -color_trc bt709 -colorspace bt709 -an "$M/Media/Reel 2 - harbour v002.mp4"
[ -f "$M/Media/Temp score.wav" ] || $FF -f lavfi -i "sine=frequency=330:sample_rate=48000" -t 20 -af "tremolo=f=2:d=0.6,volume=-20dB" "$M/Media/Temp score.wav"
[ -f "$M/Media/Delivery spec.pdf" ] || python3 make-pdf.py "$M/Media/Delivery spec.pdf"
[ -f "$M/Media/Budget.xlsx" ] || python3 make-xlsx.py "$M/Media/Budget.xlsx"
# the notes, with the media folder's place written in
E="$(printf %s "$M/Media" | sed "s/ /%20/g")"; for f in notes/*.md; do sed "s|@MEDIA@|$E|g" "$f" > "$M/Notes/$(basename "$f")"; done
echo "media and notes in $M"
