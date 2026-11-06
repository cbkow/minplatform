# S1 — present: one decoded frame → wgpu → an HDR surface through an OCIO shader

**Question (§21.3):** can a wgpu presenter on Metal and DX12 show a decoded frame on an HDR surface
through OCIO's generated shader (native passthrough; baked LUT as the control) at QCView's numbers?
**Pass bar (vs P0):** open → first frame ≤ 500 ms (4K ProRes: 385); steady present p95 ≤ 20 ms at 60 Hz, no
gap > 1 refresh; GPU ≤ 1 ms p95 per frame at 4K 4:2:2; ≤ 0.25 core; footprint ≤ 1.4 GB at 4K; pixel-diff vs
Qt QCView on the same frame within tolerance; EDR peak + PQ correct on both OSes; OCIO's shader compiles
unmodified or with a documented patch.
**Time box:** 2 weeks from the day it starts. **Status: SCAFFOLD (2026-11-06) — the spike has NOT started.**

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

## Remaining plan (in order)
1. Decode one frame: `min-image`-shaped FFmpeg binding (raw `ffmpeg-sys-next` 9 + thin RAII, §4) → upload
   (zero-copy CVPixelBuffer → Metal texture via wgpu-hal on macOS; CPU upload first as the control).
2. OCIO: build the display shader with OCIO's C++ API (FFI) → MSL via wgpu's passthrough shader module
   (experimental, `unsafe` gate; name per wgpu 30) and HLSL→DXIL on Windows; baked shaper + 3D LUT in WGSL
   as route B.
3. Pixel-diff against Qt QCView's `QCV_CAPTURE_PNG` of the same frame; EDR check on the built-in XDR
   (P0 ran on the external AW3225QF with EDR off), PQ on Windows.
4. Numbers per the pass bar, then the verdict (go / go-with-LUT / no-go) written here.
