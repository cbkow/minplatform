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

**Time box.** Half a session. **Evidence** below as it lands.
