# Importing minNotes 2.x documents into 3.0 — what a 2.x document is, and what it would take

Read 2026-11-14. Nothing was built or run; no repository's state was changed. This is a spec for a proposal, not a
ruling: canon rows 283 (5), 327 and 348 (a) currently say 3.0 does not read 2.x files.

Sources. 2.x: the minNotes repository, branch `2.x` (= tag `v2.0.6`, commit `adbb0d6`), read from an archive of the
branch. References are `path` + function or constant, relative to that repository. The earlier Phase 3 specs
(`step1-core-spec.md`, `step-media-spec.md`, `step-canvas-ink-spec.md`, `step-video-spec.md`,
`step-links-chips-spec.md`) were read from that repository's `min-paths` branch (`59d1129`); the difference between
the two branches was checked (`git diff --stat 2.x min-paths`): it touches only path classification and the
path-mapping engine (`PathMap.cpp`, `MediaStore.cpp`'s source classifiers, build files) — **`Document.cpp`,
`BlockModel.cpp`, `InlineText.cpp`, `PackageFormat.*`, `CanvasDocument.cpp` and `Converter.cpp` are identical**, so
those specs describe the released 2.x format and this document points at them instead of repeating their tables.
3.0: this repository at `243bde6`.

**The main finding: most of the reader already exists in 3.0.** `crates/mn-store/src/old.rs` (`read_pages`) reads the
pages of a 2.0 file, `crates/mn-doc/src/legacy.rs` (`convert_page`) turns a page's flat block list into a 3.0 note,
and `Collection::import_v2` (`crates/mn-store/src/file.rs`) makes a folder with one note per page. They were built in
step 1 (canon row 283, ruling 5), are covered by tests, and are reachable only through the test switch `--v2`
(`apps/mn/src/main.rs`). No menu, drop or open path leads to them.

---

## 1. The file on disk

| Item | 2.x | Ref (2.x) |
|---|---|---|
| Extensions | `.mnd` (the document), `.mnpkg` (the package). Both are registered to the app on macOS and in the Store package | `packaging/macos/Info.plist`, `packaging/windows/msix/AppxManifest.xml.in` (`minnotesdocument`, `minnotespackage`) |
| `.mnd` container | One SQLite database (WAL while open; Save checkpoints and writes back by rename). The first 16 bytes are `SQLite format 3\0` | `app/core/Document.cpp` `Document::open`, `checkpoint`; README "Files are SQLite databases" |
| Tables | `blocks` (id, rank, depth, type, attrs, content, created, modified, page_id), `doc_meta` (one row), `elements`, `canvas_meta`; optional `pdf_text` (a search cache, safe to ignore); inert leftovers in older files: `comment_threads`, `comment_messages`; in 1.x files `block_ink` | `Document::open`, `ensureCanvasSchema`, `setPdfTextCache`; comment in `Document::open` |
| Version marks | `doc_meta.format = 'mnd2'`, `doc_meta.schema_version = 2`, `app_version` = the build that last saved | `Document.h` `kFormat`, `kSchemaVersion`; `Document::stampMeta` |
| `.mnpkg` container | A zip: `manifest.json` (`{"format":"mnpkg","formatVersion":3,"appVersion","created","mediaCount","mediaBytes"}`), `document.mnd` (deflated), `media/…` (stored; unpacked it is the document's `.minnotes/` folder; a packed clip's notes ride at `media/.qcview/<clip>/`) | `app/core/PackageFormat.h` (`kFormatVersion = 3`, `kDbEntry`, `kManifestEntry`, `kMediaDir`), `PackageFormat.cpp` `makeManifest`; gate in `CanvasDocument.cpp` `loadFile` |
| Beside a `.mnd` | A hidden `.minnotes/` folder: pasted / copied pictures, previews, collected media, content-hash names | `app/core/MediaStore.h` class comment, `assetsDir` |

Telling the generations apart (all three are SQLite files named `.mnd`):

| File | `doc_meta.format` | `schema_version` | Structure |
|---|---|---|---|
| 1.x | `mnd` | 1 | `blocks` without `page_id`; no `elements`; often `block_ink` |
| 2.x | `mnd2` | ≤ 2 | `elements`, `canvas_meta`, `blocks.page_id` |
| 3.0 | `mnd2` | 3 | `documents`, `note_blocks`, `canvas_elements`, `palette` (`crates/mn-store/src/file.rs` `DDL`, `FORMAT`, `SCHEMA`) |

The first bytes do not tell them apart; the `doc_meta` row does. 3.0 already reads the marks before anything else
(`mn_store::old::marks`) and answers a 2.x file with `Gate::Earlier` — "This document was made with an earlier
minNotes and can't be opened." (`Collection::open`, `file.rs`). The packages differ by layout: a 2.x package has
`manifest.json` + `document.mnd` + `media/`; a 3.0 package has no manifest, a top-level `<name>.mnd` and
`minNotes Media/…` (`apps/mn/src/gather.rs` `Run::seal`, `unpack`). Today 3.0's `unpack` would unpack a 2.x package,
find `document.mnd`, and then refuse it at the same gate.

**1.x files.** 2.x opens a 1.x file (`mnd`, v1) read-only, converts it on its working copy (`app/core/Converter.cpp`
`convertV1`: margin ink and sketch blocks become canvas elements, `block_ink` is dropped) and writes a 2.0 file on
Save As. 3.0 refuses 1.x (`Gate::Earlier`; canon row 283, ruling 2: "opened in the old app and saved as 2.0 first").
The Store package began with 2.0.6 (`packaging/windows/msix` first appears in the commit tagged `v2.0.6`), so a user
who only ever had the Store build has no 1.x files of that build's making. A 1.x file's blocks are the same `blocks`
table; `read_pages` already has the no-`page_id` branch, only the gate stops it.

## 2. The document model

A 2.x **file is one canvas**; a **page** is a canvas element that owns blocks (`app/core/CanvasDocument.h` class
comment; `docs/basics.md` "A file is a single `.mnd` — one canvas holding any number of pages"). There are no
notebooks, sections or folders: the tree is one level, file › page.

| Item | Rule | Ref |
|---|---|---|
| Pages | Rows of `elements` with `kind = 'page'`, read `ORDER BY z, created, id` — that order is the page order everywhere | `Document::allElements`; `CanvasDocument::pageIds` |
| A page's blocks | `blocks WHERE page_id = ? ORDER BY rank` (ranks are base-62 fractional strings); blocks with no page are given to the first page on every open | `Document::skinnyScan`, `assignOrphanBlocks` |
| Ids | 26-character ULIDs for blocks and elements | `makeUlid` (`Document.cpp`) |
| Page title | **Not stored.** Derived: the first heading, else the first text line, else "Untitled page" | `CanvasDocument::pageTitle` |
| File title | The file's name (`doc_meta.title` is never written) | `Document::stampMeta` |
| Page on the canvas | The element's `x, y, w, h` (width = text measure + 24 each side); `json` = `fixedH`, `view.scrollY` | `CanvasDocument.h` constants; `step-canvas-ink-spec.md` §1.3 |

3.0's `import_v2` already does: one folder named by the caller, one note per page in page order, orphans to the first
page. Its note name is the first non-empty text block's first line (80 characters) or "Page N" — close to, not equal
to, 2.x's rule (2.x prefers the first *heading*; 60 characters; "Untitled page").

## 3. Page content

Not HTML, not Qt rich text, not Markdown: **a flat list of typed blocks, each with plain text plus a list of format
spans in a JSON `attrs` column.** Offsets are UTF-16 units (`app/core/InlineText.h` header). Full tables:
`step1-core-spec.md` §3.6, §4, §5.

Block types (`blocks.type`; `BlockModel::typeToString` / `typeFromString`, `app/core/BlockModel.cpp`):

| `type` | Meaning | `attrs` keys | Stored form (synthetic) |
|---|---|---|---|
| `paragraph` | Text | `spans`, `cell` | content `Hello world`, attrs `{"spans":[{"e":5,"k":"bold","s":0}]}` |
| `heading` | Levels 1–6 | `level`, `spans`, `cell` | attrs `{"level":2}` |
| `quote` | Quote | `spans`, `cell` | — |
| `code` | Code, literal text | `lang`, `cell` | attrs `{"lang":"python"}` |
| `list_item` | Bullet; nesting in the `depth` column (0–8) | `spans`, `cell` | — |
| `ordered_item` | Numbered; the number is computed, not stored | `spans`, `cell` | — |
| `task_item` | Task with three states | `state` (0 to do, 1 doing, 2 done), `spans`, `cell` | attrs `{"state":1}` |
| `divider` | Rule | `cell` | empty content |
| `media` | Picture, video, PDF, file chip; `content` is a JSON descriptor (§4) | `cell` | content `{"src":".minnotes/ab12.png","w":800,"h":600}` |
| `split` | A hidden "record": the blocks after it tagged `cell = n` are its lanes. Without `header` it is a side-by-side row (columns); with `header ≥ 1` it heads a **table** that runs over the records directly below | `ratios`, `header`, `table` | attrs `{"ratios":[0.5,0.5]}`; a table head: `{"header":1,"table":{"cols":[{"w":120},{"k":1,"o":[{"id":"a","l":"Open","c":"#0189F1"}]}]}}` |
| `table` | Retired; never written by 2.x, loads as a paragraph | — | — |

Table details (`attrs.table`, `step1-core-spec.md` §4.4): per column `w` width, `a` alignment, `bg` / `fg` colours,
`k` kind (1 choice with options `o`, 2 check, 3 timecode with `fps`); per row `bg`, `fg`, per cell `cbg[]`, `cfg[]`.
A choice or check cell is one paragraph holding one chip. A cell may hold any blocks, several of them. The **kanban
board and the row filter are views only — nothing of them is stored** (`BlockModel.h` "T4 (S9c): the view-only row
filter"; no `kanban` / `board` key anywhere in `app/core/`; the board is `app/qml/MinNotes/App/BlockKanban.qml` over
a choice column).

Inline marks (`mn::inl::Kind`, `kindToString`, `app/core/InlineText.{h,cpp}`), each a span `{"s","e","k","u"?}`:

| `k` | Payload `u` |
|---|---|
| `bold`, `italic`, `code`, `strike`, `underline` | — |
| `link` | the address |
| `color` | text colour, hex |
| `highlight` | highlight colour, hex |
| `choice` | chip payload `{"o":[{"id","l","c"}…],"v":"<selected id>"}`; the span's text is the label |
| `comment` | removed 2026-10-21; old files may still carry the spans; the loader drops them (`InlineText.h`, kind 9 reserved) |

What does not exist in 2.x pages: tags, callouts, toggles, embeds, inline file links, page-to-page links, per-block
colours outside tables, footnotes (`step-links-chips-spec.md` §1). One wrinkle: text typed with Markdown markers
that 2.x had not yet consumed is stored literally; 2.x consumed marker pairs on load (`BlockModel::loadFromStore`,
`convertMarkdown`), 3.0 takes the text as stored (canon row 284 b). A file last saved by 2.x has normally been
through that pass.

## 4. Media

| Item | Rule | Ref |
|---|---|---|
| Bytes | Never inside the database. A media block's `content` (or a canvas element's `json.media`) is a descriptor naming a file | `MediaStore.h` class comment |
| Descriptor | `{src, w, h}` = picture (no `kind`); `kind` = `video` (`durMs`, `frames`, `fps`), `pdf` (`pages`), `file` (`name`, `ext`), and on the canvas only `audio`, `folder`, `bookmark`; `preview` = a rendered stand-in for PSD / HEIC / RAW / HDR with the original's `{src, format, mtime, size}`; `dw` display width; `name` | `MediaStore.cpp` `imageDescriptorJson`, `videoDescriptorJson`, `pdfDescriptorJson`, `fileDescriptorJson`, `audioDescriptorJson`, `folderDescriptorJson`, `previewDescriptorJson`; `step1-core-spec.md` §5.2 |
| `src`, three forms | (a) `.minnotes/<hash>.<ext>` relative to the document's folder — pasted and copied pictures, previews, collected media; (b) an absolute path of the machine that wrote it — files on a network share are referenced in place, video and PDF always; (c) an object `{"vol":"<id>","rel":["seg",…]}` under an application-level path mapping; also an `http(s)` address for a picture not yet downloaded | `MediaStore::importFile`, `resolveUrl`; `app/core/PathMap.h` `toRef`, `resolveRef` |
| Path mappings | App-wide, not in the document: a list of volumes `{id (a generated ULID), label, win, mac, lin, enabled}` kept in the application's settings under the key `pathMappings/json` | `PathMap.h` `PathMapping`; `app/core/PathMapController.cpp` `kSettingsKey` |
| Video review notes and drawings | **Not in the document.** QCView's sidecar, byte-compatible: `<media folder>/.qcview/<media file name>/notes.json` + `images/note_<TC>.png` (falls back to `.ump` when reading). For a collected clip that is `.minnotes/.qcview/<clip>/` | `app/notes/annotation_io.h` header ("VERBATIM port of QCView-Player's"), `MediaStore::anchorPathFor` |
| PDF annotations | In the PDF's own descriptor: `ink["<page index>"]` = `{"version":"2.0","coordinate_system":"normalized","shapes":[…],"texts":[…]?}` | `BlockModel` media ink (`step1-core-spec.md` §5.2); `CanvasDocument::setMediaElementInk` |
| Picture annotations | Same key, page `"0"` — only in files converted from 1.x; 2.x has no tool to draw on a picture | `step-canvas-ink-spec.md` §6.7 |

