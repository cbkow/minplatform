# Step 2 piece 7 — the rails, the settings service, the Settings panel, the Inspector (2026-11-07)

Canon: §25.4 (the rail reference), rows 142 (settings service), 174–177 (rails / Settings / `min-settings` / Inspector).
The user: "do side rails because we need a settings panel before we go too much further"; the two rulings on the
proposal: "go with sub-headers, right rail starts collapsed".

## What was read first (row 131)
QCView `Main.qml` (the rail row: 280 / 320, `kMinRailWidth` 200, `kMaxRailWidth` 600, `ui.layout` Settings, the
140-ms OutCubic width Behavior, `HResizeHandle` = 1 px + ±3-px margins + SplitH cursor, Ctrl+1 / Ctrl+2 / Ctrl+3,
`alwaysOpenMinimal` collapsing both), `LeftRail.qml` (`CollapsibleSection`, `revealSection`, the Settings section:
`GroupHeader`s Performance / Interface / Updates / Audio, `RowLabel` with its `help` tooltip, `FlatSpinBox` ranges,
`SlotSwitch` 110 px, the Windows-only hw-decode row, the "Reset all preferences?" dialog and what it resets),
`ViewportOverlay.qml` (the rail-open chevrons), `window_manager.cpp` + `main.cpp` (the 36 settings keys with
defaults, `QCV_SETTINGS_DIR`), `RightRail.qml` / `InspectorPanel.qml` (section list, 320 px).

## Built
- `crates/min-settings` — the service (row 176); 2 tests. `apps/qcv/src/settings.rs` — QCView's schema verbatim; 1 test.
- `min-ui`: `Rail` (geometry, collapse ramp, splitter drag, open tab, wheel scroll, overlay scrollbar, clip layer),
  `Section`, `Switch`, `NumberField` (on `TextField`; `select_all` added), `Picker` (+ `Popover::open_at_prefer`),
  `TextButton`, `Dialog`; 12 tests in the crate.
- `apps/qcv`: `settings_panel.rs` (3 groups, 20 rows, reset + confirm), `inspector_panel.rs` (4 sections), the shell
  wiring in `main.rs` (layout, content rect from the rails, routing, keys, IME to a focused field, wheel, persistence,
  window geometry restore / save, settings applied at open and live), `AudioHooks` gained `set_muted` /
  `set_sync_offset_ms` / `set_scrub_muted`, `HoverThumbs.enabled`.

## Numbers (headless `--ui-bench 20 --ui-size 2560x1440`, the 1080p fixture, both rails open)
Scene build p50 0.61 ms (was 0.06–0.14 without the rails: ~35 settings rows + ~25 inspector rows of parley layout per
build), vello render p50 0.68 ms; the settings panel 1698 px tall at 2× (fits a 1440-px-tall window's rail without
scrolling), the inspector 1404 px. The live window: 6 s unattended, the store written on exit.

## Deviations, written
QCView's accent splitter hover → grey (blue rationed); 140 ms OutCubic → 160 ms slide; `CollapsibleSection` card →
flat header; `FlatSwitch` 32 × 18 accent → S2 26 × 14 neutral; `FlatComboBox` bordered → quiet picker;
`hwScrubInterCodecs` default back to QCView's OFF on Windows; the right rail collapsed by default.

