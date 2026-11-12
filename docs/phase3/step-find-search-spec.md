# minNotes find and search — what the Qt app did (Phase 3, find / search step)

Source read: the minNotes repository at commit `59d1129` (branch `min-paths`), read only. Nothing in it was built or
run; every statement is read from code, from the app's own manual or from its regression test, and anything deduced
rather than read is marked **(inference)**. "Not found" means the code was searched and has no such thing.
References are `alias:line` relative to the minNotes repository.
`SE` = `app/core/SearchEngine.cpp`, `SEh` = `app/core/SearchEngine.h`, `SB` = `app/qml/MinNotes/App/SearchBar.qml`,
`Mn` = `app/qml/MinNotes/App/Main.qml`, `Ed` = `app/qml/MinNotes/App/Editor.qml`, `Mc` = `app/main.cpp`,
`Doc` = `app/core/Document.cpp`, `Doch` = `app/core/Document.h`, `BM` = `app/core/BlockModel.cpp`,
`BMh` = `app/core/BlockModel.h`, `CDh` = `app/core/CanvasDocument.h`, `BV` = `app/qml/MinNotes/App/BlockView.qml`,
`ChP` = `app/qml/MinNotes/App/ChoicePicker.qml`, `LP` = `app/qml/MinNotes/App/LayerPanel.qml`,
`NH` = `app/qml/MinNotes/App/NavHistory.qml`, `PB` = `app/qml/MinNotes/App/PathBar.qml`,
`TB` = `app/qml/MinNotes/App/TitleBar.qml`, `SC` = `app/qml/MinNotes/App/ShortcutCatalog.js`,
`ManB` = `docs/basics.md`, `Sh` = `docs/shortcuts.md`, `T` = `tests/regression_test.cpp`.

The new side (minplatform at `50281b8`): `sb` = `apps/mn/src/sidebar.rs`, `ft` = `apps/mn/src/footer.rs`,
`mnu` = `apps/mn/src/menu.rs`, `mn` = `apps/mn/src/main.rs`, `mda` = `apps/mn/src/media.rs`, `pdf` = `apps/mn/src/pdf.rs`,
`st` = `crates/mn-store/src/file.rs`, `stg` = `crates/mn-store/src/staging.rs`, `ed` = `crates/mn-edit/src/lib.rs`,
`lay` = `crates/mn-layout/src/lib.rs`, `lc` = `crates/min-ui/src/list_column.rs`, `qn` = `crates/qcv-notes/src/note.rs`,
`qio` = `crates/qcv-notes/src/io.rs`, `chc` = `apps/mn/src/choice.rs`.

Sibling specs this one leans on and does not repeat: `step1-core-spec.md` (blocks, spans, UTF-16 offsets),
`step-links-chips-spec.md` (links, choice chips), `step-media-spec.md` (the media descriptor, a missing file),
`step-tables-columns-spec.md` (cells, the grid frame), `step-video-spec.md` (the notes sidecar, the studio).

