# The outputs' frame rate (canon row 259) — the walk, QCView's path, the measurements, the proposals

2026-11-10. The first AJA hardware walk (a T-TAP Pro, macOS): start / stop, the raster changes, SDR and PQ, the HDR
metadata, the layers and the audio all worked. One fault: on a long 1080p HEVC PQ clip the frame rate on the monitor was
uneven. QCBridgeA's own log showed the card fed at 24/s with nothing dropped while the NEW frames per second fell to 12–13
for about three seconds with no seek near it (and dipped for a second around each seek, which is expected).

## QCView's path (read from `QCView-Player`, nothing edited)

- No QRhi read-back. Each native renderer owns a render THREAD that renders, starts the copy, collects it and commits to the
  ring; the GUI thread only sets the spec and runs the 250 ms handshake.
- Publish on change only (an `OutputStamp` per target); no output clock. Metal: the loop is paced by the drawable at the
  display's vsync, and the output pass shares the frame's command buffer.
- Metal is zero-copy: each ring slot is wrapped once as a no-copy `MTLBuffer` and the wire texture is blitted into it; one
  frame in flight per sink, committed on the next loop turn. D3D11: a persistent wire texture + three persistent staging
  textures as a FIFO, settled every 3 ms while a copy is pending, oldest first, stopping at the first unfinished copy.
- The descriptor (`presentation_ns`, `source_frame_index`) is stamped at ENCODE time.
- The ring: 3 slots, the reader takes the LATEST only — two commits between two reads and the earlier one is never seen.
- QCBridgeA: one thread — take the latest, copy it to one DMA buffer, then `AutoCirculateTransfer` if the card can accept,
  else wait for the output's vertical interrupt. In the steady state the ring is read about once per card frame; a repeat
  is a transfer of the unchanged buffer. It reads neither `presentation_ns` nor the sequence gap. "N/s to the card" counts
  transfer calls; "M/s new" counts frames taken from the ring.

Differences in the port: everything runs in the main thread's tick; the copy is collected once per tick; a wire texture and
a bind group are created per frame; the bytes are mapped and copied (no blit into the slot); the descriptor is stamped when
the copy is collected, so two frames collected together carry one frame index.

QCView / QCBridgeA items noticed (listed for the user, not fixed): the Metal renderer blocks its thread on
`waitUntilCompleted` when frames change faster than the GPU finishes; Metal's skipped frame is uncounted; D3D11 sets
`kOutputFrameHasB` only in dual; a D3D11 frame skipped with the FIFO full just before a pause is never sent; QCBridgeA counts
failed transfers as sent; `presentation_ns` is documented as the sinks' pacing clock and no sink reads it;
`OutputRequest.max_fps_*` is never read.

## The measurements (synthetic fixtures, the stand-in sink, an Apple M5 Max, a screen refreshing at 60 Hz)

`QCV_TRACE_OUTPUT=1` on the app, `fake_sink --cadence` as the sink; 20 s of playback each at 23.976 fps.

| Case | Copy back, ms (p50 / worst) | Refused | Paired | Out of order | Main thread per call, ms | Arrival at the sink, ms (p5 / p50 / p95) |
|---|---|---|---|---|---|---|
| ProRes 1080p → SDR | 16.0 / 26 | 0 | 0 | 0 | 0.28 | 29 / 35 / 51 |
| HEVC 10-bit 1080p → PQ | 16.4 / 20 | 0 | 0 | 0 | 0.28 | 20 / 34 / 51 |
| HEVC 10-bit 1080p → SDR | 16.3 / 20 | 0 | 0 | 0 | 0.29 | 15 / 34 / 51 |
| ProRes 1080p → PQ | 16.3 / 20 | 0 | 0 | 0 | 0.26 | 33 / 47 / 51 |
| HEVC 10-bit UHD → PQ | 16.4 / 20 | 0 | 0 | 0 | 0.29 | 33 / 47 / 51 |
| ProRes UHD → SDR | 16.5 / 19 | 0 | 0 | 0 | 0.29 | 33 / 47 / 51 |

The app's side is the same in every case: the copy back takes one refresh, nothing is refused or reordered. What is NOT
even is the arrival: a 23.976 picture on a 60 Hz screen changes every 2 or 3 refreshes, so frames reach the ring 33 or 50 ms
apart, never 41.7.

The stand-in card (the newest frame once per 41.7 ms, as QCBridgeA reads): five of the six runs repeated 0–3 frames; ONE
(ProRes UHD → SDR) repeated 136 and skipped 137 of 450 — the same app numbers, a different phase between the two clocks.
With the card's clock 1500 ppm slow, so the phase sweeps:

| Case | Card reads the newest per frame | Card takes the oldest of a 3-frame queue |
|---|---|---|
| ProRes 1080p → SDR, 70 s | 201 repeats, 204 skips — bursts of ~7 s every ~27 s | 1 repeat, 2 skips |
| HEVC 10-bit 1080p → PQ, 58 s | 142 repeats, 144 skips — the same bursts | 1 repeat, 2 skips |

The arithmetic: arrivals alternate 33.3 / 50 ms; a 41.7 ms reader whose phase falls in the 8.3 ms between "just after the
short gap" and the next frame sees one frame twice and never sees its neighbour — a fifth of all phases, 12 distinct
pictures a second while it lasts. That is the walk's 12–13 new frames a second. Neither the codec nor PQ is in it; the PQ
clip was the only one played long enough for the phase to cross the window.

Not measured: the real card (its clock's actual offset from the screen's), Windows.

NDI (read after the walk, correcting this record's first draft): QCBridgeN does NOT read the ring the way QCBridgeA did — it
polls every millisecond and sends each frame as it lands (`clock_video = false`, a synthesized timecode); there is no
second clock sampling the ring, so the beat cannot happen there. The 33 / 50 ms arrival goes out as it is and the receiver
reclocks it.

## Proposals — rulings needed

1. **QCBridgeA takes frames as they arrive** [proposed: yes; a branch in `QCBridgeA`, never `main`]. A reader that polls the
   ring every millisecond or two and queues up to three frames; the card loop sends the oldest, repeats only on an empty
   queue, drops the oldest only on overflow; flushed on Paused / a seek so a still is immediate. Cost: about one frame of
   added delay. It also fixes QCView 2.x feeding the same sink.
2. **The port stamps the descriptor at submit** [proposed: yes] — QCView's rule; today two frames collected together carry
   one frame index and one timestamp.
3. **QCBridgeN the same way** [proposed: after the AJA walk confirms 1]. — CLOSED without a change, see above.

The walk of 1 and 2 (the user, 2026-11-10, the T-TAP Pro): "this is working much better".
4. (REVERSED the same day by spike S8 and canon row 261: the publish moved to the frame's own tick and a collector thread
   writes the ring — 33 ms → 2.5–6 ms.) **The port publishes off the main thread / reuses the wire texture / blits into the slot** [proposed: not now — the
   measurements show no cost there on this machine; revisit with the Windows box's numbers].
5. **Evening out the producer's cadence** (publish on the frame clock, not the redraw) [proposed: no — QCView publishes on
   its redraw too, and 1 absorbs it].
