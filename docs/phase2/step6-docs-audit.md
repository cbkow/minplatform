# QCView docs audit: the 2.x site against the 3.0 code

Audited 2026-11-10. Read-only; nothing in either repository was changed.

- 2.x docs: `../QCView-Player/docs/*.md` on `main` (clean, at cb890143, tag v2.7.3 + 3 commits).
  There are **24** pages, not 22.
- 3.0 truth: `../minplatform`. Evidence paths below are relative to that repo.
  `canon N` = row N of the Decisions tables in `docs/rust-wgpu-port.md` (rows 160-252 are lines 2464-2550).
  `src/` = `apps/qcv/src/`.
- Images were not opened. "Stale" judgements come from the alt text and from what the code says the UI now is.

Evidence labels:
- **[code]** read in the 3.0 source at the cited line.
- **[canon]** a dated ruling says so; the code was not separately read for that point.
- **[absent]** a search of `apps/qcv/src` found no such string or handler. Absence is evidence, weaker than a cited line.
- **UNVERIFIED** not checked, or checked without a conclusion.

---

## 1. The changes that touch many pages

These explain most of the per-page findings. Each is stated once here.

| # | Change in 3.0 | Evidence |
|---|---|---|
| G1 | The whole interface is redrawn (new design system, rails as rounded tiles, three bottom bands: status strip, transport, scrubber). Every QCView screenshot is stale. | canon 160-169, 174 |
| G2 | View menu is: Compact Mode, Always on Top, Fullscreen, Default Size & Position, Left Rail, Right Rail, Color Panel, Notes. Gone: Minimal Mode (Ctrl+0), Show All Panels (Ctrl+9), Status Bar (Ctrl+5), Zoom In/Out Timeline items, View > Timeline. No Ctrl+R "Default View". | [code] `src/menu.rs:63-73`; key handler `src/main.rs:2325-2375` has no 0 / 5 / 9 / R; canon 245, 219 |
| G3 | Color and Notes are bottom **drawers** above the transport, one open at a time, toggled by Ctrl+3 / Ctrl+4, the transport buttons, or a caret under the picture. | [code] `src/color_drawer.rs:1-9`; canon 199, 215, 219 |
| G4 | minColor is the default engine. One whole chain per clip, plus a project default. The Setup / Clip / View groups, the OCIO \| minColor segment, the shared View half, the reset arrows and the A / B tabs are gone. | canon 198, 205, 207, 213; [code] `src/color_drawer.rs` strings ("Own chain", "Following the project default", "Apply to all clips", "Make project default", "New OCIO chain…") |
| G5 | No display-mode list. The surface is chosen automatically (macOS: one EDR float surface; Windows: 10-bit sRGB on an SDR monitor, scRGB float on an HDR monitor). No "HDR10 PQ" or "EDR - Linear P3" modes to pick. | canon 179, 220, 221; [code] `crates/min-present/src/presenter.rs:93-98` |
| G6 | Waveform only, inside the Color drawer. The vectorscope is dropped. Scale chips are SDR / HDR. | canon 203; [code] `src/color_drawer.rs:136` |
| G7 | No track edit mode. Dual and playlist clips are edited directly on thin clip bars under the scrubber. The overview zoom bar is retired. | canon 166, 167, 212; [code] `src/shortcuts.rs:7-8`, `crates/min-ui/src/scrubber.rs:7` |
| G8 | The Stream output card (SRT / RTMP) does not exist. | canon 244 (given) |
| G9 | Dual-view controls (A / B chips, four mode buttons, seam / gain slider, Save) sit on the right of the status strip, not above the viewport. | canon 173; [code] `src/dual_controls.rs` |
| G10 | Bundled OCIO configs: ACES 2.0, ACES 1.3, Blender 5.2. Blender 5.1 is dropped. No built-in OCIO presets. | [code] `src/color.rs:74`; `apps/qcv/assets/OCIO/README.md`; canon 198 |
| G11 | Right rail holds Inspector + Keyboard Shortcuts and starts collapsed. Left rail order: Media, Safety Guides, Background, Output, Settings. | [code] `src/main.rs:1019-1027`, `1045-1046`; `src/settings.rs:139` |

---

## 2. Per page

### index.md (Home)
**Verdict: small edits** (about a third of the feature bullets).
Images: `qcv001.jpg`. Stale (G1).

| Statement | 3.0 | Evidence |
|---|---|---|
| "Vulkan/D3D11-video on Windows" | D3D12VA, with FFmpeg's Vulkan device for ProRes-class intra codecs; DX12 renderer | [code] `crates/min-ffmpeg/src/ffmpeg.rs:6,19,139`; `src/settings_panel.rs:61`; `presenter.rs:72` |
| "HDR10 (PQ / ST.2084) on Windows via DXGI + DirectComposition" | scRGB float surface on an HDR monitor, relative brightness; no HDR10 mode | G5; canon 221 |
| "Bundled OCIO configs: ACES 2.0, ACES 1.3, Blender 5.2, Blender 5.1" | No Blender 5.1 | G10 |
| "A second, OCIO-free colour engine, minColor: Input, Highlight Knee, AgX, Display" | minColor is the default; no Display stage | G4; canon 181, 198 |
| "ASC CDL and `.cube` LUTs" | No Scene LUT / Display LUT / CDL controls in the Color drawer | [absent] no such string in `src/`; `src/color_model.rs:53` keeps a stored `sceneLut` path and `src/color.rs:235` passes it to OCIO, so a 2.x project's LUT may still apply. Whether it does is UNVERIFIED |
| "GPU vectorscope and waveform" | Waveform only | G6 |
| "Screenshots and notes exports apply the OCIO transform" | The clip's chain (either engine), re-targeted to SDR | canon 216 |
| "or as an H.264 + AAC stream over SRT or RTMP to Louper ..." | Removed | G8 |
| "notes ... open in minNotes" | No "Open in minNotes" action | [absent] `src/notes_drawer.rs`; canon 233 lists it as owed |
| "independent A / B controls, a live feed on either side" | A live source cannot be a dual side | [code] `src/main.rs:379-391` ("anything else is not a side") |
| "TIFF, PNG, JPEG sequence playback" | Also DPX, TGA, BMP | [code] `crates/qcv-project/src/extensions.rs:11` |
| "Adobe project metadata import ... via ExifTool" | ExifTool retired; XMP is read through FFmpeg | canon 134 |
| "Custom project files (`.qcvproj`)" | True; 2.x projects open | canon 141, 248 |
| Links: Source Code, Acknowledgments, Third-Party Notices point at QCView-Player files | Source lives in minplatform; notices are a generated HTML shown by About > Licenses… | canon 248 (ruling 8), 252 |

### installation.md
**Verdict: small edits** to the top half; the Version History below is a log and stays.
Images: `download.png`, the Microsoft Store badge (remote). Neither is a UI screenshot.

| Statement | 3.0 | Evidence |
|---|---|---|
| "Vulkan-capable GPU with up-to-date drivers (FFmpeg software fallback when unavailable)" | DirectX 12 and a current driver. With no usable device the app shows a message and exits | [code] `src/fatal.rs:12-13`; canon 250 |
| "HDR-capable display + Windows HDR mode enabled for HDR10 output" | HDR monitor gets the scRGB surface automatically | G5 |
| "Windows 10 version 1809 or later (10.0.17763.0), x64" | Unchanged | [code] `packaging/windows/msix/AppxManifest.xml.in:24` |
| "macOS 13.0 (Ventura) or later", "Apple Silicon" | Unchanged | [code] `packaging/macos/make_plist.py:40` |
| "native Metal rendering" | Still Metal, through wgpu | [canon] |
| Updates paragraph (daily check, About > Check for Updates…, Settings > Updates) | Still true | [code] `src/sparkle.rs`, `src/menu.rs:78`, `src/settings_panel.rs:94-95` |
| "QCViewBridge page ... Adobe After Effects, Adobe Premiere Pro, and Blender add-ons" | Also NDI and AJA; the name is QCBridge | wording only |
| No upgrade note | Needs: 3.0 replaces 2.x in place; 2.x settings are not imported; 2.x projects open | canon 248 |
| No "What's new in 3.0" block | See section 5 | |

