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
