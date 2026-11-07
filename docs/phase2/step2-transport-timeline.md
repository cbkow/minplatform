# Phase 2 · step 2 — transport + timeline (planned 2026-11-07; rows 154–159 RULED 2026-11-07 "go, start with the scrub decoder"; the scrub decoder + the Transport + the timeline edits + the hover cache + qcv-meta LANDED 2026-11-07)

**Question.** Does QCView's Transport — J / K / L, seeks, scrubs, the two-slot playlist cut, shuttle, loop, in / out,
review speeds — run on `qcv-player` with S2's numbers, driven by `min-ui`'s FIRST nodes (the transport bar and the
timeline panel) over the presenter's composite, with `qcv-meta` feeding the timecode origin?

**What step 2 is (row 138's cut).**
- **The scrub decoder + cache** ported from `scrub_decoder.{h,cpp}` / `scrub_frame_cache` (latest-wins, intra =
  seek-exact, inter = GOP LRU in the cheap native form with deferred convert, forward-no-reseek within 90 frames) with
  S2's one measured addition (RETARGET instead of abandon when a newer target is ahead within reach) and QCView's pool
  rule (no hardware cache on pool-based backends: D3D12VA scrub entries are read back to planes before caching).
  Publishes into `VideoDecoder`'s slot (`publish_external`) so the renderer keeps one fetch path.
- **The Transport** ported from `WindowManager`'s transport slice + `TransportBar.qml` / `TimelineStatus.qml`
  behaviours: the constants (shuttle 2× doubling each second to 32× on a 33 ms tick, resets review speed, holds at the
  media edge, a final commit seek on release; review speeds 0.5 / 0.75 / 1 / 1.25 / 1.5 / 2 with ⇧R; the 50 ms playlist
  seek-and-resume debounce; loop default on; in / out off in playlist mode; the modal backdrop pauses and resumes) and
  the key map of row 143 for transport keys, through the shortcut registry (row 74) — the first registry client.
- **The timeline**: S2's model (`qcv-timeline`) + QCView's `timeline_controller` / `timeline_edits` / `timeline_flattener`
  / `playback_timer` ported (the edit set as undo commands — trim head / tail, slide, slip, split, delete, insert, move —
  gapless recompute, `snapTolPx` 12, Esc restores the drag snapshot, the metadata-arrival duration patch, WallClock vs
  FrameLocked), and the **hover-thumbnail cache** from `timeline_thumbnail_cache` (320×180, LRU 100, 3 workers idle / 1
  playing, prefetch stride 6, ≤ 64 uploads per frame, the clear-generation counter, nearest-cached on a miss, the 16 ms
  request throttle; through the colour chain once step 4 exists).
- **`qcv-meta`** (rows 132–134, 136): `VideoMetadata` / `AdobeMetadata` / `ImageSequenceData` and `ffmpeg_metadata_extractor`
  ported verbatim (the codec-flavour tables, SAR / rotation / range / HDR, all-streams audio, camera vendor), the
  `MetadataService` generations, the XMP reader over FFmpeg's `export_xmp` packet (exiftool's dozen keys), the timecode
  formatter; the MXF scanner (row 137) follows as 2d once the core is in. exiftool stays as the oracle.
- **`min-ui`'s first nodes** (row 156): the transport bar, the timecode well + go-to popup (the parley field from S3 =
  the first text input), the timeline panel's ruler / lanes / cache band / failed-frame ticks / in-out zone, tooltips —
  drawn by vello + parley in the one scene as stage 4's UI layer, with the theme tokens (`min-theme`, row 76) born here.
OUT: audio (2b), image-sequence layers (2c), the rails and the Inspector (3), the colour chain (4), drag and drop (5).

