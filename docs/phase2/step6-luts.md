# Spec: Scene LUT / CDL / Correction ID and Export LUT — 2.x read, 3.0 state, port proposal

Read-only spec, 2026-11-10. Nothing was modified in any repository.

- `Q/` = `../QCView-Player` (HEAD `cb890143` = v2.7.3 + 2 commits, neither touching colour).
- `M/` = `../minplatform` (HEAD `8b01ff0`, canon rows to 254).
- **[V]** = verified by reading the code at the cited lines. **[I]** = inferred (from OCIO's documented behaviour, from
  comments, or from code paths not traced to the end). Nothing was run.

## 0. The three answers first

1. **The controls are absent in 3.0 — confirmed [V].** The OCIO side of the drawer has Config · Input · Look · View ·
   Highlight Knee and nothing else (`M/apps/qcv/src/color_drawer.rs:194-206`, column list `:272-273`). No Scene LUT
   tile, no Correction ID field, no Export LUT button, no file dialog for a LUT.
2. **The stored LUT path is carried, and the chain stage exists — confirmed [V].** `OcioChain.scene_lut`
   (`M/apps/qcv/src/color_model.rs:10`), written as `sceneLut.path` (`:53`), read (`:58`), handed to the engine
   (`M/apps/qcv/src/color.rs:235`), built as an OCIO `FileTransform` in both the plain and the Knee-split chain
   (`M/crates/qcv-color/src/ocio.cpp:130-134`, `:165`). The Correction ID is NOT carried: written as the constant `""`
   (`color_model.rs:53`), never read, never passed to OCIO (no `setCCCId` in `ocio.cpp`).
3. **A 2.x project's stored Scene LUT is silently ignored when it opens in 3.0 — verified [V], and it can then be
   silently erased.**
   - 3.0 reads `ocioClip` as a chain only when it carries 3.0's own marker `"engine": "ocio"`
     (`color_model.rs:83`) AND a non-empty `"view"` (`:57`). 2.x writes neither: its pin has only `input`, `look`,
     `sceneLut {path, cccId}`, `knee`, `hostDisplay` (`Q/src/color/ocio_chain_spec.h:85-110`). So a 2.x pin falls
     through to "follows the project default" (`color_model.rs:84-88`), and `color_on_for_project` does not even turn
     colour on for it (`color.rs:148-151`). No toast, no log line, no marker in the drawer.
   - The pin stays in the file while the clip stays on minColor or the default (`to_clip_maps` keeps the old map,
     `color_model.rs:76`).
   - The first time an OCIO chain is written to that clip (New OCIO chain…, an OCIO preset, Apply to all clips), the
     whole `ocioClip` map is replaced (`color_model.rs:77`, `:52-55`): `sceneLut.path` becomes `""`, `cccId` `""`,
     `hostDisplay` is dropped. Saving the project then loses the 2.x LUT pin for 2.x too.
   - What IS applied today: a chain 3.0 itself wrote with a non-empty `sceneLut.path` — reachable only by hand-editing a
     project / `color-presets.json`, since no UI sets it. Even then a live change of the path would not rebuild the
     pass: the rebuild key omits it (`color.rs:232`; the thumbnail key has it, `thumb_color.rs:56`).
   - Context that softens this [V]: 2.x itself opens with colour off (`m_engaged = false`,
     `Q/src/color/ocio_config_manager.h:373`) and does not store config / display / view in the project, so in 2.x the
     pinned LUT also shows only after the user turns OCIO on under a config where the pin's names exist.

## 1. What 2.x does

### 1.1 Controls (all in `Q/src/ui/qml/ColorPanel.qml`, translation context `ColorPanel`)

The panel is three groups: Setup (Presets | Config), **Clip** (Input, Look, Scene LUT, Highlight Knee), **View** (Output,
View, Display LUT) — header comment `:50-58`, user doc `Q/docs/color.md:25-32`.

**Scene LUT tile** — `LutTileColumn` instance at `:917-935`, component at `:2298-2595`. [V]
- Position: Clip group, after Look, before Highlight Knee. Hidden under the minColor engine (`visible:
  !root.minColorMode`, `:919`).
- Collapsible column. Default **collapsed** (`sceneLutExpanded: false`, `:76`; persisted in QSettings category `color`,
  `:73-81`). Collapsed = a 32-px strip with the rotated title, a 6-px accent dot when the clip has set it
  (`ClipSetDot`, `:1403-1409`, used `:2567`), the `cube` icon (green when a file is loaded), a caret. Expanded =
  130 min / 160 preferred px (`:2314-2315`).
- Collapsed strip tooltip: `"%1 — %2"` (title — file name) when loaded, else `"%1 — click to expand"` (`:2540-2544`).
  Click expands.
- Expanded body: one big click target (`:2400-2406`) → opens the picker. Text `"+ Add LUT"` (empty) / `"Replace LUT"`
  (loaded) (`:2431-2435`); under it the file's base name in the mono font, middle-elided (`:2444-2456`); when empty the
  italic hint `"click to choose"` (`:2468-2478`).
- Clear: a 22-px ✕ at the tile's top right, only when loaded, tooltip `"Clear LUT"` (`:2486-2517`) → sets the path to
  `""` (`:928`).
- Header: title `"Scene LUT"` (`:921`, translator comment "A LUT applied in scene-linear space, before the view"),
  a ↺ `RevertButton` when the clip has pinned this slot (`:2343-2349`; tooltip `"Set on this clip — click to go back to
  the default"`, `:1397`; action `setSlotPinned("sceneLut", false)`, `:1394`), a collapse caret with tooltip
  `"Collapse"` (`:2377`).
