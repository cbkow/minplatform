# minNotes export and import — what the Qt app did (Phase 3, export / import step)

Source read: the minNotes repository at commit `59d1129` (2026-11-10, branch `min-paths`), read only. Nothing in it was
built or run; every statement is read from code, from the app's own manual or from its regression test's names, and
anything deduced rather than read is marked **(inference)**. "Not found" means the code was searched and has no such
thing. References are `alias:line` relative to the minNotes repository.

`Ex` = `app/core/Exporter.cpp`, `Exh` = `app/core/Exporter.h`, `CEx` = `app/core/CanvasExporter.cpp`,
`CExh` = `app/core/CanvasExporter.h`, `PE` = `app/core/PackageExporter.cpp`, `PEh` = `app/core/PackageExporter.h`,
`PF` = `app/core/PackageFormat.h`, `PFc` = `app/core/PackageFormat.cpp`, `Im` = `app/core/Importer.cpp`,
`Imh` = `app/core/Importer.h`, `Dx` = `app/core/DocxReader.cpp`, `Dxh` = `app/core/DocxReader.h`,
`Odh` = `app/core/OdfReader.h`, `Od` = `app/core/OdfReader.cpp`, `Xlh` = `app/core/XlsxReader.h`,
`DMh` = `app/core/DocumentMerger.h`, `CD` = `app/core/CanvasDocument.cpp`, `BM` = `app/core/BlockModel.cpp`,
`Mn` = `app/qml/MinNotes/App/Main.qml`, `Ed` = `app/qml/MinNotes/App/Editor.qml`, `ManX` = `docs/export.md`,
`ManI` = `docs/import.md`, `ManP` = `docs/packages.md`, `Sh` = `docs/shortcuts.md`, `T` = `tests/regression_test.cpp`.

Sibling specs this one leans on and does not repeat: `LC` = `step-links-chips-spec.md` (§5: links, inline choice chips
and file chips through every exporter and importer), `V` = `step-video-spec.md` (§9: videos in exports, the package
and Collect Media; §6: the notes sidecar), `Md` = `step-media-spec.md` (the media descriptor, the copy policy),
`TC` = `step-tables-columns-spec.md`. The new side (minplatform at `b320c45`): `tr` = `crates/mn-doc/src/tree.rs`,
`inl` = `crates/mn-doc/src/inline.rs`, `sf` = `crates/mn-store/src/file.rs`, `so` = `crates/mn-store/src/old.rs`,
`lg` = `crates/mn-doc/src/legacy.rs`, `qx` = `crates/qcv-notes/src/export.rs`, `mda` = `apps/mn/src/media.rs`.

Vocabulary. The old app's file is a **canvas** holding **pages**; "the document" in its exporter is one page's block
list, and `documentName` is the *file's* name. A **split row** is the old name for both a columns row and a table row
(a table is a run of split rows with a header count, a "derived table"); a **lane** is a column of a split row or a
table cell. Block-kind names below are the rebuild's (`tr:8-27`) where they differ: Bullet / Numbered / Task (old
`ListItem` / `OrderedListItem` / `TaskListItem`), Columns (old layout split row), Table (old derived table).

---

## 0. The shape everything below rests on

| Item | Rule | Ref |
|---|---|---|
| One walker, four emitters | `Exporter` walks the **loaded** page model (never the file) once per format: Markdown, HTML, DOCX, PDF. Each emitter is a separate hand-written pass over the same block list | `Exh:1-9`, `Ex:690`, `Ex:1600`, `Ex:2484`, `Ex:3321` |
| Scope is one page | Every page format writes the page the model points at — whole, top to bottom. No selection, subtree or multi-page export exists, except Copy as Markdown (a block range) and the canvas exports (every page as a card) | `Ex:599-602`, `Ex:780-797`, `CEx:302-322` |
| Assets go through a sink | Markdown uses a folder sink, HTML a data-URI sink, the clipboard a null sink; DOCX and PDF embed images themselves | `Exh:54-70`, `Ex:542-591`, `Ex:1486-1504`, `Ex:787-792` |
| Two options only | `includeVideoNotes` (default on) and `ufbLinks` (default off, the one persisted preference) | `Exh:45-51`, `Mn:902-904` |
| Pictures travel, big files point | A picture's bytes are always collected into the export; a video, PDF or file chip is a still plus a reference card (name, absolute path on the exporting machine, metadata). Stated reason: sandboxed viewers cannot read absolute paths; large files stay pointers "by design" | `Ex:471-481`, `ManX:77-87` |
| Every page export leads with a name | A quiet mono label, not a heading. It is the **file's** name (`documentName`), although the manual calls it the page's name | `Ex:693-698`, `Ex:1857-1859`, `Ex:2497-2510`, `Ex:3764-3774`, `BM:126`, `ManX:9-11` |
| Atomic writes | Markdown, HTML, PDF and canvas HTML go through a save-file (written beside, swapped in); the package through a temp zip and an atomic replace. DOCX and the PNG poster write the target directly | `Ex:847-851`, `Ex:1940-1944`, `Ex:3822-3833`, `PE:317-327`, `Ex:2908-2921`, `CEx:594` |
| Import converges on one intermediate | Markdown, HTML, RTF and Evernote are read into a rich-text document and walked into block specs by one walker; DOCX, XLSX, ODT / ODS have hand-rolled readers that emit the same specs; CSV / TSV go through the table grid | `Imh:12-29`, `Im:167-265` |
| No ODT, plain-text, RTF or EPUB export | **Not found** | `Mn:1230-1238` |

---

## 1. How export is reached

### 1.1 Menu, shortcut, dialogs

| Item | Rule | Ref |
|---|---|---|
| Menu | A top-level `Export` menu: `as Markdown…`, `as HTML…`, `as Word Document…`, `as PDF…`, `as Package…`, separator, `Canvas as HTML…`, `Canvas Poster (PNG)…`. Page entries need an open document; canvas entries an open canvas. Same list in the native (macOS) and the in-window menu bar | `Mn:1230-1238`, `Mn:1382-1390` |
| Shortcuts | None for any export. `Copy as Markdown` is `⇧⌘C` (document view only) and a `Document` menu row | `Ed:3369-3372`, `Mn:1260-1261`, `Sh:76` |
| Toolbar, context menu | **Not found** | — |
| Pre-scan | Before any dialog the page is scanned: videos, videos with notes, note count, inked PDF pages, and `refPaths` (media blocks that will print a path: every video, PDF and file chip) | `Ex:799-833` |
| Options dialog | Shown **only** when the scan finds video notes or at least one path-printing block; otherwise the save panel opens at once. Rows appear only when they apply: `Include video notes (N notes on M videos)` (reset to on at every export); `Include ufb links under file paths (N paths)` (the persisted preference), with a hint line when on. Buttons `Cancel`, `Export…` | `Mn:302-319`, `Mn:426-545` |
| Save panel | Title per format; default name `<file name or "Untitled">.<md|html|docx|pdf|mnpkg>`, `-canvas.html`, `-poster.png`; the folder is the panel's remembered one; the name is reseeded on every open | `Mn:320-345` |
| Result | Toast `Exported <file> in <folder>`, or `Export failed — <reason>` where the reason is `<path> — <system error>` (DOCX: "the file could not be opened for writing" / "could not be written"; PDF: "the PDF could not be rendered") | `Mn:361-371`, `Ex:848`, `Ex:2910`, `Ex:2921`, `Ex:3831` |
| Canvas exports | No pre-scan, no options dialog: straight to the save panel. Video notes always on; ufb links follow the stored preference | `Mn:306`, `Mn:348-352` |
| Package | Its own pre-scan and its own option (§4) | `Mn:307-312` |
| Persisted | `export/ufbLinks` only. The save panel's folder is the panel's own memory. Nothing else (format, notes option) is kept | `Mn:902-904`, `Mn:304` |

