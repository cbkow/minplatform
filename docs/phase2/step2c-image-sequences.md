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

## Piece 2 — the sequence cache (DONE 2026-11-08, 5bc6b83)

`crates/qcv-player/src/image_sequence_cache.rs` = QCView's `ImageSequenceCache` (old QCView's `DirectEXRCache`): the
10-ms cache thread (window eviction with loop / range modulo math; queue = playhead first, then ahead, then behind at
stride 1; ≤ 32 requests; batches of max(4, ahead / 2)), the IO thread starting one load thread per frame under the
thread cap (the seek target may take one more, and decodes with 8 internal threads), generation discard, gap (1×1,
transparent) / broken sentinels, the failed set with the seek amnesty, GPU push + evict sinks, loop-seam suppression.
Settings: `performance/imageSeqThreads` (1–64, default 16), `performance/imageSeqReadAhead` (24–240, default 72;
read-behind = max(12, ahead / 6)). Divergences: completions wake the IO loop (QCView polled on 10 ms), one shared
broken sentinel (QCView allocated a full-size one per frame and costed it at 4 bytes), QCView's write-only overrun
flags not carried.

| 4K EXR (ZIP, 72 frames) | macOS | Windows (synthetic, on the box) |
|---|---|---|
| seek target, nothing else in flight | 8.8 ms | 12.3 ms |
| window fill | ~258 fps | ~269 fps |

## Piece 3 — the transport + presenter (DONE 2026-11-08, d962347 …)

A slot's decoder is `SlotDecoder::{Video, Sequence}` (`sequence_decoder.rs`); the transport's calls map onto QCView's
sequence mode: a user seek = a forced playhead (the target lands when cached — a hole lands too, holding the picture),
a drag = an unforced playhead (QCView: forcing per move starved the IO pool), playback pulls each frame once when
cached and HOLDS the last good one on a miss / gap / broken frame (QCView's renderer never sees sentinels). Formats
`min-image` does not read (DPX, TGA, BMP) stay on FFmpeg's image2; `QCV_SEQ_FFMPEG=1` forces that route for A/B.
The presenter reads the cached planes in place (`min_present::Plane` bytes are shared, no 50-MB copy per 4K frame).
The cache wraps with the loop toggle (from the open — QCView only on a toggle) and fills only [in, out] when a range
is set. The scrubber draws the cache underline (wrapping at the edges / the range) and failed-frame ticks, repainted
when the cache's status changes (QCView polls at 33 ms).

| 4K EXR through the transport | macOS | Windows |
|---|---|---|
| frame 30, cache vs FFmpeg image2 | snapshots byte-identical (encoded + display) | — |
| headless play, 72 frames | first frame 23 ms, show p50 9.9 ms, 1 dropped | first frame 46 ms, show p50 3.8 ms, 0 dropped |
| random seek, window 24 — cold / cached | p50 76 / 11 ms | p50 81–92 / 4–11 ms |
| scrub sweep 0→71 at 16 ms | 0 frames behind the pointer | 0 (max 2) |

Cold seeks pay for the previous window's loads still decoding beside the target (QCView cannot cancel them either).
A crash at exit on Windows in the new bench was the bench dropping the engine before the hardware devices built on its
D3D12 device (the rule from the slow-close fix); fixed in the bench.

Open for the user / later pieces:
- EXR in bypass: QCView's SDR swapchain shows linear EXR values AS CODE VALUES; ours tags the frames Linear and the
  engine shows them as light on the linear surface. Part of the banked colour question (§18.6) — not changed.
- The upload runs on the presenter thread (show ≈ 4–10 ms at 4K half); QCView has a GPU upload thread fed by the push
  sink — carry it if playback needs it.
- Piece 4: seed the detector's missing frames at open (the ticks before any IO), sequences in playlists (the cache
  window per clip), hover thumbnails, the bins; piece 5: the EXR layer picker.

## The walk + pieces 4 / 5 (2026-11-08)

