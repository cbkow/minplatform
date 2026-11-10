#!/bin/zsh
# The macOS release's native libraries, from source, arm64, minos 13.0, into the gitignored external/deps/macos-arm64
# (canon row 248; QCView-Player's recipes, dependencies.md). Imath, OpenEXR and OpenColorIO are dylibs with absolute ids
# (the bundling step rewrites them to @rpath); libjpeg-turbo (with the TurboJPEG API, which min-image binds) and libtiff
# are static. Nothing is taken from Homebrew: its prefixes are ignored by every configure. The crates' build scripts use
# this prefix when it exists and their environment variable is not set.
# Run from the repo root:  zsh tools/deps/build-macos.sh        (SDKROOT is honoured if set)
set -euo pipefail
ROOT="$PWD"; source "$ROOT/tools/deps/deps.pin"
PREFIX="$ROOT/external/deps/macos-arm64"; SRC="$ROOT/external/deps/src"; NCPU="$(sysctl -n hw.ncpu)"
export MACOSX_DEPLOYMENT_TARGET=13.0
export PKG_CONFIG_LIBDIR="$PREFIX/lib/pkgconfig"; unset PKG_CONFIG_PATH CMAKE_PREFIX_PATH   # pkg-config sees this prefix only
CM=(-G Ninja -DCMAKE_BUILD_TYPE=Release -DCMAKE_INSTALL_PREFIX="$PREFIX" -DCMAKE_PREFIX_PATH="$PREFIX"
    -DCMAKE_OSX_DEPLOYMENT_TARGET=13.0 -DCMAKE_OSX_ARCHITECTURES=arm64
    "-DCMAKE_IGNORE_PREFIX_PATH=/opt/homebrew;/usr/local" -DCMAKE_FIND_FRAMEWORK=NEVER)
SH=(-DBUILD_SHARED_LIBS=ON -DCMAKE_INSTALL_NAME_DIR="$PREFIX/lib")
ST=(-DBUILD_SHARED_LIBS=OFF -DCMAKE_POSITION_INDEPENDENT_CODE=ON)
rm -rf "$PREFIX"; mkdir -p "$SRC"

get() {   # name url tag commit
  local d="$SRC/$1"
  rm -rf "$d"; git clone -q --depth 1 --branch "$3" "$2" "$d" 2> /dev/null
  local head="$(git -C "$d" rev-parse HEAD)"
  if [ -n "$4" ] && [ "$head" != "$4" ]; then echo "$1: tag $3 is $head, the pin says $4" >&2; exit 1; fi
  echo "$1 $3 $head"
}
build() {   # name, then cmake arguments
  local d="$SRC/$1"; shift
  cmake -S "$d" -B "$d/build" "${CM[@]}" "$@" > "$d/configure.log" 2>&1 || { tail -30 "$d/configure.log" >&2; exit 1; }
  cmake --build "$d/build" -j"$NCPU" > "$d/build.log" 2>&1 || { tail -40 "$d/build.log" >&2; exit 1; }
  cmake --install "$d/build" > /dev/null
}

get imath https://github.com/AcademySoftwareFoundation/Imath.git "$IMATH_TAG" "$IMATH_COMMIT"
build imath "${SH[@]}" -DBUILD_TESTING=OFF -DPYTHON=OFF
get openexr https://github.com/AcademySoftwareFoundation/openexr.git "$OPENEXR_TAG" "$OPENEXR_COMMIT"   # libdeflate, OpenJPH and zstd fetched and built in
build openexr "${SH[@]}" -DBUILD_TESTING=OFF -DOPENEXR_BUILD_TOOLS=OFF -DOPENEXR_BUILD_EXAMPLES=OFF -DOPENEXR_INSTALL_EXAMPLES=OFF \
  -DOPENEXR_INSTALL_DOCS=OFF -DOPENEXR_BUILD_PYTHON=OFF -DOPENEXR_FORCE_INTERNAL_DEFLATE=ON -DOPENEXR_FORCE_INTERNAL_OPENJPH=ON -DOPENEXR_FORCE_INTERNAL_ZSTD=ON
get ocio https://github.com/AcademySoftwareFoundation/OpenColorIO.git "$OCIO_TAG" "$OCIO_COMMIT"   # ALL: expat, yaml-cpp, pystring, minizip-ng, zlib and Imath's half fetched and linked in statically (MISSING took Homebrew's yaml-cpp and minizip-ng)
build ocio "${SH[@]}" -DOCIO_INSTALL_EXT_PACKAGES=ALL -DOCIO_BUILD_APPS=OFF -DOCIO_BUILD_TESTS=OFF -DOCIO_BUILD_GPU_TESTS=OFF \
  -DOCIO_BUILD_PYTHON=OFF -DOCIO_BUILD_DOCS=OFF -DOCIO_BUILD_OPENFX=OFF
get jpeg-turbo https://github.com/libjpeg-turbo/libjpeg-turbo.git "$JPEGTURBO_TAG" "$JPEGTURBO_COMMIT"
build jpeg-turbo -DENABLE_SHARED=OFF -DENABLE_STATIC=ON -DWITH_TURBOJPEG=ON -DCMAKE_POSITION_INDEPENDENT_CODE=ON
get tiff https://gitlab.com/libtiff/libtiff.git "$TIFF_TAG" "$TIFF_COMMIT"   # JPEG-in-TIFF from the libjpeg above, zlib from the system
build tiff "${ST[@]}" -Dtiff-tools=OFF -Dtiff-tests=OFF -Dtiff-docs=OFF -Dtiff-contrib=OFF -Dcxx=OFF \
  -Dlzma=OFF -Dzstd=OFF -Dwebp=OFF -Djbig=OFF -Dlerc=OFF -Dlibdeflate=OFF

# the checks: no dylib names Homebrew, every dylib and archive is built for 13.0
bad=0
for f in "$PREFIX"/lib/*.dylib; do
  [ -L "$f" ] && continue
  if otool -L "$f" | grep -qE '/opt/homebrew|/usr/local'; then echo "HOMEBREW in $f" >&2; bad=1; fi
  m="$(vtool -show-build "$f" | awk '$1=="minos"{print $2}')"; [ "$m" = "13.0" ] || { echo "minos $m in $f" >&2; bad=1; }
done
[ -f "$PREFIX/lib/libturbojpeg.a" ] && [ -f "$PREFIX/lib/libjpeg.a" ] && [ -f "$PREFIX/lib/libtiff.a" ] || { echo "a static library is missing" >&2; bad=1; }
[ $bad = 0 ] && echo "installed → $PREFIX (no Homebrew, minos 13.0)" || exit 1