## Owed
Background section (waits for stage 4's backgrounds, row 153); Safety Guides (row 152); Add Media / project tree
(row 141); routing picker + meters into the Inspector's Audio; the Updates group; `revealSection` deep links from the
bar's guides / background / settings buttons; scroll-into-view for a focused field; AccessKit on the new nodes.

## Windows (rhombus, 2026-11-07 pm — the first Windows build of steps 2b–7)
The clone (`$HOME\Documents\GitHub\minplatform`, fed by git bundles) builds `qcv` release clean at 0d14760 with
`FFMPEG_DIR` = QCView's BtbN FFmpeg 9 and `LIBCLANG_PATH` set per command (neither is in the ssh environment). Two
WASAPI compile errors fixed blind-written code: windows 0.62 moved `AvSetMmThreadCharacteristicsW` /
`AvRevertMmThreadCharacteristics` to `Win32::System::Threading`, and `IMMDevice::Activate` is gated on the
`Win32_System_Com_StructuredStorage` + `Win32_System_Variant` features. Headless (`qcv-bench*.ps1` in `C:\qcb-lab`, logs in
`C:\qcb-lab\minplatform-logs\bench-win*.log`):
- **UI layer with both rails** (DX12, 2560×1440 @2×): scene build p50 1.12 ms p95 1.76, vello p50 1.38 ms p95 3.10 (max 23
  on the first build); the snap `fixtures/phase2/ui-rails-win.png` matches the macOS one line for line, plus the
  Windows-only "Enable hardware decoding" row. Settings panel 1772 px tall at 2× (macOS 1698: a wrap difference).
- **Audio** (NullDevice, the 1080p ProRes with a stereo track): start inside ±20 ms at 0.05 s, |drift| p50 5.2 ms; seek
  0.20 s, p50 +17.9; pause→play 0.07 s, p50 +1.6; shuttle grains fed 253 callbacks; 2× tempo p50 +13.5 (max 43, 7
  starved callbacks — the wall-paced null device meets Windows' 15.6-ms sleep granularity; not a product path); 0.5× never
  settled inside the window's 36 samples and back-to-1× sat at +25.6 — the same servo as macOS, a slower clock. A LIVE
  WASAPI check still needs a desktop session on the box (the ssh session has no audio endpoint to speak of).
- **The hw-scrub check (row 149, owed since S1) — DONE, and QCView's default (OFF) holds:** D3D12VA as the SCRUB session
  is slower than software everywhere measured — h264 1080p request→ready p50 21.2 ms vs 4.8 software (max 42 vs 13);
  h264 4K ONE request served in 2.0 s vs software p50 6.8 ms, 49 of 50 served. `performance/hwScrubInterCodecs` stays
  false (the schema), `ScrubOptions::default()` stays `cfg!(windows)` only as the harness's flag.
- **FILED — Windows software HEVC 10-bit 4K scrub serves nothing at a 40-ms cadence** (0 of 50; frames behind p50 151),
  while macOS serves 49 at p50 1.2 ms on the same fixture (15 keyframes, ~45-frame GOPs). At a 400-ms cadence each
  Windows request completes in 377–409 ms (CPU 158–287 % of one core → ~3 threads); h264 4K software on the same box
  serves 49 at p50 6.8 ms and ProRes 4K 49 at p50 19.8 ms. So the per-frame cost of the Windows software HEVC decoder in
  the scrub session (frame threads for inter codecs, row 155) is the problem, not the GOP walk: check the session's
  thread count on Windows (the 14900KF has 24 cores; 3 are busy), the BtbN build's hevc decoder vs macOS's, and whether
  the scrub session inherits `threads: 0 = auto` as FFmpeg's auto (which caps frame threads at 16) or as 1. Next
  Windows session; with a real display (row 130) still owed.

## The user's walk (macOS + Windows, 2026-11-07 pm) — fixes landed, commits 77af428 … cee7d53
"both are working well … the sidebar looks like and resizes well". Fixed on the walk: the overlay scrollbar is draggable
(12-px hit band, thumb drag, track jump, thicker on hover) and sits 4 px from the edge, past the splitter's hot zone;
pills resolve `corner-radius-full` BEFORE scaling (`paint::radius`; 0.5 × 2 read as 1 px on macOS); the number field is
one well with a bare text field inside (no inner corners at the stepper seam); the column-resize cursor whenever the
splitter is hovered; QCView's collapse arrow in a fixed 32-px header strip above the scrolling body, the reopen tab the
same square at the same height 4 px in from the window edge, both with tooltips ("Hide / Show the left rail (⌘1)");
inspector values at 12 with the labels; the wells round to 6 px and carry tooltips (frame / timecode / fps — QCView has
none); the A/V sync help texts carried in full; the app OPENS PAUSED like QCView (`--play` for the harness) — the
waveform probe suspends during playback by design, so play-on-open had left the strip empty until the first pause;
fullscreen's bands POP like the rails (row 171 revised; the 160-ms slide retired). OPEN: on Windows "not accepting
mouse clicks after leaving the window and coming back" — winit-win32's leave / enter path read, nothing found; the
Windows build carries `QCV_TRACE_INPUT=1`; the user could not reproduce it on the second session (2026-11-07 pm) — parked, the trace flag stays.

## Stage 0 — the viewport background (row 153, BUILT 2026-11-07 pm; commits 989c69d, 280e828)
QCView's `D3D11Compositor` carried verbatim (`backgroundColor` / `fillColor`, the 2026-09-15 retune): Black ·
DarkGray #1f1f1f · DarkChecker #1f1f1f / #2e2e2e · LightChecker #b3b3b3 / #cccccc, 32 DEVICE-pixel tiles (the Metal
path's contentsScale-aware size → `Engine::set_ui_scale`), the fill mixed toward Theme.bg #161616 OUTSIDE the media
rect by 0 / 1 / 0.7 / 0.7 per mode (`QCV_BOUNDS_MIX` = one value or four), everything "inside" when nothing is shown.
The constants are sRGB-encoded; decoded and placed at SDR reference white like the UI layer, never through the colour
chain. `background.wgsl` draws first (clearing the target); the picture pipelines (`stage 1+3 → target`, `blit→target`)
became blended One / OneMinusSrcAlpha and `planar4` / `packed` / the blit write premultiplied alpha (a 4444 snapshot is
premultiplied now — noted). `U` gained `bg / media / vp`. Test: `the_background_fills_under_alpha_and_mixes_outside_the_picture`.
Shell: `display/backgroundMode` applied at start and on change; the Background section sits ABOVE Settings (QCView's
order) as a fused 2 × 2 block of size-S action buttons (`ActionButton::size_s`: 24 px, bold 12 — the reference's chips; one
row of four did not fit a 280-px rail; the user: "interface is quite large text"); the bar's Background button reveals it
and Ctrl+, reveals Settings (QCView's `revealSection`: open the rail, expand that section alone, scroll to the top).

## Compact mode (row 172, BUILT 2026-11-07 pm; 8f1f1df) + the shell's shortcuts (919662c)
`apps/qcv/src/compact_strip.rs` + `Scrubber::compact`. QCView's `CompactStrip.qml` read first: a 22-px band under the
viewport — play / pause and the timecode on the left (the timecode is the move handle: `startSystemMove`), a 2-px scrub
line with the playhead and the in / out marks through the middle, the pin and the exit on the right; the window drops
its title bar (`setCompactBorderless`); fullscreen stays on F. Ours: 32 px (the family's band), `drag_window()` on the
timecode, `set_decorations(false)`, the rails and every band gone (QCView's `chromeless`), Escape leaves compact before
fullscreen, entering compact leaves fullscreen first (QCView refuses the window style while fullscreen). The pin is real
now: `set_window_level(AlwaysOnTop)` + `ui/alwaysOnTop`, Ctrl+Alt+T, selected state on both pins.
Shortcuts carried after the user's note ("remember to check shortcuts as we go along"): B cycles the background (the
ported key map already returned `CycleBackground`; the shell now acts on it), Ctrl+0 Minimal Mode (both rails collapsed),
Ctrl+9 Show All Panels, Ctrl+L loop, Ctrl+Q quit, Ctrl+= / Ctrl+- zoom the timeline about the playhead, Ctrl+, Settings,
Ctrl+Shift+C compact, Ctrl+Alt+T pin. Not yet: Ctrl+R Default Size & Position, Ctrl+3 / 4 / 5 (panels not built),
Ctrl+Shift+O, Open / Save (the project crate), Ctrl+/ (the shortcuts sheet).
The rail reopen tabs are overlays on the picture: shown while the pointer is in the window, faded out (160 ms) when
it leaves or the window loses focus (the user: "I quite like them as overlays on the viewport, but not always").

## The slow Windows close (found and fixed 2026-11-07 evening; 7256960 → 7a1222c)
The user: "it takes a bit to close" on Windows (macOS 28 ms). The teardown was timed stage by stage: audio 4 ms,
player 4 ms, waveform / thumbnails 0, gpu 17–21 ms, window 0 — and the log ENDED there: the next drop, the D3D12VA
device context (`HwDevice`, built ON the presenter's `ID3D12Device`), was being released AFTER the presenter had let
go of that device, and FFmpeg's `av_buffer_unref` → the context's uninit stalled on the dying device. The rule now:
**the hw device context drops right after the player (its decoders hold hw frames) and BEFORE the presenter**, while
the GPU device is alive. Result on rhombus: teardown 22 ms, the event loop returns 1 ms later, the process is gone
~0.5 s after that (the launcher's own stamp costs most of that; DLL detach the rest) — the user: "much faster". The
stage lines stay in the log under `close:`; the ordering is the fix, the timing the evidence.

## The Inspector, refilled faithfully (2026-11-07 evening)
The user: "look at the old inspector first and ensure we are faithfully porting over what we had for videos, some of
those fields were specific user requests". `InspectorPanel.qml` read section by section (labels, value expressions,
notes, visibility). Now on `qcv-meta` (the extractor + XMP reader QCView runs, through `MetadataService` at open —
both requests, results drained in the tick; the decoder's facts fill in until they land): every row in canon row 177.
Three pickers are live because the player backs them: Range (`Transport::set_range_override` → the stream + scrub
decoders), Mix (`AudioHooks::set_routing_mode`), Origin (`StatusStrip.origin` → the timecode base; the compact strip
follows). Meters: `AudioHooks::peak_levels` per source channel, 4-px bars, notice colour above 0.9. Pickers are the
2 × 2 block's style laid in a row (size-S chips on the card colour). Panel height with the 1080p master: 2296 px at 2×
(scrolls). Owed: Aspect + Rotation pickers (the presenter applies neither SAR nor rotation yet), the Transfer picker
(the colour stage), Copy path / Reveal (clipboard + OS reveal), hero thumbnail, Linked Projects / Playlist Contents.

## Geometry in the presenter (2026-11-07 evening; d106e95) + the Range fix (5e4fa1d) + the quiet readouts (6ca5486)
QCView's `effDim` (metal_player_renderer.mm) and `rotatedSrcUv` (d3d11_compositor.cpp) carried: the PAR widens the
larger-pixel axis in STORED space (num > den → w × num / den, else h × den / num), then the display rotation swaps the
dims for 90 / 270 and the vertex stage inverse-rotates the sampling uv — the fit, the media rect (stage 0's bounds mix),
the hover thumbnail's anchor all see display-orientation dims. `U.pad0` became `rot`. Defaults are QCView's: Aspect
Square (stored pixels untouched — QC-safe), Rotation Auto (the file's display matrix). The Inspector's Aspect (Square /
"Detected n:d") and Rotation ("Auto (90°)" / 0° / 90° / 180° / 270°) chips drive `Engine::set_geometry`; the Display row
shows what the viewport shows. Per clip in memory, reset at open; persistence waits for media items (row 141). Custom PAR
(a scalar or W:H) waits for its two fields. Test: `geometry_rotates_the_picture_and_the_par_widens_the_fit`.
Range (5e4fa1d): the override reached both decoders but the Metal zero-copy import trusted the CVPixelBuffer's range
attachment over the frame's metadata — `ColorMeta.range_forced` now lets a forced override win (Auto still defers to the
attachment, the better detector). A headless probe clicks Full and watches a pixel (0.148 → 0.185). The Inspector's chip
rows wrap like QCView's Flow. The strip's readouts are quiet text with a hover fill (the user's call; QCView's are wells).
Custom aspect (same evening): the third chip and QCView's two fields — Pixel aspect, a scalar clamped 0.05–20 and
rationalised at 1e4; Display aspect as one decimal ": 1", PAR = DAR × storageH / storageW rationalised at 1e3 (a single
scalar round-trips to a fixed point, QCView's note on why not a W:H pair). The Inspector gained field focus: keys, IME,
Enter commits, Escape reverts, a press elsewhere commits (`has_focus / key / ime / blur`), the same routing the
Settings panel has.

### The loaded row, the bin icons, the waveform on a clip switch (2026-11-07 night)
- The user: the blue in the rail "feels faded … low contrast against the grey hover background". It was the port's
  deviation, not the token: the loaded row drew the name in blue-900 over the grey hover / selection fill (3.3–3.7:1,
  under the 4.5:1 text floor) with an invented 2-px left bar. QCView's `LeftRail.qml` never puts accent text on grey:
  priority active → selected → hover, the loaded row = a muted cobalt fill (`Theme.rowActive`) + a 1-px accent border +
  the name in `textBright`, "no left rule". Carried: tokens `rail-row-active-fill` (blue-400) / `-label` (gray-900); the 1-px border was tried and
  REMOVED the same night (the user: "the outline is not very Adobe Spectrum" — a ring means keyboard focus there; Spectrum 2 marks
  the current item with fill + weight, so the loaded name is BOLD); the accent tokens themselves are unchanged (Spectrum 2 dark blue-900; QCView's
  own accent is cobalt #0189f1 on a Display-P3-tagged Qt swapchain, which is why the same hex reads more vivid there — noted,
  not carried).
- The user: "move the icons from the media items to the bins too, we don't need so many icons" — the type glyph is drawn once
  on the bin header before "Label (n)"; rows are the name only.
- Switching from a clip with audio to one without left the previous clip's peaks on the scrubber (the whole waveform block
  was behind `has_audio`). QCView's loader goes inactive without audio and draws nothing; the peaks and the request now
  clear in that case. Audio → audio was already right: the request is keyed by path, columns fill as peaks land, the old
  clip stays in the probe engine's small LRU so switching back is instant.

### Bypass as stored + the Transfer override (2026-11-07 night; canon §18.6 rows 179–184 banked, row 180 landed)
- The "Auto range is wrong for ProRes" chase ended elsewhere: the range rules are QCView's verbatim and the code values match FFmpeg;
  the bug was the no-chain display path linearising BT.1886 with gamma 2.4 on the EDR layer, which macOS then re-encodes with the
  sRGB curve — every SDR clip crushed (stored 20 → 7, 71 → 61). `Snapshot::Encoded` (matrix + range only, QCView's PNG-capture
  equivalent) could not see it. Now: with no chain the display set selects the sRGB EOTF for sRGB / BT.1886 tags (QCView's bypass =
  as stored; its compositor's `srgbToLinearSrc`), the chain-in set keeps the tagged transfer, and `Snapshot::Display` /
  `--snap-display path@N` read the screen path encoded as the OS would. Measured on the 1080p master at frame 24: display path vs
  FFmpeg rgb24 within ±2 at every probe point (it was −11 at code 71 before). Engine tests: `bypass_shows_sdr_as_stored_on_the_linear_surface`,
  `the_transfer_override_changes_how_the_clip_is_read`; `identity_chain_matches_the_fused_route` now runs on an sRGB-tagged ramp
  and asserts the BT.1886 ramp DIFFERS between bypass and an engaged chain by design.
- The Inspector's Transfer chips are QCView's six (Auto · SDR 709 · PQ 2020 · PQ P3 · HLG · Linear; Auto labels itself
  "Auto (SDR)" / "Auto (PQ 2020)" / "Auto (PQ P3)" / "Auto (HLG)" / "Auto (SDR, untagged)" from the tags, as QCView's `detectedLabel`).
  Engine-level (`Engine::set_transfer_override(TransferChoice)` → the effective meta's transfer + primaries), per clip on the project
  item (`transferOverride`, persisted), reapplied on load, reset on open. No 2.2-vs-2.4 chip on purpose (see canon §18.6 ¶7).
- Open: PQ / HLG in bypass decode to nits (the linear surface's literal "as stored for the display"); QCView's SDR-surface bypass showed
  the code values flat. Carried into the §18.6 ruling.

### The pointer grab (2026-11-07 night) — "scrubbing overruns until the end of the clip"
- The user: scrubbing with the mouse sometimes ran the playhead on to the end. The logs held nothing (the manual launches run
  without `QCV_TRACE_INPUT`); the router did. The shell hit-tests the rails first and RETURNED for any event with the pointer
  over a rail — including the primary RELEASE. A scrub drag whose release landed with the pointer drifted up into a rail never
  reached the scrubber, which stayed pressed; every later Move over the viewport then drove the playhead to the pointer's x, so
  moving right ran it to the end. A held fast-forward had the same hole: its release-on-leave never arrived and the shuttle ran
  to the end on its own. The user: "that is exactly what it is. its our leave window boundary and return fix" — the parked
  Windows "clicks lost after leaving and returning" (2026-11-07 pm) is the same swallow.
- Fix: THE POINTER GRAB — while a band node is pressed (`Scrubber::is_pressed`, `TransportBar::is_pressed` = any button down or
  the volume slider dragging), Move / Up skip the rails and the status strip and go to the bands; the compact strip's
  consumption yields to a pressed scrubber the same way. Qt's MouseArea grab, which QCView relied on without writing it down.
- Seen in the live store while looking: `playback/loopEnabled = false` and `performance/hwScrubInterCodecs = true` — both QCView
  defaults inverted, left by the stale binary earlier; with loop off a resumed playback stops at the end rather than wrapping,
  which made the overrun read as "to the end". Flagged to the user (L toggles loop; the Settings rail shows both) — not edited.

### "Scrubbing plays toward the pointer" (2026-11-07 night) — the live decoder behind the scrub slot
- The user, on 6.7K ProRes from the NAS: "it doesn't even feel like the scrub decoder is working … it looks like it's just
  playing back every frame until it gets to its destination." Everything upstream was at parity and FAST: the scrub decoder
  (QCView's, latest-wins, VideoToolbox for intra) answered cold NAS frames in 10–12 ms, the warmer and its QoS matched
  QCView, the slot is a slot of one, the shell ticks at the refresh rate. `QCV_TRACE_SCRUB=1` (new: every request, every
  served preview with request → ready, every preview shown with its lag, and per tick the slot's contents) showed the truth:
  the STREAM decoder reported `playing` while the clip was paused; each scrub publish cleared the slot and the live decoder
  refilled it behind the preview with the frames after its own position; the tick took the newest of those. 398 stream frames
  reached the screen against 132 previews.
- Two deviations from QCView, both carried now: (1) QCView's `VideoDecoder::open` "Open paused. Autoplay-on-open was
  disorienting in QC review" — ours opened in the decoding state to prefill the clocked lookahead beside a paused timer
  (the first frame comes through the seek gate either way; the lookahead fills on the first play); (2) QCView's press pauses
  the streaming decoder UNCONDITIONALLY (`videoDecoder.pause()` in TimelinePanel.qml pressAt) — ours only when the timer
  ran; now `begin_scrub` and `start_fast_seek` pause it regardless.
- The scrub bench counts stream frames shown mid-sweep and prints FAIL above 0 (49 previews shown, 0 stream frames after the
  fix on the 4K fixture). The cold-NAS sweeps on two untouched 6.7K clips: request → ready p50 12.0 / 10.4 ms (warmer on / off),
  0 frames behind — the link and the warmer were never the limit.
- Still owed from the QCView read: the timer is seeked per move here (QCView seeks it once, on release, the playhead following
  the cursor visually) and QCView's scrub SKIM AUDIO (`beginScrubAudio` → the shuttle grain engine at a velocity-EMA speed).
  Windows has its own list (software ProRes at 6.7K + CPU upload, a 1.7 s first frame, a present stall) — separate.
