# S8 — output latency: how much sooner can a shown frame reach the output ring?

**Question (§21.3; canon rows 259–260, `docs/phase2/step6-output-cadence.md` proposal 4):** the port publishes an output
frame from the main thread's tick and collects its copy on a later tick. How long is it from the moment a frame is shown
(`Engine::show`) to its commit in the QCVO ring, and how much of that goes away by (1) publishing in the tick that shows the
frame and (2) collecting the copy as soon as the GPU has it — the thing a worker thread would do?
**Pass bar:** on the Mac (Apple M5 Max, a 60 Hz screen), 23.976 fps, 1080p HEVC 10-bit → PQ and UHD ProRes → PQ, 20 s each,
the stand-in sink: shown → ring at p50 **at least 14 ms shorter** than today (one refresh, less 2 ms), with the viewport's
dropped / late counts and its 2 : 3 cadence unchanged, and nothing refused or reordered on the wire.
**Time box:** one session, 2026-11-10. **Status: VERDICT GO (2026-11-10) — 28–31 ms shorter.**

## What the spike was
A patch on the app, never committed (kept, ignored, in `docs/spikes/s8/out/s8-spike.patch`): the engine notes when it shows
a frame; the frame's meta carries that to the ring commit; `QCV_TRACE_OUTPUT` prints shown → ring once a second.
`QCV_SPIKE_OUT=1` calls the publish again right after the frame is shown, in the same tick; `=2` then waits for the GPU
(`device.poll(wait)`) and collects in that tick too — on the main thread, as a stand-in for what a worker would do.

## Findings

| Mode | 1080p HEVC → PQ: shown → ring, ms (p50 / worst second's max) | UHD ProRes → PQ | Viewport (dropped / late / cadence) |
|---|---|---|---|
| Today | 33.4 / 39.6 | 34.2 / 37.6 | 0–2 / 0 / 2 : 3 |
| 1 — publish in the frame's own tick | 16.7 / 19.9 (the unobstructed seconds) | 23.6 (obstructed window — see below) | not judged |
| 2 — and collect when the GPU is done | **2.7 / 4.6** | **6.0 / 8.5** (two start-up outliers of 22 and 49) | 0–1 / 0 / 2 : 3 |

- **Today's 33 ms is two refreshes, by construction**: `outputs_tick` runs at the top of the tick, the frame is shown further
  down, so the publish happens one tick after the show and the collection one tick after that.
- Mode 1 removes the first refresh for nothing: no thread, no wait.
- Mode 2 removes all but the GPU's own time: under 3 ms at 1080p, about 6 ms at UHD (the copy of 33 MB out of the GPU and
  into the ring). On the main thread that wait is paid once per new frame, before the viewport's draw; the viewport's
  numbers did not move in the clean runs, but it is main-thread time a worker would not spend.
- The wire stayed whole in every run: every frame in step, nothing refused, nothing reordered.
- A caution on the first pass: for about a minute the test window was covered on the desktop (`surface occluded` in the log)
  and those runs dropped ~40 frames each whatever the mode. Mode 2 was re-run clean for both clips and today's for UHD; mode 1
  was not re-run — its 16.7 ms is from the eleven unobstructed seconds of the 1080p run, its UHD figure is an obstructed one.

## What it is not
The 28–31 ms is the port's share only. Downstream of the ring, QCBridgeA 0.0.2 holds up to two frames in its queue and the
card three in AutoCirculate (its log's "card level 3") — about 125 ms at 23.976 before the picture leaves the card. Not
measured here: Windows (DX12 read-back timing), two outputs at once, dual view.

## Verdict
GO. Publishing in the frame's own tick is worth doing on its own (−16.7 ms, a reordering of the tick). Collecting when the
GPU is done is worth another 11–14 ms; whether that is a short wait on the main thread or a worker thread is the build's
ruling — the spike says the wait is 3–6 ms per new frame on this machine. The spike code is gone, as spikes go.
