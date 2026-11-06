# P0 — the performance baseline of the Qt apps (macOS, 2026-11-06)

**Question:** what numbers must the Rust spikes beat or match? (§21.3; row 47 made concrete.)
**Verdict: MEASURED on macOS.** Windows is OWED (the box over SSH; same scripts cannot run there —
xctrace is macOS-only, so Windows needs PIX/ETW or QCView's own `QCV_FPS_LOG`, which is D3D11-only).
No app source was modified. Everything here is reproducible with `tools/p0/`.

## Machine
MacBook Pro, Apple M5 Max, 128 GB, macOS 26.7.1. **The QCView window and the minNotes probe window
were on the external display "AW3225QF" at 60 Hz** (QCView logged `EDR-supported=no, headroom=1.00`
there); the built-in XDR was not the surface under test. ffmpeg 9.0.2 (Homebrew), VideoToolbox.
QCView = `build/dist/qcview.app` (Release, built Nov 5 14:29, the shipped tree); minNotes = 2.0.6
`build/release` (Oct 24) for the GUI runs, `build/debug` (RelWithDebInfo) for the offscreen type probe
(the release bundle carries no `offscreen` platform plugin).

## Fixtures (`fixtures/`, gitignored; `tools/p0/make-fixtures.sh` regenerates them from `fixtures/sources.env`)
The sources are client footage: the fixtures, the traces, the logs and the source paths stay on this machine.
One REAL source per resolution, every other codec derived from it so content entropy is identical:
| Fixture | What | Source |
|---|---|---|
| `prores_1080.mov` | ProRes 422 HQ 1920×1080 yuv422p10 23.976, 30 s | a real 30-s commercial master (private) |
| `prores_4k.mov` | ProRes 422 HQ 3840×2160 yuv422p10 23.976, 28.5 s | a real short-film export (private) |
| `prores_long.mov` | ProRes 422 HQ 1080p, 306 s (symlink, 5.8 GB) | a real 5-min selects reel (private) — the long-seek case |
| `h264_1080.mp4` / `h264_4k.mp4` | H.264 High 4:2:0 8-bit, 20 / 60 Mb/s, GOP 48, VideoToolbox | derived |
| `hevc10_pq_4k.mp4` | HEVC Main10 p010, 50 Mb/s, tagged PQ/BT.2020 (SDR content — a DECODE fixture, not HDR truth) | derived |
| `exr_4k/` | 72 × EXR half ZIP16 3840×2160 (1.8 GB) | derived |

## 1. Decoder floor (app-independent; `tools/p0/decode-floor.sh` → `fixtures/p0-decode-floor.txt`)
`ffmpeg -benchmark … -f null -`, all frames, wall-clock fps. **hw = `-hwaccel videotoolbox` INCLUDING the
download of every frame to system memory** (that is what `-f null` forces), so for inter codecs the hw
column is a download-bound number, not the decoder's; QCView keeps frames on the GPU (zero-copy) and
never pays it. sw = software decode, auto threads.
| Fixture | hw fps | sw fps | hw maxrss | sw maxrss |
|---|---|---|---|---|
| ProRes HQ 1080p | 696 | 830 | 359 MB | 230 MB |
| ProRes HQ 4K | 268 | 249 | 1355 MB | 948 MB |
| H.264 1080p | 225 (download-bound) | 1194 | 160 MB | 143 MB |
| H.264 4K | 99 (download-bound) | 466 | 489 MB | 463 MB |
| HEVC Main10 4K | 193 | 200 | 1064 MB | 996 MB |
| EXR 4K half ZIP16 | — | 98 | — | 2554 MB |

Seek floor = wall time of `ffmpeg -hwaccel videotoolbox -ss T -i F -frames:v 1` (8 seeks spread over the
file; INCLUDES `avformat_open_input` ≈ 100 ms, so this is "open + seek + decode one frame"):
ProRes 1080p 141 ms p50 · ProRes 4K 172 · ProRes long 142 · H.264 4K 181 · HEVC10 4K 126 (p95 within +5 ms of p50 everywhere).

## 2. QCView (Qt + native Metal), traced with `xctrace` Metal System Trace, no instrumentation
Method (`tools/p0/qcview-run.sh`, `qcview-log.py`, `pacing.py`, `seeklat.py`, `qcv_trace.py`): the dev
binary is launched directly with QCView's own automation env vars (`QCV_AUTOPLAY_SECONDS`, `QCV_START_SECONDS`,
`QCV_SCRUB_FRAMES`, `QCV_QML_EVAL="N:Qt.quit()"`), a Metal System Trace is attached ~1 s later, RSS sampled at
2 Hz, one `footprint` mid-run. The trace shows TWO CAMetalLayers: Qt Quick's window layer (4532×2962 = the
2266×1481 window @2x) and the native player layer (3248×2638). Gotcha found on the way: `xctrace --launch`
on the .app went through LaunchServices and ALSO started the installed `/Applications/qcview.app` — hence
launch-then-attach.

