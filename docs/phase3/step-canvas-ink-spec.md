# minNotes canvases and ink — what the Qt app did (Phase 3, canvas / ink step)

Source read: the minNotes repository at commit `59d1129` (branch `min-paths`), read only. Nothing in it was built or
run; every statement is read from code or from the app's own manual, and anything deduced rather than read is marked
**(inference)**. "Not found" means the code was searched and has no such thing.
References are `alias:line` relative to the minNotes repository.
`CD` = `app/core/CanvasDocument.cpp`, `CDh` = `app/core/CanvasDocument.h`, `Doc` = `app/core/Document.cpp`,
`Doch` = `app/core/Document.h`, `Cam` = `app/core/CanvasCameraMath.h`, `CamC` = `app/notes/CanvasCamera.cpp`,
`CamH` = `app/notes/CanvasCamera.h`, `CL` = `app/core/CanvasLayout.h`, `CEx` = `app/core/CanvasExporter.h`,
`ExC` = `app/core/CanvasExporter.cpp`, `Cnv` = `app/core/Converter.h`, `CnvC` = `app/core/Converter.cpp`,
`MS` = `app/core/MediaStore.cpp`, `BM` = `app/core/BlockModel.cpp`, `Cap` = `app/notes/CanvasInkCapture.cpp`,
`CapH` = `app/notes/CanvasInkCapture.h`, `Ink` = `app/notes/InkNodeItem.cpp`, `InkH` = `app/notes/InkNodeItem.h`,
`Tes` = `app/notes/ink_tessellate.cpp`, `TesH` = `app/notes/ink_tessellate.h`, `AS` = `app/notes/active_stroke.h`,
`Ser` = `app/notes/annotation_serializer.cpp`, `Mod` = `app/notes/ink_stroke_modeler_wrapper.h`,
`ModC` = `app/notes/ink_stroke_modeler_wrapper.cpp`, `MH` = `app/notes/MediaHost.cpp`, `MHh` = `app/notes/MediaHost.h`.
QML, all under `app/qml/MinNotes/App/`: `CV` = `CanvasView.qml`, `CE` = `CanvasElement.qml`,
`SF` = `CanvasSelectionFrame.qml`, `CP` = `CanvasPage.qml`, `CM` = `CanvasMediaItem.qml`, `CF` = `CanvasFrameItem.qml`,
`CS` = `CanvasShapeItem.qml`, `CI` = `CanvasInkItem.qml`, `CTE` = `CanvasTextEditor.qml`,
`Ctx` = `CanvasContextMenu.qml`, `CT` = `CanvasToolbar.qml`, `PF` = `PenFlyout.qml`, `LP` = `LayerPanel.qml`,
`FP` = `FramePanel.qml`, `In` = `Inspector.qml`, `NH` = `NavHistory.qml`, `LB` = `Lightbox.qml`,
`SW` = `SmoothWheel.qml`, `Ed` = `Editor.qml`, `Mn` = `Main.qml`, `ChT` = `ChromeToolbar.qml`, `MB` = `MediaBlock.qml`,
`PB` = `PathBar.qml`, `ZB` = `ZoomBadge.qml`. Manual: `ManC` = `docs/canvas.md`, `ManD` = `docs/annotations.md`,
`Sh` = `docs/shortcuts.md`.

The new side (minplatform at `e51923a`): `st` = `crates/mn-store/src/file.rs`, `old` = `crates/mn-store/src/old.rs`,
`sb` = `apps/mn/src/sidebar.rs`, `mn` = `apps/mn/src/main.rs`, `mda` = `apps/mn/src/media.rs`,
`srch` = `apps/mn/src/search.rs`, `col` = `apps/mn/src/colours.rs`, `qs` = `crates/qcv-notes/src/strokes.rs`,
`qi` = `crates/qcv-notes/src/ink.rs`, `qa` = `crates/qcv-annotate/src/lib.rs`, `mp` = `crates/min-present/src/lib.rs`,
`eng` = `crates/min-present/src/engine.rs`, `canon` = `docs/rust-wgpu-port.md`.

Sibling specs this one leans on and does not repeat: `step1-core-spec.md` (the file, blocks, the format gate),
`step-media-spec.md` (the media descriptor, `{vol, rel}` references, a missing file), `step-video-spec.md` (the studio,
the notes sidecar), `step-find-search-spec.md` (search over canvas cards).

Vocabulary of the old code: a **file** is one `.mnd`; its root is the **canvas**; an **element** is one row on the
canvas; a **page** is an element that owns blocks; a **card** is how a page or a media element shows on the canvas;
a **frame** is an element of kind `group` (a **group** when elastic, a **board** when fixed); a **chip** is a text
shape; the **page view** is one page full-window; a **full-frame tab** is one PDF or video full-window; **world** is
canvas units, **screen** is window pixels.

---

## 0. The shape everything below rests on

| Item | Rule | Ref |
|---|---|---|
| The canvas is the file | One canvas per file, and it is the root: every page is an element on it. There is no file without a canvas and no second canvas | `CDh:18-24`, `ManC:9` |
| At least one page | Every open ends with at least one page element; removing the last page is refused (also on undo) | `CD:165-182`, `CD:570-573`, `CD:1894` |
| Two surfaces, one window | The canvas view sits over the one editor in the same rectangle; "the canvas is up" = no page focused and no full-frame tab | `Mn:1635-1660`, `Ed:469-477` |
| Elements are a flat list | One table, one z-ordered in-memory mirror reloaded after every write | `CD:676-681`, `CDh:141-142` |
| Frames do not nest | One level of parent only; children keep absolute world coordinates | `CDh:261-267`, `CD:1750`, `CD:1787` |
| Two undo logs | The canvas has its own log; each page has its own. Undo acts on the log of the surface in front | `CDh:44-47`, `Ed:497` |
| Page cards are read-only | A page is edited only in the page view. The in-place "live card" was built and removed (2026-10-22) | `CV:355-357`, `CV:453-456`, `CE:70-72` |
| Ink is elements | Every stroke on the canvas is its own element. Ink pinned to page text does not exist in 2.0 (§6.6) | `CV:732`, `Cnv:6-20`, `ManD:21-24` |

---

## 1. The data model

### 1.1 Tables

| Item | Rule | Ref |
|---|---|---|
| `elements` | `id TEXT PRIMARY KEY, kind TEXT NOT NULL, x REAL, y REAL, w REAL, h REAL` (each `NOT NULL DEFAULT 0`), `z INTEGER NOT NULL DEFAULT 0`, `parent_id TEXT REFERENCES elements(id) ON DELETE SET NULL`, `json TEXT NOT NULL DEFAULT '{}'`, `created INTEGER`, `modified INTEGER` | `Doc:431-443` |
| Index | `elements_z ON elements(z, created)` | `Doc:444` |
| Read order | `ORDER BY z, created, id` — this order IS the paint order and the "z order" everywhere | `Doc:471-478`, `CE:11-14` |
| `canvas_meta` | `key TEXT PRIMARY KEY, value TEXT` | `Doc:445` |
| `canvas_meta` keys written | `focusPage` (a page id, or empty = the canvas), `camera` (JSON `{"x","y","z"}`, written only once the user has taken the camera). The 1.0 converter also writes `defaultView` = `canvas`, `pageWidthDefault`, `convertedFrom` = `mnd1` | `CD:807-816`, `CD:325-331`, `CnvC:205-207` |
| A page's blocks | `blocks.page_id TEXT REFERENCES elements(id) ON DELETE CASCADE`; index `blocks_page_order ON blocks(page_id, rank, depth, type)`. Blocks with no page go to the first page on open | `Doc:447-449`, `CD:180` |
| Ids | ULIDs for elements | `CD:171`, `CD:961` |
| Kinds | `page`, `media`, `ink`, `shape`, `connector`, `group` | `Doch:124` |
| Geometry | `x, y, w, h` in world units, axis-aligned. **Rotation: not found.** Scale, skew, flip: not found | `Doc:431-443` |
| z | An integer per element. A new element takes `max(z) + 1`. Bring to front writes `top + 1`, send to back `bottom − 1` (so z can go negative and has gaps); a reorder re-packs every z to its index | `CD:509-513`, `CD:1413-1426`, `CD:1496-1515` |
| Timestamps | `created` / `modified` in ms; `modified` is stamped on every rect, json, z and parent write | `Doc:502`, `Doc:498-570` |
| Lock, hidden, opacity, name columns | **Not found** | `Doc:431-443` |
| Format | `doc_meta.format = 'mnd2'`, `schema_version = 2`. A newer schema is refused; a 1.0 file (`mnd`, v1) is adopted read-only and converted on the working copy | `Doch:67-70`, `CD:139-161` |
| Per-element version | **Not found**: `json` carries no version field of its own (an ink envelope carries `"version":"2.0"`, §6.3) | `CD:958-978` |

