# Step spec: the PDF studio

Written 2026-11-14 by reading code only (nothing was built or run). It covers the owner's rulings of 2026-11-14:

1. The studio is a full-window view of ONE PDF block of a note (or PDF card of a canvas): the same block, the same
   file, the same drawings, only larger. Nothing new is stored; the sidebar tree does not change.
2. Entering: "Open in Studio" in the PDF's context menu, and a double click on its page. Esc or a Back button in the
   footer returns to the note / canvas at the same scroll position.
3. Layout: continuous vertical scroll of the pages, a thumbnail strip at the left, zoom to fit width / fit page / a
   percentage. The sidebar is hidden while in the studio.
4. Tools: the drawing toolbar pages already have, plus text selection and copy. Drawings made in the studio are the
   block's drawings.
5. (LATER) text notes pinned to a spot on a page. Not planned here; section B notes what is already in the record.
6. Cmd+F finds in the PDF's text across pages.
7. Reading mode: the studio opens in the window's mode; in reading one can page, zoom, select and copy, not draw.
8. A canvas's PDF card opens the same studio; canvas drawings stay the canvas's, PDF drawings the PDF's.

References:

- 2.x = the Qt app, branch `2.x` of the minNotes repository. `file:line` references are into that branch and are
  stable.
- 3.0 = `apps/mn` in this repository. `apps/mn/src/main.rs` and `canvas.rs` had uncommitted edits while this was
  written and their line numbers moved between two reads, so 3.0 code is cited by **function / type name**, not line.
- "Verified" = read in the code. "Inferred" = a conclusion drawn from the code that was not run; marked as such.

---

## A. What 2.x did for PDFs

### A.0 A naming point first

In 2.x "the studio" is the **video** view (`Editor.qml:7227-7234`, `studioFrame`). A PDF's full-window view is called
the "PDF tab" / "PDF view" (`pdfFrame`, `Editor.qml:7086-7225`); the menus say "Open in tab" for a PDF and "Open in
studio" for a video (`Editor.qml:8330-8331`; on a canvas "Open PDF" / "Open in studio", `CanvasView.qml:208`). The
shortcut list titles it "PDF View (full-window)" (`ShortcutCatalog.js:48`). So the 3.0 "PDF studio" takes the video
studio's name and the PDF tab's job. Everything below is about the PDF tab.

2.x also had a second, lighter view: the **lightbox** (`Lightbox.qml`), one page fit to the window over a scrim,
PageUp / PageDown and a pager bar, no tools (`Lightbox.qml:116`, `:136-140`, `:185-198`).

### A.1 Behaviour by behaviour

