# Step 2b — audio: QCView's design whole (canon row 139; §24's audio row) — 2026-11-07

**Question:** can the player's audio be carried from QCView module by module (row 131) and play in sync through a direct
device layer (not cpal), with J / K / L audible and the scrubber's waveform fed from a probe engine — measured, not felt?
**Bar (row 139 + §24):** video is the master; the servo holds drift inside the ±20 ms quiet band at 1× with no re-seeks
and no underruns; a seek or a pause → play re-converges in well under a second with one fade, no pop; the shuttle feeds
the device from grains; the waveform's visible window is probed on demand and memoised on the stride ladder.

## What was read first (QCView `src/audio/`, 8.7 k lines)
`audio_player` (servo tiers, hold / release, deferred stop, the soft limiter), `audio_sync_servo` (PI, tiered caps,
slew), `audio_drain_stage` (the fades at every break), `fractional_resampler` (Catmull-Rom, 5-frame history),
`audio_ring_buffer` (stale marking + generations), `audio_decoder` (best stream, swresample, back-pressure, seek
trim, peaks), `audio_routing_matrix` (Auto / 5.1 / 7–8), `audio_chunk_reader` (synchronous, continuation fast path),
`shuttle_audio_engine` (grains), `waveform_probe_engine` + `waveform_strip` (the stride ladder), `coreaudio_device`
(AUHAL default output), `tempo_stage` (SoundTouch), `multi_stream_audio_decoder` (avfilter merge / pan).

## What landed (`crates/qcv-player/src/audio/`, slices 1–3)
| Module | Port of | Notes |
|---|---|---|
| `ring.rs` | `AudioRingBuffer` | f32 samples, SPSC, `mark_stale` / `discard_stale` / generations; test |
| `resampler.rs` | `FractionalResampler` | verbatim; the ramp test pins the 3-frame base delay with no drift across blocks |
| `servo.rs` | `AudioSyncServo` | verbatim constants (kP .15, kI .01, ±.2 % → ±1.5 % over 20–100 ms, slew .001); test |
| `drain.rs` | `AudioDrainStage` | verbatim over an `AudioSource` trait; counts underruns and fades; test (fade-in, stale tail fades out uncounted) |
| `routing.rs` | `computeRoutingMatrix` | verbatim; test |
| `ffmpeg_audio.rs` | `openAudioStream` / `setupResampler` / `setupSwr` | one `AudioCtx` both decoders share; `rematrix_volume` 0.9 for swresample's own fold; the peaks per source channel for FLT / FLTP / S16 / S16P / S32 / S32P |
| `decoder.rs` | `AudioDecoder` | the decode thread verbatim (back-pressure 100 ms, EOF idle on the seek condvar, seek → stale mark + flush + trim to the target sample) |
| `chunk_reader.rs` | `AudioChunkReader` | verbatim (2-ms continuity, seek-and-prime, FIFO) |
| `device.rs` + `coreaudio.rs` | `CoreAudioDevice` | one trait; the AUHAL default-output unit with the stream format, render callback, buffer frame size and latency property through `objc2-audio-toolbox`; `NullDevice` paces the callback at wall time for headless runs; WASAPI = the rhombus pass |
| `player.rs` | `AudioPlayer` | the `AudioHooks` impl: play / pause (deferred stop 30 ms via `tick`), seek (anchor + servo reset), hold / release (skip ≤ 400 ms, else re-seek), `update` (the three tiers verbatim), mute / volume / soft limiter in the callback, the shuttle path in the callback, `AudioStats` (drift samples, re-seeks, holds, skips, underruns, fades, callbacks, contended) |
| `shuttle.rs` | `ShuttleAudioEngine` | verbatim (80-ms grains, pitch cap 1.0, snap max(.25 s, .25 × speed), reverse grains reversed with 5-ms edges, 120-ms hold silence, reader LRU 2, the ring flush on a direction flip) |
| `waveform.rs` | `WaveformProbeEngine` | verbatim (5 ms · 2^rung ladder, linear ≤ 60 s else 20-ms probes, synthesis from finer rungs, refine vs uncovered, abort on a newer vantage / playback, requeue); `sample_columns` falls back across rungs; `data_generation` replaces the Qt signal |

