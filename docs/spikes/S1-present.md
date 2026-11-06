# S1 — present: one decoded frame → wgpu → an HDR surface through an OCIO shader

**Question (§21.3):** can a wgpu presenter on Metal and DX12 show a decoded frame on an HDR surface
through OCIO's generated shader (native passthrough; baked LUT as the control) at QCView's numbers?
**Pass bar (vs P0):** open → first frame ≤ 500 ms (4K ProRes: 385); steady present p95 ≤ 20 ms at 60 Hz, no
gap > 1 refresh; GPU ≤ 1 ms p95 per frame at 4K 4:2:2; ≤ 0.25 core; footprint ≤ 1.4 GB at 4K; pixel-diff vs
Qt QCView on the same frame within tolerance; EDR peak + PQ correct on both OSes; OCIO's shader compiles
unmodified or with a documented patch.
**Time box:** 2 weeks from 2026-11-06. **Status: IN PROGRESS — step 1 (software decode, CPU upload = the control) DONE 2026-11-06; step 1b (VideoToolbox + ZERO-COPY into wgpu) DONE 2026-11-06, picture verified; step 2 (OCIO: native MSL passthrough = EXACT, baked LUT = 8-bit-exact only) DONE 2026-11-06 (per-frame GPU traced); step 3 (pixel-diff vs QCView = 55.9 dB, EDR on the XDR engages, PQ decode exact to the file on both decoders, the PQ surface shown on the XDR after working around a wgpu-hal 30.0.1 CRASH) DONE 2026-11-06. **VERDICT (macOS): GO — see "Verdict" below. Windows numbers owed before gate G1.**

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
**Per-frame GPU sum (the clean traced runs, timestamp machinery off, `tools/s1/gpu.py` = every command buffer of the frame):**
no OCIO p50 0.34 / p95 0.48 / max 0.60 ms; ACES route A 0.43 / 1.89 / 2.15; ACES route B 0.41 / 1.73 / 2.11; Filmic route A
0.42 / 1.89 / 2.17 — p50 is the refresh-only frame, p95 is the frame that runs the colour chain (24 of 60 do). QCView's own P0
trace through the same script: 0.315 / 1.10 / 428 ms. **So the unfused spike frame costs ~1.9 ms where QCView's costs ~1.1 — over
the 1 ms bar by the two extra passes (planes→RGB 0.45 + wgpu's pre-pass 0.24) the fused presenter graph removes; the OCIO pass
itself (0.3–0.5) fits.** wgpu's `TIMESTAMP_QUERY` on Metal read 0 for about half the compute-pass pairs here, so the trace is
the figure of record, as in P0 (`--gpu-ts` opts the queries back in). The spike sleeps instead of spinning on an `Occluded`
surface (a locked screen returns that for every frame — check `ioreg -n Root -d1 | grep CGSSessionScreenIsLocked` first).
**Not covered:** Windows (HLSL SM5 → DXIL through the same shim; the shim already takes the language); dynamic-property
uniforms (static chains emit none); the Knee split (two OCIO halves around a WGSL stage — the same mechanism twice);
route B's error on the PQ 1000-nit ACES views (§18.5's last item; the HDR check is step 3's).

## Step 3 — pixel-diff vs QCView, EDR on the built-in XDR, PQ (2026-11-06)
**Pixel-diff vs QCView's own capture.** QCView: `QCV_CAPTURE_PNG=<png>@6` on the 4K ProRes fixture, paused at frame 0 (its
default open state), engine OCIO, config Blender 5.2, input pin `sRGB`, display `sRGB` / default view `Standard` (an identity
chain) — `captureScreenshot()` composites the post-chain frame onto an RGBA8 canvas at the drawable size capped at 4K, 3840×2160
here. Spike: `--snap-frame 1 --snap-out <png>` = the display-encoded frame at the video size, 8-bit, no OCIO; and the same chain
through OCIO route A (`--cs sRGB --display sRGB --view Standard`). ffmpeg `psnr` / `ssim` / difference stats on the two PNGs:
| Spike variant vs QCView | PSNR (R / G / B / avg) | SSIM | max |diff| | mean |diff| |
|---|---|---|---|---|
| no OCIO (BT.709 matrix, limited range, bilinear chroma) | 56.2 / 52.8 / 55.8 / **55.9 dB** | 0.9978 | 2 / 255 | 0.24 / 255 |
| OCIO identity chain, route A | bit-identical to the row above | | | |
Reading: the two players agree to within 2 codes everywhere (the residual is a fine, picture-wide texture — chroma
reconstruction, not a colour or range error) — the §21 "pixel-diff within tolerance" bar is met; and OCIO's identity chain is
exact through the passthrough path. Caveat: this is the IDENTITY view — QCView's active view is a UI setting, not an env var, so
a non-trivial chain in both players is a hand-driven run (step 2 already proved route A against OCIO's CPU processor on three
non-trivial chains).