### 1.2 Copy as Markdown and the clipboard's HTML (not exports, same code)

| Item | Rule | Ref |
|---|---|---|
| Copy as Markdown | With a selection: the selected **whole blocks** `loRow..hiRow`, no name header. Without: the whole page with the header. No files are written: pictures and files keep absolute paths (as `file://` URLs), stills that would need a file are simply left out. Toast `Copied block / N blocks / document as Markdown` | `Ed:2996-3010`, `Ex:780-797`, `Ex:116-126` |
| Clipboard HTML | `htmlFragment(lo, hi)` runs the HTML walker in fragment mode (no page chrome, pictures as data URIs); `tableCellsHtml` writes a rows × columns sub-grid as one `<table>`. Used by copy, only when the range holds a table or columns (`LC` §3.6) | `Ex:1863-1927`, `Ed:2966-2967` |

---

## 2. Output on disk, per format

| Format | What is written | Naming and collisions | Ref |
|---|---|---|---|
| Markdown | `<name>.md` (UTF-8) and, created on first need, `<name>.assets/` beside it | Asset names are the source's base name with every character outside `A-Z a-z 0-9 . _ -` replaced by `_`; a clash within one export gets `-2`, `-3`. A copied **file** is written once per source (the same picture in ten cells is one file); generated stills are not deduplicated. An existing file of that name in the folder is deleted and replaced; the folder is never emptied first, so files of an earlier export stay **(inference)** | `Ex:835-852`, `Ex:542-591` |
| HTML | One `.html`, UTF-8, everything inline: fonts, CSS, scripts, every image as a `data:` URI | — | `Ex:1929-1945`, `Ex:1486-1504` |
| DOCX | A zip: `[Content_Types].xml`, `_rels/.rels`, `word/document.xml`, `word/_rels/document.xml.rels`, `word/numbering.xml`, `word/media/image<N>.<png|jpeg>`. No styles part. Links are external relationships | Images numbered in order of use; a task box is a new image per item | `Ex:2832-2923`, `Ex:2097` |
| PDF | One `.pdf`: vector text, embedded images. A4 when the system locale is metric, else Letter; margins 18 / 15 / 18 / 18 mm; creator `minNotes` | — | `Ex:3729-3740`, `Ex:3817-3835` |
| Canvas HTML | One `.html` (the page design plus the canvas chrome), all inline | Default `<file>-canvas.html` | `CEx:574-586`, `Mn:325` |
| Canvas poster | One `.png`, 1600 px wide, height by the canvas's aspect | Default `<file>-poster.png` | `CEx:450-458`, `CEx:588-596`, `Mn:349` |
| Package | One `.mnpkg` zip: `document.mnd`, `manifest.json`, `media/…` (§4) | `.mnpkg` appended when missing | `PF:33-38`, `PE:358-359` |

---

## 3. How each block kind is written

Links, inline choice chips and file chips are in `LC` §5; the video's still, metadata line and notes are in `V` §9.1.
The rows below give the rest and only name those.

### 3.1 Text blocks

| Kind | Markdown | HTML | DOCX | PDF |
|---|---|---|---|---|
| Name header | `` `name` `` then a blank line (backticks stripped) (`Ex:696-698`) | `header.doctitle`: the name and a `Readable width` button; also the `<title>` (`Ex:1856-1857`) | Courier 9 pt line with a grey rule under it, then an empty paragraph (`Ex:2497-2510`) | Mono 8.5 pt line, a rule, 18 units of space (`Ex:3764-3774`) |
| Paragraph | Inline text; a line break inside becomes two spaces + newline. An empty paragraph is an empty block (blank separation) (`Ex:757-767`) | `<p>`; empty → `&nbsp;`; a line break → `<br>` (`Ex:1808-1810`, `Ex:989`) | A paragraph of runs in Geist; 7 pt with tight spacing inside a table cell (`Ex:2763-2778`, `Ex:1973-1981`) | 10.5 pt Geist (Helvetica / Arial fallback); 6 pt in a table cell (`Ex:3477-3487`, `Ex:2939-2940`) |
| Heading 1–6 | `#` × level (`Ex:710-713`) | `<h1>`–`<h6>` (`Ex:1736-1741`) | **Direct formatting**, not a Word style: bold at 20 / 17 / 14.5 / 12 / 11 / 10 pt (`Ex:2332-2338`) | Light weight at 22 / 17.5 / 15 / 13 / 12 / 11 pt, scaled down in a cell (`Ex:3401-3411`) |
| Quote | `> `, repeated on each inner line (`Ex:714-718`) | `<blockquote><p>` (`Ex:1742-1745`) | Italic, left indent 0.5 in (`Ex:2705-2713`) | Left margin 24 (12 in a cell) (`Ex:3417-3425`) |
| Code | A fence one backtick longer than the longest run inside (min 3), the language id on the fence, the text verbatim (`Ex:719-725`) | `pre` holding a Copy button, a language chip (the language's display name) and `code.language-<id>`; tokens as inline-styled spans from the app's dark syntax theme; plain when the language is unknown (`Ex:1746-1756`, `Ex:1218-1260`) | Optional right-aligned muted language line, then one paragraph of Courier New 9 pt runs on `EFEFEF` shading, tokens in the light theme's colours (`Ex:2184-2220`) | A bordered frame on the light theme's background, optional language line, mono 8.5 pt, light-theme colours, wraps anywhere (`Ex:3159-3185`) |
| Bullet | `- ` with 2 spaces of indent per depth; consecutive list items are separated by one newline, not two (`Ex:726-747`, `Ex:769-770`) | Nested `<ul>` by depth (`Ex:1758-1787`) | List paragraph, numbering 1 (bullet `•`), level = depth (`Ex:2717-2739`, `Ex:2789-2828`) | Disc list, indent depth + 1 (`Ex:3434-3459`) |
| Numbered | `N. ` with the number the app shows (`Ex:732-733`) | Nested `<ol>`, `start=` when the first item is not 1 (`Ex:1769-1773`) | Numbering 2 (decimal). One shared numbering for the whole file; the shown number is not written (`Ex:2721`, `Ex:2818-2825`) | Decimal list starting at the shown number (`Ex:3447-3451`) |
| Task | `- [ ] `, `- [/] ` (doing), `- [x] ` (`Ex:734-739`) | `li.task` with a CSS-drawn box carrying the block id and the authored state; done items get a `done` class. Clickable in the browser (§6) (`Ex:1002-1009`, `Ex:1777-1786`) | A bullet item whose text starts with a painted 11 px box image named `mnTask<state>` (the importer reads that name back) (`Ex:2094-2111`, `Ex:2734-2735`) | A list item with the same painted box image (`Ex:3016-3047`, `Ex:3457-3458`) |
| Divider | `---` (`Ex:748-750`) | `<hr>` in a wrapper (`Ex:1789-1791`) | An empty paragraph with a bottom border (`Ex:2740-2750`) | A line image with 10 units above and below (`Ex:3461-3465`) |

### 3.2 Inline marks and colours

| Mark | Markdown | HTML | DOCX | PDF |
|---|---|---|---|---|
| Bold, italic, strike | `**`, `*`, `~~` | `<strong>`, `<em>`, `<s>` | `w:b`, `w:i`, `w:strike` | Weight, italic, strike-out |
| Underline | Raw `<u>…</u>` | `<u>` | `w:u` | Underline |
| Inline code | Backticks, one longer than any run inside; padded when the text starts or ends with one | `<code>` | Courier New on `EFEFEF` | Mono, 1.5 pt smaller, on `EFEFEF` |
| Text colour | **Dropped** | `span style="color"` | `w:color` | Foreground |
| Highlight | **Dropped** | Background + a black or white text colour picked by the swatch's luma | `w:shd` fill | Background + contrast text |
| Link, chip | `LC` §5 | `LC` §5 | `LC` §5 | `LC` §5 |
| Ref | `Ex:160-220` | `Ex:883-945` | `Ex:2120-2159`, `Ex:2278-2296` | `Ex:3053-3102` |

Markdown details: nesting order is link, bold, italic, strike, underline, code (innermost); emphasis never wraps
leading or trailing whitespace (the spaces are moved outside the markers); prose is escaped for `` \ ` * _ [ ] ~ < ``
and nothing else — a line starting with `#` or `>` is not escaped (`Ex:63-82`, `Ex:158-170`, `Ex:229-234`,
`Ex:255-296`). HTML nesting order is link, colour, highlight, bold, italic, strike, underline, code, chip
(`Ex:883-896`). DOCX and PDF give every run its full properties, so there is no nesting (`Ex:2237-2239`,
`Ex:3049-3052`).

