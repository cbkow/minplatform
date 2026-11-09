#!/bin/zsh
# The family's macOS FFmpeg (canon row 187, revised 2026-11-08): FFmpeg n${FFMPEG_VERSION} from source with the three local
# patches QCView carries (tools/ffmpeg/patches: 0001 DNxHR 444 adaptive colour transform, 0002 MXF RGBA range, 0003 ProRes RAW
# Bayer patterns), shared, into the gitignored external/ffmpeg/macos-arm64. Homebrew's stock build lost the patches (the
# Avid DNxHR 444 speckle, the RGB MXF range). Decoders only beyond FFmpeg's own: dav1d (Homebrew) for software AV1 — the
# player encodes nothing — plus libsrt (static, over static mbedTLS for the passphrase's AES) for the live srt:// input
# (2026-11-09: the first build left it out; QCView's recipe, dependencies.md §2). Run from the repo root:  zsh tools/ffmpeg/build-macos.sh
set -euo pipefail
ROOT="$PWD"; source "$ROOT/tools/ffmpeg/ffmpeg.pin"
SRC="$ROOT/external/ffmpeg/src-$FFMPEG_VERSION"; PREFIX="$ROOT/external/ffmpeg/macos-arm64"
DEPS="$ROOT/external/ffmpeg/deps-macos-arm64"; NCPU="$(sysctl -n hw.ncpu)"
CM=(-DCMAKE_BUILD_TYPE=Release -DCMAKE_INSTALL_PREFIX="$DEPS" -DCMAKE_PREFIX_PATH="$DEPS" -DCMAKE_OSX_DEPLOYMENT_TARGET=13.0 -DCMAKE_OSX_ARCHITECTURES=arm64)
if [ ! -f "$DEPS/lib/libsrt.a" ]; then
  mkdir -p "$ROOT/external/ffmpeg/deps-src" && cd "$ROOT/external/ffmpeg/deps-src"
  [ -d mbedtls ] || { curl -sL -o mbedtls.tar.bz2 "https://github.com/Mbed-TLS/mbedtls/releases/download/mbedtls-$MBEDTLS_VERSION/mbedtls-$MBEDTLS_VERSION.tar.bz2" && tar xjf mbedtls.tar.bz2 && mv "mbedtls-$MBEDTLS_VERSION" mbedtls && rm mbedtls.tar.bz2; }
  cmake -S mbedtls -B mbedtls/build "${CM[@]}" -DENABLE_TESTING=OFF -DENABLE_PROGRAMS=OFF > /dev/null && cmake --build mbedtls/build -j"$NCPU" > /dev/null && cmake --install mbedtls/build > /dev/null
  [ -d srt ] || git clone -q --depth 1 --branch "v$SRT_VERSION" https://github.com/Haivision/srt.git srt
  cmake -S srt -B srt/build "${CM[@]}" -DENABLE_SHARED=OFF -DENABLE_STATIC=ON -DENABLE_APPS=OFF -DUSE_ENCLIB=mbedtls > /dev/null && cmake --build srt/build -j"$NCPU" > /dev/null && cmake --install srt/build > /dev/null
  # FFmpeg's configure calls plain pkg-config (no --static): fold the mbedTLS archives + libc++ into srt.pc's Libs (QCView's fix-up)
  MBED="$DEPS/lib/libmbedtls.a $DEPS/lib/libmbedx509.a $DEPS/lib/libmbedcrypto.a -lc++"
  for pc in "$DEPS"/lib/pkgconfig/srt.pc "$DEPS"/lib/pkgconfig/haisrt.pc; do sed -i '' -e "s|^Libs: \(.*\)$|Libs: \1 $MBED|" -e 's|^Libs.private:.*$|Libs.private:|' "$pc"; done
  printf '#include <srt/srt.h>\nint main(void){ srt_startup(); srt_cleanup(); return 0; }\n' > $ROOT/external/ffmpeg/deps-src/srt-check.c
  cc $ROOT/external/ffmpeg/deps-src/srt-check.c -o $ROOT/external/ffmpeg/deps-src/srt-check $(PKG_CONFIG_PATH="$DEPS/lib/pkgconfig" pkg-config --cflags --libs srt) && $ROOT/external/ffmpeg/deps-src/srt-check && echo "libsrt $SRT_VERSION + mbedTLS $MBEDTLS_VERSION: links and runs"
fi
export PKG_CONFIG_PATH="$DEPS/lib/pkgconfig:${PKG_CONFIG_PATH:-}"
[ -d "$SRC/.git" ] || git clone -q --depth 1 --branch "n$FFMPEG_VERSION" https://github.com/FFmpeg/FFmpeg.git "$SRC"
cd "$SRC"
git checkout -q -- . && git clean -qfdx
for p in "$ROOT"/tools/ffmpeg/patches/*.patch; do git apply --verbose "$p" 2>&1 | grep -E '^Applied patch' ; done
./configure --prefix="$PREFIX" --enable-shared --disable-static --enable-pthreads \
  --enable-gpl --enable-version3 \
  --enable-videotoolbox --enable-audiotoolbox \
  --enable-libdav1d --enable-libsrt \
  --disable-doc --disable-debug --arch=arm64 \
  --extra-cflags="-mmacosx-version-min=13.0" --extra-ldflags="-mmacosx-version-min=13.0" > "$ROOT/external/ffmpeg/configure-macos.log"
make -j"$NCPU" > "$ROOT/external/ffmpeg/make-macos.log" 2>&1
rm -rf "$PREFIX"; make install > /dev/null
echo "installed $(grep -o 'n[0-9.]*' "$PREFIX/include/libavutil/ffversion.h" | head -1) → $PREFIX"
