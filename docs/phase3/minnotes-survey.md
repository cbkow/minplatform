# minNotes — survey of the Qt application (the input to the port plan)

**Surveyed:** the sibling `minNotes` repository, branch `min-paths`, commit `59d1129`, project version 2.0.6
(`CMakeLists.txt:13`), on 2026-11-10. Read-only: nothing was built, run or changed. Paths below are relative to the
minNotes repository root unless they say otherwise. "Not found" means a search of `app/` returned nothing.

**How it was read.** The block model, the editor, the shortcuts and the accessibility / input questions were read
directly. The canvas / ink / QCView interop, the persistence / platform layer, and the import / export / clipboard /
search / spelling layer were each read by a separate pass and merged here; §11 lists what none of the passes verified.

**The six facts that shape a port**

1. A file is a SQLite database (`.mnd`, format `mnd2`, schema 2) holding a canvas of *elements*; a *page* is an element
   whose blocks are rows of one flat `blocks` table. Tables and side-by-side layout are not block types: they are
   derived from runs of `split` records.
2. Text is **plain text + a span list per block**, UTF-16 offsets, owned by a C++ model (`BlockModel`) with its own
   inline engine (`mn::inl`). No `QTextDocument` is the model. Markdown is an input method, never storage.
3. The editor is a custom "passive surface": one `FocusScope` takes every key, a logical cursor `(row, col)` is the
   only caret, and read-only `TextEdit`s are used purely as a layout engine. There is **no input-method path at all**
   (no preedit, no IME, one UTF-16 unit per key event) and **no accessibility exposure at all** (zero `Accessible.*`).
4. Qt rich text is nevertheless load-bearing in three places: `QTextDocument` is the *parser* for Markdown / HTML / RTF /
   ENEX import and HTML paste, the *layout and pagination engine* for PDF export, and the per-block text layout on screen.
5. There is no autosave, no crash recovery, no document file watcher and no print path. Save is explicit; the app edits
   a private working copy and writes back with `VACUUM INTO` + atomic rename.
6. The QCView notes sidecar code is a declared verbatim port of QCView-Player's, with a "fix QCView first, then
   re-port" rule; the player (`app/player/`) is a copy of QCView's by way of ufb. minplatform already owns both.

---

## 1. Size and shape

### 1.1 Lines by area (tracked files, `wc -l`)

| Area | C++ (`.cpp` + `.h` + `.mm`) | QML / JS | Other | What it is |
|---|---:|---:|---|---|
| `app/core/` | 27,018 (78 files) | — | — | The model: document store, block model, canvas document, media store, import / export, clipboard, search, path maps, packages |
| `app/player/` | 16,546 (66 files) | — | 4 shaders | The video / audio engine (FFmpeg decode, scrub cache, audio devices, the RHI video surface) |
| `app/notes/` | 5,133 (31 files) | — | 2 shaders | Ink engine, QCView notes sidecar, video annotator, media host |
| `app/spell/` | 1,711 (13 files) | — | `en.aff`, `en.dic`, `grammar-rules.xml` | Spelling (Hunspell) and the grammar rule interpreter |
| `app/` (root) | 1,522 (17 files) | — | `CMakeLists.txt` 704 | `main.cpp`, application class, window chrome, updater shims, guide builder, dev probes |
| `app/qml/MinNotes/App/` | — | 18,463 QML (53 files) + 127 JS | 32 TTF | The whole UI |
| `tests/` | 9,491 (1 file) | — | 115 fixture files | One headless regression executable |
| `scripts/`, `packaging/`, `cmake/` | 91 | — | 981 sh, 363 py, 303 ps1 | Release, notarisation, appcast, MSIX, probes |

Total for `app/`: about 52,000 lines of C++ and 18,600 of QML. Three files dominate: `app/qml/MinNotes/App/Editor.qml`
(8,689 lines, of which roughly 2,300 — about `:3526–5900` — are in-app dev probes), `app/core/BlockModel.cpp` (6,904) and
`app/core/Exporter.cpp` (3,835). The repository has 718 commits; tags run to `v2.0.6`.

### 1.2 Module map

| Path | Purpose |
|---|---|
| `app/core/Document.*` | The SQLite store (a ref-counted connection handle, optionally scoped to one page) |
| `app/core/CanvasDocument.*` | The file: elements, canvas undo log, open / save / working copy, frames, layers |
| `app/core/BlockModel.*` | One page: rows, spans, split rows, derived tables, undo, paste, media blocks |
| `app/core/InlineText.*` | The inline text engine: text + spans edit rules, choice chips |
| `app/core/LayoutIndex.h`, `FenwickTree.h`, `ViewportSlots.*` | Height index for virtualisation; stable delegate-slot assignment |
| `app/core/MediaStore.*`, `PathMap*`, `MinPaths.h`, `FileLinks.*` | Media descriptors, the `.minnotes/` sidecar, OS-neutral path refs, ufb links |
| `app/core/Importer.*`, `DocxReader`, `XlsxReader`, `OdfReader`, `RtfConvert_*` | Import |
| `app/core/Exporter.*`, `CanvasExporter.*` | Markdown / HTML / DOCX / PDF / canvas HTML / poster |
| `app/core/PackageFormat.*`, `PackageExporter.*`, `AssetTransfer.*`, `DocumentMerger.*` | `.mnpkg`, Collect Media, merge |
| `app/core/Clipboard.*`, `BlockClipboard.*`, `ClipboardPaster.*`, `TableGrid.*` | Clipboard flavours, paste routing, the table import IR |
| `app/core/SearchEngine.*`, `CodeHighlighter.*`, `CodeSyntax.*`, `InlineMarkdownHighlighter.*` | Find in file; syntax and inline highlighters |
| `app/core/Converter.*`, `DocumentManager.*`, `Preview*`, `PsdSyncFile.*` | 1.x → 2.0; open files; PSD / HEIC / RAW / HDR previews |
| `app/notes/` | See §4 |
| `app/player/` | See §4.6 |
| `app/spell/` | See §3.9 |
| `docs/` | The public manual (Jekyll); the same Markdown is bundled as the in-app User Guide |
| `packaging/`, `scripts/` | macOS bundle + DMG + Sparkle appcast; Windows MSIX (Store) and the retired Inno installer |

### 1.3 Licence and dependencies

minNotes is **GPL-3.0-or-later** (`LICENSE`). Third-party texts are in `LICENSES/`, indexed by
`LICENSES/THIRD_PARTY_NOTICES.txt`.

| Dependency | Version / pin | Licence (as the repo records it) | How obtained | Used for |
|---|---|---|---|---|
| Qt 6 — Core, Gui (+ GuiPrivate, CorePrivate), Qml, Quick, QuickControls2, Sql, Pdf, Network, ShaderTools | ≥ 6.11 (built on 6.11.1) | LGPL-3.0 | System install | UI, SQLite driver, PDF render + text, network, QRhi; CorePrivate for `QZipWriter` (DOCX) |
| SQLite | Qt's bundled copy | Public domain | With Qt | The document store |
| FFmpeg (avcodec, avformat, avutil, avfilter, swscale, swresample) | Not pinned in the repo | LGPL-2.1+ | Prebuilt, gitignored `external/ffmpeg` | Decode, probing, posters, `subfile` streaming |
| KSyntaxHighlighting (KF6) | ≥ 6.0 (built 6.14.0) | MIT | Built by `external/build-ksyntax.*` | Code-block colouring, in the app and in exports |
| ink-stroke-modeler (+ Abseil) | commit `da42d439` | Apache-2.0 | FetchContent, static | Freehand stroke smoothing (same pin as QCView-Player) |
| SoundTouch | 2.4.1 | LGPL-2.1 | FetchContent, static | Constant-pitch review speeds |
| Hunspell | 1.7.3 | LGPL-2.1 (of the tri-licence) | FetchContent, static | Spelling |
| SCOWL word lists | 2026.02.25 | SCOWL notice | Qt resource | Merged en_US + en_GB dictionary |
| LanguageTool English rules (data only) | v6.8 subset | LGPL-2.1+ | Qt resource | Grammar rules |
| miniz | 3.0.2 | MIT | Vendored `external/miniz` | `.mnpkg`, and the zip reader for DOCX / XLSX / ODF / Notion |
| stb_image | 2.30 | MIT | Vendored | Radiance HDR previews |
| psd_sdk | commit `bfa1173` | BSD-2-Clause | Vendored | PSD / PSB composite previews |
| min-paths (from minplatform) | pinned revision | Not listed in the notices file | cargo static lib via `cmake/MinPaths.cmake` | Path refs, source classifiers, file URLs, ufb links |
| Sparkle | 2.9.2 | MIT | `scripts/fetch_sparkle.sh` | macOS updates |
| WinSparkle | Not pinned | MIT | Optional `external/winsparkle` | Legacy Windows updates (≤ 2.0.5) |
| Phosphor icons (4 TTF) | — | MIT | Bundled | Icons |
| Geist, Geist Mono, Inter, JetBrains Mono, Aspekta, Lora, PT Serif | — | SIL OFL 1.1 | Bundled TTF | UI and document type; Geist + Geist Mono are the ones in use |

