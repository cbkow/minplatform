# S1 — present: one decoded frame → wgpu → an HDR surface through an OCIO shader

**Question (§21.3):** can a wgpu presenter on Metal and DX12 show a decoded frame on an HDR surface
through OCIO's generated shader (native passthrough; baked LUT as the control) at QCView's numbers?
**Pass bar (vs P0):** open → first frame ≤ 500 ms (4K ProRes: 385); steady present p95 ≤ 20 ms at 60 Hz, no
gap > 1 refresh; GPU ≤ 1 ms p95 per frame at 4K 4:2:2; ≤ 0.25 core; footprint ≤ 1.4 GB at 4K; pixel-diff vs
Qt QCView on the same frame within tolerance; EDR peak + PQ correct on both OSes; OCIO's shader compiles
unmodified or with a documented patch.
**Time box:** 2 weeks from 2026-11-06. **Status: IN PROGRESS — step 1 (software decode, CPU upload = the control) DONE 2026-11-06; step 1b (VideoToolbox + ZERO-COPY into wgpu) DONE 2026-11-06, picture verified; step 2 (OCIO: native MSL passthrough = EXACT, baked LUT = 8-bit-exact only) DONE 2026-11-06 — one clean per-frame GPU trace owed (display locked); step 3 (pixel-diff vs QCView, EDR, PQ) next.**

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

## Step 2 — OCIO on wgpu: native passthrough (route A) vs baked LUT (route B), measured 2026-11-06
`cargo run --release -p s1-present --features ocio -- --hw --edr --file F --ocio CONFIG [--cs CS --display D --view V]
[--route native|lut] [--ocio-verify] [--snap out.png] [--screen N] [--seconds S]` (needs an installed OpenColorIO 2.5:
`OCIO_ROOT=` or `pkg-config`; QCView's `external/install` served). `ocio_shim.cpp` = a C ABI over OCIO (one
`DisplayViewTransform` → `GpuShaderDesc` MSL 2.0 / HLSL SM5 + LUT inventory + the CPU processor + a 3D bake), `ocio.rs` =
the two routes. The spike's graph per NEW video frame: planes → encoded R'G'B' (Rgba16Float, video size) → **OCIO pass**
(compute, 16×16) → display-encoded Rgba16Float → blit with fit + the display EOTF onto the extended-linear surface every refresh.

**Route A = QCView's compute-kernel wrapper around OCIO's generated MSL, loaded with `Device::create_shader_module_passthrough`**
(wgpu 30: feature `PASSTHROUGH_SHADERS`, an `unsafe fn`, descriptor `ShaderModuleDescriptorPassthrough { msl, entry_points:
[{ name, workgroup_size }] }`; the kernel is dumped to `docs/spikes/s1/out/ocio-kernel.metal`). **The slot mapping is a rule, not a
guess:** wgpu's Metal backend numbers each resource class (buffers / textures / samplers) sequentially in bind-group-entry order per
stage, so a layout of [src texture, dst storage texture, sampler, LUT…] lands on `[[texture(0)]] [[texture(1)]] [[sampler(0)]]
[[texture(2..)]]` — exactly QCView's wrapper. No reflection → the pipeline layout must be explicit; wgpu validates nothing about
the MSL's bindings. LUTs are ordinary wgpu textures (D1 / D2 / D3, Rgba32Float or R32Float, `FLOAT32_FILTERABLE` is available on
Apple silicon) and OCIO's call-argument order (all 3D LUTs by registration index, then 1D/2D, then the pixel) is reproduced
from the texture names, as QCView does. **Route B** = the CPU processor sampled on a 65³ lattice over the encoded [0,1]³ input →
one Rgba16Float 3D texture → one WGSL compute stage (trilinear). **Ground truth** = OCIO's own CPU processor on the readback of
the same encoded frame (frame 24; `--ocio-verify`); errors in display-encoded [0,1].

| Chain (config · input → display / view) | Route | LUT textures bound | max err (/255 · /1023) | PSNR | samples off > ½ LSB: 8-bit · 10-bit |
|---|---|---|---|---|---|
| ACES 1.3 studio · Camera Rec.709 → sRGB - Display / ACES 1.0 - SDR Video | A native | none (315 lines of MSL, 0 uniforms) | 0.13 · 0.51 | 79.1 dB | 0 % · 0.046 % |
| same | B baked 65³ | — | 0.55 · 2.20 | 78.2 dB | 0 % · 0.535 % |
| Blender 5.2 · sRGB → sRGB / AgX | A native | 3D 57³ RGB | 0.13 · 0.52 | 78.1 dB | 0 % · 0.054 % |
| same | B baked 65³ | — | 1.69 · 6.79 | 75.0 dB | 0.061 % · 1.37 % |
| Blender 5.2 · sRGB → sRGB / Filmic | A native | 2D 4096×2 R (the 1D LUT) + 3D 33³ RGB | 0.13 · 0.50 | 77.2 dB | 0 % · 0 % |
| same | B baked 65³ | — | 0.73 · 2.91 | 79.9 dB | 0.012 % · 0.033 % |

