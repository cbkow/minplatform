# Phase 2 · step 2 — transport + timeline (planned 2026-11-07; rulings 154–159 proposed in the canon)

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
(as it lands)
