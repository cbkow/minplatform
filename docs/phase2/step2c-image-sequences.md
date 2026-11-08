# Step 2c — image sequences (`min-image` + the sequence cache)

Canon rows 34 / 148 (RULED 2026-11-07), 189 (proposed). QCView's sources: `src/decode/{image_loader,exr_image_loader,
png_image_loader,tiff_image_loader,jpeg_image_loader}.{h,cpp}`, `image_sequence_cache.{h,cpp}`.

Pieces: **1** the EXR loader · **1b** PNG / TIFF / JPEG · 2 the sequence cache · 3 transport + presenter ·
4 project / bins / playlists / hover thumbnails · 5 the EXR layer UI.

## Piece 1 — EXR (DONE 2026-11-08, 9fe19a5 … e45fc47)

`crates/min-image/src/exr.rs` binds OpenEXRCore's C API (bindgen; no Imf C++). QCView's rules carried: the part is
chosen by layer name; channels resolve `layer.R/G/B[/A]`, then the bare channels, then the first prefixed layer;
the DISPLAY window with the data window placed inside it, zero fill outside. Chunks (scanline or level-0 tiles)
decode in parallel with `std::thread::scope` (up to 8 threads, per-thread decoders) straight into half-float planes
G, B, R [, A], rows padded to 256 bytes. `layers(path)` lists the layers (step 2c piece 5's picker).

| Check | macOS | Windows (rhombus) |
|---|---|---|
| 4K ZIP EXR decode, one frame | 23 ms debug | 15 ms release |
| vs FFmpeg's `exr` decoder, every half of the frame | 0 of 24.9 M differ (real fixture + synthetic) | 0 of 24.9 M differ (synthetic, generated on the box) |

The comparison must be against `gbrpf16le` — FFmpeg 9's EXR decoder's NATIVE output. Asking FFmpeg for `gbrpf32le`
routes the frame through swscale's f16 input path, which is lossy (≈ 2e-4 everywhere): that looked like a decoder
difference until the whole-frame offset test showed (0,0) as the only match.

QCView declares a 15-KiB "broken file" threshold (`kBrokenFileThresholdBytes`) but never applies it; the port has no
size gate either — a missing file is a gap, a file that fails to decode is broken.

## Piece 1b — PNG / TIFF / JPEG (DONE 2026-11-08, 69862f9)

| Format | Library | Reference | Result (both OSes) |
|---|---|---|---|
| PNG | `png` 0.18 (pure Rust) | FFmpeg's decode in its native format | IDENTICAL: rgb24, rgba, gray, ya8, pal8, monob, rgb48, rgba64, gray16, ya16 |
| TIFF | `tiff` 0.11 (pure Rust) | same | IDENTICAL: rgb24, rgba, gray, ya8, rgb48, rgba64, gray16 × raw / LZW / deflate / PackBits, and planar |
| JPEG | libjpeg-turbo 3 (TurboJPEG) | `djpeg` (libjpeg-turbo, QCView's decoder) | IDENTICAL: 4:2:0, 4:4:4, gray (macOS; the box has no `djpeg`, the decode runs) |

Why JPEG is C: it is lossy, so its pixels are the decoder's IDCT + upsampling. `zune-jpeg` measured 2 code values
off libjpeg-turbo at 4:4:4 / gray and 4 at 4:2:0 (5 % of samples) — a QC viewer that shows different JPEG pixels from
the previous version of itself is wrong. PNG and TIFF are lossless: the pure crates are exact, need no per-OS pin and
are memory-safe on hostile files. The libraries are pinned in `tools/image-libs/README.md`.

QCView's expansions carried: palette → RGB, tRNS → alpha, < 8-bit → 8, gray → three planes, 16-bit kept as U16,
8-bit as U8, JPEG CMYK / YCCK refused. Fixed, with the reason beside the code:
- 16-bit gray PNG (and 1/2/4-bit gray) came out 3-channel under an RGBA16 label in QCView — garbage.
- Float TIFFs (16- and 32-bit) were copied into an integer buffer ("the pixels will look wrong", QCView's comment) →
  F16 planes here.
- TIFF Orientation is honoured at every depth (QCView: 8-bit only); 16-bit gray TIFF reads (QCView refused it).
- A corrupt JPEG reached libjpeg's default `error_exit` in QCView (the error manager never longjmps) — the PROCESS
  exited. TurboJPEG returns an error → a broken frame.
- Palette, YCbCr and Lab TIFFs: the `tiff` crate refuses them (QCView's libtiff RGBA helper read them) → a broken
  frame. Rare in sequences; libtiff joins if a real file shows up.

Owed from QCView's loaders for piece 4: the thumbnail fast paths (PNG skip-downsample, JPEG DCT scaling).