### 3.3 Tables and columns

| Item | Markdown | HTML | DOCX | PDF |
|---|---|---|---|---|
| Table | A GFM pipe table. The **first** header row is the header; further header rows become body rows. Column alignment in the rule row (`Ex:663-686`) | `div.tablewrap > table`, fixed layout at the app's pixel column widths (a wide table runs past the page; the page scrolls); header rows in `<thead>` as `<th>` (`Ex:1663-1682`, `Ex:1691-1701`) | One bordered fixed-layout table; pixel widths converted and **scaled down** to the 6.5 in text width; header rows flagged to repeat per page (`Ex:2583-2631`) | Widths from the **content**, not the screen; columns packed into **bands** that fit the page, header rows repeated per page, a key column repeated per band, a muted `columns a–b of n` line before each later band; hairline grid (`Ex:3504-3528`, `Ex:3571-3581`, `Ex:3641`, `Ex:3651`) |
| Trailing columns | Empty untyped trailing columns are trimmed, in every format | | | `Ex:609-624` |
| Cell content | Blocks joined by `<br>`; list items as `• `; a code block as inline code; a divider as `---`; media as in §3.4; `\|` escaped (`Ex:632-661`) | The cell's real blocks (paragraphs, lists, figures) (`Ex:1708-1724`) | The cell's real blocks at 7 pt (`Ex:2632-2653`) | The cell's real blocks at 6 pt; pictures at most 140 units wide (`Ex:3528`, `Ex:3675-3677`) |
| Cell colours | **Dropped** | Background and text colour as inline style (`Ex:1715-1717`) | Shading; the text colour on paragraph runs only (`Ex:2639-2645`, `Ex:2766-2767`) | Background and text colour (`Ex:3504-3511`) |
| Check column | `[ ]` / `[/]` / `[x]` (header rows stay text) (`Ex:636-643`) | The task box, clickable (`Ex:1803-1806`) | The painted box (`Ex:2756-2762`) | The painted box (`Ex:3490-3491`) |
| Choice column | The label | The chip (`LC` §5) | A shaded run | A shaded run |
| Timecode column | As its text **(inference: no case in any emitter)** | | | — |
| Ragged row | — | Missing cells are filled with empty cells (`Ex:1650-1651`, `Ex:1711-1712`) | The same (`Ex:2531-2538`, `Ex:2562`) | — |
| Columns | **No columns**: the lanes are stacked in reading order (`Ex:703-705`, `ManX:39-41`) | `div.lanes` with a `div.lane` per column, flex shares from the stored ratios (`Ex:1703`, `Ex:1726-1728`) | A borderless one-row table at the ratios with a 0.25 in gap; pictures capped to the lane (`Ex:2654-2700`) | A borderless one-row table at the ratios with a 24-unit gap (`Ex:3329-3331`, `Ex:3360-3396`) |

### 3.4 Media

| Kind | Markdown | HTML | DOCX | PDF |
|---|---|---|---|---|
| Picture | `![name](<name>.assets/<file>)`, the original bytes copied; when the source cannot be read, the absolute path as a `file://` URL (`Ex:471-481`) | `figure > img` with the bytes as a data URI; a user-set display width is honoured (`width:Npx`, may exceed the page); unreachable → `src="file://<path>"` (`Ex:1093-1123`) | Inline picture; original bytes for PNG / JPEG, a PNG re-encode for anything else; width capped at 624 px (or the lane / cell) (`Ex:2364-2388`, `Ex:2074-2087`) | The picture at its pixel width, capped to the content width and to **one page** in height (`Ex:3268-3275`, `Ex:2992-3010`) |
| … display width | Ignored | Honoured | Ignored | Ignored |
| … ink on it | **Not written** (the picture is copied clean; `Ex:471-481`) | A **separate transparent PNG layer** stacked over the picture, positioned in percent so ink that overshoots the frame still lands right; the lightbox stages both (`Ex:309-376`, `Ex:1104-1120`) | **Baked** into the picture; ink beyond the frame is clipped (`Ex:2349-2370`) | Baked, clipped the same way (`Ex:3257-3272`) |
| PDF block | Page 1 as an image with its ink baked, then a fence with info string `pdf`: name, path, `N pages`; then one baked image per **other page that carries ink**, captioned `name — page N` (`Ex:499-531`) | `figure.ref`: page 1 with its ink as a layer, caption (name, path, `pdf · N pages`); then a figure per other inked page (clean page + ink layer), captioned `name · page N of M` (`Ex:1138-1203`) | Page 1 (ink baked), bold name, mono path, mono `pdf · N pages`; then each other inked page baked, with its caption (`Ex:2401-2450`) | The same parts, pages at 480 units wide (`Ex:3287-3316`) |
| … pages without ink | Not exported beyond page 1, in every format | | | `Ex:518-522` |
| Video | `V` §9.1: poster (frame 0), reference, optional notes | | | |
| File chip | `LC` §5 | | | |
| Audio | Not a page block kind in the old app (**not found** in the page model); an audio file dropped on a page falls to a file chip **(inference from the routing order, `LC` §2.2)**. The canvas has an audio card (§5) | | | `CEx:408-412` |
| Stills | Rendered at up to 1280 px wide: video frame 0, PDF pages | | | `Ex:488`, `Ex:502`, `Ex:1139` |

### 3.5 Things with no export form

| Item | State | Ref |
|---|---|---|
| Links between pages / notes | **Not found**: the old app has no page-to-page link. A link span's target is written as typed, whatever its scheme (`LC` §1.3) | `Ex:921-922` |
| Ink over text (page-space ink) | Gone from the 2.0 app; the header still lists it as "skipped". No emitter writes any | `Exh:20-22`, `Exh:127-129`, `Ex:1819-1820` |
| Block numbers in the margin | Hooks remain, emit nothing | `Ex:1618-1629` |
| Sketch blocks | Retired; the package walker still packs a sketch's image layers | `PE:157-162` |

---

## 4. The package and Collect Media

`V` §9.2 has the video and sidecar side. The rest:

### 4.1 Package (`.mnpkg`)