3.0's side: the media record **is the 2.x descriptor, unchanged** (`apps/mn/src/media.rs` header: "The block's
descriptor is the Qt app's, unchanged"); `ink` per page is read and written in the same form (`apps/mn/src/ink.rs`
`page_set` and its test); `dw` and `preview` are read (`export.rs`, `gather.rs`); the notes sidecar is the same
contract (`crates/qcv-notes`). A kept file's `.minnotes/<name>` is looked for in `minNotes Media/` and then in the old
hidden `.minnotes/` beside the collection (`media::SIDE`, `OLD_SIDE`; canon row 333).

Three things do not carry by themselves:

1. **`.minnotes/` files are found only if the new collection is saved in the old document's folder.** Anywhere else
   the import has to copy them (content-hash names, so a second copy of the same picture is harmless), and a
   collected clip's `.qcview` folder with them.
2. **`{vol, rel}` references name a 2.x volume by a ULID** that only 2.x's settings can translate. 3.0's table is per
   machine and names a volume by its label (`mappings.json`, canon rows 350, 356; row 350 (7): "2.x's mappings are
   not imported"). Without a bridge these references show as missing.
3. A 2.x package's media sit under `media/`, not `minNotes Media/`.

## 5. Canvases, boards, kanban, "streams", the studio

| Thing | What it is in 2.x | Stored as |
|---|---|---|
| Canvas | The root of every file: an infinite sheet with a camera | `elements` rows; `canvas_meta` keys `camera` (`{"x","y","z"}`) and `focusPage` |
| Page card | A page shown on the canvas, edited in place or full-window | `kind = 'page'` (§2) |
| Media card | Picture, video, PDF, audio, folder, file, bookmark | `kind = 'media'`, `json.media` = a descriptor, `json.ink` for PDF pages |
| Ink | One element per stroke | `kind = 'ink'`, envelope `{"version":"2.0","coordinate_system":"element-local","shapes":[one shape]}` |
| Text chip | Plain text on the sheet | `kind = 'shape'`, `{"shape":"text","text","fontSize","fill","color","fixedW"}` |
| Frame: "group" (elastic) or "board" (fixed size) | A titled, tinted box that owns elements through `parent_id`. A canvas "board" is **not** a kanban | `kind = 'group'`, `{"title","tint","sizing","layout","aspect"}` |
| Connector | Declared, never created by any caller | `kind = 'connector'` |
| Kanban | A view of a table with a choice column, in a page | not stored (§3) |
| Stream | A QCView stream folder shown as a folder card ("Open as stream"); its notes are QCView's sidecar | a `folder` media card (`CanvasDocument::folderIsStream`) |
| Studio | The full-window view of a video or PDF block; the notes live in the sidecar | nothing of its own |
| Layers, history | Derived at run time; the undo log is in memory | not stored |

Refs: `Document.h` `Element`, `CanvasDocument.h` (frames comment, `addBookmarkElement`, `layerRows`),
`step-canvas-ink-spec.md` §1, §4, §6.

Standing ruling: old canvases are not brought in (row 327: "no one used the canvas feature in the old app"). One fact
to weigh against it: in 2.x **the canvas is not optional**. A user who only typed in pages loses nothing but the
cards' layout; a user who dropped a file, a link, a folder or a picture *on the canvas* (not in a page), or wrote a
text chip or drew there, has content that exists nowhere else, and a pages-only import leaves it behind without a
word. 3.0's canvas has a home for almost all of it (`canvas_elements` kinds `media`, `text`, `note`, `ink`, `frame`;
records in `apps/mn/src/canvas.rs`, `ink.rs`): the mapping would be page → note card, media → media, shape → text,
group → frame, ink → ink with points re-expressed as fractions of the element's box.

