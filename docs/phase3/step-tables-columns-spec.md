# minNotes tables and columns — what the Qt app did (Phase 3, tables / columns step)

Source read: the minNotes repository at commit `59d1129` (2026-11-10, branch `min-paths`), read only. Nothing in it was
built or run; every statement is read from code, from the app's own manual or from its regression test's section
titles, and anything deduced rather than read is marked **(inference)**. References are `alias:line` relative to the
minNotes repository.
`BM` = `app/core/BlockModel.cpp`, `BMh` = `app/core/BlockModel.h`, `Ed` = `app/qml/MinNotes/App/Editor.qml`,
`Kb` = `app/qml/MinNotes/App/BlockKanban.qml`, `ChP` = `app/qml/MinNotes/App/ChoicePicker.qml`,
`ChE` = `app/qml/MinNotes/App/ChoiceColumnEditor.qml`, `Tb` = `app/qml/MinNotes/App/ChromeToolbar.qml`,
`TG` = `app/core/TableGrid.cpp`, `TGh` = `app/core/TableGrid.h`, `CP` = `app/core/ClipboardPaster.cpp`,
`CPh` = `app/core/ClipboardPaster.h`, `BC` = `app/core/BlockClipboard.cpp`, `BCh` = `app/core/BlockClipboard.h`,
`Ex` = `app/core/Exporter.cpp`, `Im` = `app/core/Importer.cpp`, `Imh` = `app/core/Importer.h`,
`Xh` = `app/core/XlsxReader.h`, `TC` = `app/player/timecode_formatter.{h,cpp}`, `Man` = `docs/tables.md`,
`ManE` = `docs/editing.md`, `Sh` = `docs/shortcuts.md`, `T` = `tests/regression_test.cpp`.

Sibling specs this one leans on and does not repeat: `S1` = `step1-core-spec.md` (the stored records: §4.2 the `split`
record, §4.3 derived tables, §4.4 the `table` attr, §4.5 load repair, §1.6 chips), `L2` = `step2-layout-spec.md`
(§8 columns' geometry, §9 tables' geometry, §10 selection paint), `BMv` = `step-block-moves-spec.md` (block selection,
drags into and out of lanes and cells, the block clipboard). The new side: `tree` = `crates/mn-doc/src/tree.rs`,
`legacy` = `crates/mn-doc/src/legacy.rs`, `codec` = `crates/mn-store/src/codec.rs`, `lay` = `crates/mn-layout/src/lib.rs`.

Vocabulary of the old code: a **split row** is one `split` block (its **record**) followed by its **lanes'** blocks; a
**layout row** is a split row outside any table (the new `Columns`); a **table** is a head record (`header > 0`) plus
the headerless records directly under it, each record one table row and each lane one **cell**; a **typed** cell is a
body cell of a choice, check or timecode column; a **set** is a grip-picked group of whole rows or whole columns; a
**cell rectangle** is a selection whose two ends are in different cells of one table.

---

## 0. The shape everything below rests on

| Item | Rule | Ref |
|---|---|---|
| One mechanism | Columns and tables are the same stored thing: split records and lane blocks in the flat list. A table is *derived* (a header count on a record), never stored as an object | `BMh:230-242`, `BM:331-359`; `S1` §4.2–4.3 |
| A cell is a lane | Every rule that holds for a lane holds for a cell unless a table says otherwise: a cell holds one or more ordinary blocks and the caret walks them as it walks any blocks | `BMh:261-266` |
| Two differences | (1) a table's widths are px per column on the head; a layout row's are fractions on its record. (2) "Tables keep shape, layouts follow content": an emptied cell is refilled with an empty paragraph; an emptied lane disappears | `BM:381-416`, `BM:555-581`, `BM:2186-2226` |
| No nesting | No record inside a lane: no table in a lane, no lanes in a cell, no lanes in lanes. Tables always sit at the top level | `BM:314`, `ManE:92-94` |
| One caret, one selection | The page's single anchor / focus pair serves cells too. A cell rectangle and "the whole table" are *readings* of that pair, not separate state. The only extra state: a grip-picked set (`tableSet`) and the "selected as an object" flag (`selObject`) | `Ed:2402-2412`, `Ed:802`, `Ed:2416-2420` |
| Every grid edit | Rebuilds the whole table band (`rebuildTable` → `replaceBand`) inside one undo step | `BM:583-626`, `BM:628-638` |
| Limits | 63 columns per table; a lane index 0…63; `insertTableRows` takes 1…10 000 rows; imports stop at 100 000 cells (§7.2) | `BM:695`, `BM:2112-2113`, `Imh:60` |

Already ruled for the rebuild (canon row 273) and already built: `tree` has `Columns → Column{width}` and
`Table{header_rows, columns} → Row{bg, fg} → Cell{bg, fg}` as blocks with ids; `Kind::may_hold` allows a table in a
column and forbids columns in a column, a table in a cell and columns in a cell (`tree:56-67`); `legacy::convert_page`
turns the stored records into that tree (§1.5); `codec` writes it; `lay` places columns side by side and sizes a
table's columns and rows (§6.4). None of the *editing* below exists on the new side yet.

---

## 1. Storage

### 1.1 Where each thing lives (2.0 file)

`S1` §4.2–§4.4 and §5.1 give the encodings with worked examples; this table is the index.

| Thing | Stored as | Ref |
|---|---|---|
| A layout row | One `split` block, `content ""`, `attrs.ratios` = one fraction per lane (sum 1, 4 decimals) | `S1` §4.2 |
| A lane's content | The following blocks, each with `attrs.cell = <lane index>` | `S1` §4.2 |
| A table | Nothing of its own. Its first row's record carries `attrs.header` (1…255) and `attrs.table.cols` | `S1` §4.3 |
| A table row | One `split` block; `attrs.table` may hold `bg`, `fg`, `cbg[]`, `cfg[]` | `S1` §4.4 |
| A cell | The row's blocks with `cell = <column index>`; never zero blocks while the app is running | `BM:555-581` |
| Header rows | The first `header` rows of the table. No other mark on those rows or their cells | `BM:2052-2055` |
| Column width | `cols[c].w`, integer px; absent = automatic | `BM:2005-2028` |
| Column alignment | `cols[c].a`: 1 centre, 2 right; absent = left | `BM:931-947` |
| Column colours | `cols[c].bg`, `cols[c].fg`, strings as given (hex in every caller) | `BM:913-929` |
| Column kind | `cols[c].k`: 1 choice, 2 check, 3 timecode; absent = text | `BM:1355-1408` |
| Choice options | `cols[c].o` = `[{"id","l","c"}]`, only when `k = 1` | `BM:1421-1449` |
| Timecode rate | `cols[c].fps`, a number; absent reads as 24 | `BM:1266-1270` |
| Row colours | `table.bg`, `table.fg` on that row's record | `BM:903-911` |
| Cell colours | `table.cbg[c]`, `table.cfg[c]` on the row's record; `""` = none; trailing empties trimmed | `BM:821-828`, `BM:890-901` |
| A choice cell's value | One paragraph whose whole text is one `choice` span; the span's payload repeats the column's full option list and `v` = the selected id. An empty paragraph = no value | `BM:1192-1209` |
| A check cell's value | The same, with the fixed options `0 To do #8A8A8A`, `1 Doing #0189F1`, `2 Done #58A65C`. State 0 is stored as the **empty** paragraph, never as a chip | `BM:1157-1168`, `BM:1562-1573` |
| A timecode cell's value | A plain paragraph whose text is the normalised timecode (§2.4) | `BM:1272-1287` |
| A text cell | Any blocks: paragraphs, headings, quotes, lists, code, media, dividers; inline chips allowed | `Man:9-10`, `Ed:2982` |
| Table `ratios` | Written as equal fractions on every table record, never read | `BM:608-609`; `S1` §8 #23 |

Not stored anywhere in the file: sort order as a setting (a sort rewrites the row order), the row filter, the grip
selection, the board view and its grouping column (app settings, §8), the frozen header (pure view).

### 1.2 Colour resolution

A cell's background and text colour: the cell's own, else its row's, else its column's (`BM:854-863`). Setting a
*text* colour on cells, rows or columns also removes every text-colour span inside the affected cells, so the cell
colour is what shows ("make this row red makes the row red"); setting it to `""` strips them too (`BM:879-888`,
`BM:890-929`). Background colours never touch spans.

### 1.3 Ragged rows