A port inherits a licence question: minNotes is GPL-3.0-or-later and compiles in LGPL / GPL-compatible parts
(Hunspell statically, LanguageTool rule data). The licence of the family platform it moves to has to be compatible
with whatever of this is carried.

---

## 2. The document model

### 2.1 Containment

```
file (.mnd)  =  one canvas
  elements[]            kind: page | media | ink | shape | group      (connector: reserved, unimplemented)
    page element  ──►  blocks[] WHERE page_id = element.id, ordered by rank   (one flat list)
       block  =  { id (ULID), rank, depth, type, attrs JSON, content TEXT }
```

There is no tree in memory. A page's `BlockModel` holds four parallel vectors — `rows_` (type, level, task state,
depth, spans, lane, ratios, table attrs), `ids_`, `ranks_`, `content_` (`app/core/BlockModel.h:1107-1110`) — loaded
eagerly from the store (`BlockModel.cpp:2472`). Every edit writes through to SQLite in the same call (`persistContent`
`:2432`, `persistAttrs` `:3562`), so the working copy is always current.

### 2.2 Block types — the complete list

The enum is `BlockModel::BlockType` (`BlockModel.h:135-142`); the stored `type` strings are in
`BlockModel.cpp:2383-2408`. Numeric values are used literally all over the QML (`=== 3`, `=== 10`).

| # | Stored `type` | `content` holds | `attrs` keys | Notes |
|---|---|---|---|---|
| 0 | `paragraph` | Plain text | `spans`, `cell` | The default; unknown types load as this |
| 1 | `heading` | Plain text | `level` 1–6, `spans`, `cell` | Markdown markers stay literal in headings |
| 2 | `code` | Verbatim text with `\n` | `lang`, `cell` | No spans; KSyntaxHighlighting colours it |
| 3 | `media` | **Descriptor JSON** (§5.3) | `cell` | Opaque to the caret; kinds: image, video, pdf, file, preview (image + `preview`), remote image |
| 4 | `quote` | Plain text | `spans`, `cell` | Nesting is flattened |
| 5 | `list_item` | Plain text | `spans`, `cell`; column `depth` 0–8 | Bullet |
| 6 | `divider` | — | — | Opaque |
| 7 | (`table`) | — | — | **Retired**; reserved number, never instantiated, loads as paragraph |
| 8 | `task_item` | Plain text | `state` 0 / 1 / 2 (todo / doing / done), `spans`; `depth` | Tri-state |
| 9 | `ordered_item` | Plain text | `spans`; `depth` | The number is computed at render time, never stored |
| 10 | `split` | Lane layout descriptor | `ratios` [floats, 4 dp], `header` n, `table` {…} | A **record**: never a caret target; its lanes' blocks follow it |

**Split rows and tables.** A `split` record is followed, in flat document order, by its lanes' blocks, each tagged
`cell = lane index` (max 63). A record is one visual row of side-by-side lanes. A **table is derived, never stored**:
a record with `header > 0` heads a table that runs over the adjacent records below it (`BlockModel.h:236-243`). The
head's `table` attr carries the column spec (width, alignment, colours, kind: 0 text / 1 choice / 2 check /
3 timecode, options, fps); each record carries row and cell colours. A cell holds one or more ordinary blocks, so
table cells contain formatted text, lists, images and video. Structure is validated and repaired on load
(`repairStructure`, `BlockModel.h:1128-1141`). The model exposes about 90 table operations (`BlockModel.h:238-370`).

### 2.3 Rich text inside a block

- **Representation:** `QString` text + `std::vector<Span>` where `Span = {s, e, kind, href}`, half-open `[s, e)` in
  **UTF-16 code units** (`app/core/InlineText.h:7`, `:22`). Not `QTextDocument`, not Markdown source, not a rope.
- **Persisted** in the block's `attrs` as `"spans":[{"s":int,"e":int,"k":"<name>","u":"<payload>"?}]`
  (`BlockModel.cpp:3542-3553`).

| Kind | # | Name in the file | Payload `u` |
|---|---|---|---|
| Bold | 1 | `bold` | — |
| Italic | 2 | `italic` | — |
| Inline code | 3 | `code` | — |
| Strikethrough | 4 | `strike` | — |
| Underline | 5 | `underline` | — |
| Link | 6 | `link` | URL |
| Text colour | 7 | `color` | hex |
| Highlight | 8 | `highlight` | hex |
| (Comment) | 9 | — | **Removed**; reserved, dropped on load |
| Choice chip | 10 | `choice` | `{"o":[{"id","l","c"?}…],"v":"<selected id>"}` |

- **Two encodings of the same spans exist:** string kinds in block attrs and the clipboard; numeric `k` in the QML feed
  and in table-grid JSON.
- **Edit rules** (`InlineText.cpp`): same-kind non-payload spans merge when they touch; payload spans coalesce only on
  an identical payload; typing at a span's exact end does not extend it; a replacement that covers a whole span keeps
  the span (a spelling fix keeps its bold or link); "clear formatting" removes everything except chips.
- **Choice chips** are the only inline object. The chip is a span whose *text is the selected option's label*; it is
  atomic to the caret, to Backspace / Delete and to partial copy (`Editor.qml:1734-1770`, `:1995-2011`). Typed table
  cells (choice, check) are one paragraph holding one chip.
- **Not in the model:** mentions, tags, inline images, footnotes, comments (removed 2026-10-21), wiki links,
  block references. Images and other media are always whole blocks.
- **Markdown** exists only at the edges: typed markers are consumed into spans when the caret leaves the block and on
  load (`commitMarkdown`, `BlockModel.cpp:5228`; load pass `:2520-2530`), and it is an import / export / copy format.

### 2.4 Where the model lives and where it is rendered

| Concern | Lives in |
|---|---|
| Content, structure, spans, undo, paste, table logic | C++ `BlockModel` (a `QAbstractListModel` used mostly through ~250 `Q_INVOKABLE`s, not roles) |
| Row heights / y positions | C++ `LayoutIndex` (two-level Fenwick index); delegates *report* measured heights back (`setMeasuredHeight`) |
| Caret and selection | QML `cursor` object in `Editor.qml:1611-1858`; mirrored into the model only for undo (`noteCaret`) |
| Drawing a block | QML `BlockView.qml`: a read-only `TextEdit` + overlays, pooled by `ViewportSlots` |
| Inline styling | `InlineMarkdownHighlighter` — a `QSyntaxHighlighter` on the `TextEdit`'s document that applies char formats from the spans |

Reactivity is by hand-rolled revision counters (`layoutRevision`, `geomRevision`, `contentRevision`,
`structureRevision`, per-slot `rev`), added one by one as measured performance fixes (`BlockModel.h:31-55`).

---

## 3. The editor

### 3.1 Architecture

`Editor.qml:7-15` states the design: blocks are passive; a single `FocusScope` holds focus for the whole page; the
logical cursor (anchor + focus as `row, col`) is the one source of truth; caret and selection are overlays computed
from it. Per-block `TextEdit`s are `readOnly: true`, `selectByMouse: false`, `textFormat: PlainText`
(`BlockView.qml:555-594`) and exist only for `positionToRectangle` / `positionAt` / wrapping.

- **Virtualisation:** a delegate pool (`Repeater` over a grow-only `ListModel`, `Editor.qml:6058-6081`, `:6187-6196`)
  with stable slot assignment in C++; overscan half a screen, capped at 60 rows; pre-warmed to at least 480 delegates
  while idle.
