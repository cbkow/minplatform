# Phase 2 · step 2 — transport + timeline (planned 2026-11-07; rows 154–159 RULED 2026-11-07 "go, start with the scrub decoder"; the scrub decoder + the Transport + the timeline edits + the hover cache + qcv-meta + the playlist Sequencer LANDED 2026-11-07; S2 retired)

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

### 2 · piece 5 — the Sequencer: playlists on two decoder slots (landed 2026-11-07)

**What landed.** `Transport` is row 84's ONE Sequencer: every source is a timeline (single media = `Editor::load_single_media`'s
one locked clip; a playlist = `load_playlist`'s), the clock runs in TIMELINE frames, and each refresh resolves the frame to
(clip, source frame) through `qcv-timeline` and fetches from that clip's SLOT. Two slots (row 85, S2's gapless pair): the
active clip's and the next non-gap clip's, prewarmed on its in-point on a thread (`Slot::open` + the in-point seek; the
tick never stalls), swapped in at the cut with its in-point frame already due; a reused slot (the loop's return to a clip
still open) re-parks on its in-point. QCView's rules carried: `applyPlaylistSeek` (the 50 ms debounce → the clip containing
the position, past the end → the last non-gap, inside a gap → the next else the previous, swap, the decoder seek, the
deferred resume), `playlistAdvanceToClip`'s next-non-gap + loop-to-first, the end-of-playlist stop when not looping,
in / out off in playlist mode, the shuttle's "no per-tick decoder seeks on playlists" (the swap is the one expensive hit,
deferred to the release), `audioPosition`'s clip translation for the audio hooks. The one divergence stays row 85's:
QCView reopens its single decoder at every clip change; the port's prewarm removes the frame-1 flash and the boundary
stall S2 measured. `--cut a@in-out,b@in-out[,…] [--loop]` in the qcv app plays a playlist at a 60 Hz cadence and reports
the boundary refresh gaps, the wrap, and 12 random playlist seeks.

**The seek gate learned two rules from the playlist tests** (both in the code): the landing is the oldest post-seek frame
AT OR PAST the target (a prewarmed slot's in-point frame, below it, is dropped — the first cut re-seated the clock on the
in-point), and the "landing consumed by a scrub publish" fallback requires a publish since the seek was issued (the
decoder's `published_seq`).

**Numbers (macOS, headless engine at a 60 Hz refresh cadence, VideoToolbox):**

| Playlist | cuts | wrap (`--loop`) | 12 random seeks (incl. the 50 ms debounce) |
|---|---|---|---|
| 1080p ProRes 0–48 → H.264 100–148 → ProRes 4444 0–48 | 0 dropped, 0 black, cadence {2: 70, 3: 71}; both boundaries 2–3 refreshes apart, swaps prewarmed 2 / opened 0 | — | 9 cross-clip: landed p50 143 ms, p95 232 (the H.264 clip's VideoToolbox landing); prewarmed 9 / opened 2 |
| 1080p ProRes 0–24 → H.264 100–124, looping | gapless | frame 47 → frame 0 in ONE refresh (the landing 11.5 ms), 2 wraps, 0 dropped | — |
| 4K ProRes 0–96 → H.264 200–296 | 0 dropped, 0 black, cadence {2: 94, 3: 96}; boundary 2 refreshes apart | — | 6 cross-clip: p50 60 ms, p95 609 (4K H.264 VideoToolbox after a flush, as single media: 339 / 520) |
| 4K H.264 200–248 → ProRes 0–48 → HEVC 10-bit 100–148 | 0 dropped, 0 black, cadence {2: 69, 3: 72}; both boundaries 2 refreshes apart | — | — |

S2's bar ("the cut shows no black and no duplicated frame") holds on every pair, including H.264 in-points mid-GOP and
the 10-bit HEVC; single media is unchanged by the refactor (the same 111 ms first frame, 0 dropped, the same seek and
scrub numbers). **S2's spike is retired with this piece** (`spikes/s2-scrub` removed; `docs/spikes/S2-scrub.md` stays the
record).

**Owed from piece 5**: image-sequence clips in playlists (2c: the slot is a `Source::Sequence`, the cache is its decoder),
the per-clip range override from the project (step 3), the playlist's audio through the hooks (2b).

**Owed from piece 4**: an After Effects render for `aeProjectLink:fullPath` (none among the local masters), the MXF scanner
(2d), `qcv-meta-ffi`, the Transport's use of the timecode origin (the UI's timecode well, with `min-ui`).