A row may have fewer cells than the table has columns; the missing cells are at its end. Column count =
`max(len(cols), 1 + the largest cell index in any row)` (`BM:343-352`). A missing cell is created (one empty paragraph)
the first time something writes to it (`ensureGridCell`, `BM:1533-1542`). Interior holes are refilled after every
mutation (`refillTableCells`, `BM:555-581`).

### 1.4 The import intermediate (`TableGrid`)

Not a stored form: a rectangular grid the CSV / TSV / XLSX / ODS readers and the TSV paste build, turned into records
and cells by `tableSpecsFromGrid` (`BM:1579-1657`). Its JSON keys (`TGh:12-18`): `cols`, `header`, `w[]`, `a[]`,
`rbg[] / rfg[]` (row colours), `cbg[] / cfg[]` (column colours), `rows[][]` where a cell is a string or
`{t, s, bg, fg, m}`; a `ct` map carried column kinds in the retired Table block. `tableSpecsFromGrid` forces at least
one header row, caps at 63 columns, writes a media cell as a media block followed by a paragraph only when the cell
also has text, and writes choice / check values as chips (`BM:1611`, `BM:1582`, `BM:1634-1653`).

### 1.5 What the new tree maps, and what it drops

| Old | New (`legacy::convert_page`) | Note |
|---|---|---|
| Layout record | `Columns`, keeping the record's id | `legacy:139-150` |
| Lane *k* | `Column{width}` with a derived id; widths by the load repair's ratio rules | `legacy:62-76`, `legacy:145-149` |
| Run of table records | `Table{header_rows, columns}` with a derived id; each record becomes a `Row` keeping the record's id | `legacy:151-178` |
| `cols[c]` → `ColumnSpec{width, align, bg, fg, kind}`; `o` → `ChoiceOption{id, label, color}`; `fps` default 24 | `legacy:78-87` | every key of §1.1 is carried |
| Row `bg` / `fg` → `Row{bg, fg}`; `cbg[c]` / `cfg[c]` → `Cell{bg, fg}` | `legacy:169-173` | |
| Ragged rows, empty cells | Squared off: every row gets a cell per column, every cell at least an empty paragraph; `header_rows` clamped to the row count | `legacy:187-198` |
| A cell's lane number | Kept as its column (the old load repair could shift a cell left over a gap; the new reader does not) | `legacy:9-11`, `legacy:124-127`; `S1` §8 #16 |
| A layout row with one lane | Unwrapped to top-level blocks, with a note | `legacy:132-134` |

Dropped or changed on conversion:

- A table record's `ratios` (never meaningful).
- Layout lanes beyond 64 share the last column (`legacy:131`).
- A cell's typed value stays what it was: a paragraph with a chip span (choice, check) or plain text (timecode). The
  new tree has **no typed-value field on `Cell`**; canon row 273 speaks of "a typed value". Whether the value stays a
  chip in a paragraph or moves onto the cell is not decided (§10 Q3).
- The chip's payload repeats the whole option list per cell (`S1` §1.6). The new `ColumnKind::Choice{options}` already
  holds the list once; the per-cell copy survives conversion inside the span (§10 Q4).
- A column wider than the 63-column cap, a `header` on a non-record: see `S1` §4.5.

`codec` writes `table` attrs `header`, `cols[{w, a, bg, fg, k, o, fps}]`; `row` and `cell` attrs `bg`, `fg`; `column`
attr `width` rounded to 4 decimals (`codec:21-43`). Nothing of §1.1 is without a place in it.

---

## 2. Column kinds

There are four kinds: 0 text, 1 choice, 2 check, 3 timecode (`BMh:308-334`). **No number, date, link, person or
formula kind exists** in the code; "numeric" appears only as a sort rule for text columns (§2.6). Header-row cells are
plain text in every kind (`Ed:2460`, `BM:1545`).

### 2.1 Text

Any blocks (§3.1). The only kind that accepts dropped files and dragged blocks (`Ed:1522`, `Ed:878-880`).

### 2.2 Choice

| Item | Rule | Ref |
|---|---|---|
| Arity | Single value per cell. No multi-select | `BM:1544-1556` |
| Option | `{id (ULID), label, colour}`; the label is sanitised (Markdown-active characters and newlines removed; empty → not added) | `BM:1451-1459`; `S1` §1.6 |
| Cell | One paragraph = one chip; `""` clears to an empty paragraph | `BM:1192-1209` |
| Pick by mouse | A click anywhere in the cell parks the caret at its start and opens the picker under the cell | `Ed:6652-6661` |
| Pick by key | With the caret in the cell: Space opens the picker; any printable character opens it with that character in its field (the list filters; Enter picks). Other typing does nothing | `Ed:3511-3520` |
| Picker | The options (click selects), Clear, a field that adds a new option **and selects it** (colour = `palette[count % n]`), "Edit options…" | `ChP:63-64`, `ChP:104-182` |
| Option editor | A modal working on a draft: rename, reorder (up / down), delete, recolour (curated swatches + a full picker), add. Cancel discards; Done commits the whole set as **one** undo step (`tableSetColumnOptions`): existing ids are kept so cells keep their value, an option that went clears the cells that had it | `ChE:55-91`, `BM:1504-1517` |
| A change to the set | Rewrites the head's `o` and then **every chip in the column** (each carries the list) | `sweepColumnOptions`, `BM:1421-1449` |
| Per-option calls | `tableAddOption / RenameOption / RecolorOption / MoveOption / RemoveOption` exist; the QML calls only `AddOption` (the picker's field) and `MoveOption` (the board's lane menu) | `BM:1451-1502`, `ChP:166`, `Ed:394-396` |
| Shared sets | None: two columns with the same statuses are unrelated. Ruled out permanently in the old plan ("a document, not a database") | local notes |

### 2.3 Check

Three states, not two: 0 to do, 1 doing, 2 done (`BM:1558-1577`). A click on the box (hit zone `colLeft − 2 … colLeft
+ 18`) or Space with the caret in the cell cycles 0 → 1 → 2 → 0; key repeat is ignored (`Ed:6662-6667`,
`Ed:3480-3487`). The cell's text is the hidden label; the box is drawn over it (`L2` §9.4). It is the same primitive
as a task list item's box.

### 2.4 Timecode