| Item | Rule | Ref |
|---|---|---|
| What it is | A sealed snapshot of the **whole file** (every page, every canvas element) with its media inside. Opened read-only; `Save As` makes an editable copy | `PEh:1-9`, `ManP:9-36` |
| Reached | `Export ▸ as Package…`. A pre-scan counts videos; only when there are videos does the options dialog show, with `Include videos (N, size)` and the line "Videos left out stay as references to their original locations." | `Mn:307-312`, `Mn:441-471` |
| Contents | `document.mnd` (a snapshot of the database, compressed), `manifest.json` (`format: mnpkg`, `formatVersion: 3`, a creation stamp), `media/<name>` (stored, uncompressed, written in chunks), `media/.qcview/<name>/…` for a packed video's notes | `PF:33-38`, `PFc:49-51`, `PE:232-295`, `PEh:17-19` |
| What is packed | Every media block of every page and every media element of the canvas whose source resolves to a local file: pictures, PDFs, file chips, audio, videos (when included). **Skipped:** missing or remote sources (the descriptor stays as it was), folder and bookmark cards (references, not assets), excluded videos | `PE:81-104`, `PE:140-183`, `PEh:84-86` |
| What is rewritten | In the **snapshot only**, each packed descriptor's `src` becomes `.minnotes/<packed name>`. The open document is untouched | `PE:107-119`, `PE:186-228`, `PEh:8-9` |
| Names | The readable file name; `-2`, `-3` on a clash, compared case-insensitively; one entry per source | `PE:66-77`, `PE:84` |
| From a package | Exporting while viewing a package splices the source archive's entries across without extracting | `PE:243-254`, `PEh:4-7` |
| Run | Planning and the snapshot on the UI thread; the zip on a worker with byte-weighted progress, the current item's name and `Cancel` (between items; the temp file is removed, the destination untouched) | `PE:349-408`, `Mn:374-424` |
| Messages | `Exported <name>`, `Export cancelled`, `Export failed — Snapshot failed / Package write failed / No path` | `Mn:377-383`, `PE:357`, `PE:367`, `PE:400-403` |
| No size cap | Stated in the manual | `ManP:21-23` |

### 4.2 Collect Media

| Item | Rule | Ref |
|---|---|---|
| What it does | Copies every media file referenced **outside the document's folder** into the document's `.minnotes/` folder and re-points the descriptors at the copies | `PEh:140-151`, `PE:427-465` |
| Reached | `Document ▸ Collect Media…`; also offered, quietly, right after a successful `Save As` | `Mn:1266`, `Mn:107-112` |
| Dialog | "Copies N files (size) referenced outside the document into its media folder, so the document stays whole if the originals move." An `Include videos (N, size)` box, **off** by default, shown when external videos exist. `Collect` is disabled when there is nothing but unticked videos | `Mn:589-654`, `Mn:564` |
| Nothing to do | Toast `All media already lives with this document` (silent after Save As); in a package: `Packages are sealed — use Save As to make an editable copy` | `Mn:554-563` |
| Names | As the packer, also avoiding names already in `.minnotes/` | `PE:435-437` |
| Sidecars | A collected video's notes folder is copied to `.minnotes/.qcview/<name>/`, content untouched | `PE:518-531` |
| Safety | Copies run on a worker; cancel or any failure removes everything copied so far; descriptors are rewritten only after every copy succeeded, as **one undo entry per page** | `PE:480-540`, `PEh:148-151`, `PEh:177-178` |
| Afterwards | The document is **saved automatically** (so the new references and the copies cannot part); an untitled document is not | `Mn:568-582` |
| Messages | `Collected N files`, `Nothing to collect`, `Collect cancelled`, `Collect failed — Unreadable: / Write failed: / Read failed: / Sidecar copy failed: <name>` | `Mn:570-585`, `PE:498-531` |

---

## 5. Canvas export

| Item | Rule | Ref |
|---|---|---|
| Canvas as HTML | One file that opens on the canvas: an absolutely positioned "world" the size of the canvas's bounds plus a 24 px margin, panned by drag or wheel, zoomed by `⌘` / Ctrl-wheel and `+ − 0 1`, fitted on load. A 30 px strip on top holds a `Canvas` tab, a tab per opened card, and a zoom readout with `fit` | `CEx:151-154`, `CEx:217-264`, `CEx:269-277`, `CEx:443-444` |
| Page card | The page export's HTML body (fragment mode) inside a card at the element's rectangle, clipped to the card's height with a fade and `N more blocks — open the page`. A click (never a drag) moves that one copy of the page into a full-window reader tab; `Esc` returns | `CEx:303-322`, `CEx:247-256` |
| Picture card | A figure with the picture; its ink as an overlay layer; opens in the lightbox | `CEx:329-339` |
| Video / PDF card | The poster (frame 0 / page 1) with name, metadata and path; a click opens a reader tab with the large poster, the reference caption, and the clip's notes as a filmstrip, or the PDF's other inked pages. A PDF's page-1 ink shows on the card too | `CEx:340-397` |
| Web-link card | An anchor card: title (or host), host, URL | `CEx:398-401` |
| Folder card | Name, `N items · first entries`, path | `CEx:402-407` |
| Audio card | Name, `m:ss`, path | `CEx:408-412` |
| File card | Name, path | `CEx:413-416` |
| Strokes | Each ink element is an inline **SVG** in element-local pixels: freehand paths, lines, rectangles, ovals, arrows with a drawn head; a single-point stroke is a dot | `CEx:56-79`, `CEx:94-149`, `CEx:417-420` |
| Frames | A tinted rounded rectangle with a title bar, drawn under its children; a child of a fixed-size frame is clipped to the frame | `CEx:294-301`, `CEx:421-425` |
| Text | A positioned text box: plain text in its colour, or a filled box with contrast text; size and padding from the element | `CEx:80-91`, `CEx:426-434` |
| Shapes other than text, connectors | Nothing is written | `CEx:428`, `CEx:436` |
| Paths | Every non-picture card prints its plain path; the `ufb` link only when the preference is on | `CEx:279-293`, `CExh:5-8` |
| Deep links | `#canvas`, `#pm-<id>` (a page), `#m-<id>` (a media reader) | `CEx:219-220`, `CEx:255-262` |
| Poster (PNG) | The canvas scaled to 1600 px wide on the desk colour: pages as sheets with their title and one grey bar per block ("greek" text), media as thumbnails (or the name when none), ink through the app's painter, frames, text. Web-link, folder, audio and file cards draw as a plain box with the name | `CEx:450-551` |

---

## 6. The HTML export's CSS and scripts, in outline

Not transcribed; the rebuild uses its own look.