**Hover for the MOUSE (added 2026-11-07, after the user's point that most hovering is far from the playhead).** Four
measured changes beside QCView's design, each with its reason in the code: the hover miss is ONE latest-wins slot (QCView
queued every miss at the front and never dropped one); `precache_strip` on open thumbnails the whole file every
`frames / 400` frames (floor 6) with the budget at 500 entries (115 MB, inside QCView's 256 MB pool) so every hover shows
the nearest frame through QCView's own fallback while the exact one loads; the video loader runs slice threads for intra
codecs and frame threads at half the cores for inter ones (QCView: one thread); and the Transport's `hover_thumbnail`
asks the clip's scrub-decoder GOP cache first (`ScrubDecoder::peek_thumbnail`: a `peek`, never a decode, swscale on the
caller's thread). `qcv --hover` now sweeps the whole timeline in 2 s at the 16 ms throttle, rests, and then drags 100–190
and hovers the span. `QCV_HOVER_QCVIEW=1` runs QCView's budget without the strip for the comparison (the loader's threads
are global, so its miss numbers are already the threaded ones; the earlier table has the one-thread figures).

| File | miss → thumbnail, one thread → threaded | strip on open | sweep: nearest shown, frames away p50 / p95 (QCView's budget) | exact lands after the mouse rests | GOP-cache peek after a drag |
|---|---|---|---|---|---|
| 4K H.264 | 628 → **101 ms** | 114 thumbs in 7.6 s | **2 / 3** (73 / 109), 0 unanswered | 0 ms (141) | 0.4 ms where held (the cache keeps 43 frames of 4K) |
| 4K ProRes | 132 → **34 ms** | 114 in 1.7 s | **2 / 3** (8 / 17) | 0 ms | none: intra-direct caches nothing, by design; the loader is 34 ms |
| 1080p H.264 | 67 ms (threaded) | 120 in 3.0 s | **2 / 3** (19 / 33) | 6 ms | 0.2 ms where held |
| 1080p ProRes, 5-min | 35 ms | 408 in 1.4 s | — | — | — |

The cost: the threaded loaders' peak RSS during the strip (2.4 GB on 4K H.264 with three workers at eight threads, 0.6 GB
for QCView's one-thread loader); a per-worker thread cap is the knob if that matters on the Windows box. Log:
`fixtures/phase2/step2-hover.log`.

**Owed from piece 3**: the playlist slot swap behind the Transport's debounce (rows 85 / 157: two decoder slots + the
next-clip prewarm — the next piece after `qcv-meta`), the timeline SURFACE (`min-ui`, row 86), still-image loaders (2c).

**Owed from piece 2**: the stream's resume on the playlist path (the timeline step brings the clip resolve + slot swap behind
the debounce), the audio hooks' implementation (2b-audio), the shell's actions (B / T / F / ⌘Z) wired when the shell exists,
`loopEnabled` persistence with the settings model (step 3), and a windowed key walk on the user's desk (the harness can't press keys).

**Owed from piece 1**: the Windows headless run (D3D12VA scrub for inter codecs + the readback-to-planes entries), the stream's
resume-after-seek measurement (that is the Transport's `seekToTime` shape — it lands with 2b's Transport port), and the
S2 spike's retirement once the cut numbers hold.

(as it lands)

### 2 · piece 6 — `min-ui`'s first nodes (designed 2026-11-07; not built)
Talked through before building (row 156): the design direction is canon §25 — Spectrum 2's token structure and scales
with Geist + Phosphor, Spectrum blue rationed to four uses, the timeline as a 32-px scrubber with the waveform as a faded
fill, dual and playlists as clip bars under it with no edit mode. Build order and the harness bar are in §25's closing
paragraph; `min-theme` is first.

