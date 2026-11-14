# minNotes docs audit: the 2.x manual against the 3.0 application

Audited 2026-11-13. Read-only: nothing in either repository was changed except this file. The application was not
run and nothing was built.

**Summary.** The hosted manual (15 pages, 27 screenshots) describes minNotes 2.x: one file = one canvas with pages on
it, a path bar, an Inspector, a built-in spellchecker, a video "studio", and a read-only archive package. minNotes 3.0
is a rethink (canon 268): a collection is a tree of notes and canvases in a sidebar, saved by hand with a merge;
tables and columns are blocks; media is played, paged and drawn on in place; the spellchecker is gone in favour of
exposing the text to desktop writing tools; the package is unpacked on opening; and there is a new web folder export
with playable video and a password. No page survives as it stands. Two pages are wrong from the first sentence
(`spelling.md`, `basics.md`), five need a substantial rewrite (`canvas.md`, `annotations.md`, `media.md`,
`packages.md`, `shortcuts.md`), the rest need a section-by-section pass. Every one of the 27 screenshots shows the
2.x interface and must be retaken; only the logo can stay. The most urgent corrections are the release facts: 3.0
does not open 2.x (or 1.x) files, and the manual today promises both an upgrade path and double-click opening that
3.0 does not have.

| Page | Wrong statements | Missing 3.0 features | Images to retake |
|---|---|---|---|
| `index.md` | 14 | 7 | 1 of 1 |
| `installation.md` | 6 | 3 | 0 of 0 |
| `basics.md` | 13 | 8 | 3 of 3 |
| `editing.md` | 12 | 8 | 5 of 5 |
| `tables.md` | 9 | 5 | 3 of 3 |
| `media.md` | 11 | 7 | 3 of 3 |
| `annotations.md` | 9 | 5 | 2 of 2 |
| `canvas.md` | 16 | 8 | 5 of 5 |
| `import.md` | 7 | 5 | 1 of 1 |
| `export.md` | 9 | 6 | 3 of 3 |
| `packages.md` | 6 | 3 | 1 of 1 |
| `shortcuts.md` | 17 | 12 | 0 of 0 |
| `spelling.md` | whole page | 2 | 0 of 0 |
| `privacy.md` | 5 | 4 | 0 of 0 |
| `licenses.md` | 13 table rows + 3 sections | 1 | 0 of 0 |

Counts are of the table rows in section 2; one row may cover several sentences of the manual.

## 0. Sources and labels

- 2.x manual: `../minNotes/docs/*.md` read from the `main` branch (`git show main:docs/<file>`), at `adbb0d6`.
  A `2.x` branch and a `3.0-release` branch exist in that repository; `docs/` is identical on `main` and
  `3.0-release` (no diff), so what is audited here is what is hosted.
- 3.0 truth: this repository. `canon N` = row N of the last Decisions table of `docs/rust-wgpu-port.md` (rows
  268-348 are lines 2566-2646). `src/` = `apps/mn/src/`. `handoff A.N` = Addendum N of
  `docs/phase3/handoff-2026-11-10.md`.
- Images were not opened. "Retake" judgements come from the alt text and from what the canon says the interface
  now is.

Evidence labels:
- **[code]** read in the 3.0 source at the cited line.
- **[canon]** a dated ruling or its "built" column says so; the code was not separately read for that point.
- **[absent]** a search of `apps/mn/src` (and the `mn-*` crates) found no such string or handler, and no row of
  268-348 mentions it. Absence is evidence, weaker than a cited line. It is NOT a claim that the feature was dropped
  on purpose unless a row is cited beside it.
- **UNVERIFIED** not checked, or checked without a conclusion.

Many canon rows end with a "NOT tried by hand" list. The manual should describe only what has been walked; the
rows to re-read before writing each page are named in section 5.

---

## 1. The changes that touch many pages

Each is stated once here and cited as G1-G14 below.

| # | Change in 3.0 | Evidence |
|---|---|---|
| G1 | The interface is new: a sidebar tree on the left, a toolbar band over the page, a footer under it, native menus File / Edit / Format / Export / View. No path bar, no Inspector, no Document menu, no Help menu. Every screenshot is stale. | canon 290, 294, 296-300; [code] `src/menu.rs:27-54` |
| G2 | A file (`.mnd`) is a **collection**: a tree of folders, notes and canvases. Notes and canvases are siblings; no canvas is special and a file is not "one canvas holding pages". 2.x "pages" are 3.0 "notes". | canon 271, 272, 290 |
| G3 | One collection per window, one window. No file switcher, no several open files. | canon 296 (2) |
| G4 | Saving: every edit is kept on this machine at once; the document is written only at Save / Save As; a save by someone else meanwhile is **merged** by block (both versions of a paragraph kept when both changed it), never overwritten; work left by a crash is offered back. | canon 272, 285, 288, 293, 295 |
| G5 | 2.x and 1.x files are not read. A 2.x file is refused with "This document was made with an earlier minNotes and can't be opened." A 2.x build refuses a 3.0 file as "newer". Old canvases are not imported. | canon 283 (1, 2), 327 (10, 11), 347 (d), 348 (a); [code] `crates/mn-store/src/file.rs:103-107`, `crates/mn-store/src/old.rs:18` |
| G6 | The built-in spelling and grammar checker is removed. The note's text is exposed to the desktop as one text field so that Grammarly-class tools and OS text services work on it. | canon 268, 269, 276, 281 |
| G7 | There is no full-window "studio", no full-window PDF view, no full-window table and no lightbox inside the application. A video is played, scrubbed, annotated and a PDF paged and drawn on **in its block or card**. (A lightbox exists only in the HTML exports.) | canon 273, 316 (8), 325 (1), 326; [absent] `lightbox` occurs only in `src/export*.rs` |
| G8 | Tables and columns are real blocks. The board (kanban) view is dropped; sorting and the row filter are not built. | canon 273, 317, 319 ("NOT in the step") |
| G9 | Kept files live in a visible `minNotes Media/` folder beside the document, not in the hidden `.minnotes/` (still read, never written). | canon 333 |
| G10 | The package is one `.mnpkg` (a stored zip) that is **unpacked to a folder** when opened and is then an ordinary, editable collection. | canon 334 |
| G11 | Import makes a new note in the open collection, from File > Import only. A file dropped on the page is a picture or a chip, never an import. Evernote and Notion are out. | canon 270, 335 (1-6) |
| G12 | Export: HTML, Web Folder (new), Markdown, Word, PDF, Canvas Poster, Collection as Package, Copy as Markdown; two ticked options in the menu (Include File Paths, Include Video Notes) instead of an options dialog. | [code] `src/menu.rs:46-51`; canon 328, 339 |
| G13 | Nothing appears or disappears as the pointer moves: no hover handles, grips or pills. Handles belong to the caret's block, the caret's table, the selected card. | canon 300, 305, 312, 317 (6) |
| G14 | Release: 3.0.0 replaces 2.x under the same identity and feed; macOS first; Windows not yet run; English only. | canon 347, 348 |

---

## 2. Per page

### index.md (Home)
**Verdict: rewrite** (the lead paragraph and four of the five feature groups).
Images: `mn001.png` ("The canvas: pages, a video, a PDF and ink on one surface") - retake (G1; and the canvas is no
longer the front door, so the hero picture should probably be a note beside the sidebar).
Also `images/minnotes-logo.png` (site logo and favicon, `_config.yml:5`, `_includes/head_custom.html`) - could stay
(canon 347: "the icon is 2.x's").