- **Incremental text:** a keystroke emits `textInserted` / `textRemoved`; the one affected delegate applies
  `te.insert` / `te.remove` instead of resetting its text (`BlockView.qml:657-658`).
- **Line height** is pinned (`round(pixelSize × 1.35)`, `BlockView.qml:645`) because `TextEdit` has no line-height
  property.
- **Overlays below the text:** selection rects (one per visual line, `Editor.qml:1883`), inline-code chips,
  highlights, choice chips, spelling underlines. They are overlays because a char-format background paints above the
  selection, and because one document takes only one `QSyntaxHighlighter`.

### 3.2 Text input — the part that matters for accessibility

All typing arrives in one handler, `Keys.onPressed` (`Editor.qml:3272-3522`). The last branch is the whole text-input
path:

```
else if (event.text.length === 1 && event.text >= " ") { cursor.insertChar(event.text) }
```

Consequences, as code facts:

| Question | Answer |
|---|---|
| Input method / IME / preedit | **None.** No `inputMethodQuery`, `inputMethodEvent`, `ImEnabled`, preedit or `Qt.inputMethod` anywhere in `app/` (the one hit is a digits-only hint on a colour field). CJK composition, and anything else delivered as an input-method commit rather than a key event, has no path into the page |
| Characters outside the BMP | A key event whose text is two UTF-16 units fails `length === 1` and is dropped; such text enters only by paste |
| Caret movement units | Left / Right move one **UTF-16 unit** (`navLeft` / `navRight`, `Editor.qml:1991-2018`); no grapheme or surrogate handling, so a caret can land inside a surrogate pair or a combining sequence |
| Word movement / deletion | **Not found.** Arrow keys ignore ⌥ / Ctrl; there is no word-left / word-right or delete-word |
| Line Home / End | **Not found.** `Home` / `End` go to the start / end of the *whole page* (`navHome` / `navEnd`, `:2611-2620`) |
| Bidi | Nothing explicit; whatever `TextEdit` lays out, with a caret model that assumes logical = visual order |
| System text services (dictation, emoji picker, macOS text replacement, autocorrect, press-and-hold accents) | No hook; these depend on the input-method protocol the surface does not implement |
| Real text fields | 14 `TextInput`s and one `TextEdit` outside the page (search, link, filter, option names, frame title, canvas text chip — `CanvasTextEditor.qml`). Those are ordinary Qt controls and do get IME |

So the page's text is invisible to the OS in both directions: nothing is exposed (see §8.1) and nothing composed can
come in. A port that implements the platform text-input protocol and an accessibility tree for the page is adding a
capability the current app does not have, not porting one.

### 3.3 Caret, selection and block boundaries

- **Selection** is one range `(anchorRow, anchorCol) → (focusRow, focusCol)` across any number of blocks. Each block
  draws its own slice; blocks fully inside get a wash. Mouse drag hit-tests through `blockAt` (model) then the
  delegate's `positionAt` (`Editor.qml:1928-1935`), with edge auto-scroll.
- **Opaque rows** (media, divider) are whole-in or whole-out; the caret can sit *on* one (it then acts as a selected
  object: Backspace deletes it, typing opens a paragraph after it).
- **Records** (type 10) are never caret targets; navigation uses `nextLeaf` / `prevLeaf` / `leafBelow` / `leafAbove`
  with a page-relative sticky goal-x, so vertical movement keeps its column across lanes (`Editor.qml:2512-2547`).
- **Widening:** a selection whose ends sit in different top-level rows takes the split rows and tables it reaches
  into whole (`rowRangeSpan`, `:2086`). Inside one table a drag selects a cell rectangle; grips select rows / columns.
- **Ladders:** `⌘A` climbs block → cell → table or split row → page; `Esc` descends (selection → row → table → caret
  below → out to the canvas).
- **Mouse:** click places the caret, Shift-click extends, double-click selects a word by `\w` regex (ASCII-centric),
  triple-click selects the block (`:6636-6642`, `:6813-6829`). A click never follows a link; links open from a hover
  tip or the context menu.
- **Enter / Backspace** carry a long rule set (code blocks, empty list items, table rows, lane exit, merge only within
  a container, "auto-repeat never makes a structural change") in `splitLine` (`:1789-1857`), `backspaceAtStart`
  (`:2022`) and `deleteAtEnd` (`:2052`).

### 3.4 Undo / redo

- **Design:** region-snapshot transactions in `BlockModel` (`beginTxn` / `endTxn`, `BlockModel.cpp:3786-3909`). An
  entry stores the touched band of rows before and after as full `BlockSnap`s, or one of two compressions: sparse
  per-row patches when ids line up, or two `(id, rank)` orders for a pure reorder (sort, row move).
- **Coalescing:** single-character inserts share the key `"type"` and merge while the block and caret are contiguous;
  deletes, colour drags and image-width nudges have their own keys.
- **Caret** before and after is stored with each entry and restored (`caretRestoreRequested`).
- **Tree-shaped, linear in use:** each entry has a `parent`; redo follows the newest child; the Inspector's History
  list shows the active path and can jump to any state (`undoHistory`, `undoJumpTo`).
- **In memory only**; cleared when the document changes. Not persisted.
- **Three more logs:** the canvas has its own (§4.3); video annotations have their own stacks; PDF page ink goes to
  whichever log owns the media. `⌘Z` is routed by what is in front (`Editor.qml:3341-3355`, `:3390-3392`).

### 3.5 Clipboard

**Written on copy** (`app/core/Clipboard.cpp`; call sites `Editor.qml:2914-2973`):

| Situation | Flavours |
|---|---|
| A text range or one block | `application/x-mnd-blocks` + `text/plain` |
| A range holding a split row or table | the above, with `text/plain` as Markdown, + `text/html` |
| One image block | blocks + `text/plain` + image data |
| A table cell rectangle or grip selection | blocks (with a `grid` hint) + quoted TSV + one HTML `<table>` |
| Copy as Markdown (`⌘⇧C`) | `text/plain` only |

`application/x-mnd-blocks` is JSON: `{format:"mnd-blocks", version:1, minReader:1, source:{docPath, docDir, package},
blocks:[…], grid?, assets:[{rel, abs, entry, bytes, video}]}` (`BlockClipboard.cpp:106-133`). Assets are a copy-time
snapshot of where each sidecar file's bytes live, so a paste into another file can copy them (on a worker, with
progress) as part of one undo step. It is a transient interchange format, not a persisted one.

**Accepted on paste**, in this order (`ClipboardPaster::route`, `ClipboardPaster.cpp:33-48`): own blocks → verbatim
text when the caret is in a code block → `text/html` → local file URLs (as media, or into the import flow) → raster
image → TSV into a table → tabular text as a new table → plain text through the Markdown-aware `pasteText`. A flavour
that yields nothing is masked and the router asked again. **RTF is not read from the clipboard.** Cut is copy + delete.

### 3.6 Markdown shortcuts and autoformat

- **Block prefixes**, on the space that completes them, in paragraphs only (`matchMarkdownPrefix`,
  `BlockModel.cpp:3490`): `#`…`######`, `>`, `-` / `*` / `+`, `- [ ]` / `- [/]` / `- [x]`, `<1–3 digits>.`, `---`.
- **On Enter:** a ```` ``` ```` or ```` ```lang ```` line becomes a code block; a line that is exactly `---` / `***` /
  `___` becomes a divider.
- **Inline**, consumed when the caret leaves the block: `` `code` ``, `**bold**`, `*italic*`, `~~strike~~`,
  `[label](url)`. Not handled: `_italic_`, `__bold__`, images, escapes. While typing, the highlighter styles the run
  and dims the markers.
- **Armed attributes:** with no selection, `⌘B` etc. arm a mark (and the palette arms a colour) for the next typed run.

### 3.7 Slash commands, find / replace

- **Slash commands: not found.** Block insertion is the toolbar's Insert flyout, the block context menu (about 80
  entries, `Editor.qml:8200+`) and the Markdown triggers.
- **Find:** `⌘F` opens Find in File (`SearchBar.qml`, `app/core/SearchEngine.cpp`): a synchronous linear scan of the
  whole file — every page's blocks, canvas cards, PDF text (extracted once with QtPdf and cached in the file), video
  notes — by diacritic- and case-folded substring. First match per block only; no options (case, whole word, regex);
  no all-hits highlight; no next / previous command.
- **Replace: not found.**

### 3.8 Code blocks