| Part | What it is | Ref |
|---|---|---|
| Fonts | Nine faces (Geist regular / italic / bold / bold italic / light / medium / medium italic, Geist Mono regular / bold) embedded as base64 `@font-face`; silently falls back to system stacks when the font resources are missing | `Ex:1270-1299` |
| Tokens | One `:root` block of dark colours (ground, text tiers, border, accent, code, a violet for notes, selection, quote), two radii | `Ex:1309-1312` |
| Page | One dark ground; `main` runs the window's width; `html.cap` caps it at the 760 px measure. Type at the app's sizes and line height so lines wrap where the app wraps them | `Ex:1313-1320`, `Ex:1301-1306` |
| Rules for | Paragraph spacing, headings, links, rule, quote, `pre` with its tool strip, inline code, pictures, bullets drawn by `::before`, task boxes in three states, tables, chips, reference cards (`.ref`, `.fname`, `.fpath`, `.fmeta`, `a.ufblink`), note cards, ink stacks (`.inkwrap`, `img.ink`), columns (`.lanes`, `.lane`), the lightbox, a print block | `Ex:1308-1482` |
| Code background | Patched at run time to the syntax theme's background | `Ex:1549-1560` |
| Script 1: lightbox | Collects every figure, note-card and table-cell image (not ink layers); click opens; the stage rebuilds the base and its ink layers with the layers' own percent geometry; arrows and buttons step, `Esc` or the backdrop closes; a counter. Removes itself when there are no images | `Ex:1507-1547` |
| Script 2: readable width | The header button toggles `html.cap`; remembered in `localStorage` (`mn.readableWidth`) | `Ex:1844-1850` |
| Script 3: task boxes | A click cycles to do → doing → done; kept in `localStorage` (`mn.tasks`) keyed by block id with the authored state, and forgotten when a re-export changes the authored state | `Ex:1561-1575` |
| Script 4: code copy | Copies the block's text (async clipboard, hidden-textarea fallback for `file://` pages); the icon shows a check for 1.2 s | `Ex:1579-1590` |
| No ink toggle | Ink layers always show; the earlier "Annotations" switch was removed | `Ex:1819-1821`, `CEx:440-441` |
| Canvas additions | The tab strip, the world, the cards, the reader; one more script (pan / zoom, tabs, hash routing, the width toggle) | `CEx:155-265` |

---

## 7. Import

### 7.1 How it is reached and where it lands