**Pass bar (macOS, the S2 fixtures and display; Windows headless + the monitor session):**
| Bar | Number |
|---|---|
| seeks (the scrub decoder answers, the stream resumes) | 4K ProRes ≤ 20 ms p50 (S2 15.9), 1080p ≤ 10 (7.4); H.264 fwd ≤ 2 ms / back ≤ 15 p95 (S2 1.3 / 13.5) |
| scrubs (one request per refresh) | ≤ 8 ms p50 4K ProRes (S2 5.6), ≤ 5 1080p (3.5); frames behind the pointer ≤ 4 |
| the cut (two slots, prewarmed) | 0 black, 0 duplicates, cadence 2 / 3 only, ProRes and mid-GOP H.264 (S2) |
| shuttle 2× → 32× | one new frame per refresh at every rate, lag = one refresh of clock travel (S2) |
| playback | lateness p95 ≤ 1 refresh, 0 dropped, cadence exact (step 1) |
| the H.264 first frame carried from step 1 | ≤ 60 ms (the scrub decoder answers the open; step 1: 229) |
| the Transport's constants | a scripted key battery (the winharness-style drivers) gives the same frame / state as Qt QCView for the same keys: step, shuttle ladder, speeds, loop, in / out, go-to |
| timeline edits | the eight edits round-trip through undo / redo on a 3-clip playlist; gapless after insert / move; the 12-px snap |
| hover thumbnails | ≤ 1 refresh to the nearest cached frame; the LRU never exceeds 100 |
| `qcv-meta` | `VideoMetadata` field-for-field equal to QCView's extractor on every fixture (`tools/probe-metadata` is the oracle); the XMP reader finds the keys exiftool finds on the masters |
| `min-ui` | the transport bar + timeline panel draw at ≤ 1 ms CPU per frame on the UI thread with AccessKit live; the go-to popup types through the S3 editor |
| Windows | the scrub decoder HARDWARE on D3D12VA for inter codecs (S2: 7.5 ms p50 vs 42.8 software), readback-to-planes before caching; the rest headless; pacing with the monitor |

**Time box.** One session for the decoder + Transport + timeline model and edits; one for `qcv-meta` + the first nodes;
the evidence note fills as each lands. **Spikes retired by step 2:** `s2-scrub` (once the scrub + cut numbers hold).

## Evidence

### 2 · piece 1 — the scrub decoder + GOP cache (landed 2026-11-07)