**Transport:** `AudioHooks` gained `open / close / tick / set_volume / is_muted / volume / stats / take_drift`;
`Transport::set_audio` installs a player and opens the active clip's audio; every refresh ticks the hooks; **the hold is
now released on the first frame SHOWN after `play` as well as on a seek landing** (QCView releases on the first frame
published after play; the port had only the seek-gate path, so a plain pause → play kept the hold forever — found by the
bench). `set_tempo` off 1× makes the audio SILENT until slice 5 (SoundTouch) lands — written in the player rather than
letting the servo re-seek every second against a stretched clock.

**App:** the player installs on open (`QCV_NO_AUDIO=1` leaves it out); mute is live in the bar (icon mirrors the state,
the deliberate-grey selected fill); the volume slider (`min-ui::Slider`, §25.4's rule: 1-px track, 16-px hollow ring,
no accent) sits in mute's cluster at QCView's 90 px; `--stats` prints an audio line (drift p50 / p95 / max, re-seeks,
holds, skips, underruns, fades, callbacks); the scrubber draws the waveform from the probe engine's columns — the band's
fill at gray-800 × 0.2 (lifted from 0.12: a −20 dBFS commercial track was invisible at 0.12); the probe suspends while
the transport runs. **`--audio-bench SECONDS file`** = the harness: the Transport through the silent paced device.

## Numbers (M5 Max, the 1080p ProRes master with its stereo track, `--audio-bench 6`)
| Segment | first inside ±20 ms | \|drift\| after settling p50 / p95 / max ms | re-seeks | underruns | fades |
|---|---|---|---|---|---|
| play from 0 | 0.08 s | 5.7 / 14.2 / 16.8 | 0 | 0 | 1 (the fade-in) |
| seek to ⅓ while playing | 0.15 s | 10.3 / 24.0 / 27.9 | 1 (the discontinuity tier, by design) | 0 | 3 |
| pause 300 ms → play | 0.08 s | 5.9 / 13.9 / 21.8 | — | 0 | 5 (hold out / in, the skip) |
| shuttle ff 1.5 s + rw 1 s | — | 255 device callbacks fed by grains, both directions active | — | 0 | — |
| play on after the shuttle | 0.06 s | 5.5 / 14.1 / 17.3 | — | 0 | — |
**Unattended window run, real CoreAudio output, UI layer on, 25 s, no hands (2026-11-07):** drift p50 between −8 and
+7 ms every 2-s window (p95 ≤ 16, max 17.5), 1 hold, 0 re-seeks, 0 underruns, 0 dropped frames, present max 17 ms, the
UI layer 1.25 ms — the first attended walks had read +20–30 ms with 130-ms present stalls: a pause / play every few
seconds re-anchors the servo before it converges, and scrubbing fires the thumbnail strip's precache burst, which now
runs at utility QoS. Two app fixes fell out: the harness's play-on-open ran BEFORE the player was installed (silent
until the first space), and `--play` exists for unattended measurement. The decode thread + probe at idle:
process ≈ 60 % of one core during 1080p playback with audio (decode threads included). Waveform: 2 528 columns of an
11-s window probed in < 3 s on open (the linear regime).

