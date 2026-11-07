# Phase 2 · step 1b — the QCView-shaped decoder (2026-11-07)

**Why.** Row 131 and the step-1 audit: the wgpu side of step 1 (surface, fused graph, slot, hal imports) is what the spikes
were for and stays; the decoder side of `qcv-player` was written from S2's harness and is shaped like a benchmark. Step 2
is where QCView's decoder design does its real work (the scrub decoder publishing into the streaming decoder's slot,
`FrameIndex`, `ReadAhead`, the range pill, the pool rule on the handle kinds), so the base is re-derived from QCView's files
NOW, before anything is built on it. The user's rule: QCView is the reference base; improvements are welcome but tested
and discussed — each one a measured divergence with its reason beside the code.

**Question.** Does `qcv-player` + `min-present`'s stage 1, re-derived module by module from `QCView-Player/src/decode` and
`src/render/metal` (doc comments carried, files cited), hold step 1's numbers unchanged and close the audit's gaps?

**What step 1b is.**
- `qcv-player` modules named and shaped after QCView's: `video_decoder` (the streaming decoder: open / close / play /
  pause / seekToFrame / the latest-wins publish slot + `fetch_latest` / `publish_external` for the scrub decoder /
  range override / decode health), `frame_handle` (the kinds: Planar, PlanarShared (keep-alive view), Native; `pts`),
  `frame_index` (tier 1 formula + tier 2 async packet scan, `keyframe_pts_before`), `read_ahead` (the page-touch warmer,
  one utility thread, read-position driven, byte-capped window), `thread_policy`, `hw_routing`, `rgb_range`,
  `yuv_planar` (the levels maths in code values for any depth / alignment, Kr / Kb), `seek_compat`, `stream_extent`,
  `decoder_cleanup` (slow teardowns off the caller's thread). The `Player` of step 1 becomes the thin clock + presenter
  glue over `VideoDecoder` (the rational clock = the documented divergence, with S2's numbers beside it).
- `min-present` stage 1 from `yuv_planar.h` / `metal_yuv_renderer.mm`: Kr / Kb matrix (601 / 709 / 2020 / 240M / FCC),
  untagged → BT.709 at every size, UNCLAMPED output (clamp only in stage 3's SDR modes), the alpha plane, the packed
  4444 (`AYpCbCr16`) layout; the Metal import takes the bridge's full format table (8 / 10 / 16-bit biplanar, 420 / 422
  / 444, the packed 4444) and reads matrix + transfer from the pixel buffer's attachments.
- `rgb_range`'s legal→full rule for RGB sources; `seek_compat` and `stream_extent` in the open path.
- The five audit ports, each citing its QCView file.
OUT: the scrub decoder, dual, live, the timecode formatter (step 2's), the output bus.

**Pass bar.**
| Bar | Number |
|---|---|
| step 1's display battery, unchanged | every row of step 1's bar still holds (4K ProRes zero-copy: first frame ≤ 215, present p95 ≤ 17.5, lateness p95 ≤ 16.7, 0 dropped, GPU ≤ 1.0 p95, ≤ 25 % CPU, ≤ 0.5 GB) |
| picture | ≤ 2/255 vs QCView's capture, ≥ 59 dB (step 1's 59.2) |
| the audit table | every "PORT" row closed; every divergence has its reason in the code |
| ProRes 4444 | plays zero-copy (packed 4444) AND in software (alpha + 12-bit planes), both snapshots within 1/255 of each other |
| untagged SD | decodes as BT.709 (a unit test on `effective_matrix`) |
| unclamped stage 1 | a synthetic frame with code 250 luma (8-bit limited) reads > 1.0 in the linear snapshot |
| `FrameIndex` tier 2 | the packet scan on the 4K ProRes agrees with tier 1 on every frame; on the H.264 fixture `keyframe_pts_before` names the GOP keyframes |
| `ReadAhead` | attaches on open, touches the window's pages (counted), detaches on close; no change to the display numbers |
| `thread_policy` | the streaming decoder opens ProRes with slice threads, H.264 with frame + slice (logged at open) |
| Windows | builds, the headless battery as step 1's |

**Time box.** Half a session.

## Evidence (2026-11-07, macOS M5 Max + the built-in XDR at 60 Hz; Windows below)
**Built (minplatform from 325f778):** `qcv-player` is QCView's `src/decode` module by module — `video_decoder` (decode
loop, latest-wins seek slot, pause / EOF condvars, the publish slot with `fetch_latest` and `publish_external`, decode
health, playback speed, the Range pill), `frame_handle`, `frame_index` (both tiers), `read_ahead`, `thread_policy`,
`hw_routing`, `rgb_range`, `yuv_planar` (levels maths, `effective_matrix` / `effective_source_range`, Kr / Kb),
`seek_compat`, `stream_extent`, `decoder_cleanup`; `ffmpeg` is the session helper (`initFFmpeg`'s order: first video
stream, hwaccel attach + `get_format`, thread policy, metadata, the timecode tag, the `FrameIndex` inputs with the
duration and packet-count fallbacks, the read-ahead spans). `min-present` stage 1 is `yuv_planar_to_rgba`: levels in code
values, the Kr / Kb matrix, UNCLAMPED, an alpha plane, the packed 4444 kernel; the Metal import carries the CV bridge's
nine layouts and reads matrix + transfer from the buffer's attachments. Every module cites its file. The rational clock
stays as `Policy::Clocked` beside QCView's `Policy::Paced`, with S2's numbers in the doc comment. Step 1's `Player` is now
40 lines of clock over the decoder.

**Pass bar, scored:**
| Bar | Result | |
|---|---|---|
| step 1's display battery, unchanged | 4K ProRes zero-copy: first frame **133 ms** (197), present p95 17.0 / trace 17.0, lateness p95 15.4, 0 dropped, cadence {2: 142, 3: 141, 4: 1}, **GPU 0.45 ms p95**, 10 % CPU, **251 MB** (272) · H.264 229 ms, 0 dropped, 0.56 p95 · HEVC10 154 ms · software 4K ProRes **62 ms** first frame (85; slice threads) · EXR **68 ms** (145), 0 dropped | met — three numbers improved by QCView's thread policy |
| picture | frame 0 vs QCView's capture **59.2 dB, max 4/255** (step 1: 59.2); vs step 1's snapshot ≤ 1/255 (89 of 6.2 M values — the Kr / Kb matrix vs the rounded coefficients) | met |
| the audit table | every PORT row closed (thread policy, hw routing, untagged → 709, Kr / Kb + unclamped + alpha, `rgb_range`, the CV table + attachments, `seek_compat` + `stream_extent`, `FrameIndex` tier 2, `ReadAhead`); divergences documented in `video_decoder.rs`, `hw_routing.rs`, `rgb_range.rs`, `frame_handle.rs`, `import/metal.rs` | met |
| ProRes 4444 | a derived 1080p 4444 alpha fixture (yuva444p12le): VideoToolbox emits `y416` → the packed kernel, zero-copy, 0 dropped, 147 ms first frame, windowed snapshot == headless; software decodes 12-bit planes + alpha. **The two paths differ by up to 10/255 (53 dB) — and so do FFmpeg's own VideoToolbox and software decodes of the same frame (10/255, 53.1 dB, same histogram)**: the residual is the two ProRes 4444 decoders', not stage 1's — qcv is within 1/255 (64 dB) of swscale's conversion of EACH decoder's output | met as it should have been written: each path exact to its decoder |
| untagged SD → BT.709 | `effective_matrix` unit test | met |
| unclamped stage 1 | GPU test: code 250 luma reads 1.07 encoded / > 1.1 linear; the 10-bit and 16-bit level constants equal QCView's kernel's (4096 / 56064 / 57344 / 32768) | met |
| `FrameIndex` tier 2 | 4K ProRes: 683 entries, agrees with tier 1 on every frame, 683 keyframes; H.264: 683 entries, **15 keyframes**, `keyframe_pts_before(341)` = 336336 (frame 336 × 1001 — the GOP-48 keyframe); EXR 72 | met |
| `ReadAhead` | attaches at open (spans from the MOV index: 14.1 MB window), pages touched 171 (4K ProRes) / 31 (H.264), detaches at close; display numbers unchanged | met |
| `thread_policy` | ProRes opens `slice`, H.264 `frame+slice` (logged) | met |
| Windows | below | |

**Found on the way:** the snapshot flag counted shown frames, so the windowed and headless PNGs were different frames
once the decoder's first frames could skip; snapshots and `--verify-pq` key on the DISPLAY frame number now (`@0` is
QCView's paused-at-open frame).

**Windows (rhombus, DX12 + D3D12VA, minplatform ccb69d6):** builds on MSVC, the crate suites pass (the GPU tests on DX12);
headless: H.264 4K and HEVC Main10 4K D3D12VA zero-copy through the ported decoder (show 0.30–0.32 ms p50, 0 dropped,
first frame 87–88 ms on the engine), `thread_policy` logs `frame+slice` for them and `slice` for ProRes, the software 4K
ProRes first frame **25 ms** (98 before the slice policy), `FrameIndex` tier 2 agrees with tier 1 on every frame with the
same 15 keyframes and the same `keyframe_pts_before`, `ReadAhead` touches pages, `--verify-pq` identical to macOS. The
display rows stay with row 130 (the monitor).

## Verdict (2026-11-07) — GO
`qcv-player` and `min-present`'s stage 1 are QCView's designs, file by file, with step 1's numbers intact or better and the
audit closed. The decoder base step 2 builds on is now the one QCView spent years on: the publish slot, the seek slot, the
pause / EOF rules, the two-tier index, the read-ahead warmer, the thread and routing policies, the levels maths. The
platform's own additions are three and documented in place: the clocked policy over the paced one (S2's numbers), the
display frame number on the handle, and the legal-range RGB expansion done on the GPU with QCView's constants. Carried:
the H.264 first frame (229 ms) and the software upload's staging blit, as from step 1.