**What landed** (`crates/qcv-player`): `scrub_decoder.rs` ← QCView's `scrub_decoder.{h,cpp}` (own `Session` + hwaccel,
the intra-only hw gate with `hw_inter_scrub` lifting it, `intra_direct` for intra codecs, thread policy auto — never the
user's knob —, the worker's latest-wins target + condvar, skip-if-already-shown, the cache-invalidate flag the Range pill
sets, the three-way decision cache hit → forward-no-reseek within 90 → keyframe seek BACKWARD + flush, every decoded
frame cached, the target keyed exactly, `publishEntry` → `VideoDecoder::publish_external` through an `ExternalSink`);
`scrub_frame_cache.rs` ← `scrub_frame_cache.{h,cpp}` (Planar entries = one `av_frame_clone` ref, the plane repack
deferred to show time; Native entries = a VideoToolbox ref; pool backends read back to planes before caching);
`simple_lru.rs` ← `simple_lru.h` (byte budget, `get` touches / `peek` does not, never evicts the only entry; 1 test).
The one divergence, row 155's RETARGET: a newer target ahead within reach retargets the running fill (QCView finishes
the fill, then takes the latest). `Session` grew `receive_raw` / `release_raw` / `hw_readback` / `handle_from_avframe`
for it. The harness: `qcv --seeks N [--seek-gap ms]` (random targets, request → served → shown) and `--scrub a-b:ms`
(a pointer sweep, one request per refresh for 2 s, the slot drained each refresh; the sweep positions on `a` first);
`--scrub-hw` lifts the gate; measurement knobs `QCV_SCRUB_THREADS` / `QCV_SCRUB_CACHE_MB` / `QCV_SCRUB_SETTLE_MS` /
`QCV_SCRUB_ROWS` (per-seek rows + the fill's phase timers). Log: `fixtures/phase2/step2-scrub.log`.

**Numbers (macOS, headless engine, the stream decoder paused on VideoToolbox; 40 random seeks spaced 300 ms; the sweep
20→200 at 16.7 ms):**

| File | scrub decoder | seeks request → ready ms p50 / p95 (bar) | sweep request → ready p50 / p95, frames behind p95 (bar) | first frame via the scrub decoder (bar) |
|---|---|---|---|---|
| 4K ProRes 422 | VideoToolbox, intra-direct | **5.5 / 7.9** (≤ 20; S2 15.9) | **4.1 / 5.5**, 0 (≤ 8; S2 5.6) | 104 ms (VT's first-seek prime; the stream's own is 133) |
| 1080p ProRes 422 | VideoToolbox, intra-direct | **2.8 / 3.3** (≤ 10; S2 7.4) | **2.3 / 2.8**, 0 (≤ 5; S2 3.5) | 98 ms |
| 1080p ProRes, 5-min (7,343 frames) | VideoToolbox, intra-direct | **2.7 / 3.3** | **2.1 / 2.6**, 0 | 100 ms |
| 1080p ProRes 4444 + alpha (48 frames) | VideoToolbox, intra-direct | 4.8 / 6.1 | (the clip is shorter than the sweep) | 108 ms |
| 4K H.264 GOP 48 | software, frame threads | 106 / 166 (no bar; the software bound — see below) | **0.3 / 0.9**, 0 forward; backward 0.4 / 89, 16 behind (75 of 120 cache hits) | **17.5 ms** (≤ 60; step 1 carried 229) |
| 1080p H.264 GOP 48 | software | 53 / 79 | 0.1 / 0.2 forward; backward 0.3 / 6.8, 5 behind (104 hits) | 5.5 ms |
| 4K HEVC 10-bit PQ | software | 193 / 335 | 0.5 / 0.8, 0 | 41 ms |
| 4K H.264 with `--scrub-hw` (VideoToolbox for an inter codec) | VideoToolbox | 384 / 578 | 20 / 1,739 — 11 of 120 served, 314 frames behind | 175 ms |

Every ProRes bar is met with 2–4× to spare; the H.264 first frame is answered at 17.5 ms against the 60 ms bar (the stream's
VideoToolbox pipeline fill was step 1's 229). Forward sweeps on inter codecs are decode-once-per-GOP at 0.1–0.5 ms per request
(the frame-threaded decoder is already a frame ahead); backward sweeps are cache hits inside the GOP and a keyframe re-fill at
each GOP boundary (89 ms p95 on 4K — QCView's behaviour, S2's 13.5 p95 was on 1080p). `--scrub-hw` on H.264 proves QCView's
gate: VideoToolbox pays a flush + re-prime per seek (384 ms) and a 60 Hz sweep starves — `hwScrubInterCodecs` stays OFF on
Apple; Windows D3D12VA (S2: 7.5 ms) is the per-OS default and is owed its headless run.

**What the measurements found (the notes beside the code):**
- **H.264 random seeks are the software bound, not a regression.** Per seek: ~55 ms of frame-thread pipeline fill (a 2-frame
  prefix costs 59 ms) + ~3 ms per frame of GOP prefix at 16 threads; every ms is inside `avcodec_send_packet` (the fill's phase
  timers: receive / cache / publish ≈ 0). S2's spike on the same 40 targets today reads 72 ms p50 — but its H.264 rows go
  stale after the second seek (`answered in 0.0 ms (shown +1515)`: a previous round's result consumed 1.5 s later), so its
  statistic and the note's "76 / 135" are polluted; its two honest rows (51, 137 ms) match the port's (60, 110). The
  harness here is the reference from now on.
- **Back-to-back seeks pay the previous fill's in-flight frames**: `avcodec_flush_buffers` waits for all 16 frame threads
  (113 ms p50 at gap 0 vs 106 spaced). QCView has the same; a user's seeks are spaced. Not changed.
- **The cache is bounded as designed** (43 entries = 510 MB of 512 on 4K 8-bit — under a GOP; QCView's reality too). Holding
  pool frames grows FFmpeg's frame pool to cache + DPB + in-flight (≈ 1.0–1.3 GB RSS on 4K H.264 at 16 threads; 0.6 GB with
  the cache at 0); not the leak below.
- **A headless-engine leak, fixed in `min-present`**: `queue.write_texture` parks its copy + staging on the queue's
  pending-writes encoder, which only a `submit` flushes and a maintain reclaims; the windowed presenter submits every refresh,
  a headless engine showing without drawing never did → 4.1 GB after 120 software 4K shows (every earlier headless software
  number carried it). `Engine::headless` now sets `standalone`, and `show` ends with an empty submit + poll on that path:
  370 MB. The windowed path is untouched.
- **The feed's shape is QCView's** (`av_read_frame` → `avcodec_send_packet` → drain `avcodec_receive_frame` until EAGAIN;
  the send's return is not consulted in either). Kept as is (row 131).

### 2 · piece 2 — the Transport (landed 2026-11-07)

**What landed** (`crates/qcv-player`): `transport.rs` ← `WindowManager`'s transport slice (play / pause / toggle with the
user-intent flag, `seekToTime` / `seekToFrame` / `…AndResume` with the 50 ms playlist debounce as the seam, `stepFrames`
(pause first, clamp), start / end, the review speeds — ring 0.5 / 0.75 / 1 / 1.25 / 1.5 / 2, setter clamp 0.25–4,
reset to 1× on shuttle start —, loop (default ON) + in / out with the "I clears O when they'd cross" rule, the auto-enable
on a valid range, `pushInOutToTimer`'s out + 1 and the read-ahead hydrate rule, the fast-seek shuttle — 2× doubling per
second to 32× on a 33 ms tick, hold at the edge, the final commit seek on release —, begin / endModal, the loop wrap with
the 1-frame guard) plus `TimelinePanel.qml`'s scrub gesture (press pauses, moves preview through the scrub decoder, the
heavy seek on release, resume if the press interrupted play); `playback_timer.rs` ← `PlaybackTimer` verbatim (WallClock /
FrameLocked, fmod wraps inside [in, out) or the duration, speed clamp 0.0625–8, nearest-frame `currentFrame`; 2 tests);
`key_map.rs` ← the event filter's transport table (A / J, D / L hold with repeat ignored; Q / ←, E / → step with repeat;
Space / K / W / S; Home / End; I / O / ⇧I; V loop; R / ⇧R; M; ↑ / ↓; the shell's B / T / ⌘T / F / ⌘Z / ⌘⇧Z returned as
actions; 1 test). `AudioHooks` is the audio player's call-site trait in QCView's shape (hold / release / update / tempo /
shuttle grains / mute / volume) with `NoAudio` until 2b-audio. `Player` (step 1's thin clock) is retired; the qcv app's
windowed, headless and harness paths all run on the Transport, and its keys go through the map.

**The one structural note** (in the module doc): the streaming decoder is `Policy::Clocked` (step 1b), so the
`PlaybackTimer` LEADS video playback — each refresh fetches the frame due at the timer's nearest frame; a seek while
playing holds the integrator until the sought frame lands and re-seats the clock there (QCView's decoder re-baselines its
pacing on that frame); the loop wrap is the timer's with the decoder re-seated at the wrap frame; FrameLocked advances one
frame per refresh only when the decoder delivered it. QCView's single-video mode is decoder-leads with the timer mirrored;
its shuttle / dual / image-sequence paths are timer-leads already.

**Numbers (macOS; playback headless + windowed on the XDR, seeks via the harness with the stream's own landing now timed):**

| Check | Result |
|---|---|
| 4K ProRes playback through the Transport, headless 120 frames | first frame 111 ms, 0 dropped, 176 MB (step 1b: 133 ms, 251 MB) |
| 4K ProRes windowed 8 s | 24.3 fps shown, 0 dropped, 0 late, cadence {2: 118, 3: 70}, lateness vs the nearest-frame clock p95 −3.8 ms, 6 % CPU, 234 MB |
| the stream's own seek landed (`seek_land_ms`) | 4K ProRes **5.9 / 8.6 ms** p50 / p95 (S2's stream: 67.6 / 72.8 — slice threads + VT seek-exact now); 4K H.264 **339 / 520** (VideoToolbox after a flush, S2: 278 / 400); 1080p H.264 194 / 242 |
| the scrub decoder beside it | unchanged from 2a (ProRes 5.1 ms, H.264 97 ms, sweeps 0.3–4.3 ms) |
| seek correctness | 0 mismatches over 90 random seeks on three files (the gate takes the OLDEST post-seek frame; the first cut took the newest and landed one frame late on ProRes) |

### 2 · piece 3 — the timeline model + edits, and the hover-thumbnail cache (landed 2026-11-07)

**What landed.** `crates/qcv-timeline` ← `timeline_types.h` / `timeline_flattener.{h,cpp}` / `timeline_edits.{h,cpp}` /
`timeline_controller.{h,cpp}` carried onto RATIONAL time (row 83): `Clip` / `Track` / `EditPolicy` / `Timeline` with
frames at the timeline rate and source frames at the clip's rate; the flattener (visible clip at T on the topmost
visible track, per-track, audible clips with QCView's under-track rule, timeline → source, next / prev boundary, S2's
`resolve`); the eight edits as ops with inverses (row 84: trim head / tail, slide, slip, split, delete, insert, move);
the `Editor` = the controller's API (`snap_edge_index_at` = `snapEdgeIndexAtTime`, `recompute_contiguous_start_times`,
`patch_clip_duration_from_source` with its full-range-only rule, the live-preview + Esc-restore hooks, undo / redo with
QCView's labels, `load_single_media` / `load_playlist` with the gap clips, `set_rate`, `clear`; undo cleared on load);
row 84's policy ENFORCED (locked tracks refuse, source bounds clamp, overlaps refused outside playlists); 7 tests.
The timeline panel's `snapTolPx` 12 lives there as `SNAP_TOL_PX`. `crates/qcv-player/src/thumbnail_cache.rs` ←
`timeline_thumbnail_cache.{h,cpp}` + `video_image_loader.cpp`: keys (path, frame), 320 × 180, LRU 100, three workers
idle / one playing with the park-and-requeue, misses at the front / prefetch at the back (stride 6), nearest-cached and
numbered-sibling fallbacks, ≤ 64 adopts per drain, the clear generation; the loader trait with `VideoThumbLoader` =
QCView's (an own one-thread `FLAG2_FAST` session, seek BACKWARD + the frame nearest (f + 0.5) / fps, `SWS_POINT` to RGBA8,
the 8-bit legal expansion for RGB sources); stills come with `min-image` (2c); the textures are the caller's (the harness
makes wgpu textures on the headless engine). The 16 ms hover throttle and the 33 ms drain are constants beside the cache.