- **Correction ID** field: a `FlatTextField` inside the tile, visible only when the path ends `.ccc` or `.cdl`
  (case-insensitive regex, `:930`; NOT for `.cc`), placeholder `"Correction ID (first)"` (`:2464`), committed on
  editing-finished (`:2466`) → `activeSceneLutCccId` (`:932`). No label, no tooltip, no validation.
- Shortcuts: **none** for any LUT control or for Export LUT [V] (no reference outside `ColorPanel.qml` and
  `src/color/`; the only key is the panel's own `Ctrl+3`, `Q/src/ui/qml/Main.qml:729`).

**Picker** — one shared `FileDialog` `lutPicker` (`:86-104`), title `"Choose LUT"`, open-file mode. Filters for the scene
slot: `"LUT / CDL files (*.cube *.3dl *.csp *.cc *.ccc *.cdl)"`, `"All files (*)"`; for the display slot: `"LUT files
(*.cube *.3dl *.csp)"`, `"All files (*)"`.

**Display LUT tile** — same component in the View group (`:1066-1076`), icon `monitor`, no Correction ID, no pin (shared
by everything on screen). Listed because it shares the component and the audit should rule on it too (§5 R12).

**Export LUT button** — bottom row, far right, after the HDR-mode combo (`:1345-1360`). [V]
- `FlatButton`, raised, text `"Export LUT…"`, icon `export`.
- Enabled when OCIO's Input, Display and View are all non-empty (`:1352-1355`) — independent of On / Off.
- Tooltip: `"Bake the active OCIO chain to a 65³ .cube LUT"` when enabled, `"Configure input / display / view first"`
  when not (`:1356-1358`).
- Dialog (`:114-131`): save-file, title `"Export LUT…"`, default suffix `cube`, filter `"3D LUT (*.cube)"`. No
  suggested file name, no start folder [V].
- Result: toast `"LUT exported: %1"` (file base name, kind 0) or `"Export LUT failed — %1"` (kind 2) + a console
  warning (`:122-129`).

### 1.2 Accepted files [V for the gate, I for what OCIO parses]

- Gate = **extension + file exists**, nothing else (`Q/src/color/ocio_config_manager.cpp:628-642`): scene slot accepts
  `cube`, `3dl`, `csp` (`isSupportedLutExtension`, `:77-83`) and `cc`, `ccc`, `cdl` (`isCdlExtension`, `:88-94`); display
  slot accepts the first three only (`:671-686`). A rejected path leaves the previous value and writes a `qWarning`;
  the user sees nothing. The picker's "All files" filter lets any file through to this silent rejection.
- The file is not parsed at pick time. It is loaded by OCIO's `FileTransform` when the chain is built
  (`Q/src/color/ocio_chain_builder.cpp:165-172`), `INTERP_BEST`, forward direction.
- [I] So sizes and 1D / 3D are whatever OCIO 2.5's readers take: `.cube` = Iridas / Resolve (1D, 3D, or a 1D shaper +
  3D, any size); `.3dl`; `.csp` (prelut + 1D or 3D). QCView adds no size limit of its own.
- **Correction ID** → `FileTransform::setCCCId` (`ocio_chain_builder.cpp:168-169`): the header says "its id, or an
  index; empty = the first" (`Q/src/color/ocio_config_manager.h:62-65`); the value is trimmed (`.cpp:658`). Picking a new
  file clears it (`.cpp:643-651`, "A new file starts at its first correction").
- Design reason (commit `24fbbab1`, 2026-09-28): ".cc, .ccc and .cdl load through the existing OCIO FileTransform — the
  grade a dailies colourist sends, applied rather than dialled in."

### 1.3 Where each stage sits in the chain [V]

`Q/src/color/ocio_chain_builder.cpp:15-25` and `buildGroupTransform` `:270-291`:

```
Input colourspace values
  → [Look]            LookTransform(input → the look's result space)
  → [Scene LUT / CDL] FileTransform, applied to the values as they are — NO conversion into or out of any space
  → DisplayViewTransform(src = look result space, else the Input)
  → [host-display emulation + clamp]   (a QCBridge Blender stream's clip only)
  → [Display LUT]     FileTransform on the display-encoded output
```

- With the Highlight Knee on, the chain is split: pre = Look → Scene LUT → interchange role; the knee in linear light;
  post = view → emulation → Display LUT (`:313-361`). The Scene LUT is always before the knee.
- Viewer aids (Exposure, Gamma, channel view) are outside this chain; the scopes read Input → interchange only
  (`buildScopeTransform`, `:364-387`) — a Scene LUT never moves the waveform.
- Scope of each: Scene LUT (+ Correction ID) = **per clip** — a default plus per-clip pins; every edit while a clip is
  loaded pins that clip, with no clip loaded it sets the default (`ocio_config_manager.cpp:643-651`, `editPin`
  `:984-991`). Display LUT = **view-wide**, one value (`m_activeDisplayLutPath`), cleared on a config switch
  (`:1344`, `:1402`). A pinned LUT survives a config switch (the pin is not validated against the config,
  `resolveScene` `:1217-1220`).
- A change of LUT path or Correction ID is a new shader (`sameShader`, `Q/src/color/ocio_chain_spec.h:51-56`); every
  clip's distinct chain is pre-built, capped at 8 (`ocio_config_manager.cpp:1304-1317`).

### 1.4 Storage [V]