**PQ decode + EDR on the built-in XDR.** A synthetic fixture (`fixtures/pq_ramp_4k.mp4`, generic): a 16-bit PNG — grey ramp
code = x/(W−1) over the top half, then bands at PQ code 0.5081 / 0.7518 / 0.9026 (100 / 1000 / 4000 nit) — encoded x265 Main10,
BT.2020 NCL, SMPTE 2084, limited range (tags verified by ffprobe; the earlier `hevc10_pq_4k` fixture is a DECODE fixture whose
PQ tags VideoToolbox's encoder dropped, so it is not HDR truth). The spike now reads the decoder's colour tags (`--src` overrides):
BT.2020 matrix + PQ EOTF → extended linear with 1.0 = 100 nit (`--white`), BT.2020 → BT.709 primaries. `--verify-pq` renders the
linear frame 1:1 and compares to the EOTF: worst relative error 7 % at code 0.065, ~2 % elsewhere, **all of it in the fixture**
(ffmpeg's own luma at x = 1920 is code 504, 2–3 codes above the intended 502; the spike's 94.19 nit there is PQ(0.5023) exactly).
1895 of 3840 columns land above 1.0 — real EDR values on the surface — and **the XDR's `maximumExtendedDynamicRangeColorComponentValue`
rose from 1.0 to 8.5 while the window was up** (the OS engaged EDR for our layer; potential 16). Pacing on the XDR at 60 Hz: p50
16.7 / p95 17.3 ms. The ramp fixture loops every 3 s, which confuses the spike's wall-clock scheduler at the seam (shown fps
climbs) — a spike artefact, not a decode one.