## 6. Anything else stored per document

| Thing | 2.x |
|---|---|
| Settings in the file | None beyond `canvas_meta` (`camera`, `focusPage`; from the 1.x converter `defaultView`, `pageWidthDefault`, `convertedFrom`) and a page's `fixedH` / `view.scrollY` |
| Tags | None |
| History / versions | None in the file (undo is in memory; `created` / `modified` per block and element, in ms) |
| Locks | None. A session lock exists only on the app's own scratch folder (`app/main.cpp`) |
| Conflict copies / merge data | None in the file. 2.x compares the original's time and size on save and offers to overwrite (`CanvasDocument::externalChangeDetected`, `overwriteSave`); there is no merge. `DocumentMerger` copies another file's pages in (File ▸ Merge) and leaves no trace |
| Passwords / encryption | None (searched `app/core`) |
| Comments | Removed 2026-10-21; tables and spans may remain in older files, unread |
| Search cache | `pdf_text` (PDF page text keyed by source + time + size); disposable |
| Unsaved documents | An untitled document lives in a per-session scratch folder under the application's data folder and is **deleted on a clean quit** (`app/main.cpp`); nothing to import from there |

## 7. The size of the job

What is already built and tested in 3.0: reading the marks and pages (`old.rs`), block conversion with the old
load-repair rules (`legacy.rs`, canon row 284 d), `import_v2`. Neither the HTML nor the Markdown importer
(`apps/mn/src/paste.rs` `from_html`, `from_markdown`; `import.rs`) is the right road: 2.x content is already a block
model one step from 3.0's, and those importers would lose ids, chips, typed columns, colours and media records.