**Startup (from the first log line; the file is opened after QCView's deliberate 500 ms delay):**
| Milestone | ms after first log line |
|---|---|
| Metal present loop started | 290–350 |
| avformat open done (file opened at +500 by design) | 780–830 |
| `get_format` → `videotoolbox_vld` picked | 785–830 |
| first frame zero-copy published to Metal (`'x422'`/`'420v'`/`'x420'` CVPixelBuffer) | 885–1000 |
So **open → first frame on the renderer ≈ 385–500 ms** across all four codecs (ProRes 4K 385, HEVC10 4K 397,
ProRes 1080p 424, H.264 4K 502). All four decode through VideoToolbox with ZERO CPU copies (no `swscale`
lines were logged in any run).

**Playback, steady window 5–20 s of the trace (`QCV_AUTOPLAY_SECONDS=3`):**
| Fixture | player-layer present interval p50 / p95 / max (ms) | >1.5× 23.976 period | GPU cmd-buffer p50 / p95 / max (ms) | proc CPU mean / p95 (% of a core) | max RSS | phys footprint |
|---|---|---|---|---|---|---|
| ProRes 1080p | 16.6 / 18.0 / 25.5 | 0 | 0.37 / 1.14 / 8.2 | 16 / 40 | 365 MB | 1067 MB |
| ProRes 4K | 16.6 / 19.6 / 20.6 | 0 | 0.36 / 1.04 / 2.8 | 20 / 50 | 546 MB | 1362 MB |
| H.264 4K | 16.6 / 20.1 / 28.4 | 0 | n/a | 19 / 50 | 578 MB | 1406 MB |
| HEVC Main10 4K | 16.6 / 20.5 / 61.1 | 0 | 0.43 / 9.47 / 45.9 | 23 / 60 | 965 MB | 1813 MB |
Reading it: the native present loop re-presents the player layer at the DISPLAY rate (60 Hz here), so the
interval columns say "the present loop never missed more than one refresh (one 61 ms gap on HEVC10)";
whether an individual 23.976 fps video frame was late is NOT visible without renderer instrumentation —
that is the one QCView number P0 could not take and S2's harness must. ~114 command buffers/s (two layers
× 60 Hz), p50 0.36 ms of GPU per buffer; the 10-bit HEVC path has a GPU tail (p95 9.5 ms) worth a look in S1.
Playback costs about a fifth of one core.