| Item | Rule | Ref |
|---|---|---|
| Cell | A plain paragraph. Free typing; normalised afterwards | `BMh:331-334` |
| Rate | Per column, `fps`; default 24. The menu offers 23.976, 24, 25, 29.97, 30, 60. Changing it re-normalises every body cell | `BM:1266-1270`, `Ed:8454-8463`, `BM:1335-1353` |
| Formatter | 23.976 → 24000/1001 non-drop; 29.97 → 30000/1001 **drop-frame**; 59.94 → 60000/1001 drop-frame (each within ±0.01); anything else → `round(fps)`/1 non-drop. There is no way to ask for 29.97 non-drop | `BM:1243-1249` |
| Reading a cell | Trimmed. Empty → not a timecode. A bare integer → that many frames (negative → 0). Else exactly four integer fields split on `:` (a `;` is read as `:`): `hh ≥ 0`, `mm 0…59`, `ss 0…59`, `ff 0 … displayFps − 1`; anything else is unreadable | `BM:1256-1264`, `TC` `parse` |
| Writing | `hh:mm:ss:ff`, or `hh:mm:ss;ff` for a drop-frame rate (per `TC`'s header comment; the formatting functions themselves were not read) | `BM:1251-1254`, `TC:44-47` |
| When it normalises | On leaving the cell (the same hook that consumes inline Markdown: `commitMarkdown`, its own undo step), when a value is pasted or filled in, when the kind is set, when the rate changes | `BM:5232-5240`, `BM:1703`, `BM:1824`, `BM:1403-1404` |
| Unreadable text | Left exactly as typed. No error mark, no refusal | `BM:1280-1281` |
| Scope | Body cells that are a paragraph; header rows and other block types are skipped | `BM:1275-1277` |
| Drawing | None of its own: plain text in the block view | `L2` §9.4 |
| Entry format | No mask, no partial forms (`1:02` is unreadable), no frames-vs-timecode display switch | not found |

### 2.5 Changing a column's kind (`tableSetColumnKind`, `BM:1355-1408`)

One undo step; needs a header (`headerCount > 0`). For every **body** cell the cell's blocks are joined with a space
and simplified to one value `v`; the cell is then cut down to **one block** (the first), which is rewritten:

| To | The cell becomes | Ref |
|---|---|---|
| Choice | Options are **harvested**: the distinct non-empty values of the column in order of first appearance (compared trimmed and case-folded), each given a new id and `palette[i % 8]` from `#8A8A8A #0189F1 #58A65C #E5A33B #D9534F #9B6BD6 #2BB3A6 #C6689D`. Each cell becomes the chip of its value; an empty cell stays empty | `BM:1368-1382`, `BM:1395` |
| Check | A cell whose value equals a state's label (`To do`, `Doing`, `Done`, case-folded) takes that state; anything else becomes empty (= to do). The old text is lost | `BM:1395-1396` |
| Timecode | Plain text `v`, then normalised | `BM:1394`, `BM:1403-1404` |
| Text | Plain text `v` (a chip becomes its label, a done box becomes the word `Done`); `k` and `o` are removed from the spec | `BM:1384`, `BM:1394` |

Consequences read from the code: a cell that held several blocks, a list, an image or formatting loses all but the
joined plain text when its column becomes typed; header cells are untouched. The menu marks only "Make text column" as
destructive (red) (`Ed:8464`). `tableSetColumnsKind` applies one kind to a set of columns as one step (`BM:1410-1419`);
the bulk menu offers choice, check and text, not timecode (`Ed:8516-8521`).

### 2.6 Sort and filter

| Item | Rule | Ref |
|---|---|---|
| Sort | A one-shot command (column menu: ascending / descending), shown when the table has ≥ 2 body rows. It **reorders the stored rows**; nothing remembers a sort. Stable; header rows stay | `Ed:8436-8444`, `BM:999-1046` |
| Text column | The cell's blocks joined with `\n`; numeric when both sides parse as numbers; numbers before text; else case-insensitive text | `BM:1010-1016` |
| Choice column | By option order; a cell with no value after every option | `BM:1028-1031` |
| Check column | By state 0, 1, 2 | `BM:1028` |
| Timecode column | By frames; unreadable or empty last | `BM:1023-1026` |
| No header glyph | `L2` §9.1 records 18 px reserved in the first row "for the sort glyph"; no clickable glyph was found in the current QML | `BM:398` |
| Filter | Only in the table's own full-window view (§8.2): a text field; body rows with no cell containing the text (case-insensitive) fold to zero height. View only: never stored, cleared on leaving, copy / export / undo see every row; a "Showing n of m rows" count | `Ed:368-382`, `BM:1321-1333`, `BM:1288-1319` |

---

## 3. Editing inside a table

### 3.1 What a cell holds

| Item | Rule | Ref |
|---|---|---|
| Text-column cell, header cell | One or more blocks of any leaf kind: paragraph, heading, quote, bullet / numbered / task item, code, media (image, video, PDF, file…), divider. Never a record | `BMh:261-266`, `Man:9-10` |
| New cell | One empty paragraph | `BM:588-591` |
| More blocks | Shift+Enter splits the block in its cell; Enter in a list item or code block behaves as it does outside (a new item / a newline); a paste or a drop adds blocks | `Ed:1865-1871`, `Ed:1813-1859` |
| Typed body cell | Exactly one paragraph. It refuses text input, inline chips, dropped files and dragged blocks | `Ed:3511-3520`, `Ed:2982`, `BM:1909`, `Ed:878-880` |
| Media in a cell | Sized to the column less the 8 px insets; a video in a cell narrower than 300 px shows no transport and a click opens the review view | `Ed:294`, `Ed:1030`, `Ed:6646-6650` |
| Files dropped on a cell | Appended as media blocks, in order, after the cell's last block; the cell is highlighted while aimed at | `BM:1907-1925`, `Ed:1563-1571` |

Canon row 273 says a cell holds "text, a typed value or ONE media item"; the old app (and `tree::may_hold`, and the
old files) allow several blocks per cell. See §10 Q1.

### 3.2 Caret movement

There is no table-specific arrow code: the same functions serve lanes and cells (`BMh:208-214`).

| Key | Rule | Ref |
|---|---|---|
| Left / Right | Within the block; at its start / end, the previous / next block in **reading order** — the cell's blocks top to bottom, then the next cell to the right, then the next row's first cell; out of the table at either end | `Ed:1991-2019`, `BM:2827-2837` |
| Up / Down | Within the block by visual line; at its top / bottom, the previous / next block of the same cell; at the cell's top / bottom, the row above / below entered at the caret's page x (which lands in the same column because columns are px-aligned). Entering from above lands in the cell's **first** block, from below in its **last** | `Ed:2512-2546`, `BM:2839-2863` |
| Goal x | Page-relative, kept across a run of vertical moves | `Ed:2520-2521` |
| Tab | The next cell, landing at the **end of its last block**, no selection. Past the row's last cell: the next row's first cell. Past the table's last cell: a row is appended and the caret lands in its first cell (not on key repeat) | `Ed:2484-2499`, `BM:3349-3361` |
| Shift+Tab | The previous cell (end of its last block); from a row's first cell, the block before it in reading order (the row above's last cell; above the table from the first row) | `BM:3356` |
| Tab never indents here | List indent / outdent in a cell is Cmd+] / Cmd+[ | `Ed:3401-3407` |
| Enter (paragraph, heading, quote) | **Down the column**: the same column of the next row, at the end of its last block. In the last row: a row is appended and entered. In an **empty last body row**: the row is removed and the caret lands in a new paragraph below the table. A header row never exits. Key repeat only navigates | `Ed:1864-1868`, `Ed:2386-2400`, `BM:1103-1124` |
| "Empty row" for that exit | Every block of the row is an empty paragraph, heading or quote (an unset typed cell counts as empty) | `BM:1103-1111` |
| Shift+Enter | Splits the block inside its cell (a second paragraph in the same cell) | `Ed:1865` |
| Cmd+Enter | Inserts a row below the caret's row, caret in the same column (not on key repeat) | `Ed:3471-3474`, `Ed:2451-2457` |
| Home / End, Page Up / Down | Unchanged: the document's start / end, a viewport | `Ed:2611-2637` |
| At the document's edge | Left / Up from the first cell of a table that starts the page, or Right / Down from its last cell at the page's end, makes a top-level paragraph above / below the table and puts the caret there | `Ed:1984-1990`, `BM:1126-1136`, `BM:3236-3252` |
| Row filter | A caret is moved out of a row the filter hides; arrows entering a hidden row are a known unfixed case | `Ed:373-380`; local notes |

### 3.3 Typing

Text cells type as any block does, including Markdown-as-you-type: no gate on cells was found in
`applyMarkdownTrigger` or `commitMarkdown` (other than the timecode branch), so `# `, `- `, `> `, `1. `, `[ ] ` and the
inline marks convert inside a cell as they do outside (`BM:4803-4830`, `BM:5228-5245`) **(inference from the absence of
a gate)**. Header cells draw their text bold (`L2` §9.2). Backspace at the start of a cell's first block does not join
into the previous cell: blocks merge only inside their container; on the only, empty block of a cell it does nothing,
since a cell refills (`Ed:2022-2031`, `BM:3173`).

### 3.4 Selection

| Grain | How it is made | What it is | Ref |
|---|---|---|---|
| Text in one block | Drag, Shift+arrows, double-click (word), triple-click (block) | Ordinary text selection | `Ed:6636-6644`, `Ed:6813-6831` |
| Several blocks of one cell | Drag / Shift+arrows within the cell; Cmd+A rung 2 | Ordinary range | `Ed:2569-2600` |
| Cell rectangle | Any selection whose ends are in **different cells of one table**: drag across cells, Shift+click another cell, Shift+arrows across a cell edge **(inference: the rectangle is derived from the anchor / focus pair, `Ed:2402-2412`)** | `{r0, c0, r1, c1}` = the bounding rectangle of the two ends' cells. Painted as whole filled cells; text bands suppressed | `Ed:2402-2412`; `L2` §10.3 |
| Row / column **set** | A grip (§4.1): click = that row / column alone; Shift+click = the span from the last pick; Cmd+click = toggle. Also "Select row" / "Select column" in the menu | Whole rows **or** whole columns, never mixed, need not be contiguous. Lasts until the caret moves. The caret parks in the pick (a row's first cell; a column's first body cell) | `Ed:2276-2291`, `Ed:2340-2346`, `Ed:8414`, `Ed:8477` |
| Whole table as cells | Cmd+A rung 3 | The rectangle covering every cell | `Ed:2582-2585` |
| A row as an **object** | Esc with a caret in a cell (selects the row and flags it) | Delete removes the row | `Ed:3322-3331` |
| The table as an object | Esc again | Delete removes the table | `Ed:3299-3309` |
| A range leaving the table | A selection with one end inside and one outside | Widens to the **whole table** ("no orphaned table elements") | `Ed:2086-2100`, `BM:6410-6419` |

Cmd+A ladder inside a cell, one rung per press: the block → every block of the cell → every cell of the table → the
document (`Ed:2569-2600`). Esc ladder: any selection collapses first; then caret → its row (object) → the table
(object) → the caret in the block below the table (a paragraph is made if none) (`Ed:3297-3331`; `BMv` §1.2). The same
visual selection can therefore be "cells" (reached by Cmd+A or a drag: Delete clears) or "an object" (reached by Esc:
Delete removes); nothing on screen was found that tells them apart **(inference)**.

### 3.5 Copy and cut

| Selection | Own payload | Plain text | HTML | Ref |
|---|---|---|---|---|
| Inside one cell | The blocks, as outside a table | Their text | none | `Ed:2936-2944`, `BM:6060` |
| Cell rectangle, or a set | A **cell fragment**: the chosen rows × columns as records and cell blocks, plus `grid = {cols: [the columns' specs], header: how many of the rows were header rows}`; row colours and the cells' own colours travel | TSV: a cell's blocks joined by `\n` (media and dividers skipped), a choice as its label, a check as `[ ]` / `[/]` / `[x]`; a field holding a tab, newline or quote is quoted, `"` doubled | One `<table>`, header rows in `<thead>`, colours and alignment, check boxes as glyphs | `Ed:2914-2935`, `BM:6681-6737`, `BM:6642-6679`, `Ex:1872-1927` |
| A column set | Every row of the table × those columns (header rows included) | same | same | `Ed:2928-2931` |
| A range holding whole tables / layout rows | The blocks with their structure | Markdown (tables as pipe tables, lanes stacked) | The page exporter's fragment | `Ed:2962-2969` |

Cut = copy, then the delete of §3.7 (a rectangle or a set is **cleared**, not removed) (`Ed:3107-3111`). The old code
keeps a private clipboard type (`application/x-mnd-blocks`); the rebuild carries blocks in the HTML (canon row 308).

### 3.6 Paste

The router tries the clipboard's forms in a fixed order (`CP:33-48`): own blocks → (in a code block: text verbatim) →
HTML → file URLs → picture → text. For tables:

| Clipboard | Caret | Result | Ref |
|---|---|---|---|
| Own cell fragment, or own copied table rows | In a table cell | **Has header rows** (and "Paste into cells" not chosen): *append by label*. Else *fill by position* | `CP:136-151`, `BM:1828-1905` |
| Own cell fragment | Outside a table | A new table; its first row becomes the header unless the fragment says more | `CP:149-151`, `BM:1762-1772` |
| HTML holding only a table | In a table cell | No `<thead>` / all-`<th>` row found (spreadsheets write none): fill by position. Found: append by label | `BM:5925-5934` |
| HTML holding a table | Elsewhere | A new table; a head is always given (first row when none detected) | `BM:5935`, `BM:1774-1784` |
| Plain text with a tab or a newline | In a table cell | Read as TSV and filled by position from the caret's cell (from a rectangle's top-left cell when one is selected) | `CP:45`, `Ed:3077-3083`, `BM:1659-1711` |
| Plain text that "looks tabular" | Outside a table | A new table below the caret's block, first row the header. Tabular = ≥ 2 lines, every line non-empty, every line the **same** number (≥ 1) of tabs | `CP:17-31`, `BM:1713-1730` |
| Markdown pipe table inside pasted text | Outside a table | A table with its alignment row honoured | `BM:5640-5710` |
| Any table or layout row | Caret in a **layout lane** | Lands below the split row, with a message ("split rows and tables can't go inside a lane") | `BM:6755-6760`, `Ed:1606` |
| Paste Special ▸ "Paste into cells" (cell menu) | In a table | Forces fill by position: a grid's header rows paste as content | `Ed:8387-8388`, `Ed:2948-2953` |