### overview.md (Technical Overview)
**Verdict: substantial rewrite.** Every section describes the Qt architecture.
Images: none.

| Statement | 3.0 | Evidence |
|---|---|---|
| "Qt 6.11 Quick / QML application" | Rust + wgpu; own UI layer (min-ui, vello, parley) | canon title, 160-165 |
| GPU row: "Direct3D 11 + DirectComposition (multi-visual for HDR)" | DX12 through wgpu, one surface | [code] `presenter.rs:68-72` |
| HW decode row: "Vulkan-video for ProRes, D3D11 for h.264/h.265" | Vulkan for intra codecs bridged to D3D12; D3D12VA for the rest | [code] `ffmpeg.rs:19,76,139` |
| Audio row: WASAPI / CoreAudio | Still true | [code] `crates/qcv-player/src/audio/device.rs:57-61` |
| "MMCSS 'Pro Audio' priority" | UNVERIFIED | |
| HDR output row: "HDR10 (PQ / ST.2084, BT.2020) via DXGI swapchain" | scRGB (extended linear sRGB) float | G5 |
| "Pipeline modes" table (Normal RGBA8 / High-Res / Ultra / HDR) | The working image is half float; the four modes are not a 3.0 concept | canon 202. Whether any per-source precision choice remains is UNVERIFIED |
| HDR, Windows: "own DirectComposition visual", "scRGB ... not recommended" | Reversed: scRGB is the only HDR path | canon 220, 221 |
| HDR, macOS: "ACES 2.0 and Blender 5.2 configs include Linear sRGB EDR and Linear P3 EDR display outputs" | One patched EDR display per config; the user never picks an output | canon 183, 200 |
| "you pick which file's channel goes to which output from the Audio routing controls" | Inspector has a Mix picker (Auto / 5.1 to 2 / 7-8) and meters | canon 177; [code] `src/inspector_panel.rs` ("Mix", "Meters"). A per-channel matrix UI: UNVERIFIED |
| "JPEG-2000 / HTJ2K (via OpenJPH)" sequences | Not in the image list | [code] `extensions.rs:11`. (2.x's own list did not have them either: `QCView-Player/src/project/media_extensions.h:62-63`) |
| "Bundled configs: ... Blender 5.1" | Dropped | G10 |

### basics.md (App Basics)
**Verdict: substantial rewrite.**
Images: `qcv002` to `qcv010`, `qcv044.png`. All stale (G1); `qcv004`/`005`/`006` show panels and toggles that moved (G3, G11); `qcv009` shows the old shortcuts card (now filtered, different groups).

| Statement | 3.0 | Evidence |
|---|---|---|
| File menu table | Items match. New: New Project has Ctrl+N; Connect to After Effects and Connect to Premiere Pro are two items; Connect to Blender is a checkable item; Windows has Exit (Ctrl+Q) | [code] `src/menu.rs:54-62`, `src/main.rs:2340` |
| "QCViewBridgeAE" | Product name is QCBridgeAE | wording |
| "onto the Project panel's media list ... outlines in blue" | Drop on the left rail adds to the project. The blue outline: UNVERIFIED (blue is rationed, canon 162) | [code] `src/main.rs:176` |
| `.lnk` shortcuts on Windows | Still true | [code] `src/os_actions.rs:34-50`, `src/main.rs:286`. The comment at `src/main.rs:277-278` saying it is "still owed" is out of date |
| "Left Rail (Project, Settings, Background, Safety)" | Media, Safety Guides, Background, Output, Settings | G11 |
| "Right Rail (Scopes, Inspector, Shortcuts)" | Inspector, Keyboard Shortcuts | G6, G11 |
| "Status Bar \| Ctrl + 5" | Removed; the strip is always shown | G2 |
| "The color and notes panels can be opened via buttons on the transport row" | True; also the caret under the picture | G3 |
| "Minimal Mode \| Ctrl + 0", "Show All Panels \| Ctrl + 9", "Default View \| Ctrl + R" | None exist | G2 |
| Compact Mode: "a 22 px strip" | 32 px band | canon 172 |
| Compact Mode: "Drag the timecode to move the window" | True, and dragging the picture also moves it | canon 251 |
| Fullscreen: "Viewport only, no UI" | The three bands appear while the pointer rests at the bottom edge | canon 171 |
| "Open the Background section ... (Tools menu > Background, or Ctrl + 1 then expand)" | True; also a transport button and the `B` key cycles | [code] `src/menu.rs:74`, `src/transport_bar.rs:79`, `src/shortcuts.rs:68` |
| Backgrounds "grey, black, light checkerboard, dark checkerboard" | Black, Dark Gray, Dark Check, Light Check | [code] `src/background_panel.rs` |
| "full keyboard shortcut reference ... bottom of the Right Rail (Ctrl + /)" | True; now has a filter field | [code] `src/shortcuts.rs:1-8` |
| "Settings panel opens in the Left Rail (Ctrl + , or Tools menu > Settings…)" | True | [code] `src/menu.rs:74`, `src/main.rs:2355` |
| Link `/basics/#layout-presets` used elsewhere | The page's permalink is `/app-basics/` (already broken in 2.x) | `basics.md:3` |

### timeline.md
**Verdict: substantial rewrite of the lower half; the transport and shortcut tables are right.**
Images: `qcv011` to `qcv018`, `qcv033`, `qcv034`. All stale (G1). `qcv015` shows the overview zoom indicator, which no longer exists; `qcv033` shows the pencil / edit mode, which no longer exists; `qcv014` shows the readout "top-left".

| Statement | 3.0 | Evidence |
|---|---|---|
| Transport table and keys | All correct | [code] `crates/qcv-player/src/key_map.rs:24-50`, `src/shortcuts.rs:58-70` |
| "The right end of the row holds loop, fullscreen and Compact Mode, the two screenshot buttons, the Color and Notes panel toggles and the volume" | Also an Open media button at the left, Safety guides and Background buttons, and a review-speed chip | [code] `src/transport_bar.rs:50-94` |
| Link `/basics/#layout-presets` | Broken permalink | see basics.md |
| "In playlist mode, additional Prev clip / Next clip buttons appear" | The two buttons are always drawn and always **disabled** | [code] `src/transport_bar.rs:53,61` (`Bind::Later`), `:128` (`Bind::Later => false`) |
| Screenshot table (T, Ctrl/Cmd+T), "with OCIO transforms applied" | Keys right. File is named after the media; formats PNG / PNG 16-bit / JPEG / TIFF 16-bit; alpha kept when the media has it | [code] `src/settings_panel.rs:80-81`; canon 216, 223, 224 |
| "The top-left corner displays the current timecode and frame count" | They are wells on the status strip, in the bottom bands | canon 169; [code] `src/status_strip.rs` |
| Not mentioned | Click the frame or timecode well to go to a frame / timecode / time | [code] `src/status_strip.rs:89-90` |
| "Timecode Origin pill" | An Origin picker in the Inspector's Timecode group | [code] `src/inspector_panel.rs` ("Origin", "From start", "Embedded", "QT Start", "QT TC", "XMP Alt") |
| "Dropping media on the timeline": lanes A / B load a side; the single lane loads the active source | Only a playlist's clip bar / scrubber takes drops. Dual sides are loaded by dropping on a chip or on the picture | [code] `src/main.rs:173-184` |
| "drag the edges of the overview-bar zoom indicator" | Retired. Double-click fits; Ctrl/Cmd + = and - zoom; middle-button drag pans | G7; [code] `src/shortcuts.rs:90-92`; canon 247 |
| Zoom / pan table (Wheel, Alt + Wheel) | Still true | [code] `src/shortcuts.rs:91` |
| Volume, mute, loop tables | Correct | [code] `key_map.rs:38-41`, `src/main.rs:2330` |
| "In/out range is disabled in playlist mode" | UNVERIFIED. The buttons are enabled whenever something is loaded (`src/transport_bar.rs:128`); no playlist gate was found | |
| "Editing ... toggle edit mode for a track by clicking the pencil icon" | No edit mode. Ctrl/Cmd+K and Delete act directly; dual adds `[` `]`, Alt+arrows, Shift+Delete | G7; [code] `src/shortcuts.rs:79-88` |
| "In Playlist mode ... drag it to a new location" | True on the clip bar, and in the Playlist Contents tree | canon 167, 192 |
| Not mentioned | Review speed: `R` cycles, `Shift + R` resets | [code] `key_map.rs:42,47` |

### manager.md (Project Panel)
**Verdict: small edits, plus one section that describes something 3.0 does not have (Clip colour).**
Images: `qcv019`, `qcv020`, `qcv040`. All stale; `qcv040` shows the badges and the bulk Input picker, neither of which exists.

| Statement | 3.0 | Evidence |
|---|---|---|
| "The Project panel lives in the Left Rail" | The section is titled **Media** | [code] `src/project_panel.rs:76` |
| Bins "Media, Playlists, Dual Views" | Groups: Videos, Image Sequences, Images, Audio, Live, Playlists, Dual Views | [code] `src/project_panel.rs` strings |
| Not mentioned | A loaded playlist opens into a tree of its clips (click seeks, drag reorders, bin rows drop in) | canon 192 |
| "Dragging items past the app's borders is restricted to a link-action drop" | UNVERIFIED | |
| Whole "Clip colour" section: badge on the row, right-click "Input >", "Use Clip Chain of ...", "Reset Clip Chain" | Not ported. The row menu is Create Playlist from Selection and Delete. The only row badge is the unsupported-media warning. The nearest equivalents are the Color drawer's "Apply to all clips" and "Reset to default" | [code] `src/project_panel.rs:9-12` (states the deviation), `:245`, `:339` |
| "Save / Load Projects ... `.qcvproj`" | True | canon 141 |
| "Copy this link to your clipboard from the main menu under File" | True | [code] `src/menu.rs:60` |

### inspector.md
**Verdict: substantial rewrite.** The Scopes section (a third of the page) belongs on the colour page now.
Images: `qcv021`, `qcv022`, `qcv024`, `qcv041`. All stale; `qcv041` shows the vectorscope and the waveform in the right rail.

| Statement | 3.0 | Evidence |
|---|---|---|
| "The Vectorscope and Waveform sections sit at the top of the Inspector" | No vectorscope. The waveform is a card in the Color drawer | G6 |
| Scope labels "Input / Assumed / Signal" | The Signal tier is not carried | canon 203 ("Not carried: QCView's Signal tier") |
| "Vectorscope" paragraph | Delete | G6 |
| "the Auto · % · nits chips" | SDR / HDR chips; HDR is the default for PQ / HLG sources | [code] `src/color_drawer.rs:136`; canon 203 |
| "pick the top of the scale with the 300 … 4k chips" | The HDR top is cycled through the same five values | canon 203; `src/settings.rs:135` |
| "Frame · Max ... hover it for the brightest channel, and click Reset" | Readout and tooltip exist. The Reset control: UNVERIFIED | [code] `src/color_drawer.rs:359-377`; canon 213 |
| "The choice is shared with the vectorscope" | Delete | G6 |
| "In dual view each side is read from its own file, drawn in its own colour" | True (A cyan, B orange) | canon 209 |
| Path actions: Copy path, Reveal | True, as the File group's last row | canon 193 |
| File and Video cards (container, MXF OP, DNx names, Bitrate) | Carried row for row | canon 177 |
| "Projects saved before these fields existed are back-filled on open" | UNVERIFIED | |
| "togglable pills" | Pickers / chips. Range, Mix, Origin, "Transfer / encoding override" exist | [code] `src/inspector_panel.rs` strings |
| "Timecode Origin ... (DV, TimeCode, MXF), or From start" | From start / Embedded / QT Start / QT TC / XMP Alt | [code] as above |
| "Broadcast Master Audio Mix ... 5.1 toggle" | Mix picker: Auto / 5.1 to 2 / 7-8 | canon 177 |
| "Adobe Projects section scans ... via ExifTool ... with an Open button" | The group is **Linked Projects**; rows have Open, Copy, Reveal, disabled when the path is not reachable; no ExifTool | canon 134, 193; [code] "Linked Projects", "Not reachable from this machine" |
| Image Sequences paragraph | True (Layer, Frame rate, Cache stride, cache ahead / behind) | [code] `src/inspector_panel.rs:418-434` |
| Not mentioned | Geometry group: pixel aspect and rotation pickers. In dual view the Inspector follows the selected side | canon 177, 213 |

### color.md (OCIO Color)
**Verdict: substantial rewrite.** The page's whole structure (three groups, clip half and view half) is gone.
Images: `qcv025`, `qcv039`. Both stale; `qcv025` shows the Setup / Clip / View groups, `qcv039` the A / B tabs and chip badges.

| Statement | 3.0 | Evidence |
|---|---|---|
| "second, OCIO-free engine ... picked with the OCIO \| minColor segment" | minColor is the default; an OCIO chain is started with "New OCIO chain…" in the one preset list | G4; [code] `src/color_drawer.rs` |
| "Bundled configs: ... Blender 5.1" | Dropped. New: "Open config…" loads a config file of your own | G10; [code] `src/color_drawer.rs` ("Open config…") |
| "appended with Linear sRGB EDR and Linear P3 EDR display outputs" | Not something the user selects any more | canon 183 |
| "Use the OCIO On / Off switch at the bottom" | One Color On / Off switch for whichever engine; **off by default** | [code] `src/settings.rs:130`, `src/color_drawer.rs` ("Color On", "Color Off") |
| Layout table (Setup / Clip / View) | OCIO columns are Config, Input, Look, View, Highlight Knee, with a Presets list | [code] `src/color_drawer.rs` strings; canon 205 |
| "Presets ... dimmed, and each one is tagged SDR or HDR" | No tags, no dimming; there is no display mode | G5; [absent] |
| "Scene LUT ... `.cube` file, or an ASC CDL (.cc, .ccc, .cdl) ... Correction ID field" | No such column | [absent]; see index.md row on stored `sceneLut` |
| "Host display — set by a QCBridge for Blender stream link" | Not carried | canon 236 |
| "A column the clip has set shows the reset arrow in its header" | Replaced by words: "Own chain" / "Following the project default", with Reset to default, Apply to all clips, Make project default | canon 205 |
| "A clip with its own chain shows a badge ... on its row in the Project panel and on its A / B chip" | No badges | [code] `src/project_panel.rs:339` (the only badge is "unsupported") |
| View group: "Output", "View", "Display LUT" shared by everything on screen | No Output, no Display LUT. View is part of each clip's chain | G4, G5 |
| "In dual view the Clip group has an A and a B tab" | No tabs; the drawer edits the selected side | canon 213 |
| "use the Project panel's right-click menu" | Not ported | see manager.md |
| Highlight Knee (Source peak, Use file MaxCLL, Target peak, Knee start, KNEE pill) | Carried; also "Use mastering peak" | [code] `src/color_columns.rs` strings |
| Viewer aids table (Exposure ±4, Gamma 0.25-4, channel view) | Ranges match. New in the same row: **Video gamma** | [code] `src/color_drawer.rs:339-340, 465-466`; canon 246 |
| "They need OCIO on" | UNVERIFIED | |
| "click Save as…, Save updates the current preset, Delete removes it, arrows step ... shows modified" | Editing makes a "Custom (from …)" row with Save as preset…; Rename / Update / Delete are on a user preset's right-click menu | canon 206; [code] `src/color_drawer.rs` strings |
| "Export LUT" | No such action | [absent] in `src/` (the phrase survives only in the inherited translation catalogs) |
| Not mentioned | Amber "SDR display — highlights clip" pill; the waveform lives here | canon 222; G6 |

### mincolor.md
**Verdict: substantial rewrite** (smaller than color.md: the AgX and Input sections survive).
Images: `qcv042`, `qcv043`. Both stale (the panel is a drawer with different columns; the preset named in `qcv042`'s alt text no longer exists).

| Statement | 3.0 | Evidence |
|---|---|---|
| "The Color panel has a second engine beside OCIO" | minColor is the first and default engine | canon 198 |
| "Pick the engine with the OCIO \| minColor segment ... a fresh install starts on OCIO" | No segment. A fresh install has colour off; the project default is the minColor "Rec.709 video" chain | canon 207; [code] `src/settings.rs:130`, `src/color_model.rs:119` |
| Chain table: "View: AgX, Rendering, Display" | Columns: Input gamut, Input transfer, Highlight Knee, AgX, Rendering. No Display | [code] `src/color_drawer.rs:5-6`; canon 181 |
| "clips with their own settings carry a badge" / reset arrow | Gone | see color.md |
| Input gamut and Input transfer lists | UNVERIFIED item by item | |
| AgX table | Controls match | [code] `src/color_columns.rs` strings |
| Rendering: "Un-tone-mapped ... the only rendering the reel offers" | Un-tone-mapped or AgX; an OpenDRT look is shown when a chain carries one | canon 199; [code] `src/color_drawer.rs` ("Rendering · AgX", "OpenDRT · {}") |
| Whole "Display" section | Removed | canon 181, 199 |
| "minColor has its own preset list, separate from OCIO's" | One list for both engines | canon 207 |
| Preset table (each input to sRGB / EDR P3 / Rec.2100 PQ) | One preset per input. Built-ins: Rec.709 video, sRGB, ACEScg, ACES 2065-1, Linear Rec.709, Linear Rec.709 · AgX, Linear Rec.2020, Linear Rec.2020 · AgX, P3-D65 PQ master | [code] `src/color_model.rs:101-116` |
| "Netflicker SDR" | The built-in is now "P3-D65 PQ master" | same |
| "Export LUT… bakes the active minColor chain" | No such action | [absent] |
| "an `Input · minColor` badge naming the transfer" on the scopes | UNVERIFIED | |
| "Switching engines never loses anything" | The model changed (one chain per clip); needs rewording | canon 205 |
| "The full notices ship with the app in `LICENSES/minColor-NOTICE.txt`" | Notices are one generated `THIRD-PARTY-NOTICES.html`, opened by About > Licenses… | canon 252 |

### annotations.md
**Verdict: small edits.**
Images: `qcv026`. Stale (Notes is a bottom drawer now).

| Statement | 3.0 | Evidence |
|---|---|---|
| "Click Add Note" | The button reads "Add note here" | [code] `src/notes_drawer.rs` |
| "A diamond marker appears on the timeline" | No note markers on the scrubber were found | [absent] `src/main.rs:1988` (the scrubber's state carries no notes); `crates/min-ui/src/scrubber.rs:4` ("pennants (later: notes)") |
| "annotation toolbar on the right of the Notes panel" | Position in the drawer: UNVERIFIED. Tools match (Select, Freehand, Rectangle, Oval, Arrow, Line, Eraser, colour, width) | [code] `src/notes_drawer.rs` strings |
| Off-picture strokes paragraph (2.7.0) | UNVERIFIED in 3.0 (canon 238 says strokes are "clipped to the picture" on outputs) | |
| "captured through the active OCIO chain and marked OCIO" | Through the clip's chain, either engine; tags OCIO / VIEWER exist | canon 216, 217 |
| "If the Notes panel is open, screenshots are captured with embedded annotations" | True, and now the rule: only while the panel shows them | canon 238 |
| Export table | Formats match. HTML is now a synopsis table with a lightbox; PDF and DOCX are one note per page | canon 217; [code] `src/notes_drawer.rs` ("Markdown folder…", "HTML (single file)…", "PDF…", "DOCX…") |
| Stream notes folder `<project folder>/.qcview/streams/<stream name>/` | `<name>__<first 8 of the id>` | [code] `crates/qcv-notes/src/io.rs:36-39` |
| "or click Add note here to freeze it" | Opening the panel no longer freezes; a stroke freezes at its start | canon 233 |
| "Sending notes to minNotes ... Open in minNotes" | Not present | [absent]; canon 233 (owed) |
| Not mentioned | Notes work in single view only; Color and Notes cannot be open together; another app's edits to the sidecar are merged while the panel is open | canon 215, 219 |

### image.md (Image Sequences)
**Verdict: small edits.**
Images: `qcv024`. Stale (G1).

| Statement | 3.0 | Evidence |
|---|---|---|
| Loading, frame rate, EXR layer | True | [code] `src/inspector_panel.rs:418-434` |
| Formats implied (EXR, TIFF, PNG, JPEG) | Add DPX, TGA, BMP | [code] `extensions.rs:11`; canon 190, 191 |
| "The cache progress bar" | A cache underline on the scrubber; Inspector shows cache ahead / behind | [code] `src/main.rs:1987` |
| "The Transfer chips on the Image Sequence card" | True ("Transfer / encoding override") | [code] `src/inspector_panel.rs` |
| "tell the scopes how to read the frames ... With OCIO on, the clip's Input still drives" | Reword for the waveform and for either engine | G4, G6 |
| "A red bar will appear over missing frames" | True | [code] `crates/min-ui/src/scrubber.rs:228-234` |
| Stride "2x, 3x, 4x" chips | "Cache stride" chips exist; exact values UNVERIFIED | [code] `src/inspector_panel.rs:434` |

### settings.md
**Verdict: small edits.**
Images: `qcv010`. Stale (G1).

| Statement | 3.0 | Evidence |
|---|---|---|
| "Ctrl + , (or Tools > Settings…) ... Left Rail under the Settings section" | True | [code] `src/menu.rs:74` |
| Image Sequence Settings, A/V Sync (Single / Dual), Hover Thumbnails | True | [code] `src/settings_panel.rs:68-69, 77, 84-85` |
| "Drag Viewport to Move Window ... while a drawing tool is selected, while dragging the Split-Wipe seam, or in fullscreen" | True; also works in Compact Mode | canon 251 |
| Language: "Languages marked *machine translation*" | The label is dropped | canon 196 |
| Language: link to `QCView-Player/tree/main/translations` | Catalogs are `apps/qcv/i18n/qcview_<code>.po` in minplatform | canon 195 |
| Blender sessions: "with the host's colour setup applied" | The host's colour never drives QCView | canon 236 |
| Blender sessions: "the host's colour notes appear under it in red" | Still shown | canon 236 (last sentence) |
| Link `/qcbridge-blender/` | Permalink is `/qcbridge/blender/` (already broken) | |
| Live audio: A/V sync (Live), live audio buffer | True | [code] `src/settings_panel.rs:88-89` |
| Output: "the NDI sink, the AJA SDI / HDMI sink and the stream to a review service" | No stream | G8 |
| Not on the page | Performance group (decode threads, hardware decoding on Windows, scrub cache, hardware scrub); Timeline audio waveforms; Screenshot format; Include safety guides in screenshots; Mute scrub audio; Updates (macOS); Reset all preferences… | [code] `src/settings_panel.rs:59-100` |
| Not on the page | 2.x settings are not imported. "Always open in minimal mode" and the "Listen" row are gone | canon 248, 245, 231 |

### dualview.md (Dual Video Review)
**Verdict: substantial rewrite.**
Images: `qcv035` to `qcv038`. All stale: `qcv035` shows chips above the viewport (G9), `qcv038` shows edit mode (G7).

| Statement | 3.0 | Evidence |
|---|---|---|
| "Unlike QCView v1 ..." | Dated framing | |
| "Above the viewport are an A chip and a B chip ... buttons on the right side of this panel" | On the status strip, right side | G9 |
| "The timeline lanes work the same way: drop on lane A ..., on lane B ..." | Lanes are not drop targets; chips and the picture are | [code] `src/main.rs:173-184` |
| "Dual view exits to single only when you choose Single mode, load a playlist, or start/open a different project" | UNVERIFIED | |
| View modes table, seam drag, gain slider | True (gain ×1 to ×16) | canon 173 |
| "In the Color panel, the A and B tabs pick which side's clip you are editing; the Output, View and Display LUT are shared" | No tabs, nothing shared; the drawer and the Inspector follow the selected side | canon 205, 213 |
| "Each chip shows its clip's colour badge" | No badges | see color.md |
| "Per-side mute and an A/B level mixer live in the Inspector's Audio routing controls" | Per-side mute is a speaker pill on each chip. No level mixer was found | [code] `src/dual_controls.rs` ("A audio on — click to mute"); mixer [absent] |
| "The right side is muted by default" | True | canon 211 |
| Separate A/V sync offset for dual | True | [code] `src/settings.rs:99` |
| Whole "Live sides" section | A live source cannot be a dual side | [code] `src/main.rs:379-391`; canon 211 (d) |
| "Aligning Clips ... In edit mode (the per-track edit toggle)" | No edit mode. Add: target side, snapping, `[` `]`, Alt+arrows, Delete vs Shift+Delete, moving a clip carries later clips, offset readout in the status strip | G7; canon 212; [code] `src/shortcuts.rs:79-88` |
| "Save as Dual View (the floppy-disk button in the viewport overlay)" | A button on the status strip; opens a name dialog; Update for a saved one | canon 173; [code] `src/dual_controls.rs` |
| Not mentioned | Dual hover thumbnails (A above B), dual waveform overlay, dual screenshots at source resolution | canon 170, 209, 216 |

### output.md
**Verdict: small edits to the top half; the bottom half ("Stream (SRT or RTMP)", about 30 of 66 lines) describes something 3.0 does not have.**
Images: `qcbridge_016.png`, `qcbridge_017.png`. Both stale: the alt text names the Stream card, and the cards are redesigned (switch and status dot instead of Start / Stop).

| Statement | 3.0 | Evidence |
|---|---|---|
| "or as a stream over SRT or RTMP" | Removed | G8 |
| "Start / Stop in the header ... the controls behind the chevron" | A status dot and an on / off switch per card; controls are on the card | [code] `src/output_panel.rs:1-8` |
| "Mute feed (the chip at the top)" | A speaker toggle in the section header. **The feed is muted by default** | [code] `src/output_panel.rs:2`, `src/settings.rs:106` |
| "Guides and Notes (the other two chips)" | Two header toggles. Note strokes go out only while the Notes panel is open in single view | canon 238 |
| Raster and Transfer paragraphs | True | [code] `src/output_panel.rs` strings |
| NDI and AJA paragraphs | Design unchanged | canon 237 |
| "For example 'T-TAP Pro UHDp25 10-bit PQ 2020, SDI + HDMI RGB'" | The AJA path has run only against a stand-in sink in 3.0; the hardware walk is owed | handoff "Next" item 1 |
| Everything from "## Stream (SRT or RTMP)" to the end | Delete | G8 |
| Front matter `nav_order: 12` | Same value as dualview.md | |

### playlists.md
**Verdict: small edits.**
Images: `qcv030`. Stale (G1, G7).

| Statement | 3.0 | Evidence |
|---|---|---|
| "click the + button to the right of the Playlist header in the left rail" | True; also right-click > Create Playlist from Selection | [code] `src/project_panel.rs:9-12, 245` |
| "drag ... from the Project panel into the timeline" | Onto the clip bar / scrubber, or into the playlist's tree in the Media section | canon 167, 192 |
| "Enter edit mode on the playlist track to drag clips" | No edit mode; drag directly | G7 |
| Per-clip trims | True; plus a precision popover (source in / out / duration) | canon 167 |
| "the transport bar shows Prev clip and Next clip buttons when a playlist is active" | Both buttons are disabled | [code] `src/transport_bar.rs:53,61,128` |
| Audio paragraph | UNVERIFIED | |

### thumbnails.md
**Verdict: small edits.**
Images: `qcv031`. Stale (position changed).

| Statement | 3.0 | Evidence |
|---|---|---|
| "preview the frame in the corner of the viewport" | The thumbnail follows the pointer above the bands; in dual, a pair (A above B) | [code] `src/hover_thumbs.rs:1-6` |
| "render through the same OCIO pipeline" | Through the clip's chain, either engine | canon 216 |
| "or by clicking the toggle on the transport button row" | The transport button exists but is disabled; the Settings switch works | [code] `src/transport_bar.rs:80,128`; `src/settings_panel.rs:77` |

### hdr.md
**Verdict: substantial rewrite.** The page is built around a Display mode list that no longer exists.
Images: `qcv032`. Shows the Display mode list: describes something 3.0 does not have.

| Statement | 3.0 | Evidence |
|---|---|---|
| "The Display list at the bottom right of the Color panel switches between the SDR and HDR modes" | No list; the surface follows the monitor | G5 |
| "`SDR — sRGB` ..., `HDR10 PQ` ..., `EDR — Linear P3`" | None of these are choices | G5 |
| "presets that don't suit it are dimmed, and each preset is tagged SDR or HDR" | No | see color.md |
| "SDR mode: use an sRGB or a Rec.1886 output" | No Output column | G4 |
| "To check an HDR master on an SDR display, turn on the clip's Highlight Knee ... KNEE pill" | True, and now the only way: there is no automatic tone mapping. The drawer shows an amber "SDR display — highlights clip" pill | canon 220, 222 |
| "HDR mode (Windows) ... `HDR10 PQ` ... `Rec.2100-PQ` output" | scRGB; brightness is relative (PQ 100 nits = the user's SDR white) | canon 221 |
| "HDR mode (macOS — EDR) ... matching `Linear P3 EDR` output" | Nothing to match | canon 183 |
| "With minColor ... three flavours per input ... The Display reel" | One preset per input; no Display | see mincolor.md |
| "HDR sources: pick the source's colorspace as the clip's Input" | True for OCIO; minColor uses Input gamut + Input transfer | G4 |
| "The Inspector's HDR metadata row" | True | [code] `src/inspector_panel.rs` ("HDR metadata") |
| "A QCBridge for Blender 0.3 stream ... shown through the host's own view and display device" | Not carried; a Blender stream reads "Auto · PQ 2020" from its tags | canon 236, 234 |
| "QCBridge Transmit feed ... untagged" / Transfer pill and live-strip chip | True; the Auto label for a host feed is "Auto · working space, untagged" | canon 230; [code] `src/live_strip.rs` |
| "HDR out of an output" | True for AJA (hardware walk owed) | canon 237 |
| A stray bullet after that section ("With OCIO on, the clip's Input still decides ...") | Misplaced in 2.x already | `hdr.md:65` |
| "The Auto · % · nits chips ... see Scopes (/inspector/#scopes)" | SDR / HDR chips; the waveform is in the Color drawer | G6 |
| Not mentioned | Video gamma toggle; Windows 10-bit surface on SDR monitors | canon 246, 220 |

### privacy.md
**Verdict: small edits.**
Images: none.

| Statement | 3.0 | Evidence |
|---|---|---|
| "Windows: `%LOCALAPPDATA%\QCView\`", "macOS: `~/Library/Application Support/QCView/`" | Settings and presets: `~/Library/Application Support/qcv/`, `%APPDATA%\qcv\`. Logs: `~/Library/Logs/qcv`, `%LOCALAPPDATA%\qcv\logs`. The `QCView` folders still hold `toolbox.json`, the ffmpeg mirror (Windows) and the Inputs / Outputs registrations | [code] `crates/min-settings/src/lib.rs:65-71`, `src/settings.rs:8`, `src/logging.rs:2,14`, `src/toolbox.rs:5-6`, `src/blender.rs:25-35`; canon 232. Where these land inside the Store package's redirected storage is UNVERIFIED |
| "Frame cache and temporary render files" | UNVERIFIED (the caches read as in-memory) | |
| "Project files and timeline data", "Annotation notes ..." stored there | Notes are sidecars beside the media or in the project folder | canon 215, 233 |
| "The Help menu contains links" | The menu is **About** (Docs, Licenses…). Docs opens the browser at qcview.app; Licenses… opens a local file | [code] `src/menu.rs:76-81`, `src/main.rs:1728-1731` |
| Software Updates (macOS) section | Accurate | [code] `src/sparkle.rs`; `src/settings_panel.rs:95` |
| "Last updated: May 2026" | Revise the date | |
| Not mentioned | QCView listens on a network port when the user opens a listener stream or switches on Blender sessions; outputs are published only when switched on | canon 235, 243, 237 |

Note: the repo's root `PRIVACY_POLICY.md` (line 31: "QCView makes no automatic network connections") is the stale one; `docs/privacy.md` was already corrected in May.

### llm_transparency.md
**Verdict: accurate as is.** A personal statement; nothing in it is version-specific. The owner may want to add a sentence about the 3.0 rewrite, but nothing is false.
Images: `Don_Quixote_fighting_windmills.jpg` (not a screenshot).

### qcbridge.md (QCBridge Add-Ons)
**Verdict: small edits.**
Images: none.

| Statement | 3.0 | Evidence |
|---|---|---|
| "The Output section also streams the viewport as H.264 to a review service such as Louper without an add-on" | Removed | G8 |
| "A live feed ... can be sent on any of these ways" | On NDI or AJA | G8 |
| "converted to 1/2 float in QCView" | True (RGBA16F) | canon 228 |
| "there is the option to use QCview as the live preview window" | True | canon 235 |

### qcbridge-adobe.md
**Verdict: small edits.**
Images: `qcbridge_006.png` (After Effects preferences: not QCView, likely fine), `qcbridge_007.png`, `qcbridge_008.png` (QCView with a feed: stale).

| Statement | 3.0 | Evidence |
|---|---|---|
| Requirements: "QCView 2.4.0 or newer ... the stream output need QCView 2.5.6" | Drop the stream output clause; add 3.0 | G8 |
| "In QCView: File > Connect to After Effects (or Connect to Premiere Pro) ... Live bin" | True | [code] `src/menu.rs:56`, `src/live.rs:62-64` |
| "Set QCView's OCIO input to your project's working space" | Set the clip's Input in the Color drawer (minColor gamut + transfer, or an OCIO Input) | G4 |
| "with OCIO off the scopes read it as SDR" | The Auto label is "Auto · working space, untagged" | canon 230; [code] `src/live_strip.rs` |
| "Listen in QCView" described under the Inspector's Audio card | A speaker toggle on the live row only; the Inspector switch is not carried. Off by default | canon 231; [code] `src/live_strip.rs` ("Listen in QCView") |
| "Mute scrub audio applies to the feed too", "A/V sync (Live) in Settings", jitter buffer | True | [code] `src/live.rs:76-78` |
| "Dual view takes a feed on either side" and "use them in QCView Dual View review modes" | Not in 3.0 | [code] `src/main.rs:379-391` |
| "Sending it on ... or as an H.264 stream to a review service" | Removed | G8 |
| Not mentioned | A host that is not at 48 kHz is resampled; Review freezes the float frame | canon 228, 233 |

### qcbridge-aja.md
**Verdict: small edits.**
Images: `qcbridge_018.png` (the AJA card live: stale, the card is redesigned).

| Statement | 3.0 | Evidence |
|---|---|---|
| "QCView 2.5.7 or newer" | Add 3.0 | |
| "open its chevron for the settings", "Press Start. The row reads *starting*, then *live*" | A switch; status words are live / waiting for the sink / refused / not installed | [code] `src/output_panel.rs:1-8`, `src/outputs.rs` strings |
| "Mute feed silences it" | True; note the feed starts muted | [code] `src/settings.rs:106` |
| "1080p, 720p or UHD" | Raster choices in QCView are Auto / 1080p / UHD | [code] `src/output_panel.rs` strings |
| "Windows is where it has been proven; the macOS build ... has not yet run against a device" | With 3.0 neither OS has run against a device; only a stand-in sink | handoff "Next" item 1; canon 237 |
| Picture and colour section | Design unchanged | canon 237, 238 |

### qcbridge-blender.md
**Verdict: small edits to setup and usage; the three colour paragraphs at the end (about a fifth of the page) describe behaviour 3.0 deliberately does not have.**
Images: `qcbridge_001.jpg`, `qcbridge_005.png`, `qcbridge_009` to `qcbridge_016.png`. Most show the Blender agent and add-on, not QCView, so they are likely fine. `qcbridge_005.png` (alt text "QCBridge", placed after "the picture arrives") may show QCView: check it. `qcbridge_016.png` is used here as "Agent settings window" under Path mapping **and** in output.md / qcbridge-ndi.md as the Output section; one of those uses is wrong in 2.x already.

| Statement | 3.0 | Evidence |
|---|---|---|
| "QCView 2.7 or newer on the host" | Add 3.0 | |
| "The replica needs `ffmpeg`; QCView installed there is the easy way" | Still true: 3.0 registers its ffmpeg for the suite | [code] `src/toolbox.rs:5-6` |
| "File > Connect to Blender", "Settings > Blender sessions, off at every launch" | True | [code] `src/menu.rs:56`, `src/settings_panel.rs:87` |
| Inputs folder paths | True | [code] `src/blender.rs:25-35` |
| "load it on one side of a Dual View against an approved render" | Not in 3.0 | [code] `src/main.rs:379-391` |
| "The card ... the colour setup, the mismatch rows" | The card's colour block is read but unused; the mismatch rows still show under the switch | canon 236 |
| "Colour (QCBridge 0.4 and later)": the card pins the clip's Input, turns colour management on, shows a toast | Nothing automatic. The stream shows as "Auto · PQ 2020" from its tags; any chain is the user's | canon 236 |
| "The host's view, for the host's display": Host display pin, the chip and its menu | Not carried | canon 236 |
| "With the minColor engine on screen only the Input is pinned ... the toast offers the switch to Blender 5.2" | Not carried | canon 236 |
| "the Transfer chip on the live strip names the encoding" | True | [code] `src/live_strip.rs` |
| Not mentioned | A listener URL opened by hand listens without the Blender switch; the passphrase is hidden everywhere | canon 243, 234 |

### qcbridge-ndi.md
**Verdict: small edits.**
Images: `qcbridge_016.png` (the NDI card live: stale).

| Statement | 3.0 | Evidence |
|---|---|---|
| "QCView 2.5.5 or newer" | Add 3.0 | |
| "press Start on the NDI row ... reads *starting*, then *live*" | Switch the card on | [code] `src/output_panel.rs:1-8` |
| "The feed mute and the layers QCView allows are the chips at the top" | Header toggles; the feed starts muted, so a first NDI session is silent until it is switched on | [code] `src/settings.rs:106` |
| "The audio is the mixed audio QCView plays, before your own monitoring volume and mute" | Doubtful. Canon 237 says the tap is before Listen / mute / volume, but the Mute feed tooltip says "The master mute (M) silences both" | [code] `src/output_panel.rs` tooltip. UNVERIFIED which is right |
| "It is a Rec.709 SDR source" | True (the NDI sink names SDR itself) | [code] `src/output_panel.rs:5-6` |

---

## 3. 3.0 features and behaviours that no page covers

Colour
- **Video gamma** toggle and its amber pill (canon 246).
- The amber "SDR display — highlights clip" pill; no automatic tone mapping (canon 220, 222).
- Project default chain, "Apply to all clips", "Make project default", "Reset to default" (canon 205).
- The "Custom (from …)" row and saving presets from it; right-click Rename / Update / Delete (canon 206).
- "New OCIO chain…" and "Open config…" for a config file of your own (`src/color_drawer.rs`).
- Colour is off on a fresh install (`src/settings.rs:130`).
- Windows picks a 10-bit surface on SDR monitors and a float surface on HDR monitors, by itself; HDR brightness is relative on both OSes (canon 220, 221).

Viewing and transport
- Review speed: `R` / `Shift + R` and the speed chip (`key_map.rs:42,47`). In 2.x since 2.4.0, never documented outside the version log.
- Go to frame / timecode / time by clicking the status strip's wells (`src/status_strip.rs:89-90`). In 2.x since 2.2.5, same.
- Fullscreen: the bands appear at the bottom edge (canon 171).
- Middle-button drag pans the timeline; Ctrl/Cmd + = and - zoom; double-click fits (canon 247).
- `B` cycles the background (`key_map.rs:40`).
- Inspector Geometry: pixel aspect and rotation pickers (canon 177).
- Hover thumbnail follows the pointer; a pair in dual view (canon 170).
- An Open media button on the transport row.

Dual and playlists
- Editing without an edit mode: target side, snapping, `[` `]`, Alt + arrows, Delete vs Shift + Delete, the precision popover, the offset readout (canon 167, 212).
- Playlist Contents tree in the Media section: click to seek, drag to reorder, drop bin rows in (canon 192).
- Save as Dual View dialog and Update (canon 173).

Files, screenshots, notes
- DPX, TGA, BMP sequences (canon 190, 191).
- Screenshot formats (PNG, PNG 16-bit, JPEG, TIFF 16-bit), alpha kept, file named after the media, the "Include safety guides in screenshots" setting (canon 216, 223, 224).
- HTML notes export with a synopsis table and lightbox (canon 217).
- Notes sidecar merged with another app's edits while the panel is open (canon 219).
- Safety guide choice, opacity, width and colour all persist (canon 215).

App
- `Ctrl/Cmd + N` New Project (`src/main.rs:2340`).
- About > Licenses… (canon 252).
- No usable graphics device: the message and exit (canon 250).
- Dragging the picture moves the window in Compact Mode too (canon 251).
- 2.x settings are not imported; 2.x projects open (canon 248).
- The right rail starts collapsed (`src/settings.rs:139`).
- Shortcuts card filter (canon 218).
- Settings: Performance group, Timeline audio waveforms, Reset all preferences… (`src/settings_panel.rs`).
- Paste works in every text field; Open Stream has a hidden passphrase field (canon 243, 234).
- A listener stream URL opened by hand listens without the Blender switch (canon 243).
- The output feed is muted by default (`src/settings.rs:106`).

---

## 4. 2.x features documented that 3.0 lacks

Verified in code or by a dated ruling:
1. Stream output card: SRT / RTMP, Louper (canon 244).
2. Vectorscope (canon 203).
3. Display mode list and the HDR10 PQ / EDR mode choices (canon 179, 220).
4. OCIO Output column, Display LUT, the shared "View" half (canon 199, 205).
5. Scene LUT / ASC CDL / Correction ID controls ([absent]; the stored path is still carried).
6. Export LUT ([absent]).
7. The OCIO \| minColor segment and per-engine preset lists (canon 207).
8. Blender 5.1 config; built-in OCIO presets; minColor's per-output preset triples; SDR / HDR preset tags (canon 198).
9. Clip colour badges in the Project panel and on the A / B chips (`src/project_panel.rs:339`).
10. Project panel bulk colour menu: Input >, Use Clip Chain of, Reset Clip Chain (`src/project_panel.rs:9-12`).
11. A / B tabs in the Color panel (canon 213).
12. Minimal Mode, Show All Panels, the Status Bar toggle, Default View (Ctrl + R), View > Timeline, "Always open in minimal mode" (canon 245, 219).
13. Track edit mode and its pencil; the overview zoom bar (canon 166, 167).
14. Dropping media on timeline lanes in single and dual view (`src/main.rs:173-184`).
15. **Live sources as a dual-view side** (`src/main.rs:379-391`).
16. **Prev clip / Next clip transport buttons**: drawn but disabled (`src/transport_bar.rs:53,61,128`).
17. **Hover-thumbnail toggle on the transport row**: drawn but disabled (`src/transport_bar.rs:80,128`).
18. Blender colour link: Input pin, Host display chip, automatic colour management, the toasts (canon 236).
19. "Listen in QCView" in the Inspector (canon 231).
20. "Open in minNotes" (owed, canon 233).
21. The "machine translation" label (canon 196).
22. ExifTool (canon 134).
23. The scopes' Signal tier and the Auto · % · nits chips (canon 203).
24. Import of existing settings on upgrade (canon 248).

Not found, weaker evidence:
25. Note markers on the timeline ([absent]).
26. The A / B level mixer in the Inspector ([absent]).
27. JPEG-2000 / HTJ2K sequences (not in `extensions.rs`; not in 2.x's list either).

Items 15, 16, 17 and 20 read as unfinished rather than ruled out. They are worth raising with the owner before the docs are written, because the docs either drop them or wait for them.

---

## 5. Raw material for "What's new in 3.0"

Style reference: `docs/installation.md`, "What's new in 2.7.3" (bold lead word, one or two sentences, user-visible effect).

| Topic | Change | Source |
|---|---|---|
| Rewrite | QCView is rebuilt in Rust on a GPU-drawn interface; it upgrades 2.x in place through the same channels (Sparkle on macOS, the Store on Windows) | canon 248; `docs/phase2/step5-release.md` |
| Upgrade | Settings start from defaults; 2.x projects, sidecar notes and presets-in-projects open as they are | canon 248 (ruling 2), 141, 215 |
| Look | New interface: softer greys, rounded section tiles in the rails, less blue, one bottom stack of status strip, transport and scrubber | canon 160-169, 174 |
| Timeline | One 32 px scrubber with ruler, playhead, in / out, cache line and a faded audio waveform; the overview bar is gone; double-click fits, middle-drag pans | canon 166, 247 |
| Editing | No edit mode. Dual view and playlists are edited directly on clip bars, with snapping, a frame-accurate popover and new keys (`[`, `]`, Alt + arrows, Shift + Delete) | canon 167, 212 |
| Playlists | The loaded playlist's clips are a tree in the Media section: click to seek, drag to reorder, drop to add | canon 192 |
| Color | minColor is the default engine and OCIO the option; each clip carries one whole chain; clips without one follow a project default; "Apply to all clips" | canon 198, 205 |
| Color | The Color panel is a drawer above the timeline with the waveform inside it | canon 199, 203 |
| Color | Presets are starting points: an edited chain becomes "Custom" with Save as preset…; one list for both engines | canon 206, 207 |
| Color | **Video gamma**: shows a chain's SDR picture the way an unmanaged Rec.709 desktop does | canon 246 |
| HDR | No display mode to pick. The picture uses the screen's HDR headroom when it has some; an amber pill says when highlights clip, and the Highlight Knee brings them in | canon 179, 220, 221, 222 |
| Windows | 10-bit output on SDR monitors (no banding), switching as the window moves between monitors | canon 220 |
| Windows | DirectX 12 renderer; a clear message when there is no usable graphics device | canon 250; `src/fatal.rs` |
| Windows | Title bar, menu row and rails are one dark surface | canon 249 |
| Scopes | Waveform only; the vectorscope is retired. In dual view A and B overlay in cyan and orange | canon 203, 209 |
| Dual view | Chips, modes, the seam / gain slider and Save sit on the status strip; Save asks for a name; hover thumbnails come as a pair | canon 173, 170 |
| Dual view | Sides reuse the single-view decoders: gapless, sequences keep their own rate, changing B keeps the playhead and A's edits | canon 211 |
| Screenshots | Named after the media; PNG 16-bit joins PNG, JPEG and TIFF 16-bit; alpha is kept when the media has it; pixel aspect and rotation are applied; optional safety guides | canon 216, 223, 224 |
| Thumbnails | Every thumbnail goes through the clip's colour chain and Range override; the hover thumbnail follows the pointer | canon 216, 219, 170 |
| Notes | A drawer in the Color panel's slot; HTML export is a synopsis table with a lightbox; edits from minNotes are merged while the panel is open | canon 215, 217, 219 |
| Annotations | Fixes: undo across clips, redo on a new note, clean thumbnails, selection handles in screenshots, hit testing, stroke width on Windows | canon 215 |
| Safety guides | The chosen guide, opacity and width persist; guides and strokes keep their on-screen proportion in screenshots and outputs | canon 215, 238 |
| Image sequences | DPX, TGA and BMP sequences | canon 190, 191 |
| Inspector | Linked Projects with Open / Copy / Reveal (greyed when the path is not on this machine); no ExifTool; pixel aspect and rotation pickers | canon 193, 134, 177 |
| Live feeds | Review freezes the full float frame; a host that is not at 48 kHz plays at the right speed; restart counts and status lines corrected; Listen is one speaker toggle on the live row | canon 228, 231, 233 |
| Streams | Open Stream hides the passphrase everywhere; a listener URL you open listens without the Blender switch; paste works in every field | canon 234, 243 |
| Blender | A Blender session's stream is shown on QCView's own terms; the host's colour setup no longer changes the clip's chain | canon 236 |
| Output | Cards with a status dot and a switch; the feed starts muted; timecode and paused state are stamped | canon 237 |
| Removed | Stream output (SRT / RTMP): NDI covers it | canon 244 |
| Removed | Minimal Mode, Show All Panels, the Status Bar toggle, View > Timeline | canon 245, 219 |
| Shortcuts | The card has a filter; `Ctrl/Cmd + N` New Project | canon 218; `src/main.rs:2340` |
| Fullscreen | The transport appears when the pointer rests at the bottom edge | canon 171 |
| Window | Dragging the picture moves the window in Compact Mode too | canon 251 |
| Languages | The same 11 languages; the "machine translation" label is gone; 145 new strings per language are drafts | canon 195-197; handoff "Next" item 3 |
| Licences | About > Licenses… opens the full third-party notices, which now ship in both packages | canon 252 |
| Under the hood | FFmpeg 9.0.2; half-float working image; OCIO as native shaders | canon 187, 200, 202 |

Two candidates for a "known gaps" line, if the owner wants one: live sources in dual view, and Open in minNotes.

---

## 6. Size estimate

| Level | Pages | Count |
|---|---|---|
| Accurate as is | llm_transparency | 1 |
| Small edits (a few sentences or table rows each) | index, installation, annotations, image, settings, playlists, thumbnails, privacy, qcbridge, qcbridge-adobe, qcbridge-aja, qcbridge-ndi | 12 |
| Small edits plus a section to delete or rewrite | manager (Clip colour), output (Stream half), qcbridge-blender (colour paragraphs), timeline (lower half) | 4 |
| Substantial rewrite | overview, basics, inspector, color, mincolor, dualview, hdr | 7 |

Text: roughly 800 to 900 of the 1,520 non-changelog lines need touching. The seven rewrites are the bulk; color + mincolor + hdr are really one job (the colour model changed) and are best planned together, with the Scopes section moving from inspector to color.

Screenshots: 40 QCView screenshots are referenced (`qcv001`-`qcv044` less four unreferenced) and all are stale, because the whole interface is redrawn. Of the 14 referenced `qcbridge_*` images, 4 or 5 show QCView (`007`, `008`, `016` as used in output / NDI, `017`, `018`, possibly `005`) and are stale; the rest show After Effects or the Blender agent and are probably fine. Four images describe UI that no longer exists at all and have no like-for-like replacement: `qcv015` (overview bar), `qcv032` (display mode list), `qcv033` (edit mode), `qcv040` (badges and bulk Input picker). Unreferenced files that could be removed: `qcbridge_002`-`004`, `qcv023`, `qcv027`-`029`.

Already broken in 2.x, cheap to fix in the same pass: links to `/basics/#layout-presets` (permalink is `/app-basics/`), `/qcbridge-blender/` (is `/qcbridge/blender/`), `qcbridge_016.png` used for two different things, `nav_order: 12` on two pages, a stray bullet at `hdr.md:65`.

Questions the owner has to answer before writing starts:
1. Live sources in dual view, Prev / Next clip, the hover-thumbnail button, Open in minNotes: ship as gaps, or wait?
2. Scene LUT / CDL and Export LUT: confirmed dropped, or owed? No ruling names them.
3. Do the 2.x pages stay reachable (a `2.x` branch is planned for the code; nothing says what happens to its manual)?
4. Does the AJA page keep its tested-hardware claims before the hardware walk?
