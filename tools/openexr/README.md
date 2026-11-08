# OpenEXR for the family (canon row 148: EXR through the OpenEXRCore C library)

| OS | Source | How `min-image` finds it |
|---|---|---|
| macOS | Homebrew `openexr` 3.5.x (`brew install openexr`; 3.5.3 on 2026-11-08) | pkg-config `OpenEXR`; nothing to set |
| Windows | vcpkg `openexr:x64-windows` 3.5.x (`C:\vcpkg\installed\x64-windows` on rhombus) | `OPENEXR_DIR=<that prefix>` for the build; `PATH += <prefix>\bin` at run time (OpenEXRCore-3_5.dll + deflate) |

The C core (`openexr.h`, `OpenEXRCore`) is the only part bound — no C++ (Imf) API. Moving the version: a point release is
free; a minor bump re-runs `cargo test -p min-image` (the 4K fixture decode + the FFmpeg comparison) on both machines.