**A QCView bug found by the port's tests, filed in §17.6:** `MoveClipEdit::undo` moves the clip back to slot `from`, which
is right for forward moves and wrong for backward ones (the clip then sits BEFORE its old slot, so the old slot is
`from + 1`); a two-clip playlist with the second moved to the front never undoes at all (the target is its own slot + 1, the
no-op case). Fixed in the port with the reason beside it.

**Numbers (`qcv --hover 20`: random hover frames at the 16 ms throttle, the drain every 33 ms; then the same frames; then
the whole file at stride 6):**

| File | hover miss → thumbnail landed ms p50 / p95 | re-hover hit | prefetch, 3 workers | playing, 1 worker (121 frames) |
|---|---|---|---|---|
| 4K ProRes 422 | 132 / 133 | 0.000 ms | 56 ms per thumbnail | 12.9 s |
| 1080p ProRes 422 (5-min) | **34 / 34** | 0.001 ms | 23 ms per thumbnail (1,222 thumbs, 1,162 evictions at the 100 budget, 135 MB) | 2.3 s |
| 4K H.264 GOP 48 | 628 / 1,090 | 0.001 ms | 192 ms per thumbnail | > 60 s (timed out) |

The bar "a hover answers within one refresh" holds once cached (a hit is a map lookup). A MISS costs one thumbnail decode
on QCView's loader design — single-threaded, seek-exact — which is 34 ms on 1080p ProRes and 0.6 s on 4K H.264 (a GOP
prefix at one thread). That is QCView's behaviour today; the obvious improvement — serve hover thumbnails from the scrub
decoder's GOP cache (the frames are already decoded) or give the loader frame threads for inter codecs — is a discussion
for the UI step, not a change here (row 131).