**Seek and scrub (paused app; seek = the idle→busy CPU burst of the process, 10 ms buckets):** the player
layer presents every refresh even when paused, so "onset → next present" is meaningless (3–25 ms); the
honest proxy is the BURST LENGTH = the wall time the app works on the seek, after which the next refresh
(≤ 16.7 ms) shows it.
| Run | seek work wall ms p50 / min / max (n=6) | CPU-ms inside the burst |
|---|---|---|
| `QCV_START_SECONDS` seeks, ProRes 1080p 306 s file | 120 / 80 / 130 | 25–42 |
| `QCV_START_SECONDS` seeks, ProRes 4K | 60 / 40 / 110 | 20–35 |
| `QCV_SCRUB_FRAMES` scrubs, ProRes 4K | 30 / 30 / 90 | 11–24 |
| `QCV_SCRUB_FRAMES` scrubs, ProRes 1080p 306 s | 40 / 20 / 80 | 7–28 |
So a random seek on QCView lands in **≈ 60–130 ms + one refresh**, a timeline scrub in **≈ 30–40 ms + one
refresh**; the ffmpeg floor's 140–180 ms includes ~100 ms of file open, so QCView's seek is already at the
decoder's floor. Max RSS during seeks/scrubs: 366–500 MB.

## 3. minNotes 2.0.6
- **Startup to a rendered document** (`--shot` with `MN_SHOT_DELAY=1`, 3 runs, release): **2.45 / 2.50 / 2.51 s**
  wall for launch + 1,622-block `type-probe.md` import + render + PNG. (Includes the dev settings domain
  and a markdown import; a cold `.mnd` open is owed.)
- **Typing (`scripts/type-probe.sh`, debug build, offscreen, 160 keys/phase, 1,622 blocks)** — per-key QML
  wall µs p50 / p95: para-mid 339 / 984 · para-spans 847 / 1373 · table-cell 816 / 7126 · code 469 / 893 ·
  task 624 / 992 · backspace 803 / 1241 · struct 17 660 / 21 264 (40 keys). 0 fails of 960 checks.
- **Key→swap (`--key-probe=200`, release, on screen):** the harness predates the 2.0 canvas root — only 29 of
  200 synthetic keys registered and they measured ~490 ms (the keys land on the canvas view, not a page
  caret), so the figure of record stays the 2026-10-16 measurement in `app/main.cpp`: **key→swap p50 3.4 ms**
  (from 11.9 with the display-link path). Re-measurement with the probe pointed at a page is OWED.
  The probe's wheel-glide part worked: **swap interval p50 / p95 / max 16.7 / 17.0 / 23.1 ms at 60 Hz, 0 gaps
  > 25 ms**, polish per frame p50 0.01 ms, 46 visibility flips over the glide.
- Its video player is the QCView lineage copy (FFmpeg + VideoToolbox + Metal zero-copy); not timed separately.

## 4. What this gives the spikes (pass bars, §21.3)
- **S1 present:** open → first frame ≤ 500 ms (4K ProRes 385); steady present p95 ≤ 20 ms at 60 Hz with no
  gap > 1 refresh; GPU ≤ 1 ms p95 per frame for 8/10-bit 4:2:2 at 4K (HEVC10's 9.5 ms p95 is the thing to beat,
  not match); ≤ 0.25 core for 4K playback; footprint ≤ 1.4 GB at 4K.
- **S2 scrub:** seek ≤ 60 ms (4K ProRes) / ≤ 120 ms (1080p in a 5-min file) of work + one refresh; scrub ≤ 40 ms;
  and the number P0 could not take — per-video-frame lateness — must be a first-class output of S2's harness.
- **S3 text:** per-key ≤ 1 ms p95 in paragraphs (minNotes: 0.98 ms), key→swap p50 ≤ 3.4 ms.
- **decode floor** is the ceiling for `min-image`/`qcv-player`'s FFmpeg binding: 250–270 fps 4K ProRes,
  ~200 fps 4K HEVC10, 98 fps EXR half ZIP16 (single process, all cores).

## Owed
Windows numbers (same fixtures, the box); QCView per-frame video lateness (needs S2's harness or a renderer
signpost); minNotes key→swap on a page under 2.0; a cold `.mnd` open; QCView on the built-in XDR with EDR on
(the HDR path was not exercised — `EDR-supported=no` on the AW3225QF); EXR sequence playback in QCView.