#### 6a · `min-theme` (landed 2026-11-07)
`crates/min-theme` — the tokens as data in Spectrum 2's three-tier JSON shape (`tokens/color-palette.json` the 13-step
gray ramp + 8 hue ramps, `color-aliases.json` the roles, `layout.json`, `typography.json`, `component.json` for the
first nodes), one extension over Spectrum's format (`"alpha"` on a colour reference — the hover-ramp twin rule), and a
`build.rs` that resolves every reference (dangling / cyclic → a build error by name) into one typed `pub const` per
token plus `tokens::ALL` / `tokens::get`. 396 tokens resolved. Also the Phosphor slug → codepoint table (QCView's
`PhosphorIcons.js`, 1 512 names, `icons::glyph`) and the six bundled faces (Geist ×5 OFL, Phosphor MIT; the family
names inside the files verified: "Geist", "Geist Mono", "Phosphor") behind the default `fonts` feature.
`min-theme-dump [--json] [prefix]` is the parity oracle for the later `Theme.qml` generator.
**Tests (7):** the contrast roles hold on all three layers (content ≥ 7:1, subdued ≥ 4.5:1, disabled < 3:1), white on
the accent fill ≥ 4.5:1 (cobalt was 3.6:1), inverted text on selected / tooltip fills ≥ 10:1, the alpha extension,
Spectrum's desktop scales, colour maths, the icon table, the font files. Values of note (row 168's picks): accent
blue-800 #4069fd / blue-900 #5681ff; playhead gray-900; in = green-900, out = orange-900, failed = red-900; side A =
cyan-900, side B = cinnamon-900; note marker purple-900; waveform gray-800 at 0.12; in/out zone white at 0.06.

#### 6b · the UI layer: stage 4 in `min-present`, `min-ui`'s `UiLayer` + `Fonts`, the token sketch (landed 2026-11-07)
- **Stage 4** (`engine.rs` `set_overlay`, `shaders/overlay.wgsl`): the overlay is vello's output — an `Rgba8Unorm`
  texture at the viewport size, sRGB-encoded with STRAIGHT alpha (vello's fine stage stores separated alpha; read in
  `fine.wgsl`). Each texel is decoded to linear light, placed at the display's SDR reference white (UI white = 1.0 =
  `white` nits, so chrome never blazes on an EDR / PQ surface), encoded as stage 3 encoded the video (EDR linear, PQ,
  SDR linear, sRGB), premultiplied, and blended One / OneMinusSrcAlpha over the finished video in a `LoadOp::Load`
  pass. Exact where alpha is 0 or 1; anti-aliased edges over a non-linear target blend in the encoded space, as every
  UI toolkit does. The engine's `encode` is now video → overlay; the headless `render_offscreen` composites too.
  **Test** `overlay_composites_at_sdr_white_in_linear_light`: white at α .5 → .502 linear, sRGB 128 → .214 linear,
  α 0 → nothing, and the same texels pass through unchanged in `SdrEncode`.
