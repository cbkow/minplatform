# One FFmpeg for the family

`ffmpeg.pin` names the FFmpeg every app builds against — the Rust crates through `ffmpeg-sys-next` and the Qt apps
through their CMake `FFMPEG_DIR` — so a machine carries ONE build and every app links the same libraries (canon row 187).

| OS | Source | How the build finds it |
|---|---|---|
| macOS | built from source with the local patches by `build-macos.sh` into `external/ffmpeg/macos-arm64` (gitignored) | the build scripts use that prefix when it exists; pkg-config (a Homebrew `ffmpeg`) otherwise, for development only |
| Windows | BtbN's gpl-shared recipe with the local patches, built by `build-win64-wsl.sh` (or the stock dated asset fetched by `fetch-win64.ps1`) into `external/ffmpeg/win64` (gitignored) | `FFMPEG_DIR=<repo>\external\ffmpeg\win64` for the build; `PATH += <FFMPEG_DIR>\bin` at run time |

The binaries never enter the repository (the repo is public; the Windows zip is 82 MB): the pin and the fetch script do.
The pin names a DATED BtbN release asset (immutable), never the `latest` one, which is re-zipped daily; the script
verifies the sha256.

## Why gpl-shared
`--enable-vulkan --enable-libplacebo` are in that build: `ffmpeg -h decoder=prores` lists `vulkan` as a hardware device
(the ProRes route for Windows, canon row 149), HEVC lists `d3d12va` beside the rest. The macOS build decodes through
VideoToolbox; the Homebrew formula carries it by default.

## Moving the pin
A point release (9.0.x): edit the pin, run the fetch on the Windows box, rebuild, run the headless harness and the scrub
bench. A new major: a ruling first — the API assessment lives in `docs/phase2/step2c-rails-settings.md`
("FFmpeg upgrade + API assessment").


## The local patches (2026-11-08)

`patches/` carries QCView's three FFmpeg patches (`QCView-Player/external/patches/ffmpeg`, GPL like FFmpeg), applied on every
build — a stock build loses them:

| Patch | What breaks without it |
|---|---|
| 0001 DNxHR 444 adaptive colour transform | Avid DNxHR 444 (AE / Media Composer "Avid DNxHR Codec") decodes with green/magenta macroblock speckle; with it the frames come out `gbrp10/12`, tagged limited |
| 0002 MXF RGBA ComponentMin/MaxRef | RGB essence in MXF probes range "unknown" |
| 0003 ProRes RAW Bayer patterns | iPhone 17 Pro ProRes RAW (pattern 3) is refused |

**macOS:** `zsh tools/ffmpeg/build-macos.sh` (n9.0.2 + the patches, shared, VideoToolbox + AudioToolbox + dav1d + libsrt (static libsrt + mbedTLS, built first into `external/ffmpeg/deps-macos-arm64`, versions in the pin; added 2026-11-09 for the live `srt://` input) →
`external/ffmpeg/macos-arm64`, ~3 min). Check: an Avid DNxHR 444 file probes `gbrp12le,tv` with
`external/ffmpeg/macos-arm64/bin/ffprobe` (stock: `yuv444p12le,unknown`).

**Windows:** `tools/ffmpeg/build-win64-wsl.sh` in WSL2 Ubuntu + Docker on rhombus (set up 2026-11-08: the WSL + VM Platform
features, the WSL package, Ubuntu-24.04, docker.io) → `n9.0.2-48-gd4070a14ce` + the patches (~6 min), extracted to
`external/ffmpeg/win64` (the stock tree kept as `win64-stock`). Checked 2026-11-08: the four "Applied patch … cleanly." lines,
Vulkan + libplacebo still on, the GPU ProRes path still zero-copy, FFmpeg's own DNxHR 444 tagged limited (stock: unknown).
The Avid ACT path itself wants an Avid DNxHR 444 file on the box (FFmpeg's encoder never mixes RGB / YCbCr macroblocks).
