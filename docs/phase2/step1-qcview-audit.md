# Phase 2 · step 1 — the QCView audit (2026-11-07, row 131)

Row 131 (ruled 2026-11-07): port QCView's basics, never reinvent them. Step 1 was rewritten from the spikes, and its two
defects were both wheels QCView had already built. This note holds the step-1 crates against QCView's player and Metal
renderer (`QCView-Player/src/decode`, `src/render/metal`, read 2026-11-07) and lists what to port, what diverges with a
reason, and what is step 2's. Nothing here is built yet; the ports are taken on a "go".

## What QCView has, and where step 1 stands
| QCView piece (file) | What it does | Step 1 | Disposition |
|---|---|---|---|
| **Publish slot + `fetchLatest`** (`video_decoder.h`) | latest-wins slot: at most 1 handle in the slot + 1 with the consumer; the decoder paces itself (`publishHandle` sleeps to `baselineWall + (pts − baselinePts)`), the renderer draws on `frameAvailable` | the streaming slot's bounded channel (3) + the player's `LOOKAHEAD` (3) + the presenter's ring (3); the **rational clock** picks the frame per refresh (S2) | **diverges with evidence**: S2 measured exact 3:2 cadence and 0 drops against P0's decoder-paced 17.4–20.5 ms p95 — keep the clock; the memory discipline is QCView's and is now in (`LOOKAHEAD`) |
| **`thread_policy.h`** | intra-only + slice-capable → `FF_THREAD_SLICE`; else `FRAME \| SLICE`; dual = half the cores; measured table in the header | a caller flag (`low_latency`), the default elsewhere | **PORT** verbatim into `Decoder::open` (`avcodec_descriptor_get` + `AV_CODEC_CAP_SLICE_THREADS`); cite the header |
| **`hw_routing.h`** | `softwareOnlyCodec(PRORES_RAW)` — no hwaccel session ever opened for RAW (VT returns null buffers; Vulkan output flipped) | `avcodec_get_hw_config` gate only | **PORT** the rule beside the gate |
| **`get_format` SW fallback + hw→CPU fallback** (`video_decoder.cpp`) | falls through to the first software format; logs the VT-then-CPU fallback once; the renderer keeps drawing | a software frame from a hw-opened decoder takes the planar path; an import error reopens in software | equivalent; keep |
| **`FrameHandle`** kinds (`frame_handle.h`) | Cpu RGBA · **CpuShared** (zero-copy view + keep-alive: saves 70 MB/frame on 4K EXR) · Metal · Vulkan · D3D11 · **CpuYuv** (planes, converted on the GPU, unclamped) | `Pixels::Planar` (= CpuYuv + planar RGB f16) · `Native` | CpuShared = row 46's hal wrap (step 2, image sequences); the rest equivalent |
| **`yuv_planar.h` + `yuv_planar_to_rgba`** (`metal_yuv_renderer.mm`) | ANY depth / alignment / subsampling: `codeMax, levelK, fullMax, chromaMid`; the matrix from **Kr / Kb** so 601 / 709 / 2020 / 240M / FCC all decode; bilinear chroma at the luma centre; **UNCLAMPED** output (a DNxHR HQX range chart read 100 % clamped where VT read 109 %) | per-depth constants (equivalent levels maths); three hard-coded matrices; `clamp(rgb, 0, 1)` | **PORT**: Kr / Kb matrix (adds 240M, FCC), **no clamp in stage 1** (clamp only in stage 3's SDR modes), alpha plane |
| **`effectiveMatrix` / `effectiveSourceRange`** (`sws_threaded.h`) | untagged → **BT.709 at every size** ("the GPU converters use the same rule"); RGB sources declare FULL and `rgb_range.h` owns the legal→full expansion (Auto: tagged limited → expand; the pill overrides) | untagged SD → 601 (my heuristic); RGB range ignored | **PORT** both rules (the SD heuristic was mine, not QCView's) |
| **Range pill** (`setRangeOverride`: Auto / Full / Limited, per clip) | re-decodes the current frame on change | not carried | step 2 (per-clip overrides on `ColorMeta`) |
| **`rgb_range.h` 16-bit publish rule** | sources deeper than 8 bits publish as UNORM16, never half (11-bit mantissa loses near white); RGBA16F stays with EXR | planes stay at native depth (`R16Unorm`), EXR `R16Float` | equivalent (better: no RGBA conversion at all) |
| **CV bridge format table** (`cv_pixbuf_metal_bridge.mm`) | 8 / 10 / **16-bit** biplanar 420 / 422 / 444, video + full range, **`4444AYpCbCr16` packed** (ProRes 4444 alpha), matrix + transfer from the buffer's attachments (`YCbCrMatrix_ITU_R_2020`, `SMPTE_ST_2084_PQ` / `HLG`) | 8 / 10-bit biplanar only; range from the fourcc; matrix/transfer from the stream tags | **PORT** the table (16-bit biplanar, the packed 4444 kernel, the attachment reads) — today ProRes 4444 falls back to software and the software path has no alpha / 12-bit yet |
| **Bridge release rule** | the previous frame's wrappers are released on the next `bridge()` (Qt orders the frames) | the ring of 3 (`desired_maximum_frame_latency` 2) | equivalent; keep (wgpu has no frame-order guarantee to lean on) |
| **`MetalUploadRing`** (CPU frames) | 3 shared-storage textures, a slot is reused only when its last command buffer completed; `replaceRegion`, no staging | `write_texture` into one texture set (wgpu stages + copies: the 6–14 ms blit the trace shows at 4K) | row 46 / step 2: the hal-wrapped shared buffers; the in-flight rule comes with it |
| **`MetalTexturePool`** (LRU, byte budget, 3-frame graveyard) + **`MetalGpuUploadThread`** (generation-tagged uploads off the render thread) | image sequences | — | step 2 (the image-sequence cache) |
| **`FrameIndex`** (`frame_index.h`) | tier 1 formula at open; tier 2 async packet scan → exact (pts, keyframe) table, `keyframePtsBefore` for direct keyframe seeks | the formula (`round((pts − start) · fps)`) | **PORT** in step 2 (seeks + scrub need the exact table) |
| **`ReadAhead`** | page-touch warming on network volumes, one utility-QoS thread, driven by read position | — | step 2 / 3 (the user reviews over LucidLink / SMB) |
| **`ScrubDecoder` + `ScrubFrameCache`** | latest-wins, GOP LRU in the cheap native form, deferred convert, intra = seek-exact; **no hw cache for pool-based backends** (D3D11 / Vulkan starve the decoder — only CoreVideo's pools grow) | S2's scrub decoder (adds the RETARGET rule S2 measured) | step 2; carry the pool-starvation rule onto D3D12VA (read back to planes before caching) |
| **`seek_compat.h`** (webp_anim reopen-to-seek), **`stream_extent.h`** (duration-less containers: count packets, size-gated) | | — | **PORT** into `Source` / `Decoder` (small, and the clip never opens without them) |
| **`DecoderCleanupQueue`** | slow teardowns (VT hwframes unref ≈ 50 ms) off the UI thread | the slot's decoder drops on its own thread (Quit) | equivalent by construction; keep |
| **`MetalHdrSwapchain`** | EDR modes on the layer; HDR10 → extended linear with a one-time warning | S1's PQ layer by name + EDR | superset; keep |
| **`TimecodeFormatter`**, source TC, drop-frame, `setStartTimecode` | | — | step 2 (the Transport) |
| **Dual** (`dual/`), live sources, the output bus | | — | later steps (§21.4 "out of the slice" / step 7) |

## The ports to make before step 2 builds on step 1 (proposed, taken on a "go")
1. `thread_policy` verbatim in `Decoder::open`; `softwareOnlyCodec` beside the hw gate.
2. Stage 1 from `yuv_planar.h`: Kr / Kb matrix (601 / 709 / 2020 / 240M / FCC), untagged → 709 at every size, unclamped
   output, the alpha plane; `rgb_range.h`'s legal→full rule for RGB sources.
3. The CV bridge's format table: 16-bit biplanar, the packed `4444AYpCbCr16` kernel, matrix / transfer from the buffer's
   attachments (over the stream tags) — and the software side's 12-bit / alpha planes so ProRes 4444 plays either way.
4. `seek_compat` + `stream_extent` in `Source` / `Decoder`.
5. Every ported module cites the QCView file in its doc comment; every divergence keeps its reason beside the code.
Then step 2 ports `FrameIndex`, `ReadAhead`, the scrub cache rules, the timecode formatter and the range pill as its own
work, with S2's additions (the rational clock, retarget, the two-slot cut) as the documented divergences.