- **`crates/min-ui`** (GPL, like the apps): `UiLayer` (vello 0.11 `Renderer`, area AA only, one `Scene`, the target
  texture with STORAGE | TEXTURE | COPY_SRC, `resize`, `scene_mut` marks dirty, `render` only when dirty and returns
  the CPU ms), `Fonts` (parley 0.12 `FontContext` with min-theme's six faces registered from bytes; the family names
  read back from fontique equal `min_theme::fonts::FAMILY_*` — tested; `layout` / `layout_width` single-style layouts
  at line-height 1.3, `icon(slug)` through the Phosphor table, `draw_layout` = S3's glyph loop with hinting), `paint`
  (rect / rounded / hline in theme colours; `corner-radius-full`'s 0.5 → half the short side).
- **`apps/qcv`**: the layer at the surface's physical size on the presenter's device, handed to `set_overlay` (again
  after a resize); `ui_sketch.rs` draws the three bands STRAIGHT FROM THE TOKENS — status strip 24 (the two recessed
  wells in Geist Mono 12, fps right), transport row 32 (QCView's button order as 32-px action buttons with 20-px
  Phosphor glyphs, spacing instead of dividers, loop shown in the neutral selected state, unbacked buttons disabled,
  the pin flush right), scrubber 32 (ticks at QCView's ≥ 80-px rule with mono 11 labels, frame ticks at ≥ 6 px/frame,
  the waveform as a faded gray fill — synthetic until piece 6c reads the probe memo — a sample in/out zone with the
  positive / notice flags, the 2-px cache underline, the near-white playhead at the current frame). Rebuilt when the
  frame number or the size changes. `QCV_NO_UI=1` leaves the layer out. **`--ui-bench N [--ui-snap PATH] [file]`** is
  the harness: headless engine + layer at 2560×1440 @2, N rebuilds at a moving frame, the composite as an sRGB PNG.
- **Numbers (M5 Max):** headless — scene build 0.06 ms p50 / 0.08 p95 (5.6 on the first build: font loading), vello
  render 0.14 ms p50 / 0.24 p95 (CPU, encode + submit), a clean layer renders nothing (asserted), fonts register in
  25 ms, composite + readback 24 ms, RSS 209 MB. In the window during 1080p ProRes playback (the timecode changes every
  frame, so the layer re-renders every frame): ui render 0.8–1.3 ms p50 / 2.2 max against the shared queue; present
  cadence unchanged (16.7 p50, 0 dropped), process 19 % of one core with decode. **The bar (≤ 1 ms when the layer
  changes, nothing when it doesn't) is met headless and borderline in-window; the per-frame cost is vello's fixed
  submit on a busy queue, not the scene — piece 6c will measure whether re-rendering only every other frame during
  playback is needed, or the readouts should live in a smaller layer.**
- **Seen in the snap** (`fixtures/phase2/ui-layer.png`, client footage, gitignored): the bands read as §25 ruled them —
  layer-1 bands over the base, hairlines, soft wells, 20-px glyphs, the gray selected loop, the faded waveform, the
  green / amber flags, the white playhead. **Known gap:** the video still fits the WHOLE viewport, so the bands cover
  its bottom 88 px — the engine's fit needs a content rect (the shell's job in piece 6c, with the first real layout).

#### 6c · the first nodes: ActionButton, ActionGroup, Tooltip — the transport bar live (landed 2026-11-07)
The transport row was held against Spectrum CSS's `spectrum-two` action-button theme and the S2 token data (fetched, not
remembered): 32 / icon 20 (`workflow-icon-size-100` — my ramp had 18, corrected to S2's 16 / 20 / 22 / 26) / edge 6 /
radius 8 / no border matched; the gaps did not (2 → 8 in a group, 12 → 16 between clusters) nor hover (gray-100 →
gray-200). The user's rulings on top: the transport cluster as a COMPACT fused group (gap 0, the radius only on the two
ends), the tooltip in S2's softer gray-500 with white text, and **"more deliberate greys than inverts"** — the selected
state is gray-300 with a gray-900 glyph (hover gray-400, down gray-500), not S2's inverted gray-800. Down stays one step
past hover (gray-300; S2 has no pressed step — a written deviation, §16.4's contract).
- **`min-ui`**: `input::Event` (Move / Down / Up / Leave in physical px), `anim::Ramp` (ease-out toward a target over
  the token duration, reversals start where they are) + `mix`, `ActionButton` (icon, optional label, tooltip, enabled /
  selected / emphasized / hold / focused; per-corner radii; click = release inside, a `hold` button reports press begin /
  end and ends on leaving — QCView's shuttle rule; the focus ring 2 px + 2 px gap), `ActionGroup` (regular or compact;
  layout returns its width; a hairline seam between fused cells), `Tooltips` (QCView's 600 / 5000 ms, hidden by a
  press until the pointer moves on, above the anchor or below when there is no room, clamped, with the tip). 4 tests.
- **`apps/qcv`**: `transport_bar.rs` = QCView's `TransportBar.qml` order and tooltips as nodes; backed buttons emit the
  keys' `Action` through `Transport::apply` (start / end, step, play-pause with the icon mirrored, rewind / fast-forward
  as hold → `ShuttleBegin` / `ShuttleEnd`, in / out selected while set, clear enabled while either is set, loop selected,
  fullscreen through the shell path); the unbacked ones (clip step, compact, screenshots, guides, background, hover
  thumbs, colour, notes, mute, pin) drawn disabled. winit 0.31's `PointerMoved` / `PointerButton` / `PointerLeft` →
  one `App::pointer` router. The layer re-renders when a pointer event changed a node, while a ramp runs, when the
  tooltip's visibility changes, or when the frame number changes. `--ui-bench` now stages hover (+ tooltip), pressed,
  selected and disabled in the snap.
- **Numbers (M5 Max, 2560×1440 @2):** scene build 0.07 ms p50, vello render 0.15 ms p50 headless; in the window during
  1080p playback 0.6–1.3 ms p50 (unchanged by the nodes), present 16.7 p50, 0 dropped, 20 % of one core with decode.
- **Seen in the snap:** the fused cluster with its seams, the hovered step-forward cell squared between its neighbours
  with the tooltip above it, set-in pressed, loop selected in the deliberate grey, the disabled glyphs, the pin.
  Open question kept for the user: the hairline seams between fused cells (S2's quiet compact group has none).

#### 6d · the status strip, the go-to popover (the first text input), the video above the bands (landed 2026-11-07)
- **`min-present`**: `Engine::set_content_rect` — the fit's NDC scale + offset (`fit_in`, pure, tested) through every
  stage shader (`U.off`), so the video letterboxes into the rect above the bands instead of under them (the known gap
  from 6b closed). `qcv` sets it from the three band heights at creation and on every resize.
- **`min-ui`**: `input::{KeyEvent, Key, Named, ImeEvent}` (the shell translates winit's); `Fonts::detail` (Spectrum's
  tracked-capitals detail style via parley `LetterSpacing`); **`TextField`** on S3's `PlainEditor` (the recessed well,
  border default / hover / focus-accent / error-negative, text centred in any height — QCView's `FlatTextField` rule;
  S3's key table for one line; Enter → `Accepted`, Escape → `Cancelled`, newlines never enter; IME preedit / commit
  routed; caret + selection from the editor's geometry; 1 test); **`Popover`** (Spectrum's elevated layer: gray-75,
  1-px gray-200 border, radius 8, the three-layer `drop-shadow-elevated` as vello blurred rects with σ = blur / 2,
  8-px padding, an optional detail title; above the anchor, below when there is no room, clamped).
- **`apps/qcv`**: `status_strip.rs` = QCView's `TimelineStatus.qml` as nodes — the frame well and the SMPTE well
  (recessed, the border steps up on hover over the 100-ms ramp; the SMPTE well at the source's start timecode through
  `qcv-meta`'s `TimecodeFormatter`, drop-frame in the notice colour; fps right, subdued); a click opens the go-to
  popover in frame or timecode mode (time mode waits for playlists) with QCView's rules: Resolve-style digit padding
  ("1000" → 00:00:10:00), warn-toned digits on a bad entry until the next edit, Enter commits (frame clamped to the
  media, timecode relative to the start TC), Escape / a press outside / window focus loss closes and discards; a press
  outside also goes no further (minNotes' flyout rule). **Divergence, with its reason beside the code:** the popover
  opens ABOVE the well — QCView opened downward only because an in-scene popup could not rise over the native surface.
  Keys route to the open field before the key map; IME is enabled while it is open. The bench stages it.
- **Numbers:** unchanged — scene build 0.07 ms, vello 0.15 ms headless; 0.85–1.23 ms p50 in the window during playback.
- **Seen in the snap:** the video now sits above the bands in its own letterbox; the popover rises over it from the SMPTE
  well with "GO TO TIMECODE" in tracked capitals and the accent-bordered field.

#### 6e · the scrubber node and the hover thumbnail over the viewport (landed 2026-11-07)
- **`min-ui::Scrubber`** — the timeline surface as ONE node drawing from a `ScrubState` each frame (§17.5 item 4,
  row 166): the 32-px band is the ruler. QCView's camera (`pps` + `scrollX`; wheel zooms about the cursor by
  `1.0015^dy` between fit and max(200 px/s, fit × 50); Alt-wheel pans; a resize keeps the visible range; the 20-px
  page-turn follow while playing) and QCView's gesture (press → `Begin` + `To`, move → `To`, release → `To` + `End`,
  leaving ends it; the playhead follows the cursor, not the decode) through the Transport's `begin_scrub` /
  `scrub_to_frame` / `end_scrub`. Ticks for the VISIBLE range only (QCView's ≥ 80-px rule; frame ticks at ≥ 6 px per
  frame, frame numbers at ≥ 24 px), the in / out zone and flags (in → out + 1), a cache underline when coverage is known,
  a 2-px extent marker when zoomed, the hover line, the near-white playhead. Double-click fits (ours: QCView's overview
  bar carried that). The waveform draws from `peaks` only — the synthetic fill of the sketch is gone; the audio probe
  (step 2b) fills it. 2 tests (zoom about the cursor / snap to fit / pan; press-move-release and the double-press).
- **Wheel**: winit's `LineDelta` as notches (× 120, Qt's angle units) and `PixelDelta` at 2 units per px; Alt folds x
  into y (QCView's Windows rule). The cursor is the pointing hand over a loaded scrubber (QCView's `cursorShape`).