| Route | What happens | Ref |
|---|---|---|
| `File ▸ Import…` | A file panel with a filter per family. A single-document file opens in a **fresh untitled file** (nothing is on disk until saved). A multi-document file (Evernote, Notion) first asks for a destination folder, writes one `.mnd` per note there, and opens the first | `Mn:123-153`, `Mn:158-162`, `Mn:260-269`, `Mn:1217`, `Imh:88-101` |
| Drop on the welcome screen | `.mnd` / `.mnpkg` open; an importable file imports as above | `Mn:1702-1713` |
| Drop or paste of a file into a page | The media router finds an importable format (after video, PDF, preview formats and pictures, before the file-chip fallback), inserts nothing and raises the same flow: a **fresh untitled file**. A `.mnpkg` opens. The manual says it "lands in that page" | `BM:4466-4474`, `Mn:271-284`, `ManI:9-13` |
| Drop on the canvas | A new page at the drop point receives the import. Evernote / Notion: one page per note in a reading-order grid from the drop point, one history step. Several files stagger | `Mn:1027-1048`, `CD:1027-1035`, `Im:1183-1229` |
| Another minNotes file dropped on the canvas | `.mnd` / `.mnpkg`: every element is copied in, arranged as it was, pages through the merge engine, sidecar media re-homed into this file's store, in-place references left as they are. Notices `Copied N page(s) from <name>` / `Couldn't open <name>` | `CD:1027-1030`, `CD:1081-1145` |
| `File ▸ Merge File Into This Page…` | Copies every block of **another open file's page** into this page at a gap; collected media is copied across, linked media passes through; one undo entry. Toast `Merged "<name>" — N blocks (… undoes)` | `Mn:1070-1108`, `Mn:1218`, `DMh:1-8` |
| Shortcut | **Not found** | — |
| Progress | A modal `Importing…` with the file's name and `Cancel` (folder imports stop between notes; a single file's parse finishes) | `Mn:229-258`, `Imh:53-54` |
| Result | `Imported <name>`, `Imported N documents`, `Imported N notes`; failures `Import failed — <error>` with `Unsupported file`, `Read failed`, `Import failed`, `Nothing importable found`, `no notes found`, `not a multi-document file`, `no canvas`. A cancel at the cap dialog reports error `cancelled` | `Mn:166-183`, `Mn:1044-1045`, `Im:136-138`, `Im:400-406`, `Im:462-471`, `Im:1189-1208` |

### 7.2 Sources

The classifier is the file's extension only (`Im:82-115`).

| Source | Extensions | What it maps to | Ref |
|---|---|---|---|
| Markdown | `md`, `markdown`, `mdown` | Read through Qt's Markdown reader (GitHub dialect) into the shared walker (§7.3). `- [/] ` items are tagged before the parse and restored as "doing" tasks. Relative pictures resolve against the file's folder. A table always gets a header (its first row when none is detected) | `Im:170-189`, `Imh:104-107` |
| Plain text | `txt`, `text`, `log` | Decoded (BOM sniff, UTF-8 default) and pushed through the editor's **paste-as-text** rules at the top of the page, so Markdown-style prefixes convert | `Im:404-407`, `Im:518-525`, `Im:43-53`, `ManI:20` |
| CSV / TSV | `csv`; `tsv`, `tab` | One table, first row the header; trailing empty rows and columns trimmed | `Im:199-208`, `ManI:21` |
| HTML | `html`, `htm` | Qt's HTML reader into the walker; `<th>`-only leading rows found by a raw scan become header rows; relative pictures resolve against the file's folder; remote pictures become placeholders fetched after the insert | `Im:190-198`, `Im:538-541`, `Imh:145-147` |
| Word | `docx` | A hand-rolled reader, "the DOCX emitter's mirror": headings by style or outline level, else by the exporter's direct sizes (current and legacy tables); numbering 1 / 2 or the file's own bullet-vs-decimal; task boxes by the image name `mnTask<state>` or a legacy `☐ ` prefix; code = every run Courier on `EFEFEF`; any other fill = highlight; named highlight colours; hyperlinks; pictures by relationship; tables with repeat-header rows, spans, widths, fills, alignment. Review comments are not read | `Dxh:1-10`, `Dx:289-338`, `Dx:357-360`, `Im:209-214` |
| Excel | `xlsx` | Each sheet a table, a level-2 heading per sheet when there are several; pictures in cells both ways Excel stores them. Losses: merged cells keep the top-left value, dates stay serial numbers, formulas import their cached value | `Xlh:1-10`, `Im:254-256` |
| OpenDocument text | `odt` | Paragraphs, headings, lists, bold / italic / underline, links, pictures, inline tables; "Preformatted" paragraphs become code. Losses: colours, numbering restarts, footnotes | `Odh:1-9`, `Od:19-23`, `Od:82-85`, `Im:260-262` |
| OpenDocument sheet | `ods` | Sheets to tables as Excel; sheet-edge padding is clamped | `Odh:7-9`, `Od:13-14`, `Im:257-259` |
| RTF | `rtf` | Only where the OS converter exists (macOS in practice): RTF → HTML → the walker; trailing spaces trimmed. Elsewhere the format is not offered | `Im:93`, `Im:215-238`, `Mn:126-130` |
| Source code | 37 extensions (`py`, `js`, `rs`, `json`, `yaml`, `css`, …) | One code block, language = the extension, line ends normalised, trailing newlines dropped | `Im:60-78`, `Im:239-253` |
| Evernote | `enex` | One document per note (streamed). The title becomes a level-1 heading; the note's markup goes through the HTML route; pictures inline by their hash; other attachments are **written into the document's sidecar** and become file chips; to-dos become tasks (to do / done) | `Im:928-1045`, `Imh:116-119`, `LC` §2.1 |
| Notion | `zip` | Only a zip that holds `.md` or `.csv` entries is claimed; any other zip stays a file chip. Every `.md` page and every stand-alone `.csv` database becomes a document, flat (sub-pages too), names stripped of Notion's 32-hex id, percent-encoded picture links resolved | `Im:104-111`, `Im:1134-1181`, `Im:690-692` |
| minNotes file | `mnd`, `mnpkg` | Never "imported": opened, or copied into a canvas (§7.1) | `Mn:1708-1711`, `CD:1027-1030` |
| PDF, pictures, video | — | Not imports: they are media blocks | `BM:4462-4465` |
| Anything else | — | A file chip | `BM:4475` |

### 7.3 The shared walker (Markdown, HTML, RTF, Evernote, and HTML paste)

| Input | Becomes | Ref |
|---|---|---|
| Rule | Divider | `Im:606-616` |
| Fenced or indented code, `<pre>` | One code block per run of consecutive code lines, text verbatim, the fence's language kept | `Im:587-590`, `Im:617-619` |
| Headings, quotes, bullets, numbered items, nesting depth | The matching block kinds | `Imh:17-22` |
| GFM task items | Tasks (the checkbox marker is read only inside a list: it leaks onto later paragraphs in Qt's reader) | `Im:640-642`, `Imh:23-25` |
| Bold, italic, underline, fixed-pitch, strike | Marks. A heading's own bold is not a mark | `Im:718-744` |
| Anchor | A link mark with the `href` as written. **Inside an anchor only strike is also read**: bold, italic, colour on linked text are lost | `Im:718-719`, `Im:743` |
| Text colour / background | Colour / highlight marks, unless black on white (browsers stamp those on whole documents) or equal to the enclosing cell's background | `Im:602-605`, `Im:727-741` |
| Picture | Its own media block in reading order: embedded data decoded into the store, a local file **referenced in place**, a relative one resolved against the file's folder, a remote one a placeholder fetched afterwards | `Im:594-601`, `Im:700-710`, `Im:368-370` |
| Table | A table: one record per row, each cell's content through the same walker; header rows from `<thead>` or `<th>`; a merged cell's content in its origin, covered cells empty; a nested table flattened to one paragraph per inner row with ` · ` between cells; fixed column widths, cell backgrounds and alignment kept; **at most 63 columns** | `Im:746-753` |
| Excel's clipboard HTML | Floating pictures are mapped to their own cells from the VML shapes before the parse | `Imh:171-178`, `Im:285-362` |
| Inline choice chips, file chips, note links | Nothing recreates them (`LC` §5) | — |

### 7.4 The table cap and "Attach file"

| Item | Rule | Ref |
|---|---|---|
| Cap | 100 000 table cells per imported file (every cell is a block) | `Imh:55-60` |
| When over | The parse is held and a dialog asks: "This table is very large — `<file>` has N rows and M cells. Documents are for tables you read and edit; big data stays in spreadsheets. Import the first K rows, or attach the file to open it in its own app." | `Im:416-427`, `Mn:193-228` |
| `Import first K rows` | Keeps specs while the count fits; the cut lands at a row boundary; **nothing after the cut survives** (later tables and text too) | `Im:442-456`, `Im:463-466` |
| `Attach file` | Appends the file as a file chip at the end of the page | `Im:467-469` |
| `Cancel` | Drops the import (the fresh untitled file stays open, empty **(inference)**) | `Im:470-471`, `Mn:267` |
| Where it applies | The single-file route only. The multi-document and canvas routes apply specs without the check **(inference: `tableCellCount` is called only in `startImportFile`)** | `Im:415-416`, `Im:1220` |

---

## 8. Tests that pin this behaviour

Names only (bodies not read): `testExportMarkdown` (`T:455`), `testImporterWalker` (`T:782`), `testImportFileCores`
(`T:883`), `testPackageFormat` / `testPackageExporter` / `testPackageLifecycle` / `testAsyncPackagePaths` /
`testPackageStreaming` (`T:978-1406`), `testEnexImport` (`T:1517`), `testNotionImport` (`T:1607`),
`testDocxRoundTrip` (`T:1670`), `testRtfImport` (`T:1754`), `testImportCopyPolicy` (`T:1851`), `testCollectMedia`
(`T:1891`), `testPdfPageInkExport` (`T:2090`), `testExportPdf` (`T:2172`), `testNewImportFormats` (`T:2478`),
`testMergeEngine` / `testMergeAssetsAndEdges` (`T:2714`, `T:2822`), `testSplitRowExports` (`T:5102`),
`testCanvasHtml` (`T:6442`), `testPastedHtmlTables` (`T:7982`), `testGridTableExports` / `testGridTableDocxPdf`
(`T:8152`, `T:8235`), `testImportCapAndPackages` (`T:8822`). A canvas-poster test and an HTML-page export test of
their own: **not found** by name.

---

## 9. What the new app has

| Area | State | Ref |
|---|---|---|
| Exporter, importer | None: `apps/mn/src` has no export or import module, and its menu has no such rows | file list of `apps/mn/src` |
| Model | A tree: Paragraph, Heading, Quote, Code, Bullet, Numbered, Task, Divider, Media, Columns ▸ Column, Table ▸ Row ▸ Cell, with typed columns (text, choice, check, timecode). Marks: bold, italic, code, strike, underline, link, colour, highlight, choice | `tr:8-43`, `inl:11` |
| Store | A **collection**: a tree of folders, notes and canvases in one file; a canvas is a list of elements (`note`, `media`, `text`, `ink`, `frame` kinds seen) with a rectangle, a z, a parent and a lock | `sf:53-72`, `apps/mn/src/canvas.rs` |
| Old files | A reader of an old file's pages and a page converter exist | `so:24-37`, `lg:18-88` |
| Paste | HTML and Markdown paste parsers and an HTML copy flavour exist (`LC` §6) — the natural base of an HTML / Markdown importer | `apps/mn/src/paste.rs`, `apps/mn/src/clip.rs` |
| A video's still | The media record carries the card's chosen `frame`; a PDF record its `page` | `mda:275-277` |
| The annotation export (the owner's bar for the HTML look) | Single-file HTML: fonts passed in and embedded as `@font-face` data; a `:root` token set (greys, accent, radii, font stacks) with a light override; the frame and its strokes as two stacked images; a lightbox with a counter, stepping, and a key that hides the stroke layer. Also a Markdown writer producing `<name>/<name>.md` + `images/`, and helpers (`b64`, `esc`, `sanitize`) | `qx:1-5`, `qx:29`, `qx:42-59`, `qx:88-115`, `qx:134-137`, `qx:170`, `qx:184`, `qx:207-217` |

What could be shared with the annotation export: the font embedding, the token block and its light variant, the
lightbox with its separate ink layer, the escaping / base64 / file-name helpers. What is specific to it: the synopsis
table, the brand mark, timecode captions.

---

## 10. Oddities, bugs and limits

Listed for the owner to rule on; nothing here proposes changing the Qt app.

| # | What | Why it looks accidental | Ref |
|---|---|---|---|
| 1 | The name at the top of a page export is the file's name; the manual says the page's | The page model forwards `documentName` to the canvas file; the canvas export uses the page title for the same header | `BM:126`, `Ex:1859`, `CEx:316-321`, `ManX:9-11` |
| 2 | Ink on a picture is written as a layer (HTML), baked (DOCX, PDF) and **dropped** (Markdown), while PDF-page ink is baked in Markdown | The Markdown picture branch copies the file and returns; the header's promised "HTML-comment note" does not exist | `Ex:471-481`, `Ex:499-507`, `Exh:20-22` |
| 3 | The `file://` fallbacks | An unreadable picture exports as a link to a path on the exporting machine (HTML and Markdown), which no reader can open | `Ex:1095`, `Ex:478-480` |
| 4 | Data-URI MIME by extension, default `image/png` | A picture in any other format (TIFF, BMP, HEIC, a preview format) is embedded with the wrong type and may not show | `Ex:1489-1495` |
| 5 | DOCX headings are bold text at a size, not heading styles | Word's navigation pane and Google Docs' outline see no headings; the reader guesses them back by size, and the current and legacy size tables overlap (24 and 22 half-points mean different levels in each) | `Ex:2332-2338`, `Dx:298` |
| 6 | DOCX numbered lists share one numbering | Two separate lists continue each other's count; a list that starts at 5 starts at 1 | `Ex:2721`, `Ex:2818-2825` |
| 7 | DOCX tasks are bullets with a picture | Each item embeds its own PNG; Word shows `• ☐ text` | `Ex:2097`, `Ex:2734-2735` |
| 8 | A picture's display width survives only in HTML | DOCX and PDF use the pixel width up to a cap; Markdown has no size | `Ex:1099-1103`, `Ex:2370`, `Ex:3272` |
| 9 | Markdown tables keep one header row | Further header rows silently become body rows; cell colours and column widths are lost | `Ex:663-664` |
| 10 | A multi-block cell in Markdown is joined with `<br>`, a code block in a cell becomes inline code | A code block's newlines are then turned into `<br>` inside backticks | `Ex:651`, `Ex:656-659` |
| 11 | Markdown escapes inline punctuation only | A paragraph that begins with `#`, `>`, `-` or `1.` changes kind when the file is read back | `Ex:63-66` |
| 12 | `- [/]` is not standard Markdown | Other readers show the literal text; only this app's importer reads it back | `Ex:736`, `Im:55-57` |
| 13 | The `.assets` folder is never cleared | Re-exporting to the same name leaves the earlier export's files beside the new ones **(inference)** | `Ex:547-568` |
| 14 | The pre-scan counts inked PDF pages and nothing uses the count | A leftover of an option that was removed | `Ex:803-830`, `Mn:475-530` |
| 15 | The canvas HTML export has no options | Video notes are always on, and the `ufb` preference can only be changed from a *page* export's dialog — and only when that page has a path-printing block | `Mn:306`, `Mn:349`, `Mn:505-527` |
| 16 | The package's video option | Initialised on at every export; the comment beside the box says "default off — size" (`V` §9.2) | `Mn:292`, `Mn:305`, `Mn:441` |
| 17 | Missing media is skipped by the packer without a word | The package then holds a descriptor pointing at a path of the exporting machine | `PEh:84-86`, `PE:84` |
| 18 | The absolute path of the exporting machine is printed in every reference card | Useful to a colleague on the same volumes; a disclosure to anyone else. No option leaves it out | `Ex:468`, `Ex:1169-1172` |
| 19 | Dropping an importable file into a page opens a new untitled file | The manual says it lands in the page; the code comment says "fresh untitled tab" | `Mn:117-122`, `Mn:278-283`, `ManI:11-13` |
| 20 | Marks on linked text are lost on import | The walker reads the anchor **or** the style marks, never both (strike excepted) | `Im:718-744` |
| 21 | Over the table cap, "first K rows" discards everything after the cut | A file with a small table, then a huge one, then text loses the text | `Im:442-456` |
| 22 | The cap is not applied to multi-document or canvas imports | Only the single-file route counts cells **(inference)** | `Im:415-416`, `Im:1220` |
| 23 | A table wider than 63 columns is cut | A cell index is a signed byte | `Im:753` |
| 24 | Plain text goes through paste rules | A `.log` or `.txt` whose lines start with `#`, `-` or `>` turns into headings, lists and quotes | `Im:518-525` |
| 25 | Any zip with a `.md` or `.csv` inside is "a Notion export" | A project archive dropped on a page becomes many documents instead of a file chip | `Im:104-111` |
| 26 | Evernote attachments are copied into the sidecar | The one place a file chip owns a copy; everywhere else a chip references in place (`LC` §2.1) | `Im:1004-1020` |
| 27 | Evernote to-dos are two-state; Notion sub-pages flatten | By design of the sources, but nothing records the hierarchy | `Im:948-950`, `Im:1138-1139` |
| 28 | DOCX and the poster are not written atomically | A failed write can leave a truncated file where the others would leave the old one | `Ex:2908-2921`, `CEx:594` |
| 29 | PDF: a tall picture is shrunk to one page; a heading can be orphaned at a page's foot; a wide code line wraps | Listed as v1 losses in the header | `Exh:124-129`, `Ex:2998-3002` |
| 30 | PDF paper size follows the system locale | No choice of size or orientation; a wide table has no landscape option | `Ex:3735-3738` |
| 31 | Task boxes in the HTML export change per browser | A reader's ticks live in that browser only; the file itself never changes, so two readers see different states | `Ex:1561-1575` |
| 32 | Pages of a PDF block without ink are never exported | Only page 1 and inked pages appear; a reader cannot tell the others exist except from the page count | `Ex:518-522` |
| 33 | The canvas poster draws web-link, folder, audio and file cards as empty boxes with a name | Only picture, video and PDF have thumbnails | `CEx:508-530` |
| 34 | Rectangles, ovals and other non-text shape elements are missing from the canvas HTML | Only `shape == "text"` is handled | `CEx:426-435` |