| # | Behaviour | Where (2.x) | Carry? |
|---|---|---|---|
| 1 | **One full-frame mode per window**, mutually exclusive with the table / video / canvas views; the document is hidden, not destroyed. State is one id: `activePdfId` (a block) or `activeElementId` (a canvas element). | `Editor.qml:400-407`, `:474-479`, `:553-595` (`setActiveTab`) | Yes, as one `Option<Studio>` (rulings 1, 8). |
| 2 | **One seam for both hosts**: `MediaHost` is built from (model, block id) OR (canvas, element id) and answers kind, url, size, pages, per-page ink, and routes writes and undo to the owner's history (the page's or the canvas's). | `MediaHost.h:1-8`, `:79-88`; `MediaHost.cpp:128-146`; `Editor.qml:1410-1417` | **Yes - the most useful idea in the 2.x code.** Ruling 8 needs exactly this: one studio, two owners. |
| 3 | **Continuous vertical scroll**: a `ListView` with one delegate per page, 10 px between pages, pages centred, white page with a hairline edge on the dark field. Delegates off screen are recycled; `cacheBuffer` keeps 1.5 view heights alive. | `Editor.qml:7112-7120`, `:7148-7165` | Yes (ruling 3). The 1.5-view-height look-ahead is a reasonable first figure for 3.0's own window of pages kept. |
| 4 | **Each page has its own size**, read from the file per page (`pagePointSize(index)`). | `Editor.qml:7153-7160` | Yes. 3.0 has no call for this yet (B.1). |
| 5 | **Rendering**: the Qt PDF module's `PdfPageImage`, asynchronous, at `sourceSize.width = settled width x device pixel ratio`. | `Editor.qml:7166-7181` | The idea (render at the size shown, off the window's thread) is already 3.0's rule. |
| 6 | **Zoom re-render is debounced**: while the zoom changes the rendered picture is only scaled; 150 ms after the width stops changing, one crisp render. The comment records the fault this fixed: "a zoom drag used to re-render every visible page per slider pixel (a flash per render and a stall on the render lock)". | `Editor.qml:7174-7181` | **Yes.** 3.0's cache would do the same wrong thing without it (B.3). |
| 7 | **Zoom model**: `pdfZoom` 1.0 = fit width; clamp 0.25 ... 4 x fit width; steps of sqrt(2); "100 %" = one PDF point per unit; fit = back to 1.0. The readout is in true-scale units (`zoom x fitWidth / pageWidthInPoints`). | `Editor.qml:409-429`, `:203-221` | Yes for fit width, steps and the true-scale readout. 2.x had **no fit-page**; ruling 3 adds it. |
| 8 | **Zoom is remembered per tab for the session** (a map keyed by block / element id), not written into the file. | `Editor.qml:411-421`, `:617-642` | Yes [proposed] - it fits "nothing new is stored". |
| 9 | **Zooming past fit width** grows a horizontal axis; the list then pans both ways. | `Editor.qml:7122-7129` | Yes. |
| 10 | **Zoom input**: Cmd +/-, Cmd 0 = 100 %, Cmd 1 = fit; Cmd-wheel; a zoom badge and slider in the chrome. | `Editor.qml:3361-3366`, `:7137-7146`, `ChromeToolbar.qml:269-270` | The gestures yes. The keys need a ruling: in 3.0 Cmd+1 is the sidebar and Cmd+0 is the canvas's "Fit Everything" (D.2). |
| 11 | **Wheel smoothing** for a notched mouse wheel and for wheels relayed by a remote desktop. | `SmoothWheel.qml:1-17` | Not needed as such: 3.0's `MouseWheel` handler already takes line and pixel deltas. |
| 12 | **Hand pan**: hold Space and drag, open-hand cursor; released on tab change so it cannot stick. | `Editor.qml:430-435`, `:408`, `:3250-3256`, `:3356-3357`, `:7132-7135` | Yes when zoomed past the width (the canvas already has Space-pan in 3.0). |
| 13 | **Drawing on pages**: a capture item over every page delegate, active when a drawing tool is armed; pen, rectangle, oval, line, arrow, eraser; tool letters V / P / R / O / L / A / E; the stroke in progress is drawn by a second ink item. | `Editor.qml:436-439`, `:7197-7213`, `:3358-3360` | Yes; 3.0 has the same set and letters on a page already (row 325). |
| 14 | **How a stroke is stored**: appended to the page's envelope `ink["<page>"]` in the media record - points as fractions of the page frame, width converted to source pixels (`x sourceW / frameW`), one history entry per stroke; the eraser removes the topmost whole stroke within reach; an emptied page loses its key, an empty `ink` leaves the record. | `MediaHost.cpp:178-230`; `BlockModel.h:757-764`; `BlockModel.cpp:4622-4681` | Already carried unchanged by row 325 (`ink::page_set`). 3.0's eraser is better (it cuts a freehand line; 2.x removed it whole). |
| 15 | **Undo in the view** goes to the owner's history (Cmd Z / Shift Cmd Z / Cmd Y). | `Editor.qml:3348-3355`, `MediaHost.cpp:145-146` | Yes. In 3.0 a page gesture is already one ordinary step of the note's (or the canvas's) history. |
| 16 | **All other keys are swallowed** "so typing can't invisibly edit the hidden document". | `Editor.qml:3348-3351`, `:3367` | **Yes - essential.** In 3.0 the note's editor stays live underneath; the studio must take the keys first. |
| 17 | **Esc is a ladder**: drop the armed tool first; then (canvas element) close the tab and select the element. | `Editor.qml:3281-3291`, `:482-486`; `ShortcutCatalog.js:53` ("Esc (repeat)") | The ladder yes [proposed]. See A.2 (1) for what was missing. |
| 18 | **Entering**: block menu "Open in tab"; canvas: double click / Return / the open glyph / the menu on a PDF element; the path bar's menu; a search hit; restore of the last view at open. | `Editor.qml:8330`; `CanvasView.qml:338-349`; `CanvasElement.qml:71`; `PathBar.qml:38`, `:307-313`; `SearchBar.qml:58-67`; `Editor.qml:642` | Menu and double click yes (ruling 2). Restore-at-open: no ("nothing new is stored"). |
| 19 | **Leaving a canvas element's view** returns to the canvas with the element selected. | `Editor.qml:482-486` | Yes. |
| 20 | **Search**: a file-wide search that includes PDF text, extracted once per file on a worker into a `pdf_text` table keyed by source + mtime + size; a hit shows "name - page N" and a snippet. | `SearchEngine.h:1-8`; `SearchEngine.cpp:56-62`, `:161-177`, `:196-218`; `Document.cpp:635-650` | The idea of extracting once off-thread and keying by mtime + size: yes, in memory (ruling 6). A table in the document: no (nothing new is stored). |
| 21 | **Per-page ink shown inline and on canvas cards** from the same record. | `MediaBlock.qml:164`; `CanvasMediaItem.qml:45` | Already so in 3.0 (`ink::page_paint`). |
| 22 | **Inline page render** through an image provider keyed by path + page, default 760 px wide; a fresh `QPdfDocument` is loaded for every page rendered. | `PdfPageProvider.h:6-27`; `MediaStore.cpp:237-246` | No. 3.0's `pdf::page` has the same re-open-per-page shape (B.1) and that is the first thing to measure. |

### A.2 What 2.x did badly, or lacked

1. **No Esc out of a block's PDF tab.** The Esc ladder closes only a canvas element's tab (`Editor.qml:3291`); with
   `activePdfId` set, Esc falls through to the document's own rungs and ends at a branch guarded by
   `activeFrameId === ""` (`:3332`). One left through the path bar. (Verified by reading the ladder `:3281-3337`;
   not run.) Ruling 2 fixes this.
2. **No thumbnails, no page number, no "go to page".** The only navigation in the PDF tab was scrolling. Ruling 3
   adds the strip.
3. **A search hit did not scroll to its page.** The jump sets the page in the map used by the INLINE block and the
   lightbox (`SearchBar.qml:58`, `Editor.qml:1104-1118`) and opens the tab; nothing in the tab reads that map or
   positions the list (the only uses of `pdfList` are sizes, `Editor.qml:7157-7158`). (Inferred from the absence of
   any positioning call.)
4. **No text selection or copy, no find inside the view, no highlighted hits.** The view is `PdfPageImage` pictures;
   there is no selection or search model in the QML (a grep for the Qt PDF selection / search types finds only the
   C++ text extraction, `SearchEngine.cpp:12`). 3.0 already does better in a note (row 315).
5. **Zoom did not anchor at the pointer**: `onZoomRequested: (dir, x, y) => root.pdfZoomStep(dir)` drops `x, y`
   (`Editor.qml:7142`), and the zoom is applied about the list's own origin. 3.0's canvas zooms about the pointer
   (`Canvas::zoom_by`); the studio should too.
6. **The shown page was keyed by the file's path**, so two blocks of one file always showed the same page
   (`Editor.qml:1104-1110`). 3.0 already rules otherwise (row 315 (3): `page` is in each block's record).
7. **Text chips on pages were half built.** The model has add / set / move / remove (`BlockModel.h:767-778`,
   `BlockModel.cpp:4683-4759`) and `setMediaPageInk` takes care not to erase a page's `texts` (the comment calls the
   earlier loss "the 2026-08-20 chips blocker", `BlockModel.cpp:4674-4677`) - but no QML calls them (only
   `tests/regression_test.cpp:2050-2125`), and the tab ends in a comment announcing a "Text-chip edit overlay" with
   no code under it (`Editor.qml:7218-7222`). Relevant to ruling 5 only: the `texts` array may exist in old records
   and 3.0 already preserves it (`ink::page_set`).
8. **Stale comments**: the view's header says "PdfMultiPageView owns its own scrolling/selection" and the next
   comment says it is NOT used because it "always overflows horizontally" (`Editor.qml:7087-7098`).
9. **The slider's range had to be cut** because 4 x fit width "is far past useful and made the slider's right half a
   cliff" (`Editor.qml:212-215`). A fixed percentage list is simpler than a slider.

---

## B. What 3.0 already has

### B.1 `apps/mn/src/pdf.rs` - the engine's one interface

| Item | What it does | For the studio |
|---|---|---|
| `info(path) -> (pages, w, h)` | Page count and the FIRST page's size in points, rotation applied. | **Not enough**: a multi-page layout needs every page's size. New: `sizes(path) -> Vec<(f32, f32)>` (one open, `render_dimensions()` per page). Cost for several hundred pages is unknown - measure (C.5). |
| `page(path, page, width) -> (rgba, w, h)` | One page as opaque RGBA on white, `width` pixels wide. The scale is capped so no side passes 65 000. | Usable as it is for whole pages and thumbnails. Two limits: (a) every call runs `open()`, which parses the file again (`Pdf::new`) and builds a new `RenderCache` - the bytes are shared (`FILES`, 512 MB kept) but the parse is not; spike S10 measured the open alone at 3 / 20 / 66 ms (min / median / max) on 4-61 MB files, which is paid per page here; (b) it renders the WHOLE page - there is no "this rectangle of the page at this scale", which a zoom past the picture cap needs (B.3). |
| `text(path, page) -> PageText` | Every glyph with its place (points from the page's top left). Also opens the file again. | Usable per page. For find across pages it must run for all pages off the window's thread, ideally with one open. |
| `PageText::at`, `words`, `rects` | Hit a character at a point (nearest on the nearest line otherwise); the text of `a..=b` as words and lines; the rectangles covering `a..=b`. | **Usable as they are** for selection, copy and for painting find hits (`rects` over a match's character range). |
| `fenced` | Every engine call is wrapped against a panic and answers in words. | Keep using it for the new calls. |

There is no outline / bookmarks / links call. None of the rulings needs one.

### B.2 `apps/mn/src/ink.rs` - strokes on a page (row 325)

Storage, verified in `page_lines` / `page_set` and their test
(`a_pages_ink_lives_in_the_blocks_record_as_the_qt_app_kept_it`): the block's record holds
`ink["<page number>"] = { version, coordinate_system: "normalized", shapes: [...], texts?: [...] }`. Points are
**fractions of the page frame**; widths are in the record's own pixels (`w` across, i.e. PDF points of the first
page); a pen's pressure is in `pressures`. So ink is independent of the size shown: **the studio needs no conversion
and writes the same record.** Ruling 4 ("seen in the note afterwards") holds by construction.

| Item | What it does | For the studio |
|---|---|---|
| `page_lines(rec, page, size)` | A page's strokes as `Line`s in the page's units. Takes the page as a parameter. | As it is. |
| `page_set(descriptor, page, size, lines)` | The record with one page's strokes replaced; keeps `texts`; drops empty keys. Takes the page as a parameter. | As it is. |
| `page_size(rec, frame)` | The page's units: `(w, w x frame.height / frame.width)`. | As it is, given the page's frame (but see B.8 (1)). |
| `page_no(rec)` | The page a record SHOWS (`page`). | Not for the studio: it draws on the page under the pointer, not the shown one. |
| `Gesture::press(pick, block, descriptor, rec, frame, p, ...)` | Begins a gesture. **Takes the page from `page_no(rec)` internally.** | **Must change**: add a `page: usize` parameter (the note passes `page_no(rec)`). `drag`, `live`, `release` are page-agnostic and stay. |
| `page_paint(scene, id, descriptor, rec, frame)` | Paints the SHOWN page's strokes into `frame`, with a cache of built paths `PAGES: HashMap<BlockId, (hash, Vec<Drawn>)>` - **one entry per block**, cleared whole past 400 entries. | **Must change**: with several pages of one block on screen the single entry would be rebuilt for every page every frame. Add a page parameter and key the cache by (block, page). |
| `placed`, `drawn`, `hit`, `rub`, `reduce`, `settled`, `line_of`, `svg` | Geometry, hit test, eraser, export paths. | As they are. |
| `Pick`, `Ink` | The armed page tool. | As they are (`Files.pick`). |

### B.3 `apps/mn/src/media.rs` - rendering, cache, text

Verified in `Media::view`, `Media::poll`, `Media::trim`, `work`:

- `view(descriptor, w, h)` is the only way to get a page's picture. The page comes **from the record's `page`**;
  the cache key is `"{src}#{page}"`; the size asked is `w x h` in physical pixels, scaled down so the longer side is
  at most `LONGEST = 4096` ("the drawing library keeps every picture of a frame in one 8192 px atlas").
- One entry per page holds **one picture**. It is rendered again only when shown more than 1/8 wider than what was
  asked before; it is **never made smaller**, and a larger render replaces the smaller one without a fade.
- Jobs go to 1-4 worker threads, **newest first**; a new ask for the same key replaces the queued one. There is no
  way to withdraw asks for pages that left the view.
- Memory: `BUDGET = 4 GiB` of decoded pictures (`MN_MEDIA_BUDGET_MB` overrides). `trim` runs only when something
  arrives, and never drops what the current frame used.
- `page_text(descriptor)` returns the SHOWN page's `PageText` from `TEXTS`, filled by the worker **after it rendered
  that page**. `TEXTS` is keyed like the pictures, holds at most 64 pages and is then **cleared whole**.
- `Media::held()` reports bytes and count of pictures held (already printed by a scripted run).

What this means for the studio:

| Need | As it is? |
|---|---|
| A picture of page N of a block, N not the shown page | **No.** Add `view_page(descriptor, page, w, h)` (the body of `view` with the page passed in), or have the studio pass a copy of the record with `page` replaced - the first is cleaner. |
| Thumbnails and large pages at once | Works but wastefully: one key per page means a thumbnail is drawn from whatever size the page was last rendered at, and a page visited at full size stays at full size for its thumbnail's whole life. Give thumbnails their own key (`"{src}#{page}@thumb"`) at their own small size so the large pictures can be let go. |
| Zooming | Every step past +12.5 % queues a new whole-page render of every visible page. Needs the 2.x debounce (A.1 #6): paint the held picture scaled, ask for the sharp one once the zoom has rested. |
| Zooming OUT | Pictures never shrink, so a zoomed-out view shows many pages each still held at its largest size. (Inferred: several of these in one frame can pass what the 8192 px atlas holds - the comment at `LONGEST` is the only statement of that limit; measure, C.5.) The studio should ask by size bucket when zoomed out, or keep the thumbnail picture for small pages. |
| Sharpness at high zoom | **Capped.** A portrait Letter page (612 x 792 pt) fit to a 1400-point-wide view on a 2x screen is already about 2800 x 3620 px; the 4096 cap on the longer side is reached at about 113 % of that. Past the cap the picture is only magnified. Going further needs either tiles (a new `pdf::region(path, page, scale, rect)` - hayro's `render` takes scales, a clip / offset was not checked) or a ruling that the cap is the ceiling (D.4). |
| Memory for a long file | A page at fit width is about 13 MB (1600 x 2070 x 4) to 40 MB (2800 x 3620 x 4). Scrolling through 300 pages can hold 4-12 GB before the general budget trims anything. **The studio needs its own window**: keep the pages within about 1.5 view heights (2.x's figure) at full size, the rest as thumbnails, and drop the others when they leave - a `Media::forget(key)` or a per-kind budget. |
| Stale work | A fast scroll through hundreds of pages queues one job per page seen; newest-first keeps the screen fresh but every stale page is still rendered and held. Add a way to drop queued asks whose page is out of the window (`queue.jobs.retain`). |
| Text of pages not rendered, for find | **No.** `TEXTS` is filled only as a by-product of rendering the shown page and forgets everything at 64. Find needs its own store: all pages' `PageText` (or just their strings plus per-match rectangles), built once per (file, mtime, size) on a worker, kept while the studio is open. |

`Places::record` writes a PDF's record as `{kind: "pdf", src, w, h, pages, name}` with `w`, `h` the first page's
points, rounded.

### B.4 `apps/mn/src/main.rs` - the PDF block in a note

| Item | What it does | For the studio |
|---|---|---|
| `struct PdfSel { id, a, b, drag }` (`Files.pdf`) | Text selected on a page: the block and the first / last character index **of the shown page**. | Needs a page: the studio's selection is `(page, a, b, drag)`. Either add `page: usize` to `PdfSel` or give the studio its own type; a selection never spans pages today (D.6). |
| `pdf_boxes(id)` / `pdf_under(x, y)` | A PDF block's page rectangle and its strip (the lower `PDF_STRIP` = 36 units) in the window, from the note's layout. | Note only. The studio has its own layout (C.2) and its own "page under the pointer". |
| `pdf_char(id, x, y)` | The character under a window point: `k = text.w / page.width`, then `PageText::at`. | The arithmetic is reusable given a page rectangle; the function itself reads the note's layout and the shown page. Write the studio's twin over its own page rectangles. |
| `pdf_turn(id, by)` | Changes the record's `page` outside the history; in reading it is not marked as an edit. | Reusable for the open question "does the block show the page last seen in the studio" (D.3). Not needed for scrolling. |
| `ink_block()` | The current block that can be drawn on: its frame and record. Returns `None` on a canvas without an entered card. | Add a studio branch in front (the page under the pointer / the page of the gesture), or bypass it with studio twins of the three functions below. |
| `page_press` / `page_drag` / `page_release` / `page_show` / `page_point` / `page_keep` | A gesture: `Gesture::press` in the page's units, the record updated outside the history while dragging (`ed.doc.set_kind`), put back at release and committed as ONE step (`ed.set_kinds`). | The sequence is exactly what the studio needs for a NOTE's block. It reaches the frame through `ink_block()` and `Files.iblock`, and the page through the record; with `Gesture::press` taking a page and `iblock` set by the studio to (block, that page's rectangle, record) these work unchanged. For a canvas card the commit goes to the canvas instead (B.6). |
| `page_picked(f)` | Restyles / deletes the picked stroke; uses `page_no(rec)`. | Needs the picked stroke's page (store it in `Files.psel`). |
| `page_key(key)` | V / P / L / A / R / O / E arm or put down a tool; Esc ladder (gesture, picked stroke, tool); Delete removes the picked stroke. Guarded by `ink_block().is_some()`. | The body is reusable once `ink_block()` knows the studio. |
| `redraw`: the selection wash (`t.rects(sel.a, sel.b)` scaled into the block's rectangle) and the live stroke / picked stroke's box (from `Files.iblock`, `Files.gest`, `Files.psel`) | Paints what is in progress. | The same few lines, with the studio's page rectangle. |
| `key`: Cmd+C copies `t.words(a, b)`; Cmd+A selects the page's text; Esc lets go; any other key drops the selection | PDF text keys. | Same behaviour in the studio (Cmd+A = the page under the pointer / the current page). |
| Left `PointerButton`: page gesture first, then the strip turns the page, then a press on text starts `PdfSel` | Order of claims on a press. | Same order in the studio, minus the strip. **A double click on the page is not used in a note today** (the press starts a text selection and returns before the editor's own double-click code), so ruling 2 is free there. |
| `find_open` / `find_refresh` / `find_show` / `find_out`, `Files.find`, `Files.hits` | Find in the note (row 321): hits are `mn_edit::find::Hit { block, start, end, kind }`; a video block is a hit when one of its notes holds the words. **PDF text is not searched** (row 315 (2) left it "when search exists"). | The bar (`findbar::FindBar`) is reusable; its hit type is not (it addresses a block's text). The studio needs `(page, a, b)` hits and must hide the Replace row. |
| `set_reading`, `self.reading` | Reading: `ed.locked`, the toolbar band put away (`Toolbar::layout` returns 0 when `reading`), `Files.tool = None`. | Ruling 7 mostly falls out: the band is already hidden in reading. The studio must also refuse `page_press` / `page_key` tool letters when reading (in a note this is implied by other guards; make it explicit). |
| `origin`, `note_point`, `self.scroll`, `self.side_w`, `self.top`, `self.bottom` | The note's place in the window. | `self.scroll` must be left alone (or saved) so the note comes back where it was; the studio lays out with the sidebar's width forced to 0. |
| `swap_note`, `end_session`, `open_collection`, `canvas_leave`, `guide_open`, `guide_leave` | Every path that changes what is in the window; each clears `Files.pdf`. | **Each must also close the studio.** |
| `text_element::set_obscured(...)` in `redraw` | Tells the desktop's writing tools the note's text is covered (reading, a canvas, a dialog, a menu). | **Add the studio** to that list: the note's text is still laid out underneath and must not be offered while the studio covers it (row 315 (1): a PDF's own text is not offered either). |

### B.5 `apps/mn/src/toolbar.rs`

`Toolbar::sets[3]` is the page set (row 325): `DrawSelect, DrawPen, DrawLine, DrawArrow, DrawRect, DrawOval,
DrawErase` and `PenColor, PenThinner, PenThicker`, with tooltips carrying the keys. `Shown.ink: Option<InkShown>`
with `page: true` selects it. **Usable as it is**: in the studio `redraw` passes `InkShown { page: true, tool, pen }`
whenever not reading. The zoom controls and the Back button are new buttons (rulings 2, 3) - the footer
(`footer.rs`: `Footer`, `Do`, `Shown`) has the room at its left end, where the sidebar's button sits and is of no use
while the sidebar is hidden.

### B.6 `apps/mn/src/canvas.rs`, `paint.rs` - the PDF card

- A PDF card is a `media` element whose record is the same JSON as a note's block (`kind_of(e) == Some("pdf")`).
  `Canvas::paint` draws it with `media.view(&e.record, r.width(), r.height())` and then
  `ink::page_paint(scene, e.id, &e.record, &rec, r)` - the same two calls as `paint::note`.
- `Canvas::pdf_page(forward)` turns the selected card's page (Option+Left / Right, and the menu's "Previous Page" /
  "Next Page"); in reading it is not a step.
- Ink ON a card (row 325 (2)): the card is **entered by pressing it twice** (`Canvas.inked`), then `card_page`,
  `card_lines`, `card_set` convert between the page's units and the sheet's (`ink::placed`) and write the element's
  record as a step of the CANVAS's history. Ink drawn over a card that is not entered is the canvas's own `ink`
  element. So ruling 8's separation already exists in the data: the PDF's drawings are in the card's record, the
  canvas's are separate elements.
- The same double press also calls `Canvas::open(id)`, which moves the camera to the card and remembers the camera
  before in `Canvas.back`.

For the studio: the card's record is read with `Canvas::get(id)`; a page's strokes are written back through a new
small method beside `card_set` that takes a page and the page's own units (no `placed` conversion is needed: the
studio works in the page's units, not the sheet's) and makes one canvas step. **Conflict to rule on**: ruling 2's
"double click on its page" is already taken on a canvas by "enter the card to draw on it" (D.1).

In a note, `paint::note` draws the page in the block's rectangle minus the strip: a white ground, the picture fit
inside the box and centred, `ink::page_paint` over the BOX, then the strip ("3 / 12" and two carets).

### B.7 `apps/mn/src/export.rs` - how exports draw page ink

`Files::item` (the `What::Pdf` arm): the stills of a PDF block are **the page shown plus every page that has
shapes**, in page order, each rendered with `pdf::page(path, p, STILL = 1280)` and labelled "page N of M"; a page's
drawings come from `ink::page_lines(&rec, page, size)` with `size = (record w, record w x h / w of the rendered
page)` and are written as an SVG layer (`ink::svg`) or baked into the picture, by export mode. **Nothing to change**:
drawings made in the studio on any page appear in every export (ruling 4), because the export already walks all the
keys of `ink`.

### B.8 Faults found on the way (listed for a ruling; none was changed)

1. **Pages of another shape than the first.** A block's box has the FIRST page's shape (`mn-layout` sizes a media
   block from the record's `w`, `h`). `paint::note` fits other pages inside it, centred. But `ink::page_size(rec,
   frame)` is given the box, so in a note ink is stored as fractions **of the box**, while the export computes the
   page's units from the rendered page's own shape - fractions **of the page**. For a file whose pages all have one
   shape the two are the same; for a mixed file (a portrait report with landscape plates) the same stroke lands in
   different places in the note and in the export. `pdf_char` has the same assumption (`k = t.w / page.width`, from
   the box's left edge), so text selection is offset on such pages. The studio gives each page its own shape, so it
   must pick one rule; "fractions of the page itself" is the export's rule and the Qt app's (its tab drew ink over
   the page rectangle, `Editor.qml:7183-7191`). (Verified by reading; not run on a mixed file.)
2. `TEXTS` is keyed by source and page only: a file replaced on disk under the same name keeps its old characters
   until 64 other pages have been read. (Inferred.)
3. The doc comment for `PdfSel` sits two declarations above it in `main.rs` (cosmetic).

---

## C. Build plan for rulings 1-4 and 6-8

### C.1 The state

One new field, `Files.studio: Option<Studio>`, and a new file `apps/mn/src/studio.rs` holding the type, its layout
and its painting (main.rs keeps only the hooks).

```
struct Studio {
    host: Host,                    // Block(BlockId) of the open note | Card(BlockId) of the open canvas
    path: PathBuf,                 // the located file (Media::locate)
    sizes: Vec<(f32, f32)>,        // every page's size in points (pdf::sizes); empty until read
    zoom: Zoom,                    // FitWidth | FitPage | Percent(f64)   (100 % = one point per unit, as 2.x)
    scroll: (f64, f64),            // of the page column; x only when wider than the view
    strip_scroll: f64,             // of the thumbnail strip
    rested: Instant,               // when the zoom last changed (the sharp render waits for it)
    over: Option<usize>,           // the page under the pointer
    sel: Option<(usize, usize, usize, bool)>,   // page, first, last, dragging
    find: ...,                     // C.4 stage 5
    back: Back,                    // the note's scroll, or nothing for a canvas (its camera is untouched)
    sidebar_was_open: bool,
}
```

The record itself is NOT copied into the state: it is read from the host each time (`ed.doc.get(id)` or
`canvas.get(id)`), so undo, a merge or a relink underneath is always what is shown. If the host is gone or is no
longer a PDF, the studio closes (2.x's `onGone`, `Editor.qml:1416`).

A small host seam, after 2.x's `MediaHost` (two functions on `App`): `studio_record() -> Option<String>` and
`studio_commit(page, before, lines)` - the first reads from the note or the canvas, the second calls `page_keep`
for a block or the new canvas method for a card. Undo / redo should need nothing new: the footer already takes
`can_undo` / `can_redo` from the canvas when one is open with no card entered, else from the note. (Inferred from
`footer::Shown` in `redraw`; the Undo command's own routing was not read - check it in stage 3.)

### C.2 New code

- **`pdf::sizes(path)`**: one open, all pages' `render_dimensions()`.
- **The page column** (pure functions, unit-testable without a window): from `sizes`, the zoom and the view's width
  -> each page's rectangle (pages centred, a fixed gap, 2.x used 10), the total height, `page_at(y)`, `visible(range)`.
  Fit width = the widest page fills the view less margins; fit page = the page at the view's centre fits whole.
- **`Media::view_page(descriptor, page, w, h)`** and a thumbnail variant with its own key and size; **`Media::forget`**
  (or a studio budget) and a way to drop queued asks (B.3).
- **The thumbnail strip**: a column at the left (about the sidebar's width, 240 units, since the sidebar's place is
  free), each row a small page picture with its number, the current page marked; its own wheel scroll and scroll bar
  (`min_ui` scroll bar, as the note's `pbar`); a click scrolls the column to that page; the strip follows the column.
  Only the rows in view ask for pictures. Thumbnails show the page's ink [proposed] - `ink::page_paint` at that size
  costs little.
- **Zoom**: Cmd-wheel and pinch about the pointer (the arithmetic of `Canvas::zoom_by`), keys (D.2), and a control in
  the footer: fit width, fit page, and a percentage list built from `min_ui` parts (`choice::ChoicePicker`'s `plain`
  face is the existing list popup). While the zoom is changing the held pictures are painted scaled; the sharp
  render is asked for once `rested` is 150 ms old (2.x's figure).
- **Find**: C.4 stage 5.
- **Strings**: every new string through `min_i18n::trc` with drafts (CLAUDE.md); note that minNotes 3.0 ships English
  only (row 347), so drafts are marked unreviewed.

### C.3 The hooks, by function

| Function (main.rs) | Change |
|---|---|
| `redraw` | (1) When the studio is open, lay the chrome out with the sidebar's width as 0 (do not call `sb.layout`; skip its paint) so the band and footer span the window. (2) `toolbar::Shown.ink = Some(InkShown { page: true, .. })` unless reading. (3) `footer::Shown` gains what the Back button and zoom control need. (4) Add the studio to `text_element::set_obscured`. (5) In place of `cv.paint(...)` and `paint::note(...)`: `studio.paint(...)` - the column's ground, visible pages (white, hairline edge, `view_page`, `page_paint`), the selection wash, find hits, the live stroke and picked stroke's box, the strip. The note's `NoteLayout` is still built (cheap, and other code unwraps it) but not painted; `self.scroll` is not touched. (6) Request another frame while pictures are loading or the zoom has not rested. |
| `WindowEvent::MouseWheel` | A studio branch before the canvas branch: over the strip -> its scroll; Cmd / Ctrl -> zoom about the pointer; else scroll (Shift or a sideways delta -> x when wider than the view). |
| `WindowEvent::PinchGesture` | Today guarded by `self.files.canvas.is_some()`; add the studio. |
| `WindowEvent::PointerMoved` | Update `studio.over`; drag of a text selection (the twin of the `Files.pdf` drag line); `page_drag` when a gesture is under way; Space-drag pan. |
| `WindowEvent::PointerButton` Left | After `chrome_pointer(e)` (band, footer, menus stay live) and before the canvas / note code: release -> `page_release` or end the selection drag; press -> strip click, else `page_press` on the page under the pointer (not in reading), else start a text selection there. Everything else is swallowed. |
| `WindowEvent::PointerButton` Right | In the studio: a small menu (Copy, Select All on this page, Back) or none [proposed: Copy / Select All / Close Studio]. |
| The note's block menu builder (where "Reveal in Finder" / "Copy Path" are prepended for a media block) and `ctx_pick` | Prepend "Open in Studio" for a single PDF block; the pick opens it. Enabled in reading too (the reading filter in the right-button handler keeps only listed labels - add this one). |
| The canvas's menu list (with "Previous Page" / "Next Page") and its pick arm | Add "Open in Studio" for one selected PDF card. |
| Double click | Note: in the left-press code where a press on a PDF page with no tool armed starts `PdfSel`, detect a second press within 400 ms at the same place (the `last_click` pattern) and open the studio instead. Canvas: see D.1. |
| `key` | First thing after the Save shortcut: `if self.studio_key(key) { return }`, which handles Esc (ladder: stroke in progress, picked stroke, armed tool, text selection, find bar, then leave), tool letters via `page_key`, Delete, Cmd+C / Cmd+A, zoom keys, PageUp / PageDown / Home / End / arrows (scroll), Space (hand), Cmd+F - and **returns true for everything else** except Cmd+S, Cmd+Z / Shift+Cmd+Z, Cmd+R (reading) and Cmd+Q / window keys, so nothing reaches the hidden editor (2.x rule, `Editor.qml:3348-3351`). |
| The `KeyboardInput` arms before `key` (the canvas's Space arm, `video_key`) | Must not fire in the studio (a canvas is still open underneath for a card's studio). |
| `menu_cmd` | Find -> the studio's find; Sidebar -> ignored (or leaves the studio); the Export, Import and note-editing commands -> ignored or disabled while the studio is open [proposed: ignored with a notice]. |
| `set_reading` | On entering reading with the studio open: drop the gesture, the picked stroke and the armed tool (`Files.gest`, `psel`, `pick`). |
| `ink_block`, `page_press`, `page_picked`, `Files.psel` | Know the studio's page (B.4). |
| `set_cursor` | Crosshair over a page with a drawing tool, text cursor over text, hand while Space is held. |
| `swap_note`, `end_session`, `open_collection`, `canvas_leave`, `card_enter`, `guide_open`, `guide_leave`, the sidebar's row activation, files dropped on the window | Close the studio first (one `studio_close()` that restores the note's scroll and the sidebar). |

Entering (`studio_open(host)`): finish any text composition and note-words field as `set_reading` does; stop a
playing video (`video_stop`); close the note's find bar; remember `self.scroll`; fold the sidebar by not laying it
out (its `rail.collapsed` state is left alone so nothing is persisted); read `sizes` on a worker and show the first
page's shape meanwhile from the record's `w`, `h`; scroll to the record's `page` [proposed]. Leaving: clear the
state, set `self.scroll` back, select the card on a canvas (2.x did, `Editor.qml:482-486`), request a redraw.

### C.4 Stages, each walkable

| Stage | Built | Walk |
|---|---|---|
| 0 | The measurements of C.5, and three changes with no visible effect: `Gesture::press` takes a page; `page_paint` takes a page and caches by (block, page); `Media::view_page`. The existing ink tests and the scripted `MN_PAGE_INK` / `MN_PDF` runs must still pass. | Nothing new to see; the note behaves as before. |
| 1 | Enter (context menu, double click in a note), leave (Esc, Back in the footer), the page column at fit width, wheel scroll, sidebar hidden, keys swallowed, existing ink SHOWN on every page, the window of pages kept. No tools, no zoom, no strip. | Open a long PDF, scroll end to end, leave: the note is where it was. Reading and editing both. |
| 2 | The thumbnail strip; zoom (fit width, fit page, percentages, Cmd-wheel, pinch, keys) with the rested re-render; horizontal scroll and Space-pan when wider than the view. | Jump by thumbnail; zoom in and out on text and on a picture page; watch for flashes and stalls. |
| 3 | The drawing set: band shown, tools on the page under the pointer, picked stroke, colour / width, undo / redo; nothing of it in reading (ruling 7). | Draw on page 1 and page 40; leave; turn the block to those pages; export as HTML: both pages are there with their drawings. |
| 4 | Text selection and copy on a page (drag, Cmd+A, Cmd+C), in both modes. | Select, copy, paste into a note. |
| 5 | Cmd+F: the find bar without its Replace row; all pages' text read once on a worker (a readout "reading page N of M" in the footer while it runs); hits washed on the pages via `PageText::rects`; Next / Previous scroll to the hit; the count "3 of 12". Matching is over each page's `words(0, n)` string with a map from string offsets back to character indices (new, small). Case switch as the note's bar. | Find a word that occurs on several pages; step through; close; reopen keeps the words. |
| 6 | The canvas card: "Open in Studio" in the canvas menu (and the double press if D.1 rules so); drawings committed to the card's record as canvas steps; Back selects the card, camera untouched. | Draw in the studio on a card's page 3; back on the canvas turn the card to page 3; draw canvas ink over the card: the two stay apart (delete the card: its ink goes, the canvas's stays). |

Ruling 5 (pinned text notes) is not staged. When it comes, the record's per-page `texts` array is where the Qt app
meant such things to live and `page_set` already keeps it intact.

### C.5 Risks, and what to measure first

Risks:

1. **Memory.** 13-40 MB per page at fit width, 52 MB at the 4096 cap; the shared budget is 4 GiB and trims late
   (B.3). Without a studio window of pages, a long file fills it.
2. **Render time and the re-open per page.** S10: a middle page at 2000 px took 22 / 49 ms (median / max), plus an
   open of 20 / 66 ms that `pdf::page` pays every time. Fit width on a 2x screen is nearer 2800 px - about twice the
   pixels. Four workers hide some of it; a zoom step re-renders every visible page.
3. **Several hundred pages.** `pdf::sizes` cost is unknown; the strip has one row per page (only the visible rows
   may ask for pictures); find must read every page's text (unknown cost; S10 did not time `text`).
4. **The atlas.** Many large pictures in one frame when zoomed out (B.3). Unmeasured.
5. **Sharpness past the cap** (B.3, D.4).
6. **The hidden editor.** Any key or command that slips past the studio edits a note nobody sees (2.x's reason for
   swallowing keys). Undo while hidden may move `self.scroll` through the caret-follow code - hence the saved
   scroll (2.x recorded the same nit, `Editor.qml:1870-1874`).
7. **A file that changes or goes missing while open**: `sizes` and the text store are keyed by (path, mtime, size);
   a failed page says so in words (`View::Failed`), as in a note.
8. **Mixed page shapes** (B.8 (1)) - must be ruled before stage 3, or studio ink and note ink will disagree.

Measure before stage 1 (a throwaway harness or an `#[ignore]` test in `pdf.rs`; file paths from
`fixtures/sources.env` or the command line, never literals; numbers into `docs/spikes/` per the spike protocol):

- **M1 - one page at the studio's sizes.** Function: `pdf::page(path, p, width)`. Input: three files - (a) about 10
  pages of text, (b) a picture-heavy deck of about 200 pages and 50 MB or more, (c) 500 pages or more of text - at
  pages first / middle / last, widths 240 (a thumbnail at 2x), 1600, 2800 and 4096. Record: milliseconds per call
  (min / median / max over 5 runs), and separately the milliseconds of `pdf::info` on the same file (the open that
  every call repeats), and the bytes returned. Proposed pass bar: median under 60 ms at 2800 px and under 15 ms at
  240 px with the open excluded; if the open is more than a third of a call, keep the parsed file between calls
  (one `Pdf` per worker thread) before building anything else.
- **M2 - all pages' sizes and text.** Functions: the new `pdf::sizes(path)`; `pdf::text(path, p)` for every page.
  Input: files (b) and (c). Record: total milliseconds for each, total characters, and the bytes of all `PageText`
  held (characters x the size of `Char` plus its string). Proposed pass bar: sizes under 200 ms; all text of 500
  pages under 5 s on one worker (it runs behind a progress readout); if the held text passes about 100 MB, keep
  strings plus per-match rectangles instead of every glyph.
- **M3 - what a scroll-through holds.** With stage 0's `view_page` in a scripted run: ask for every page of file (b)
  at 2800 px in order, calling `Media::poll` between asks. Record `Media::held()` (bytes, count) at the end with the
  default budget, and the frame time of a frame that paints 2, 8 and 24 held pictures at once (the zoomed-out case).
  This sets the studio's page window and tells whether zoomed-out pages must use smaller pictures.

---

## D. Open questions for the owner

Real ones first - the code cannot answer these.

1. **Double press on a canvas PDF card.** Today it ENTERS the card so one can draw on it there (row 325 (2):
   `Canvas.inked`), and moves the camera to it. Ruling 2 gives the double click to the studio. Which wins on a
   canvas? [proposed: the double press opens the studio - drawing on a PDF card then happens in the studio; a
   PICTURE card keeps "press twice to draw on it". "Open in Studio" is in the menu either way.]
2. **Zoom keys.** 2.x had Cmd+1 = fit and Cmd+0 = 100 %. In 3.0 Cmd+1 is the sidebar and Cmd+0 is the canvas's "Fit
   Everything". [proposed: Cmd+0 = fit width, Cmd+9 = fit page, Cmd+= / Cmd+- = step, no key for 100 % (it is in the
   percentage list).]
3. **The page the block shows afterwards.** After scrolling to page 40 in the studio and going back, does the
   note's block show page 40, or the page it showed before? The record's `page` exists and turning it is already
   "not a step of the history" (row 315 (3)), so either is cheap. [proposed: it stays as it was; the studio OPENS at
   the block's page.]
4. **How far zoom goes.** Pictures are capped at 4096 px on the longer side (the drawing library's atlas); on a 2x
   screen a portrait page is at that cap a little past fit width in a large window. Accept "sharp up to the cap,
   magnified beyond" for the first build, or is crisp 200-400 % a requirement (then a tiled renderer is part of
   stage 2 and the engine must be checked for drawing a part of a page)? [proposed: accept the cap first; decide on
   tiles after the walk of stage 2.]
5. **Pages of another shape than the first** (B.8 (1)): ink in a note is stored against the block's box, in exports
   against the page. Which is right? [proposed: the page - as the export and the Qt app have it; the note's painting
   and `pdf_char` are corrected to the fitted page rectangle. Ink already drawn in 3.0 on an odd-shaped page would
   move; none is likely to exist yet, but only the owner knows.]
6. **Selecting text across a page boundary.** A selection is within one page today. Needed in the studio, or is
   per-page enough? [proposed: per page for the first build.]

Smaller defaults, proposed unless told otherwise:

- Esc is a ladder (stroke in progress, picked stroke, armed tool, text selection, find bar, then leave) [proposed],
  as 2.x and the note's `page_key`; the Back button always leaves at once.
- 100 % means one PDF point per screen unit (2.x); the percentage list is 50, 75, 100, 125, 150, 200, 300, 400.
- Zoom is remembered per block for the session only; nothing is written to the file.
- Thumbnails show the page's drawings.
- The thumbnail strip can be folded with the footer's sidebar button (Cmd+1) while in the studio, since the sidebar
  itself is away.
- Find in the studio searches the PDF only and has no Replace row; closing the studio closes its find bar.
- While the studio is open the Export / Import menu commands do nothing and say so in the footer's readout.
