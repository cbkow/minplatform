# minplatform

The source code of four desktop applications for people who work with footage, on Windows and macOS, and of the
Rust + wgpu platform they share.

| Application | What it is | Site and manual | Releases | Source here | Its earlier version |
|---|---|---|---|---|---|
| **QCView 3** | a media player and reviewer | [qcview.app](https://qcview.app) | [QCView-Player](https://github.com/cbkow/QCView-Player/releases) | `apps/qcv`, tags `v3.*` | [`2.x`](https://github.com/cbkow/QCView-Player/tree/2.x) (Qt Quick) |
| **minNotes 3** | a notes editor: pictures, video, PDFs and tables in notes, canvases, exports for the browser | [minnotes.app](https://minnotes.app) | [minNotes](https://github.com/cbkow/minNotes/releases) | `apps/mn`, tags `minnotes-v*` | [`2.x`](https://github.com/cbkow/minNotes/tree/2.x) |
| **minRender 1** | a render-farm coordinator for small teams | [minrender.com](https://minrender.com) | [MinRender](https://github.com/cbkow/MinRender/releases) | `apps/mr`, tags `minrender-v*` | [`0.5.x`](https://github.com/cbkow/MinRender/tree/0.5.x) (C++ / Qt) |
| **UFB 2** | a file browser and project tracker | [ufbrowser.com](https://ufbrowser.com) | [ufb](https://github.com/cbkow/ufb/releases) | `apps/ufb`, tags `ufb-v*` | [`1.4.x`](https://github.com/cbkow/ufb/tree/1.4.x) (C++ / Qt and Rust) |

Windows builds ship through the Microsoft Store; macOS builds are signed, notarized and update themselves. Each
application's own repository holds its site, its manual and its macOS update feed; the code is here.

Each is a rewrite of its earlier version on one renderer ([wgpu](https://wgpu.rs): Metal on macOS, Direct3D 12 on
Windows) and one interface toolkit of our own. Each opens what its earlier version wrote - projects, notes, a farm,
a metadata database - and installs over it.

## What is here

```
apps/qcv/       QCView 3 (the binary is `QCView`)
apps/mn/        minNotes 3 (`minNotes`)
apps/mr/        minRender 1 (`minRender`: node, leader, window, tray and supervisor in one executable)
apps/ufb/       UFB 2 (`UFB`)
crates/         the platform, shared: decoding (min-ffmpeg, min-image, min-pdf, qcv-mxf), presenting (min-present),
                the interface (min-ui, min-theme, min-i18n), drag and drop (min-dnd), running FFmpeg (min-transcode),
                settings and paths (min-settings, min-paths);
                QCView's own and what the others borrow of it: colour (qcv-color), the player, timeline, project,
                notes, annotations and metadata (qcv-*), the QCBridge rings (qcv-ring);
                minNotes' document, store, layout and editing (mn-*); UFB's project types (ufb-projtype)
packaging/      the macOS bundles, the Windows MSIX packages, and the third-party notices' generator
tools/          the native libraries' build scripts and version pins (deps/, ffmpeg/, sparkle/), translations (i18n/),
                scripted screenshots (docs-shots/, mn-shots/, ufb-shots/)
vendor/         byte-exact reference copies of the contracts shared with the companion apps (stamped)
docs/           the design record: docs/rust-wgpu-port.md, whose Decisions tables date every ruling; docs/phase2/
                (QCView), phase3/ (minNotes), phase4/ (minRender), phase5/ (UFB) for specs and handoffs;
                docs/spikes/ for the measured verdicts
spikes/         the experiments those verdicts came from
```

## Building

The instructions below build QCView. The other three build the same way, from the same native libraries:

| | Build | macOS bundle | Windows package |
|---|---|---|---|
| QCView | `cargo build --release -p qcv` | `make-app.sh`, `sign-macos.sh` | `make-msix.ps1` |
| minNotes | `cargo build --release -p mn` | `make-app-minnotes.sh`, `sign-macos-minnotes.sh` | `make-msix-minnotes.ps1` |
| minRender | `cargo build --release -p mr` | `make-app-minrender.sh`, `sign-macos-minrender.sh` | `make-msix-minrender.ps1` |
| UFB | `cargo build --release -p ufb` | `make-app-ufb.sh`, `sign-macos-ufb.sh` | `make-msix-ufb.ps1` |

(the scripts are in `packaging/macos` and `packaging/windows`.)

## Building QCView

Rust 1.98 or later (`rustup`), plus the native libraries QCView links: FFmpeg, OpenColorIO, OpenEXR (with Imath),
libjpeg-turbo and libtiff. Their versions are pinned in `tools/deps/deps.pin`, `tools/deps/vcpkg-windows.pin` and
`tools/ffmpeg/ffmpeg.pin`. Nothing binary is in the repository: the scripts fetch sources and build into `external/`
(gitignored).

### macOS (Apple Silicon, macOS 13 or later)

Xcode's command-line tools, CMake, Ninja, Meson and pkg-config are needed by the library builds.

```sh
zsh tools/deps/build-macos.sh        # Imath, OpenEXR, OpenColorIO, libjpeg-turbo, libtiff → external/deps/macos-arm64
zsh tools/ffmpeg/build-macos.sh      # FFmpeg with QCView's three patches (tools/ffmpeg/patches) → external/ffmpeg/macos-arm64
cargo build --release -p qcv         # → target/release/QCView
```

The app bundle, as released:

```sh
zsh tools/sparkle/fetch.sh           # Sparkle, the updater (external/sparkle)
zsh packaging/macos/make-app.sh      # → target/dist/qcview.app, signed ad hoc: it runs on the machine that built it
```

`packaging/macos/sign-macos.sh` makes the signed and notarized DMG; it needs a Developer ID and a notary profile of
your own (`CODESIGN_TEAM_ID`, `NOTARY_PROFILE`).

### Windows (x64)

Visual Studio 2022 (the C++ workload) and [vcpkg](https://github.com/microsoft/vcpkg) at the commit named in
`tools/deps/vcpkg-windows.pin`:

```bat
C:\vcpkg\vcpkg.exe install opencolorio openexr libjpeg-turbo tiff --triplet x64-windows
```

FFmpeg: `tools/ffmpeg/build-win64-wsl.sh` builds the patched FFmpeg, and `tools/ffmpeg/README.md` describes the pin.
Point the build at both, and put their DLL folders on `PATH` to run the result:

```bat
set FFMPEG_DIR=<repo>\external\ffmpeg\win64
set OCIO_ROOT=C:\vcpkg\installed\x64-windows
set OPENEXR_DIR=%OCIO_ROOT%
set TIFF_DIR=%OCIO_ROOT%
set TURBOJPEG_DIR=%OCIO_ROOT%
cargo build --release -p qcv
```

`packaging\windows\make-msix.ps1` stages the build with every DLL it imports and packs the MSIX (unsigned, as the
Microsoft Store takes it).

### Tests

```sh
cargo test --release --workspace
```

Tests that need media take their paths from `fixtures/sources.env` or the command line and skip when it is absent;
`fixtures/` is not in the repository.

## Third-party notices

Each application shows them (QCView under **About ▸ Licenses…**, the others in their settings or About). They are generated by `packaging/notices/make_notices.py` from the
lock file and `packaging/notices/components.json`; the texts are in `packaging/notices/licenses/`.

## Licence

**GPL-3.0-or-later** (`LICENSE`), with the exceptions listed in [`LICENSE.md`](LICENSE.md): `crates/min-paths` and
`crates/min-paths-ffi` are MIT OR Apache-2.0, and the ring contract under `vendor/qcbae` is MIT OR GPL-3.0.

Copyright © 2025–2026 cbkow.
