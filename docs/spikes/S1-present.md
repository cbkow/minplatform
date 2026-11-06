# S1 — present: one decoded frame → wgpu → an HDR surface through an OCIO shader

**Question (§21.3):** can a wgpu presenter on Metal and DX12 show a decoded frame on an HDR surface
through OCIO's generated shader (native passthrough; baked LUT as the control) at QCView's numbers?
**Pass bar (vs P0):** open → first frame ≤ 500 ms (4K ProRes: 385); steady present p95 ≤ 20 ms at 60 Hz, no
gap > 1 refresh; GPU ≤ 1 ms p95 per frame at 4K 4:2:2; ≤ 0.25 core; footprint ≤ 1.4 GB at 4K; pixel-diff vs
Qt QCView on the same frame within tolerance; EDR peak + PQ correct on both OSes; OCIO's shader compiles
unmodified or with a documented patch.
**Time box:** 2 weeks from 2026-11-06. **Status: IN PROGRESS — step 1 (software decode, CPU upload = the control) DONE 2026-11-06; step 1b (VideoToolbox + zero-copy) next; FIRST re-run step 1 on a quiet machine.**

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
uploaded, the present loop runs every refresh (60 Hz here), like QCView's. Release build, window on the external
60 Hz display. **CAVEAT: taken while an unrelated heavy CPU task was running on the machine (the user's) — the
upload/present SPIKES below may be contention, not the path; RE-RUN OWED on a quiet machine before any reading.**
| Fixture (sw decode) | open → first frame | decoded fps | shown fps | upload ms p50 / max | present ms p50 / p95 / max |
|---|---|---|---|---|---|
| ProRes HQ 4K 4:2:2 10-bit | 231 ms | 24.0 | 20.5–24.0 | 4.5–6.2 / 58–191 | 16.5 / 26–83 / 75–203 |
| ProRes HQ 1080p | 351 ms | 23.8–24.0 | 23.8–24.0 | 1.2–1.6 / 2–68 | 16.6 / 18.1 / 19–60 |
| HEVC Main10 4K 4:2:0 | 91 ms | 24.0 | 23.5–24.0 | 3.9–4.1 / 5–42 | 16.6 / 21–22 / 21–62 |
| H.264 4K 4:2:0 8-bit | 102 ms | 23.9–24.3 | 23.3–23.4 | 2.1–2.4 / 5–83 | 16.7 / 19–50 / 22–100 |
Reading: open → first frame already beats P0's 385–500 ms. Decode keeps up on every codec (FFmpeg frame threads).
The CPU path's problem is the UPLOAD: 33 MB per 4K 4:2:2 frame through `write_texture` (wgpu's internal staging)
spikes to 60–190 ms and drops frames; 4:2:0 and 1080p sit near the pacing bar but still spike. This is the
expected shape of the control path, not the verdict: the real path is 1b. If the CPU path is ever needed
(DX12 fallback), the fixes are known — persistent mapped staging ring + `copy_buffer_to_texture`, no repack when
`linesize % 256 == 0` (true for 4K planes), upload off the render thread.
Also noted: `TEXTURE_FORMAT_16BIT_NORM` must be requested for R16Unorm planes (present on Metal/DX12).

## Remaining plan (in order)
1. ~~Decode one frame~~ DONE as step 1. **1b:** VideoToolbox hwaccel in `decode.rs` (`av_hwdevice_ctx_create`, the
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