---

## 11. Open questions for the redesign

Each carries a proposed default. Already ruled and not raised: old canvases are not imported.

| # | Question | Old behaviour | Proposed default |
|---|---|---|---|
| Q1 | Which export formats does the rebuild carry? | Markdown, HTML, DOCX, PDF; Copy as Markdown; package; canvas HTML and PNG | [proposed] HTML, PDF, DOCX, Markdown and Copy as Markdown (`⇧⌘C`). No ODT, plain text or RTF |
| Q2 | What is the scope of an export, now that a collection is a tree of notes? | One page | [proposed] The current note. A second scope, "this note and the notes under it", for HTML and PDF only (one file, a contents list), built after the single note works. No selection export — Copy as Markdown covers a range |
| Q3 | Where does the HTML look come from? | Its own CSS, shared with the canvas export | [proposed] One shared module for the family: the annotation export's font embedding, token block with its light print variant, lightbox and helpers move there; the notes export adds block rules on top. The annotation export keeps its synopsis table |
| Q4 | Are drawings a separate layer with a switch? | A separate layer in HTML, always shown; the switch was removed | [proposed] A separate layer and the annotation export's switch (one key and a button in the lightbox), since the newer family export has it |
| Q5 | Do task boxes stay clickable in the exported HTML? | Yes, remembered per browser | [proposed] No: boxes show the state at export. A page whose content differs per browser misleads a shared review |
| Q6 | Code blocks: syntax colours, language chip, Copy button? | All three in HTML; colours in DOCX and PDF | [proposed] HTML: all three, using the editor's own highlighter. DOCX, PDF, Markdown: mono text with the language named, no colours ("stay simple") |
| Q7 | The "Readable width" button | A toggle between window width and a 760 px measure | [proposed] No button: the export uses the note's own measure and reflows below it |
| Q8 | What travels and what points? | Pictures embedded; videos, PDFs, files as a still and a reference card | [proposed] The same |
| Q9 | Which still stands for a video and a PDF? | Always frame 0 / page 1 | [proposed] The card's chosen frame / page, as the note shows it |
| Q10 | Which PDF pages are exported? | Page 1, plus every page with ink | [proposed] The shown page, plus every page with ink; the caption says `page N of M` on each |
| Q11 | Video notes in a note export | An option, on by default, offered only when notes exist; per format as `V` §9.1 | [proposed] The same option. HTML: note cards with the layered frame, in the lightbox. PDF, DOCX, Markdown: frame with the drawing baked, timecode, text, addressed mark |
| Q12 | Is the absolute path printed under a reference? | Always | [proposed] An option "Include file paths", on by default and remembered; off prints the name and metadata only |
| Q13 | The `ufb` link line | An option, off, the only remembered one | [proposed] Carried as it was (pending `LC` Q21) |
| Q14 | The options sheet | Shown only when an option applies | [proposed] The same rule; the chosen options are remembered as app settings |
| Q15 | Markdown's layout on disk | `<name>.md` + `<name>.assets/` beside it; the annotation export writes `<name>/<name>.md` + `images/` | [proposed] Keep `<name>.md` + `<name>.assets/` (the file can be placed where it is wanted); the assets folder is emptied of this export's earlier files first |
| Q16 | Markdown's losses | Colour and highlight dropped, underline as `<u>`, chips as labels, columns stacked, one header row, picture ink dropped | [proposed] The same, except that ink on a picture is baked into the copied image, as PDF-page ink already is |
| Q17 | The doing state `[/]` and line-start escaping in Markdown | `[/]` written; line starts not escaped | [proposed] Keep `[/]` (our importer reads it; others degrade to text). Escape a paragraph's leading `#`, `>`, `-`, `+`, `N.` so the kind survives a round trip |
| Q18 | DOCX structure | Direct formatting, shared numbering, image task boxes | [proposed] Real `Heading 1–6` styles, a numbering instance per list with its start value, tasks as a box character in the item text. The importer keeps reading the old markers (`mnTask<state>`, the size tables, Courier + `EFEFEF`) so old exports still come back |
| Q19 | How is the PDF made? | A rich-text document printed to a PDF writer, with a column-band packer for wide tables | [proposed] A spike first (§21.3): lay the note out with the rebuild's own layout at the paper width under a white-paper token set and write pages. First version: A4 / Letter by locale, a wide table scaled to the page (no bands), drawings baked, links clickable |
| Q20 | Paper size and orientation for PDF | Locale decides; portrait only | [proposed] Locale decides the default; the options sheet offers A4 / Letter and portrait / landscape, remembered |
| Q21 | Tables in DOCX and PDF | Small type (7 pt / 6 pt), cell colours kept, header rows repeated | [proposed] The same sizes and the repeated header; cell colours kept |
| Q22 | Canvas export | A pan-and-zoom HTML file with reader tabs; a PNG poster | [proposed] A later step. First the PNG (the canvas renderer drawn off screen at a chosen width), then the HTML world with note cards opening the note export |
| Q23 | Is there a package? | `.mnpkg`: a sealed, read-only snapshot with media inside | [proposed] "Export collection as package": a copy of the collection file with its referenced media gathered beside it in one archive; opening one is a later ruling. Videos included by default, with the count and size shown and a box to leave them out |
| Q24 | Collect Media | A command, plus a quiet offer after Save As; videos opt-in; auto-save afterwards | [proposed] Carried whole, including the offer after Save As and the roll-back on cancel |
| Q25 | Which import sources, in what order? | Fourteen families (§7.2) | [proposed] First: Markdown, plain text, HTML, DOCX, CSV / TSV, source code (the paste parsers are the base). Second: XLSX, ODT / ODS, RTF. Third: Evernote, Notion |
| Q26 | Where does an import land? | A fresh untitled file; a new page on a canvas | [proposed] A new note in the open collection, beside the selected entry, named after the file. Dropped on a canvas: the new note plus its card at the drop point. Dropped into a note's text: the same new note, never spliced into the open one |
| Q27 | Multi-document sources | N files in an OS folder, or N pages on a canvas | [proposed] A new folder in the collection named after the source, one note per document; Notion's page hierarchy kept as sub-notes |
| Q28 | An old minNotes file (`.mnd`, `.mnpkg`) as an import source | Opened, or copied into a canvas | [proposed] `Import…` accepts both: each page becomes a note in a folder named after the file, media references kept, sidecar media copied in |
| Q29 | "Merge File Into This Page" | Copies another open file's page into this one | [proposed] Not carried: copy and paste between notes of one collection covers it |
| Q30 | Plain text | Through paste rules (prefixes convert) | [proposed] Verbatim paragraphs, one per line; a `.md` extension is what asks for conversion |
| Q31 | The table cap | 100 000 cells; a dialog with first-K-rows / Attach file / Cancel; the tail is dropped | [proposed] Keep the dialog and its three choices; set the number from a measurement of the rebuild; when cutting, cut only the oversized table and keep what follows it; apply it on every import route |
| Q32 | Pictures in an imported document | Embedded ones stored; local ones referenced in place; remote ones fetched | [proposed] All copied into the collection's store (an import should not depend on the folder it came from); remote ones fetched in the background with a placeholder |
| Q33 | Marks on linked text, chips, cell colours on import | Lost / never recreated | [proposed] Read link and style marks together. Chips are not recreated from foreign files (pending `LC` Q19) |
| Q34 | An unreadable picture at export time | A `file://` link to the exporting machine | [proposed] A placeholder box with the picture's name and "not available at export", in every format |
| Q35 | The name at the top of an export | The file's name as a quiet mono label | [proposed] The note's title, same quiet label; the HTML `<title>` too |
| Q36 | Links to other notes | No such link existed | [proposed] In a multi-note HTML or PDF export a link to an exported note jumps inside the file; otherwise it is written as the note's title, unlinked |
| Q37 | Audio | A canvas card only (name, duration, path) | [proposed] A reference card like a file's, with the duration when known |
| Q38 | Strings and messages | English literals; toasts `Exported <file> in <folder>` / `Export failed — <reason>` | [proposed] The same wording through `trc` with drafts in the 11 languages; export text inside the files (captions such as `page N of M`, `Notes (N)`, `addressed`) follows the app's language |
| Q39 | Atomic writes | All but DOCX and PNG | [proposed] Every export is written beside the target and swapped in |
| Q40 | Shortcuts | None but `⇧⌘C` | [proposed] `⇧⌘C` carried; `⇧⌘E` opens the export sheet with the last format chosen |

