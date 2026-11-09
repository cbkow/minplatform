# Step 4 — the Stream card (SRT / RTMP out, built for Louper)

Canon row 239 (ruled 2026-11-09). QCView's sources: `src/output/` (stream_sink, stream_muxer, aac_encoder, vt_video_encoder,
mf_video_encoder), `src/window/output_bus.cpp` (the stream parts), `LeftRail.qml` 2318–2605, `docs/output.md` § Stream; commits
f33b3b74 (A0), dd4f562d (A1), 71666829 + 44be3223 (A2), c765a8aa (A3). File:line references below are QCView's.

**The purpose is Louper** (the user 2026-11-09: "the intention is to use this to stream to Louper specifically so design choices
are built around that for the stream itself"). Louper's requirements as recorded when the original was designed (2026-10-12):
H.264 over RTMP or SRT, AAC (or Opus), **zero B-frames, a keyframe at most every 2 s**; its recipe = Baseline, CBR ≥ 2500 kbps,
keyframe every second; browser viewers get 8-bit Rec.709 only. The recipe in QCView's docs: 1080p, 6000 kbps, Baseline,
AAC 160, frame rate auto.

## The architecture is the original's (the user 2026-11-09: "zero copy as much as possible and using the design of the original as a guide")

The original's reasons for NATIVE encoder sessions, as decided 2026-10-12 — kept:
- the encoder is fed **zero-copy from our own GPU BGRA→NV12** (BT.709, limited range, our maths — "never swscale / NVENC defaults");
- **one OS API per platform, no vendor SDK** (Media Foundation lands on the vendor's MFT: NVENC / AMF / QuickSync);
- FFmpeg's encoder wrappers were not dependable on the Windows box (`h264_nvenc` / `h264_mf` encoded; AMF / QSV opened and emitted nothing);
- the property sets were already proven (the QCBridge capture agents);
- the pacing is ours: a paced clock, **≤ 3 frames in flight, skip rather than queue**;
- FFmpeg only for AAC (its native encoder), the mpegts / flv muxers and the srt / rtmp(s) protocols.

(A first proposal here — FFmpeg's `h264_videotoolbox` / `h264_mf` behind a read-back — was withdrawn: the read-back costs one to
two frames of latency and two copies per frame.)

| Piece | The port |
|---|---|
| macOS video | a native VideoToolbox session, QCView's property set; the encoder owns an IOSurface-backed NV12 pool; the output pass writes the planes through wgpu (Metal textures over the IOSurface planes); handed over when the GPU is done |
| Windows video | a native Media Foundation session, QCView's settings. **Open: the surface.** QCView rendered on D3D11 (MF's own device); the port renders on DX12 → a SPIKE (`docs/spikes/`) before the Windows build: can a DX12 frame reach a hardware H.264 MFT with no CPU copy (a texture shared into a D3D11 device, or a D3D12-aware session)? Reference: QCView on the box — 6.05 Mbps, exact pts, picture within 1 code value |
| Audio, mux, transport | FFmpeg: `aac`, mpegts over `srt://`, flv over `rtmp(s)://` (both OS builds checked 2026-11-09: encoders, muxers, protocols present; Windows TLS = Schannel) |
| Session | QCView's StreamSink: paced / audio / mux threads, the 96-unit queue, reconnect |
| Picture | the existing output pass (row 237) at the stream's raster + the layers (row 238) |

## What QCView does (the spec)

**Config** (`output/stream/<key>`, read at Start): `url` ""; `key` ""; `bitrateKbps` 6000 (500..50000, step 500); `resolution`
"1080p" (UI order 1080p, 720p, 540p, 2160p → 1920×1080, 1280×720, 960×540, 3840×2160); `fpsMode` "auto" | 24 | 25 | 30;
`profile` "baseline" | "main"; `audioKbps` 160 (Off 0, 128, 160, 192, 256, 320); `srtLatencyMs` 120 (20..5000, step 20). Never
persisted: the running state (never on at launch), the key's reveal. Fixed: connect timeout 5000 ms, pbkeylen 16; passphrase has
no UI.

**URL + key:** `rtmp://` / `rtmps://` → trailing `/` stripped, then `url + "/" + key`. `srt://` → the key is the `streamid`
protocol option unless the URL's query already names one. Errors: "enter the server URL first", "no hardware encoder on this
platform yet", "stream needs a URL and an even raster", "the URL must start with srt://, rtmp:// or rtmps://: <url>".

**Session:** Start → state "waiting for the first frame" → the paced thread blocks for the first surface → "opening the
encoder" (GOP = round(fps) frames) → connect ("connecting to <url>", "connecting (attempt N)", "connecting failed: <err> —
retrying in N s") → "streaming". Rate = the fixed choice, else the source's rate at Start, else 30; fixed for the session.
Clock: steady, t0 after connect, tick k due at t0 + k·period; each tick encodes the newest surface or re-encodes the last
(`repeated`); more than 2 periods behind → skip ticks (no burst). A new surface replacing an untaken one = `skipped`. A keyframe
is forced on the first frame and after a reconnect.

**Video (common):** H.264, NV12 8-bit video range in, Annex-B access units out (4-byte start codes), SPS / PPS in band on every
keyframe, no B-frames, pts = dts in 90 kHz.
- *VideoToolbox:* hardware required; `RealTime`, no frame reordering, `PrioritizeEncodingSpeedOverQuality`, `ExpectedFrameRate`,
  `MaxKeyFrameInterval` = GOP, `MaxKeyFrameIntervalDuration` 1.0, `AverageBitRate` = kbps·1000, `DataRateLimits` = [2× the byte
  rate, 1 s], `ProfileLevel` Baseline / Main AutoLevel, entropy CAVLC (Baseline) / CABAC (Main), primaries / transfer / matrix
  709. Pool: IOSurface-backed, Metal-compatible, ≥ 4 buffers. AVCC → Annex-B by hand, parameter sets from the format description
  prepended on keyframes. Close: CompleteFrames, invalidate.
- *Media Foundation:* the first hardware H.264 MFT (async), `MF_LOW_LATENCY`, on the renderer's device through
  `IMFDXGIDeviceManager`; `AVLowLatencyMode`, rate control CBR, mean bitrate, 0 B-pictures, GOP, QualityVsSpeed 33; output type
  H264 + profile + 709 tags + 16–235; input NV12; forced keyframe by `AVEncVideoForceKeyFrame`; pts from a FIFO of submitted
  frames; SPS / PPS cached and prepended when a keyframe lacks them; input queue cap 4. Traps recorded by the original: the
  session may start before the device exists (create surfaces on first acquire); the wire texture must be unbound as a render
  target before it is read.

**Audio:** FFmpeg `aac`, 48 kHz stereo FLTP, GLOBAL_HEADER, frame 1024. The tap (before Listen / mute / volume; zeros when the
master mute or the feed mute is on) → a 2 s ring → an audio thread: ≥ 1024 frames buffered → encode (more than 8 frames buffered
→ drop to 7); else, when 80 ms behind the clock, a silent frame; pts = samples emitted + one video period. Off = no audio stream.

**Mux:** `flv` for rtmp(s) (`rtmp_live=live`, `flvflags=no_duration_filesize`; the header waits for the first keyframe's SPS /
PPS when none is known), `mpegts` for srt (`mode=caller`, `latency` = ms × 1000, `connect_timeout` 5000, `pkt_size` 1316,
`transtype=live`, `streamid`, `mpegts_flags=+resend_headers+initial_discontinuity`). Video stream 1/90000 with 709 / MPEG-range
tags; audio 1/48000. `av_interleaved_write_frame` on one mux thread; a queue of 96 units (video + audio), overflow drops the
oldest (`dropped`). A failed write → reconnect: close, clear the queue, "reconnecting (N)", 1 s, then 1-2-4-8-10-10… s until
Stop; on success wait for a keyframe (forced), the encoder and the clock never stop, pts are not reset.

**Card:** title "Stream" + a state word (streaming / reconnecting / connecting / failed / starting / "no hardware encoder");
the line: "%1 kbps · %1 fps · %n encoded, %n repeated" + (when > 0) ", %n skipped", ", %n dropped", ", %n send error(s)",
", %n reconnect(s)"; any other running state = the state string; stopped = the error, or the summary "%1 kbps · <resolution> ·
Auto fps | %1 fps · Main | Baseline · no audio | AAC %1 · SRT %1 ms". URL (placeholder `srt://host:port`) and Key (placeholder
"Stream key (SRT stream id)", hidden, an eye) always visible; the six settings behind the chevron, each with its help text;
everything locked while running. No shortcuts.