Reading: **route A is exact** — every chain, with and without LUTs, matches OCIO's CPU path to ≤ 0.13/255 at every sample (the
residual is fp16 storage + GPU vs CPU float order); the 1D-LUT-as-`texture2d` and `texture3d` arguments resolve and sample
correctly through wgpu's bind groups; **OCIO 2.5's MSL 1D-LUT channel bug (#2272, §18.5) does not bite this pin on a red-only
1D LUT.** **Route B is 8-bit-exact and NOT 10-bit-exact**: worst case 1.7/255 (AgX), 0.5–1.4 % of samples off by more than half a
10-bit code — the canon's "approximate for some ops" made a number. Pictures (snapshots) are visually identical between routes.
Costs: OCIO chain build 7–30 ms; route A's MSL compile + pipeline 90–190 ms cold, **0 ms warm (the OS's Metal shader cache)**;
route B's bake 3–11 ms + stage 5 ms (138 ms the first time, WGSL compile). Open → first frame 163–183 ms with either route.
Verification itself costs one ~450 ms present gap (two 4K readbacks + the CPU processor, 77–300 ms); it is off by default.

**GPU + pacing (Metal System Trace, attached; 4K ProRes 4:2:2 10-bit, zero-copy, the external 60 Hz display, release):**
| Variant | `ocio` compute encoder ms p50 / p95 / max | spike's planes→RGB pass | blit to the 2560×1440 surface | present-request interval p50 / p95 / max | CPU |
|---|---|---|---|---|---|
| no OCIO | — | — | 0.19 / 0.25 | 16.7 / 17.0 / 17.6 | 10 % mean |
| ACES 1.3, route A | 0.31 / 0.43 / 0.48 | 0.44 / 0.58 | 0.19 / 0.26 | 16.6 / 17.3 / 20.4 | 10 % |
| ACES 1.3, route B | 0.33 / 0.42 / 0.48 | 0.47 / 0.59 | 0.19 / 0.25 | 16.6 / 17.7 / 19.4 | 10 % |
| Blender Filmic, route A (2 LUTs) | 0.33 / 0.47 / 0.49 | 0.47 / 0.65 | 0.19 / 0.28 | 16.6 / 17.5 / 20.8 | 10 % |
Reading: the OCIO pass is 0.3–0.5 ms at 4K — the same cost for the generated code and for a LUT; pacing is unchanged with
OCIO on (p95 ≤ 17.7 ms, no gap > 1 refresh; QCView P0: 17.4–20.5); the per-frame GPU sum is ≈ 1.0 ms p50 by encoder sum, of
which the separate planes→RGB pass (0.45 ms) is a spike artefact — the real presenter reads the planes inside the first colour
stage, as QCView's compositeRaw does. The bar "GPU ≤ 1 ms p95 at 4K" is met by the colour pass with room; the whole-frame
figure needs the fused graph. For the record: QCView's own P0 trace, the same per-frame script (`tools/s1/gpu.py`): 4K ProRes
p50 0.315 / p95 1.10 ms; this spike without OCIO 0.32 / 0.46 ms.
**Owed:** one clean traced run per variant with the wgpu timestamp machinery off (`--gpu-ts` now defaults off) — the three
re-runs hit a locked screen (every frame `Occluded`; the spike now sleeps instead of spinning on it). wgpu's `TIMESTAMP_QUERY`
on Metal read 0 for about half the compute-pass pairs here, so the trace is the figure of record, as in P0.
**Not covered:** Windows (HLSL SM5 → DXIL through the same shim; the shim already takes the language); dynamic-property
uniforms (static chains emit none); the Knee split (two OCIO halves around a WGSL stage — the same mechanism twice);
route B's error on the PQ 1000-nit ACES views (§18.5's last item; the HDR check is step 3's).

## Remaining plan (in order)
1. ~~Decode one frame~~ DONE as step 1 + 1b. Formerly: **1b:** VideoToolbox hwaccel in `decode.rs` (`av_hwdevice_ctx_create`, the
   `get_format` callback) → `CVPixelBuffer` ('x422'/'420v'/'x420') → `CVMetalTextureCache` → `MTLTexture` per plane
   → `wgpu::Device::create_texture_from_hal` (Metal hal `texture_from_raw`) — zero copies; biplanar shader variant.
   Then the same table as above. Formerly: `min-image`-shaped FFmpeg binding (raw `ffmpeg-sys-next` 9 + thin RAII, §4) → upload
   (zero-copy CVPixelBuffer → Metal texture via wgpu-hal on macOS; CPU upload first as the control).
2. ~~OCIO~~ DONE as step 2 (macOS; Windows DXIL owed with the rest of Windows).
3. Pixel-diff against Qt QCView's `QCV_CAPTURE_PNG` of the same frame; EDR check on the built-in XDR
   (P0 ran on the external AW3225QF with EDR off), PQ on Windows.
4. Numbers per the pass bar, then the verdict (go / go-with-LUT / no-go) written here.