- Sequence items load (Add Media, drops, the command line) — they had been selected only.
- The Image Sequence card (QCView `ImageSequenceInspector`): frame-rate chips (retune live, the item keeps the rate), Transfer,
  Range, Pattern, Resolution, Compression (EXR), Duration, cache stride 1x–4x (off-grid frames show a grid neighbour — QCView's
  `getClosestFrame`, kept to stride > 1; at 1x the picture holds until the target lands), cache ahead / behind. The video cards
  hide for a sequence (QCView's `videoLoaded`).
- Hover thumbnails: QCView's per-frame-file keys + the still loaders (`ImageThumbLoader`), the sequence cache peeked first (ours);
  EXR thumbnails get the sRGB curve to match the viewport (QCView: linear as code values — §18.6). The thumbnail fast paths
  (EXR scanline skip, PNG skip-downsample, JPEG DCT scaling) are still owed.
- The detector's gaps seed the failed set at open (QCView `startImageSequence`); playlist clips open from the item's directory +
  pattern (they opened the first frame's path). Owed: the seed for playlist sequence clips, a playlist clip's EXR layer.
- Loop fixes (the user: "after a loop the grey overwrites" the red ticks): a sequence's wrap is QCView's unforced frame advance,
  not a seek; the seam uses [in, out] when set (QCView: every range wrap was a seek); the amnesty evicts cached sentinels so
  failed frames really are retried and re-flag (QCView's comment promised it, its code kept them cached).
- Piece 5, the EXR layers: `min_image::exr::layers` = QCView `discoverLayers` verbatim (part names, "default (part N)", single-part
  prefixes before the LAST dot, "default" for bare channels, cryptomatte filtered), `compression` = QCView's names; the project
  takes them at add (`set_exr_import_metadata`, the first layer current), the cache opens at the item's layer, the card's LAYER
  grid = QCView's tiles (first frame per layer, two columns) with the selection as a FILL one grey step up (the user 2026-11-08,
  not QCView's accent border — blue stays rationed, row 162); a click = `Transport::set_sequence_layer` + the item.

## Formats, thumbnails, the upload thread (2026-11-08, the user's list)

- **Formats** (canon rows 189–191): native DPX (`dpx.rs`, 13 layouts IDENTICAL to FFmpeg), TGA + BMP (the `image` crate, those two
  only; IDENTICAL), and the libtiff fallback for the TIFFs the `tiff` crate refuses (palette = FFmpeg's decode; YCbCr 4:4:4 /
  4:2:2 / 4:2:0 / 4:1:1 = libtiff's `tiff2rgba`). DPX / TGA / BMP joined the image table, so they detect as sequences.
- **Playlist sequence clips** carry the item's EXR layer and gaps on disk; the decoder's seed survives the open's positioning seek.
- **Thumbnail fast paths** (`min_image::load_thumbnail`, QCView's `loadThumbnail`): 4K stills, per thumbnail — DPX 41 → 0.8 ms
  (only the sampled rows are read), JPEG 16 → 6 ms (DCT scaling 1/8), striped TIFF 28 → 19 ms, PNG 40 → 31 ms (rows still
  inflate), EXR unchanged (29 ms: ZIP / DWA chunks hold every sampled row — QCView's scanline skip only pays on 1-line chunks).
  Tested: the row-skipping paths hold exactly the full decode's samples.
- **The GPU upload thread** (QCView's, sized down): `min_present::engine::Uploader` (device + queue clones) and `Pixels::Gpu`;
  the sequence decoder starts a thread on the first PLAY (at the open it delayed the first frame's decode, median 104 vs 32 ms)
  that copies the next 3 cached frames into a pool of ≤ 10 reused texture sets and submits them itself; a frame that is ready
  arrives bound-only. 4K EXR headless, 3 runs: show p50 6.3 → 2.2 ms, ~65 / 70 frames arrive uploaded, frame 30 encoded- and
  display-identical, drops unchanged. Open: the worst show is higher (78–108 vs 35–59 ms), at the play start while the thread's
  first copies overlap the presenter's own — watch it on the walk. `QCV_SEQ_GPU_UPLOAD=0` turns it off.
- **Windows (rhombus, RTX 4090, D3D12), 2026-11-08** at c6b3823: build clean; min-image / qcv-player / qcv-project / qcv-timeline /
  min-present tests green. 4K EXR, 3 runs: show p50 5.1 → 2.2 ms with upload-ahead (67–68 / 71 frames uploaded ahead), max
  ~21 ms either way, 0 dropped — the play-start spike seen on the Mac does not show here; frame 30 encoded- and
  display-identical with and without. Gapped 4K EXR plays (3 drops = the 3 bad frames held). 10-bit DPX through the cache
  encoded-identical to FFmpeg's image2 route; TGA plays. Cold seeks p50 90 ms, cached 8.5 ms; the scrub keeps up (max 1 behind).
  Recipe note: FFmpeg run over ssh waits on the console at exit — `-nostdin` on every call AND `ssh -n` (the Rust tests spawn
  FFmpeg too); the first run sat there for an hour after writing its last frame.