**Fill by position** (`BM:1881-1891`, `BM:1659-1711`): from the anchor cell rightwards and downwards; rows and columns
are **appended** as needed (the table grows, up to 63 columns); each target cell's content is **replaced**.
**Append by label** (`BM:1836-1880`): the source's first header row names its columns; each is matched to the target's
first-header-row label (trimmed, case-folded; duplicates in order); a source column with an empty label goes by
position; an unmatched one is **added as a new column** with its header text, kind and options; the source's body rows
are inserted **below the caret's row** (at the table's end when the caret is in a header row); the source's header rows
are not pasted. The rule's own words in the code: "position without a header, labels with one".

Into a typed column, by either rule (`BM:1793-1826`, `BM:1691-1704`): choice — the pasted text is matched to an option
by label, else **a new option is added** (colour `#8A8A8A`); check — `[x]` / `[/]` / `[ ]` or the state labels;
timecode — text, then normalised. A TSV fill writes one plain paragraph per cell (formatting and extra blocks of the
target are dropped).

Known limit, recorded in the local notes: a paste anchors at the focus cell (or a rectangle's top-left) and ignores a
grip set.

### 3.7 Delete

| Selection | Backspace / Delete | Ref |
|---|---|---|
| Text in a cell | Deletes the text | — |
| Cell rectangle (incl. Cmd+A's whole table) | Clears the cells' contents to one empty paragraph each; rows, columns and **colours stay** | `BM:6396-6404`, `BM:757-766` |
| Row / column set | Clears contents, one undo step. Removing rows or columns is menu-only by ruling | `Ed:3465-3468`, `Ed:2311-2316`; `Man:74-75` |
| A row picked by Esc | Removes the row (a header row: the whole table; the last body row: refused) | `Ed:2421-2433`, `BM:661-669` |
| The table picked by Esc | Removes the table | `BM:750-755` |
| A range reaching into a table from outside | Removes the whole table with the rest of the range | `BM:6410-6418` |
| Key repeat | Never removes structure | `Ed:1720-1722` |

Typing over a selection deletes it first by the same rules (`Ed:1766-1767`), so typing over a rectangle clears it and
types into its top-left cell.

---

## 4. Structure edits

### 4.1 Entry points

| Action | Grip / handle | Menu (right-click a cell) | Key | Ref |
|---|---|---|---|---|
| New table (header + 2 body rows × 3 columns) | Toolbar ▸ Insert block ▸ Table | Insert table below (hidden inside a table) | — | `Tb:198`, `Ed:2904-2910`, `Ed:8366` |
| Add a row at the end | The "+" strip under the caret's table (full table width, 14 px high, 4 px below) | — | Tab past the last cell; Enter in the last row | `Ed:7986-7996`, `Ed:2328-2332` |
| Insert a row above / below | — | Row ▸ Insert row above / below | Cmd+Enter (below) | `Ed:8479-8482` |
| Add a column at the end | The "+" strip right of the caret's table (14 px wide, table height, 4 px right) | — | — | `Ed:7997-8007`, `Ed:2333-2337` |
| Insert a column left / right | Cmd-drag out of a cell's left / right inner edge (8 px band, > 12 px pull) | Column ▸ Insert column left / right | — | `Ed:2195-2204`, `Ed:8416-8419` |
| Move a row | Drag its row grip (left of the row) or its rail handle: an ordinary block drag of the row | Row ▸ Move row up / down (body rows) | Opt+Cmd+Up / Down with the row selected **(inference from `moveTarget`)** | `Ed:6715-6725`, `Ed:8484-8487`, `BM:3254-3285` |
| Move several rows | Drag a row grip inside a contiguous grip-picked body-row set | — | — | `Ed:2255-2268` |
| Move a column | Drag its column grip (above the first row): an accent line shows the gap, chosen by column midpoints; the moved column stays picked | Column ▸ Move column left / right | — | `Ed:2245-2251`, `Ed:2317-2327`, `Ed:8420-8423` |
| Duplicate | — | Row ▸ Duplicate row (body rows; the copy below); Column ▸ Duplicate column (the copy to the right, spec included) | — | `Ed:8488`, `Ed:8424`, `BM:683-691`, `BM:738-748` |
| Delete a row / column | — | Row ▸ Delete row; Column ▸ Delete column; the set menu's Delete n rows / n columns | Esc then Delete (a row) | `Ed:8491`, `Ed:8467`, `Ed:8524-8530` |
| Delete the table | — | Table ▸ Delete table; "Delete row" on a header row; a set that includes a header row | Esc, Esc, Delete | `Ed:8400`, `BM:664`, `BM:958` |
| Move the table | Drag a **header row's** grip or rail handle: it carries the whole table | Move up / Move down | — | `Ed:2255-2259` |
| Fill down / right | — | — | — (the model has `tableFillDown / FillRight`, tested at `T:7426-7429`; no caller was found in the QML) | `BM:768-802` |

Grips (`L2` §9.2): a row grip is the band 18…2 px left of the table beside each row; a column grip the band 18…2 px
above the first row. A press is a click (selection, §3.4) until the pointer moves more than 4 px, then a drag
(`Ed:6715-6717`). Modifiers are read at the press.

### 4.2 Rules of the grid operations

| Operation | Rule | Ref |
|---|---|---|
| Insert row | The new row has as many cells as the row above it (the row below, at index 0); every cell an empty paragraph. No kind-specific default value | `BM:651-659` |
| Delete row | A header row → deletes the table. The last body row cannot be deleted | `BM:661-669` |
| Delete rows (set) | Any header row in the set → deletes the table. A set covering every body row is refused | `BM:953-962` |
| Insert column | Refused at 63. A ragged row too short to reach the index is left alone | `BM:693-702` |
| Delete column | The last column cannot be deleted; a set covering every column is refused | `BM:704-714`, `BM:964-978` |
| Move row | `to` clamped to the table; **no guard between header and body in the model** — the menu hides the cases (body rows only), and a drag is kept out of the header rows by the view (`BMv` §2 R7) | `BM:671-681`, `Ed:8483-8487` |
| Move / duplicate column | The column's spec travels with it | `BM:716-748` |
| Duplicate | Blocks get new ids; cell colours are copied | `BM:644-649` |
| Undo | One step per operation, including each bulk operation | `BM:628-638` |
| A table row dragged out of its table | Allowed: it lands as a top-level split row and follows layout rules (a one-cell row unwraps to blocks) **(inference from `BMv` §2 and `T:8020`'s title "a body row moves out")** | `T:8020` |

### 4.3 Header rows

| Action | Rule | Ref |
|---|---|---|
| Add / remove a header row | Table menu: the count ± 1. "Add" is offered while `header < rows − 1`; "Remove" while `header > 1` | `Ed:8347-8351` |
| Unassign header | Offered on a header row: sets the count to 0. The table stops being a table: its rows become layout rows (the manual: "plain side-by-side blocks"), the column spec is deleted, one-cell rows unwrap | `Ed:8352-8353`, `BM:2092-2099` |
| …directly under another table | The two are **joined by label** instead (columns matched by first-row text, unmatched ones appended; chips re-matched to the surviving options by label) | `BM:2073-2080`, `BM:1048-1100`, `BM:1927-1976` |
| Assign as header | Offered on any layout row and on any table **body** row: gives that record `header = 1`. On a layout row it makes a table (auto-width text columns); on a body row it **splits the table in two** at that row | `Ed:8395`, `Ed:8224`, `BM:2086-2091` |
| A table with no header | Cannot exist: `header ≥ 1` is what makes a table. Every import and paste gives one | `BM:596`, `BM:1774-1784` |
| Break and rejoin | Any top-level block placed between two rows ends the table there; removing it rejoins the rows (they never had settings of their own) | `S1` §4.6 D |

In the new tree a table is an object, so "unassign" and "assign" have no stored meaning; what remains is the user's
gestures they stood for (table → columns / blocks; columns → table; split a table; join two tables). See §10 Q7.

### 4.4 Column width

| Item | Value | Ref |
|---|---|---|
| Automatic width | widest hard line of any text block in the column + 22 px, clamped 48…360; 160 when nothing measures; follows typing live | `BM:381-416`; `L2` §9.1 |
| Manual width | Drag a column's right border (hit ± 4 px, the last column's too): a 2 px accent line previews; written on release; never under 48; no snapping | `Ed:2113-2123`, `Ed:2161-2165`, `Ed:2169-2176` |
| Stored | `cols[c].w`, integer px, clamped 48…4000 | `BM:2005-2007` |
| Back to automatic | `setTableColumnWidth(≤ 0)` removes `w`; **no gesture or menu item calls it** (no double-click on a border, no "auto width" item was found) | `BM:2006`, `BM:2012-2013` |
| Independent of the page | A table's width is the sum of its columns; the page width does not enter | `L2` §9.3 |
| Imported widths | Kept only where the source authored them | `Im:774-778` |

### 4.5 Alignment and colours

Alignment: per column, left / centre / right, three buttons in the column menu and in a column set's menu; applies to
every block of the column, header rows included (`Ed:8427-8435`, `Ed:8506-8514`, `BM:931-947`). There is no per-cell
or per-row alignment and no vertical alignment (cells are top-aligned, `L2` §9.2).

Colours come from the app's palette panel (text colour / background), targeted by what is selected (`Ed:2725-2741`):

| Selection when a colour is picked | Target |
|---|---|
| A row set / a column set | The rows' / the columns' colour |
| A cell rectangle | Each cell's own colour |
| A caret in a cell, nothing selected | That cell's colour |
| Text selected inside one cell | A span on the text, as outside a table |

"Revert to default" sets the same target to `""` (`Ed:2674-2684`). There is no colour item in the table menus.

### 4.6 Not present

Merged cells (no row span or column span: imports keep a merged cell's content in its origin cell and leave the
covered cells empty, `Im:786`); a command to turn selected text into a table or a table back into text (the only
conversions are the header role of §4.3, pasting tabular text, and Copy as Markdown); a caption or title for a table;
row height settings (a row is as tall as its tallest cell, minimum 31, `L2` §9.2); hiding a column; a formula or total
row.

---

## 5. Split rows / lanes (the new Columns)

### 5.1 Making one

| Way | Result | Ref |
|---|---|---|
| Block menu ▸ "Split into columns" | The block (or the selected top-level run, as one lane) on the left, a new empty paragraph on the right, 50 / 50; the caret goes to the new paragraph | `Ed:8376-8377`, `Ed:2348-2351`, `BM:2995-3030` |
| Cmd-drag from a block's inner left or right edge (8 px hot band, shown as a 3 px accent bar) | A divider follows the pointer, snapping to ¼ ⅓ ½ ⅔ ¾ of the block's width within 8 px, both sides ≥ 120 px; released after > 12 px it splits there; the new empty lane is on the side pulled **from**. With a top-level run selected, the whole run becomes one lane. Esc cancels | `Ed:2132-2146`, `Ed:2181-2213`, `Ed:6610-6618`, `Ed:6693-6700` |
| The same pull on a block already in a lane | A new lane beside that lane, taking part of its width | `BM:2351-2385` |
| A dragged block (or run) dropped on a block's side band (`min(24, width / 6)`) | A new lane on that side holding the run, 50 / 50 | `Ed:882-885`, `BM:3287-3310`; `BMv` §2.4 |
| A file dropped on a block's side band | A new lane holding the media | `BM:3335-3347` |
| Paste / import | Own payload keeps its lanes; foreign HTML has none (a pasted layout comes back stacked) | `BM:6033-6052` |
| Not offered | No toolbar button, no key, no Markdown trigger makes a split row. Not on a record, not in a table cell (there the pull adds a table column) | `BM:2313`, `Ed:2135` |

### 5.2 Lanes, widths, resize

| Item | Rule | Ref |
|---|---|---|
| Count | 2 … 63 in practice (a new lane is refused at 63; the structure pass allows 64) | `BM:2347`; `S1` §8 #18 |
| Width | A fraction per lane of `page width − 24 × (n − 1)`; 24 px gap; minimum 120 px, applied when fractions are written (raised lanes are paid for by the wider ones in proportion); a page too narrow for the minimum gets equal lanes | `BM:2881-2901`; `L2` §8 |
| Resize | Drag the gap (hit: the whole 24 px gap); preview line; written on release; snaps to ¼ ⅓ ½ ⅔ ¾ **of the page width** within 8 px; neither neighbour under 120 px. Cmd held means "pull", never resize | `Ed:2148-2178`, `BM:2962-2993` |
| Aligned dividers | Dividers at the same x (± 1 px) in directly adjacent layout rows above and below move together; Option moves this row's alone; the chain stops at a table | `BM:2930-2960`, `Ed:6605-6608` |
| Height | The tallest lane; lanes top-aligned; a hairline in the gap | `L2` §8 |
| Narrow page | Nothing stacks and nothing is re-clamped at layout time: lanes stay side by side at their fractions, down to the page's 320 px minimum (two lanes of 148). The old ruling: "a too-narrow page scrolls horizontally, never stacks" | `Ed:72`; `L2` §8; local notes |

### 5.3 What a lane holds

One or more blocks of any leaf kind (as §3.1's text cell). Never a record: no lanes in a lane, **no table in a lane**
(`BM:314`). Media sizes to the lane's width (`BM:2693-2698`). A table or split row pasted with the caret in a lane
lands below the split row with a message (§3.6).

Canon row 273 changes this: a table **may** sit in a column. The old app has no behaviour to read for that case
(§10 Q9).

### 5.4 Caret and selection

| Item | Rule | Ref |
|---|---|---|
| Left / Right | Reading order: lane 0's blocks top to bottom, then lane 1's… then the block after the row | `BM:2827-2837` |
| Up / Down | Stay in the lane to its top / bottom; then the row above / below at the caret's page x (entering a split row picks the lane under that x) | `BM:2839-2863` |
| Tab / Shift+Tab | The next / previous lane, landing at the end of its last block; past the last lane, the block after the row; before the first, the block before it. Tab never indents in a lane (Cmd+] / Cmd+[ do) | `BM:3362-3365`, `Ed:2484-2499` |
| Enter | Splits the block in its lane. On an **empty paragraph that is the lane's last block**: that paragraph is removed (kept if it is the lane's only block) and the caret lands in a paragraph below the split row (an empty one already there is reused). Not on key repeat; Shift+Enter always adds a block | `Ed:1869-1876`, `BM:1138-1155` |
| Backspace at the start of a lane's first block | Never merges across lanes. If the block is the lane's only block and empty: the lane is removed and the caret goes to the end of the previous lane (else the next lane's start). Not on key repeat | `Ed:2022-2031`, `BM:3167-3187` |
| Delete at the end of a lane's last block | The mirror: an only, empty block removes the lane; the caret goes to the next lane's start (else the previous lane's end) | `Ed:2053-2063` |
| Edge of the document | Left / Up out of a split row that starts the page (Right / Down at its end) makes a top-level paragraph there | `Ed:1984-1990` |
| Selection within one lane | Ordinary range | — |
| Ends in different lanes of one row | Reads as "every lane from the low end's to the high end's"; Delete **clears** those lanes to one empty paragraph each — the structure stays | `BM:6405-6406`, `BM:3189-3213` |
| Ends in different top-level rows | The end inside a split row widens to the whole split row | `Ed:2086-2100`; `BMv` §1.3 |
| Cmd+A | The block → its lane → the whole split row → the document | `Ed:2569-2600` |
| Esc | Caret in a lane → the whole split row selected → the caret in the block below it | `Ed:3310-3331` |
| Copy | A range inside one lane copies as plain blocks; any other range touching a split row widens to whole split rows and keeps their structure | `BMh:1142-1144`, `BM:6054-6066` |

### 5.5 Moving blocks in and out; an emptied lane

Drags and key moves are in `BMv` §2–§4 (a block can be dragged into a lane gap, out to the top level, or to a side
edge; Opt+Cmd+Up / Down stay inside the lane; a whole split row moves as one slot among top-level blocks). After any
change (`normalizeStructure`, `BM:2284-2307`):

| Situation | Result | Ref |
|---|---|---|
| A lane has no blocks left | The lane goes; its width is added to the nearest surviving lane on its **left** (else the first); the others keep theirs | `BM:2142-2180` |
| A layout row has one lane left | The record goes; the lane's blocks become top-level blocks in place | `BM:2216-2218` |
| A layout row has no blocks | The record goes | `BM:2216` |
| A **cell** has no blocks left | Refilled with an empty paragraph | `BM:555-581` |

So a lane can be empty only as "one empty paragraph", and it is the *removal of that paragraph as a block* (Backspace
on it, dragging its last block away, Delete lane) that removes the lane; clearing a multi-lane selection does not.

Canon row 305 already departs: on the new side "a column or a cell the blocks leave empty keeps an empty paragraph …
here that waits for editable columns". See §10 Q10.

### 5.6 Commands on a split row (block menu)

| Item | Rule | Ref |
|---|---|---|
| Align lanes | Offered when some lane holds more than one block: re-splits the row into several rows, one block per lane per row (block *j* of each lane together), shorter lanes padded with empty paragraphs; each new row copies the fractions | `Ed:8378-8379`, `Ed:8190-8191`, `BM:3098-3134` |
| Merge with the row below | Offered when the next block is a split row with the same lane count: the two become one row, each lane the concatenation top to bottom; the upper row's fractions win | `Ed:8380-8381`, `Ed:2353-2357`, `BM:3136-3165` |
| Delete lane | Removes the lane's blocks (then §5.5) | `Ed:8398-8399`, `Ed:2359-2370` |
| Assign as header | Makes the row a table (§4.3) | `Ed:8395` |
| Not present | Reordering lanes; "unsplit" as a command (other than emptying or deleting lanes); a per-row style (border, background) | not found |

---

## 6. Layout numbers

`L2` §8 (columns) and §9 (tables) hold the full tables with sources; the constants a design needs at hand:

### 6.1 Columns

| Quantity | Value |
|---|---|
| Gap between lanes | 24 |
| Minimum lane width | 120 |
| Divider | 1 px `border`, centred in the gap, full row height; 2 px `accent` on hover; hit zone the whole gap |
| Pull band | 8 px inside each block edge; bar 3 px `accent` at 0.55 opacity; a pull under 12 px is a click |
| Snap fractions | ¼ ⅓ ½ ⅔ ¾, within 8 px |
| Side-drop band | `min(24, width / 6)` |
| Padding of a lane | none (blocks keep their own 6 / 6) |
| Minimum page width | 320 |

### 6.2 Tables

| Quantity | Value |
|---|---|
| Cell padding | 8 left and right; none of its own top and bottom (each block's 6 / 6) |
| Row height | tallest cell; minimum 31; cells top-aligned |
| Automatic column | widest line + 22, clamped 48…360; 160 when empty |
| Manual column | 48…4000 |
| Grid lines | 1 px `border` on every cell edge; the column border 2 px `accent` on hover |
| Header rows | fill `surfaceHover`, text weight 700, otherwise as body |
| Space above / below a table | 32 each (the "pocket"; the column grips and the row "+" strip live in it) |
| Row grip | 8 wide, at 13 left of the table, hit 18…2 left |
| Column grip | 8 high, 13 above the first row, hit 18…2 above |
| "+" strips | 14 thick, 4 px off the table's bottom / right edge, shown only for the table holding the caret |
| Column drag line | 3 px `accent`, table height |
| Border resize hit | ± 4 px |
| Check box | 14 × 14 at the cell's left; does not follow the column's alignment |

### 6.3 Overflow and frozen parts

A table wider than the page is neither scrolled in a box nor shrunk: it shifts left by half its overhang (never
closer than 24 px to the view's left edge) and the rest runs past the page's right edge, widening the whole page's
horizontal scroll (`Ed:274-279`; `L2` §9.3). A table narrower than the page is left-aligned and not stretched. Header
rows stick to the top of the view while the table scrolls under them; the first column is mirrored at the view's left
once the page is scrolled sideways past it (`L2` §9.3). A canvas page card clips a wide table.

### 6.4 What the new layout already does

`lay` (`lib.rs:135-160`, `:171-180`, `:238-250`): `Columns` divide `width − 24 × (n − 1)` by each `Column.width`
(equal shares when a width is 0); a table's column is its manual width or widest unwrapped text line + 22, clamped
48…360, 160 when empty; cells pad 8 on all sides; every cell is as tall as its row; rows touch. Not there yet (the
handoff's own list): a table wider than the page, the 32 px pockets, the 31 px minimum row, grid lines' and header
fill's painter parts beyond the first pass, sticky / frozen parts. Differences from the old numbers: the new cell pads
8 **top and bottom** as well (the old cell had no vertical padding of its own); the old first-row + 18 allowance is
not carried.

---

## 7. Export and import

### 7.1 Export

A trailing column that is a text column and empty in every row is left out of every export (`tableExportCols`,
`Ex:611-624`). Typed cells are written as their visible value: a choice as its label, a timecode as its text, a check
as a box.

| Format | Table | Layout row (lanes) | Ref |
|---|---|---|---|
| Markdown | A pipe table: the **first** header row is the header line (further header rows become body rows); the alignment row from the columns; a cell's blocks joined with `<br>`, list items as `• `, code as `` `…` ``, a divider as `---`, images as `![]()`, a check as `[ ]` / `[/]` / `[x]`; `\|` escaped | Lanes **stacked** in reading order; the structure is lost | `Ex:634-690`, `Ex:702-705` |
| HTML | One `<table>` with fixed layout at the app's px column widths (`<colgroup>`), header rows in `<thead>`, cell colours and alignment, check boxes as glyphs; a wide table runs past the page and the page scrolls | `<div class="lanes">` flex row, each lane `flex: <fraction × 1000> 1 0`, 24 px gap | `Ex:1666-1682`, `Ex:1689-1730`, `Ex:1351-1357`, `Ex:1413-1414` |
| DOCX | One table: column widths = px × 15 (twentieths of a point), scaled down to the text width when wider; header rows marked to repeat; cell shading, text colour, alignment; cell text at 7 pt | A borderless one-row table, one cell per lane at its fraction, the gap as the cell's right margin | `Ex:2583-2652`, `Ex:2654-2700`, `Ex:1973` |
| PDF | One table per **band of columns**: widths from the content, not the screen (cell text 6 pt); columns that do not fit one page width continue in further bands, the header rows repeating on every page and a narrow "key" column repeating on every band, with a "columns a–b of n" line | A borderless one-row table at the lanes' fractions | `Ex:3505-3641`, `Ex:3360-3398`, `Ex:2940` |
| Copy as Markdown / the HTML fragment of a copy | The same emitters over a range | same | `Ex:780`, `Ex:1863` |
| Package (`.mnpkg`) | The records as stored; cell media travel as any media | same | `T:8823` (title) |

Not exported by any format: a column's kind as such, option lists and option colours (only the selected label's text;
the HTML chip styling was not read in detail), the timecode rate, the row filter.

### 7.2 Import and foreign paste

| Source | Rule | Ref |
|---|---|---|
| CSV, TSV | Comma / tab separated with quoted fields; one table; first row the header; trailing wholly empty rows and columns trimmed; every cell plain text. **No type inference** (ruled: a column is converted by hand afterwards, §2.5) | `TG:315-357`, `TG:359`, `TG:158-176`, `Im:199-207` |
| XLSX, ODS | One table per sheet (a level-2 heading per sheet when there are several); pictures land in their cells; merged cells keep their value in the top-left cell; date serials stay raw numbers; formulas give their cached value; column widths and alignment where authored | `Xh:3-10`; `T:8725`, `T:8823` (titles) |
| HTML, DOCX, ODT, RTF (through a text document) | Header rows = the source's header-row count or the leading rows made only of `<th>`; a file's table always gets at least one. Merged cells: content in the origin cell, covered cells empty. A nested table is flattened to one paragraph per inner row, its cells joined with ` · `. Rich cell content (several blocks, lists, images, marks) is kept. Cell backgrounds other than white become `cbg`; a column's alignment is taken from its first aligned cell; widths only where fixed | `Im:767-860`, `Im:538-570`, `Im:181`, `Im:213` |
| A spreadsheet's HTML clipboard with floating pictures | The pictures are placed into the cells they sit over before parsing | `Im:285-345` |
| Markdown | Pipe tables with their alignment row | `BM:5640-5710` |
| Size cap | 100 000 cells per import: over it, the user is asked, and the import is cut at a row boundary | `Imh:56-60`, `Im:415-464` |
| At most 63 columns | Further columns are dropped | `BM:1582`, `Im:769` |

No importer ever produces a layout row (`BMh:598-599`).

---

## 8. Ruled out: the board view and the separate table view

Both are dropped by canon row 273. They are described only for the stored data they leaned on.

### 8.1 The board (kanban) view

A second presentation of one table, shown only in the table's full-window view (`Kb:2-11`). Grouped by one choice or
check column: one lane per option in option order plus a trailing "No status" lane, or three fixed lanes for a check
column. Cards = the body rows in row order; a card's title = its first text column's text; it could show the cell's
first image and the row's background colour (`Kb:29-35`, `Kb:67-90`). Dragging a card to another lane set that cell's
value and, between two cards, also moved the row — one undo step (`Kb:187-213`). "Add card" inserted a row after the
lane's last card with the lane's value (`Kb:222-231`). A lane's menu moved the option left / right (reordering the
column's option list) and opened the option editor (`Ed:6965-6997`).

Stored data it relied on — all of it ordinary table data that must still round-trip: the column's kind and option
list **in their order**, each row's value in that column, the **row order**, the first text column, cell images, row
colours. Nothing board-specific is in the file: which table shows as a board, and by which column, lived in the
application's own settings keyed by the head record's id (`Ed:658-670`).

### 8.2 The table's own full-window view (the "sub-item editor" for tables)

Every table could be opened on its own from the path bar: the same page view restricted to the table's rows, with a
strip above it holding a Table / Board toggle and the row filter of §2.6 (`Ed:346-382`, `Ed:6999-7078`; `Man:85-93`).
It edited the same blocks as the page — there was no second copy of the data. Its extras were view state only: the
filter text and the hidden rows (memory only, cleared on leaving), the scroll position. PDFs and videos had sibling
full-window views (the media and video specs).

Stored data it relied on: none beyond the table itself. What does **not** survive if nothing replaces it: the row
filter, which existed nowhere else (§10 Q13).

---

## 9. Tests that pin this behaviour (`T`, section titles)

| # | Line | Pins |
|---|---|---|
| 74–75 | `T:4388`, `T:4484` | Split rows load, repair, survive undo and save; split / insert / remove / move keep a valid structure |
| 76–77 | `T:4592`, `T:4648` | Lane x and width, hit-testing by x; caret reading order, up / down at goal x, Tab |
| 78, 80 | `T:9092`, `T:4694` | Collapsing an empty lane, clearing lanes, deleting row ranges; moves within the container |
| 81–82 | `T:4759`, `T:4855` | Ratios with a minimum width, aligned dividers, wrap a run, align and merge rows; drops into lanes and on side edges |
| 83–84 | `T:4931`, `T:5103` | Copy by grain, paste routing; lanes in each export format |
| 85–86 | `T:7173`, `T:7291` | The header role, grouping, ragged rows, break and rejoin; px columns, auto and manual widths |
| 87–88 | `T:7336`, `T:7463` | Rows / columns inserted, deleted, moved, duplicated; clear, fill, refill; colours, alignment, bulk ops, sort, the label join |
| 89, 108 | `T:7641`, `T:8933` | Choice and check cells as chips, the harvest, option sweeps; timecode normalisation and sort |
| 91–93 | `T:7809`, `T:7843`, `T:7903` | Tab walks cells and asks for a row; an empty last row exits; rectangles clear; TSV fills; tabular text makes a table |
| 94–97 | `T:7921`–`T:8020` | A lane's double Enter; pasted HTML tables; a header row carries its table; files dropped on a cell |
| 98–106 | `T:8090`–`T:8823` | Imports, exports per format, copy by grain, the paste rules, foreign tables, the cell cap |
| 107, 109 | `T:8897`, `T:8973` | A board card move is one undo step; the row filter leaves the document untouched |

---

## 10. Open questions / things that looked accidental

For the user to rule on. "Accidental" = behaviour that follows from the old storage rather than from a decision found
in the code or the notes.

**What a cell is**

| # | Question | Evidence |
|---|---|---|
| Q1 | **One media item or several blocks per cell?** Canon row 273 says a cell holds "text, a typed value or ONE media item". The old app, old files, `tree::may_hold` and `legacy` all allow any number of leaf blocks (several paragraphs, a list, an image with text under it, code). If cells are narrowed, converted files need a rule for cells that hold more | §3.1; `tree:64`; `legacy` test B (a paragraph + a picture in one cell) |
| Q2 | Should a cell take headings, code blocks and dividers at all? The old app allowed every leaf kind by construction, not by a rule found | §3.1 |
| Q3 | Where does a typed value live: as today (a paragraph whose text is a chip / a timecode) or as a value on the cell? As today, a typed cell is "one paragraph that refuses text", enforced in four separate places | §1.1, §3.1 |
| Q4 | Each choice cell stores a full copy of its column's option list, and every option edit rewrites every cell of the column. The old plan kept this on purpose (rows stayed meaningful when a table was broken apart by adjacency). With tables as objects that reason is gone; a cell could store only the option's id | §2.2; `S1` §1.6 |
| Q5 | The check is three-state and state 0 is stored as "empty", so "to do" and "no value" are the same thing; a choice column's "no value" is likewise an empty cell. Keep three states? Keep empty = to do? | §2.3, §1.1 |

**Column kinds**

| # | Question | Evidence |
|---|---|---|
| Q6 | Changing a text column to a typed kind silently reduces every cell to one line of plain text (formatting, extra blocks and images in those cells are lost; text that matches no check state is lost entirely), with one undo as the only guard and no red menu item. Warn, refuse when cells hold more than text, or keep? | §2.5 |
| Q6b | Timecode: 29.97 and 59.94 are always drop-frame; unreadable text is kept silently with no mark; a bare number means frames; partial entries (`1:02`) are unreadable; the rate menu is six fixed values. Which of these are wanted? | §2.4 |
| Q6c | Pasting text into a choice column adds unknown values as new grey options. Wanted, or should an unknown value be refused / left empty? | §3.6 |
| Q6d | No number or date kind exists; text columns sort "numeric when both parse". Is a number kind wanted now that columns are typed objects? | §2, §2.6 |

**Structure**

| # | Question | Evidence |
|---|---|---|
| Q7 | The header role did four jobs: make a table from side-by-side rows, dissolve a table into them, split a table in two at a row, and join two adjacent tables by column label. Which survive as commands on a Table block ("Convert to columns", "Split table here", "Join with the table above")? | §4.3 |
| Q8 | A table must have ≥ 1 header row (it was what made it a table). `Kind::Table.header_rows` can be 0. Is a headerless table allowed, and is the header count still "the first n rows" (multi-row headers) or a single on/off? | §4.3; `tree:23` |
| Q9 | A table may now sit in a column (row 273); the old app never allowed it, so nothing says how a table wider than its column behaves (the old overflow rule shifts and scrolls the *page*), nor where its grips and "+" strips go inside a 24 px gap | §5.3, §6.3 |
| Q10 | An emptied column: the old app removed it and gave its width to the left neighbour, and unwrapped a one-column row; row 305 says the new side keeps an empty paragraph for now. Which is it for Columns — and is "Backspace in an empty column removes the column" kept as the way to unsplit? | §5.5 |
| Q11 | A table row dragged out of its table became a layout row (then possibly plain blocks). In the new tree a `Row` cannot live outside a `Table`. Should rows be draggable only within and between tables? | §4.2 |
| Q12 | A manual column width has no way back to automatic in the interface (the model supports it). Add one (a double-click on the border is the usual gesture)? | §4.4 |
| Q13 | The row filter existed only in the table's full-window view, which is dropped. Is filtering wanted in place, or dropped with it? Sort is a one-shot reorder of the stored rows with no indicator — keep it that way? | §2.6, §8.2 |
| Q14 | Column limits: 63 columns per table and 63 / 64 lanes come from an 8-bit lane index. Is there a limit worth keeping for Columns (the user's uses suggest 2–4)? | §0, §5.2 |

**Keys, selection, gestures**

| # | Question | Evidence |
|---|---|---|
| Q15 | Enter moves down the column and appends a row at the bottom; Shift+Enter makes a second paragraph in the cell; Enter inside a list item in a cell makes a new item instead. Keep the three, and keep "Enter in an empty last row leaves the table"? (All three were ruled for the old app after a walk; listed because row 304 gives Enter and Escape new meanings for block selection) | §3.2 |
| Q16 | Escape: the old ladder is caret → row (as an object) → table (as an object) → out below. Row 304's provisional rule is "Escape selects the caret's whole block". Inside a cell, which wins — the block, the cell, the row? | §3.4; canon row 304 |
| Q17 | The same highlighted table can be "cells" (Cmd+A, drag: Delete clears) or "an object" (Esc: Delete removes), with no visible difference found. With tables as blocks, block selection could mean "the object" and cell selection "the cells" | §3.4, §3.7 |
| Q18 | Row and column sets are picked on hover-revealed grips beside and above the table, and the "+" strips show for the caret's table. Canon row 300 forbids hover-only controls. Where do row / column handles show? | §4.1; canon rows 300, 305 |
| Q19 | Deleting rows and columns is menu-only by an old ruling (Backspace on a set only clears). Keep? | §3.7 |
| Q20 | Making columns: Cmd-drag from a block's edge, a menu item, and a side drop. No toolbar button and no key existed; the new toolbar already has a Columns button. What does the button make (two columns from the caret's block, as the menu did?), and are the Cmd-drag pull and the snap fractions kept? | §5.1 |
| Q21 | Aligned dividers (adjacent rows' dividers move together, Option for one) and "Align lanes" / "Merge with the row below" exist because one tall row and many short rows were two ways to lay text side by side. Are they wanted for Columns, given tables now cover aligned rows? | §5.2, §5.6 |
| Q22 | A selection across two columns: the old app clears whole lanes between the two ends; the new editor already deletes only the selected text and keeps the structure (`mn-edit`). Confirm the new behaviour stands, and say what a selection across two **cells** deletes (the old app: the whole rectangle) | §5.4, §3.7 |
| Q23 | Narrow pages: lanes never stack and may fall under their 120 px minimum on a page narrower than the one they were set on. With the new responsive width and cards on a canvas, should Columns stack below some width? | §5.2 |

**Looked accidental**

| # | Item | Ref |
|---|---|---|
| A1 | `tableFillDown` / `tableFillRight` are implemented and tested but nothing in the interface calls them; the per-option rename / recolour / remove calls are likewise unused | `BM:768-802`, `BM:1461-1502` |
| A2 | The automatic width adds 18 px for a block in the table's first row "for the sort glyph"; no glyph exists | `BM:398` |
| A3 | `tableMoveRow` has no header / body guard of its own; only the menu and the drag aim keep body rows out of the header | `BM:671-681` |
| A4 | The bulk column menu offers choice, check and text but not timecode | `Ed:8516-8521` |
| A5 | A divider drag snaps to fractions of the **page** width while a pull snaps to fractions of the **block's** width | `Ed:2167`, `Ed:2193` |
| A6 | Two different option palettes: the harvest uses eight colours, the picker and the editor another set; an option added by paste is always `#8A8A8A` | `BM:1369-1370`, `ChP:64`, `ChE:29`, `BM:1695` |
| A7 | Append by label inserts the pasted rows under the **caret's row**, not at the end of the table, unless the caret is in a header row | `BM:1873` |
| A8 | A TSV fill cuts a multi-block target cell to its first block and overwrites it even where the pasted field is empty | `BM:1678-1681`, `BM:1701` |
| A9 | Up / Down entering a cell from above lands in its first block and from below in its last, while Tab and Enter always land at the end of the last block | §3.2 |
| A10 | "Unassign header" on a table directly under another table silently joins the two instead of dissolving it | `BM:2073-2080` |
| A11 | Markdown export writes only the first header row as the header; further header rows become body rows without a mark | `Ex:665-690` |
| A12 | The check box ignores the column's alignment; the cell's hidden label text still occupies the cell | `L2` §9.4 |
| A13 | `S1` §8 #16–#23 (lane renumbering in table rows, the unclamped header count, table extent by adjacency, the unused table `ratios`) all disappear with the new tree; listed there | `S1` §8 |

---

## 11. Not determined

- The exact timecode strings `TC`'s `format` produces (field widths, the drop-frame separator) — only its header
  comment and `parse` were read.
- Whether Shift+arrows across a cell edge was exercised as a way to make a cell rectangle; it follows from the code
  (§3.4) but no test or manual line names it.
- Whether a block's type buttons (heading, quote, list, code) are refused in a typed cell; the refusals found are for
  typing, chips, drops and drags only.
- The paint of a cell rectangle, a set and the menu's row / column / table wash beyond what `L2` §10.3 records
  (`BlockView.qml` was not re-read for this document).
- The HTML and DOCX markup of a choice chip in a cell (colour, pill) and the DOCX / ODT readers' handling of row and
  column spans beyond the test titles (`DocxReader.cpp`, `OdfReader.cpp`, `XlsxReader.cpp` were not read line by
  line).
- How the sticky header and the frozen first column behave for a table with several header rows or a filtered view
  (`L2` §9.3 has the geometry; the editing behaviour under them was not traced).
- What the rail (the left margin's handles) shows for the rows of a table beyond `BMv` §2.1.
- Undo granularity of typing inside a cell compared with outside (assumed identical: the same text calls).
- The behaviour of Opt+Cmd+Up / Down on a selected table row at the header boundary (`moveTarget` treats a split row
  as one top-level slot; the header guard is in the view's drag aim only, so a key move of a body row above the header
  row may be possible) — not run.
- Whether any path could leave a typed cell holding more than one block (a drop and a paste refuse; `moveBlocks` into
  a typed cell is refused by the view's aim, not by the model).