---

## 12. Not read

- The CSS text itself (`Ex:1320-1482`): selectors skimmed, values not recorded.
- The PDF column-band packer beyond its comments (`Ex:3530-3620`) and the PDF block loop's table branch.
- `DocxReader.cpp`, `OdfReader.cpp`, `XlsxReader.cpp` beyond their header comments and the lines cited; the RTF
  converters (`RtfConvert_*.{mm,cpp}`).
- The walker's table branch (`Im:754-860`) and the Excel-picture rewrite (`Im:285-362`) beyond their comments.
- `PackageFormat.cpp` (the zip reader / writer, the manifest beyond its three fields), `DocumentMerger.cpp`,
  `AssetTransfer.cpp`; how a package is opened (`V` and `Md` cover the media side).
- `BlockModel::pasteText`'s rules (the plain-text import's whole behaviour) and `promoteGridRuns`,
  `tableSpecsFromGrid`.
- The bodies of the regression tests (§8 lists names and lines only); no test was run.
- Qt's own Markdown and HTML readers (what dialect details they accept).
- What `blockModel` points at when a canvas has no focused page, i.e. what a page export writes then.
- On the new side: `paste.rs`, `clip.rs`, `old.rs` and `legacy.rs` beyond the lines cited; whether the rebuild's
  highlighter can emit coloured spans for an export.
- Local notes of the old project (plans, design records): not consulted for this spec.