### 2 · piece 4 — `qcv-meta` (landed 2026-11-07)

**What landed.** `crates/qcv-meta` (MIT OR Apache-2.0, the family crate of row 132): `VideoMetadata` / `AdobeMetadata` /
`ImageSequenceData` as QCView's `media_item.h` has them; `extract` / `probe_rotation` / `probe_container` /
`probe_duration` ← `ffmpeg_metadata_extractor.cpp` verbatim (the colour-tag tables, depth / alpha from the pixel-format
name, the display-matrix snap, the embedded-timecode walk over every stream, the ProRes fourcc → marketing names with the
MXF profile fallback, the container family on `major_brand`, the MXF OP from the UL bytes, the authoring-tool assembly,
the DNxHD / DNxHR CID tables and the fixed-frame bitrate, the all-streams audio rule with the "N mono tracks" label, HDR10
side data, the container-range capture between open and find_stream_info, the packet-count extent for duration-less
containers — `stream_extent` now lives here and `qcv-player` takes it from here); `xmp` ← `adobe_metadata_extractor.cpp`'s
dozen keys read from FFmpeg (row 134: the `mov` demuxer's `export_xmp` packet through a quick-xml walk — Adobe writes the
structs as EMPTY elements with attribute-form fields, the Premiere link under `creatorAtom:windowsAtom`, the Pantry /
History / Ingredients copies skipped, first wins — plus the container's `creation_time` in exiftool's date form and the
tmcd / MXF `timecode`); `timecode` ← `TimecodeFormatter` verbatim (row 136; here so ufb / minNotes get it, re-exported by
`qcv-player`; drop-frame round-trips tested across the 10-minute cycle); `service` ← `MetadataService` on a thread pool
with row 132's latest-wins generations; `probe-metadata` (the bin) prints every field and runs exiftool beside it.
Tests: 4 (tables + DNx header, XMP shapes, timecode, service compiles). The C ABI arrives with the first non-Rust consumer.

