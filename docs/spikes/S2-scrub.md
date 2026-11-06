# S2 — scrub: rational time + a Sequencer with two decoder slots + a scrub decoder on the S1 presenter

**Question (§21.3, §17):** can a Rust `Sequencer` (rational time, two decoder slots, a latest-wins scrub decoder with
forward-no-reseek + keyframe seek) on the S1 present path seek and scrub at QCView's numbers, play a playlist cut
GAPLESS, shuttle to ±32× without stalling, and report per-video-frame lateness as a first-class number — on long ProRes,
H.264 and an EXR sequence?
**Pass bar (vs P0):** random seek ≤ 60 ms of work (4K ProRes) / ≤ 120 ms (1080p in the 5-min file) + one refresh; scrub
request → frame ≤ 40 ms + one refresh with latest-wins coalescing; the cut shows no black and no duplicated frame at the
boundary; shuttle 2×→32× keeps the shown frame within one refresh of the shuttle clock (the pitch cap is the AUDIO grain
engine's — no audio in Phase 1 spikes, so the shuttle bar here is the video side only); playback lateness per video frame
p95 ≤ 1 refresh with a 3:2-correct cadence (no half-frame-per-second drops as in S1's wall-clock loop); EXR sequence seeks
and scrubs like video.
**Time box:** 2 weeks from 2026-11-06. **Status: MEASURED 2026-11-06 on macOS — VERDICT: GO (every bar met; see below). Windows owed.**

## What the spike is
`spikes/s2-scrub`: the S1 modules by `#[path]` (decode / vt / readbacks / present shaders — spikes share files, not crates)
plus `timeline.rs` (rational time, clips, resolve), `scrub.rs` (the scrub decoder: own FFmpeg context + hwaccel, latest-wins
target, LRU of decoded frames, 3-way hit / forward ≤ 90 / keyframe seek — QCView's `ScrubDecoder` shape, §17.4) and a
`Sequencer` in `main.rs` with two streaming decoder slots (A plays, B is prewarmed on the next clip; swap at the boundary).
Harness modes, each printing its own numbers: `--play` (lateness per video frame, cadence), `--seeks t,t,…` (paused
random seeks: request → decoded → presented), `--scrub a-b:ms` (a pointer sweep issuing a request per refresh; lag of every
shown frame vs the latest request), `--shuttle` (2×→32×, timer leads / decoder chases), `--cut A@in-out,B@in-out` (the
gapless boundary). `--file F` or an EXR pattern `dir/frame_%04d.exr`.

## Results (macOS, M5 Max, the external 60 Hz display, release, 2026-11-06)
Fixtures: 4K ProRes HQ 4:2:2 10-bit (683 frames, intra), 1080p ProRes 5-min file (intra), H.264 4K GOP 48 (inter), 72-frame
4K EXR half ZIP16 sequence (intra). Streaming slots = VideoToolbox zero-copy (`--hw`) for video, FFmpeg's exr decoder for EXR;
the scrub decoder = SOFTWARE (QCView's choice on Apple — VideoToolbox pays a flush/re-prime per seek: 4.7 ms p50 but 116 ms
max on ProRes, and NOTHING comes out of an inter stream after a flush within a 2-s sweep), slice-threaded for intra codecs,
frame-threaded for inter.

**Playback — per-video-frame lateness, the number P0 could not take** (`--play`, 8 s; `--cut` = the two-slot playlist):
| Run | frames shown / dropped / late (> 1 refresh) / black | lateness vs the ideal clock p50 / p95 / max (ms) | cadence (refreshes per frame) | present ms p50 / p95 / max |
|---|---|---|---|---|
| 4K ProRes, play | 192 / 0 / 0 / 0 | 8.2 / 15.9 / 16.5 | 2: 95, 3: 95, 4: 1 | 16.7 / 16.9 / 22.8 |
| H.264 4K, play | 191 / 0 / 0 / 0 | 7.4 / 14.9 / 16.4 | 2: 94, 3: 96 | 16.7 / 17.0 / 28.2 |
| EXR sequence, play (72 frames) | 72 / 0 / 0 / 0 | 7.6 / 14.5 / 15.5 | 2: 34, 3: 35 (+1, +4 at the start) | 16.6 / 17.1 / 20.3 |
| **cut** 4K ProRes @100–200 → @400–500 | 200 / 0 / 2 / 0 | 8.4 / 15.7 / 16.8 | **2: 98, 3: 101 — nothing else at the boundary** | 16.7 / 16.9 / 25.7 |
| **cut** H.264 @130–230 → @400–500 (both in-points mid-GOP) | 200 / 0 / 1 / 0 | 8.3 / 16.1 / 16.7 | **2: 99, 3: 100** | 16.7 / 17.1 / 26.0 |
Reading: lateness is uniform inside one refresh (p95 < 16.7 ms = the frame always lands on the refresh it belongs to), the 3:2
cadence is exact, nothing drops — S1's wall-clock half-frame-a-second loss is gone by scheduling on the rational clock. **The
cut is gapless: no black refresh, no duplicate, no cadence break, with the second slot prewarmed on the next clip's in-point —
including H.264 in-points in the middle of a GOP.** (The one or two "late" entries are the display's own 23–28 ms hiccups,
visible in the present max.)

**Seeks** (`--seeks`, paused, 8 random targets per file; the Sequencer answers from the SCRUB decoder and the streaming slot
catches up — QCView's `seekToTime` + resume shape):
| File | scrub-decoder answer ms p50 / p95 (+ one refresh to show) | streaming slot resumes after ms p50 / p95 | P0 bar / QCView |
|---|---|---|---|
| 4K ProRes | **15.9 / 17.8** | 67.6 / 72.8 (VideoToolbox's flush + re-prime) | ≤ 60 of work / QCView 60–130 |
| 1080p ProRes, 5-min file | **7.4 / 10.6** | 31.1 / 38.5 | ≤ 120 / QCView 80–130 |
| EXR sequence | 22.3 / 23.7 | 141 / 145 (the frame-threaded pipeline fill) | — |
| H.264 4K GOP 48 | 76 / 135 (keyframe seek + up to 47 frames of fill) | 278 / 400 (VideoToolbox after a flush) | — (no P0 number) |
The streaming decoders' first frame after a flush costs a PIPELINE FILL everywhere (VideoToolbox 65 ms ProRes / 280 ms H.264;
FFmpeg frame threads 140 ms EXR) — the scrub decoder is the seek path, the stream is the playback path. Before the slice-thread
fix the same 4K ProRes scrub cost 64–71 ms either way: frame threading needs a pipeline of packets before the first frame
comes out after a flush, so one frame costs a dozen.

**Scrub** (`--scrub a-b:ms`, a pointer sweep issuing one request per refresh for 2 s — QCView issues one per move, decoders
coalesce latest-wins):
| Sweep | requests → frames shown (cache hits) | request → frame ready ms p50 / p95 / max | request → shown p50 / p95 | frames behind the pointer p50 / p95 / max | P0 bar / QCView |
|---|---|---|---|---|---|
| 4K ProRes forward 100→500 | 120 → 120 (0) | **5.6 / 7.6 / 18.3** | 16.7 / 19.5 | 3 / 4 / 4 | ≤ 40 / QCView 30–40 (+ a refresh) |
| 4K ProRes backward 500→100 | 120 → 120 | ≈ the same (intra: one decode per request) | | | |
| 1080p 5-min file forward | 120 → 120 (0) | **3.5 / 5.3 / 8.6** | 16.7 / 17.1 | 3 / 4 / 4 | ≤ 40 / QCView 20–80 |
| EXR sequence 10→60 | 120 → 119 (69) | 0.0 / 13.0 / 25.8 | 16.7 / 16.9 | 0 / 1 / 1 | — |
| H.264 4K forward 100→500 | 105 → 96 (0) | **1.3 / 1.4 / 15.0** (forward fill tracks the pointer, retargeting mid-fill) | 19.3 / 19.5 | 4 / 4 / 4 | — |
| H.264 4K backward 500→100 | 107 → 62 (55) | **0.0 / 13.5 / 32.7** (decode-once-per-GOP: the fill's frames all enter the LRU) | 19.1 / 19.4 / 37.6 | 4 / 4 / 8 | — |
| H.264 4K backward, 4-s sweep | 210 → 145 (131) | 0.0 / 11.6 / 76.3 | 19.2 / 20.9 / 91.8 | 2 / 2 / 9 | — |
"Frames behind the pointer" is latency × pointer speed: at one refresh of latency and 200 frames/s of pointer travel that is 3–4
frames, the floor for a 60 Hz pointer. The three rules that made inter codecs work: frame threads for the scrub decoder on inter
streams; RETARGET instead of abandon when a newer request is ahead within reach (a 60 Hz request stream starved the
QCView-literal "newer target → bail" on 4K H.264: 1 frame shown in 2 s); every frame of a GOP fill into the LRU.

**Shuttle** (`--shuttle`, 2× → 32× doubling each second; the clock leads, the scrub decoder chases):
| File | frames shown per second at 2× / 4× / 8× / 16× / 32× | lag behind the clock in frames p50 (max) |
|---|---|---|
| 1080p ProRes 5-min | 55 / 60 / 60 / 61 / 56 | 1 (2) / 2 (2) / 3 (4) / 6 (7) / 13 (26) |
| 4K ProRes | 61 / 60 / 60 / 54 / (file too short) | 1 (2) / 2 / 3 (4) / 6 (7) |
| H.264 4K | 53 / 53 / 52 / 44 / (file too short) | 1 / 2 / 4 / 7 (8) |
Reading: one new frame per refresh at every rate, and the lag is one refresh of clock travel (32× × 24 fps / 60 Hz = 12.8
frames) — the bar "within one refresh of the shuttle clock" holds; the 26-frame max at 32× is a two-refresh hiccup. The pitch
cap belongs to the audio grain engine, which no Phase 1 spike runs.

## Verdict (macOS, 2026-11-06) — GO
| Pass bar | Result | |
|---|---|---|
| seek ≤ 60 ms of work (4K ProRes) / ≤ 120 (1080p 5-min) + one refresh | 15.9 / 7.4 ms p50 via the scrub decoder (QCView 60–130 / 80–130) | met, 4–10× better |
| scrub request → frame ≤ 40 ms + one refresh, latest-wins | 5.6 (4K ProRes) / 3.5 (1080p) / 1.3 fwd, 13.5 p95 back (H.264) | met |
| the cut: no black, no duplicate at the boundary | 0 black, cadence 2/3 only, both ProRes and mid-GOP H.264 | met |
| shuttle 2×→32× within one refresh of the clock | one frame per refresh, lag = one refresh of travel | met (video side) |
| playback lateness p95 ≤ 1 refresh, 3:2-correct, no drops | p95 14.5–16.1 ms, cadence exact, 0 dropped | met |
| EXR sequence seeks and scrubs like video | 22 ms seeks, scrub hits/13 ms, plays at cadence | met |
**What the GO rests on:** rational time + resolve (the `qcv-timeline` shape, row 83 — it did the cut arithmetic without a
double in sight), two streaming slots with the next clip prewarmed (row 85), a scrub decoder with the four rules above (slice
threads for intra, frame threads + retarget + GOP-wide LRU for inter, software on Apple), and the Sequencer answering seeks
from the scrub decoder while the stream catches up. **Not covered:** Windows (D3D12VA flush behaviour, two hw contexts
resident — §17.8), audio + the pitch cap, hover thumbnails, the 50-ms seek debounce and other Transport constants (row 88 —
the spike has no UI to debounce), resolution changes at a cut (the fit is written once), mixed-rate playlists (the rescale is
exercised only at equal rates).

## Facts for the canon
- The scrub decoder's threading is the whole scrub story: SLICE threads for intra codecs (a flush costs nothing; 4K ProRes 5.6
  ms), FRAME threads for inter codecs (a GOP fill needs the throughput; the pipeline-fill latency is paid once per fill).
- A streaming decoder's first frame after a flush is a PIPELINE FILL: VideoToolbox 65 ms (ProRes) / 280 ms (H.264, and it
  outputs nothing within a sweep), FFmpeg frame threads 140 ms (EXR). Seeks are the scrub decoder's job; the stream resumes.
- Latest-wins must not starve: retarget forward within reach, finish the fill otherwise. QCView's literal rule starved at 60 Hz.
- The rational-time Sequencer gives exact 3:2 cadence and zero drops on 23.976 content on a 60 Hz display with no special
  casing; lateness per frame is a one-line statistic once the clock is rational.

## Windows (rhombus: i9-14900KF, RTX 4090, 2026-11-06) — D3D12VA slots + the software scrub decoder
Same battery (`C:\qcb-lab\s2-runs.ps1`, desktop launches), DX12 backend, `--hw` = D3D12VA on wgpu's device (`d3d.rs`; the decoders
open AFTER the GPU so the hw context is wgpu's `ID3D12Device`). **The box has no physical display (P0 §5): presents run at ≈ 13 Hz
(72–77 ms), so every PACING statistic below (shown fps, lateness, cadence, "frames behind the pointer", shuttle frames/s) is
the virtual display's, not the Sequencer's** — the macOS section is the pacing record. What IS valid here: decoder answer times,
the two-slot cut running on two D3D12VA contexts, D3D12VA's behaviour after a flush, CPU.

**Seeks (8 random targets; the scrub decoder answers, the slot resumes):**
| File | scrub-decoder answer ms p50 / p95 | streaming slot resumes after ms p50 / p95 | macOS |
|---|---|---|---|
| ProRes 4K (software both; slice-threaded scrub) | 13.9 / 26.2 | 118 / 124 (frame-threaded software fill) | 15.9 / 17.8 · 68 / 73 |
| H.264 4K (`--hw`: D3D12VA slots; software frame-threaded scrub) | 116 / 156 | **107 / 163** | 76 / 135 · 278 / 400 |
**D3D12VA after `avcodec_flush_buffers` recovers in 42–149 ms on H.264** (VideoToolbox: 278–400 and nothing within a sweep) —
§17.8's question, answered: the Windows hw decoder tolerates seeks. **And the hardware scrub decoder is viable on Windows:**
`--scrub-hw` on H.264 4K answered in **7.5 ms p50 / 20 p95 / 34 max** at 41 % of a core, vs 42.8 / 53.9 / 61 for the software
frame-threaded scrub (365 % of a core) — the opposite of Apple, where VideoToolbox is the one to avoid for inter codecs.

**Scrub sweeps (one request per present → only ~40 requests in 2 s here):** ProRes 4K software 18.8 / 24 / 27 ms ready (macOS 5.6 /
7.6 / 18 — the i9's slice-threaded ProRes decode is ~3× slower than the M5 Max's); H.264 forward 42.8 / 53.9 (each request lands 15
frames ahead at 13 Hz → a 15-frame fill per request); backward 0.0 / 61 (GOP-wide LRU hits); EXR 28 / 43 / 53 (13 hits of 40).
**Shuttle:** 13 frames/s at every rate = one per (virtual) refresh; lag = the clock's travel per 77 ms (4 / 7 / 14–15 / 28–30 frames) —
consistent with "within one refresh", at this refresh.
**Cut (two slots prewarmed, 200 frames):** ProRes 4K software; **H.264 4K and HEVC10 4K on TWO D3D12VA decoder contexts resident at
once on the same ID3D12Device** (mid-GOP in-points, 18–20 % of a core): 0 black refreshes on all three; duplicates cannot be judged at
13 Hz (the cadence histogram is {1, 2} because frames are SKIPPED by the display, not repeated). Two hw contexts at 4K: fine on the
RTX 4090 (§17.8's VRAM / session question — no limit hit).
**Play** (same caveat): ProRes 4K software 373 % of a core (frame-threaded decode), H.264 D3D12VA 19 %, HEVC10 D3D12VA 29 %, EXR 386 %.

**Verdict on Windows (partial):** the Sequencer + slots + scrub decoder run unchanged on DX12 / D3D12VA; the decoder-side numbers
match or beat macOS where the hardware applies (H.264 seeks 107 vs 278 ms on the stream, 7.5 ms hw scrubs) and lag where the CPU
is slower (software ProRes); **pacing owed on a real display.**