| Where | Shape | Cite |
|---|---|---|
| `.qcvproj`, per media item | `"ocioClip": { "sceneLut": { "path": "<absolute path>", "cccId": "<id or empty>" }, … }` — only set slots are written; an explicit empty path = "this clip has no LUT although the default does" | `ocio_chain_spec.h:85-94`, `:116-120`; `Q/src/project/project_io.cpp:453-454`, `:506`; `Q/src/project/media_item.h:453-459` |
| User presets, `color-presets.json` in Qt's AppDataLocation, `schema: 1`, `user_presets[]` | `slots.scene_lut` (path, always written), `slots.scene_lut_cccid` (only when non-empty), `slots.display_lut` | `Q/src/color/preset_manager.cpp:805-807`, `:853-857`, `:751-756` |
| The default chain's Scene LUT (set with no clip loaded) and the Display LUT | **session only** — no QSettings write exists for OCIO's slots (only `minColor/engine`, `ocio_config_manager.cpp:703`, `:726`) | grep of `Q/src` |
| Note sidecars | the note's `"ocio"` string: `OCIO: <input> + <look> + <LUT file name> → <display> / <view> + <display LUT file name>` (file names only) | `Q/src/window/window_manager.cpp:7819-7834`; `Q/src/annotations/annotation_note.cpp:21`, `:68` |
| Panel state | `color/sceneLutExpanded`, `color/displayLutExpanded` | `ColorPanel.qml:73-78` |

The path is stored verbatim: no `${PROJECT_DIR}`, no relative form [V]. Project load restores pins without the
exists / extension check the setter applies (`replaceAllPins`, `ocio_config_manager.cpp:1188-1198`).

Presets: applying one sets Scene LUT + Correction ID on the selected clip and Display LUT on the View
(`preset_manager.cpp:395-407`); capture and "modified" compare all three (`:454-456`, `:601-603`). Bulk: "Use Clip Chain
of" copies the LUT + id to other clips (`copyClipChain`, `ocio_config_manager.cpp:1142-1146`).

Badges: a clip with a pinned LUT shows `LUT` or `CDL` (by extension) in its badge (`:1016-1022`) and
`"Scene LUT: %1"` (file name, or `"(none)"`) in the badge tooltip (`:1042-1047`).

### 1.5 Per engine, dual view, outputs

- **OCIO**: everything above.
- **minColor**: no LUT of any kind. Both tiles are hidden (`ColorPanel.qml:919`, `:1067`); the chain is Input → Knee →
  AgX → Rendering → Display (`Q/docs/installation.md:143`). OCIO pins (incl. the LUT) are kept but unused while the
  minColor engine is selected; the engine choice is one global setting, not per project (`minColor/engine`).
- **Dual view**: A and B each render through their own clip's chain, so each side has its own Scene LUT; the A / B tabs
  pick which clip the tile edits (`focusClipId`, `ocio_config_manager.cpp:839-843`); the Display LUT applies to both
  (`Q/docs/dualview.md:35`).
- **Screenshots, note thumbnails, modal backdrop**: the SDR-capture twin keeps the Scene LUT and **drops the Display
  LUT** ("calibrated for the live display", `Q/src/color/ocio_chain_builder.h:42-44`; `resolveParts`
  `ocio_chain_builder.cpp:107-109`).
- **Hover thumbnails**: "render through the same OCIO pipeline as the main viewport" (`Q/docs/thumbnails.md:11`) [I —
  doc, not traced].
- **NDI / AJA / stream outputs**: "OCIO or minColor transforms … are applied as on screen" (`Q/docs/output.md:19`) [I —
  doc, not traced]; so both LUTs ride the outputs.
- **A missing or malformed LUT**: the chain build fails with OCIO's message; the renderers log it
  (`Q/src/render/metal/metal_ocio_renderer.mm:187-190`, `d3d11_ocio_renderer.cpp:849-856`, `player_rhi_item.cpp:797-804`).
  No string for it exists in any catalog, so the user is told nothing [V]; what stays on screen was not traced [I].

### 1.6 Export LUT [V]

`OCIOConfigManager::exportLut(outPath, cubeSize = 65)`, `Q/src/color/ocio_config_manager.cpp:483-554`; writer
`Q/src/color/ocio_lut_baker.cpp:21-84`.

- **Which chain**: the focused clip's (`focusedSpec()`, `:507`) — single view = the clip on screen, dual = the A / B
  tab's clip, no clip = the default. Works with colour Off.
- **OCIO, baked stages**: Look → Scene LUT (+ Correction ID) → Display/View → host-display emulation → **Display LUT**
  — the live Display / View, not the SDR capture twin. With the Highlight Knee on, pre → knee → post is sampled
  (`:508-530`); if the chain cannot split, it exports **without** the knee and only a `qInfo` says so (`:531-533`).
  Never baked: Exposure (resolved at gain 1.0, `:504-508`), Gamma, channel view.
- **minColor, baked stages**: Input → knee → AgX → rendering → display encoding through the CPU twin (`:491-499`),
  resolved for the live display but not as an EDR hand-off (`sdrCapture=false, edrLinear=false`).
- **Format**: Iridas `.cube`, 3D only, `LUT_3D_SIZE 65` (the QML passes 65; not user-selectable, `ColorPanel.qml:122`),
  R fastest, 6 decimals fixed, header lines exactly:
  `# Generated by QCView Player` / `TITLE "OCIO Transform LUT"` / `LUT_3D_SIZE 65` / `DOMAIN_MIN 0.0 0.0 0.0` /
  `DOMAIN_MAX 1.0 1.0 1.0` (`ocio_lut_baker.cpp:46-52`).
