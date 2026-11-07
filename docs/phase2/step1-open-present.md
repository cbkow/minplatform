# Phase 2 · step 1 — open → presenter (2026-11-06)

**Question.** Can the first three real crates (`min-present`, `qcv-player`, `qcv-timeline`) and `apps/qcv` on winit 0.31
open a file — or an image-sequence frame through `min-paths` — and play it on the FUSED presenter with S1's and S2's
numbers intact, including the one bar S1 itself missed (the per-frame GPU cost of the unfused spike)?

**What step 1 is.** `qcv <file>` (or a drop onto the window): a winit 0.31 window with a native menu bar, one streaming
decoder slot (VideoToolbox / D3D12VA zero-copy, software + plane upload as the control and the remote-desktop fallback),
the rational clock from S2 driving which frame each refresh shows, and `min-present`'s four-stage graph with stages 1 + 3
FUSED into one fragment pass when no colour chain is bound (stage 2 is a slot — the `ColorPass` trait — exercised by an
identity pass in a test, filled by OCIO / minColor in step 4; stage 4 composites nothing yet — the UI layer arrives with
`min-ui`). Colour metadata rides on every frame (row 39). Output modes: SDR (an sRGB surface), EDR (extended-linear
fp16, 1.0 = SDR white), PQ (BT.2100 PQ with S1's wgpu-hal workaround on Metal). Space = play / pause, F = fullscreen,
Esc = quit; `--stats` prints the evidence; `--snap-out` writes the display-encoded frame for the pixel-diff.
OUT of step 1: the scrub decoder, seeks, the second slot and the cut (step 2), audio, any UI beyond the menu bar.

**Pass bar (macOS, the same fixtures and display as S1 / S2; every row must hold):**
| Bar | Where it comes from | Number |
|---|---|---|
| open → first frame presented | S1 1b: 123–195 ms zero-copy (QCView 385–500) | ≤ 215 ms hw, ≤ 120 ms software (4K ProRes) |
| steady present interval | S1: p95 16.9–17.2, max ≤ 19.8 at 60 Hz | p95 ≤ 17.5 ms, max ≤ 2 refreshes |
| per-video-frame lateness, drops, cadence | S2 play: p95 < 1 refresh, 0 dropped, cadence 2/3 only | p95 ≤ 16.7 ms, 0 dropped, cadence ∈ {2, 3} |
| **GPU time per presented frame, 4K 4:2:2, no chain** | S1: 0.3–0.5 ms colour pass; the UNFUSED frame 1.9 ms p95 — the bar S1 missed | **≤ 1.0 ms p95** (Metal System Trace, all command buffers) |
| render-thread cost per frame (import + encode) | S1: 0.1 ms zero-copy | ≤ 0.3 ms p50 |
| CPU | S1: 10 % of a core mean | ≤ 25 % of a core (decode threads included) |
| footprint, 4K ProRes zero-copy | S1: 0.40 GB (no chain) | ≤ 0.5 GB max RSS |
| picture | S1 step 3: 55.9 dB / max 2 per 255 vs QCView's capture | bit-identical to S1's `--snap-out` of the same frame; ≤ 2/255 vs QCView's PNG |
| EDR + PQ | S1 step 3: headroom 1.0 → 8.5 on the XDR; PQ surface via the layer workaround | EDR engages (headroom > 1 reported while up); the PQ ramp fixture presents on the PQ surface; `--verify-pq` arithmetic unchanged (worst 7.2 % at code 0.065 — the fixture's) |
| image sequence from ONE frame | S2: the 72-frame 4K EXR plays at cadence | a single `.exr` path opens the sequence through `min_paths::sequence` (start number honoured) and plays with the rows above |
| the stage-2 slot | §11.2 / row 91 | an identity `ColorPass` through the three-pass route is pixel-identical to the fused route (unit test) |
| Windows | row 130 | builds + plays on rhombus (D3D12VA zero-copy, software fallback); pacing / EDR / PQ wait for the monitor |

**Time box.** One session for the crates + the app + the macOS numbers; the Windows build the same day.

**Crates (rewritten from the spikes, row 128):**
- `crates/min-present` (MIT OR Apache-2.0) — `Presenter` over a `wgpu::Surface` the app creates (toolkit-agnostic; a raw
  window handle only for the Metal PQ layer workaround), `Frame { pixels: Planar | Native, meta: ColorMeta }`, the fused
  stage 1+3 pipelines (planar / biplanar / planar-RGB-f16), the `ColorPass` slot (three-pass route), the held-frame ring,
  the per-OS zero-copy imports (`import/metal.rs`, `import/dx12.rs`), readback / snapshot helpers. No FFmpeg, no winit.
- `crates/qcv-player` (MIT OR Apache-2.0) — FFmpeg 9 RAII (`Decoder` + `HwDevice` shared across decoders; image-sequence
  open options: frame rate + start number), the streaming `Slot` thread with serial seeks, the `Player` (one slot, the
  rational clock, the ready map, loop at EOF, the lateness / cadence / drop statistics). Step 2 adds the scrub decoder,
  the second slot and the Transport.
- `crates/qcv-timeline` (MIT OR Apache-2.0) — `Rate`, `RationalTime`, `Clip`, `Timeline::{playlist, resolve}` from S2,
  with their tests; grows in step 2.
- `apps/qcv` (GPL-3.0-or-later) — the window (winit 0.31, the workspace's `winit31` pin), the menu bar (objc2), drop-in
  through `min-paths`, the evidence printer.

**Spikes after step 1:** none deleted yet. `s1-present` still owns the OCIO stage (step 4), `s2-scrub` the scrub decoder
(step 2), `s5-window` the DnD matrix (step 5). Each dies at the step that carries its last piece.

## Evidence (macOS, M5 Max, 2026-11-06 → 07)
**Built:** `crates/qcv-timeline` (4 tests), `crates/min-present` (engine + presenter + imports; 3 tests, one of them on the
GPU), `crates/qcv-player` (FFmpeg RAII, `HwDevice`, `Slot`, `Player`, `Source`), `apps/qcv` (winit 0.31 through the
workspace's `winit31` pin). 2.4k lines, all four build in release; `qcv --headless N` runs the player + the engine with
no window for the evidence that needs no display.

**A stage-1 inexactness found and fixed on the way (the GPU test caught it).** S1's shader applied the 8-bit limited-range
constants (`−16/255`, `×255/219`, chroma `−0.5`, `×255/224`) at every bit depth; the chroma neutral is `2^(d−1)/(2^d−1)`,
not 0.5, and the 10-bit luma scale is `1023/876`, not `255/219` — 10-bit white came out at 0.9969 and neutral grey carried
a +0.9/255 red tint. `min-present` derives the four constants per sample depth (`range_constants`, unit-tested at 8 and
10 bit). Consequences below: closer to QCView than the spike was, and the PQ arithmetic now matches the fixture's codes.

**Picture (frame 1 of the 4K ProRes HQ, display-encoded 8-bit, same sampler over every 2nd pixel):**
| Pair | max \|diff\| | PSNR | \|diff\| = 0 / 1 / 2 / 3 / 4 (of 6.2 M values) |
|---|---|---|---|
| **qcv (VideoToolbox zero-copy) vs QCView's `QCV_CAPTURE_PNG`** | 4/255 | **59.2 dB** | 5 745 634 / 473 147 / 1 918 / 94 / 7 |
| qcv (software + upload) vs QCView | 4/255 | 59.7 dB | 5 800 080 / 418 074 / 2 531 / 106 / 9 |
| qcv zero-copy vs qcv software | 1/255 | 59.6 dB | — |
| qcv vs S1's spike snapshot | 1/255 | 55.5 dB | the corrected constants |
| S1's spike vs QCView (same sampler, for scale) | 4/255 | 54.7 dB | 4 841 870 / 1 376 771 / 2 058 / 93 / 8 |
The crate agrees with QCView better than the spike did (59.2 vs 54.7 dB; 92 % of values identical vs 78 %); the ≤ 2/255 bar
holds for all but 101 of 6.2 M sampled values (the chroma-siting residual S1 described, now ≤ 4 codes in 0.002 % of samples).
The identity `ColorPass` route (stage 1 → working texture → blit) produces a **byte-identical PNG** to the fused route.

**PQ decode (the synthetic ramp fixture, HEVC Main10 BT.2020 PQ, frame 24, `--verify-pq`):** VideoToolbox and software give
IDENTICAL linear output. Against the fixture's ACTUAL luma codes (read with ffmpeg: the ramp is encoded 1–4 codes high, e.g.
v = 504 where 502 was intended, the 1000-nit band at v = 725 for 724): qcv's 102.15 / 1025.0 / 4143.8 nits at the three
bands vs 102.2 / 1025.4 / 4144.5 nits from PQ((v − 64)/876) — **exact to the codes**; the "worst 4.3 % at code 0.996" the
check reports is the fixture's own offset (S1's 7.2 % figure included the spike's constants). 1895 of 3840 ramp columns land
above 1.0 (EDR values), as in S1.

**Image sequence from one frame:** `qcv fixtures/exr_4k/frame_0010.exr` → `min_paths::sequence` → `frame_%04d.exr` with
`start_number` 1 → FFmpeg's exr decoder (software; the hwaccel is only wired when `avcodec_get_hw_config` lists our device
type for the codec) → planar RGB half, linear, through the planar-RGB branch of stage 1. Plays. (Note for step 2: FFmpeg's
frame-threaded exr decode holds many 66 MB frames — 4.5 GB max RSS headless; the image-sequence cache policy is step 2's.)

**Headless decode / show costs (48 frames, hw zero-copy):** show 0.13–0.23 ms p50 (S1 0.1), open → first frame on the engine
97–185 ms, CPU 11–14 % of a core at 4K; max RSS 4K ProRes 404 MB, H.264 4K 265 MB, HEVC10 4K 535 MB, 1080p 101–173 MB.

**Display rows (present pacing, lateness on the external display, the fused GPU figure, EDR engagement, the PQ surface):**
NOT YET MEASURED — both displays were asleep when the build landed (wgpu-hal reports the surface occluded while the NSWindow
is not visible, by design). The first windowed run before the screen slept showed the player side: 24.0 fps, cadence
{2: 148, 3: 87}, 0 dropped, 10 % of a core, show 0.17 ms p50. The rows run on the next awake screen (`tools/phase2/trace.sh`).

**Windows:** see below.