**Picture:** BGRA8 SDR Rec.709 composite, letterboxed over black, layers = the Guides / Notes toggles; NV12 = BT.709 limited:
`Y = dot(rgb, (0.2126, 0.7152, 0.0722))·219/255 + 16/255`; chroma from the 2×2 average: `Cb = (B−Y)/1.8556·224/255 + 128/255`,
`Cr = (R−Y)/1.5748·224/255 + 128/255`; on encoded (non-linear) RGB, saturated first. The original's A0 bar: a paused frame
decoded back matches the capture within 1 code value on flat regions; constant pts delta; GOP exact; VUI 709 tv.

## Rulings (the user 2026-11-09: "sounds good to me")

| # | Subject | Ruled |
|---|---|---|
| 1 | Encoders | native VideoToolbox / Media Foundation, zero-copy, the original's design; Windows surface = a spike first |
| 2 | Card | the port's card (status dot, on / off switch, two quiet lines for the state and the counters); URL + key always visible with the eye; the six settings behind a disclosure; QCView's defaults, ranges, option order, help texts |
| 3 | Layers | follow the Guides / Notes toggles LIVE, under row 238's rule (QCView froze them at Start) |
| 4 | Bad URL | refused at Start with QCView's message (QCView retried a wrong scheme forever) |
| 5 | The key | stored in the settings as QCView does; redacted wherever shown or logged (QCView printed the RTMP key in the state line and the log) |
| 6 | Timestamps | exact at fractional rates: pts = round(tick × 90000 × den / num) (QCView rounded the per-frame duration: video ran ~1.2 s / h ahead of audio at 59.94) |
| 7 | Paused at Start | the current frame goes out at once (QCView could wait at "waiting for the first frame") |
| 8 | Stop | never blocks the window (QCView joined on the GUI thread, up to the 5 s connect timeout) |
| 9 | Typed, then Start | Start commits a field still being edited |
| 10 | The rate shown | the session's rate to three decimals ("23.976 fps") |
| 11 | State strings | translated; drafts in 11 languages for those QCView left as raw English |
| 12 | Kept as QCView | no passphrase field; the master mute silences the stream; VideoToolbox's rate control (average + the 2× / 1 s cap); Auto follows any source rate; the kbps shown is the elementary streams' |
| 13 | The feed mute | stays ON by default (row 237); **switching the stream on flips it off** — a stream to Louper starts with sound (the user 2026-11-09: "We can flip it when we enable the stream. It makes sense"); the header speaker still mutes every output |