- **Input domain / shaper**: a plain 0..1 lattice in the **Input colourspace's own code values**; no shaper, no 1D
  prelut, no extended domain. Output values are not clamped (negatives and > 1 are written, `:66-69`).
- **Naming**: none suggested; the user types it; `.cube` appended by the dialog's default suffix.
- **Atomic**: `QSaveFile`; a failed bake leaves an existing file untouched (`:33-39`, `:73-82`). Synchronous on the GUI
  thread ("65³ takes <100 ms", `Q/src/color/ocio_lut_baker.h:9-11`).
- **Errors** (the `%1` of `"Export LUT failed — %1"`): translated — `"Output path is empty"`, `"Cube size must be ≥ 2
  (got %1)"`, `"No OCIO config loaded"`, `"Chain build failed"` (context `qcv::OCIOConfigManager`); untranslated English —
  the chain builder's message (e.g. "OcioChainBuilder: active chain incomplete"), `"OCIO error: <what>"`,
  `"OcioLutBaker: cannot open '<path>' for writing: <reason>"`, `"OcioLutBaker: OCIO sampling failed: <what>"`,
  `"OcioLutBaker: failed to commit '<path>': <reason>"`.
- User doc: "The **Export LUT** action serializes the current chain to a `.cube` file for use in other software"
  (`Q/docs/color.md:103-105`); minColor: "**Export LUT…** bakes the active minColor chain to a `.cube`"
  (`Q/docs/mincolor.md:84`).

## 2. Design reasons found outside the code

Searched: `~/.claude/projects/…-QCView-Player/memory/`,
`…-QCView-Player-QT/memory/`, the QCBridge / BlenderAddon project memories, `~/.claude/plans/` (three plans — none
mentions LUTs or CDL), `Q/docs/color.md`, `Q/docs/mincolor.md`, and the two commits that introduced the features.

- **Why CDL lives in the Scene LUT slot** — memory `project_softknee_scopes_study.md:30` (the 2.5.0 colour study):
  "NO grading controls (offset/LGG/contrast/sat); supplied grades via CDL (.cc/.cdl/.ccc through existing Scene LUT
  FileTransform — picker filter at ColorPanel.qml:71)." QCView is not a grading tool; a grade arrives as a file and is
  applied. Same reasoning in commit `24fbbab1`: "the grade a dailies colourist sends, applied rather than dialled in."
- **Why the LUT is per clip** — memory `project_per_side_ocio.md:35`: "the line falls at the display transform: SCENE
  side (Input, Look, Scene LUT/CDL, knee) belongs to the CLIP (pin, else default); DISPLAY side (View, Display LUT) +
  config + display + viewer aids shared."
- **Why the knee is in the export and the aids are not** — same study, `:28`: "Chain step in Color panel between Scene
  LUT and Output; saved in presets; baked into LUT export (needs C++ knee in baker)"; `:30`: viewer aids are "Not in
  presets/exports".
- **Why captures drop the Display LUT** — memory `project_sdr_capture_chain.md:24`: "The override also drops the display
  LUT"; code comment: it "is calibrated for the live display" (`ocio_chain_builder.h:42-44`).
- **Why minColor has Export LUT but no LUT slot** — commit `3ec803f5`: "Everything that hangs off the OCIO chain now
  works against the engine tag … LUT export: exportLut bakes the focused chain through the CPU twin under minColor, no
  config needed." No note anywhere proposes a LUT slot for minColor.
- **Display LUT's origin** — memory `project_metal_playback_fixes.md:60-68` (old app): an open question, "Should we
  generate a 3D LUT from the OCIO transform and apply it as a final display LUT?" — i.e. a display-calibration /
  EDR-matching slot, not a creative one. No later note records anyone using it.
- Docs wording: `Q/docs/color.md:45` — "optional LUT applied in the scene-linear stage" (see §4, bug B1: the code does
  not convert to scene-linear).

## 3. What 3.0 already has

### 3.1 The rulings that place this

- **Row 205 (RULED 2026-11-08)** — the Setup / Clip / View structure is replaced: "a clip's chain is the whole thing —
  Input, Look / Scene LUT, Knee, Rendering … Clips that never set one follow the project's default chain … the ↺ markers
  and the invisible default go." **The Scene LUT is named as part of the chain in the ruling itself.**
- **Row 208 (RULED 2026-11-08)** — presets in the user's library, projects carry chains; "Caveat: a Scene LUT is stored
  by path (min-paths maps drives / mounts, not a missing file)." — again assumes the LUT exists.
- **Row 200 (RULED 2026-11-08)** — native shaders, no baked LUTs for display; "Row 92's baker survives for LUT EXPORT
  only." — Export LUT assumed.
- **Row 204** — the built OCIO chain is "input → [look] → [scene LUT / CDL] → the display's view"; "OWED: OCIO's dynamic
  properties, display LUT (if kept)." — the Display LUT is the only one of the family with an open "if kept".
- **Rows 179 / 183** — one linear surface; every config gets one display built in code, "Linear Screen"
  (`M/crates/qcv-color/src/ocio.cpp:17-18`); the user picks config · input · view · look. No Output column, hence no
  natural home for a Display LUT and **no display encoding at the end of the chain** (matters for Export, §4 H1).
- **Row 199** — the drawer; **206 / 207** — the Custom row, one preset list for both engines; **198** — minColor is the
  default engine, OCIO the option; **203** — the waveform reads the source through the Input only.