- **The hover thumbnail** (`apps/qcv/src/hover_thumbs.rs`, row 170 BUILT): `qcv-player`'s cache + `VideoThumbLoader`
  with the GOP-cache peek, requests at the 16-ms throttle, playback drops the loader to one worker, the strip
  precaches on open; the pixels become a vello image (`paint::image_rgba8`) drawn bottom-left of the CONTENT rect at
  0.18 of its width with a 1-px border — a UI node, display-referred, no longer composited into the frame before the
  colour chain (§17.5 item 5). The eviction callback drops the image.
- **Numbers (2560×1440 @2):** scene build 0.14 ms p50 (the ticks and labels), vello 0.35 ms p50 headless; in the
  window 0.7–1.3 ms p50 during playback, present 16.7 p50, 0 dropped. The strip precache costs ~100 % of one core
  for its first seconds after open (3 workers × 8 threads — the hover pass's known one-time burst).
- **Seen in the snap:** zoomed to ~160 px/s with 1-s ticks, the extent marker along the bottom, the hover line at
  62 %, the thumbnail of that frame in the viewport's corner; the playhead at frame 0 correctly out of view.

#### 6f · the manual review's fixes and fullscreen (2026-11-07)
- The user's walk ("fantastic", "perfect"): the scrub grab now survives the pointer leaving the window (Qt's MouseArea
  kept it for QCView; winit sends `PointerLeft`, so the scrubber ignores it while pressed — 46152c3); the picture is
  drawn as an exact QUAD — the fit-scaled fullscreen triangle's overhang had smeared the edge column diagonally into the
  letterbox at odd window aspects, present since S1 and invisible while every window matched the video's aspect (a
  headless test renders a square picture into a 64 × 16 target and checks both pillarbox sides; 972ca31); scrub / seek /
  shuttle jumps no longer count as dropped frames.
- **Fullscreen (row 171)**: the picture alone; the bands slide in from the bottom edge while the pointer rests in a
  24-px zone (or on the shown bands) and slide away 800 ms after it leaves, over 160 ms; the content rect becomes the
  whole window so the picture does not re-fit as the bands move; F, the transport button and Escape share one toggle
  (Escape leaves fullscreen before it quits the harness). Compact (row 172) and dual (row 173) are proposed, not built.

