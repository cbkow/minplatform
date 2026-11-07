# One FFmpeg for the family

`ffmpeg.pin` names the FFmpeg every app builds against — the Rust crates through `ffmpeg-sys-next` and the Qt apps
through their CMake `FFMPEG_DIR` — so a machine carries ONE build and every app links the same libraries (canon row 187).

| OS | Source | How the build finds it |
|---|---|---|
| macOS | Homebrew `ffmpeg` at the pinned version (`brew list --versions ffmpeg`) | pkg-config; nothing to set |
| Windows | BtbN's gpl-shared build, fetched by `fetch-win64.ps1` into `external/ffmpeg/win64` (gitignored) | `FFMPEG_DIR=<repo>\external\ffmpeg\win64` for the build; `PATH += <FFMPEG_DIR>\bin` at run time |

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
