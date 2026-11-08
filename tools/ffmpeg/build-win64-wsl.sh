#!/bin/bash
# The family's Windows FFmpeg (canon row 187, revised 2026-11-08): BtbN's recipe (github.com/BtbN/FFmpeg-Builds) with the three
# local patches injected (QCView-Player dependencies.md §2's recipe). Runs as root inside WSL2 Ubuntu with Docker, on rhombus:
#   wsl -d Ubuntu-24.04 -u root -- bash /mnt/c/.../build-win64-wsl.sh     (keep the session open: WSL kills detached work)
# One-time: apt install docker.io git; git clone --depth 1 https://github.com/BtbN/FFmpeg-Builds /root/FFmpeg-Builds;
# copy tools/ffmpeg/patches/*.patch to /root/FFmpeg-Builds/ffpatches/ and strip the CRLF (sed -i 's/\r$//').
# Output: artifacts/ffmpeg-n9.0.x-…-win64-gpl-shared-9.0.zip — the log must show the four "Applied patch … cleanly." lines.
set -e
cd /root/FFmpeg-Builds
git checkout -q -- build.sh
# 1. the patch loop right after `cd ffmpeg` in the BUILD_SCRIPT heredoc (\$ escaped: it runs inside the container)
sed -i '0,/^    cd ffmpeg$/s//    cd ffmpeg\n    for _p in \/ffpatches\/*.patch; do git apply --verbose "\\$_p" || exit 1; done/' build.sh
# 2. the patches mounted into the build container
sed -i 's#-v "\$PWD/ffbuild":/ffbuild -v "\$BUILD_SCRIPT":/build.sh#-v "$PWD/ffbuild":/ffbuild -v "$PWD/ffpatches":/ffpatches -v "$BUILD_SCRIPT":/build.sh#' build.sh
grep -n 'ffpatches' build.sh
./build.sh win64 gpl-shared 9.0
ls -la artifacts/