`CodeHighlighter` attaches a stock `KSyntaxHighlighting::SyntaxHighlighter` to the delegate's document
(`app/core/CodeHighlighter.cpp:14-26`). The language list is every non-hidden definition in the KSyntax repository;
a stored `lang` is resolved by name, file extension, an alias table, then a case-insensitive sweep
(`CodeSyntax.cpp:20-55`). One dark theme. Exports drive `AbstractHighlighter::highlightLine` themselves.

### 3.9 Spelling and grammar (today)

- **Entirely in-app; no OS spell service** (no `NSSpellChecker`, no `ISpellChecker`).
- **Spelling:** Hunspell 1.7.3, **English only** (a merged US + UK dictionary of about 49k entries). User words in
  `<AppData>/spelling/user.dic`, one per line.
- **Grammar:** an in-house interpreter (`app/spell/GrammarRules.cpp`) over 1,735 LanguageTool pattern rules that
  need no part-of-speech tagger.
- **Tokenizer** skips URLs, emails, code and chip spans, words with digits / underscores, camelCase and ALLCAPS
  (`SpellTokenizer.cpp`). It is per UTF-16 unit.
- **Flow:** one worker thread; 300 ms debounce, immediate when the caret leaves a row; a background sweep of
  unchecked rows; results cached per block id. The view pulls `issuesForRow` and tiles a dot image under each range.
  An issue under the caret is withheld. The context menu offers up to five suggestions, Add to dictionary, Ignore,
  Ignore rule, and Fix all. A fix is one undo step and keeps the word's formatting.
- **Checked:** paragraphs, headings, quotes, the three list types, table text cells. Not code, media or chips.

For the new version's goal: exposing text through accessibility would let external checkers work, but this built-in
checker is a shipped, documented feature with its own data and user dictionary, so it is a separate decision whether
it stays, is replaced by the OS service, or coexists.

---

## 4. The canvas

### 4.1 What it is

Every file is one unbounded canvas in a flat world space (px, y down, no rotation, no per-element transform).
`screen = world × zoom + pan`; zoom is clamped to 0.05–4.0 (`app/core/CanvasCameraMath.h`). Elements live in the
`elements` table and are mirrored in memory in paint order (`z, created, id`).

| Kind | What it is | `json` |
|---|---|---|
| `page` | A card showing a page. Width = the text measure (+ 2 × 24 inset, measure 400–4000); height follows content up to a square, or is pinned | `{fixedH?, view?:{scrollY}}` |
| `media` | A card for a file or link | `{media:<descriptor>, ink?:{"<page>":<envelope>}}` — image, preview, video, pdf, audio, folder, file, bookmark |
| `ink` | **One element per stroke**, including rectangles, ovals, lines and arrows | `{version:"2.0", coordinate_system:"element-local", shapes:[one]}` |
| `shape` | Only the text chip is implemented | `{shape:"text", text, fontSize, fill, color?, fixedW?}` |
| `group` | A frame (the UI word). `sizing:"elastic"` = a group that hugs its children; `"fixed"` = a board that clips them and may hold an aspect | `{title, sizing, layout:"free", tint?, aspect?}` |
| `connector` | **Not implemented**: the kind string, an unused `addConnector` and a placeholder exist; no tool, renderer or exporter | — |

Frames do not nest; membership is explicit (`parent_id`); children keep absolute coordinates; removing a frame removes
its children, ungroup keeps them. `canvas_meta` keys: `focusPage` ("" = the canvas is in front), `camera`
(`{x, y, z}`), and three keys the 1.x converter writes that nothing reads.

**Pages and the canvas.** A page is edited only full-window ("the page view"). On the canvas a page card is a
mouse-less rendering of the *real block delegates* over that page's own model, through a duck-typed stand-in for the
editor (`CanvasEditorFacade.qml`) and a C++ loader that swaps the `blockModel` context property
(`app/notes/PageContextLoader.h`). Three levels of detail with hysteresis: frame and title only below zoom 0.10, one
bar per block to 0.40, live delegates above (at most 120 rows per card, built 12 per frame).

### 4.2 View, pan / zoom, selection, tools

- **Drawing:** one scaled-and-translated `Item`; a pooled `Repeater` creates a QML subtree per visible element
  (`CanvasView.qml`, `CanvasElement.qml`). Culling is a linear scan of all elements against the viewport ± half a
  viewport; there is no spatial index.
- **Pan:** middle-drag, Space-drag, the hand tool. **Zoom:** `⌘`-wheel about the pointer, pinch, `⌘±` in √2 steps,
  `⌘1` fit, `⌘0` 100 %. Wheel input goes through `SmoothWheel.qml`, which has a momentum model for wheel notches and
  for the phase-less fractional events a remote-desktop session delivers. The user's camera is saved in the file.
- **Selection:** click, Shift-click, marquee (rectangle intersection, not shape-accurate). Eight handles on a single
  selection only. **Not found:** snapping, alignment guides, rotation, multi-element resize, copy / cut / duplicate of
  elements by key (Duplicate is menu-only).
- **Tools** on single keys: V select, H hand, N page, F frame, T text box, ⇧T plain text, P pen, R rectangle, O oval,
  L line, A arrow, E eraser.
- **Drop and paste** onto the canvas: files become cards (several land in a grid as one history entry); importable
  documents become new pages; a `.mnd` / `.mnpkg` copies its pages in; a URL becomes a bookmark or a remote image;
  text becomes a chip.
- **Layers** list in the Inspector: front-most first, contiguous ink collapsed into one "Ink · N" row, frames with
  their children threaded under them; drag to reorder or to change frame membership.

`app/CanvasGestures.cpp` is not the gesture layer: it is a dev harness that posts synthetic events for the in-app
probes. The real gestures are Qt Quick pointer handlers in QML.

### 4.3 The canvas undo log

A second, separate log in `CanvasDocument` (`CanvasDocument.cpp:1866-2087`): an entry stores whole element rows
before and after (and, for a page add / remove, the page's raw block rows so ids and ranks survive). Same tree shape,
coalescing keys (`move:`, `resize:`, `text:`, …) and nested batches as the page log. Camera, page-height caches and
media-descriptor patches are deliberately outside it. `⌘Z` goes to the canvas log when the canvas is in front, else to
the page's.

### 4.4 Ink

- **Stroke model** (shared with QCView): `{id, type: freehand | rect | oval | arrow | line, color [r,g,b,a],
  stroke_width, filled, is_modeled, points [[x, y]…]}` in an envelope `{version, coordinate_system, shapes}`
  (`app/notes/annotation_serializer.cpp`). Rect = 4 corners, oval = centre + radii, line / arrow = two points.
- **No pressure, tilt or per-point width — captured or stored.** Capture is mouse events only (`CanvasInkCapture.cpp`,
  `VideoAnnotator.cpp`); no tablet or touch event handling exists; the modeler is fed `pressure = -1`. A stylus works
  as a mouse.
- **Smoothing:** Google's ink-stroke-modeler for freehand only (wobble smoother + spring model, 300 Hz minimum output,
  prediction off; `app/notes/ink_stroke_modeler_wrapper.*`).
- **Rendering:** each segment is a capsule quad with a signed-distance fragment shader and a one-pixel feather
  (`ink_tessellate.cpp`, `InkNodeItem.cpp`, `shaders/ink.frag`); the CPU painter `paintStroke` is the matching
  reference used for thumbnails and exports.
- **Eraser** removes whole strokes on all three surfaces. **A partial (pixel) eraser does not exist.** On the canvas
  it acts on press only, by bounding box.
- **Restyle:** changing pen colour or width with strokes selected restyles them live (canvas only).

| | Canvas ink | Media / PDF page ink | Video annotations |
|---|---|---|---|
| Stored in | One `ink` element per stroke | The media block's content or the media element's json, under `ink[page]` | The clip's `.qcview` sidecar, per note |
| Coordinates | Element-local px | Normalised over the page / image | Normalised over the frame |
| Width unit | World px | Source px | Source px |
| Renderer | GPU node | GPU node | `QPainter` in a painted item |
| Select / move | As elements | Not found | Built in (marquee, move, resize) |
| Undo | Canvas log | The owner's log | Its own stacks |

### 4.5 What it shares with QCView

Declared in file headers as ports, with the rule that the sidecar must stay byte-compatible:

| File(s) | Stated provenance |
|---|---|
| `app/notes/annotation_note.*`, `annotation_io.*` | "VERBATIM port of QCView-Player's …; fix QCView first, then re-port" |
| `annotation_serializer.*`, `active_stroke.h`, `ink_stroke_modeler_wrapper.*` | "direct lift from old QCView's …; schema preserved exactly" |
| `viewport_annotator.*` | Adapted (ImGui polling replaced by a push API) |
| `annotation_thumbnail.*` | Same output as QCView's for thumbnails; the arrow head differs slightly at render time |
| `app/player/` (namespace `ufbplayer`) | QCView-Player's engine by way of ufb; "re-sync from QCView; push fixes upstream" |

### 4.6 Media and the player

- **Cards and blocks:** image, video, PDF, audio, folder, file, bookmark, and "preview" (PSD / PSB, HEIC, camera RAW,
  Radiance HDR rendered once to a PNG; HEIC and RAW are macOS-only).
- **Video:** one shared decoder and audio player for the whole editor; one thing plays at a time. Frames reach the
  screen through a `QQuickRhiItem` (`app/player/VideoSurfaceItem.*`): zero-copy VideoToolbox → Metal on macOS, D3D11VA
  and a Vulkan decode bridge on Windows, an RGBA upload fallback. SDR only, one pass. Skim and shuttle audio,
  constant-pitch review speeds. The same transport keys as QCView.
- **Review studio:** a video full-window with the annotator and the notes list.
- **PDF:** QtPdf. Inline and card views are provider images; full-window is a `ListView` of `PdfPageImage` with
  per-page ink.
- **Lightbox:** a Quick-Look-style overlay for image / video / PDF / audio (stated as ported from ufb).
- **Posters and thumbnails** are extracted on demand and cached only in Qt Quick's in-memory pixmap cache.

---

## 5. The file format and everything else persisted

### 5.1 `.mnd`

One SQLite database (`app/core/Document.cpp`). Pragmas at open: WAL, `synchronous=NORMAL`, `foreign_keys=ON`.

```sql
blocks(id TEXT PK, rank TEXT NOT NULL, depth INTEGER DEFAULT 0, type TEXT NOT NULL, attrs TEXT,
       content TEXT NOT NULL DEFAULT '', created INTEGER, modified INTEGER,
       page_id TEXT REFERENCES elements(id) ON DELETE CASCADE)
  INDEX blocks_order(rank, depth, type); INDEX blocks_page_order(page_id, rank, depth, type)
doc_meta(id INTEGER PK CHECK (id = 1), title, schema_version, app_version, created, modified,
         last_cursor, page_width, format)
elements(id TEXT PK, kind TEXT NOT NULL, x, y, w, h REAL, z INTEGER,
         parent_id TEXT REFERENCES elements(id) ON DELETE SET NULL,
         json TEXT NOT NULL DEFAULT '{}', created, modified)       INDEX elements_z(z, created)
canvas_meta(key TEXT PK, value TEXT)
pdf_text(key, page, mtime, size, text, PRIMARY KEY (key, page))    -- search cache, created on first use
```

- **Ids** are ULIDs (26 Crockford base-32 characters). **Order** is a fractional rank: base-62 strings compared as
  plain strings, `rankBetween(a, b)` for inserts (`BlockModel.cpp:2411`); no rebalancing pass was found.
- **Versioning:** `doc_meta.format = "mnd2"`, `schema_version = 2` (`Document.h:68-70`). The format *string* was
  changed at 2.0 so that every 1.x build refuses a 2.0 file.

| File | On open |
|---|---|
| `mnd2`, schema ≤ 2 | Opens |
| `mnd2`, schema > 2 | Refused ("made with a newer minNotes") |
| `mnd`, schema 1 | Converted on the working copy, opened read-only; Save As writes a 2.0 file; the original is untouched |
| Anything else | Refused |

- **Migration:** there is no general migration framework — one converter (`app/core/Converter.cpp`) for 1.x: block-
  pinned margin ink and sketch blocks become canvas ink, text chips and media elements beside the page.
- **Unused columns:** `doc_meta.title` and `last_cursor` are never written; `page_width` is read once by the converter.
  Old files may carry inert `comment_threads` / `comment_messages` tables.

### 5.2 Save lifecycle

1. **Open** byte-copies the file to a per-session scratch directory under the app's data folder and runs SQLite only
   there — never on the document's real location (network shares, sync folders).
2. **Edits** write through to the working copy immediately.
3. **Save** (`⌘S`): checkpoint, `VACUUM INTO` a temp file beside the original, atomic rename over it.
4. **Conflict check**, only at Save: the original's mtime and size against the values recorded at load / last save.
   A difference raises a dialog with Overwrite.
5. **Save As** does the same at a new path and copies the whole media folder; a "collect" offer follows.

| Capability | State |
|---|---|
| Autosave / timed save | **Does not exist** (one automatic save after Collect Media) |
| Crash recovery | **Does not exist.** Orphaned session directories are deleted at the next launch, never offered back |
| Watching the document for external change | **Does not exist** (only QCView sidecars are watched) |
| Locking / presence across machines | Does not exist |
| Sync, multi-device merge | Does not exist; the mtime / size check is the only protection |
| Restoring open files at launch | Does not exist; only a recent-files list (10) |
| Single instance | Windows only (lock file + local socket forwarding arguments); macOS relies on the OS |
| Multiple windows | Not found; one window, several open files behind a switcher in the path bar |

### 5.3 Media

- **Sidecar folder** `.minnotes/` beside the file, flat, content-addressed (`<sha1>.<ext>`); Collect Media and
  cross-file transfers use readable names with `-2`, `-3` suffixes.
- **Copy or reference:** a pasted or dropped *local* image is copied in; anything on a network share is referenced in
  place; video, PDF, audio, folders and generic files are always referenced. The classifiers come from min-paths.