## Order (the original's A0–A3)

1. **4a** macOS, headless: the VideoToolbox session + the NV12 pass + mpegts over SRT into a local listener (the A0 bar). — **DONE 2026-11-09**: `crates/qcv-stream` (`vt.rs`, `nv12.rs`, `mux.rs`, example `stream_out`); 1080p25 Baseline: pts delta 3600, GOP 25, no B-frames, VUI bt709 tv, every flat patch within 1 code value; 23.976 Main: pts average exactly 3753.75.
2. **4b** audio, the session's lifecycle in the app, the card. — **BUILT 2026-11-09** (macOS; the user's walk owed): `aac.rs`, `sink.rs` (the session), the outputs' publish pass feeds the encoder's surface (`apps/qcv/src/outputs.rs`, `stream.rs`), `stream_card.rs`. From the app to a local SRT listener: H.264 Baseline + AAC, pts exact, 6043 kbps under motion against 6000, audio continuous, the guide in the picture; quitting waits for the session's threads (a quit during a connect crashed in the network library's teardown otherwise). `QCV_STREAM_SPIKE=<url>` starts a session at launch (QCView's dev aid). SRT only until 4c.
3. **4c** RTMP / FLV, reconnect, counters; the walk; the Louper room is the user's test. — **BUILT 2026-11-09** (macOS): FLV over rtmp(s) with the header deferred to the first keyframe's SPS / PPS (cached for a reconnect), a 5 s socket timeout (`rw_timeout` — RTMP's own `timeout` option means LISTEN), and the test matrix below.
4. **4d** the Windows spike, then Windows (in the one Windows walk). — **Spike S7: GO** (row 241, `docs/spikes/S7-winstream.md`); **the session BUILT 2026-11-09** (`mf.rs`), tested headless on the box — see "Windows, tested in a vacuum" below; the app's window on the box is the Windows walk.

## Tested in a vacuum (2026-11-09, macOS)

The user 2026-11-09: "we won't be able to do a full louper test before shipping" — so everything that can be checked without
a Louper room was, from the real app (an isolated instance, synthetic clips) into local receivers: our own FFmpeg (SRT), Homebrew's
FFmpeg (RTMP — a different build), and our own SRT input.

**Against Louper's published requirements** (H.264 over RTMP or SRT; AAC; zero B-frames; a keyframe at most every 2 s; its
recipe: Baseline, CBR ≥ 2500 kbps, keyframe every second): every run below is H.264 Baseline (or Main where chosen) with no
B-frames, a keyframe every 1.0 s (0.96 s at 23.976), AAC-LC 48 kHz stereo, BT.709 limited-range tags; the bitrate under motion
holds the target (6043 kbps average against 6000; per second 5134–6468).

| # | Scenario | Result |
|---|---|---|
| 1 | RTMP (FLV) to an RTMP listener, the key as the stream name | 1080p25 Baseline level 4.0 + AAC, first frame a keyframe, frame gaps a constant 40 ms; the key appears in no log |
| 2 | SRT, encrypted (passphrase, 16-byte key) + a stream id | streams; a WRONG passphrase never streams (retries with backoff, nothing received) |
| 3 | SRT reconnect — the listener ends mid-session, a new one comes up later | "reconnecting", failed attempts with 1-2-4 s backoff, then streaming; the new listener starts on a keyframe (passed 4 runs; in 1 earlier run the second listener never received — most likely the single-connection test listener, not reproduced) |
| 4 | RTMP reconnect, the same way | refused attempts, then streaming; the new listener starts on a keyframe, audio continuous |
| 5 | A 23.976 source, frame rate Auto, 720p, Main, 4000 kbps, audio off | 24000/1001, level 3.1, frame gaps 41.70 / 41.71 ms (exact), video only |
| 6 | Lip-sync: a flash + a beep in the source, measured in the received stream | a session started mid-playback: audio +1…+13 ms; started together with playback: −33…+12 ms; 3 minutes: no drift, memory flat after the first minute. (Before a fix: +67 ms for the whole session when started with playback — see below) |
| 7 | Our own SRT input as the receiver (`stream_probe`) | 1920×1080 h264, bt709 tags, 0 drops |
| 8 | RTMPS | the TLS handshake is made and an UNTRUSTED (self-signed) certificate is refused — so a full publish needs a server with a real certificate: **not tested** |
| 9 | A UHD source into 2160p30, 16000 kbps | level 5.1, 16110 kbps average, playback dropped 0 frames meanwhile |
| 10 | Quit while connecting / while streaming | clean exits (a quit during a connect crashed before the session's threads were waited for) |

**End-to-end latency** (the user's walk 2026-11-09: "just testing srt locally here is like a 15 second lag" — with a stock `ffplay`): measured with `stream_session` stamping each flash as it is published (`STREAM_FLASH_CLOCK`) and a NO-BUFFER receiver stamping its decode: **about 390 ms** on loopback (381–394 over six flashes, SRT latency 120 ms included). The 15 s is the player: a stock `ffplay` gathers its probe data first (up to 5 MB or 5 s — far longer on a quiet picture) and then plays from the first packet at 1×, so its start-up delay stays as lag. A receiver for a walk: `ffplay -fflags nobuffer -flags low_delay -framedrop -analyzeduration 200000 -probesize 500000 -sync ext "srt://127.0.0.1:9000?mode=listener"`.

**Found and fixed by the matrix:** RTMP's `timeout` option turned the sender into a listener (now `rw_timeout`); a quit during a
connect crashed; audio stamped late after a start-up burst — the tap's first second arrives late, after silence was already
filled in, and QCView's rule (drop only above 8 buffered frames) left the rest of the session ~67 ms late: now what waits
plus what went out may not run more than two frames ahead of the clock.

**Known, by design (QCView's too):** the clock samples the newest picture, so a one-frame event can be skipped or doubled when
the source's rate equals the clock's (12 of 180 one-frame flashes in 3 minutes) — "review latency, not sync".

**Not testable here:** the Louper room itself (its ingest's handshake, its stream-key form, its player's tolerance); RTMPS end
to end; a lossy or slow network (everything above is loopback); Windows (4d). Dev aids: `QCV_STREAM_SPIKE=<url | empty = the
saved settings>` starts a session at launch, `QCV_STREAM_SPIKE_DELAY=<s>` starts it later, `QCV_STREAM_SPIKE_PASSPHRASE`.

## Windows, tested in a vacuum (2026-11-09, the box: RTX 4090, Windows 11 26200)

The Windows session is `crates/qcv-stream/src/mf.rs`: QCView's Media Foundation session on a D3D11 device, its NV12 surfaces
shared from wgpu's DX12 device (spike S7's route). Everything below ran over SSH — no window, nothing on the desktop — through
two harnesses: `stream_out` (the encoder + the muxer) and `stream_session` (the REAL session as the app drives it: the
work-done hand-over, real-time audio, a paused stretch; its source carries a flash + a beep every 4 s), into the box's own
FFmpeg as the listener; the recordings were analysed on the Mac. Synthetic pictures only.

| # | Scenario | Result |
|---|---|---|
| W0 | `stream_out`, 1080p25 Baseline over SRT | 200 / 200 frames, pts delta a constant 3600, a keyframe every 25, Constrained Baseline level 4.0, bt709 / tv; **all 16 patches decode back exact** |
| W1 | the session, SRT, 1080p25 + AAC, the picture paused 4 s | continuous 40 ms frame gaps through the pause (100 repeated), AAC continuous; lip-sync +35 ms |
| W2 | RTMP (FLV), the key as the stream name | first frame a keyframe, AAC; lip-sync +13 ms |
| W3 | SRT reconnect — the listener ends at 6 s, a new one at 16 s | reconnecting → a failed attempt → streaming; the new listener starts on a keyframe |
| W4 | 23.976 (24000/1001), 720p, Main, no audio | Main level 3.1, frame gaps 41.70 / 41.71 ms, a keyframe every 1.001 s |
| W5 | 2160p30, 16000 kbps | level 5.1, every frame, AAC; lip-sync +36 ms |
| W6 | three minutes | 4498 of 4500 frames encoded, 0 dropped, working set flat at 225 MB; lip-sync +16 ms on all 44 markers — no drift |
| W7 | SRT encrypted + a stream id; a wrong passphrase | streams; the wrong passphrase never connects (0 bytes received) |

**The app itself on the box** (2026-11-09, the user: "sure, go for it" — isolated instances on the desktop session by a one-shot
task, synthetic clips, no input sent; `QCV_STREAM_SPIKE=<url>` starts the session):

| # | Scenario | Result |
|---|---|---|
| D1 | SRT, the moving clip, the 16x9 guide on | 1080p25 Constrained Baseline + AAC, a keyframe a second, **6097 kbps against 6000** (per second 5744–6326); the guide is in the received picture |
| D2 | RTMP, the key in the stream name | starts on a keyframe, 5680 kbps, AAC; the key appears in no log |
| D3 | the flash + beep clip, 40 s | lip-sync: audio 39 ms EARLY on eight of nine markers (the first +10 ms) — steady |
| D4 | the 23.976 clip, frame rate Auto | 24000/1001, GOP 24, frame gaps 41.70 / 41.71 ms, 6120 kbps |
| D5 | the UHD clip into 2160p30 at 16000 | level 5.1, 15174 kbps (per second 14155–16209); playback dropped 4 of 355 frames meanwhile |
| D7 | quit while connecting (no listener) | a clean exit |

**Not tested on Windows:** the card by hand (typing, paste, the switch — the Windows walk); AMD / Intel encoders (the box has
NVIDIA's only); RTMPS; a lossy network. Note: while a connection is down the queue keeps filling and overflowing,
so `dropped` climbs through a reconnect (591 in W3's ten seconds) — QCView's accounting.
