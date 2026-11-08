# The C image libraries `min-image` links (canon row 148)

`min-image` binds two C libraries; PNG and TIFF are pure Rust (`png`, `tiff` crates — lossless formats, bit-identical
to FFmpeg's decode on every layout the tests cover).

| Library | Why C | macOS | Windows |
|---|---|---|---|
| OpenEXR 3.5.x, the C core (`openexr.h`, `OpenEXRCore`) | multi-part, every compression, the parallel chunk decode the 4K EXR workflow needs (row 148) | Homebrew `openexr` (3.5.3 on 2026-11-08); pkg-config `OpenEXR` | vcpkg `openexr:x64-windows` → `OPENEXR_DIR=C:\vcpkg\installed\x64-windows` |
| libjpeg-turbo 3.x, the TurboJPEG 3 API (`turbojpeg.h`, `tj3*`) | JPEG is lossy: the pixels ARE the decoder's IDCT + upsampling, and QCView's are libjpeg-turbo's (the pure-Rust decoder measured 2–4 code values off) | Homebrew `jpeg-turbo` (3.2.1); pkg-config `libturbojpeg` | vcpkg `libjpeg-turbo:x64-windows` → `TURBOJPEG_DIR=C:\vcpkg\installed\x64-windows` |

At run time on Windows the prefix's `bin` (OpenEXRCore-3_5.dll, deflate, turbojpeg.dll) goes on PATH.
Moving a version: a point release is free; a minor bump re-runs `cargo test -p min-image` on both machines (the EXR
comparison against FFmpeg and the JPEG comparison against `djpeg` are bit-exact).