### 1.2 The `json` payload, by kind

| Kind | Keys | Ref |
|---|---|---|
| `page` | `fixedH` (bool: the height was set by hand), `view.scrollY` (the page view's remembered scroll). Nothing else is written. The title is **derived**, not stored: first heading, else the first text line (60 characters), else "Untitled page" | `CD:702-704`, `CD:1362-1365`, `CD:770-783`, `CD:584-608` |
| `media` | `media` = the page's media descriptor verbatim (see `step-media-spec.md`); `ink` = `{ "<page index>": envelope }` for ink drawn on a PDF page (§6.7) | `CD:980-984`, `CD:827-833`, `CD:1609-1620`, `CM:1-5` |
| `media.kind` values | `image` (also when the key is missing), `video`, `pdf`, `audio`, `folder`, `file`, `bookmark` | `CD:910-914`, `CM:56-57` |
| `media.src` | A string (a file under the file's `.minnotes/` folder, an absolute path, or an `http(s)` address) or a `{vol, rel}` portable reference (`rel` an array of path parts) | `CD:835-842`, `CD:921-924`, `CD:1096` |
| `media.preview` | For a rendered stand-in (PSD, HEIC, RAW, HDR): `{src, format, mtime, size}` of the original; the descriptor's own `src` is the rendered picture | `MS:197-206`, `CD:868-884` |
| Bookmark descriptor | `kind:"bookmark"`, `src` (the address), `host`, `title` (the host until fetched), `icon` (a stored picture, empty until fetched), `fetched` | `CD:1185-1191` |
| `ink` | An envelope: `version:"2.0"`, `coordinate_system:"element-local"`, `shapes:[one stroke]` (§6.3) | `Cap:154-160` |
| `shape` | `shape` (`"text"` is the only one made), `text`, `fontSize`, `fill` (a colour, or `"transparent"` / missing = plain text), `color` (plain text's colour), `fixedW` (bool: the width is pinned and the text wraps) | `CV:45-46`, `CV:86`, `CV:261`, `CS:16-21` |
| `group` | `title`, `tint` (`""` = none), `sizing` (`"elastic"` \| `"fixed"`), `layout` (`"free"`; `"grid"` is named and not built), `aspect` (height ÷ width, boards only) | `CD:1638-1646`, `CD:1757-1760`, `FP:18-26`, `CDh:261-263` |
| `connector` | Opaque JSON, zero rect. `addConnector` exists and **has no caller**; a connector would paint as a bordered box with the word "connector" | `CD:1339-1341`, `CE:128-134` |

### 1.3 Pages as elements

| Item | Rule | Ref |
|---|---|---|
| Width | The element's width = the page's text measure + 24 on each side. Measure clamped to 400 … 4000; default 760 | `CDh:59-61`, `CDh:292-293`, `CD:532`, `CD:1349` |
| Auto height | The laid-out content height + 48, cached into the element's `h` (no undo entry, not marked dirty). Never under 400; capped at a **square** (height ≤ measure) | `CD:757-768`, `CD:697-700`, `CD:731-733`, `CP:40-58` |
| Fixed height | Dragging a top or bottom handle sets `fixedH` and a height in 240 … 8192; the content cache then leaves it alone. "Fit height to content" removes it (one undo entry) | `CD:1348-1355`, `CDh:158-164`, `CD:709-729` |
| A new page | One empty paragraph. From the page tool or the menu: at the point. Otherwise: 64 to the right of the right-most page, at y = 0 | `CD:515-524`, `CD:539-568` |
| Removing a page | Its blocks go with it (cascade); the undo entry carries the raw block rows so undo restores ids, ranks and timestamps. The page's own edit history is lost | `CD:570-582`, `CDh:345-352` |
| Loading | A page's block model is created on first use and kept; a page at ghost zoom loads nothing (title from a cache) | `CD:610-618`, `CP:23-25`, `CD:590-607` |

### 1.4 Media elements and files

| Item | Rule | Ref |
|---|---|---|
| One store per file | Every element and every page resolves `src` through the file's one media store | `CDh:20-22`, `CD:843-854` |
| Reference crossing | A `{vol, rel}` value arriving from QML is unwrapped first (a bug fixed 2026-10-21: such references resolved to nothing on the canvas) | `CD:835-842` |
| Original vs loadable | A preview element has two paths: the rendered picture (what is drawn) and the original (what Reveal, Open, Copy path and search use) | `CD:855-874`, `CD:894-897` |
| Collect Media | Rewrites element `src` values directly, with no undo entry | `CD:651-671` |
| Descriptor patches | A folder rescan, a fetched bookmark title or icon, a downloaded remote image: written with no undo entry | `CD:1325-1337`, `CDh:216-218` |

---

## 2. Navigation

### 2.1 The camera

| Item | Rule | Ref |
|---|---|---|
| Transform | `screen = world × zoom + pan`. One uniform zoom, no rotation | `Cam:3`, `Cam:37-38`, `CV:535-538` |
| Zoom limits | 0.05 … 4.0 (5 % … 400 %) | `Cam:10-15` |
| Zoom anchor | The world point under the anchor stays under it: `pan' = a − (a − pan) × z'/z` | `Cam:17-25` |
| Fit | The union of all element rects, centred, with 48 px of margin, zoom clamped | `CV:439`, `Cam:27-35`, `CD:818-825` |
| "User camera" | Until the user pans or zooms, the canvas shows a fit and **re-fits** on every resize and every element change. Any user pan / zoom ends that | `CamH:2-5`, `CV:449-451`, `CV:511-515` |
| Persistence | 300 ms after a user camera change the camera is copied to the document; it reaches the file on the next save. Setting it does not mark the file dirty | `CV:517-518`, `CD:799-816` |
| On open | The file's camera when it has one, else fit. Each tab also keeps its camera for the session | `CV:442-448`, `Ed:625-643` |
| Animated camera | A 180 ms ease-out camera move with a callback, plus "readable camera" helpers (fit one element inside the view less its chrome; pages held to 100 – 200 %). No caller found inside `CanvasView.qml` **(inference: left from the removed in-place forms)** | `CV:459-496` |
| Minimap, overview, scroll bars, rulers, grid | **Not found** | `CV:1-771` |

### 2.2 Pan

| Gesture | Rule | Ref |
|---|---|---|
| Middle-button drag | Always pans — whatever the tool, the selection or the element under the pointer | `CV:744-751` |
| Space held + left drag | The hand: Space down sets it, Space up clears it. While held, drawing and element gestures are off | `CV:16`, `CV:22-28`, `CV:762-770`, `Ed:3444-3447` |
| Hand tool (`H`) | Left drag pans | `CV:27`, `CT:20` |
| Left drag on empty canvas, pointer tool | **Not a pan — a marquee** (§3.1). The file's own header comment still says "empty-space drag pans" | `CV:634-638`, `CV:19-20` |
| Option + left drag | Pans only while a text chip is being edited | `CV:752-761` |
| Trackpad two-finger scroll | An event with a scroll phase and pixel deltas pans directly by those deltas (the system supplies the momentum) | `SW:69-80`, `CamC:14` |
| Mouse wheel | Each notch adds a kick to a velocity that decays every frame: one notch travels 80 px in total, friction 0.90 per 60 Hz frame; a sustained burst multiplies the kick up to 6× by notch **rate** over a 300 ms window. A reversal cancels the tail. Shift + wheel pans sideways | `SW:32-36`, `SW:85-98`, `SW:101-117` |
| Remote desktop | A relayed wheel arrives as a touchpad device, as pixel deltas **without** a phase, one notch split into several fractional events (angle ±40 × 3, 0 ms apart). So: the wheel handler must accept touchpad devices, such events take the notch model, and the gain counts notch-equivalents (angle ÷ 120), not events | `SW:15-17`, `SW:57-60`, `SW:81-84`, `CV:558-560` |
| Arrow keys | **Not found** (every key not listed in §8.3 is swallowed on the canvas) | `Ed:3440-3462` |

### 2.3 Zoom

| Gesture | Rule | Ref |
|---|---|---|
| Ctrl / Cmd + wheel | One step per event, ×2^0.25 in or out, about the pointer. The step's size ignores the delta's size | `SW:65-68`, `CV:553-557` |
| Pinch | Zoom = the zoom at the pinch's start × the gesture's scale, about the centroid | `CV:562-567` |
| `⌘+` / `⌘=` / `⌘−` | ×√2 per press about the view's centre | `CV:440`, `Ed:3433-3434` |
| `⌘0` | 100 % about the centre | `Ed:240`, `Ed:3435` |
| `⌘1` | Fit everything | `Ed:239`, `Sh:32` |
| The zoom badge | The toolbar's percent readout; a click opens Fit, 100 % and a slider. The slider is logarithmic on each side of 100 %, with 100 % at its centre | `ZB:1-4`, `Ed:209-234`, `ChT:269-270` |
| If modifiers do not pass (remote desktop) | Wheel zoom needs the Ctrl modifier on the wheel event and pinch needs a pinch gesture; without them only the keys and the badge zoom **(inference)** | `SW:65` |

### 2.4 Opening things: the ladder

| Item | Rule | Ref |
|---|---|---|
| The ladder | Click = select. Double-click, Return, the open glyph or the menu's first row = open in full. No in-place form | `CE:70-85`, `CV:355-357` |
| Page | Opens the page view (`focusPage` = its id) | `CV:344` |
| Text chip | Opens its inline editor (double-click always edits) | `CV:345`, `CE:83` |
| Frame | Open = rename the title in place | `CV:346` |
| Video, PDF | Double-click / glyph / menu: its full-frame tab (the studio, the PDF view). **Return**: the lightbox instead | `CV:349`, `CV:505-510` |
| Image | The lightbox. The system's default app is not reachable from the ladder for an image | `CV:352` |
| Audio | Plays in place (Return: the lightbox) | `CV:350`, `CV:508` |
| Bookmark | The browser | `CV:351` |
| Folder, other file | The system's default app (a preview opens its original) | `CV:353`, `CD:895` |
| The open glyph | A 20 px square off the selection's top-right corner, for one selected page, chip or media element; its tooltip is the open label ("Open page", "Edit text", "Open in studio", "Open PDF", "Open link", "Open folder", "Play", "Open") | `SF:31-34`, `SF:91-102`, `CV:202-209` |
| Double-click on empty canvas | With the pointer tool: a new page there, opened at once | `CV:32-37`, `CV:611` |
| Back to the canvas | In the page view, Esc with nothing left to cancel returns to the canvas with that page selected. In a full-frame tab, Esc (after dropping the tool) returns with the element selected | `Ed:3331-3335`, `Ed:3286-3291` |
| The path bar | File › Canvas › Page › frame; the segments left of the current level navigate up | `PB:1-7` |

### 2.5 Back / forward

| Item | Rule | Ref |
|---|---|---|
| A location | (file tab, focused page or the canvas, open frame or none) | `NH:1-8`, `NH:21-26` |
| Rules | A browser's: a navigation truncates the forward tail and pushes; back / forward restore without pushing; consecutive duplicates collapse; a closed file's entries drop; a deleted page's entry is skipped. 100 entries, this session only | `NH:30-38`, `NH:46-60`, `NH:72-81` |
| Not a location | The camera, the selection, the lightbox | `NH:8`, `NH:21-26` |
| Controls | View ▸ Back / Forward (`⌥⌘←` / `⌥⌘→`), chevrons left of the path, the mouse's back / forward buttons. Tooltips name the destination | `Mn:1297-1298`, `Sh:34`, `NH:83-94` |

### 2.6 The lightbox

| Item | Rule | Ref |
|---|---|---|
| What | An overlay over the whole window for one image, video, PDF or audio item | `LB:1-11`, `LB:44` |
| Open | Space **tapped** on a selected media element (Space is also the hand: it counts as a tap only if the selection and the camera are unchanged at release); Return; a double-click on an image | `Ed:3262-3267`, `Ed:1255-1261`, `CV:352` |
| Bar | ‹ › step through the file's media elements in z order (wrapping), the name, a PDF's pager, × | `LB:4-5`, `LB:93-100` |
| Keys | Space / Esc close; ← / → step (on a video they step frames); PageUp / PageDown page a PDF; a video takes the transport keys. Nothing else types through | `LB:109-120` |
| Close | Pauses a playing video; the selection stays so Space re-opens | `LB:79-84` |

---

## 3. Selection and manipulation

### 3.1 Selecting

| Item | Rule | Ref |
|---|---|---|
| The selection | A set of element ids, per view, not stored. Hiding the canvas clears it; removed elements leave it | `CV:101-125`, `CV:451` |
| Click | Selects that element alone (a click on an already selected element changes nothing) | `CE:73-79` |
| Shift + click | Adds or removes that element | `CE:76`, `CV:109` |
| Click on empty canvas | Clears the selection (pointer tool) | `CV:601-610` |
| Marquee | Pointer tool, drag from empty canvas: a screen-space box; on release every element whose rect **intersects** it is selected. It replaces the selection (Shift does not add). Under 2 px it does nothing | `CV:634-666`, `CD:740-746` |
| Hit testing | By rectangle, not by shape: an ink stroke is hit anywhere in its bounding box. The top-most element is the last in z order | `CV:99`, `CV:620-623`, `CD:740-746` |
| A frame under its child | A tap over a frame's child belongs to the child | `CE:38-45`, `CE:74` |
| Right-click | Selects the element under the pointer if it was not selected (keeps a multi-selection that contains it); on empty canvas clears; then opens the menu | `CV:615-628` |
| Select all | **Not found** on the canvas | `Ed:3440-3462` |
| Press ownership | Decided at the press: over an element or the selection's chrome, the element's handlers own it; over empty canvas, the view's | `CV:520-531` |

### 3.2 Moving and resizing

| Item | Rule | Ref |
|---|---|---|
| Move | Drag any element: if it was not selected it becomes the selection; the whole selection moves; a selected frame carries its children. One history row, "Move" | `CE:87-106`, `CV:137-143`, `CV:158-172` |
| Delta | The screen delta ÷ zoom, applied to each element's rect as it was at the drag's start | `CE:101-105`, `CV:160` |
| Snapping, alignment guides, grid | **Not found** | `CV:126-190` |
| Nudge with arrow keys | **Not found** | `Ed:3440-3462` |
| Constrain to an axis (Shift) | **Not found** | `CE:87-106` |
| Handles | Eight 8 px squares, drawn in screen space. Shown for exactly **one** selected element; a multi-selection has a frame and no handles | `SF:104-114`, `SF:138-140` |
| Minimum size | 8 world px on each side | `CV:176-181` |
| Aspect lock | Only for a video and for a board with an aspect. **Images are not locked** | `SF:132`, `CV:182-188` |
| Page | Width handles reflow the text (measure clamp, §1.3); top / bottom handles pin a height; a double-click on a top / bottom handle = fit height | `SF:3-6`, `SF:115-119`, `CD:1348-1355` |
| Text chip | Side and corner handles set the **width** only (minimum 24) and pin it; no top / bottom handles; a double-click on a handle returns to auto width | `CV:80-97`, `SF:109`, `SF:115-119` |
| Elastic frame | No handles | `SF:17`, `SF:109` |
| No takeover | A handle drag cannot be taken over by a larger element behind it | `SF:120-124` |

### 3.3 Commands

| Command | Rule | Ref |
|---|---|---|
| Delete | Delete / Backspace or the menu: one row "Delete". The last page refuses; a frame takes its children | `Ed:3452`, `CV:191-197`, `CD:1585-1607` |
| Duplicate | Menu only (no key found). A copy at +24, +24, on top; the copies become the selection. **Pages are not duplicated**; a frame duplicates with its children; a child whose frame is also selected is not duplicated twice | `CV:358-367`, `CD:1568-1583`, `Ctx:87` |
| Copy / cut of elements | **Not found.** `⌘C` on the canvas reaches the page's copy routine **(inference: it acts on the hidden page's selection)** | `Ed:3408-3410`, `Ed:3440-3462` |
| Paste (`⌘V`) | At the view's centre. Files on the clipboard → elements (several: a grid); else a picture → an image element; else HTML that is one remote picture → a remote image; else text → §5.5. The result joins a frame under the point and becomes the selection | `CV:315-335`, `Ed:3453` |
| Bring to front / send to back | `⌥⌘]` / `⌥⌘[` or the menu; a frame goes with its children | `Ed:3395-3399`, `CV:378-379` |
| Step one layer | `⌘]` / `⌘[`: over one neighbour — but a selection that is not all ink steps over a whole **run** of ink at once | `CV:376`, `CD:1538-1566` |
| Lock, hide, align, distribute, rotate, flip | **Not found** | `Ctx:52-93` |

### 3.4 Undo

| Item | Rule | Ref |
|---|---|---|
| One entry | An element's state before and after (rect, json, z, parent; a page's raw blocks on add / remove) | `CDh:345-361`, `CD:1869-1877` |
| Coalescing | A run with the same key on the same element folds into the current entry: `move:<id>`, `resize:<id>`, `text:<id>`, `hug:<frame>`, `inkw:<id>`, `inkc:<id>` | `CD:1961-1972`, `CV:133`, `CV:175`, `CTE:48`, `CD:1372`, `CV:374-375` |
| Batches | Everything pushed between begin and end is one row (a multi-element drag, a delete, a drop of several files, a group). A batch of one part is that part; a batch can be relabelled ("Add to frame") or discarded (a chip added and removed empty leaves no row) | `CD:1917-1949`, `CV:167-168`, `CTE:50-60` |
| Tree | Entries point at a parent; redo follows the newest child, so an undone branch stays reachable by jumping | `CD:1973-1984`, `CD:2004-2016` |
| Labels | "Add page", "Remove page", "Add media", "Draw", "Add shape", "Move / Resize / Edit / Reorder / Duplicate / Remove <kind>", "Edit ink", "Group", "Ungroup", "Add to frame", "Remove from frame", "Move frame", "Pen width", "Fit page height", "Drop N files", "Copy in <file>" | `CD:564`, `CD:939-947`, `CD:983-989`, `CD:1361`, `CD:1384`, `CD:1693`, `CD:1717`, `CD:1847` |
| Not undoable | The page height cache, a page's scroll, the camera, descriptor patches, Collect Media | `CD:757-783`, `CD:1325-1337`, `CD:651-671` |
| Lifetime | In memory only; cleared on open and close. An undo in the middle of a gesture drops the gesture's open batch | `CD:188`, `CD:2018-2025`, `CD:1994` |

---

## 4. Frames: groups and boards

| Item | Rule | Ref |
|---|---|---|
| One kind | A frame is an element of kind `group`. `sizing:"elastic"` = a **group**; `sizing:"fixed"` = a **board** | `CDh:261-264`, `CD:1662-1665` |
| Constants | Padding 24, title strip 28 high, minimum 160 × 120 | `CDh:283` |
| The frame tool (`F`) | A click = an empty elastic frame 320 × 240. A drag over 8 px = a board of that size | `CV:608`, `CV:670-691` |
| `⌘G` | No frame in the selection: a group around it. Exactly one frame and other elements: those join that frame. Otherwise nothing | `CV:383-391`, `Ed:3451` |
| A new group | Rect = the union of the members + 24 all round + 28 above; the members leave any frame they were in; the frame is selected. Titles count up: "Group", "Group 2", … ("Frame", "Frame 2", … from the tool) | `CD:1745-1767`, `CD:1626-1636`, `CD:1640` |
| `⌘⇧G` | One frame selected: the frame is removed, the children stay and become the selection | `CV:385`, `CV:392`, `CD:1769-1777` |
| No nesting | A frame among the members is skipped; a frame never gets a parent | `CD:1750`, `CD:1787`, `CV:148-152` |
| z | A frame sits directly under its children and the block is contiguous; joining re-packs it | `CD:1726-1743`, `CD:1791` |
| Elastic hug | After every child move the frame's rect is recomputed (union + padding + title), in the same history row. An empty elastic frame keeps its rect. Switching a board to a group hugs at once | `CD:1372`, `CD:1702-1724`, `CD:1677` |
| A plain drag never changes membership | Dragging a child outward stretches its group | `CV:144-146`, `ManC:46-47` |
| `⌘`-drag (Ctrl on Windows) | Membership by drag: while held, the group does not stretch and the frame under the pointer shows a 2 px accent border; at the release the dragged non-frame elements join that frame, or leave theirs when released over none. The row is relabelled "Add to frame" / "Remove from frame" | `CV:144-172`, `CE:97-99`, `CF:30` |
| Drops and pastes | A file dropped or pasted inside a frame joins the top-most frame containing the point | `CV:173-174`, `CV:333`, `CV:582`, `CD:1681-1685` |
| The menu | "Group" (two or more, no frame), "Add to frame", "Ungroup", "Remove from frame" (every selected element has a parent), "Rename frame" | `Ctx:60-64`, `CV:383-386` |
| Boards clip | A child of a board is clipped to the board's rect (its item is the visible part; fully outside = invisible). A group never clips | `CE:23-36` |
| Board aspect | 16:9, 1:1, 9:16, 4:3: sets `sizing:"fixed"`, stores the ratio, keeps the width and sets the height (at least 120); the handles then keep the ratio | `FP:18-28`, `SF:18`, `SF:132` |
| With its children | A frame moves, deletes, duplicates and z-steps with its children | `CV:141`, `CD:1589-1598`, `CD:1571-1581`, `CV:376-379` |
| Title | Double-click or Return on the frame, or the menu: edited in the strip; Return commits, Esc cancels, a click elsewhere commits; an empty title is ignored | `CV:346`, `CF:63-76` |
| Look | A rounded rect filled with the tint at 10 % (untinted: a neutral grey at 18 %), border at 50 % (90 %); a title strip at 12 % (22 %); the title bold, in the tint | `CF:26-46` |
| Tints | None, and six fixed colours | `FP:17`, `FP:62-75` |
| Grid layout | **Not built** (`layout` is always `"free"`). A grid was built and removed, per local notes | `CD:1642`, `FP:1-3` |
| Search | A frame's title is not searched | `step-find-search-spec.md` §1.3 |

---

## 5. Element kinds in detail

### 5.1 Page cards

| Item | Rule | Ref |
|---|---|---|
| What is drawn | The page's real block views from its own model, at the card's measure, read-only | `CP:1-6`, `CP:118-130` |
| LOD tiers | Ghost below zoom 0.10: the frame and the title. Greek 0.10 – 0.40: the title and one bar per block from the model's row geometry. Live from 0.40: the real blocks. Hysteresis: live holds down to 0.32, greek down to 0.08 | `CP:8-13`, `CP:66-76` |
| Greek bars | One per block, as wide as about 7.2 px per character (capped at the lane), toned by block type; at most 400 | `CP:131-148`, `CP:110` |
| Live rows | At most 120 block views per card, created 12 per frame shared across all cards that need rows | `CP:109-117`, `CV:419-432` |
| Overflow | Rows below the card's height are clipped under a 72 px fade with a link "N more blocks — open the page" | `CP:60-65`, `CP:153-172` |
| Spelling | Off on cards | `CP:88-94` |
| Look | Surface colour, 1 px border, the small radius, 24 inset | `CP:26-32` |

### 5.2 Text chips

| Item | Rule | Ref |
|---|---|---|
| Two tools | Text box (`T`): filled with the pen colour, glyphs black or white by the fill's luminance (above 0.6 = black). Plain text (`⇧T`): no fill, the text colour | `CV:39-54`, `CS:17-20` |
| Make | A click on empty canvas with a text tool makes a chip and opens its editor | `CV:606` |
| Size | Font = the document's body size by default. Padding = 0.3 × font size. Auto width: the box fits the text, no wrap. Pinned width: the text wraps and the height follows. Height is never set by hand | `CV:56-73` |
| Editing | A text field in screen space over the element, glyphs × zoom (at least 4 px); the box follows the typing | `CTE:1-7`, `CTE:44-49`, `CTE:71-79` |
| Commit | Esc, `⌘Return` or a click elsewhere. Return alone adds a line. An empty chip is removed | `CTE:51-67`, `CTE:80-84` |
| History | One row per edit session; a new chip that ends empty leaves none | `CTE:6-7`, `CTE:57-60` |
| Formatting inside a chip | **Not found**: plain text, one size, one colour | `CS:28-34` |
| Font size control | `drawTextSize` is read from the Inspector, but no file under `app/qml` declares it (searched) — so a new chip always takes the body size | `CV:42`, `In:28-54` |

### 5.3 Media cards

| Item | Rule | Ref |
|---|---|---|
| Rendering | The page's media block component, fed by a stand-in for one row, sized to the element | `CM:1-48`, `CM:72-86` |
| Drop size | 480 wide at the media's aspect (4:3 when unknown); an image or preview smaller than 480 keeps its width | `CD:996-1026`, `CDh:77` |
| Image | The picture. No aspect lock on resize (§3.2) | `CD:1020-1026` |
| Preview formats | PSD, HEIC, RAW, HDR: a rendered picture with a format pill at its top right; the pill reads "<format> · changed" when the original is newer. "Refresh preview" re-renders (one history row) | `CD:1013-1019`, `MB:34-37`, `MB:192-203`, `CD:885-893`, `Ctx:68` |
| PDF | Shows one page. Selected: a pager "‹ n / N ›" under the card, and PageUp / PageDown. The shown page is kept per file path, so the same PDF shows the same page everywhere | `SF:57-90`, `CV:210-215`, `CV:249-250`, `Ed:3454-3455` |
| Video | A poster at the remembered playhead. Selected: a play button and scrub line under the card, and the keyboard is the transport (the tool letters wait). It plays inside the card on the one shared player (one thing plays at a time) | `CM:53`, `SF:40-56`, `CM:69-95`, `Ed:3438-3439`, `ManC:110-115` |
| "Notes page below" | A video's notes become a new page under the card | `CV:223-248`, `Ctx:74` |
| Audio | A 360 × 56 card: name, duration, path, play / pause | `CDh:72`, `CD:1009-1012` |
| Folder | A 320 × 112 card: name, count, first entries, path. "Spread files here" lays its first 48 entries out as elements below it; "Refresh"; "Open as stream" when it holds a notes file | `CDh:73-76`, `CD:1147-1166`, `Ctx:70-73` |
| Other file | A 320 × 56 card: name and full path | `CDh:74`, `CD:1037-1038` |
| Path actions | Reveal in Finder / Show in Explorer, Open in ufb, Copy path — on the original | `Ctx:65-67`, `CD:894-897` |
| Missing file | Handled by the media block (see `step-media-spec.md`) | `MB:217` |

### 5.4 Links

| Item | Rule | Ref |
|---|---|---|
| Bookmark card | 320 × 72: favicon, title, address. The title and icon are fetched once in the background (8 s timeout, the first 256 KB of the page; icon up to 512 KB) and stored | `CDh:71`, `CD:1171-1177`, `CD:1221-1257` |
| "Add link…" | On the empty-canvas menu: a one-field prompt at the point; `www.` gets `https://`; anything that is not an `http(s)` address is refused | `Ctx:85`, `CV:266-286`, `CV:287-314` |
| A dropped or pasted address | An address ending in a picture extension becomes a remote image (a 480 × 300 placeholder until downloaded into the file's store); any other becomes a bookmark | `CV:251-255`, `CD:1258-1296` |
| "Copy link" | On a bookmark's menu | `Ctx:69` |
| Links between elements, connectors, arrows that attach | **Not found** (the arrow tool draws a free arrow, §6) | `CD:1339-1341` |

### 5.5 Drops and pastes

| Dropped | Becomes | Ref |
|---|---|---|
| One file | An element at the point, by type, tried in order: folder, video, PDF, audio, preview format, image, another minNotes file, an importable document, else a file card | `CD:991-1039` |
| Several files | A reading-order grid from the point: columns = clamp(ceil(√(N ÷ 2)), 1, 4), gap 24, each row as tall as its tallest card. One history row | `CD:1040-1080`, `CL:11-15`, `CDh:75` |
| An importable document | A new page at the point holding the import | `CD:1031-1035`, `Mn:1027-1038` |
| Another minNotes file or package | Its pages and other elements are **copied in**, keeping their layout; no link | `CD:1081-1146` |
| Text | One line that is an address: §5.4. Otherwise a filled text chip, pinned at 480 wide when longer | `CV:256-265` |
| Hover feedback during a drag from outside | **Not found** | `CV:570-594` |

---

## 6. Ink

### 6.1 Tools

| Item | Rule | Ref |
|---|---|---|
| Tools | Pen (`P`, freehand), Rectangle (`R`), Oval (`O`), Line (`L`), Arrow (`A`), Eraser (`E`) | `CV:22`, `CT:25-30` |
| Highlighter, lasso, fill, pixel eraser, shape recognition | **Not found** | `Cap:16-23` |
| The tool is the mode | There is no pen on / off switch. The armed tool (`drawTool`) decides what a press does; `type` / `select` are the pointer. The tool is never persisted — the app starts with the pointer | `In:35-45`, `CV:19-28` |
| The capture layer | While a draw tool is armed, a full-view overlay takes the left button; the pointer shows a cross | `CV:724-731`, `Cap:40-46`, `CV:219-222` |
| Space while drawing | The hand: the overlay steps aside | `CV:22`, `CV:727` |
| Esc | Cancels the stroke in progress; then drops the tool; then clears the selection | `Ed:3456-3460` |
| Changing tool mid-stroke | Cancels the stroke | `Cap:34` |
| On view changes | Arriving at a page view drops to typing; a PDF / video tab keeps the tool | `Ed:555-575` |

### 6.2 Pen settings

| Item | Rule | Ref |
|---|---|---|
| Colour | One pen colour, default `#FF0000`, kept in settings. Alpha is part of the colour | `In:41`, `In:47-54` |
| Width | 1 … 64, default 6, kept in settings. The slider spends 70 % of its travel on 1 – 20 | `In:46`, `PF:23-27`, `CD:1823` |
| Width units | World units on the canvas: a stroke keeps its width relative to the content at every zoom | `CapH:28`, `Cap:145`, `ManD:17-19` |
| Pressure, tilt, velocity width | **Not found.** The smoother is given pressure −1 (unknown); a stroke has one width | `ModC:75`, `ModC:113`, `AS:47-48` |
| Smoothing | Freehand runs through Google's stroke modeler with fixed settings: wobble 0.04 s / 31 / 620, spring-mass 11 ÷ 32400, drag 72, prediction off. No user control | `Mod:34-47`, `Cap:73-83` |
| Smoothing space | The modeler is fed positions normalised over the **view** (screen), then mapped to world — so the smoothing strength depends on the zoom **(inference)** | `Cap:78-80`, `Cap:98-100` |
| Fill | A stroke has a `filled` flag (rect / oval); nothing on the canvas sets it **(inference)** | `AS:49`, `Cap:142-147` |
| Restyle after the fact | With ink selected, the width slider and the pen colour restyle the selected strokes live; one history row per drag. A width change grows the rect by half the difference and shifts the points | `CV:368-375`, `PF:45-54`, `CD:1820-1864` |

### 6.3 A stroke as stored

| Item | Rule | Ref |
|---|---|---|
| One element per stroke | Each committed stroke or shape is one `ink` element holding an envelope with one shape | `Cap:154-160`, `CV:732`, `CD:985-987` |
| Envelope | `{"version":"2.0","coordinate_system":"element-local","shapes":[…]}` | `Cap:154-157` |
| Shape | `id` (`stroke-<ms>-<1000..9999>`), `type` (`freehand` \| `rect` \| `oval` \| `arrow` \| `line`), `color` `[r,g,b,a]` floats 0 … 1, `stroke_width`, `filled`, `is_modeled`, `points` `[[x,y],…]` | `Ser:12-39`, `Ser:170-198` |
| Points | Freehand: the smoothed polyline. Rect: four corners. Oval: centre, then radii (a size, not a position). Line / arrow: start, end | `Cap:172-178` |
| The rect | The points' bounding box grown by `width ÷ 2 + 1` on every side; points are stored relative to its top-left | `Cap:132-153` |
| Per-point pressure, time | **Not found** in the stored shape (timestamps exist only in memory and are written as 0) | `AS:44-45`, `Cap:152` |
| Runs | Not stored. The Layers panel derives a "run" each time: consecutive ink elements in z order with the same parent | `CD:1434-1443`, `CDh:245-248` |
| After a stroke | The new element becomes the selection | `CV:732` |

### 6.4 Erasing

| Item | Rule | Ref |
|---|---|---|
| On the canvas | The eraser acts on the **press only**: the top-most ink element whose **rect** contains the point is removed (one history row). Dragging removes nothing further | `Cap:62-67`, `Cap:91-92`, `CV:395-400`, `CV:733` |
| On a PDF page | The press removes the top-most stroke whose drawn line is within `max(6, width ÷ 2 + 4)` px | `MH:213-230` |
| Partial erase | **Not found** | `Cap:62-67` |
| Select and delete | Ink elements are selected, moved, duplicated, reordered and deleted like any element | `ManC:140-141` |

### 6.5 Drawing ink on screen

| Item | Rule | Ref |
|---|---|---|
| Geometry | Every stroke is a union of capsules (a segment with round caps): one quad per segment carrying the segment's ends and half width; the fragment shader evaluates the distance and feathers one screen pixel | `TesH:2-7`, `Tes:26-41` |
| Vertex | 32 bytes: corner, segment a, segment b, half width, colour (straight sRGB bytes) | `TesH:15-20`, `Ink:50-61` |
| Shapes | Rect: four capsules. Oval: 64 segments. Arrow: a shaft and a solid 25° head sized `4.5 × scale + 2.3 × width`. Filled shapes: solid triangles with a 1 px outline. Width never under 1 px | `Tes:57-126` |
| One node per item | Each ink item is one geometry node with batching off; on the canvas each stroke is its own item **(inference: one draw call per stroke)** | `Ink:26-30`, `CI:1-17`, `CE:121` |
| Re-tessellation | Only when the envelope, the frame or the feather margin changes; the margin is quantised to powers of two so a pan or a small zoom rebuilds nothing | `Ink:100-131` |
| The stroke in progress | A second ink item over the whole view, fed a live envelope normalised over the view with the width × zoom | `Cap:180-197`, `CV:734-739` |
| Coordinate conversion | The loader accepts only `coordinate_system:"normalized"`, so an element-local envelope is divided by the element's size on every rebuild | `Ink:77-98`, `Ser:83-86` |

### 6.6 Ink on pages (text)

| Item | Rule | Ref |
|---|---|---|
| In 2.0 | **None.** The page's ink mode was removed in 2.0 stage 1; the `block_ink` table is no longer created | `Doc:132-134`, `In:14-15`, `ManD:21-24` |
| What a 1.0 file's page ink becomes | Ink pinned to a text block → one `ink` element per anchor on the canvas beside the text, at the block's **estimated** y (heights were never stored); its text chips → `shape` elements. Ink on a media block → that block's descriptor as page `0`. A sketch block → an `ink` element the size of the sketch in a column right of the page, its texts → chips, its pictures → media elements. Rows with no usable anchor are dropped and counted | `Cnv:6-20`, `Cnv:24-34`, `CnvC:122-195` |
| The 1.0 anchor rule (as the converter reads it) | A point = (Δx from the **page's centre**, Δy from the block's top), in page pixels | `CnvC:185-186` |
| Edge-affinity migration on a page-width change | **Not found** in this code (it was a 1.x behaviour; the 2.0 converter does not use it) | `CnvC:185-191` |

### 6.7 Ink on PDF pages, images and video frames

| Item | Rule | Ref |
|---|---|---|
| Where | In the PDF's full-frame tab: the same capture component over each page at zoom 1, with the same tools | `Ed:7193-7213`, `ManD:32-38` |
| Storage | In the PDF's own descriptor: `ink["<page index>"]` = an envelope with `coordinate_system:"normalized"` (points as fractions of the page), stroke widths in the PDF's source pixels. For a PDF block in a page that is the block's attributes; for a canvas element, the element's `json.ink` | `MH:178-204`, `CD:1609-1620`, `BM:4625-4651` |
| An envelope may also hold | `texts` (text chips on a PDF page); a page with no shapes and no texts loses its key | `BM:4632-4651`, `MH:227-229` |
| Shown | In the PDF tab, on the block's inline preview and on the canvas card, for the page shown | `Ed:7183-7191`, `MB:164`, `CM:45` |
| Undo | One entry per stroke on the owner's log (the page's, or the canvas's for an element) | `MHh:5-7`, `CD:1619` |
| Space | Parks the pen so a drag scrolls | `Ed:438` |
| vs canvas ink over a PDF card | A different thing: canvas ink floats above the card and does not follow the PDF's pages | `ManD:40-41` |
| Images | Ink stored under page `0` of an image's descriptor is drawn (it comes from converted 1.0 files). A way to draw on an image in 2.0: **not found (inference)** | `MB:212`, `BM:2577-2602` |
| Video frames | The studio's annotator, stored in the notes sidecar — see `step-video-spec.md` | `ManD:45-47` |

### 6.8 Input devices

| Item | Rule | Ref |
|---|---|---|
| What the capture reads | Mouse press / move / release with the left button. Nothing else | `Cap:53-60` |
| Tablet events, pen pressure, eraser end, barrel button | **Not found** | `Cap:53-60` |
| Touch, palm rejection | **Not found** | `CV:552-567` |

---

## 7. Layers, Inspector, History

| Item | Rule | Ref |
|---|---|---|
| The Inspector | A 248 px panel on the right, closed by default, opened from the toolbar; the open state is remembered | `In:5-10`, `In:17-26`, `In:56-60` |
| On the canvas | Top to bottom: the Frame section (only with exactly one frame selected), Layers (60 % of the remaining height), History | `In:83-127`, `FP:1-3` |
| In a page | History alone | `In:80-82` |
| Layers rows | The z order, front-most first. A row: an icon by kind, a name (a page's title, a media file's name, a chip's first line, "Rectangle" / "Oval" / "Line" / "Arrow", "Connector", a frame's title) | `CD:1428-1479`, `LP:29-45` |
| Ink runs | Consecutive ink elements with the same parent are ONE row "Ink" with a count; selecting it selects every stroke; a partly selected run shows a dimmer fill | `CD:1434-1443`, `LP:111-113`, `LP:126-129`, `LP:153` |
| The tree | A frame's row sits above its children's rows, which are indented with a thread in the frame's tint; a caret collapses a frame for the session (the count of hidden children shows) | `CD:1480-1492`, `LP:14-19`, `LP:116-131`, `LP:189-197` |
| Row gestures | Click = select the row's elements; Shift + click = add / remove; double-click = open (as Return); right-click = the canvas's menu | `LP:52-59`, `LP:177-187` |
| Drag to reorder | A drop line between rows; the dragged rows (or the whole selection when the row is part of it) land above the row below the gap; one history row. A gap inside a frame's block joins that frame; a member dropped elsewhere leaves. A frame row moves with its children | `LP:60-87`, `LP:166-176`, `CD:1517-1536` |
| Follow | A selection made on the canvas scrolls its row into view | `LP:88-97` |
| Frame section | Title field, seven tint swatches, Group / Board, and for a board the four aspects | `FP:34-94` |
| History | The active undo path, oldest first, with an "Opened" row; the current state is filled, redoable states are dimmed; each row shows its label and the time; a click jumps there. At most 200 rows, then "… N earlier steps" | `In:142-202`, `CD:2027-2087` |
| Which log | The canvas's log while the canvas is up, the page's otherwise | `In:153-155`, `Ed:497` |
| Per-element properties (position, size fields) | **Not found** | `In:83-212` |

---

## 8. Toolbar, context menu, shortcuts

### 8.1 The toolbar on the canvas

| Item | Rule | Ref |
|---|---|---|
| Tools, in order | Select (V), Hand (H), Page (N), Frame (F), Text box (T), Text plain (⇧T), Pen (P), Rectangle (R), Oval (O), Line (L), Arrow (A), Eraser (E). One is checked; nothing is ever disabled | `CT:18-31`, `CT:42-57` |
| In a PDF / video tab | Select / Move, Freehand, Rectangle, Oval, Line, Arrow, Text box, Eraser | `CT:32-41` |
| In a page | No drawing tools | `CT:12-15` |
| Right side, every mode | Pen width (a button showing a line of that thickness; opens the slider), three colour clips (text, highlight, pen), undo, redo, the zoom badge | `ChT:219-262`, `ChT:269-270` |
| Undo / redo buttons | Enabled by the log of the surface in front | `ChT:259-262`, `Mn:1243-1246` |

### 8.2 The context menu

Rows that cannot apply are absent, not greyed. Sections are separated by a rule; the destructive row is red and last.

| Section | Rows and when they show | Ref |
|---|---|---|
| Primary | The open label, for one selected page, chip or media element | `Ctx:44`, `Ctx:57` |
| Own actions | Rename frame · Group · Add to frame · Ungroup · Remove from frame · Reveal in Finder / Show in Explorer · Open in ufb · Copy path (one local media element) · Refresh preview (a preview) · Copy link (a bookmark) · Spread files here · Refresh · Open as stream (a folder) · Notes page below (a video) · Next page · Previous page (a PDF) · Fit height to content (a page with a fixed height) · Fit width to text (a pinned chip) | `Ctx:60-80` |
| Insert | New page here · Add link… — only with nothing selected | `Ctx:83-85` |
| Arrange | Duplicate (something that is not a page is selected) · Bring to front · Send to back | `Ctx:87-89` |
| Delete | Delete | `Ctx:92` |

### 8.3 Keys on the canvas

| Key | Action | Ref |
|---|---|---|
| V H N F T ⇧T P R O L A E | The tools (only without Cmd / Ctrl / Alt). With a video selected the letters are its transport instead | `Ed:3442-3449`, `Ed:3438-3439` |
| Space (held) / Space (tapped) | The hand / the lightbox for a selected media element | `Ed:3444-3447`, `Ed:3262-3267` |
| Return | Open the selection (§2.4) | `Ed:3461` |
| Esc | Cancel the stroke → drop the tool → clear the selection | `Ed:3456-3460` |
| Delete, Backspace | Delete the selection | `Ed:3452` |
| `⌘G` / `⌘⇧G` | Group / ungroup | `Ed:3450-3451` |
| `⌘V` | Paste as elements at the view's centre | `Ed:3453` |
| PageUp / PageDown | Page a selected PDF | `Ed:3454-3455` |
| `⌘]` / `⌘[`, with `⌥`: | One layer forward / back; to the front / back | `Ed:3395-3399` |
| `⌘Z` / `⌘⇧Z` / `⌘Y` | Undo / redo on the canvas's log | `Ed:3390-3392` |
| `⌘+` `⌘−` `⌘0` `⌘1` | Zoom in, out, 100 %, fit | `Ed:3433-3436` |
| `⌥⌘←` / `⌥⌘→` | Back / forward | `Mn:1297-1298` |
| Everything else | Swallowed, so typing cannot edit the hidden page | `Ed:3436-3438`, `Ed:3462` |

---

## 9. Performance approach

| Item | Rule | Ref |
|---|---|---|
| Culling | Elements whose rect intersects the view grown by half a view on every side. A linear scan over all elements on each camera change | `CV:402-409`, `CD:748-755` |
| Slot pool | Views come from a pool sized in fours; an element that stays visible keeps its view while the camera moves; an idle slot is hidden by opacity and keeps its content loaded | `CV:410-417`, `CE:18-21`, `CE:36` |
| Paint order | Each view's stacking value is its element's index in the z-ordered list (not its slot) | `CE:11-14` |
| A hidden canvas | Holds no elements at all | `CV:403-404` |
| Page cost | The three LOD tiers and the per-frame row budget (§5.1); the square height cap exists so bounds, fit, culling and the poster never see a very tall card | `CP:8-13`, `CDh:65-68` |
| Pictures | Decoded at the element's width in world units × the screen's pixel ratio — the zoom is not part of it **(inference: a picture is soft when zoomed in, and decoded larger than needed when zoomed out)** | `MB:158`, `MB:185` |
| Ink | §6.5: rebuilt only on change; a new feather margin per zoom octave | `Ink:100-104` |
| Camera writes | Debounced 300 ms | `CV:517-518` |
| Revisions | Three counters drive what re-evaluates: the set (add / remove / z / json), geometry (rects), ink | `CDh:48-52`, `CD:950-956` |
| Spatial index, texture snapshots of pages, tile cache | **Not found** | `CD:748-755` |

---

## 10. Export of the canvas (brief)

| Item | Rule | Ref |
|---|---|---|
| Canvas as HTML… | One self-contained file: the canvas as an absolutely positioned world with pan and zoom; pages are the page export capped at the card's height; media are figures or cards that print the plain path; ink is SVG; chips are text; a tab strip opens a page, a video or a PDF | `CEx:2-8`, `ExC:107`, `ExC:419`, `ManC:156-158` |
| Canvas Poster (PNG)… | The world scaled to 1600 px wide: pages as sheets with a title and greek bars, media thumbnails, ink | `CEx:28-33`, `ExC:450-497`, `Mn:349` |
| Menu | File ▸ Export ▸ "Canvas as HTML…", "Canvas Poster (PNG)…"; enabled with a file open; the default name is `<file>-canvas.html` / `<file>-poster.png` | `Mn:1237-1238`, `Mn:325` |
| Canvas or board to PDF, SVG, per-frame export | **Not found** | `CEx:28-38` |

---

## 11. What the new app already has

### 11.1 Code

| Item | State | Ref |
|---|---|---|
| A canvas as a document | `DocKind::Canvas` is a row kind in the collection's `documents` table (`kind = 'canvas'`), with a parent, a rank and a title. **A stub**: there is no table or column for a canvas's content | `st:15-34`, `st:46-49` |
| Merge on save | A canvas's entry (its place and name) merges; "a canvas has no content here yet" | `st:216-220` |
| The 2.0 importer | `import_v2` makes one folder and one note per page. It reads only `elements` of kind `page`, in z order; **every other element, the rects, the frames, the ink and `canvas_meta` are not read**, and no canvas document is made | `st:296-313`, `old:35-58` |
| The sidebar | A canvas row is listed with its own icon and drawn in the disabled colour. A click or a keyboard move selects it and opens nothing | `sb:222-228`, `sb:149-154`, `sb:68`, `mn:1600` |
| Creating a canvas | **Not found** in the interface: the menus and keys make notes and folders only. A canvas entry is made only by the sample-file flag and by tests | `mn:831`, `mn:1704`, `mn:2364-2375` |
| Search | The collection's search walks notes and folders; canvases are skipped | `srch:47-48` |
| Document model | `mn-doc`, `mn-edit`, `mn-layout`: no canvas, element, ink or stroke type (searched) | — |
| The stroke model | `qcv-notes::strokes::Stroke { id, tool, color [f64; 4], width, filled, is_modeled, points }`, tools Freehand / Rectangle / Oval / Arrow / Line; reads and writes the same envelope as the old app, `normalized` only | `qs:8-16`, `qs:43-56` |
| The smoother | Our own, in Rust: a one-euro filter feeding a spring-mass-damper model with QCView's constants, output at ≥ 300 Hz, ending on the release point. No pressure input | `qi:1-8`, `qi:33-66` |
| Stroke geometry | One function from a stroke to paths in target pixels, used on screen and in exports: freehand fitted to Béziers, shapes as paths, a filled arrow head; widths in source pixels × scale | `qi:68-99` |
| Drawing | Through vello: stroked paths with round caps and joins, clipped to the picture. Not the capsule shader | `qa:436-447` |
| Tools and editing (video annotations) | Select, Freehand, Rectangle, Oval, Arrow, Line, Eraser. Select picks a stroke, drags it, resizes it by a corner (uniform), Delete removes it. The eraser removes the whole stroke under the pointer, hit-tested on the drawn line. Undo / redo per clip | `qa:1-9`, `qa:16`, `qa:319-345`, `qa:250-251` |
| Bound to a video | The annotation state is keyed by a playhead frame and saved to the notes sidecar; it is not a general canvas ink layer | `qa:33-56` |
| Pictures | A descriptor → a texture-ready picture, found, decoded and reduced off the main thread, decoded at the size shown and again when shown larger, under a memory budget; states Loading / Ready (with a fade and a format pill) / Missing / Failed | `mda:1-10`, `mda:17-35`, `mda:272` |
| Previews, PDF pages, video stills | The same job queue renders a preview's original, one PDF page, or one video frame; rendered previews are stored and cached on disk | `mda:43-46`, `mda:500-502`, `mda:522` |
| Path resolution | A descriptor's `src` (stored file, `{vol, rel}`, absolute path) → a path on this machine | `mda:321-322` |
| The presenter | Video goes through decode → colour → display → composite; stage 4 blends ONE interface texture (vello's output) over the video. There is one overlay slot, no per-element layers | `mp:1-11`, `eng:404-410`, `eng:695-696` |
| Colours | The palette has a Pen role named for canvases; not built | `col:2` |

### 11.2 Rulings in the canon (`docs/rust-wgpu-port.md`)

| Row | Ruling, in one line | Status |
|---|---|---|
| §7.2 C1, row 16 | The page-height cap on the canvas existed for cost; drop it as a cost cap, keep it only as a design choice | open |
| §7.2 C2 | Level of detail by per-page texture snapshots and greeked bars; no slot pool | proposal |
| §7.2 C3 | A real scene tree: children in local coordinates, nested frames, per-frame clipping. The file must choose absolute or local child coordinates | proposal |
| §7.2 C4 | Ink stays GPU capsules; pressure and tilt are one field per segment once the input layer delivers them | proposal |
| §7.2 C5 | The canvas (and a board) exports to PDF directly; SVG is a small emitter | proposal |
| §7.2 C6 | An R-tree for visibility; a minimap is one more render to texture; present mode is a camera animation | proposal |
| §8 (lines 478-495) | One `nodes` table, each container with one of four layout kinds (`flow`, `row`, `grid`, `free`); the canvas root and a group frame are `free`, a board is `grid` | proposal, predates row 271 |
| 54 | New drop zones: canvas hover feedback, frame lighting for outside drops, file drops on Layers frame rows | open |
| 214 | Stroke smoothing is our own, in Rust | ruled |
| 271 | Notes and canvases are sibling documents in a folder tree; no canvas is special. A note dragged onto a canvas is a live card editable in place; what is made on a canvas belongs to it and can be promoted to a note; a canvas inside a note is a preview that opens it; cards and previews render one level only. An old file converts to a folder with one canvas and one note per page | ruled |
| 272 | One file per collection, staged locally, merged on save; "canvas moves" are among the structural cases that need written merge rules | ruled |
| 274 | One ink system for a canvas, a PDF page, an image and a video frame, built on QCView 3's; pressure from a tablet (how it reaches the app to be verified); the eraser rubs out part of a stroke; whole strokes go by select and delete; the shared stroke format stays readable by QCView | ruled |
| 275 | One undo history per document across typing, canvas moves and ink; one shortcut table | proposed, confirmed |
| 283 | Step 1 converts pages only; the canvas's content is carried in the canvas step | ruled |
| 290 | A canvas is listed in the sidebar and cannot be opened until the canvas step | ruled |
| 298 | No drawing tools in the note's toolbar: drawing belongs to canvases | ruled |
| 299 | When a canvas is open, the toolbar band carries the canvas's tools | ruled |
| 301 | Three colour roles; Pen is for ink and outlines, on canvases only | ruled |
| 310, 315, 316 | Pictures, the PDF block and video blocks exist in notes; a canvas card can reuse them | ruled |

---

## 12. Open questions for the redesign

- **Q1 — What a page element becomes.** Row 271 says a note on a canvas is a live card of a note of the collection.
  The old page lived only on its canvas. Does a card hold a reference to a note (one note on several canvases, or
  twice on one)? What does a card show when its note is deleted or moved to another collection?
- **Q2 — Text made on the canvas.** Row 271: what is made on a canvas belongs to it and can be promoted. Is that only
  the text chip (plain text, §5.2), or a full block document owned by the canvas? Is the old "double-click empty canvas
  = a new page" kept, and does it now make a note in the tree or canvas-owned text?
- **Q3 — Editing a card in place.** The old app built live cards and removed them the same week in favour of
  click = select, double-click = open. Row 271 asks for in-place editing again. Which gesture enters the text, and how
  do select / move and the caret share a click?
- **Q4 — Card size.** Width = the note's measure, as before? Is the square cap kept as a design choice (row 16), a
  fixed height with scrolling, or a card as tall as its note?
- **Q5 — Frames and boards.** Do both survive? Is nesting allowed (C3)? Absolute or local child coordinates in the
  file? Is the board's grid layout in or out?
- **Q6 — The minimum useful canvas.** A proposed floor to confirm: pan and zoom, picture and file cards, note cards,
  text, ink, select / move / resize, delete, undo, z-order. Which of frames, Layers, bookmarks, folder cards, audio,
  "spread files", remote images and copy-in of another file are in the first pass?
- **Q7 — Ink on notes.** The old 2.0 has none (§6.6). Row 274 names a canvas, a PDF page, an image and a video frame
  — not note text. Confirm ink over text stays out, and say whether PDF-page and image ink are in this step or later.
- **Q8 — Stroke storage.** One element per stroke (old), or strokes grouped into an ink layer / run? This decides
  selection, the Layers list, merge on save and the draw-call count.
- **Q9 — Pressure and partial erase against the frozen format.** A stroke has one `stroke_width` and one `points`
  list. Where do per-point pressure and the pieces left by a rubbing eraser go so that QCView still reads the file —
  extra keys it ignores, or a canvas-only format with a conversion at the video edge?
- **Q10 — Smoothing space.** Smooth in screen space (old, zoom-dependent) or in world space?
- **Q11 — The renderer.** Capsule shader (C4, old canvas) or vello paths (what `qcv-annotate` does today)? One path
  for screen and export is the stated aim; which one, and does it hold for thousands of strokes?
- **Q12 — Undo and merge.** Row 275: one history per document. Is a canvas a document with its own history, separate
  from each note's — and what does undo do after typing in a card (Q3)? What are the merge rules for two people moving
  the same element, or one moving what the other deleted (row 272)?
- **Q13 — Importing an old file's canvas.** Row 271: one canvas + one note per page. The importer drops everything
  but pages today. Do page elements become note cards at their old rects? Are `connector` rows, `fixedH`,
  `view.scrollY`, the camera and frame membership carried?
- **Q14 — Media on a canvas vs in a note.** The same descriptor and the same store? Is the old rule kept that one
  PDF shows the same page everywhere and one video plays at a time across cards and notes?
- **Q15 — Open and the lightbox.** The old ladder opened a full-frame tab or the lightbox. What does "open" mean for
  each card in the new window (a note opens in the editor — and back returns to the canvas at the same camera)?
- **Q16 — Navigation history.** The old history recorded (file, page, frame) and not the camera. Does the new
  back / forward cross notes and canvases, and does it restore the camera?
- **Q17 — Input over remote desktop.** Wheel events arrive fractional and phase-less; modifiers and pinch may not
  pass. Is there a zoom path that needs neither (keys and the badge exist; a zoom tool or a drag-zoom does not)?
- **Q18 — Left drag on empty canvas.** Marquee (old) or pan? The old code and its own comment disagree.
- **Q19 — Missing basics.** Not in the old app: copy / cut / paste of elements, select all, arrow-key nudge,
  snapping and guides, align / distribute, lock, rotation, image aspect lock, a duplicate key. Which are in?
- **Q20 — Connectors.** The kind exists in the schema and was never built. In or out, and if in, what do the ends
  attach to?
- **Q21 — Storyboards.** A future idea (boards with numbered slots, captions, present mode, a contact sheet). What
  must this step leave room for — a grid layout kind, element-owns-content, per-board export?
- **Q22 — Export.** HTML and a PNG poster exist. Is PDF (C5) in this step? Does a canvas export include its note
  cards' full text or the card as shown?
- **Q23 — Where the tools and panels live.** Row 299 puts the canvas tools in the toolbar band. Where do Layers,
  the frame's properties and the history go — the old right-hand Inspector, or the sidebar?
- **Q24 — Search.** The old search matched card names, chip text and bookmark fields. Does the collection's search
  read canvases, and where does a hit land?

---

## 13. Oddities worth knowing before designing

- The header says the auto page height is capped at measure × √2; the code caps it at a square (`CDh:65-68` against
  `CD:697-700`).
- `CanvasView.qml`'s first comment says an empty-space drag pans; the handlers make it a marquee (`CV:19-20`,
  `CV:634-638`).
- The eraser removes one stroke per **click**, by bounding box, and a drag does nothing more (`Cap:62-67`,
  `CV:395-400`). On a PDF page the same tool hit-tests the drawn line (`MH:213-230`).
- Every element is hit by its rectangle: a long diagonal stroke owns its whole bounding box for clicks, marquees and
  right-clicks (`CV:99`, `CD:740-746`).
- A board clips how its children are **drawn**, but selection queries use each child's full rect — the clipped-off
  part of a child can still be hit **(inference)** (`CE:23-36`, `CV:99`).
- Return and double-click differ for a video or a PDF: Return = the lightbox, double-click = the full-frame tab
  (`CV:349`, `CV:505-510`).
- Space is the hand, a video's play key and the lightbox key at once; a "tap" is told from a "hold" only by an
  unchanged camera and selection (`Ed:3262-3267`).
- `⌘C` on the canvas is not swallowed: it runs the page's copy while the page is hidden **(inference)** (`Ed:3408`).
- Send to back and bring to front write z values outside the packed range; only a reorder re-packs (`CD:1413-1426`).
- A stroke becomes the selection as soon as it is drawn, so a selection frame appears after every stroke (`CV:732`).
- The shapes tools (rectangle, oval, line, arrow) make **ink** elements; the `shape` kind holds only text. A `shape`
  with any other `shape` value paints as an outline placeholder (`CS:36-40`, `Cap:16-23`).
- The comment on copy-in mentions a page "title override" in the element's json; no code reads or writes one
  (`CD:1124`, `CD:584-608`).
- `canvas.setCamera` does not mark the file dirty: a camera change alone is saved only if something else is
  (`CD:799-805`).
- The stroke loader refuses any coordinate system but `normalized`, so canvas ink is converted on every rebuild
  (`Ser:83-86`, `Ink:77-98`).
- Images have no aspect lock on resize; videos do (`SF:132`).
- A dropped file's descriptor patches (bookmark title, remote picture) change the file with no undo entry, and a
  remote picture also changes the element's rect that way (`CD:1289-1295`).

---

## 14. Not read

- Old app: `CanvasExporter.cpp` beyond the lines cited; `CanvasEditorFacade.qml`; `PageContextLoader`; the bodies of
  the four small cards (`CanvasFileCard`, `CanvasFolderCard`, `CanvasBookmarkCard`, `CanvasAudioCard`);
  `CompactTransport.qml`; `Lightbox.qml` beyond its header, its key handler and `step`; `ColorFlyout.qml` beyond its
  header; `ZoomBadge.qml` and `PathBar.qml` beyond their headers; `Converter.cpp` beyond the lines cited;
  `MediaStore.cpp` beyond the preview descriptor; `ink_stroke_modeler_wrapper.cpp` beyond the pressure lines; the ink
  shaders; `sketch_text.cpp`; the studio's `VideoAnnotator` / `viewport_annotator` (see the video spec);
  `DocumentManager`, `DocumentMerger`, `GuideBuilder`; `tests/regression_test.cpp`; the built-in canvas probes in
  `Editor.qml` (read only where a check's message confirmed a rule).
- Whether frames, clipping and tints appear in the HTML export and the poster.
- The text-chip tool on PDF pages (the `texts` list is stored and kept; its editor was not read).
- New side: the bodies of `qcv-annotate` (press / motion / release), `min-present`'s engine beyond the overlay slot,
  `apps/mn/src/media.rs` beyond its outline, `pdf.rs`, `video.rs`, `psd.rs`, `paint.rs`; the sibling phase-3 specs
  were used by name only.