| Statement | 3.0 | Evidence |
|---|---|---|
| "built around a canvas. A file is an infinite canvas; pages sit on it as cards" | A collection is a tree of notes and canvases; a canvas is one kind of document | G2 |
| "for macOS and Windows" | macOS first; nothing of 3.0 has been run on Windows | G14; canon 347 (e), 348 |
| "open on either platform without the file-locking trouble" | The staging design is kept and stronger (merge on save); Windows half unproven | G4; canon 293 |
| "Drop anything ... other minNotes files, Evernote and Notion exports - each becomes a card" | Media, files, text and web addresses become cards; a note is dragged in from the sidebar. Other collections, Evernote and Notion are not sources | canon 322 (6), 323, 335 (3, 5) |
| "images open in a lightbox in exports" | Still true of the HTML exports | canon 328, 331 |
| "Pen, shapes, arrows and text chips ... one undo per surface" | Pen, line, arrow, rectangle, oval, a rubbing eraser, pressure; text is a note of the canvas; one history per document | canon 324, 323 (7), 275 |
| "syntax-highlighted code" | True again, by a lexer of our own; language list and copy button in the block's corner | canon 337, 338 |
| "Spelling and grammar checking as you type - on-device, US and UK English" | Removed | G6 |
| "Pages are the width of the window, with an optional readable measure" | No full-width switch found | [absent] no "full width" in `src/`; canon 299 ("not in the menus yet") |
| "Blocks side by side: drag any block next to another to make lanes" | True as **columns** (drop beside a top-level block; six at most) | canon 319 (6) |
| "A block rail down the right side" | One handle in the LEFT margin beside the caret's block | canon 305, 306 |
| "Full undo history: the page's timeline, click to time-travel" | Undo and redo only; no history panel | [absent]; canon 275 |
| "Search the whole file: pages, cards, PDF text, video notes" | Find in the note (with replace) and a collection search; PDF text and canvases are not searched | canon 321 (6, 14) |
| "One-click kanban board view"; "Sort ... a tab per table with a row filter" | Dropped / not built | G8 |
| "Import: ... RTF (macOS), Evernote, Notion" | No Evernote, no Notion; old `.doc` added on macOS | canon 335 |
| "Sealed .mnpkg packages ... video plays straight from the archive" | Unpacked on opening | G10 |
| "local files copy into the file's media folder automatically; network shares stay referenced in place" | Still the rule for pictures; other files, videos and PDFs are referenced where they are | canon 311 (3, 7), 315, 316 |
| "Download the latest .dmg from the releases page (github.com/cbkow/minNotes/releases)" | Where 3.0's DMG is published is the user's call; the notices page names tag `minnotes-v3.0.0` in the minplatform repository | canon 348; UNVERIFIED where the DMG will be hosted |

Missing: the sidebar tree and folders (canon 290); merge on save (285); works with Grammarly-class tools (268, 281);
the web folder export with video and a password (339-343); video annotated in the page (316); PDF text selection
(315); Collect Media with its progress window (332).

### installation.md
**Verdict: rewrite the lower half; small edits to the top.** No images.

| Statement | 3.0 | Evidence |
|---|---|---|
| "macOS 13+ (Apple Silicon)" | Unchanged: minimum 13.0, an arm64 FFmpeg | [code] `packaging/macos/make_plist_minnotes.py:18`, `packaging/macos/make-app-minnotes.sh:12` |
| "Windows 10/11 (x64) - The Microsoft Store" | The Store flow is ruled; no 3.0 Windows build exists yet. Until it ships the Store still delivers 2.x | canon 347 (4, e), 348 |
| "Updating from 1.0.3" and "Moving from the old installer (2.0.5 and earlier)" notes | History of 2.x; replace with one "Coming from 2.x" note (section 4) | canon 347, 348 |
| "On macOS, minNotes checks for updates automatically (Sparkle) ... check manually from the app menu" | True: minNotes > Check for Updates...; same feed and key | canon 348 (b); [code] `src/menu.rs:104`, `make_plist_minnotes.py:21` |
| "Upgrading from 1.x ... A 1.x file opens as a canvas ... Save As writes a 2.0 file" | Wrong. Neither 1.x nor 2.x files open | G5 |
| "`.mnd` - A file - a plain SQLite database you can store anywhere" | Still one SQLite file, now a collection (notes and canvases), with `minNotes Media/` beside it | G2, G9 |
| "`.mnpkg` - A sealed package with all assets inside" | A stored zip, unpacked to a folder when opened | G10 |
| "`minnotes://` - Deep links that open a file from other apps" | The scheme is declared in the bundle; handling an address or a double-clicked file is NOT built | [code] `make_plist_minnotes.py:26`; canon 347 (b), 348 ("NOT built: documents handed over by the system") |
| "Double-clicking either file opens it in minNotes" | Not built (as above). Do not state until it is | canon 348 |

Missing: an upgrade note for 2.x users (section 4); "What's new in 3.0"; minNotes > Licenses... (canon 348).

