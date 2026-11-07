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