**New Rust crates needed: none.** `rusqlite` (bundled SQLite) is a workspace dependency used by `mn-store`;
`serde_json` is there; `zip` (with deflate) is a dependency of `apps/mn`. Reading the 2.x mapping table from 2.x's
settings would be the only reason to add anything (a property-list reader on macOS; a registry read on Windows —
`windows` bindings may already be in the tree, not checked).

Maps cleanly (no loss):

- Paragraph, heading, quote, code + language, bullet / numbered / task (depth, three states), divider — `legacy.rs` `parse`.
- All inline marks: bold, italic, code, strike, underline, link, colour, highlight, choice chip (`mn_doc::inline::Mark`; the file form is the same, `crates/mn-store/src/codec.rs`).
- Side-by-side rows → `Kind::Columns` with widths; tables → `Kind::Table` / `Row` / `Cell` with header rows, column width, alignment, colours, choice / check / timecode columns, row and cell colours.
- Media blocks: picture, video, PDF, file chip, previews, display width — the record is carried verbatim.
- PDF page drawings (the `ink` key) and video notes (the sidecar is outside the file).
- Block ids (kept, so a second import of the same file could be recognised), page order.

Maps with loss or with extra work:

- Page titles: derived on both sides by slightly different rules (§2) — align `import_v2` with 2.x's rule.
- A table cell holding more than one media item, or a columns block inside a cell: 3.0's limits (row 273) — `legacy.rs` records a note per case; what is done with those notes is not shown to anyone today.
- A kanban view: 3.0 stores "this table shows as a board" (`ColumnSpec::board`), 2.x did not; tables arrive as tables.
- `.minnotes/` files and `{vol, rel}` references (§4, points 1–2): need a copy step and a volume bridge.
- Pictures still at an `http(s)` address (never downloaded): 3.0 does not fetch — shown as missing.
- `texts` (text chips on a PDF page): 3.0 dropped written notes on PDFs (row 363); the key stays in the record, whether anything shows it was not traced.
- Markdown markers 2.x had not yet consumed stay literal.
- Malformed structure: converted to what 2.x would have shown after its own repair, except that a cell is never moved to another column (row 284 d).
- If canvases are brought in after all: element geometry, frames (title, tint; elastic vs fixed), ink (coordinate change), text chips (font size and fill may not all have a home), the camera (2.x `{x, y, z}` vs 3.0's centre + zoom — semantics to check).

No home in 3.0:

- `comment` spans and the comment tables (already dropped by 2.x).
- `folder` and `bookmark` cards as such (3.0's page has file chips and links; row 327 kept neither card kind — to check in `canvas.rs` before promising anything), a `sketch` leftover block.
- `connector` elements (never created), `pdf_text`, `focusPage`, a page's `fixedH` / scroll position.
- 2.x's path-mapping table (ULID ids) unless bridged.
- 1.x-only material (`block_ink`, sketch blocks) unless the file went through 2.x first.

Where a 2.x file would be detected in 3.0 (`apps/mn/src/main.rs`):

| Road in | Today | Hook |
|---|---|---|
| Double click / File ▸ Open / recent → `open_collection` → `open_store` → `store_of` → `Collection::open` | `Err(Gate::Earlier)` becomes the "Couldn't open" notice in `open_collection`'s `Err` arm | Tell `mnd2` ≤ 2 apart from other "earlier" files (`old::marks` already returns both values) and offer the import there. 3.0 keeps 2.x's application identity and file association (row 347), so **this is the road a Store user will take first** |
| A `.mnd` dropped on the sidebar | Opened as a collection (the `ours` test / the drop handler near "Opening the collection") | Same gate, same offer |
| File ▸ Import (`import::family`, by extension) | `.mnd` is not a family | Add it: imports into the open collection as a folder of notes (what `import_v2` does) |
| A 2.x `.mnpkg` | `gather::unpack` unpacks it, then the gate refuses `document.mnd` | Recognise `manifest.json` (`format = mnpkg`), read `document.mnd`, place `media/*` as kept files |

Constraints that already hold and should stay: the old file is only ever read from a staged copy
(`staging::Session`, row 272), and is never written over. Because both generations use `.mnd`, the converted
collection must not default to the old file's own name in the old file's folder.

## 8. Sample 2.x documents in the 2.x tree

| File | What it is | Use |
|---|---|---|
| `tests/fixtures/v1-sample.mnd` | A **1.x** file (`format = mnd`, schema 1; 24 blocks: paragraphs, headings, media, split records; has `block_ink` and the comment tables) | Tests the gate's "earlier" answer; input for a 1.x reader if one is wanted. Not a 2.x sample |
| `tests/fixtures/canvas-probe.md`, `type-probe.md` | Markdown inputs for 2.x's performance probes | Not documents |
| `tests/fixtures/*.png` | Reference screenshots | Not documents |
| `tests/regression_test.cpp` | Builds 2.0 files in code (gate cases, tables, split rows, packages) | A source of cases to transcribe, not files |

**There is no 2.0 `.mnd` and no `.mnpkg` in the 2.x tree.** 3.0's own tests build a small 2.0 file in SQL
(`crates/mn-store/src/file.rs`, `old_file` in the tests; `step1-core-spec.md` §4.6 has synthetic worked examples).
A real sample would have to be made by running a 2.x build on synthetic content (tables with every column kind,
columns, every mark, a PDF with drawings, a clip with notes, a kept picture, a canvas with each element kind) and
saved as both `.mnd` and `.mnpkg` — kept under `fixtures/` (ignored) or generated, never client material.

---

## Open questions for the owner

1. **Is the ruling reversed, and how far?** [proposed] Yes for pages: a 2.x `.mnd` or `.mnpkg` is *imported* (never
   opened in place, never written), each page a note, under a folder named after the file.
2. **What does opening a 2.x file do?** [proposed] The refusal becomes an offer: "This document was made with
   minNotes 2. Import it into a new collection?" — the new collection starts untitled (row 368) and its first Save
   asks where, with a name that is not the old file's. File ▸ Import also takes `.mnd` / `.mnpkg` into the open
   collection.
3. **Canvas content.** Row 327 stands on "only I used the canvas". In 2.x every file is a canvas, so Store users may
   have cards, links, text and ink that are in no page. [proposed] Stage A imports pages only but *counts and
   reports* what was left on the canvas; a later stage brings the old canvas in as one canvas document if the
   reports show it matters. The alternative is to do it at once.
4. **Kept media (`.minnotes/`).** [proposed] Copied into the new collection's `minNotes Media/` at import (with
   clips' `.qcview` folders), so the result does not depend on where it is saved.
5. **`{vol, rel}` references.** (a) read 2.x's mapping table from its settings and resolve each reference to a plain
   path at import; (b) list the unknown volumes after import and let the user point each at a 3.0 volume; (c) leave
   them missing with Locate File. [proposed] (a) where the settings can be read, falling back to (b). To verify
   first: whether a Store (packaged) 2.x's settings are readable by 3.0 under the same identity.
6. **1.x files.** [proposed] Still not read (row 283): the message says to open and save them with minNotes 2 — but
   once 3.0 replaces 2.x under the same identity a Store user can no longer do that. Reading 1.x pages (text and
   media, without margin ink and sketches) is a small addition if wanted.
7. **The conversion's repair notes** (`Converted::notes`): shown after an import, written to a log, or dropped?
   [proposed] One closing notice with counts; details to the log.
8. **Wording and languages**: the new strings (English only for 3.0.0, row 347).

## Build stages, smallest first

1. **Detect and say so.** Tell a 2.x file from other refused files at the gate; the notice names minNotes 2 (no
   import yet). A few lines in `file.rs` / `main.rs`.
2. **Import pages from a `.mnd`** through the existing `read_pages` → `convert_page` → `import_v2`, staged copy
   only: the offer on open (new untitled collection) and File ▸ Import (into the open collection); page titles by
   2.x's rule; opened notes go through `swap_note` / `open_collection` as every other road does.
3. **Kept media**: copy `.minnotes/*` (and `.qcview` folders) into `minNotes Media/`; check each record resolves;
   report the ones that do not.
4. **The report**: counts of pages, blocks, repairs, media not found, and canvas elements left behind by kind.
5. **`.mnpkg` of 2.x**: recognise the manifest, read `document.mnd`, place `media/*`.
6. **Volume bridge** for `{vol, rel}` (question 5).
7. **A made sample set** (question in §8) and a scripted run over it on both systems.
8. **Optional: the old canvas** as a 3.0 canvas document (pages as note cards, media, text, frames, ink).
9. **Optional: 1.x pages.**
