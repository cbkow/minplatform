#!/bin/zsh
# P0 decoder floor — app-independent numbers from ffmpeg 9 (the same FFmpeg the Rust spikes will bind):
#   hw  = VideoToolbox decode throughput (fps, -f null), sw = software decode (auto threads),
#   seek = wall time of `-ss T -i F -frames:v 1` (open + keyframe seek + decode to the exact frame), p50/p95 of 8 seeks.
# Usage: tools/p0/decode-floor.sh > fixtures/p0-decode-floor.txt
set -u
ROOT=${0:A:h:h:h}; F=$ROOT/fixtures
bench() { # $1 label, rest = ffmpeg input args
  local lbl=$1; shift
  local out; out=$(ffmpeg -hide_banner -nostdin -benchmark -v info "$@" -an -f null - 2>&1)
  local frames=$(echo "$out" | grep -o 'frame= *[0-9]*' | tail -1 | tr -dc 0-9)
  local rt=$(echo "$out" | grep -o 'rtime=[0-9.]*s' | tail -1 | tr -dc 0-9.)
  local rss=$(echo "$out" | grep -o 'maxrss=[0-9]*' | tail -1 | tr -dc 0-9)
  printf "%-22s frames=%-5s wall=%6.2fs  fps=%7.1f  maxrss=%6.0f MB\n" "$lbl" "$frames" "$rt" "$(echo "$frames/$rt" | bc -l)" "$(echo "$rss/1048576" | bc -l)"
}
seeks() { # $1 label, $2 file, $3 duration
  local lbl=$1 f=$2 d=$3; local ts=(); local i
  for i in 1 2 3 4 5 6 7 8; do
    local t=$(echo "$d * ($i * 0.1137 % 1)" | bc -l)
    local s=$(date +%s.%N); ffmpeg -hide_banner -nostdin -v error -hwaccel videotoolbox -ss $t -i "$f" -frames:v 1 -an -f null - 2>/dev/null; local e=$(date +%s.%N)
    ts+=($(echo "($e - $s)*1000" | bc -l))
  done
  python3 -c "import sys,statistics as st; v=sorted(map(float,sys.argv[1:])); print(f'{sys.argv[0]:<22} seek+decode1 ms: p50={v[len(v)//2]:.0f} p95={v[int(len(v)*0.95)-1]:.0f} min={v[0]:.0f} max={v[-1]:.0f} (n={len(v)})')" "$lbl" $ts 2>/dev/null || python3 - "$lbl" $ts <<'PY'
import sys; v=sorted(map(float,sys.argv[2:])); print(f"{sys.argv[1]:<22} seek+decode1 ms: p50={v[len(v)//2]:.0f} p95={v[-1]:.0f} min={v[0]:.0f} max={v[-1]:.0f} (n={len(v)})")
PY
}
echo "ffmpeg: $(ffmpeg -version | head -1 | cut -d' ' -f3)   machine: $(sysctl -n machdep.cpu.brand_string)   date: $(date +%F)"
for f in prores_1080.mov prores_4k.mov h264_1080.mp4 h264_4k.mp4 hevc10_pq_4k.mp4; do
  bench "$f hw"  -hwaccel videotoolbox -i $F/$f
  bench "$f sw"  -i $F/$f
done
bench "exr_4k zip16 half sw" -framerate 24 -i $F/exr_4k/frame_%04d.exr
echo "--- seeks (VideoToolbox; includes avformat open)"
seeks prores_1080.mov $F/prores_1080.mov 30
seeks prores_4k.mov   $F/prores_4k.mov 28
seeks prores_long.mov $F/prores_long.mov 306
seeks h264_4k.mp4     $F/h264_4k.mp4 28
seeks hevc10_pq_4k.mp4 $F/hevc10_pq_4k.mp4 28