**Parity.** Fixtures: `VideoMetadata` agrees with ffprobe's raw facts on 4K ProRes (profile HQ → "ProRes 422 HQ",
bt709 × 3, limited, 683 frames, the stream and container bitrates, the tmcd timecode, "stereo" PCM), 1080p H.264 ("High",
container range limited), 4K HEVC 10-bit ("Main 10", bt2020nc), ProRes 4444 + alpha ("ProRes 4444", yuva444p12le, 12-bit,
alpha). The three client masters (local only), our reader vs exiftool's `-s` output for `AdobeMetadataExtractor`'s tag list:
**every key exiftool finds, we find EQUAL** — WindowsAtomUncProjectPath + StartTimecode + MediaCreateDate on the two
Premiere renders, MediaCreateDate alone on the third (no XMP there, as exiftool agrees); we additionally read the tmcd
timecode and `altTimecode` exiftool's tag list left out. The Perl bundle can go for MOV / MP4 (it stays as the MXF oracle,
row 137). Logs under `fixtures/phase2/step2-meta-*.log` (gitignored: project paths are client data).

**Owed from piece 4**: an After Effects render for `aeProjectLink:fullPath` (none among the local masters), the MXF scanner
(2d), `qcv-meta-ffi`, the Transport's use of the timecode origin (the UI's timecode well, with `min-ui`).

**Owed from piece 3**: the playlist slot swap behind the Transport's debounce (rows 85 / 157: two decoder slots + the
next-clip prewarm — the next piece after `qcv-meta`), the timeline SURFACE (`min-ui`, row 86), still-image loaders (2c).

**Owed from piece 2**: the stream's resume on the playlist path (the timeline step brings the clip resolve + slot swap behind
the debounce), the audio hooks' implementation (2b-audio), the shell's actions (B / T / F / ⌘Z) wired when the shell exists,
`loopEnabled` persistence with the settings model (step 3), and a windowed key walk on the user's desk (the harness can't press keys).

**Owed from piece 1**: the Windows headless run (D3D12VA scrub for inter codecs + the readback-to-planes entries), the stream's
resume-after-seek measurement (that is the Transport's `seekToTime` shape — it lands with 2b's Transport port), and the
S2 spike's retirement once the cut numbers hold.

(as it lands)