- **Descriptor** (a media block's `content`, or `json.media` of an element): `src` plus per-kind fields — image
  `{w, h, name?}`; video `{kind, w, h, durMs, frames, fps}`; pdf `{kind, w, h, pages}`; audio `{kind, name, durMs,
  sampleRate, channels}`; file `{kind, name, ext}`; folder `{kind, name, count, entries, scanned}`; bookmark `{kind,
  host, title, icon, fetched}`; preview adds `preview:{src, format, mtime, size}`. Blocks add a display width and
  `ink`.
- **`src`** is a string (`.minnotes/…`, an absolute path, or a URL) or an OS-neutral reference
  `{"vol": <mapping id>, "rel": [segments…]}`. **Path mappings are app settings, not file content**; an unknown
  volume resolves to nothing (the broken-media state).
- **Remote images** in pasted HTML are downloaded in the background into the sidecar; a bookmark makes one request
  for the page title and one for its icon.

### 5.4 `.mnpkg`

A zip (miniz, zip64): `document.mnd` and `manifest.json` deflated, `media/<name>` **stored**, `media/.qcview/…` for
video-note sidecars. Manifest: `format:"mnpkg"`, `formatVersion:3` (required exactly), `appVersion`, `created`,
`mediaCount`, `mediaBytes`. Opening extracts only the database and the note sidecars; media is extracted lazily, and a
stored video plays straight from the archive through FFmpeg's `subfile` protocol with byte offsets computed from the
zip's local header. A package opens as a read-only view; Save As materialises a `.mnd`.

### 5.5 Interop with QCView's `.qcview` folders

- **Layout** (`app/notes/annotation_io.*`): `<media dir>/.qcview/<sanitised media file name>/notes.json` +
  `images/note_<timecode>.png` (+ `_annotated.png`); streams at `<project dir>/.qcview/streams/<name>__<id8>/`.
  A legacy `.ump` folder is read as a fallback. The folder is set hidden on Windows.
- **`notes.json`:** `{media_file, media_type, created, notes:[…]}` (a stream: `{stream, stream_id, stream_url, …}`).
  There is **no root version field**; the only version is `annotation_data.version` ("2.0", "1.0" readable). A note:
  `timecode` (the key), `timestamp_seconds`, `frame`, `image`, `text`, `addressed`, `annotation_data`, and optional
  `viewer`, `ocio`, `captured_at`, `host_timecode`, `stream` — round-tripped, never set by minNotes.
- **Write:** immediately on every change, with `QSaveFile`. **Merge on save:** the file is re-read; ours wins per
  timecode; notes only on disk survive unless deleted here (tombstones). No field-level merge, no lock.
- **Watch:** a `QFileSystemWatcher` with a 200 ms debounce reloads QCView's edits live; the app's own writes are
  recognised by remembered mtimes.
- **"Open in minNotes"** is QCView's button. The receiving side: an argument, file-open event or `minnotes://` link
  that is a folder containing `notes.json` (or the `notes.json` itself) opens — or on first use creates beside the
  notes — `<folder>/<name>.mnd` with every note inserted as an image block (strokes kept as vector ink) plus a caption
  paragraph (`DocumentManager.cpp:147-176`). The link to the folder is held in memory per session, not in the file.
  A toolbar chip re-inserts the folder's notes; it inserts all of them again, with no de-duplication.
- **Notes into a page:** "Notes below" on an inline video and "Notes page below" on a canvas video use the same recipe.

---

## 6. Feature inventory

| Area | What a user can do |
|---|---|
| Files | New, Open, Open Recent (10), Save, Save As (+ collect offer), Close; several files open at once behind the path bar's file switcher; unsaved-changes prompts on close and quit; conflict and save-failed dialogs; welcome screen with recents and drop-to-import |
| Navigation | Path bar `File › Canvas › Page › Frame`; Back / Forward through visited views (keys, chevrons, mouse buttons); a page's tables, videos and PDFs open "full-window" as frames; Always on Top pin |
| Pages | Add, place with the page tool, resize the card (width = text measure), pin a height, delete; title derived from the first heading. No folders, notebooks, tags, favourites, sorting or templates were found |
| Writing | Paragraph, headings 1–6, quote, bullet / numbered / task lists (tri-state, nesting to 8), code blocks (language chip, Copy chip), dividers; bold, italic, underline, strike, inline code, link, text colour, highlight, saved swatches; inline choice chips |
| Layout | Split rows (lanes): create by menu or `⌘`-drag, resize with snapping and aligned dividers, align lanes, merge rows, delete lane; block rail with drag handles; move blocks by drag or `⌥⌘↑/↓`; duplicate |
| Tables | Header rows; column types text / choice / checkmark / timecode (with frame rate); widths, alignment, cell / row / column colours; insert / move / duplicate / delete rows and columns; sort; fill; grip and rectangle selections with bulk operations; sticky header and frozen first column; full-window view with a row filter; kanban board over a choice or checkmark column |
| Media in pages | Images (resize, fit, original size), video (inline transport, studio), PDF (inline paging, full-window, drawing on pages), preview formats, file attachments; drop on a block's edge to make a lane; drop into a table cell |
| Canvas | §4 |
| Search | Find in File (§3.7) |
| Spelling | §3.9 |
| Import | Markdown, plain text, CSV / TSV, HTML, DOCX, XLSX, ODS / ODT, 38 source-code extensions, RTF, Evernote `.enex`, Notion export `.zip`; a 100,000-cell cap with a truncate / attach / cancel choice |
| Export | A page as Markdown (+ `.assets/`), self-contained HTML, DOCX, PDF; the canvas as HTML or a PNG poster; the file as `.mnpkg`; options: include video notes, include ufb links |
| Merge | Merge another open file into the page; drop a `.mnd` / `.mnpkg` on the canvas |
| Media management | Collect Media, Reveal Media Folder, Path Mappings, Reveal in Finder / Explorer, Open in ufb, Copy path, Refresh preview |
| History | Inspector History list with click-to-travel, per page and for the canvas |
| Help | User Guide built into a canvas file from the bundled manual; Rebuild User Guide; Check for Updates (macOS) |
| Drag and drop | **In:** files and URLs onto the welcome screen, a page, a table cell, the canvas, a frame. **Out: not found** — nothing starts an OS drag; only in-app block and card drags |
| Printing | **Not found** (no print support linked, no menu item); PDF export is the substitute |
| Templates | Not found |
| Settings UI | **No preferences window.** Preferences are menu toggles and remembered state (§5.6 table) |
| Themes | **One dark theme** (`Theme.qml`); no light theme, no system-appearance following; exports: dark HTML, light DOCX / PDF |
| Languages | **English only.** About 500 `qsTr` and 100 `tr` call sites exist, but there are no translation files, no translator is installed, and a few strings are unwrapped |
| Updates | macOS: Sparkle (daily, EdDSA-signed appcast). Windows: the Microsoft Store |
| Command line | A path, `file://` URL, `minnotes:` URL, stream folder or `notes.json`; headless `--export-pdf=`, `--export-html=`, `--export-canvas-html=`, `--export-poster=`; `--native-frame`; about twenty dev / probe switches |
| URL scheme | `minnotes:///<absolute path>` opens that file (or stream folder). No query, no page or block addressing |

**Import and export mechanics a port has to replace** (details in the layer notes behind §11):

| Path | Today's mechanism |
|---|---|
| Markdown, HTML, RTF-as-HTML, ENEX import; HTML paste | `QTextDocument::setMarkdown` / `setHtml`, then one walker over Qt's frame / table / block / fragment tree (`Importer.cpp:580-876`) |
| DOCX, XLSX, ODF import | Hand-written `QXmlStreamReader` over zip entries |
| RTF | OS converters: `NSAttributedString` on macOS; a RichEdit-based Windows file whose own header calls it an uncompiled draft, though the build includes it |
| Markdown, HTML, DOCX, canvas HTML export | String / XML generation (DOCX zipped with Qt's private `QZipWriter`) |
| PDF export | `QTextDocument` built with `QTextCursor`, paginated by Qt, painted to `QPdfWriter` |
| HTML export assets | Everything inlined: images as data URIs, nine whole font files (about 650 KB), small inline scripts (lightbox, readable width, checkable tasks, code copy, canvas pan / zoom / tabs) |

**Persisted settings** (QSettings; application and organisation name both `minNotes`; a dev flag switches to a
separate `minNotes-dev` domain):

| Key | Default | Controls |
|---|---|---|
| `pathMappings/json` | `""` | Path mappings `[{id, label, win, mac, lin, enabled}]` |
| `recents/paths` | `[]` | Recent files |
| `spelling/checkSpelling`, `checkGrammar`, `ignoredRules` | true, true, `[]` | The checker |
| `view/measureCap`, `capMeasure` | 760, false | Reading measure and whether it is on |
| `view/alwaysOnTop`, `frameCounter` | false, false | The pin; frames vs timecode |
| `export/ufbLinks` | false | ufb links in exports |
| `boards/map` | `{}` | Per-table board column, by block id |
| `window/*`, `inspector/open` | — | Geometry; Inspector open |
| `drawing/colorHex`, `width`; `swatches/user` | — | Pen; ten user swatches |
| `performance/*` (4 keys) | — | Decoder tuning; read-only, no UI |

Other app data: `spelling/user.dic`, the built User Guide, scratch sessions, and a media folder for untitled files
that nothing cleans up.

---

## 7. Keyboard shortcuts

**Where they are defined.** There are three sources, and they are not generated from one another:

1. `app/qml/MinNotes/App/Editor.qml:3251-3522` — the central `Keys.onPressed` / `onReleased`: everything typed in a
   page, on the canvas, in the PDF view and in the studio.
2. `app/qml/MinNotes/App/Main.qml:1192-1335` (the native macOS menu bar) and `:1343-1455` (the Windows menu) — the
   menu shortcuts, declared twice.
3. `app/qml/MinNotes/App/ShortcutCatalog.js` — a hand-maintained catalogue from which `scripts/shortcuts-doc.py`
   writes `docs/shortcuts.md`. It documents; it binds nothing.

`⌘` below is Ctrl on Windows. "Editor" = source 1, "Menu" = source 2.

| Shortcut | Action | Defined |
|---|---|---|
| ⌘N / ⌘O / ⌘S / ⌘⇧S / ⌘W | New / Open / Save / Save As / Close file | Menu (`StandardKey`) |
| ⌘F | Find in File | Menu |
| ↑ ↓ / Return / Esc | Walk hits / jump / close (search bar) | `SearchBar.qml` |
| ⌘/ | User Guide at Keyboard Shortcuts | Menu |
| ⌥⌘T | Always on Top | Menu |
| ⌥⌘← / ⌥⌘→ | Back / Forward | Menu |
| ⌘+ / ⌘− / ⌘0 | Zoom in / out / 100 % on the surface in front | Editor `:3433-3435`, `:3363-3365` |
| ⌘1 | Page: toggle Full Width; canvas: fit; PDF: fit | Editor `:3436`, `:3366` |
| ⌘Z / ⌘⇧Z / ⌘Y | Undo / redo, routed to the page, canvas, annotation or media log | Editor `:3344-3355`, `:3390-3392` |
| ⌘X / ⌘C / ⌘V | Cut / copy / paste | Editor `:3408-3410`, `:3453` |
| ⌘⇧C | Copy as Markdown | Editor `:3370` |
| ⌥⌘C | Insert a choice chip | Editor `:3378`; Menu |
| ⌘A | Select: block → cell → table / split row → page | Editor `:3471` |
| ⌘B / ⌘I / ⌘U / ⌘⇧X / ⌘E | Bold / italic / underline / strike / inline code (arms when nothing is selected) | Editor `:3488-3492` |
| ⌘K | Link | Editor `:3495` |
| ⌘⇧H | Toggle highlight in the armed colour | Editor `:3493` |
| ⌘\ | Clear formatting | Editor `:3494` |
| ← → ↑ ↓ (+ ⇧) | Move / extend by character or visual line | Editor `:3496-3499` |
| Home / End (+ ⇧) | Start / end of the page | Editor `:3500-3501` |
| PageUp / PageDown (+ ⇧) | One viewport | Editor `:3502-3503` |
| Backspace / Delete | Delete; merge blocks; remove a selected object | Editor `:3504-3505`, `:3467` |
| Enter / ⇧Enter | Split block (and the Enter rules of §3.3) / newline in code, new block in a cell | Editor `:3510` |
| ⌘Enter | Insert a table row below | Editor `:3472` |
| Tab / ⇧Tab | Indent / outdent a list item; next / previous lane or cell (adds a row past the last cell) | Editor `:3506` |
| ⌘] / ⌘[ | Page: indent / outdent. Canvas: step one layer forward / back | Editor `:3395-3407` |
| ⌥⌘] / ⌥⌘[ | Canvas: bring to front / send to back | Editor `:3396` |
| ⌥⌘↑ / ⌥⌘↓ | Move the block or selected run up / down | Editor `:3414` |
| Space | On an image / video block: lightbox. In a checkmark cell: cycle. In a choice cell: open the picker | Editor `:3476-3487`, `:3511-3519` |
| Esc | Search → stroke → tool → frame → drags → selections → armed marks → lane → out to the canvas | Editor `:3280-3338` |
| V H N F T ⇧T P R O L A E | Canvas tools (select, hand, page, frame, text box, plain text, pen, rect, oval, line, arrow, eraser) | Editor `:3442-3449` |
| P R O L A E, V | PDF view: pen tools; pointer | Editor `:3358-3360` |
| ⌘G / ⌘⇧G | Group into a frame / ungroup | Editor `:3450-3451` |
| Return | Canvas: open the selected element | Editor `:3461` |
| Space (hold) / Space (tap) | Canvas: hand / lightbox on the selection. PDF view: hand | Editor `:3251-3268`, `:3356`, `:3444` |
| PageUp / PageDown | Canvas: page a selected PDF | Editor `:3454-3455` |
| K W S (and Space in the studio) | Video play / pause | Editor `videoKeyPressed` `:1277` |
| ← → or Q E | Step a frame | same |
| J L or A D (hold) | Rewind / fast-forward | same |
| Home / End | Video start / end | same |
| R / ⇧R | Cycle review speed / reset to 1× | same |
| V / M / ↑ ↓ | Loop / mute / volume | same |
| ⌘-wheel, pinch | Zoom about the pointer | `Editor.qml:6877-6890`, `CanvasView.qml:553-567` |
| Middle-drag; Space-drag | Pan | same |
| ⌘-drag from a block's edge | Split into columns | Editor (pull strips) |
| ⌥ while dragging a lane divider | Move only this row's divider | Editor `:2148` |
| ⇧Click / ⌘Click on a table grip | Span / toggle rows or columns | Editor `:2276` |
| ⌘ held at the release of a canvas drag | Join or leave a frame | `CanvasView.qml:137-172` |

Notes for a port: the video and canvas letter keys are context-dependent (a selected video takes the transport set
first, so the tool letters wait); `⌥⌘C` matches both `C` and `ç` because of the macOS option layer (`:3375-3379`);
shortcuts are not user-configurable.

---

## 8. Platform integration today

### 8.1 Accessibility

**None.** A search of all QML, C++ and Objective-C++ for `Accessible.`, `QAccessible`, "accessib", `NSAccessibility`
and UI Automation returns nothing. No role, name or description is set on any control; the custom flat buttons,
menus, toolbar and path bar are plain items. The page is a stack of read-only `TextEdit` items that never take focus,
with the caret and selection drawn as rectangles, so even Qt's built-in text interfaces have no focused editable
element to report. A screen reader, Grammarly, the system spelling panel and dictation therefore see no document text.

### 8.2 Menus and window chrome

- **macOS:** a native menu bar via `Qt.labs.platform` (File, Export, Edit, Document, View, Help). The window keeps its
  traffic lights with a transparent, hidden-title title bar (`WindowChrome_mac.mm`); it is not frameless.
- **Windows:** a native frame; the menu is a hamburger button opening a QML menu. Dark colour scheme, Fusion style.
- **Always on Top** is native on both (`NSFloatingWindowLevel`; `HWND_TOPMOST`).
- Context menus are all custom QML. There is no system tray, Dock menu, Services, Share or Quick Look integration,
  and no Spotlight / Windows Search indexing of `.mnd` content.

### 8.3 Packaging and associations

| | macOS | Windows |
|---|---|---|
| Distribution | Developer ID, hardened runtime, notarised DMG; Apple Silicon; macOS 13+ | Microsoft Store (MSIX, x64, `runFullTrust`); the Inno installer is retired at 2.0.5 |
| Sandbox | **Not sandboxed** (no app-sandbox entitlement) | MSIX virtualises app data; real paths are resolved before being handed to other apps |
| File types | UTIs `app.minnotes.mnd` (editor) and `app.minnotes.mnpkg` (viewer) | `.mnd`, `.mnpkg` associations |
| URL scheme | `minnotes` | `minnotes` |
| Updates | Sparkle | The Store |
| Bundle / identity | `app.minnotes` | `cbkow.minNotes` |

OS-specific code: the Metal zero-copy bridge, CoreAudio device, RTF via AppKit, HEIC / RAW previews via ImageIO and
QuickLook, window chrome, Sparkle (all `.mm`); WASAPI, D3D11VA + Vulkan decode, RichEdit RTF, the single-instance
socket, the hidden attribute on `.qcview` folders (Windows). No Linux target.

---

## 9. Known rough edges

The code carries no TODO / FIXME / HACK markers at all; caveats are written as dated prose comments. What they and
the reading show:

**Performance scaffolding around Qt Quick** (each a measured fix; none would be carried, but each names a real cost)

- The whole revision-counter scheme and per-slot revisions exist so a keystroke re-evaluates one delegate
  (`BlockModel.h:31-55`).
- A child-item create / destroy makes Qt 6.11's polish walk the whole item tree (~5 ms), so the caret is one
  persistent rectangle and `cursor.settling` masks a transient selection (`Editor.qml:1631-1635`, `BlockView.qml:699`).
- The delegate pool only grows and is pre-warmed, because creating delegates mid-scroll hitched on dense tables
  (`Editor.qml:6005-6077`).
- Zoom and resize are split into a live transform and a settled layout pass 120–150 ms later (`:80-87`, `:128-141`).
- Table geometry has its own revision; the frozen column diffs its model by hand to avoid a reset per frame.

**Fragile seams**

- `CanvasEditorFacade.qml` is a duck-typed copy of the editor's interface for page cards; a missing name surfaces as
  a runtime `TypeError`.
- Pointer-handler ownership on the canvas is decided by a passive handler at press because two drag handlers raced.
- Block types and span kinds are bare integers throughout the QML.
- Roughly a quarter of `Editor.qml` is dev probes compiled into the shipping app; `app/CanvasGestures.*` and
  `KeyLatencyProbe.*` exist for them.
- Qt reader quirks are worked around in the importer (a task marker that leaks onto later paragraphs; a cell
  background stamped on every fragment; headings arriving bold).

**Behaviour to put to the user rather than copy or fix silently**

| Finding | Where |
|---|---|
| DOCX column and merge elements are written and read as `w:tableCol` / `tableSpan`; OOXML's names are `w:gridCol` / `w:gridSpan`. It round-trips with itself; real Word files' widths and merged cells are not read. Not tested against Word | `Exporter.cpp:2619`, `:2681`; `DocxReader.cpp:386`, `:427` |
| On the canvas `⌘C` and `⌘X` are not gated the way `⌘V` is, so they appear to act on the hidden page's caret. Read from the handler order; not run | `Editor.qml:3408-3410` vs `:3409`, `:3440` |
| The stream chip's "Insert notes" re-inserts every note each time | `ChromeToolbar.qml:206-219` |
| RTF import: the docs say macOS only; the build compiles the Windows converter, whose header calls it a draft | `app/CMakeLists.txt:37`, `RtfConvert_win.cpp:5` |
| A timecode column does not survive the table-grid IR (its enum has no such kind) | `TableGrid.h`; `BlockModel.cpp:1241-1285` |
| Media of an untitled file lands in an app-data folder that is never cleaned | `CanvasDocument.cpp:296-302` |
| Self-write detection for sidecars is by mtime only | `VideoNotesModel.cpp:64-72` |
| No rank rebalancing was found for the fractional order | `BlockModel.cpp:2411-2430` |
| The regression test binary sets the real application name, so it shares the user's settings domain | `tests/regression_test.cpp:9342` |
| Stale comments (page height cap, package format version, upcoming shapes); a stray `.orig` file in `app/player/d3d11/` | various |

**Absent by design or by omission:** IME, accessibility, word / line caret movement, replace, print, autosave, crash
recovery, light theme, translations, a preferences window, drag-out, pressure, a partial eraser, connectors, element
copy / paste by key, snapping.

---

## 10. What a port must carry

### 10.1 Frozen contracts

| Contract | What is fixed | Evidence |
|---|---|---|
| `.mnd` | The SQLite schema of §5.1; `format = "mnd2"`, schema 2 and the open gate; ULID ids; base-62 rank strings; block `type` strings; `attrs` keys (`level`, `state`, `lang`, `cell`, `ratios`, `header`, `table`, `spans`); span kind names and the reserved numbers 7 (block) and 9 (span); **span offsets in UTF-16 code units**; the split-record / derived-table encoding; the choice-chip payload; element kinds and their json; `canvas_meta` keys; media descriptors | §2, §5 |
| `.mnd` 1.x | Read-only adoption and conversion (or a ruling to drop it) | `Converter.cpp` |
| `.minnotes/` | The sidecar folder name, relative `src` strings, `{vol, rel}` refs | §5.3 |
| `.mnpkg` | Zip layout, manifest `formatVersion: 3`, stored media (the in-archive playback depends on it) | §5.4 |
| `.qcview` sidecar | Folder layout, file names, `notes.json` shape, the stroke envelope — byte-compatible with QCView; minplatform is now the upstream | §5.5 |
| Stream hand-off | A folder with `notes.json` opens / creates `<folder>/<name>.mnd` with the `__<id8>` suffix stripped | `DocumentManager.cpp:147-176` |
| `minnotes://` and file associations | `minnotes:///<path>`; `.mnd`, `.mnpkg`; bundle id and Store identity (for in-place upgrade) | §8.3 |
| Clipboard | `application/x-mnd-blocks` v1 only matters if old and new builds must exchange copies; otherwise transient | §3.5 |
| Exports users rely on | The Markdown dialect (tri-state `- [/]`, `<u>`, the reference fences), the self-contained HTML and its `localStorage` keys, the ufb link form | §6 |
| Settings worth migrating | Path mappings, recents, the user dictionary, ignored grammar rules, swatches, measure and view toggles, pen | §6 |
| Shortcuts | §7 — including QCView's transport set | §7 |

The UTF-16 offsets deserve a ruling of their own: a Rust text core will naturally index by UTF-8 bytes or by
grapheme, accessibility APIs differ by platform (UTF-16 on both macOS and Windows), and the file stores UTF-16
offsets. One conversion boundary has to be chosen and kept.

### 10.2 Implementation detail (free to redesign)

The QML delegate pool, revision counters, `ViewportSlots`, the Fenwick height index and measure-back loop; overlays
standing in for backgrounds; the passive-`TextEdit` layout trick; the editor facade for page cards and the three
level-of-detail tiers; the zoom / resize settle timers; the in-app probes; the two duplicate menu definitions; the
working-copy scratch layout; `SmoothWheel`'s tuning (though the remote-desktop wheel case it handles is real); the
single shared decoder; the in-memory undo representation.

### 10.3 The hardest parts to rebuild outside Qt

1. **A real text-editing core with platform text input and accessibility.** Today the page has neither, so there is
   no behaviour to copy and no Qt implementation to lean on: the port needs line layout, shaping, bidi, grapheme-aware
   caret movement and hit-testing (all currently delegated to `TextEdit`), plus the macOS and Windows input-method
   protocols and an accessibility tree that presents many blocks — including table cells and lanes — as editable text
   with correct ranges and bounds, kept in step with a model whose stored offsets are UTF-16. This is the stated goal
   of the new version and the largest piece of new engineering. It also has to preserve the block-grain behaviours of
   §3.3 (atomic chips, opaque rows, widening selections, the ladders) that ordinary text controls do not have.

2. **The page layout engine: virtualised blocks, split rows and derived tables.** `BlockModel` is 8,000 lines mostly
   because tables are derived from flat records and every structural edit must keep that encoding valid. The model
   logic is portable (it is plain C++ over vectors), but its geometry is coupled to measured heights and natural
   widths reported back by delegates. A port has to supply text measurement itself and then reproduce column auto-
   width, sticky headers, the frozen column, lane dividers and cell selection on top of it.

3. **Everything `QTextDocument` does off-screen.** It is the Markdown and HTML parser behind five import paths and
   HTML paste — with the leniency real-world Word / Docs / Excel clipboard HTML needs — and it is the paginating
   layout engine for PDF export (page breaks, repeated table headers, column bands, links, embedded fonts). Neither
   has a drop-in replacement; each needs a chosen library or a written engine, and a corpus to compare against.
   Related smaller dependencies: QtPdf (render + text extraction), KSyntaxHighlighting (a Qt-based library used on
   screen and in three exporters), `QXmlStreamReader` / `QZipWriter` for the office formats, the OS RTF converters.

4. **Media inside a document surface.** Video frames, PDF pages, images and GPU ink composited inside scrolling,
   zooming pages and canvas cards, with one transport reused in four places. minplatform already has the player, the
   ink and the notes code for QCView, so this is integration rather than invention — but minNotes embeds them inline
   in text flow, in table cells and on a zoomable canvas, which QCView does not.

Smaller but real: Hunspell + the grammar interpreter (portable C++, English only, with the "keep or replace" question
of §3.9); the 500 untranslated strings, which the platform's i18n rule would require drafting in eleven languages; and
SQLite access, which is straightforward but should keep the working-copy discipline.

---

## 11. Not determined

- Nothing was run. Every behaviour above is read from code and the public manual; the two suspected bugs in §9 (DOCX
  element names, `⌘C` / `⌘X` on the canvas) are unverified against a running build.
- Whether dead keys and press-and-hold accents reach the page on macOS and Windows. The code has no input-method path;
  what Qt's platform plugins deliver as plain key events in that situation was not tested.
- The exact macOS preferences file name (no organisation domain is set).
- Not read line by line: `DocxReader`, `XlsxReader`, `OdfReader`; the bodies of `AssetTransfer`, `DocumentMerger`,
  most of `PackageExporter`; `CanvasExporter`; `Lightbox.qml`, `VideoTransport.qml`, `CompactTransport.qml`;
  `BlockKanban.qml`; the studio's notes list; `app/player/` beyond its structure; the MSIX build script; the
  dictionary and rule build scripts.
- The FFmpeg and WinSparkle versions (binaries are not in the repository).
- Whether PDF pages take text chips: a comment says so and model functions exist (`pdfAddPageText` and siblings,
  `BlockModel.h:772-778`), but no caller was found in the QML that was read.
- Performance numbers. The repository has probe scripts; none were run, and no baseline for minNotes exists in
  minplatform's `docs/spikes/`.
- The one uncommitted edit the branch was said to carry: `git status` showed a clean tree at the time of reading.
