#!/bin/zsh
# The family's macOS FFmpeg (canon row 187, revised 2026-11-08): FFmpeg n${FFMPEG_VERSION} from source with the three local
# patches QCView carries (tools/ffmpeg/patches: 0001 DNxHR 444 adaptive colour transform, 0002 MXF RGBA range, 0003 ProRes RAW
# Bayer patterns), shared, into the gitignored external/ffmpeg/macos-arm64. Homebrew's stock build lost the patches (the
# Avid DNxHR 444 speckle, the RGB MXF range). Decoders only beyond FFmpeg's own: dav1d (Homebrew) for software AV1 — the
# player encodes nothing. Run from the repo root:  zsh tools/ffmpeg/build-macos.sh
set -euo pipefail
ROOT="$PWD"; source "$ROOT/tools/ffmpeg/ffmpeg.pin"
SRC="$ROOT/external/ffmpeg/src-$FFMPEG_VERSION"; PREFIX="$ROOT/external/ffmpeg/macos-arm64"
[ -d "$SRC/.git" ] || git clone -q --depth 1 --branch "n$FFMPEG_VERSION" https://github.com/FFmpeg/FFmpeg.git "$SRC"
cd "$SRC"
git checkout -q -- . && git clean -qfdx
for p in "$ROOT"/tools/ffmpeg/patches/*.patch; do git apply --verbose "$p" 2>&1 | grep -E '^Applied patch' ; done
./configure --prefix="$PREFIX" --enable-shared --disable-static --enable-pthreads \
  --enable-gpl --enable-version3 \
  --enable-videotoolbox --enable-audiotoolbox \
  --enable-libdav1d \
  --disable-doc --disable-debug --arch=arm64 \
  --extra-cflags="-mmacosx-version-min=13.0" --extra-ldflags="-mmacosx-version-min=13.0" > "$ROOT/external/ffmpeg/configure-macos.log"
make -j"$(sysctl -n hw.ncpu)" > "$ROOT/external/ffmpeg/make-macos.log" 2>&1
rm -rf "$PREFIX"; make install > /dev/null
echo "installed $(grep -o 'n[0-9.]*' "$PREFIX/include/libavutil/ffversion.h" | head -1) → $PREFIX"