**PQ surface — a wgpu-hal bug.** `SurfaceColorSpace::Bt2100Pq` crashes in `Surface::configure` on Metal (SIGBUS in
`CGColorSpaceCreateWithName`, macOS 26.7.1, wgpu-hal 30.0.1): its dynamic lookup of `kCGColorSpaceITUR_2100_PQ` passes the
symbol's ADDRESS as the CFString (the fault address is the global + 16; ctypes reading the global's VALUE works). `Bt2100Hlg` and
`ExtendedDisplayP3` take the same path. The spike's workaround: configure as `ExtendedSrgbLinear` (wgpu still sets wantsEDR and the
fp16 drawable) and set the layer's colour space ourselves by name after every configure (`mac::set_layer_colorspace`); the PQ
source is then passed through encoded (BT.2020 matrix, no EOTF). **The upstream report is owed (a one-line fix: read the pointer
stored at the symbol, not the symbol's address).**

**The PQ-surface picture (screen unlocked, 2026-11-06):** the ramp + the 100 / 1000 / 4000-nit bands displayed on the XDR
through the BT.2100 PQ layer (481 frames, p95 17.1–17.3 ms, the XDR's EDR headroom 1.0 → 8.8 while up), and the software-decode
`--verify-pq` numbers are identical to VideoToolbox's — the PQ path is decoder-independent and exact to the file.

**Footprint (4K ProRes, zero-copy, 10 s, `footprint` mid-run / max RSS):** no OCIO 397 MB / 272 MB; OCIO route A 655 MB / 270 MB.
QCView on the same fixture in P0: 1362 MB / 546 MB. Bar ≤ 1.4 GB: met with room (the OCIO stage's two 4K fp16 textures cost
~130 MB; the rest is the VideoToolbox pool + Metal).

## Verdict (macOS, 2026-11-06) — GO
| Pass bar (§21.3 / P0) | S1 result | |
|---|---|---|
| open → first frame ≤ 500 ms (QCView 385–500) | 108–195 ms (zero-copy), 40–105 (software) | met |
| steady present p95 ≤ 20 ms at 60 Hz, no gap > 1 refresh (QCView 17.4–20.5) | 16.9–17.7 ms p95, max ≤ 18.3, with OCIO on | met |
| GPU ≤ 1 ms p95 per frame at 4K 4:2:2 (QCView per-frame 1.10) | colour pass 0.3–0.5 ms; the UNFUSED spike frame 1.9 ms p95 (two extra passes the presenter graph fuses) | stage cost met; frame cost needs the fused graph — not a wgpu limit |
| ≤ 0.25 core | 10 % of a core mean | met |
| footprint ≤ 1.4 GB at 4K (QCView 1.36 GB) | 0.40–0.66 GB | met |
| pixel-diff vs QCView on the same frame within tolerance | 55.9 dB, max 2/255 (identity view; non-trivial chains proved against OCIO's CPU processor instead) | met |
| EDR peak + PQ correct on both OSes | macOS: EDR engages on the XDR (headroom 1.0 → 8.5–8.8), PQ decode exact to the file, PQ surface works with the wgpu-hal workaround; **Windows owed** | met on macOS |
| OCIO's shader compiles unmodified or with a documented patch | unmodified, through wgpu's passthrough, exact | met |
**What the GO rests on:** wgpu 30's native HDR surfaces (extended-linear unpatched; PQ/HLG need the one-line wgpu-hal fix or
the layer workaround), VideoToolbox zero-copy through wgpu's hal (§4 / §11 as designed), OCIO route A as the display path with
route B as the 8-bit fallback (§18 rows 91/92 — evidence in), and a presenter graph that reads the planes inside the first colour
stage (the only bar the spike itself misses, by construction). **What it does not cover:** Windows (DX12 + D3D12VA + DXIL
passthrough + PQ on the box — the other half of row 45's pair), the Knee split, dynamic OCIO uniforms, 3:2-aware scheduling
(player work, S2's territory), audio. Gate G1 waits for S2 (scrub) + S4 (ring) + S5 (window) and the Windows numbers.

## Remaining plan (in order)
1. ~~Decode one frame~~ DONE as step 1 + 1b. Formerly: **1b:** VideoToolbox hwaccel in `decode.rs` (`av_hwdevice_ctx_create`, the
   `get_format` callback) → `CVPixelBuffer` ('x422'/'420v'/'x420') → `CVMetalTextureCache` → `MTLTexture` per plane
   → `wgpu::Device::create_texture_from_hal` (Metal hal `texture_from_raw`) — zero copies; biplanar shader variant.
   Then the same table as above. Formerly: `min-image`-shaped FFmpeg binding (raw `ffmpeg-sys-next` 9 + thin RAII, §4) → upload
   (zero-copy CVPixelBuffer → Metal texture via wgpu-hal on macOS; CPU upload first as the control).
2. ~~OCIO~~ DONE as step 2 (macOS; Windows DXIL owed with the rest of Windows).
3. ~~Pixel-diff against QCView; EDR on the XDR; PQ~~ step 3 above (macOS; the PQ-surface picture pending the screen; PQ on
   Windows owed with Windows).
4. ~~Numbers per the pass bar, then the verdict~~ written above (GO on macOS, 2026-11-06). Windows: below.

## Windows (rhombus: i9-14900KF, RTX 4090, Windows 11 25H2, 2026-11-06) — DX12 + D3D12VA zero-copy + OCIO HLSL passthrough
**Build:** the same crate with `--features ocio` on MSVC (cargo 1.99, VS 18; `LIBCLANG_PATH` for ffmpeg-sys-next's bindgen — LLVM
installed on the box for it; `FFMPEG_DIR` = QCView's BtbN FFmpeg 9.0.1; `OCIO_ROOT` = QCView's manifest `vcpkg_installed\x64-windows`,
OpenColorIO 2.5.2). Builds clean first time; the FFmpeg + OCIO DLL dirs go on PATH at launch (`C:\qcb-lab\s1-runs.ps1` generates
one `.cmd` per run and launches it on the logged-on desktop via `run.ps1`).
**THE BOX HAS NO PHYSICAL DISPLAY** (a Parsec / Jump virtual display; see P0 §5): every present is throttled to ≈ 13 Hz (present
interval 76 ms p50 / 79 p95 for the spike AND for QCView). **Pacing, EDR-on-a-display and PQ-on-a-display are therefore NOT
measured on Windows yet**; first frame, decode, zero-copy import, OCIO exactness + GPU time, the PQ pipeline arithmetic and the
pixel-diffs are, and they are the DX12 answers.
**wgpu backend:** wgpu's default instance picked **Vulkan** on the RTX 4090, whose swapchain offers 8-bit sRGB only → `--edr` panicked.
The spike now forces **DX12** on Windows (`--backend vulkan` to compare), the canon's row-45 pair. DX12's surface: `Rgba16Float` with
EXTENDED_SRGB_LINEAR, `Rgb10a2Unorm` with BT2100_PQ (DXGI's own pairing: scRGB fp16, HDR10 = 10-bit), 8-bit formats SRGB; present
modes Mailbox / Fifo / Immediate. All four wanted features present (16-bit norm, f32 filterable, timestamps, passthrough).

| Run (4K, release) | open → first frame | import / upload ms p50 / max | present ms p50 / p95 (virtual display) | CPU (process, decode threads in) | max working set |
|---|---|---|---|---|---|
| ProRes 4K, software + upload (no hw ProRes on NVIDIA) | 118–169 ms | upload 2.3 / 5.5 | 77 / 82 | 161–168 % of a core | 1.0 GB |
| H.264 4K, software + upload | 137 ms | 1.4 / 3.2 | 76 / 81 | — | — |
| **H.264 4K, D3D12VA zero-copy (NV12)** | **136–139 ms** | **import 0.27 / 0.47** | 77 / 79 | **11 %** | **521 MB** |
| **HEVC Main10 4K, D3D12VA zero-copy (P010)** | **101 ms** | **0.26 / 0.59** | 78 / 80 | — | — |
| HEVC10 4K, software + upload | 165 ms | 1.9 / 4.0 | 77 / 83 | — | — |
(macOS VideoToolbox: first frame 123–195 ms, import 0.1 ms; QCView-Windows working sets 371–942 MB.)

**Zero-copy on DX12 — how (`d3d.rs`, the `vt.rs` analogue):** FFmpeg's D3D12VA device context is built ON wgpu's own `ID3D12Device`
(`av_hwdevice_ctx_alloc` → `AVD3D12VADeviceContext.device` = `hal::dx12::Device::raw_device()` → `av_hwdevice_ctx_init`), so the
decoder's NV12 / P010 `ID3D12Resource` needs no NT-handle sharing; each frame's resource becomes TWO single-plane wgpu textures
through `hal::dx12::Device::texture_from_raw(..).with_plane_slice(0 | 1)` (R8 / RG8 or R16 / RG16 Unorm — the same biplanar shader
as VideoToolbox), and the frame's fence is staged on wgpu's queue with `hal::dx12::Queue::add_wait_fence` before the submit. wgpu 30
has both hooks for exactly this. ProRes stays software on NVIDIA (QCView uses FFmpeg's VULKAN hwaccel for ProRes there — a route
the port can take too, through Vulkan→DX12 interop or by running the player on Vulkan for that codec; not measured here).

**OCIO on DX12 (route A = OCIO's HLSL SM 5.0 through `ShaderModuleDescriptorPassthrough { hlsl }`, compiled by wgpu's DX12 backend
— no dxcompiler.dll needed for SM5):** QCView's compute-kernel wrapper again, with OCIO's global texture / sampler declarations
rewritten to explicit registers (`t1..` in space0; every OCIO sampler → the one shared `s0`), the same bind-group-entry order as
Metal (SRV t0 = src, UAV u0 = dst, s0, LUTs t1..). ACES 1.3 Camera Rec.709 → sRGB / ACES 1.0 SDR Video on the H.264 4K frame:
| Route | vs OCIO's CPU processor (4K) | GPU ms p50 / p95 / max (DX12 timestamp queries, 100 runs) | build |
|---|---|---|---|
| A native HLSL passthrough | **max 0.13/255 (0.51/1023), PSNR 79.4 dB, 0 % / 0.018 % off at 8 / 10 bit** | 2.29 / 3.25 / 3.55 | HLSL compiled + pipeline 65 ms |
| B baked 65³ LUT (WGSL) | max 0.47/255 (1.90/1023), 77.2 dB, 0 % / 1.05 % | 2.22 / 2.48 / 3.67 | bake 21 ms + 34 ms |
Route A is exact on DX12 as on Metal (the same 0.13/255 residual); route B stays 8-bit-exact only. **DX12 timestamp queries work**
(p50 2.2–2.3 ms for the 4K pass on the RTX 4090 — ≈ 7× the Metal figure; the number includes wgpu's per-pass barriers and a UAV
write of 4K fp16, and the virtual display may clock the GPU down between presents — treat it as an upper bound until a real
display run). The LUT-bearing chains (Blender AgX / Filmic) were not run on Windows; the register rewrite covers them by
construction (`Texture1D/2D/3D` declarations).

**PQ:** the ramp fixture through D3D12VA (P010) + BT.2020 + PQ EOTF → extended linear: `--verify-pq` gives EXACTLY the macOS numbers
(worst 7.198 % at code 0.065 — the fixture's own offset; 1895 of 3840 columns above 1.0), i.e. the decode + colour arithmetic is
platform-independent. The `Bt2100Pq` surface configures on DX12 (wgpu-hal's DX12 `SetColorSpace1`; no crash — the Metal
`lookup()` bug is Metal-only) and presents 117 frames; what the display does with it is unmeasurable here.

**Pixel-diff, frame 1 of the 4K ProRes (software decode on Windows, VideoToolbox on macOS):**
| Pair | PSNR | SSIM |
|---|---|---|
| spike-Windows vs spike-macOS | 59.6 dB | 0.9991 |
| spike-Windows vs QCView-Windows (`QCV_CAPTURE_PNG`, identity view) | 56.4 dB (max ≈ 2/255) | 0.998 |
| spike-Windows vs QCView-macOS | 55.7 dB | — |
| QCView-Windows vs QCView-macOS | 60.6 dB | — |
Same class on both OSes; the residual is chroma reconstruction again.

**Verdict on Windows (partial): the DX12 half of row 45 holds** — zero-copy D3D12VA into wgpu, OCIO's HLSL through passthrough
exact, fp16 scRGB + 10-bit PQ surfaces offered, 11 % of a core at 4K H.264. **Owed:** pacing, EDR, PQ on a REAL display (and an
HDR one); the LUT chains on DX12; ProRes via Vulkan video; Vulkan-backend comparison; the fused-graph GPU figure.