## Found by listening with the real device (2026-11-07, the user's walks + traces)
- **`AudioOutputUnitStart` blocks the UI thread ~105 ms** on this Mac (`Stop` 2–7 ms). QCView starts the unit on play and
  stops it 30 ms after pause; with a user pausing every few seconds that was a 128-ms present stall and a burst of
  dropped frames at every play. **Divergence:** the unit starts once at file open and stays running, rendering silence
  while paused (the drain's hold path); the deferred stop remains behind `STOP_DEVICE_WHEN_IDLE`.
- **QCView's `PlaybackTimer` loses one refresh at every play and seek landing**: `m_firstUpdate` made the next update
  only re-base the delta, so the clock skipped the time between the landing and that update — ~50 ms here, and the
  audio ran that far ahead of the picture from the first frame until the servo ground it out at ±0.2 %. **Divergence:**
  `play` / `seek` take the base NOW; no tick is skipped and no stale delta can land. (Also filed in canon §17.6.)
- **`releaseHold` ignored the device latency**: audio resumed one playout latency behind the picture (+11 ms on
  CoreAudio, +20 on some devices) at the quiet band's edge. **Divergence:** the skip includes the latency.
- After the three: a cold start reads −12 … +19 ms over the first frames and settles to 0 ± 10 within ~8 s; unattended
  steady state −8 … +7 ms p50, p95 ≤ 16, no re-seeks, no underruns, no dropped frames.
`QCV_AUDIO_TRACE=1` (with `RUST_LOG=info`) prints the servo's inputs for the first 60 updates and the timer around play.

## Slices 4–5 (2026-11-07, later the same day): tempo and multi-stream
- **`tempo.rs`** — `TempoStage` (QCView `tempo_stage.cpp`) over the `soundtouch` crate (SoundTouch 2.3.2 compiled in,
  LGPL-2.1; QCView links 2.4.1): a producer-side WSOLA stage on each decode thread, 1.0 = byte-identical bypass,
  0.25–4× constant pitch, a change applied lazily at the next `put` (state cleared), flushed on seek; the knobs are
  shared atomics (`TempoControl`), the stretcher is thread-confined. Test: 2 s in at 2× → ≈ 1 s out. The Transport's
  `set_review_speed` already called `set_tempo` + a re-anchor seek (QCView's rule); the player scales its
  consumption-based position by the tempo. The "silent off 1×" stand-in is gone.
- **`multi_decoder.rs`** — `MultiStreamAudioDecoder` (QCView `multi_stream_audio_decoder.cpp`): every decodable audio
  stream gets a codec; an avfilter graph `abuffer`×N → `amerge` → `pan` → `aformat` (flt / 48 k / stereo) →
  `abuffersink` is built from the routing plan (`resolveRouting` verbatim: Auto = channels 7–8 when ≥ 8, the BS.775
  fold when ≥ 6, else the first pair; a pair across streams gets `pan=stereo|c0=cL|c1=cR`, a same-stream stereo pair
  passes through; Downmix5_1 / Stereo7_8 with QCView's fallbacks) and rebuilt on a seek (`avformat_seek_file` over the
  whole file, every codec flushed) or a routing change; peaks per real channel before the fold with a 16-channel cap;
  the sink's constraints set through `av_opt_set` strings (FFmpeg ≥ 7.1 array options). `source.rs` = `IAudioSource`,
  the trait both decoders implement so the player dispatches on `audio_stream_count(path) ≥ 2`. 5 routing-plan tests.
- **Measured** (`--audio-bench`, the derived two-mono-track fixture): the graph builds ("merge 2 streams, pan (none)"),
  plays, seeks and tempo-shifts like the single decoder. The 4K master turned out to carry ONE audio stream plus a data
  track (`ffprobe -select_streams a` had printed an empty line I counted), so it takes the single decoder — correct.
- **Tempo numbers**: 2× holds |drift| p50 ≈ 30 ms, 0.5× ≈ 4 ms, back to 1× re-anchors (QCView's structure: the position
  estimate ignores WSOLA's input window, the servo trims against it; the same in C++).
- **One more timer finding, from the bench's trace**: `PlaybackTimer::seek` returned early when the position was
  unchanged (the open at 0; the landing confirming an immediate seek) WITHOUT re-basing — and the seek gate skips
  `update` while a landing is in flight, so the first update after the gate integrated the whole landing time at once:
  the clock 20–40 ms ahead of the audio after every seek. Re-basing before that return fixed it. Headless now: start
  inside ±20 ms at 0.10 s (p50 +9 converging), seek 0.18 s, pause → play 0.06 s (p50 +1); the unattended window run
  walks +11 → −1 ms over 15 s. The residual post-seek +10–20 ms that the servo then grinds out is QCView's own scheme
  (pre-seek audio rolls uncounted until the decoder services the seek).
- The bench's silent device repays starved sleeps instead of resyncing (a resync had read as servo drift).

## Not yet
- **Routing UI** (Auto / 5.1 / 7–8), the per-channel meters, the A/V sync-offset knob — the player and decoders carry them; no node yet.
- **The review-speed chip** in the transport bar (R / ⇧R work from the keys).
- **WASAPI** on rhombus (shared / event-driven / MMCSS); no Linux.
- Dual's mixer, live audio (QCBridgeAE) — Phase 4.