- **No row drops Scene LUT, CDL, Correction ID or Export LUT** [V — searched the canon for LUT / CDL / cccId /
  Correction]. The audit's "no ruling dropping them" stands; rows 200 / 205 / 208 in fact presuppose them.

### 3.2 Where the stages already are

| Piece | State | Cite |
|---|---|---|
| Chain stage for the Scene LUT | built, both shapes; untested (no test passes a LUT) | `ocio.cpp:130-134`, `:165`; `M/crates/qcv-color/src/ocio_tests.rs` (no `scene_lut` use) |
| 3D / 1D LUT textures for the native pass | built (OCIO's own LUTs already use it) | `M/crates/qcv-color/src/ocio.rs:132-138` (`lut_texture`), `:183-186` |
| Correction ID | missing end to end: no field in `Spec` / `OcioChain`, no FFI parameter, no `setCCCId` | `ocio.rs:71`; `color_model.rs:10`; `ocio.cpp:116`, `:155` |
| Project model | `ocio_clip` / `min_color_clip` are opaque JSON maps, round-tripped byte for byte; `colorDefault` likewise | `M/crates/qcv-project/src/model.rs:50`; `io.rs:155-164`, `:178` |
| Presets | whole-chain maps in 3.0's own `color-presets.json` (`presets[]`); the LUT path rides in `chain.sceneLut.path` | `color_model.rs:121-136` |
| Dual view | each side binds its own clip's chain → per-side LUT comes free | `color.rs:213-246` |
| Thumbnails | key already includes `scene_lut` | `M/apps/qcv/src/thumb_color.rs:56`, `:84` |
| Screenshots / notes | the SDR twin passes `scene_lut`; the note tag already appends the LUT's file name, as 2.x | `M/apps/qcv/src/capture.rs:244`, `:215` |
| NDI / AJA outputs | take the engine's picture [I — not traced for this spec] | — |
| CPU twins for a bake | all three exist: OCIO `Chain::apply_cpu`, minColor `apply_cpu`, `knee_cpu` | `ocio.rs:126`; `M/crates/qcv-color/src/lib.rs:83`, `:215` |
| Strings | every 2.x string of §6 is already in the 11 catalogs (carried by `ts2po.py`, row 195) | `M/apps/qcv/i18n/qcview_*.po` |
| Clip badges (`LUT` / `CDL`) | no colour badges in 3.0 yet (the project panel's clip-chain entries are a recorded deviation) | `M/apps/qcv/src/project_panel.rs:12-13` |

### 3.3 Where it goes in 3.0's drawer (placement, not 2.x's layout)

OCIO columns today: Presets · Config · Input · Look · View · Knee (`color_drawer.rs:272-273`). Chain order puts the Scene
LUT between Look and View; `color_columns.rs` already has the pattern for an optional column that collapses to a 32-px
strip with an amber dot (the Knee and AgX cards, `M/apps/qcv/src/color_columns.rs:1-5`), and "the optional columns open
for a chain that uses them, once per target" (`color_drawer.rs:191-192`, `:204`). Under row 205 there is no ↺ and no
per-slot pin: the LUT is one field of the clip's whole chain, so Reset to default / Apply to all clips / Make project
default / Save as preset… carry it with no extra work.

## 4. Gaps and risks

### Cheap
- C1. `ccc_id` through `OcioChain`, `Spec`, the two FFI builders (`setCCCId`), the map read / write. ~30 lines.
- C2. The rebuild key must include the LUT path and the id (`color.rs:232`; add the id to `thumb_color.rs:56`).
- C3. The Scene LUT column: a third card on the Knee / AgX pattern + an `rfd` open dialog. The largest UI piece, but all
  parts exist (cards, text field, tooltips, strip).
- C4. The `.cube` writer and the bake loop over the existing CPU twins; the button, an `rfd` save dialog, two toasts.
- C5. Tests: GPU vs OCIO's CPU processor with a generated cube and a generated `.ccc` (two ids); export → re-import as a
  Scene LUT → compare.

### Hard / needs a ruling before code
- **H1. Export has no display encoding to bake.** 2.x bakes Input codes → the chosen Display's codes. 3.0's chain ends
  in display-LINEAR Rec.709, 1.0 = 100 nits (rows 179 / 183); a `.cube` of that is useless in the "other software" the
  2.x doc promises. Export must add an encode that the screen path does not have. Captures already do exactly this:
  SDR twin of the chain, then sRGB (`capture.rs:226-250`, `min_present::capture::srgb8`). Which encodes to offer is R9.
- **H2. Adopting 2.x pins.** A 2.x `ocioClip` is half a chain by 3.0's model (no config, no view — 2.x never stored
  them in the project), and 2.x's engine choice is not in the project either. Auto-adoption would have to guess both,
  and would move clips off the minColor default (row 198). Not adopting = today's silent ignore-then-erase. R6.
- **H3. A chain that fails to build is invisible.** `OcioPass::new` failing only logs (`color.rs:239`), the previous
  pass stays on screen, and the dead channel is never retried while the key is unchanged (`:233`). Today that needs a
  broken config; with user LUT files (moved, renamed, on an unmounted volume, malformed, a wrong Correction ID) it
  becomes an everyday case — the reviewer would look at the previous clip's chain with no sign. R5.
- **H4. Paths.** Absolute, per machine. Row 208 already notes it. Keeping 2.x's verbatim form keeps the QCView round
  trip (row 141 / 205); anything smarter (`${PROJECT_DIR}`, copying the LUT beside the project) is a format change 2.x
  would not read.
- H5. [I] LUT sampling without `FLOAT32_FILTERABLE`: the LUT bindings fall back to non-filterable float
  (`ocio.rs:183-186`) while the sampler is linear (`:260`). OCIO's own LUTs already live with whatever that does; a
  small user cube (17³, 33³) would show it most. Worth one measurement on the Windows box before the walk.
- H6. `INTERP_BEST` on a user 3D LUT emits tetrahedral GPU code [I]; build time and the DX12 compile should be
  measured once with a 65³ cube.

### 2.x behaviour that looks like a bug — for the owner to rule on, nothing proposed silently
- **B1. "Scene-linear" is not what the code does.** The doc (`docs/color.md:45`) and the translator comment
  (`ColorPanel.qml:920`) say the LUT is applied in scene-linear; the code applies the file to the Input's own values
  (after the Look, in the look's result space) with no conversion either side (`ocio_chain_builder.cpp:270-287`). For a
  log clip that is the usual convention (a log-to-log LUT, a CDL on log) and probably what users rely on; the wording is
  what is wrong. 3.0's `ocio.cpp` already copies the code's behaviour.
- **B2. Silent rejections.** A non-LUT file picked through "All files", or a preset whose LUT file is gone, is refused
  with only a log line; the tile keeps the previous LUT (`ocio_config_manager.cpp:630-641`). After such a preset the
  clip shows the OLD LUT under the new preset's name, flagged merely "modified".
- **B3. Silent chain failure** on a missing / malformed LUT or an unknown Correction ID (§1.5) — no message.
- **B4. Correction ID has no feedback**: free text, no list of the ids in the file, shown for `.ccc` / `.cdl` only.
- **B5. Export under minColor is gated and described by OCIO's state**: enabled only when OCIO's Input / Display / View
  are set, tooltip says "OCIO chain", and the file's `TITLE` is "OCIO Transform LUT" for a minColor bake
  (`ColorPanel.qml:1352-1358`; `ocio_lut_baker.cpp:47`).
- **B6. Export silently omits the knee** when the chain cannot split (`ocio_config_manager.cpp:531-533`).
- **B7. Export has no shaper and a 0..1 domain**: for a scene-linear Input the cube covers 0..1 linear only (everything
  above diffuse white is outside the LUT, shadows get almost no lattice points). Fine for log / display-referred Inputs.
- **B8. Export bakes the Display LUT and the host-display emulation** of the focused clip — a monitor-calibration LUT
  ends up inside a file meant for other software, while captures deliberately drop it.
- **B9. Export error text is partly untranslated and carries internal prefixes** ("OcioLutBaker: …").
- **B10. The default chain's Scene LUT and the Display LUT are forgotten at quit** (no settings write), while minColor's
  whole chain is remembered; a comment claims OCIO's slots are persisted "like" minColor's
  (`Q/src/color/mincolor_engine.h:6-7`) — they are not.
- B11. A LUT pin is restored from a project without the exists / extension check the picker path applies (§1.4).

### Adjacent gap found on the way (not LUT-specific)
- 2.x **user presets** (`user_presets[]`, which may hold `scene_lut` / `display_lut`) are not read by 3.0: it has its own
  file and shape (`color_model.rs:126-131`). No collision — different folders [I: Qt's AppDataLocation nests
  organisation/application; row 253 says the same] — and no import. No ruling found either way.

## 5. Rulings needed

| # | Question | [proposed] default | Reason |
|---|---|---|---|
| R1 | Scope of the Scene LUT | **OCIO chains only**, as 2.x (no LUT slot under minColor) | QCView wins ties; no note ever asked for a minColor LUT |
| R2 | Where in the drawer | A **"Scene LUT" card between Look and View**, collapsed to the 32-px strip by default, opened once per target when the chain has a LUT (Knee / AgX pattern); 2.x's tile contents inside (+ Add LUT / Replace LUT, file name, ✕ Clear LUT, Correction ID) | Chain order reads left to right; reuses the card the drawer already has |
| R3 | Per clip or shared | **Part of the clip's whole chain** (row 205): no ↺, no pin dot; Reset / Apply to all / Make project default / presets carry it | Already ruled; restated so the 2.x pin UI is knowingly not ported |
| R4 | Formats | **The same six**: `.cube .3dl .csp .cc .ccc .cdl`, same two filter lines | 2.x's strings and behaviour reused as is |
| R5 | A LUT that cannot load (missing, malformed, unknown Correction ID) | **Say so**: a negative toast naming the file + the card in an error state; the picture drops to **Color bypass for that clip**, not to the previous chain and not to the chain minus the LUT | Divergence from 2.x (B2 / B3), reason: a QC viewer must not show a plausible wrong picture; needs one new string |
| R6 | 2.x projects with a Scene LUT pin (the "silently ignored" case) | (a) **Never erase**: when an OCIO chain is first written to a clip holding an unmarked 2.x pin, seed Input / Look / Scene LUT / Correction ID / Knee from the pin where the config has the names; (b) on open, **one toast** when any clip carries an unapplied 2.x Scene LUT; (c) **no automatic switch** of the clip to OCIO | A 2.x pin has no config or view to build from, and row 198 keeps minColor the default; telling + preserving is the least presumptuous |
| R7 | Stored path form | **Verbatim absolute path in `sceneLut.path`**, as 2.x; min-paths mapping applied when the file is opened, never written back | Keeps the frozen project contract and the 2.x round trip (rows 141 / 205 / 208) |
| R8 | Correction ID | **As 2.x**: a text field for `.ccc` / `.cdl` only, placeholder "Correction ID (first)", cleared when the file changes, stored in `sceneLut.cccId` | Carries 2.x; a picker of the file's ids (B4) would be a new feature |
| R9 | Export LUT: what the cube's OUTPUT is, now that the chain ends in linear light | **The chain as a screenshot sees it: its SDR twin, sRGB-encoded.** One encode, no dialog options in the first cut | H1; matches what captures already do and what 2.x gave on its default sRGB display. Rec.1886 / PQ outputs can be a later ruling |
| R10 | Export LUT: which stages | **Input → Look → Scene LUT (+ id) → Knee → view / Rendering**, both engines; never the viewer aids, never Video gamma (row 246) | 2.x's rule ("a chain step is baked, a viewing aid is not") |
| R11 | Export LUT: format | **As 2.x**: `.cube`, 65³, domain 0..1 in the Input's code values, no shaper, values unclamped, atomic write, same header — and B7 stays as a known limit | Byte-compatible with what users already export; a shaper is a separate decision |
| R12 | Display LUT | **Not ported, ruled out in writing**: 3.0 has no Output / display column (rows 179 / 183) and a display-calibration `.cube` expects display-encoded codes the linear surface never has; a stored `display_lut` exists only in 2.x presets | Row 204 leaves it "if kept"; it needs an explicit row either way |
| R13 | Export LUT: where the button goes and when it is enabled | **The drawer's bottom row, right end**, enabled whenever the edited target has a complete chain (either engine), Color on or off | 2.x's position and "independent of On / Off" rule; fixes B5's OCIO-only gate (a stated divergence) |
| R14 | Export LUT: suggested file name | **None, as 2.x** (the dialog opens empty with the `.cube` filter) | A suggested name built from a clip or preset name could carry client names into a file the user sends out |
| R15 | Which 2.x bugs of §4 to fix in the port | **Fix B2 / B3 (via R5), B5, B6 (toast when the knee is left out), B9; keep B1's behaviour and correct the wording; leave B4, B7, B8 (moot without a Display LUT), B10 as they are** | Each fix is a divergence and needs the owner's yes; none is applied without it |
| R16 | 2.x user presets (the adjacent gap) | **Out of this port; its own row** | Separate question, larger than LUTs |

## 6. Strings (verbatim, with 2.x context) — all already in 3.0's 11 catalogs

Context **`ColorPanel`** (`Q/src/ui/qml/ColorPanel.qml`):

| String | Line | Translator comment |
|---|---|---|
| `Scene LUT` | 921 | A LUT applied in scene-linear space, before the view |
| `Display LUT` | 1068 | — |
| `Choose LUT` | 88 | — |
| `LUT / CDL files (*.cube *.3dl *.csp *.cc *.ccc *.cdl)` | 91 | — |
| `LUT files (*.cube *.3dl *.csp)` | 93 | — |
| `All files (*)` | 92, 94 | — (also exists under context `MediaFilters`) |
| `+ Add LUT` | 2435 | Button on an empty LUT slot |
| `Replace LUT` | 2433 | Button on a filled LUT slot |
| `click to choose` | 2473 | Tooltip on an empty LUT slot |
| `Clear LUT` | 2514 | Button |
| `Correction ID (first)` | 2464 | Field: the id of the first correction inside a CDL file |
| `%1 — %2` | 2542 | — (title — file name) |
| `%1 — click to expand` | 2544 | — |
| `Collapse` | 2377 | Button: fold this section |
| `Set on this clip — click to go back to the default` | 1397 | — (the ↺; not needed under row 205) |
| `Export LUT…` | 116, 1347 | — (dialog title and button) |
| `3D LUT (*.cube)` | 119 | — |
| `LUT exported: %1` | 128 | — |
| `Export LUT failed — %1` | 125 | — |
| `Bake the active OCIO chain to a 65³ .cube LUT` | 1357 | — |
| `Configure input / display / view first` | 1358 | — |

Context **`qcv::OCIOConfigManager`** (`Q/src/color/ocio_config_manager.cpp`):

| String | Line | Use |
|---|---|---|
| `Output path is empty` | 486 | export error |
| `Cube size must be ≥ 2 (got %1)` | 489 | export error |
| `No OCIO config loaded` | 500 | export error |
| `Chain build failed` | 539 | export error |
| `LUT` | 1021 | clip badge |
| `CDL` | 1021 | clip badge ("Badge name in the viewport's chain summary") |
| `Scene LUT: %1` | 1043 | badge tooltip line |
| `(none)` | 1045 | badge tooltip, "No input set" |
| `Set on this clip:` | 1036 | badge tooltip heading |

Untranslated in 2.x (plain `QStringLiteral`, shown inside `Export LUT failed — %1`): `OCIO error: %1`
(`ocio_config_manager.cpp:528`, `:552`); `OcioLutBaker: null CPU processor`, `OcioLutBaker: cubeSize must be ≥ 2`,
`OcioLutBaker: empty output path`, `OcioLutBaker: cannot open '%1' for writing: %2`, `OcioLutBaker: OCIO sampling
failed: %1`, `OcioLutBaker: failed to commit '%1': %2` (`ocio_lut_baker.cpp:16-81`); the chain builder's
`OcioChainBuilder: …` messages.

New strings the proposed defaults would need (each via `tools/i18n/add_drafts.py`, 11 languages, drafted / unreviewed),
wording for the owner:
- R5: a "could not load the Scene LUT" toast with the file name and OCIO's reason (pattern of the existing
  `Couldn't load the OCIO config: %1`, `color.rs:262`).
- R6(b): a toast for a project whose clips carry an unapplied QCView 2 Scene LUT.
- R13 / B5: an engine-neutral tooltip for Export LUT (2.x's says "OCIO chain"), and its disabled reason.
- R15 / B6: "exported without the Highlight Knee" when the chain cannot split.

## 7. Size of the port (estimate)

Small-to-medium: the engine stage, the textures, the storage key, the capture and thumbnail paths and every string
already exist. Roughly: Correction ID plumbing + rebuild key (½ day); the Scene LUT card + picker + error state
(1 day); Export LUT bake, writer, button, dialog, toasts (1 day incl. the SDR-twin encode); the 2.x-pin handling of R6
(½ day); tests + the two-OS walk (1 day). About 500–700 lines with tests, two to three sessions — provided R5, R6 and
R9 are ruled first, since those three decide behaviour rather than layout.

## 8. Progress (the build, from 2026-11-10)

| Stage | What | State |
|---|---|---|
| 1 | The Correction ID end to end (`Spec.ccc_id`, `OcioChain.ccc_id`, both C++ builders → `FileTransform::setCCCId`, QCView's `sceneLut.cccId`); the pass and thumbnail keys carry the LUT path and the id (C1, C2) | Built. Tests (`qcv-color` `ocio_tests`): a `.cube` and a two-correction `.ccc` act on the Input's values — id, index, the first by default; an unknown id and a missing file fail to build; the Knee split's pre half carries it; the GPU matches OCIO's CPU processor |
| 2 | The Scene LUT card (R2–R4, R8): `color_columns.rs` `LutPanel`, between Look and View, collapsed by default, opened once per target for a chain with a LUT; the picker (`Cmd::PickLut`, QCView's title and two filters; a new file clears the id); ✕ Clear; the Correction ID field for `.ccc` / `.cdl`. R5: a file that cannot load → a negative toast naming it with OCIO's reason, the card in an error state (opened), the clip in Color BYPASS, no rebuild until the chain changes. R7: the path is stored verbatim; only the separators are turned when the file is opened (the app has no drive / mount mapping for any path yet — R7's "min-paths mapping" is not there to apply) | Built; both card states checked in headless snapshots (`QCV_UI_SNAP_COLOR=ocio QCV_UI_SNAP_LUT=<path> [QCV_UI_SNAP_LUT_ERR=<text>]`). **Walk owed** with real files: a `.cube`, a `.ccc` with ids, a missing file, dual view (a LUT per side), screenshots / thumbnails / an output carrying it |
| 3 | Export LUT (R9–R14, B5 / B6 / B9): `qcv_color::bake` — the chain's SDR twin (the SDR view / the 100-nit minColor chain) on a 65³ lattice of the Input's code values, linear light → sRGB, unclamped, QCView's five header lines (`TITLE "OCIO Transform LUT"`, or `"minColor Transform LUT"` for a minColor bake — B5), a temporary file renamed into place. The drawer's bottom row ends in **Export LUT…** (either engine, Color on or off; an OCIO chain needs its config, an input and a view); the save dialog suggests no name; toasts: QCView's two, plus "exported without the Highlight Knee" when the chain cannot split (B6) | Built. Tests: the file's shape, the atomic write, a minColor bake, and — through OCIO — an exported cube read back as a Scene LUT equals the chain (plain, AgX, with the Knee); 65³ bakes in 3–26 ms. The button checked in a headless snapshot. **Walk owed**: an export from each engine opened in another application |
| 4 | 2.x projects' Scene LUT pins (R6). (a) NEVER ERASED: the first OCIO chain written to a clip holding a 2.x pin (`ocioClip` without `engine`) takes the pin's Input, Look, Scene LUT + Correction ID and Knee (`OcioChain::seeded_from_pin`; names then checked against the config; a slot the user has just changed keeps their value); QCView's `hostDisplay` rides through every write; Reset to default leaves an unused pin in the file. (b) One notice toast on open when any clip carries a 2.x Scene LUT that is not applied. (c) No clip is switched to OCIO for it | Built; the model's behaviour is unit-tested. **Walk owed** with a real 2.x project that pins a Scene LUT |

Strings beyond QCView's (ColorPanel; each drafted in 11 languages): `Couldn't load the Scene LUT %1: %2`; `Bake this chain to a 65³ .cube LUT: its SDR picture, sRGB-encoded`; `Choose an input and a view first`; `LUT exported without the Highlight Knee: %1`; `Some clips carry a Scene LUT from QCView 2 that is not applied. Give a clip an OCIO chain to use its LUT.`

R10 amended by the user 2026-11-10 ("Can we keep the video gamma if toggled?"): the Video emulation IS baked into an export while it is lit and acting on the screen (Color on, an SDR picture) — the SDR range as the 1/2.4 power — and the toast reads `LUT exported with Video gamma: %1` (a sixth string, 11 drafts). The viewer aids stay out.

Not done, knowingly: B4 / B7 / B10 as ruled; the clip badges (`LUT` / `CDL`) — 3.0 has no colour badges yet. Noticed: OCIO caches a LUT file by its path, so a file changed on disk under the same name is not re-read until the app restarts (2.x the same [inferred]). Found on the way and fixed: the
Video gamma tooltip still said screenshots and outputs were unaffected after row 262 — reworded, 11 drafts.
