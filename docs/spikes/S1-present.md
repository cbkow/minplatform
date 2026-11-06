# S1 — present: one decoded frame → wgpu → an HDR surface through an OCIO shader

**Question (§21.3):** can a wgpu presenter on Metal and DX12 show a decoded frame on an HDR surface
through OCIO's generated shader (native passthrough; baked LUT as the control) at QCView's numbers?
**Pass bar (vs P0):** open → first frame ≤ 500 ms (4K ProRes: 385); steady present p95 ≤ 20 ms at 60 Hz, no
gap > 1 refresh; GPU ≤ 1 ms p95 per frame at 4K 4:2:2; ≤ 0.25 core; footprint ≤ 1.4 GB at 4K; pixel-diff vs
Qt QCView on the same frame within tolerance; EDR peak + PQ correct on both OSes; OCIO's shader compiles
unmodified or with a documented patch.
**Time box:** 2 weeks from 2026-11-06. **Status: IN PROGRESS — step 1 (software decode, CPU upload = the control) DONE 2026-11-06; step 1b (VideoToolbox + ZERO-COPY into wgpu) DONE 2026-11-06, picture verified; step 2 (OCIO) next.**

## What the scaffold is
`spikes/s1-present`: winit 0.30 + wgpu 30 window drawing a linear-light ramp (0→1 left, 1→4 right; grey /
red / blue bands; ticks at integer multiples of SDR white) on an `Rgba16Float` swapchain.
`cargo run -p s1-present -- [--edr | --pq]` · Esc quits · it prints the adapter, the surface's formats with
their colour spaces, and reads the CAMetalLayer back.

## Facts the scaffold already settled (macOS, M5 Max, 2026-11-06)
- **wgpu 30 has HDR surfaces natively**: `SurfaceConfiguration::color_space` with `ExtendedSrgbLinear`,
  `Bt2100Pq`, `Bt2100Hlg`, `DisplayP3`…; `SurfaceCapabilities::format_capabilities` lists which formats take
  which. Metal reports `Rgba16Float` with EXTENDED_SRGB_LINEAR | BT2100_PQ | BT2100_HLG | EXTENDED_SRGB |
  EXTENDED_DISPLAY_P3 and `Rgb10a2Unorm` with BT2100_PQ | BT2100_HLG. wgpu-hal sets the CAMetalLayer's
  `wantsExtendedDynamicRangeContent` and `colorspace` itself; on DX12 it calls `SetColorSpace1`
  (`G10_NONE_P709` for scRGB fp16, `G2084_NONE_P2020` for PQ 10-bit). **The §11/§12/§18 verify item "EDR
  needs objc2 on the layer" is CLOSED: no objc2 needed for the surface.** objc2 stays only for reading
  the layer back (wgpu's CAMetalLayer is a SUBLAYER of the NSView's backing layer, not its layer).
- Present modes offered on Metal: Fifo, Immediate. Alpha: Opaque, PostMultiplied.
- `winit` is pinned at 0.30.13 for now: `accesskit_winit` 0.34 (S3) still depends on it; the 0.31 beta
  (typed data transfer, §14) is for S5. One workspace pin → the move happens for everyone at once.