### basics.md (Files & Pages)
**Verdict: rewrite from scratch**, as "Collections, Notes & Folders". The model it explains (one canvas, pages, path
bar) is gone.
Images: `mn004.png` ("The path bar with the file menu open") - retake as the sidebar tree (the path bar does not
exist); `mn036.png` ("The search bar with results") - retake (two pictures: the find bar, the sidebar's results);
`mn006.png` ("The collect offer after Save As") - retake or drop (the offer is now a line in the footer).

| Statement | 3.0 | Evidence |
|---|---|---|
| "A file is a single .mnd - one canvas holding any number of pages" | A collection: a tree of folders, notes, canvases | G2 |
| "Press Esc with nothing selected to step out to the canvas" | Esc selects the caret's whole block | canon 304 (1) |
| "The Path Bar ... File > Canvas > Page > Frame" (whole section) | No path bar. The sidebar lists the tree; a click opens a note or canvas; rename by double click, Enter or the context menu | G1; canon 290, 294, 300 |
| "The file name is the switcher. Click it for every open file" | One collection per window | G3 |
| "A dot before the name marks unsaved changes" | A dot at the end of a changed note's row; the footer's readout says "N notes changed - not saved to the document" | canon 299 |
| "the pin ... View > Always on Top, Opt+Cmd+T" | Not found in rows or code | [absent] |
| "Back and Forward (Opt+Cmd+Left / Right ...)" | Not found; canon 323 (5) says "no Back" yet | [absent] |
| "New (Cmd+N), Open, Save As, Close live in the File menu" | Cmd+N is New Note; New Collection... is Opt+Cmd+N; Shift+Cmd+N New Folder; Cmd+W Close Collection | [code] `src/menu.rs:30-33` |
| "Cmd+F (Edit > Find in File...) ... searches the whole file ... the text inside every PDF ... a video note opens the studio" | Cmd+F finds in the open note (bar at the page's top right, match case, replace). The sidebar's field (Shift+Cmd+F) searches the collection's names, text and video notes. Not searched: PDF text, canvases. No menu item | canon 321; [code] no Find item in `src/menu.rs` |
| "If the file changed on disk while you worked ... Save tells you before overwriting" | It is merged; a dialog reports paragraphs kept twice | G4; canon 295 (3) |
| "Opening 1.0 files" (whole section) | Remove | G5 |
| "Merging Files ... File > Merge File Into This Page..." | Not carried. Blocks move or copy to another note of the same collection (drag to its row, or Move to Note / Copy to Note) | canon 335 (6), 307 |
| "Save As copies the file and its media folder ... offers to collect ... Videos are opt-in ... Document > Collect Media..." | Save As carries the kept files; afterwards the footer says how many files are still referenced outside. Collect Media is in the File menu, videos ticked by default | canon 311 (4), 332, 333; [code] `src/menu.rs:32` |
| "The welcome screen ... offers the same list, plus drag-and-drop import" | A blank window with two buttons, New Collection and Open; no recent list there | canon 296 (1), 298; [code] `src/main.rs:420` |
| "Help > User Guide opens this manual inside minNotes as a canvas ... Cmd+/" | Not found in rows or code; there is no Help menu | [absent]; [code] `src/menu.rs:27-54` |
| "File > Open Recent lists your last ten files" | True (with Clear Menu) | canon 296 (6); [code] `src/menu.rs:134-142` |

Missing: folders and the tree's keys (294); a note's own name (290); notes changed-dot and the footer readout
(299); the close question and the "Unsaved changes were found" dialog (295); merge on save and what a doubled
paragraph looks like (285); find and replace (321); moving blocks between notes (307); files dropped on the
sidebar (345); a new collection is written at once at the place chosen (296 (4)).

### editing.md
**Verdict: substantial edits.** The trigger table and the inline-formatting list survive; the toolbar, page width,
side-by-side, colours and history sections do not.
Images: `mn007.png` ("Blocks and the block rail") - retake (no rail); `mn008.png` ("The toolbar in a page, with the
Insert block flyout open") - retake (no flyouts); `mn009.png` ("The colour palette") - retake; `mn009b.png` ("A
choice chip") - retake; `mn010.png` ("The History section") - drop (no such panel).

| Statement | 3.0 | Evidence |
|---|---|---|
| "Down the right side runs the block rail: a handle for every block" | One grip in the left margin beside the caret's block (or the first selected block). Drag it to move; a line shows the landing place | canon 305, 306; G13 |
| "two flyouts: Heading ... and Insert block" | Buttons, no flyouts: paragraph, heading (press again for the next level), bulleted, numbered, task, quote, code block, outdent, indent, bold, italic, underline, strikethrough, inline code, background colour, type colour, link, clear formatting, table, columns, divider, picture, file, choice chip | [code] `src/toolbar.rs:34-43`; canon 297-299 |
| "Its right end is the same everywhere: the three colour clips, the pen width, undo and redo, the zoom badge, and the Inspector toggle" | Undo, redo, Save, find and the sidebar's button are in the footer. No zoom badge for a note, no Inspector | canon 299, 300; G1 |
| "```` ``` ```` or ```` ```lang ```` then Enter" / "`---` then Enter" / headings, quote, bullets, numbers | True | canon 289 |
| "`- [ ] ` / `- [/] ` / `- [x] `" | The task trigger is `[]`, `[ ]`, `[x]`, `[/]` at the start of a paragraph; whether it also converts after `- ` has made a bullet is UNVERIFIED | canon 289 |
| (no mention) | Inline Markdown converts as the closing marker is typed: `**bold**`, `*italic*`, `` `code` ``, `~~strike~~`, `[label](address)`; one undo gives the typed characters back | canon 289 |
| "Every code block carries two small chips in its top-right corner: Copy ... and the language chip" | True in substance: a language label (a filterable list, "Plain text" when none) and a copy button, always shown | canon 338 |
| "Long lines wrap at the page" | UNVERIFIED | |
| "turn off View > Full Width (Cmd+1 toggles it) ... 640, 760 or 880" | Not found. Cmd+1 toggles the sidebar | [absent]; [code] `src/menu.rs:52` |
| "A table wider than the page runs past its right edge and the page scrolls sideways" | The table scrolls sideways inside its own block | canon 317 (8) |
| "Zoom the view with Cmd+ / Cmd- ... the zoom badge's slider ... pan a zoomed page" | No zoom of a note found; zoom is the canvas's | [absent]; canon 322 (4) |
| "A split row sets blocks side by side in lanes" (section) | A **Columns** block. Made by the toolbar's Columns button or by dropping blocks at a top-level block's left or right end; six columns at most; drag the gap to trade width (neither under 160 px); columns stack on a narrow page; an emptied column goes | canon 317, 319 (6-9) |
| "right-click a block and choose Split into columns, or Cmd-drag from the ... edge" | Not found | [absent] |
| "It snaps at 1/4, 1/3, 1/2 ...; hold Opt to move just this row's"; "Align lanes, Merge with the row below, Delete lane" | Not found | [absent] |
| "Tables always span the page, so a table never lands inside a lane" | A table may sit in a column | canon 273 |
| "The three clips at the right end of the toolbar are the text, highlight and pen colors ... Apply ... Revert ... your own saved swatches" | Roles are Type, Background, Pen (Pen on canvases and media only). A chip always opens one palette: Default, eight named colours, the collection's own colours, + (picker with Add to Palette). No Apply / Revert buttons. Custom colours belong to the collection, not the machine | canon 301 |
| (highlights) | A background is a 35 % tint and never changes the text colour on it | canon 301 |
| "Opt+Cmd+C inserts an inline choice chip" | True; it takes the options of the chip before it and opens its picker; options are editable | canon 320 (11-14) |
| "Each page has its own history and so does the canvas ... The Inspector's History section shows the timeline" | One undo history per note and per canvas; no timeline | canon 275, 290 (2); [absent] |

Missing: whole-block selection (Esc, then arrows, Shift, Cmd+A) and block moves with Ctrl+Cmd+Up / Down (304); the
blocks' context menu with Duplicate, Move to Note, Copy to Note (306, 307); list items travel with their children
(304 (2)); links: Cmd+K editor, Cmd+click opens, typed addresses become links (320); Paste and Match Style,
Paste as Markdown, pasting formatted text and its pictures from other applications (309, 314; `src/menu.rs:37`);
copy writes HTML other applications read (308); code colouring languages and its limits (337); armed formatting
with only a caret (289).

### tables.md (Tables & Kanban)
**Verdict: substantial edits; drop "& Kanban" from the title and the last two sections.**
Images: `mn011.png` ("A table with the grip handles showing") - retake; `mn012.png` ("The bulk selection menu") -
retake; `mn013.png` ("Board view") - drop.

| Statement | 3.0 | Evidence |
|---|---|---|
| "Insert one from the toolbar's Insert block flyout or the block menu (Insert table below)" | The toolbar's Table button; tab-separated text pasted outside a table still becomes one | canon 317 (stage 1), 319 (5) |
| "The header row is the table's spine ... Unassign header (the rows become plain side-by-side blocks) ... Assign as header" | A table is a block of its own and may have no header row. Table <-> columns conversion is not built | canon 273, 317 (9, "NOT in the step") |
| Column types Text / Choice / Checkmark / Timecode | True. Changing a kind asks first when cells would lose content; unreadable timecode text is kept and dimmed | canon 318 |
| "sorts the body by a column" | Not built | canon 317, 319 ("NOT in the step: sorting") |
| "Enter moves down the column; on the last row it adds one" | Enter is a new paragraph in the cell. Cmd+Enter adds a row; Tab past the last cell adds a row | canon 317 (3, 4) |
| "Grip handles appear when you hover just above a column or left of a row" | Handles and "+" strips show only on the table the caret is in | canon 317 (6); G13 |
| "Cmd+Click to add or remove one" (grips); "Drag a column grip to reorder" | Click selects and opens the menu, Shift+click extends. Cmd+click is not mentioned; column moves are in the menu (drag UNVERIFIED) | canon 319 (2), 317 (stage 2) |
| "Cmd+A climbs: the block, its cell, the table, the page"; "Esc steps back down" | Not as written: Cmd+A with blocks selected takes every top-level block; Esc selects the caret's block. Behaviour inside a table UNVERIFIED | canon 304 (1) |
| "Backspace clears a grip selection's contents ... Deleting rows or columns is menu-only, on purpose" | Reversed: Delete empties a cell rectangle but REMOVES whole selected rows / columns (all rows: the table goes) | canon 319 (3) |
| "Cells copied with their header row paste by matching column names; ... Paste into cells fills by position" | Cells paste from the caret's cell rightwards and downwards; rows are added, columns never (what does not fit is left out with a notice). Name matching not found | canon 319 (5) |
| "Tables full-window ... Table / Board toggle ... Filter rows..." (section) | No full-window table, no filter | G7, G8 |
| "Board View" (section) | Dropped | canon 273 |

Missing: a cell holds several ordinary blocks, pictures included (317 (1)); the choice picker (type to filter, Enter
adds an option, Edit options...) (318 (4, 5)); a wide table's own sideways scroll bar (317 (8)); cell rectangles by
drag or Shift+arrow, copied as a table and as spreadsheet text (319 (1, 4)); the import cap of 10 000 cells (335
(12)).

### media.md (Media & Files)
**Verdict: substantial rewrite.**
Images: `mn014.png` ("Media blocks in a page") - retake; `mn015.png` ("The Collect Media dialog") - retake (a new
window with a review face and a progress face); `mn016.png` ("Path mappings") - drop unless a mappings panel is
built.

| Statement | 3.0 | Evidence |
|---|---|---|
| "Images - hover or select one for its outline and drag any corner" | Only the current picture (caret on it, or the one selected block) has the outline and corners | canon 312; G13 |
| "Right-click for Fit width and Original size (double-clicking a corner does the same)" | Original Size only; Fit width is not carried. Double-click on a corner UNVERIFIED | canon 312 |
| "Video - ... a glyph that opens the review studio" | No studio. Click the picture or press Space to play in place; notes are shown, drawn and written in the block; one video plays at a time | canon 316; G7 |
| "the timecode (click it to show frames)" | UNVERIFIED | |
| "PDF - paged inline with prev / next; open full-frame to read, zoom and draw" | Paged inline with a strip; text can be selected and copied; drawn on in the block; no full-frame view | canon 315, 325 (1) |
| "When the source changes the pill reads 'changed'; right-click for Refresh preview" | Replaced: the original is rendered again by itself when its size or time differ | canon 313 |
| "HEIC and RAW previews are macOS-only for now; Photoshop and HDR work everywhere" | HEIC / RAW still macOS only (Windows not wired). SVG previews are new | canon 313 |
| "EXR is deliberately a plain attachment" | Ruled the other way: EXR, DPX, TGA, BMP, TIFF are shown as pictures through the family's decoder | [canon] 310; code not separately read: UNVERIFIED |
| "Illustrator ... is a PDF to minNotes: it pages inline and opens full-frame" | A flat preview of its first artboard | canon 315 |
| "Other files - an attachment chip; double-click to reveal" | A chip (icon, name, size); a second press OPENS the file; menu: Open, Reveal, Copy Path, Locate File. Chips are always references, folders included | canon 320 (6-10) |
| "Reveal, Open in ufb and Copy path" | Reveal in Finder / Show in Explorer and Copy Path on every embedded file; "Open in ufb" not found | canon 344; [absent] |
| "copies it into the file's own media folder (`.minnotes/` beside the file)" | `minNotes Media/` beside the document; the copy waits on this machine and is put there by the next Save | G9; canon 311 (4) |
| "Document > Reveal Media Folder" | Not found; no Document menu | [absent] |
| "Document > Collect Media... (videos opt-in) ... one undo step, and the file saves itself after" | File > Collect Media...; a review list with ticks (videos on), missing files shown with Locate..., room checked, per-file progress, Retry, Stop, Hide; references change at the end and the collection is saved at once | canon 332 |
| "Teach minNotes your share roots once under File > Path Mappings..." | The `{vol, rel}` references and mappings are kept, but no menu item or panel was found: mappings are read from `mappings.json` in the settings folder | [code] `src/media.rs:110`, `src/menu.rs:27-54`; canon 311 (3) |
| "ufb links in exports" | Not found | canon 320 ("NOT in the step: the ufb deep link"); [absent] |

Missing: a picture that is not found, Locate File... relinking its neighbours too (311 (9)); pictures in pasted
content are fetched and kept (314); the 5 GB preview cache on this machine (313); a video keeps the frame it stands
on, and a PDF its page, in the document (315 (3), 316 (4)); HDR only for the playing video (287); loop, speed,
volume, mute remembered (316 (6)); media in use shows no frame or caret (336).
Not found in rows or code: an audio block or card that plays (2.x "Audio - a card with its duration and a play
button"). The collect window has an Audio kind (`src/gather.rs:17`) and the chip an audio icon
(`src/paint.rs:284`); whether an audio file is anything more than a chip is UNVERIFIED.

### annotations.md (Drawing & Video)
**Verdict: substantial rewrite**; fold the keys table into shortcuts.md and consider merging the page into
media.md and canvas.md (section 3).
Images: `mn017.png` ("Ink and shapes beside a page") - retake; `mn020.png` ("The video studio") - drop or replace
with a video block showing its notes row (no studio).

| Statement | 3.0 | Evidence |
|---|---|---|
| "The canvas toolbar holds the pen, line, arrow, rectangle, oval, text box, plain text and eraser tools" | Select, pen, line, arrow, rectangle, oval, eraser, frame; no text-box or plain-text tool (text is made by pressing twice on the empty sheet) | [code] `src/toolbar.rs:88-90`; canon 322 (7), 324 (1) |
| "The eraser removes the stroke under the pointer" | The eraser rubs out the part of a freehand line it passes over; a shape goes whole | canon 324 (5), 274 |
| "Coming from 1.x: ink that was pinned to text ..." | Remove | G5 |
| "A PDF opens full-window from its card, its inline block, or the path bar" | Drawn on directly in the block (a card on a canvas is entered first by pressing it twice); the toolbar shows a drawing set while a picture or PDF block is current | canon 325 (1, 2) |
| "Hold Space to scroll with the hand while a tool is armed" (PDF view) | Applies to the canvas; in a note UNVERIFIED | canon 324 (1) |
| "Open a video full-window for the review studio" | No studio; draw on the frame in the block; the sidecar is still QCView's, byte for byte | canon 316 (8); G7 |
| Keys: Space / K / W / S, Left / Right, Q / E, J / L, A / D, Home / End, R, Shift+R, V, M | True | handoff A.52; [code] `src/main.rs:1638-1660` |
| "Mute - volume: M - Up / Down" | Up / Down leave the block: no volume key in a block | handoff A.52 |
| "Insert notes below ... Notes as a page below" | Not found. A clip's notes show as a row of small pictures under the strip; "notes page below" is listed as not on a canvas | canon 326 (7), 336 (4); [absent] |
| "Streams" (whole section: QCView stream folders, Open as stream, the stream chip) | Not found in rows 268-348 or code. Canon 271 says only that QCView's "Open in minNotes" will open an ordinary collection | [absent] |

Missing: pen pressure from a tablet (324 (3)); drawing on pictures as well as PDF pages (325); picking, moving and
restyling a stroke (325 (7)); pen width 1-64 and colour remembered (324 (2)); writing a note's words and marking it
addressed from the toolbar (`src/toolbar.rs:42`); Shift constrains lines and shapes (327 (9)).

### canvas.md (The Canvas)
**Verdict: rewrite.** Reorder it after the notes pages: a canvas is now optional.
Images: `mn030.png` ("A canvas with pages, media and ink") - retake; `mn034.png` ("Folder, file, link and audio
cards") - retake (folder and audio cards as described were not found); `mn033.png` ("The canvas context menu") -
retake; `mn032.png` ("A selected video with its transport") - retake; `mn018.png` ("Ink and shapes on the canvas") -
retake.

| Statement | 3.0 | Evidence |
|---|---|---|
| "Every file is a canvas. Pages are cards on it" | A canvas is a document in the tree, made with the sidebar's New canvas button; notes are dragged onto it from the sidebar (or Add Note...) and are live cards; one note may be on several canvases | G2; canon 322 (2), 323 (1) |
| "Cmd+1 fits everything in the window and Cmd+0 returns to 100%" | Cmd+0 fits; Shift+Cmd+1 is 100 %; Cmd+1 is the sidebar. Zoom 5-400 % | canon 322 (4) |
| "The zoom badge at the right end of the toolbar" | Controls at the canvas's own bottom right: hand, -, %, +, fit | canon 322 (4) |
| "Double-click a page ... to open it full-window ... Esc there returns to the canvas" | Twice or Return ENTERS the note in its card (camera to 100 %); Esc or a press outside leaves; Cmd+Return or Open Note opens it in the page; no Back | canon 323 (2, 5) |
| "Frames ... a group is elastic ... a board ... the Inspector's Frame section ... tint, the group / board choice" | One kind of frame: it hugs its members until given a size or a shape (16:9, 1:1, 9:16, 4:3), then clips. Title by pressing twice or Rename Frame; tint from the menu (eight named colours) | canon 327 (1, 5) |
| "drag it there holding Cmd ... A plain drag never changes membership" | Reversed: a plain drag over a frame joins it, over the bare sheet leaves it | canon 327 (3) |
| "In the Layers panel ..." and the "Layers." paragraph | No Layers panel | canon 327 (12) |
| "The page tool (N) places a new page" | Not found | [absent] |
| Video card: "Space or Return previews it in the lightbox ... Open in studio" | A selected card shows the strip; a tap of Space plays in the card; pressing twice goes to the card and arms drawing; no lightbox, no studio | canon 326, 336; G7 |
| PDF card: "PageUp / PageDown; Space previews it in the lightbox" | Opt+Left / Right or the menu | canon 323 (9) |
| Image: "Space or a double-click shows it in the lightbox" | Twice (or Return) takes the camera to the card; Esc returns | canon 322 (11) |
| "Audio - A card with its duration and a play button" | Not found | [absent] |
| "Folder ... Spread files here ... Open as stream" | A folder is a file chip; neither command found | canon 320 (7); [absent] |
| "A URL - A bookmark card: favicon, the page's title ... Add link..." | A pasted address is a link card, twice opens it. Favicon / title fetching and "Add link..." not found | canon 323 (12); [absent] |
| "`.mnd` / `.mnpkg` - Its pages copy in"; "Markdown, text, code ... - A new page holding the import"; "Evernote, Notion" | None of these. On a canvas a dropped file is a card. A collection or package dropped on the SIDEBAR is opened; documents dropped there are imported | G11; canon 345 |
| "Cmd+] / Cmd+[ to step ... one layer ... Opt+Cmd+] / [ for front and back" | Cmd+] / Cmd+[ are Bring to Front / Send to Back; no one-layer step | canon 322 (10) |
| "Tools: Hand H, Page N, Text box T, Text plain Shift+T" | V, P, L, A, R, O, E, F only; the hand is Space + drag, the middle button or the corner control. H / N / T not found | [code] `src/toolbar.rs:88-90`; canon 322 (3), 324 (1) |
| "The pen width button opens a slider" | Thinner / Thicker buttons | [code] `src/toolbar.rs:89` |
| "Text boxes take the pen color as their fill ..." (paragraph) | Text on a canvas is a note of the canvas's own, edited with the note editor; Promote to Note moves it into the tree | canon 323 (7) |
| "Export > Canvas as HTML..." | Export > as HTML... writes the canvas when a canvas is open; Canvas Poster (PNG)... is its own row | canon 331; [code] `src/menu.rs:47` |

Missing: snapping with guide lines, Ctrl to switch it off (327 (6)); Align (six ways) and Distribute (327 (7));
Lock / Unlock (327 (8)); Shift constraints (327 (9)); arrow-key nudges by 1 and 10 (322 (8)); the camera kept per
canvas (322 (5)); far-out cards drawn as bare boxes (322 (12)); a deleted note's card says so (323 (6)); canvases
are merged card by card on save (323 (14)); a video card's notes row and words field (336); what is in front of a
playing video covers it (326 (5)).

### import.md
**Verdict: rewrite (it is short).**
Images: `mn024.png` ("The import dialog") - retake or drop (import uses the system's open panel; the only window of
its own is the large-table question).

| Statement | 3.0 | Evidence |
|---|---|---|
| "File > Import... - or drop a file on the welcome screen, on the canvas, or into a page" | File > Import... only (no shortcut), and only with a collection open. A drop on the page or canvas is a picture or chip. A drop on the sidebar imports | canon 335 (1, 4), 345; [code] `src/menu.rs:31` |
| "From the menu ... a file becomes a fresh untitled file" | One file = one new note beside the selected entry (inside it when a folder), named after the file, opened. Several files are read one after another | canon 335 (2) |
| "Plain text (.txt) - Markdown-style prefixes convert on the way in" | Plain text is verbatim paragraphs; only `.md` converts; a `.log` is one code block | canon 335 (8) |
| "Source code - 37 languages" | 38 extensions, one code block, coloured | canon 335 (9), 337 |
| "RTF - macOS only" | True; old `.doc` too, through the system's converter | [code] `src/import.rs:33-42` |
| "Evernote (.enex)", "Notion export (.zip)" | Not carried | canon 270, 335 (3) |
| "minNotes (.mnd / .mnpkg) - Dropped on the canvas: its pages copy in" | Not an import source | canon 335 (5) |
| "use File > Merge File Into This Page..." | Not carried | canon 335 (6) |
| "images referenced by imported documents are copied into the file's media folder" | True: found against the document's folder and copied into `minNotes Media/`; ones not found are counted in the notice | canon 335 (11) |

Missing: the table cap of 10 000 cells and its question (Cancel / Attach File / First Rows) (335 (12)); text colour
and highlight are kept, greys are not (335 (10)); what is not read: review comments, footnotes, cell backgrounds,
column widths (335); Excel dates and formulas' last values (335); "Importing <name>..." in the footer and that an
import is not an undo step (335 (13, 14)). Obsidian-flavoured Markdown is ruled in canon 270; what of it is read is
UNVERIFIED.

### export.md
**Verdict: substantial edits plus one new section (or page) for the web folder.**
Images: `mn035.png` ("A canvas export in a browser") - retake (the exported page changed: no tab strip, video
notes row); `mn025.png` ("The export options dialog") - drop (no dialog; two ticked menu rows); `mn026.png` ("A
video reference card in an HTML export") - retake (paths now sit in a box with a copy button).

| Statement | 3.0 | Evidence |
|---|---|---|
| "The Export menu writes the page you are in, in four formats, the whole file as a package, and the whole canvas" | Add: as Web Folder... (a note, ticked notes, or the whole collection) | G12; canon 339 |
| "Markdown - a portable .md plus a .assets/ folder" | Markdown export exists, drawings drawn into its pictures; the folder's name is UNVERIFIED | canon 328 |
| "HTML - ... a Readable width button in the header" | Not found in rows | UNVERIFIED |
| "Task boxes are checkable, and the browser remembers what you ticked" | Not clickable | canon 328 |
| "syntax-coloured code with its language chip and Copy button" | True again | canon 337 (4), 338 (7) |
| "Word - ... in Geist where it is installed ... table cells are set in 7pt" | Arial / Courier New; real Heading 1-6 styles; per-list numbering; coloured code. 7pt cells not found | canon 329, 337 |
| "PDF - ... a table wider than the page continues in column bands" | A wide table is drawn smaller to fit; no column bands. Text is selectable, links clickable, Geist embedded, A4 or Letter by locale | canon 330 |
| "The canvas as HTML ... A strip across the top holds tabs" | No tabs: a click on a note card opens the whole note over the canvas (Esc returns); keys `+ - 0 1`; shapes and frames are drawn; a video card's note stills in a row | canon 331, 336 (1) |
| "Export Options ... Include video notes ... Include ufb links" | Ticked rows of the Export menu: Include File Paths (on), Include Video Notes (on). ufb links not found | canon 328; [code] `src/menu.rs:50`; [absent] |
| "References: Videos, PDFs, and file attachments export as a poster image plus a reference card" | True of the one-file exports. A file's path sits in a box with a copy button. In a web folder videos play and PDFs / files can be copied in and linked | canon 340, 339 (6) |
| "Copy as Markdown (Shift+Cmd+C)" | True | [code] `src/menu.rs:49` |

Missing: drawings as a separate layer with a switch in the lightbox (328); a video's still is the frame its block
stands on, a PDF's the page shown plus every page drawn on (328); exports are written in the background with
"Exporting <name>..." in the footer (346); the whole web folder (339-343, section 3).

### packages.md
**Verdict: rewrite (short).** Could become a section of a "Sharing a collection" page with Collect Media.
Images: `mn027.png` ("The package export dialog") - retake (it is the Collect Media window with a "One file
(.mnpkg)" switch).

| Statement | 3.0 | Evidence |
|---|---|---|
| "Export > as Package..." | Export > Collection as Package... | [code] `src/menu.rs:48` |
| "Videos are included by default (the dialog shows what they weigh ...)" | A row per file with ticks per row and per kind, sizes against the room at the destination; missing files shown before the start | canon 332 |
| "Folder and bookmark cards travel as cards; a folder's contents are not packed" | Folders, web addresses and link cards are never collected | canon 332 (d) |
| "Double-click a .mnpkg and it opens instantly as a read-only view - media loads lazily ... video plays straight from it" | Open... asks for a folder and unpacks there (progress in the footer), then opens the unpacked collection. Double-click opening is not built | canon 334, 348 |
| "you can type in the view, but Save is disabled - use Save As" | The unpacked collection is an ordinary editable collection | canon 334 |
| "Dropping a package on another file's canvas copies its pages in" | No. Dropped on the sidebar it is opened | canon 345 (1) |
| "video is stored uncompressed so packing is fast" | True (a stored zip, zip64); the document itself is deflated | canon 334 |

Missing: the folder form of the package behind the switch, and why (it can be picked up after a stop) (332, 334);
what happens when a file fails (Retry, Finish Without Them, Cancel Package) (334); a package needs room for one
copy when opened (334).

### shortcuts.md
**Verdict: rewrite the tables from the code.** The source of truth is `src/menu.rs:27-60`, the tooltips in
`src/toolbar.rs:34-95`, and the canon rows cited. No images.

| Statement | 3.0 | Evidence |
|---|---|---|
| "The same list is in the app under Help > Keyboard Shortcuts (Cmd+/)" | No Help menu, no shortcuts card | [absent]; `src/menu.rs` |
| "New file Cmd+N" | New Note Cmd+N; New Folder Shift+Cmd+N; New Collection... Opt+Cmd+N | `src/menu.rs:30` |
| "Close file Cmd+W" | Close Collection | `src/menu.rs:33` |
| "Find in file (Edit > Find in File...) Cmd+F" | Find in the note Cmd+F; next / previous Return or Cmd+G / Shift+Return or Shift+Cmd+G; collection search Shift+Cmd+F | canon 321 |
| "The User Guide ... Cmd+/" | Not found | [absent] |
| "Zoom in / out (the page, a PDF, or the canvas)" | The canvas only | canon 322 (4) |
| "Zoom to 100% Cmd+0"; "Cmd+1 toggle Full Width / fit everything" | Cmd+0 fits the canvas; Shift+Cmd+1 is 100 %; Cmd+1 Sidebar, Cmd+2 Toolbar, Cmd+3 Footer | canon 322 (4); `src/menu.rs:52` |
| "Always on Top Opt+Cmd+T"; "Back / Forward Opt+Cmd+Left / Right" | Not found | [absent] |
| "Select / Hand / Page / Frame tools V / H / N / F"; "Text box - plain text T - Shift+T" | V and F; P / L / A / R / O / E for ink. H, N, T not found | `src/toolbar.rs:88-90` |
| "Drag an element INTO a frame ... Cmd held at the release" | A plain drag | canon 327 (3) |
| "Preview ... in the lightbox Space or Return" | No lightbox; Space (a tap) plays a selected video card; Return goes to / enters the card | canon 326 (2), 322 (11), 323 (2) |
| "Page a selected PDF PageUp / PageDown" | Opt+Left / Right | canon 323 (9) |
| "Step the selection forward / back one layer Cmd+] / Cmd+[; Bring to front / send to back Opt+Cmd+] / [" | Cmd+] front, Cmd+[ back | canon 322 (10) |
| "PDF View (full-window)" (whole table) | No such view; the letters V / P / L / A / R / O / E work while a picture or PDF block is current | canon 325 (1) |
| "Redo Cmd+Shift+Z or Cmd+Y" | Shift+Cmd+Z; Cmd+Y UNVERIFIED | `src/menu.rs:36` |
| "Indent / outdent ... Tab / Shift+Tab, or Cmd+] / Cmd+[" | Tab / Shift+Tab; the bracket keys in a note UNVERIFIED | canon 289 |
| "Next / previous lane Tab / Shift+Tab" | Not found | [absent] |
| "New line in a code block Shift+Enter" | UNVERIFIED | |
| "Step out of the page to the canvas Esc" | Esc selects the caret's block | canon 304 (1) |
| "Down the column Enter"; "Select the block -> its cell -> the table -> the page Cmd+A"; "Add / remove a row or column Cmd+Click"; "A selected row -> the whole table -> the caret below it Esc" | See tables.md above | canon 317 (4), 319 |
| "Volume Up / Down" | Not in a block | handoff A.52 |
| Inline formatting table (Cmd+B, I, U, Shift+X, E, K, Shift+H, backslash) | True; Shift+Cmd+H applies the last Background colour | `src/menu.rs:41-43`; canon 301 |

Missing keys to add: Save As Shift+Cmd+S (true as is); Paste and Match Style Shift+Cmd+V (`src/menu.rs:37`);
Duplicate Cmd+D (306); move blocks Ctrl+Cmd+Up / Down (304 (3)); Esc / arrows / Shift for whole-block selection
(304 (1)); Cmd+Enter a row below (317 (4)); Opt+Down opens a chip's picker (320 (14)); Cmd+click opens a link (320
(1)); Group Cmd+G / Ungroup Shift+Cmd+G on a canvas (327 (2)) against Cmd+G = find next in a note (321 (4)); arrows
nudge cards by 1, Shift by 10 (322 (8)); Cmd+Return opens a card's note in the page (323 (5)); the tree's keys:
arrows, Enter renames, Delete asks (294); Opt held while dropping blocks on a note's row copies them (307); Ctrl
held switches snapping off (327 (6)).
Windows: the page says "on Windows use Ctrl". The Windows in-window menu bar is not built (canon 296); leave the
sentence until the Windows build exists.

### spelling.md (Spelling & Grammar)
**Verdict: delete and replace** with a short page "Writing tools (Grammarly and the system's)". No images.

Every section is wrong: the dictionary, the red / blue underlines, Add to dictionary, `spelling/user.dic`, Ignore
rule, Document > Check Spelling / Check Grammar / Reset Ignored Rules, the 1,700 rules, SCOWL and LanguageTool
(canon 269: "minNotes' built-in spellcheck goes"; [absent] no spell-checking code in `apps/mn/src` or the `mn-*`
crates; no Document menu in `src/menu.rs`).

What 3.0 has instead:
- The note's text is shown to the desktop as one text field, so a writing tool installed on the machine can check it
  and its corrections arrive as ordinary typing (canon 268, 276, 281; `src/text_element.rs`). The macOS side was
  proven with Grammarly in spike S9; the Windows side is designed (a UI Automation provider) but 3.0 has not been
  run on Windows. A PDF's text is not shown to such tools (canon 315 (1)).
- Canon 282 rules a floor for macOS users without a third-party tool: the system's spellchecker and Apple's Writing
  Tools panel. No `NSSpellChecker` or Writing Tools code was found in `apps/mn/src`: treat it as **not built /
  UNVERIFIED** and do not document it.

### privacy.md
**Verdict: section edits.** No images.

| Statement | 3.0 | Evidence |
|---|---|---|
| "Network access ... the update check (Sparkle fetching the appcast from minnotes.app)" | True, same feed; automatic checks are on (no settings switch) | canon 348 (b) |
| "downloading an image when you paste a remote image URL" | Wider: pictures named in pasted or imported content (HTML, Markdown) are fetched when the block is shown, with a plain User-Agent, and kept with the collection | canon 314, 335 (11) |
| "one request to that page for its title and icon" (link cards) | Not found | [absent] |
| "Spelling and grammar checking run fully on your machine" | Remove; see below | G6 |
| "a custom dictionary of words you added" | Remove | G6 |
| "temporary working copies of the files you have open, kept in the app's own data folder" | True (the staged session under the application-support folder); also a preview cache of up to 5 GB and the settings files | canon 288, 313 |
| "On Windows, the Microsoft Store ..."; "a folder left by the old Windows installer" | Keep for 2.x users until 3.0 ships on Windows | G14 |
| "Exports are yours ... A browser viewing an HTML export remembers its own view settings (readable width, checked tasks)" | Task boxes are not clickable; readable width UNVERIFIED. A password-protected web folder keeps its key in the tab's session storage | canon 328, 342 |
| "the complete source is at github.com/cbkow/minNotes" | 3.0's source is in the minplatform repository; the minNotes repository's `main` keeps the site, with 2.x on a `2.x` branch | canon 348 (d) |

To add (each follows from a ruling; the wording is a suggestion, not a ruled text):
- The note's text is readable by assistive and writing tools the user has installed (canon 276, 281). What such a
  tool sends to its maker is that tool's matter; minNotes itself sends nothing.
- Web folder password: pages are sealed (AES-256-GCM, PBKDF2 at 600,000 rounds); **media files are not sealed**,
  only given names the sealed pages know; no recovery (canon 342, stated to and accepted by the user).
- Video for a web folder is made on the machine by the bundled FFmpeg program; nothing is uploaded (canon 340).
- A file's full path is written into exports when Include File Paths is ticked (on by default) (canon 328, 340).

### licenses.md
**Verdict: replace the table from the generated notices.** No images.

The 3.0 page is generated (`packaging/notices/make_notices.py --app mn`; canon 348 (c): 16 components, 305 crates)
and shipped in the bundle; the application shows it from minNotes > Licenses... (`src/menu.rs:104`).

| 2.x row | 3.0 | Evidence |
|---|---|---|
| Qt 6 | Gone (Rust + wgpu) | canon 268, 287 |
| FFmpeg "LGPL-2.1+" "video decoding" | GPL-3.0-or-later as built; also runs as a program for web-folder video | [code] `packaging/notices/components.json`; canon 340, 347 (6) |
| KSyntaxHighlighting | Gone: a lexer of our own | canon 337 (1) |
| Hunspell, SCOWL, LanguageTool (rows and the three long sections) | Gone | canon 269 |
| miniz, stb_image, psd_sdk | Not in the minNotes component list; the PSD reader is our own (`src/psd.rs`) | canon 313; `components.json` |
| ink-stroke-modeler, SoundTouch | Not in the component list. What smooths strokes and keeps pitch now is in the crate list: UNVERIFIED | `components.json` |
| SQLite | Still the store (through a crate; listed with the crates) | canon 272 |
| Sparkle | Still | `components.json`; canon 348 |
| WinSparkle | 2.x history only | G14 |
| Phosphor Icons; Geist / Geist Mono | Still | `components.json` |
| Aspekta, PT Serif, Inter, JetBrains Mono, Lora | Not in the component list | `components.json` |
| "`Contents/Resources/Licenses` on macOS, the `Licenses` folder beside the executable on Windows" | One generated page in the bundle; exact path UNVERIFIED | canon 348 |
| "minNotes is free software under the GNU GPL-3.0-or-later (github.com/cbkow/minNotes/blob/main/LICENSE)" | Licence unchanged in kind; the link target after the repository cleanup is UNVERIFIED | canon 348 (d) |

New in the 3.0 list (from `components.json` entries that apply to minNotes): OpenEXR, Imath, libdeflate, OpenJPH,
Zstandard, libjpeg-turbo, LibTIFF, XZ Utils, bzip2, libspng, dav1d, SRT, Mbed TLS, zlib, minColor, and (Windows)
the DirectX Shader Compiler and the Visual C++ runtime; plus the Rust crates. Recommendation: let licenses.md be a
short page that links to, or is generated from, the same notices the application ships, as QCView's is.

---

## 3. Pages to add, merge or drop

### Add
1. **Collections, notes & folders** (replaces basics.md): the sidebar tree, names, folders, the footer's readout,
   New Collection / Open / Save / Save As, one collection per window. Rows 271, 272, 290, 294-300.
2. **Saving & working on shared storage**: kept on this machine at once, Save publishes, merge on save and the
   doubled paragraph, the four dialogs, what "missing" and "unreachable" mean. Rows 285, 288, 293, 295. This is the
   feature most worth explaining and the manual has one paragraph on it.
3. **Web folder export**: what goes (this note, ticked notes, the collection), the folder's layout, the contents
   list and search, playable video (the size cap, HDR and linear clips through the colour stage, why some clips
   stay stills), the notes caption row, paths with a copy button, the password and its stated limits, re-exporting
   into the same folder. Rows 339-343. Needs its own screenshots (the window, a page in a browser, the lock form).
4. **Code blocks**: the fence, the language list, the copy button, which exports carry colour (editor, HTML, Word,
   PDF, canvas HTML; not Markdown), the 5,000-line limit, what the lexer does not understand. Rows 337, 338. Could
   be a section of editing.md rather than a page.
5. **Collect Media & packages** (replaces packages.md and the last third of media.md): the shared window, its two
   faces, `minNotes Media/`, one-file `.mnpkg` against the folder, opening a package. Rows 332-334.
6. **Writing tools** (replaces spelling.md): see spelling.md above. Rows 268, 269, 276, 281.
7. **Blocks: select, move, copy** (or a large section of editing.md): rows 304-309.
8. **Links and chips**: rows 320. Could be a section of editing.md.
9. **Coming from minNotes 2** : what does not carry over (files, canvases, the spellchecker, the board view, the
   studio, Evernote / Notion), where things moved. Section 4 below is its outline.

### Merge
- `annotations.md` into `media.md` (drawing on pictures, PDFs and video frames is now a property of the block) and
  `canvas.md` (the canvas's ink). Its keys table goes to shortcuts.md.
- `packages.md` into the new Collect Media & packages page.
- `import.md` and `export.md` can stay separate; the web folder is long enough for its own page.

### Drop
- `spelling.md` (replaced by Writing tools).
- The sections "Opening 1.0 files", "Merging Files", "The User Guide" (basics.md); "Tables full-window", "Board
  View" (tables.md); "Streams" (annotations.md); "Cross-OS Paths" as written (media.md) unless a mappings panel is
  built - ask the user, since the mechanism is still there (`src/media.rs:110`).

### Suggested order of work
1. `installation.md`, the index's "Get Going" block and a "Coming from minNotes 2" page: these must be right on the
   day 3.0.0 appears on the feed (section 4).
2. `privacy.md` and `licenses.md`: short, factual, and the licences page can be generated.
3. Collections / saving (new 1 and 2), then `editing.md` with blocks, links, code.
4. `tables.md`, `media.md` (with annotations merged in).
5. `canvas.md`.
6. `import.md`, `export.md`, the web folder page, Collect Media & packages.
7. `shortcuts.md` last, from `src/menu.rs` and the tooltips, when the other pages have settled the names.
8. `index.md` and all screenshots at the end, in one sitting, on a release build with synthetic content only.
Before each page: walk the "NOT tried by hand" items of its rows (332, 334, 338-343, 345 have long lists).

---

## 4. Platforms, installation, updating, file formats and privacy: what the release decisions change

Rulings: canon 347 and 348 (2026-11-13), with 283, 327 and 333-335 for the formats.

**Identity and updating**
- 3.0.0 has 2.x's identity (bundle id `app.minnotes`, the `.mnd` / `.mnpkg` types, the `minnotes:` scheme) and
  2.x's Sparkle feed `https://minnotes.app/appcast.xml` and key (`make_plist_minnotes.py:14-27`). A 2.x user on
  macOS will be OFFERED 3.0.0 by the updater they already have. `installation.md` "Updates" stays true in its
  words; what it must add is what the update does to their files (next block).
- The appcast lives in the manual's own folder (`docs/appcast.xml`, latest entry 2.0.6). The 3.0.0 entry and its
  release notes are part of this documentation work; the notes should carry the file warning. `appcast-win.xml`
  serves only the 2.x Windows installer builds up to 2.0.5 and needs no 3.0 entry.
- The updater has not been run (canon 348, "NOT tried"). Do not write "updates work normally" from this audit.
- `installation.md` "Updating from 1.0.3" and "Moving from the old installer" are 2.x history: move to an
  archive note or delete.

**Old files (the most important correction)**
- 3.0 does not read 2.x files and no converter is owed (canon 348 (a): "no worries about the old app and the old
  notes"); 1.x files were dropped earlier (canon 283 (2)); old canvases are not imported (canon 327). The store
  refuses a 2.x `.mnd` with "This document was made with an earlier minNotes and can't be opened."
  (`crates/mn-store/src/file.rs:106`, `old.rs:18`).
- Wrong today: `installation.md` "Upgrading from 1.x"; `basics.md` "Opening 1.0 files"; `annotations.md` "Coming
  from 1.x"; `index.md` "open on either platform"; `packages.md` (a 2.x `.mnpkg` is a different archive: whether
  3.0 refuses it cleanly is UNVERIFIED).
- The manual should say, on the installation page and in the appcast notes: 3.0 starts new collections; documents
  made with 2.x stay readable only in 2.x; where 2.0.6 can still be downloaded; and that exporting from 2.x
  (Markdown, HTML, Word) and importing into 3.0 is the only bridge (File > Import reads Markdown, HTML and Word;
  canon 335 says Word import takes back minNotes' own export - of 3.0; a 2.x export read by 3.0 is UNVERIFIED).
  Whether to keep a 2.0.6 download link is the user's ruling, not made in rows 347-348.
- The extension is the same for both generations (`.mnd`, format mark `mnd2`, schema 3 against 2; canon 283). The
  manual must not suggest the extension tells them apart.

**File formats as they now are**
- `.mnd`: one SQLite file holding a collection (notes, canvases, folders, the collection's colours). Beside it,
  `minNotes Media/` (visible). The hidden `.minnotes/` of earlier 3.0 builds is still read (canon 333) - an
  internal detail, not for the manual.
- `.mnpkg`: a stored zip with the media and the document, unpacked on opening (canon 334).
- `.qcview` sidecars for video notes: unchanged, shared with QCView (canon 316).
- `installation.md` "File Types" says double-clicking opens either file and that `minnotes://` links open a file.
  The types and scheme are declared, but opening a document handed over by the system is NOT built (canon 347 (b),
  348). Either it is built before release or the sentences go.

**Platforms**
- macOS first, a notarized DMG, macOS 13+, Apple Silicon (canon 347 (4); plist minimum 13.0; arm64 FFmpeg). The
  Developer ID signature and notarization are the user's and have not happened in the rows read: "signed and
  notarized - it launches without warnings" is true of 2.x and is to be re-confirmed for 3.0.
- Windows: through the Microsoft Store, the flow 2.x has, "after its first run there" (canon 347 (4)). Nothing of
  minNotes 3.0 has been run on Windows; the in-window menu bar is not built (canon 296); HEIC / RAW previews are not
  wired (canon 313); the web folder's Windows encoder path is untried (canon 340). Until the Store carries 3.0 the
  manual describes an application Windows users cannot install. Suggested: a banner on each page, "This manual
  describes minNotes 3 (macOS). Windows users have minNotes 2 from the Store until 3 arrives there", and the 2.x
  manual kept reachable (a `/2/` folder, or the `2.x` branch). Whether to keep the 2.x manual online is not ruled.
- Every "Ctrl on Windows", "Explorer" and `%APPDATA%` sentence is unverified for 3.0.

**Language**
- English only (canon 347 (8)); 3.0's strings are plain English in `apps/mn` (canon 295). The manual is English
  and makes no claim about interface languages, so nothing changes in it. Whether the Store listing or the 2.x
  application advertised other languages was not checked: UNVERIFIED.

**Privacy**
- See privacy.md in section 2. The release-driven points: the update check is unchanged; the spellchecker
  paragraph and the custom dictionary go; pasted and imported content can cause picture downloads (canon 314); text
  is exposed to installed writing tools (canon 281); a password-protected web folder does not protect its media
  (canon 342); the source link moves (canon 348 (d)).

---

## 5. Not verified, and where to look before writing

| Point | Why it matters | Where |
|---|---|---|
| The macOS system spellchecker and Writing Tools (canon 282) | Ruled, no code found; decides whether the Writing tools page has a second half | `apps/mn/src/text_element.rs`; ask |
| Audio files in a note or on a canvas | 2.x documents an audio card; 3.0 rows name audio only as a kind in Collect Media | `src/media.rs`, `src/paint.rs:284` |
| Path mappings: is there, or will there be, a panel | media.md's "Cross-OS Paths" section | `src/media.rs:110`, `crates/min-paths/src/map.rs` |
| Cmd+N in a blank window making a collection (canon 309 (c), "not built" then) | basics / shortcuts | `src/main.rs` key handler |
| Cmd+Y, Shift+Enter in a code block, Cmd+] / [ in a note, timecode <-> frames click, double-click on a picture's corner, long code lines wrapping | shortcuts.md, editing.md | `src/main.rs`, `crates/mn-edit` |
| Markdown export's assets folder name; HTML export's "Readable width"; Word's table type size | export.md | `src/export.rs`, `src/export_docx.rs` |
| Obsidian-flavoured Markdown on import (canon 270) | import.md | `src/paste.rs`, `src/import.rs` |
| EXR / DPX shown as pictures (canon 310, [canon] only) | media.md | `src/media.rs` |
| What 3.0 does with a 2.x `.mnpkg` | installation / packages | `src/gather.rs` `unpack` |
| Where the 3.0 DMG is published and the release tag (`minnotes-v3.0.0` is proposed, canon 348) | index.md, installation.md | the user's ruling |
| Everything on Windows | every page | no Windows run exists |
| Link cards fetching a title and icon | privacy.md, canvas.md | `src/canvas.rs:151` |

Rows whose "NOT tried by hand" lists should be walked before their pages are written: 332, 333, 334 (Collect
Media, packages), 337, 338 (code), 339-343 (web folder), 344 (menus), 345 (sidebar drops), 346 (background
exports), 348 (the updater and the two application-menu items).