Vocabulary of the old code: a **file** is one `.mnd`; its root is the **canvas**; a **page** is a canvas element that
owns an ordered list of **blocks**; a **card** is any other canvas element (a media element, a bookmark, a folder, a
text shape); a **frame** (in the path bar's sense) is a full-window view of one table, PDF or video; a **hit** is one
search result; the **bar** is the search strip.

---

## 0. The shape everything below rests on

| Item | Rule | Ref |
|---|---|---|
| One feature, not two | The old app has **one** search: "Find in File". It is at the same time the in-page find and the across-pages search. There is no separate find-in-this-page bar | `SEh:2-8`, `SB:1-6` |
| Scope | One file: everything on its canvas. Never other files, never other open tabs ("the no-vault ruling") | `SEh:2`, `SB:5` |
| Age | Added in 2.0.1; one C++ class (233 lines) and one QML strip (168 lines) | `SEh:2`, `SB:1` |
| The result is a list | Search yields a flat list of hits shown in a drop-down. A hit is *jumped to*; nothing is highlighted in the document except the one match a jump selects | `SB:118-166`, `Ed:1121-1130` |
| No replace | **Not found** anywhere: no replace field, no replace command, no menu entry | `SB:78-116`, `Mn:1240-1250` |
| No index | No full-text index (no SQLite FTS table, no in-memory index). Every search re-reads the file's rows and scans them | `SE:100-194`, `Doc:580-600` |
| The engine follows the active tab | One `SearchEngine` for the app; its canvas is re-pointed whenever the active document changes | `Mc:254-258`, `SE:64-68` |

---

## 1. Find in the document (⌘F)

### 1.1 The bar

| Item | Rule | Ref |
|---|---|---|
| Where | A strip directly under the toolbar, full window width, above the page / canvas. Height = the tab-row height when open, 0 when closed (it pushes the content down; it is not an overlay) | `Mn:1598-1612`, `SB:21-25` |
| Exists only with a file | The strip lives in a loader that is active only while a document is open | `Mn:1600-1603` |
| Contents, left to right | A magnifier icon (14 px, muted); one text field; a count label; a close box (22 px, an `x`) | `SB:78-116` |
| Placeholder | `Search this file — pages, cards, PDFs, video notes` | `SB:95-100` |
| Count label | `indexing PDFs…` while a PDF is being read; empty for an empty query; else `no hits` / `1 hit` / `N hits`. Mono, small. It counts **hits**, not occurrences (§1.3) | `SB:102-107` |
| Not present | No case toggle, no whole-word toggle, no regex toggle, no previous / next buttons, no "n of m", no scope chooser, no replace row | `SB:78-116` |
| Open (`show`) | Opens, focuses the field, selects all its text; if the field still holds an earlier query the search runs again at once | `SB:27` |
| Toggle (`⌘F` again) | Open **and** the field has focus → close. Open with focus elsewhere → focus the field again (and re-run) | `SB:29` |
| Close (`hide`) | Closes the strip and the result list, gives keyboard focus to the editor. The query text is kept for the next open; the hit list is not cleared | `SB:28` |
| Close box | Same as `hide` | `SB:114` |
| Seeding from the selection | **Not found**: opening does not copy the document's selection into the field | `SB:27` |

### 1.2 Matching

| Item | Rule | Ref |
|---|---|---|
| Kind | Plain substring (`indexOf`) of the folded query in the folded text | `SE:110` |
| Query | Trimmed at both ends. Empty after trimming → no hits. Minimum length 1. Inner whitespace is literal | `SE:102-103`, `SB:31-32` |
| Case | Always insensitive (per-character case fold). No option | `SE:51` |
| Diacritics | Always folded: each character is decomposed (NFD) and replaced by its base character. An accented query finds the plain word and the reverse | `SE:42-54`, `T:6937` |
| Alignment | Folding is one character in, one character out, so a match's position in the folded text is its position in the original | `SE:43-45`, `SE:25-26` |
| Whole word | **Not found** | `SE:110` |
| Regex / wildcards | **Not found** | `SE:110` |
| Several words (AND / OR), quotes | **Not found**: the whole query is one substring | `SE:102`, `SE:110` |
| Across blocks | Never. Each block (each card field, each PDF page, each video note) is matched on its own | `SE:118-126`, `SE:150-158`, `SE:170-177`, `SE:181-189` |
| Matches per unit | **The first only.** One hit per block, one per card, one per PDF page, one per video note — a block that holds the word five times is one hit | `SE:120-125`, `SE:157`, `SE:171-176`, `SE:182-188` |

### 1.3 What is searched

"Content" below is the block's stored text column (`blocks.content`), read by SQL — not the editor's in-memory model.

| Content | Searched? | Detail | Ref |
|---|---|---|---|
| Paragraph, heading, quote, list / task / numbered item | Yes | The block's text | `SE:116-126` |
| Code block | Yes | Its text; no exclusion by type | `SE:119` |
| Table cells | Yes **(inference)** | A cell's content is ordinary blocks carrying a `cell` attribute; only `media` and `divider` types are skipped, so cell blocks hit as page hits | `SE:119`, `BM:2504`, `BMh:256` |
| Choice chip labels (inline) | Yes | A chip is a span over text; its label is in the block's text | `BM:4955`; links / chips spec §0 |
| Link text | Yes | Part of the block's text | `SE:120` |
| Link URL (the span's payload) | **No** | Payloads live in the block's attributes, which are not read | `SE:118-120` |
| Formatting, colours, block attributes | No | Only `content` is matched | `SE:120` |
| Inline media blocks (image, file chip, audio …): name, caption, path | **No** | A page's `media` blocks are skipped whole | `SE:112-113`, `SE:119` |
| Divider | No | Skipped | `SE:119` |
| Page titles | **No** | A page's title is only the *label* of its hits; it is never matched itself | `SE:117`, `SE:122` |
| Canvas media card (image, video, PDF, audio, file …) | Yes | Fields tried in order: display name, source path (a preview's original), folder entries | `SE:134-145` |
| Canvas bookmark card | Yes | Display name, then title, host, address | `SE:139-140` |
| Canvas folder card | Yes | Name, path, then each listed entry | `SE:142-144` |
| Canvas text shape ("text chip") | Yes | Its text; the hit's title is the literal `Text` | `SE:146-148` |
| Other shapes, frames (their titles), ink | No | Every other element kind is skipped | `SE:131`, `SE:149` |
| PDF page text | Yes | For every PDF — inline in a page **or** a canvas card. One hit per page that matches. Read from a cache (§1.7) | `SE:70-98`, `SE:164-177` |
| Video note text | Yes | For every video — inline or card. Each note's text; one hit per note. The notes sidecar is loaded from disk on every search | `SE:178-190` |
| Video note timecode, drawn ink | No | Only the note's `text` | `SE:182` |
| Comments | n/a | The comment span kind is retired | links / chips spec §0 |
| Hidden content | Yes | Nothing consults a table's view filter or a collapsed state | `SE:116-126` |

### 1.4 Order and grouping

| Item | Rule | Ref |
|---|---|---|
| Engine order | (1) the current page's blocks in rank order, (2) the other pages in canvas page order, each in rank order, (3) cards in element order, (4) PDFs and video notes, interleaved in media order: pages' inline media first (page by page), then canvas media elements | `SE:112-127`, `SE:129-159`, `SE:161-191`, `SE:70-98` |
| "Current page" | The canvas's focused page id, passed by the bar; moved to the front of the page list | `SB:32`, `SE:114-115` |
| Ranking | None. No scoring, no recency, no title-over-body boost. Order is position only | `SE:100-194` |
| Group headers | Derived in the bar from each hit's kind: `This page`, `Pages`, `Cards`, `PDFs`, `Video notes` (`Other` as a fallback). A header is drawn above the first row of each run | `SB:36-39`, `SB:140-147` |
| Filters | **Not found** (no kind filter, no page filter, no date filter) | `SB:78-116` |
| Limits | No cap on the number of hits, no paging | `SE:100-194` |

### 1.5 The result list

| Item | Rule | Ref |
|---|---|---|
| Form | A popup hung under the strip, as wide as the strip, drawn over the page / canvas. Height = its content, at most 440 px, then it scrolls | `SB:119-135` |
| Behaviour | Not modal, takes no focus, never closes by itself (a click outside does not close it). It opens when a run yields hits and the bar is open; it closes on zero hits and on `hide` | `SB:126-127`, `SB:34`, `SB:28` |
| Row | 40 px: line 1 the title (chrome size, elided right), line 2 the snippet (small, muted, elided right). A group's first row has a 22 px bold header above it | `SB:142-161` |
| Title | Page hit: the page's title. Card: its display name (`Text` for a text shape). PDF: `<name> · page N` (1-based). Video note: `<name> · <timecode>` | `SE:117`, `SE:136`, `SE:147`, `SE:173`, `SE:184` |
| Snippet | Up to 36 characters each side of the match, `…` where cut, line breaks turned to spaces, the match in bold, original case kept. HTML-escaped, shown as styled text | `SE:27-34`, `SB:156-160`, `T:6917` |
| Current row | Filled with the divider tone; a hovered row with the hover tone. The list scrolls to keep the current row in view | `SB:150`, `SB:133-134` |
| Mouse | A click on a row makes it current and jumps | `SB:162-163` |
| After a jump | The bar and the list **stay open** (nothing closes them), so the list still covers the top of the page jumped to | `SB:44-71`, `SB:127` |

### 1.6 Keys in the field, and jumping

| Key / gesture | What happens | Ref |
|---|---|---|
| Typing | Restarts a 120 ms timer; when it fires the search runs and the current row resets to the first | `SB:74`, `SB:88`, `SB:30-35` |
| `↓` / `↑` | Move the current row; clamped at both ends — **no wrap-around** | `SB:91-92` |
| `Return` / `Enter` | Jump to the current row. Does not advance; pressing it again jumps to the same hit | `SB:93`, `SB:72` |
| `Shift+Return`, `⌘G`, `⌘⇧G`, `F3` | **Not found.** (`⌘G` / `⌘⇧G` are group / ungroup on the canvas.) | `SB:89-94`, `Ed:3450-3451` |
| `Esc` in the field | Close the bar | `SB:90` |
| `Esc` in the page or on the canvas, bar open | Closes the bar **first**, before the editor's own Escape chain (collapse selection, leave the page …). The next `Esc` runs that chain | `Ed:3277-3280`, `Ed:5342-5345` |
| Jump: page hit | Focus the hit's page if it is not the focused one; a tick later look the block up by id (gone → nothing happens); leave any frame view; then `selectInRow(row, col, len)` | `SB:63-70` |
| … `selectInRow` | Commit pending Markdown in the block being left, place the caret at `col`, extend the selection by `len`, scroll the row into view, give the editor focus. **The match is shown as the ordinary text selection** — same colour, same object | `Ed:1121-1130` |
| … scrolling | `ensureVisible(row)` works from the model's per-row y and height (known for every row, drawn or not), so a far row is reached by setting the scroll offset; it brings the *row* into view, not the match's line. A block taller than the viewport moves only if wholly off-screen, so a match deep in a long block can stay out of sight | `Ed:1900-1914` |
| Jump: card hit | Switch to the canvas; a tick later select the element and animate the camera to a readable view of it (only if the camera needs to move) | `SB:47-56` |
| Jump: PDF hit | Set that PDF's shown page to the hit's page; then open the PDF's frame view — the canvas element's, or (after focusing its page) the inline block's. The matched words are **not** highlighted on the PDF page | `SB:58-67` |
| Jump: video note hit | Store the note's frame as that video's remembered playhead, then open its frame view (the studio) as for a PDF. The note itself is not selected in the notes list | `SB:57-67` |
| Focus after a jump | Page hit: the editor (so typing replaces the selected match). Card / PDF / video: whatever the view switch focuses; the field loses focus in every case, and `⌘F` returns to it | `Ed:1129`, `SB:29` |
| Caret / selection on close | Untouched: whatever the last jump selected stays selected. Closing without a jump leaves the caret where it was before `⌘F` | `SB:28` |

### 1.7 Liveness, threading, PDF text

| Item | Rule | Ref |
|---|---|---|
| Search-as-you-type | Yes, debounced 120 ms; there is no "press Enter to search" | `SB:74`, `SB:88` |
| Re-run triggers | The field's text changing; `show` with a non-empty field; a PDF's text landing in the cache while the bar is open. **Nothing else** | `SB:27`, `SB:75`, `SB:88` |
| Edits in the document | Do **not** refresh the hits. A hit keeps the block id, column and length from the moment of the search; after an edit the jump can select the wrong span, or do nothing if the block is gone | `SB:63-70`, `SE:122-125` |
| Changing page or file | Does not re-run either: the `This page` / `Pages` grouping is recomputed (it reads the focused page live) but the order and the hits are stale until the text changes | `SB:36-38`, `SB:32` |
| Where the scan runs | On the UI thread, synchronously, in the call the QML makes: one SQL read per page, a JSON parse per element and per inline media block, a file stat per PDF, and a sidecar file read per video — on every run | `SE:100-194`, `SE:70-98`, `SE:165-167`, `SE:180` |
| PDF text | Not read during a search. A PDF with no cached text is queued; a pool thread extracts every page's text; back on the UI thread it is written to the file and `cacheUpdated` fires, which re-runs the search | `SE:169`, `SE:196-219`, `SB:75` |
| The cache | Table `pdf_text (key, page, mtime, size, text)`, created on first write, inside the `.mnd` — it travels with the file. Key = the media descriptor's source value; a row set is valid only for the same file mtime **and** size, so a changed PDF simply misses and is re-read | `Doc:635-656`, `Doch:155-158`, `SEh:4-6` |
| A PDF that cannot be read | Cached as one empty page, so it is not retried on every search | `SE:217` |
| A missing PDF / unresolved path | Skipped silently; no hit, no message | `SE:163-166` |
| Queue | One job per (source, mtime, size), de-duplicated for the life of the canvas pointer; the count of running jobs drives the `indexing PDFs…` label | `SE:196-200`, `SEh:20`, `SB:105` |
| Video notes | No cache: each search loads each video's notes sidecar | `SE:178-181` |
| Limits | No timeout, no cancellation of a running scan, no result cap | `SE:100-194` |

---

## 2. Replace

| Item | Rule | Ref |
|---|---|---|
| Replace one / replace all | **Not found.** No UI, no model function, no menu entry, no shortcut | `SB:1-168`, `Mn:1240-1250`, `Sh:22` |
| The nearest thing | A page-hit jump leaves the match *selected with editor focus*, so typing replaces that one match as ordinary typing over a selection (one undo step, by the editor's own rules). The hit list then goes stale (§1.7) | `Ed:1121-1130`, `SB:63-70` |
| Exclusions (chips, typed cells, code) | Nothing to record: there is no replace | — |

---

## 3. Search across documents, tabs and the canvas

| Item | Rule | Ref |
|---|---|---|
| Across the pages of one file | **Yes — this is the same feature as §1.** All pages, all cards, all PDFs, all video notes of the open file | `SE:112-191` |
| Across open tabs / other files | **Not found**, and ruled out in the header | `SEh:2`, `Mc:254-257` |
| An index | **Not found** (no FTS, no token table). The only persisted search data is the PDF page-text cache | `Doc:635-656` |
| Ranking, filters, sort choices | **Not found** (§1.4) | — |
| Document outline (headings list) | **Not found** as a panel or menu | — |
| "Go to page" | No command. The **path bar** is the navigator: `File › Canvas › Page › Frame`; the last segment drops down to its siblings — the pages and full-frame media under the canvas; the pages plus this page's tables / PDFs / videos under a page. No filter field in those menus | `PB:1-7`, `PB:19-22` |
| Layers panel | The canvas's z-order as rows (front first), ink collapsed into runs, frames collapsible. Click selects, double-click opens. A way to *find an element by eye*; it has no text filter | `LP:1-6`, `LP:14-19` |
| Back / Forward | A browser-style history of visited locations (file, page, frame): chevrons left of the path, `⌥⌘←` / `⌥⌘→`, the mouse's back / forward buttons; capped at 100, per session. A search jump that changes page or frame lands in it like any navigation **(inference** — it records every change of the three location sources) | `NH:1-8`, `NH:21-27`, `TB:78`, `Sh:34` |
| Table row filter | Separate and older: in a table's grid frame, a 180 px field at the right of the tool strip hides body rows where **no** cell contains the text. Substring, case-insensitive (no accent folding), immediate (no debounce), view-only (copy / export / undo see every row), cleared when the frame changes; shows `Showing x of y rows`; `Esc` clears, a second `Esc` leaves the field | `Ed:365-382`, `Ed:7044-7072`, `BM:1321-1333` |
| Choice picker filter | In a table's choice picker the add-field filters the options by lower-cased substring | `ChP:27-29` |

---

## 4. Shortcuts and menu entries

| Item | Rule | Ref |
|---|---|---|
| Menu | `Edit ▸ Find in File…` — the last entry of the Edit menu, after Undo / Redo and a separator. Declared twice: the native menu bar and the in-window themed menu | `Mn:1248-1249`, `Mn:1397` |
| Shortcut | The platform's standard Find key: `⌘F` (Ctrl+F on Windows **(inference** from the standard-key binding)) | `Mn:1248`, `Mn:1397` |
| Enabled | While a document is open | `Mn:1248`, `Mn:1397` |
| Action | `findInFile()` → the bar's `toggle()` (§1.1) | `Mn:1021` |
| Works from | The page editor, the canvas and frame views alike — it is a window-level menu shortcut | `Mn:1248` |
| Undo in the field | The Edit menu's Undo / Redo deliberately carry no key equivalent so that `⌘Z` reaches the search field's own text undo | `Mn:1240-1241` |
| In the bar | `↑` `↓` walk, `Return` jumps, `Esc` closes (§1.6) | `SB:89-94` |
| Find next / previous, replace | **Not found** in any menu or key handler | `Mn:1240-1250`, `SC:17` |
| Listed in the shortcuts catalogue | One row: "Find in file (Edit ▸ Find in File…: pages, cards, PDFs, video notes) — ⌘F" | `SC:17`, `Sh:22` |
| Manual | One section, "Search", describing exactly §1 | `ManB:49-61` |

---

## 5. Highlight layers

| Item | Rule | Ref |
|---|---|---|
| A find highlight layer | **Not found.** No "all matches" tint, no "current match" colour. The only mark is the text selection of the hit just jumped to | `Ed:1121-1130` |
| Spelling / grammar | Wavy underlines drawn per issue by the block's own delegate, in two theme colours. Unrelated to search; nothing is shared | `BV:353` |
| Snippet emphasis | Bold in the result row only | `SE:30` |

So the new app has no old behaviour to match for match highlighting; it is a new design.

---

## 6. What the new app already has

| Piece | State | Ref |
|---|---|---|
| Sidebar "Search" field | **A drawn stub.** A rounded box with a magnifier and the word `Search` in the disabled colour, left of the new-note / new-folder buttons. Its only behaviour is a tooltip, `Search the collection — not yet`. It takes no click, no focus, no text; it does not filter the tree | `sb:34-35`, `sb:102`, `sb:147-158` |
| Footer magnifier | **A disabled stub.** First button of the footer's right group; tooltip `Find in this note (⌘F / Ctrl+F) — not yet`; forced disabled on every sync. A `Do::Find` value exists but nothing handles it | `ft:11-12`, `ft:25`, `ft:47` |
| Menu | No Find entry, no `⌘F` binding | `mnu` (searched) |
| The collection store | One SQLite file: `documents` (a tree of folders, notes, canvases with titles) and `note_blocks` (per note: id, parent, rank, kind, attrs JSON, `text`). Indexes on the tree only. **No FTS table, no search function** | `st:14-34` |
| `Collection` API | `list(parent)` (children in order), `first_line(note)` (first text block's first line, 80 chars, for unnamed notes), `load_note` (whole note as a `Doc`), `save_note` (rewrites changed rows only), `rename`, `create`, `remove`, `block_times`. Nothing reads text across notes; a search today would be `load_note` per note or a new SQL query over `note_blocks.text` | `st:126-207` |
| What the `text` column holds | The block's text; formatting offsets are in `attrs` in UTF-16 units (the editor works in UTF-8 byte offsets, converted by the codec) | `crates/mn-store/src/codec.rs:1-2` |
| Unsaved text | A collection is edited on a private **work copy**; the open note's editor is written to it by autosave and on switching notes, and other notes' editors are kept in memory. A cross-note search must decide whether it reads the work copy (after forcing the open note in) or the in-memory editors | `stg:1-7`, `mn:435`, `mn:1468-1474` |
| Editor find API | **None.** What exists to build on: `Pos { block, at }` / `Selection { anchor, focus }`; `set_selection` (clamps both ends to valid positions); `selection_rects(layout)` (rectangles block by block); `selected_text()` | `ed:23-27`, `ed:94`, `ed:211-225` |
| Rectangles for an arbitrary range | `NoteLayout::range_rects(doc, block, start, end)` — the piece a match-highlight layer needs, independent of the selection | `lay:327` |
| PDF page text | `pdf::text(path, page)` → `PageText { chars: [Char { text, x, y, w, h }], w, h }`, with `words(a, b)` (the text of a character range as words and lines) and `rects(a, b)` (covering rectangles). Used today for selecting / copying text on a shown page; cached in memory per (source, page), at most ~64 entries, filled **only for a page that has been drawn**. No whole-file extraction, no persisted cache | `pdf:14-17`, `pdf:81-118`, `mda:188-193`, `mda:498`, `mda:576`, `mn:351`, `mn:1568` |
| Video notes | `qcv_notes::Note { timecode, frame, text, addressed, … }`; `io::load(media)` reads a video's sidecar. The app loads notes on a helper thread and caches them per media path — for media that is shown | `qn:4-13`, `qio:102`, `mda:155-165` |
| List filter (shared widget) | `ListColumn::with_filter()` adds a "Filter…" field above a list. Matching: the trimmed, lower-cased query as a substring of each item's lower-cased label; section rows are dropped while filtering; empty query = all rows. Picks report the original row index. `Esc` clears and blurs. No accent folding, no highlighting of the matched part | `lc:40-41`, `lc:54-67`, `lc:215-222` |
| Choice picker filter | Same idea: lower-cased substring, placeholder `Find or add…` | `chc:39`, `chc:51` |

Net: on the new side every user-facing search control is a placeholder; the building blocks present are the block
store, range rectangles, selection setting, per-page PDF characters with rectangles, and the notes reader.

---

## 7. Open questions for the redesign

- **Q1. One feature or two?** The old app had a single file-wide search. The new shell shows two controls (footer
  "Find in this note", sidebar "Search the collection"). Are they two tools with different UIs (an in-note find bar with
  highlights; a collection result list), or one engine with two scopes?
- **Q2. In-note find: highlight all matches?** The old app highlighted nothing but a selection. Should the new find
  tint every match and mark the current one (and with which tokens)?
- **Q3. Next / previous.** `Return` / `Shift+Return` and `⌘G` / `⌘⇧G`, with wrap-around and an "n of m" counter? (The
  old app had none of these; `⌘G` meant *group* on the canvas.)
- **Q4. Every occurrence or one per block?** The old engine reported the first match per block / page / note. In-note
  find needs every occurrence; should the collection list also count or show them all?
- **Q5. Options.** Case-sensitive toggle? Whole-word toggle? Regex? (Old: none; always case- and accent-insensitive.)
- **Q6. Accent folding.** Keep it? If so with a length-preserving fold (the old trick) or a proper mapping table from
  folded to original offsets — the new text is UTF-8, where a fold changes byte lengths.
- **Q7. Several words.** Is `a b` one substring (old) or "both words, any order" for the collection search?
- **Q8. Replace.** In or out of this step? If in: replace one / all, scope (selection, note, collection), one undo
  step per "replace all", and what is skipped (choice chips, typed cells — date, timecode, number —, code blocks, link
  addresses).
- **Q9. What is searched in a note.** Link addresses? Media names and captions? File chip paths? (Old: none of these
  for inline media; names and paths only for canvas cards.) Hidden table rows and folded sections?
- **Q10. Note and folder titles.** The old search never matched a page title. Should the collection search match
  titles, and rank a title match above a body match?
- **Q11. Ranking.** Position order (old), or something else: title first, then recency (`modified` exists per block and
  per document), then count of matches?
- **Q12. PDF text.** Search it? If yes: whole-file extraction on a helper thread and a persisted cache (old: a table in
  the file keyed by source + mtime + size), or only pages already drawn? Highlight the match on the page (rectangles
  exist now; the old app could not)?
- **Q13. Video notes.** Search them? Read every sidecar on each search (old), or cache? Should a hit select the note in
  the notes panel as well as seek?
- **Q14. Where results show.** In the sidebar tree (filter the tree to matching notes, with a count), in a list that
  replaces the tree while a query is present, or in a drop-down under the field (old)?
- **Q15. Snippets.** One line of context with the match emphasised (old: 36 characters each side)? Several snippets per
  note?
- **Q16. Search-as-you-type vs Enter.** Old: live, 120 ms debounce, scan on the UI thread. For a collection: live with a
  helper thread and cancellation, or on `Return`?
- **Q17. Index.** Plain scan of `note_blocks.text`, or an FTS table in the collection file (its effect on the file
  format, merge and save)?
- **Q18. Unsaved text.** Does the collection search see the open note's unsaved edits (search the editors in memory) or
  only the work copy?
- **Q19. Live update.** Do results refresh as the note is edited (old: no — stale positions)? Match positions as
  block-id + offset need re-validation after every change.
- **Q20. Seeding and closing.** Does `⌘F` take the current selection as the query? On `Esc`, does the caret return to
  where it was or stay on the current match (old: stays, match selected)? Does the bar close after a jump?
- **Q21. Canvas.** The new collection has a `Canvas` document kind. Is canvas content (text elements, card names) in
  scope of the collection search in this step?
- **Q22. Scrolling to a match.** Bring the match's *line* into view (old brought only the block, and left a tall block
  alone when partly visible) — and centre it or just reveal it?

---

## 8. Oddities worth knowing

| Oddity | What the code does | Ref |
|---|---|---|
| Decomposed accents are not folded | The fold maps each UTF-16 unit to the first unit of its decomposition. A precomposed accented letter becomes its base; a base letter followed by a *separate* combining mark keeps the mark, so text stored decomposed does not match the plain query **(inference** from the loop) | `SE:47-52` |
| Case fold is per unit | No multi-character folds (a sharp s does not match `ss`); characters outside the BMP are folded as two separate surrogates, i.e. not at all | `SE:47-52` |
| One hit per block | The counter under-reports: a note with one long paragraph holding the word ten times says `1 hit`, and there is no way to step to the second occurrence | `SE:120-125`, `SB:105` |
| The list never closes by itself | It has no auto-close policy; after a jump it still covers the top of the destination until `Esc` or the close box | `SB:126-127` |
| `Return` does not advance | Walking a list takes `↓` then `Return` for every hit; once a page-hit jump has moved focus to the editor, `⌘F` is needed to get back to the field | `SB:91-93`, `Ed:1129` |
| Stale hits | Edits, page changes and file changes do not re-run the search (§1.7) | `SB:27`, `SB:75`, `SB:88` |
| UI-thread I/O | Every keystroke's run (after the debounce) stats each PDF and reads each video's notes file on the UI thread; on a slow share this is felt **(inference)** | `SE:165-167`, `SE:180` |
| Page titles and inline media names are invisible | A page cannot be found by its title; an inline image or file chip cannot be found by its name, though the same file as a canvas card can | `SE:117-119`, `SE:134-145` |
| Paths are searched | A canvas card matches on its full source path, so a query equal to a folder name returns every card stored under it | `SE:142-143` |
| A PDF or video used twice | Hits once per use (each inline block and each card is its own entry) | `SE:70-98` |
| The PDF queue forgets only on a canvas change | The de-duplication set is cleared when the engine is pointed at another canvas; a failed extraction is cached as an empty page, so it is not retried until the file's mtime or size changes | `SE:64-68`, `SE:217` |
| Table view filter is ignored | A page hit in a row hidden by the grid filter is still listed; the jump leaves the frame view first **(inference** — what the caret does on a hidden row was not traced) | `SB:68-69`, `Ed:365-382` |
| The placeholder names the scope | The field says what it searches; there is no other scope indication | `SB:98` |
| Test coverage | One regression block (the engine: order, snippet, col / len, cards, video note, PDF cache persistence, folding, blank query) and one in-app probe phase (open, two hits, both jumps, the two-step `Esc`) | `T:6891-6941`, `Ed:5308-5350` |

---

## 9. Not read

- Whether every keystroke in the editor reaches the `blocks` table before the next search runs (the model writes
  content through with a plain UPDATE — `Doc:327-330`, `BM:2434` — but the timing of that write was not traced). If
  it is deferred, the newest typing would be missing from results.
- What `selectInRow` does when the hit's block is a table cell, a block inside a split row, or a row hidden by a
  filter (the caret's cell rules were not followed).
- The stored `content` of split-row record blocks and of table head blocks (whether structural rows can produce
  spurious hits).
- The canvas side of a card jump (`readableCameraFor`, `animateCameraTo`) and the frame views opened for PDF and video
  hits, beyond the calls the bar makes.
- The notes sidecar reader (`app/notes/`), beyond its use here; the Qt PDF text call's line / word ordering.
- Windows-specific behaviour of the themed menu and of the standard Find key.
- Local-only planning notes of the old repository (none were needed; nothing here comes from them).
- On the new side: `apps/mn/src/main.rs` outside the cited lines (key routing, where a find bar would hook in), the
  canvas document kind's storage, and `crates/mn-layout` beyond `range_rects`.