## Step 1 — software decode + CPU upload (the CONTROL path), measured 2026-11-06
`cargo run --release -p s1-present -- --edr [--file F] [--threads N]` (default fixture: 4K ProRes). FFmpeg 9 through
`ffmpeg-sys-next` with thin RAII (`decode.rs`), decode on its own thread (bounded channel of 3), planes repacked
to 256-byte pitches and uploaded with `write_texture` into R8Unorm / R16Unorm planes, WGSL does Y'CbCr → BT.709 →
BT.1886 linear on the `Rgba16Float` + `ExtendedSrgbLinear` surface. Wall-clock pacing: the newest due frame is
uploaded, the present loop runs every refresh (60 Hz here), like QCView's. Release build, window on the external 60 Hz display, machine quiet (a first run under an unrelated heavy CPU
task showed upload spikes of 60–190 ms and present gaps of 75–200 ms at 4K — contention, gone on the re-run):
| Fixture (sw decode) | open → first frame | decoded fps | shown fps | upload ms p50 / max | present ms p50 / p95 / max |
|---|---|---|---|---|---|
| ProRes HQ 4K 4:2:2 10-bit | 105 ms | 23.4–24.3 | 22.9–24.3 | 3.1 / 4.6 | 16.6 / 20.1 / 21.8 |
| ProRes HQ 1080p | 40 ms | 23.8–24.5 | 23.8–24.0 | 0.95 / 1.4 | 16.6 / 17.7 / 18.2 |
| HEVC Main10 4K 4:2:0 | 59 ms | 24.0–24.3 | 23.5–24.3 | 2.45 / 3.3 | 16.6 / 19.3 / 20.2 |
| H.264 4K 4:2:0 8-bit | 59 ms | 23.8–24.5 | 23.8–24.0 | 1.44 / 2.0 | 16.6 / 18.2 / 18.5 |
Reading, against the P0 bars: open → first frame 40–105 ms vs QCView's 385–500 (the CPU path, no hwaccel yet);
present p95 ≤ 20.1 ms (bar: ≤ 20) and no interval longer than one refresh (max 21.8 ms; bar: no gap > 1 refresh)
— the pacing bar is MET by the control path; decode keeps up on every codec (FFmpeg frame threads); the 4K 4:2:2
CPU upload costs 3.1 ms of the 16.6 ms budget (33 MB per frame through `write_texture`), which is the cost 1b
removes. The frame loses ~1 fps of "shown" against 23.976 at 4K in some windows: the wall-clock scheduler
presents the newest DUE frame per refresh and 24-on-60 cadence drops one when a frame lands late by < 1 ms;
a proper 3:2-aware scheduler (QCView's audio-master servo, §4) is the fix, not the upload.
Also noted: `TEXTURE_FORMAT_16BIT_NORM` must be requested for R16Unorm planes (present on Metal/DX12).

## Step 1b — VideoToolbox + zero-copy (the REAL path), measured 2026-11-06
`cargo run --release -p s1-present -- --hw --edr [--file F] [--snap out.png]`. `decode.rs`: `av_hwdevice_ctx_create(VIDEOTOOLBOX)`
+ a `get_format` that picks `AV_PIX_FMT_VIDEOTOOLBOX`; the frame's `data[3]` CVPixelBuffer travels to the render thread as an
`av_frame_clone` reference. `vt.rs`: `device.as_hal::<Metal>().raw_device()` → one `CVMetalTextureCache`; per plane
`CVMetalTextureCacheCreateTextureFromImage` → `MTLTexture` → `wgpu::hal::metal::Device::texture_from_raw` →
`Device::create_texture_from_hal` (usage TEXTURE_BINDING, state RESOURCE). The decoder's IOSurface is what the shader
samples; a ring of 3 held frames keeps the CVMetalTexture + AVFrame alive while the GPU is ≤ 2 frames behind; the
cache is flushed every 60 frames. Biplanar WGSL (`present_bi.wgsl`): Y + CbCr, 10-bit left-justified in 16
(`yscale = 65535/65472`), video/full range from the fourcc. Machine quiet, window on the external 60 Hz display:
| Fixture (VideoToolbox, zero-copy) | CVPixelBuffer | open → first frame | decoded / shown fps | import ms p50 / max | present ms p50 / p95 / max |
|---|---|---|---|---|---|
| ProRes HQ 4K 4:2:2 10-bit | `x422` | 167 ms | 23.8–24.4 / 23.8–24.0 | 0.09 / 0.25 | 16.7 / 16.9–17.1 / 17.1–19.8 |
| ProRes HQ 1080p | `x422` | 134 ms | 23.3–23.8 / 22.8–23.8 | 0.09 / 0.25 | 16.6 / 17.0 / 17.0–17.3 |
| HEVC Main10 4K 4:2:0 | `x420` | 123 ms | 23.4–23.8 / 22.9–23.8 | 0.13 / 0.35 | 16.7 / 17.0–17.2 / 17.2–17.5 |
| H.264 4K 4:2:0 8-bit | `420v` | 195 ms | 22.3–24.3 / 21.8–24.3 | 0.12 / 0.33 | 16.6 / 17.0–17.1 / 17.3–17.4 |
**Against the P0 bars (QCView, same display, same fixtures):** open → first frame 123–195 ms vs 385–500 ✔; present
p95 16.9–17.2 ms vs QCView's 17.4–20.5 ✔ (tighter than the Qt app's own present loop); max 17.1–19.8 vs 20.3–61 ✔;
per-frame cost on the render thread 0.1 ms vs the CPU path's 3.1 ms (4K 4:2:2). Zero bytes copied per frame.
**Picture verified** (`--snap`, frame 24 of the 4K ProRes, both paths render into an Rgba8 sRGB target):
viewed — correct image, colour and range, clean 4:2:2 chroma; zero-copy vs the CPU control on the SAME frame:
PSNR 58.7 dB, SSIM 0.9987 (the residual is VideoToolbox's chroma siting vs the software path's; both are right).
Snapshots live in `fixtures/snaps/` (client frames — ignored).
Not yet done here: the wall-clock scheduler still drops ~0.5 fps at 24-on-60 (3:2-aware scheduling / audio
master is player work, §4/§17, not S1's question); EDR headroom not exercised by video content yet (the ramp did it);
Windows D3D12VA path (one copy is the expected answer, P0 bar: QCView's D3D11 path) owed on the box.

## Remaining plan (in order)
1. ~~Decode one frame~~ DONE as step 1 + 1b. Formerly: **1b:** VideoToolbox hwaccel in `decode.rs` (`av_hwdevice_ctx_create`, the
   `get_format` callback) → `CVPixelBuffer` ('x422'/'420v'/'x420') → `CVMetalTextureCache` → `MTLTexture` per plane
   → `wgpu::Device::create_texture_from_hal` (Metal hal `texture_from_raw`) — zero copies; biplanar shader variant.
   Then the same table as above. Formerly: `min-image`-shaped FFmpeg binding (raw `ffmpeg-sys-next` 9 + thin RAII, §4) → upload
   (zero-copy CVPixelBuffer → Metal texture via wgpu-hal on macOS; CPU upload first as the control).
2. OCIO: build the display shader with OCIO's C++ API (FFI) → MSL via wgpu's passthrough shader module
   (experimental, `unsafe` gate; name per wgpu 30) and HLSL→DXIL on Windows; baked shaper + 3D LUT in WGSL
   as route B.
3. Pixel-diff against Qt QCView's `QCV_CAPTURE_PNG` of the same frame; EDR check on the built-in XDR
   (P0 ran on the external AW3225QF with EDR off), PQ on Windows.
4. Numbers per the pass bar, then the verdict (go / go-with-LUT / no-go) written here.
