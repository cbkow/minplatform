# minNotes block moves and block clipboard — what the Qt app did (Phase 3)

Source read: the minNotes repository at commit `59d1129` (2026-11-10, branch `min-paths`), read only. Nothing in it was
built or run; every statement is read from code or from the app's own manual, and anything deduced rather than read is
marked **(inference)**. References are `alias:line` relative to the minNotes repository.
`BM` = `app/core/BlockModel.cpp`, `BMh` = `app/core/BlockModel.h`, `Ed` = `app/qml/MinNotes/App/Editor.qml`,
`Main` = `app/qml/MinNotes/App/Main.qml`, `CP` = `app/core/ClipboardPaster.cpp`, `CPh` = `app/core/ClipboardPaster.h`,
`Cb` = `app/core/Clipboard.cpp`, `BC` = `app/core/BlockClipboard.cpp`, `BCh` = `app/core/BlockClipboard.h`,
`ATh` = `app/core/AssetTransfer.h`, `DMh` = `app/core/DocumentMerger.h`, `Man` = `docs/editing.md`,
`Sh` = `docs/shortcuts.md`, `T` = `tests/regression_test.cpp`.

Vocabulary of the old code, used below: a **row** is one entry of the flat block list; a **split row** is a columns
block; its **record** is the `Split` row that heads it; a **lane** is one column (`cell` = the lane index, `-1` = top
level); a **table** is a run of split rows whose first record carries a header count (its **head**); an **opaque** row
is a Media or Divider block; a **gap** `g` is the position between rows `g-1` and `g`.

---

## 0. The shape everything below rests on

| Item | Rule | Ref |
|---|---|---|
| Document | One **flat** list of rows. Parallel vectors: row attributes, content, id, rank | `BM:6492-6500` |
| Block types | Paragraph 0, Heading 1, Code 2, Media 3, Quote 4, ListItem 5, Divider 6, (7 retired Table), TaskListItem 8, OrderedListItem 9, Split 10 | `BMh:135-141` |
| List nesting | A `depth` attribute 0–8 on list rows. **There is no parent/child link.** A "child" is only a following list row with a larger depth | `BMh:149`, `BMh:916` |
| Columns | A `Split` record row, then its lanes' blocks, each with `cell` = lane index, depth-first and flat. Ratios live on the record | `BMh:141`, `BMh:929-930` |
| Tables | Split rows too: a head record (`header` > 0, column spec in `table`) plus following records; each cell = a lane holding ordinary blocks | `BMh:931-932`, `BM:2195-2201` |
| No nesting of containers | A split row or a table never sits inside a lane | `BM:6006-6018`, `Man:92-94` |
| Selection | **One** selection: anchor (row, col) and focus (row, col). There is no separate block-selection mode and no set of selected blocks | `Ed:1619-1634` |
| Ids | A per-row string id (`makeUlid`), stored beside a fractional rank | `BM:6149-6150` |

Consequence for every section: "a block with its children" does not exist as a unit in the old app. A move, copy,
duplicate or delete acts on a **contiguous run of rows**, and list children travel only if they are inside the run.

---

## 1. Selection

### 1.1 How whole blocks get selected

| Gesture | Result | Ref |
|---|---|---|
| Press on a block's rail handle (§2.1) | Selects that block: anchor = (row, 0), focus = (row, end of text; 0 on an opaque row). A split row's handle selects its whole split row; a table header row's, the whole table; a body row inside a grip-picked contiguous row set, that set | `Ed:7646-7657`, `Ed:2255-2268` |
| Press on a handle **inside** the current selection | Selection kept (so a drag carries the run) | `Ed:7648` |
| Triple-click in text | Selects the whole block (col 0 → end) | `Ed:6636-6644` |
| Double-click | Selects the word (`\w` run) | `Ed:6813-6831` |
| Shift-click | Extends the focus to the hit point; anchor kept | `Ed:6683` |
| Press-drag in text | Text selection across blocks; auto-scrolls within 44 px of the top/bottom edge | `Ed:6688-6689`, `Ed:6839-6855` |
| Shift + arrows / Home / End | Extend by caret movement (`navUp(shift)` etc.) | `Ed:3498-3499` |
| ⌘A at top level | Whole document | `Ed:2572`, `Ed:2601-2610` |
| ⌘A in a lane or cell (repeat) | Ladder: the block → its lane/cell → the split row (in a table: the whole table) → the document | `Ed:2569-2600`, `Sh:104` |
| Esc with a caret in a lane | Selects its whole split row (a table row picked this way becomes an "object": Delete removes the row) | `Ed:3323-3331` |
| Esc on a selected table row | Selects the whole table (object: Delete removes the table) | `Ed:3298-3309`, `Ed:2417-2426` |
| Marquee over blocks | **Not found** in the page editor. A marquee exists only on the canvas (elements, not blocks) | `Sh:53` |
| Keyboard-only "select this block" at top level | **Not found** (no Esc rung, no shortcut; ⌘A goes straight to the document) | `Ed:3320-3337` |

### 1.2 Escape ladder (page editor, in order)

| Rung | Action | Ref |
|---|---|---|
| 1 | Close an open search bar | `Ed:3280` |
| 2 | Cancel an in-flight gesture: divider drag → lane pull → **block drag (no move)** → text drag → column drag | `Ed:3292-3296` |
| 3 | Drop a grip-picked table set; leave board view | `Ed:3297-3298` |
| 4 | Selected table row → whole table | `Ed:3299-3309` |
| 5 | Selected split row or table → caret to the block below (a paragraph is made if none) | `Ed:3310-3319` |
| 6 | Any other selection → collapse to the focus | `Ed:3320` |
| 7 | Armed typing marks/colours → cleared | `Ed:3321` |
| 8 | Caret in a lane → select its split row | `Ed:3322-3331` |
| 9 | Nothing left → leave the page for the canvas | `Ed:3332-3336` |

### 1.3 Block selection vs text selection

| Item | Rule | Ref |
|---|---|---|
| Distinction | None in state. A "block selection" is a text range whose ends happen to be col 0 and end-of-text | `Ed:7652-7654` |
| Opaque ends | Media/Divider are whole-in or whole-out: an opaque low end is treated as col 0, an opaque high end as its full length | `Ed:1703-1708`, `BM:6284-6288` |
| Row-range widening | When the two ends sit in different top-level rows, an end inside a split row widens to the whole split row and an end inside a table to the whole table. Every reader of the selection (paint, copy, delete, menu, drag) sees the widened range | `Ed:2086-2100`, `Ed:1622-1630` |
| Finer grains (not widened) | Both ends in one block, one cell, one split row, or one table | `Ed:2082-2085` |
| What a multi-block selection contains | The contiguous rows `[loRow, hiRow]`, the first and last possibly partial (text rows only) | `BM:6536-6570` |
| "Run" in the block menu | The menu treats the selection as a run of blocks only when it spans ≥ 2 rows **and** the right-click landed inside it (`menuInSel`); it then acts on whole rows `loRow..hiRow` regardless of the columns | `Ed:8179-8182` |
| Table cell rectangles / grip sets | Separate state (`cellRect`, `tableSet`), with their own copy and delete paths; out of scope here except §5 | `Ed:2914-2936` |

---

## 2. Drag and drop of blocks

### 2.1 The handle

| Item | Rule | Ref |
|---|---|---|
| Where | A 34 px rail at the viewport's **right** edge, inside the scrollbar. One 6 px dot per top-level entry, at the block's y | `Ed:7584-7595`, `Ed:7628-7633` |
| Which rows have one | Top-level rows only: a lane block or a table cell block has **no** handle. A split row / table row has one (on its record) | `Ed:7617-7618` |
| When it shows | Always (not on hover, not on focus). The focused block's dot is brighter. The whole rail fades out and stops taking the mouse when a narrow window would put it over the page | `Ed:7599-7602`, `Ed:7631-7632` |
| Hover | Deliberately none (`hoverEnabled: false`): hundreds of hover areas made every mouse event a hit-test. Cursor is an open hand over the rail | `Ed:7638-7642` |
| History | An earlier left-gutter grip was retired; `hoverRow` ("row whose grip is lit") survives but only feeds the lane gestures | `Ed:6591-6592`, `Ed:726` |
| Table rows | Also draggable from a row grip in the table's left margin (4 px threshold; a click picks instead) | `Ed:6715-6725` |

### 2.2 What a drag carries

| Start | Run carried | Ref |
|---|---|---|
| Handle of a block outside the selection | That one row | `Ed:7667-7673` |
| Handle of a block inside the selection | The whole selected run `loRow..hiRow` | `Ed:7664-7666` |
| Handle of a split row | The whole split row (record + all lane blocks) | `Ed:2267` |
| Handle of a table's header row | The whole table | `Ed:2259` |
| Handle of a table body row inside a contiguous grip-picked body set | That set of rows | `Ed:2260-2265` |
| List item | **Only the row(s) grabbed.** Deeper items below it are not gathered | `Ed:7671` (inference from the absence of any depth walk) |
| Threshold | 4 px vertical before a press becomes a drag | `Ed:7662` |

### 2.3 Feedback while dragging

| Item | Rule | Ref |
|---|---|---|
| Drag body | A 22 px chip riding the rail under the pointer, labelled `block` or `N blocks` (untranslated literals). No ghost of the content — removed by a dated user ruling | `Ed:7685-7700`, `Ed:7757-7759` |
| Source | The dragged rows' rail dots dim to 0.3; the content itself is unchanged | `Ed:7623-7624` |
| Drop line, top level | A 2 px accent line across the whole sheet (page + gutters) at the gap | `Ed:7732-7743` |
| Drop line, in a lane | The same line, only as wide as the lane | `Ed:903-910` |
| Side drop | A 3 px accent bar on the left or right edge of the target block, full block height | `Ed:7745-7755` |
| No-op gaps | No line is drawn when the gap is inside or adjacent to the dragged run and the lane is unchanged | `Ed:920-923`, `Ed:7733` |
| Auto-scroll | 16 ms ticker; within 44 px of the viewport's top/bottom; re-aims the drop each tick | `Ed:6839-6851` |
| Cancel | Esc or a cancelled grab: no move | `Ed:3294`, `Ed:6810` |
| Model during drag | Untouched; the move happens on release as one undo step | `Ed:924-940` |

### 2.4 Drop targets (`laneDropAim`, `aimBlockDrag`)

| Pointer | Target | Ref |
|---|---|---|
| Vertical position vs a row's midpoint | Gap before / after that row. Below a split row means after its last block, never inside it | `Ed:836-844` |
| Off the page horizontally | Top-level gap | `Ed:873-876` |
| Over a top-level block, away from its side bands | Top-level gap | `Ed:885-886` |
| Within the side band of a block (band = min(24 px, width/6)) | **Beside**: a new lane on that side. On a top-level block this creates a split row; on a lane block it adds a lane | `Ed:882-885`, `BM:3287-3310` |
| Over a lane block, away from its side bands | A gap **inside that lane**, before/after the block by its midpoint | `Ed:887-889` |
| Over a table cell (untyped column, or any header cell) | A gap inside that cell — ordinary blocks can be dragged into table cells | `Ed:877-889` (inference: a cell is a lane, and only typed cells are excluded) |
| Indent levels | **None.** A drop never changes list depth; there is no horizontal "drop as child" zone | `BM:6511-6516` (depth is copied unchanged) |
| Other notes / pages / a sidebar | **None.** Block drags never leave the document | — (no such drop target found) |

### 2.5 Rules that refuse or adjust a drop

| # | Rule | Where enforced | Ref |
|---|---|---|---|
| R1 | The dragged run cannot be its own target (hit inside the run → top-level gap) | view | `Ed:877` |
| R2 | A run carrying a split row (or table rows) only ever aims at top-level gaps — never a lane, never beside | view + model | `Ed:893-899`, `BM:6470-6471` |
| R3 | A run carrying a split row must be made of **whole** split rows (it may not start or end inside one) | model, silent no-op | `BM:6472-6473` |
| R4 | A top-level destination must be a real top-level gap: the row that will sit below the run may not be a lane block | model, silent no-op | `BM:6476-6479` |
| R5 | A lane destination must touch a block of that lane (above or below the gap) | model, silent no-op | `BM:6480-6484` |
| R6 | A whole table, or anything that is not a split row, never lands **between a table's rows**: the gap snaps to the nearer edge of the table | view | `Ed:857-865`, `Ed:871` |
| R7 | A table row never lands among a table's header rows: it lands under them (or after the table when it has no body) | view | `Ed:866-867` |
| R8 | A typed table body cell (choice / check column) takes nothing | view | `Ed:878-880` |
| R9 | No side drops on table cells or on a split row's record | view + model | `Ed:882`, `BM:3293-3294` |
| R10 | `moveBeside` refuses a run containing a Split record, and a target inside the run | model, returns -1 | `BM:3290-3292` |
| R11 | Dropping in place is a no-op unless the lane changes | view + model | `Ed:920-923`, `BM:6462-6463` |
| R12 | Code blocks: no special rule. A code block is one row; nothing can be dropped "into" it, and it moves like any block | — |
| R13 | Dropping into itself/its descendant: cannot arise — there are no descendants; R1 and R11 cover the degenerate cases | — |
| R14 | Lane count is capped at 64 by the structure pass | model | `BM:2210-2212` |

Refusals in the model are **silent**: `moveBlocks` returns `void` and does nothing (`BM:6462-6485`). The view computes
the drop line independently, so a line can be shown for a drop the model then refuses **(inference;** e.g. R4 when
`gapForY` yields a gap the model considers inside a split row).

### 2.6 What happens to what is left behind (`normalizeStructure`, `BM:2284-2307`; plan at `BM:2186-2227`)

| Situation after a move/delete | Repair | Ref |
|---|---|---|
| A lane has no blocks left | The lane disappears; lanes renumber in order of appearance; its width goes to the nearest surviving lane on its left (else the first) | `BM:2142-2180`, `BM:2207-2210` |
| A layout split row is left with one lane | The record is removed and its blocks become top-level ("unwraps") | `BM:2216-2218`, `Man:89-90` |
| A split row is left with no blocks | The record is removed | `BM:2216` |
| A **table** cell is left empty | It is refilled with an empty paragraph — a table cell never empties, and a table row may have a single lane | `BM:2287`, `BM:555`, `BM:2214-2216` |
| Ratios unmappable (a revisited lane, a bad sum) | Equal ratios | `BM:2152-2163` |
| Minimum lane width | 120 px at the page measure — enforced when dividers are dragged or lanes pulled, not by a drop (a side drop always splits 50/50) | `BMh:385`, `BM:3299`, `Ed:2190-2193` |
| Document left empty | One fresh paragraph is inserted in the same transaction | `BM:6375-6377` |

### 2.7 Files dragged in from outside (same aiming code)

| Item | Rule | Ref |
|---|---|---|
| Accepted | `text/uri-list` only | `Ed:1539-1541` |
| Targets | A table cell (highlighted; typed body cells refuse) → media blocks in the cell; else the same gap / lane gap / side-edge targets as a block drag | `Ed:1526-1537`, `Ed:1563-1571` |
| Several files on a side edge | The first makes the lane; the rest follow it down that lane | `Ed:1576-1586` |
| Indicator | A pulsing 3 px line with a dot and an expanding ring (different from the block-drag line) | `Ed:7763-7806` |
| Invalid lane | `insertMediaAt` sets the lane, checks `structureValid()`, and reverts to where the insert put it if invalid | `BM:3317-3329` |

---

## 3. Keyboard moves

| Keys | Action | Ref |
|---|---|---|
| ⌥⌘↑ / ⌥⌘↓ | Move the focused block, or the selected run `loRow..hiRow`, one slot. Document view only (not board view, not a frame tab) | `Ed:3411-3423` |
| …inside one lane | Moves within that lane; stops at the lane's top/bottom (does not leave the lane) | `BM:3263-3270` |
| …at top level | Steps over a **whole** split row above/below as one slot | `BM:3277-3285` |
| …selection ending inside a split row | Moves only if it covers the split row exactly from its first leaf to its last block; a partial selection across lanes does not move | `BM:3272-3274`, `T:4741-4743` |
| …at the document edge | Nothing | `BM:3278`, `BM:3283` |
| Tab / ⇧Tab at top level | Indent / outdent every **list** row in the selection (or the focused row) by one, clamped 0–8. Non-list rows are skipped. Tab never types a tab | `Ed:2484-2502`, `BM:3450-3473` |
| Tab / ⇧Tab in a lane or cell | Navigate to the next / previous lane instead; Tab past a table's last cell appends a row | `Ed:2485-2498` |
| ⌘] / ⌘[ | Indent / outdent everywhere (the lane-safe twin of Tab) | `Ed:3401-3407`, `Sh:78` |
| Enter on an empty list item | Outdent one level if nested, else become a paragraph | `Ed:1819-1829` |
| Key auto-repeat | Never makes a structural change (delete of a block, lane collapse) | `Ed:1721-1723` |
| ⌘D or another duplicate shortcut | **Not found** — duplicate is menu-only | — |

Children on indent: `indentBlocks` shifts exactly the rows in the range. Indenting one item does **not** shift the
deeper items after it, so they can end up at the same depth or shallower relative to it (`BM:3462-3468`). Children on a
keyboard move: only rows inside the selection move (`BM:3254-3285`); a parent moved alone leaves its deeper items
behind, which then attach visually to whatever list item precedes them **(inference)**.

---

## 4. Model operations (C++)

### 4.1 `moveBlocks(from, count, to, targetLane)` — `BM:6459-6531`

| Item | Rule | Ref |
|---|---|---|
| Addressing | `to` is the run's first index in the **reduced** list (after removal), `0..n-count` | `BMh:486-489` |
| `targetLane` | `-2` (default) = join the lane above the gap, or top level; `-1` = top level; `k` = lane k | `BMh:490-494` |
| Lane of the gap (`laneAt`) | After a lane block → its lane; right under a record → lane 0; else top level | `BM:2265-2271` |
| Mechanics | Rows, ids, content and measured heights are lifted out and re-inserted; **ids are preserved** | `BM:6492-6503`, `BM:6517-6519` |
| Ranks | Fresh fractional ranks chained between the destination neighbours, for every moved row; nothing else is re-ranked | `BM:6505-6510` |
| Attributes | Type, level, depth, task state, spans travel unchanged. Only `cell` is rewritten (unless the run carries a split row, which keeps its own structure) | `BM:6511-6516` |
| After | `normalizeStructure` over the touched band (§2.6); media re-derives its size at the new width | `BM:6524-6525` |
| `moveBlock(from, to)` | The count-1 case | `BM:6449` |
| `rowAfterMove(r, from, count, to)` | Static: where any row index ends up; the view remaps caret and anchor through it so the selection follows the moved blocks | `BM:6451-6457`, `Ed:958-962` |
| `moveBeside(from, count, target, side)` | `splitIntoColumns(target, side, 0.5)` makes a lane holding a placeholder paragraph; the run moves in above it; the placeholder is removed. One undo step. Tracks rows by id across the steps | `BM:3295-3310` |
| `moveTarget(lo, hi, dir)` | Pure query for the keyboard move: `[from, count, to, lane]` or `[]` | `BM:3254-3285` |
| Table row / column moves | Separate functions (`tableMoveRow`, `tableMoveColumn`), used by grips and the table menu | `BM:671`, `BM:716` |

### 4.2 Undo

| Item | Rule | Ref |
|---|---|---|
| Unit | Region-snapshot transaction: `beginTxn(lo, hi)` snapshots a band of rows; `endTxn` snapshots the same band (adjusted by the row-count delta) and stores before/after | `BM:3786-3801` |
| Band for a move | Every row between source and destination inclusive, widened to whole split rows | `BM:6489-6490` |
| Compression | Same ids in the same order → sparse per-row patches. Same ids, only order and rank changed → two `(id, rank)` lists (a permutation). Otherwise both full snapshots | `BM:3858-3895` |
| A move that changes a lane | Not a pure permutation (`cell` differs) → full band snapshots | `BM:3871-3876` |
| Snapshot content | id, rank, content, type, level, task state, depth, lang, spans, cell, ratios, header, table attrs | `BMh:936-943` |
| Grouping | `beginGroup/endGroup` nest; only the outermost commits. The view groups a move with the focused row's pending inline-markdown commit so the gesture is one step | `BM:3912-3913`, `Ed:950-956` |
| Caret | Each entry stores caret + anchor before and after; undo/redo emit a restore request. The view must close the group **before** writing the new caret | `BM:3963`, `Ed:956-960` |
| No-op | A transaction whose before and after are equal pushes nothing | `BM:3820-3826` |
| History | A tree with parent links; redo follows the newest child | `BMh:835-836` |

### 4.3 Numbered lists

Numbers are **computed at render time**, never stored: an ordered item's number = 1 + the count of ordered items at
the same depth directly above it, skipping deeper list rows; any other row ends the run (`BM:3476-3489`). A move,
paste or delete therefore needs no renumbering, and a list split by a moved-in paragraph restarts at 1.

### 4.4 Other operations

| Function | Rule | Ref |
|---|---|---|
| `duplicateBlock(row)` | Copy directly below, **new id**, rank between the row and its successor, same lane. Refuses a Split record | `BM:6319-6346` |
| `duplicateBlocks(lo, hi)` | The run is widened by the copy grain (§5.2), turned into specs, and spliced right after itself (below the split row if the copy needs the top level). New ids. One undo step | `BM:6862-6881` |
| `removeBlocks(lo, hi)` | A record in the range takes its whole split row; then §2.6 repairs. One undo step | `BM:6348-6381` |
| `deleteSelectionRange` | Text ends merge (head of the low row + tail of the high row); an opaque low end removes whole blocks; ends in different cells of one table clear the cell rectangle (structure stays); ends in different lanes of one split row clear those lanes; a range reaching into a table takes the whole table | `BM:6383-6433` |
| `wrapRun(lo, hi, side, ratio)` | A top-level run becomes one lane of a new split row beside an empty paragraph. Refuses if any row is in a lane or is a record | `BM:2995-3003` |
| `splitIntoColumns(row, …)` | Refuses records and anything inside a table | `BM:2309-2313` |

---

## 5. Clipboard

### 5.1 What a copy writes (`Clipboard::writeBlocks`, `Cb:67-80`)

| Flavour | Written when | Content | Ref |
|---|---|---|---|
| `application/x-mnd-blocks` | Every document copy | JSON `{format: "mnd-blocks", version: 1, minReader: 1, docPath, docDir, package, blocks[], assets[], grid?}` | `BCh:26-28`, `BC:108-118` |
| `text/plain` | Always | Text rows sliced to the selection and joined with `\n`; Divider → `---`; Media and Split records omitted; no markdown prefixes, no list markers | `BM:6573-6596` |
| `text/plain`, structured range | The range holds a Split record (whole layout rows or tables) | Markdown from the exporter instead (tables as GFM) | `Ed:2961-2967` |
| `text/html` | **Only** for a structured range (the exporter's page fragment) or a table cell copy (`<table>`) | — | `Ed:2966`, `Ed:2932-2933` |
| Image | The range is exactly one image block | The file loaded as a raster | `Ed:2968-2970`, `Cb:73-78` |
| Markdown | Never as its own MIME type. ⇧⌘C "Copy as Markdown" writes markdown as plain text only (whole blocks; no selection = the whole document) | `Ed:2999-3011` |

A plain text selection (bold, links, several paragraphs, lists) therefore reaches other applications as **bare text
with no HTML** — formatting survives only in the internal flavour.

### 5.2 What the internal payload contains

| Item | Rule | Ref |
|---|---|---|
| Unit | A list of `BlockSpec`: type, level, task state, depth, lang, text, spans (all payloads), media descriptor, cell, ratios, header, table attrs. **No ids, no ranks** | `BMh:555-568` |
| End rows | Text rows are sliced to the selection; spans clipped and rebased. A choice chip cut by the slice is dropped (only whole chips travel). Opaque rows are whole | `BM:6549-6566` |
| Copy grain (`copyBand`) | Both ends in the same lane of one split row → those rows, lane info stripped. Otherwise the range widens to whole split rows; if it then holds a record, lane structure is kept and lane blocks copy whole | `BM:6054-6066`, `BM:6546` |
| Assets | A copy-time snapshot of where each sidecar media file lives (absolute path or package entry, size, video flag), so a paste works after the source closes | `BM:6620-6638`, `BCh:30-36` |
| Table cell fragment | A cell rectangle / grip set copies records + cells plus `grid: {cols, header}`; plain text is quoted TSV; HTML is one `<table>` | `BM:6681-6737`, `Ed:2914-2936` |
| Version gate | A decoder refuses a foreign `format`, or a `minReader` newer than itself; the paste then falls back to the other flavours | `BC:143-148`, `BCh:9-13` |
| No selection | ⌘C copies the whole focused block | `Ed:2943-2945` |

### 5.3 Cut

| Case | Rule | Ref |
|---|---|---|
| With a selection | Copy, then `deleteSelection` (two calls; the delete is its own undo step) | `Ed:3107-3110` |
| No selection, caret on Media/Divider | Copy, then delete that block | `Ed:3110` |
| No selection, caret in text | Copies the block; deletes **nothing** | `Ed:3107-3111` |
| Menu Cut on a block outside the selection | Copy the whole block, delete it | `Ed:3122-3125` |

### 5.4 Paste routing (`ClipboardPaster::route`, `CP:33-49`) — first match wins

| # | Condition | Action | Ref |
|---|---|---|---|
| 1 | Internal payload present, caret **not** in a code block | `blocks` | `CP:37` |
| 2 | Caret in a code block (and no multi-row selection), text present | `codeVerbatim`: the plain text is inserted as-is — newlines normalised, no HTML, no markdown, no table detection | `CP:39`, `Ed:3042-3050`, `Ed:3028` |
| 3 | HTML present, unless it is a lone remote `<img>` with a raster beside it | `html` | `CP:41` |
| 4 | Local file URLs | `urls`: each becomes a media block after the caret's block, chained | `CP:42`, `Ed:3061-3071` |
| 5 | Raster | `raster`: a media block after the caret's block | `CP:43`, `Ed:3072-3078` |
| 6 | Caret in a table cell, text with a tab or newline | `tableTsv`: fill cells from the anchor, growing the table | `CP:45`, `Ed:3079-3085` |
| 7 | Text that looks tabular (≥ 2 lines, every line the same tab count ≥ 1, no blank line) | `tableFromTsv`: a new table below the caret's block | `CP:46`, `CP:16-31` |
| 8 | Otherwise | `text` (§5.6) | `CP:47` |

A flavour that yields nothing is masked and the router is asked again (up to 8 rounds) (`Ed:3030`, `Ed:3058`).
File URLs **and** a raster on one clipboard are read as a screen-capture tool's temp file: the file is force-copied
into the sidecar (`Ed:3015-3019`). A web URL in plain text gets no special handling in the page editor — it pastes as
text **(inference;** no URL branch exists in `route` or `pasteText`; link cards exist only on the canvas, `Sh:54`).

### 5.5 Pasting internal blocks (`pasteSpecsAt`, `BM:6739-6856`; `applyPaste`, `CP:93-153`)

| Item | Rule | Ref |
|---|---|---|
| Ids | Always freshly minted; nothing in the payload could carry them | `BM:6149`, `BMh:555-568` |
| Selection | Deleted first, inside the same undo entry as the paste | `CP:118-124` |
| Text caret + text-ish first block | The caret row is split at the caret. The first pasted block **merges into** the left part; the remaining blocks are inserted after; the right part is appended to the last pasted block when that is text-ish, else becomes a new paragraph after it | `BM:6784-6851` |
| Type adoption | The caret row takes the first block's type/level/depth only at a clean start (nothing left of the caret), and a pasted plain paragraph only replaces an **empty paragraph** — pasting a paragraph at the start of a heading does not demote it | `BM:6802-6811` |
| Single block | Inline insert at the caret; the row's tail stays in place | `BM:6817-6822` |
| Caret on an opaque row, or first block opaque | Everything lands **after** the caret row; an empty simple row (paragraph, heading, quote, bullet) is folded into instead | `BM:6769-6782` |
| Chips | A caret inside a choice chip is pushed to the chip's end before the split | `BM:6787-6789` |
| Caret after paste | End of the last pasted text (before the carried tail); col 0 if the last block is opaque | `BM:6816`, `BM:6837-6842` |
| Caret in a lane, payload has no split row/table | Blocks join that lane | `BM:6767`, `BM:6148` |
| Caret in a lane, payload has a split row or table | The **whole paste** lands below the caret's split row; a toast says so | `BM:6755-6764`, `Ed:1605-1606` |
| Caret in a table cell, payload is a grid (cell fragment or copied table rows) | Fills cells by position or appends rows by header label; "Paste into cells" forces fill-by-position | `CP:128-141`, `Ed:2949-2953` |
| Cell fragment pasted outside a table | Promoted to a table of its own | `CP:142-144` |
| Split rows in the payload | Sanitised by the same structure plan; a record the plan would drop becomes an empty paragraph | `BM:6033-6052` |
| Media across documents | Sidecar files are copied into the destination's sidecar on a worker thread (progress, cancel, rollback); same sidecar → no copy; a same-name same-size file is reused; absolute / network / http sources pass through | `CP:82-90`, `CP:193-226`, `ATh:9-14` |
| Async safety | The target row is re-found by id after the copy; if it moved, the selection delete is skipped | `CP:99-108` |

### 5.6 Pasting plain text (`pasteText`, `BM:5674-5912`)

| Item | Rule | Ref |
|---|---|---|
| Lines | `\r\n` and `\r` → `\n`. Each **non-blank line** becomes one block; blank lines are dropped (a single newline already separates blocks) | `BM:5679-5681`, `BM:5764` |
| Markdown | Always interpreted: block prefixes (`#`, `>`, `- `, `- [ ]`, `1. `, `---`) and inline marks (`**`, `*`, `` ` ``); a ``` fence becomes one Code block, body verbatim, tag = language, unclosed fence runs to the end | `BM:5751-5762`, `BM:5781-5791` |
| GFM pipe table | Becomes a table with header and alignment; text before and after is pasted around it | `BM:5683-5739` |
| First line | Merged into the caret block at the caret; adopts the parsed type only at a clean start **and** when the line had a real prefix | `BM:5878-5887` |
| Leading fence into non-empty text | Lands after the row instead of merging | `BM:5870-5875` |
| Opaque caret row | Everything lands after it | `BM:5796-5798`, `BM:5873-5875` |
| Tail | The text right of the caret rides the last pasted block | `BM:5842-5847` |
| List depth | Every pasted block is created at depth 0; indentation in the source text is not read | `BM:5848`, `BM:5866` |
| "Paste as plain text" | **Not found** — there is no way to bypass markdown interpretation outside a code block | — |

### 5.7 Pasting HTML (`pasteHtml`, `BM:5914-5966`)

| Item | Rule | Ref |
|---|---|---|
| Parser | `QTextDocument::setHtml`, walked by the importer into specs: headings, lists, paragraphs, bold/italic/underline/strike/links, tables | `BM:5917-5925`, `BMh:537-542` |
| One plain paragraph | Spliced inline at the caret with its formatting | `BM:5943-5961` |
| Anything else | Inserted **after** the caret row (an empty simple row is reused). The caret row is **not** split — unlike §5.5 and §5.6 | `BM:5963`, `BM:5995-6000` |
| Table-only HTML with the caret in a cell | Fills by position, or appends by label when header rows were detected | `BM:5929-5935` |
| Images | Remote `<img>` become media blocks pointing at the URL, fetched into the sidecar in the background; relative srcs do not resolve | `BM:5921-5922`, `BM:5964` |

---

## 6. Between notes / documents

| Mechanism | Exists? | Rule | Ref |
|---|---|---|---|
| Drag blocks to another note or a sidebar | No | No drop target found | — |
| "Move to…" a note | No | Not found in any menu | — |
| Copy / cut + paste across open documents | Yes | The internal payload, with media copied into the destination's sidecar (§5.5) | `CP:82-90` |
| Merge a whole document into this one | Yes | File ▸ "Merge File Into This Page…" and the block menu's "Merge a file here…" (below the clicked run). **Copies** every block of another open file at a gap; the source is untouched; one undo entry; copied files sit outside undo | `Main:1070-1092`, `Ed:8367`, `DMh:1-13` |
| Merge gap rule | — | A merge never lands inside a split row or between two adjacent split rows: it snaps below the run | `BM:6020-6031` |
| Drag a document tab into the page | Removed | The tab-strip drag was removed (2026-10-18); its aiming code in the editor has no caller left | `Main:1070-1071`, `Ed:1490-1498` |
| Duplicate | Within a document only (§4.4) | — | `Ed:2886-2899` |
| Copy link to a block | No | Not found (`rowForId` exists and is used to open media/table tabs, not for links) | `BMh:744` |

---

## 7. Block context menu (right-click on a block; `Ed:8157-8404`)

The menu opens for the block under the pointer (`Ed:6581-6589`). When the click lands inside a selection spanning
≥ 2 rows it becomes a **run menu** (header "N blocks") acting on `loRow..hiRow` (`Ed:8179-8182`).

| Section | Rows | Ref |
|---|---|---|
| Spelling | Suggestions, Add to dictionary, Ignore, Fix all (single block only) | `Ed:8267-8322` |
| Primary | Open link; Open PDF in tab; Open video in studio | `Ed:8326-8330` |
| Media | Copy image; Refresh preview; Fit width; Original size; Reveal in file manager; Copy path | `Ed:8333-8344` |
| Table | Add / remove a header row; Unassign header; View as board | `Ed:8346-8357` |
| Insert | Add block above / below; Insert choice chip; Insert table below; Merge a file here… | `Ed:8361-8367` |
| Arrange | **Move up / Move down** ("Move blocks up/down" for a run); **Duplicate block(s)**; Split into columns; Align lanes; Merge with the row below | `Ed:8370-8381` |
| Clipboard | **Cut / Copy** ("Cut blocks / Copy blocks" for a run); **Paste**; Paste into cells (in a table) | `Ed:8384-8388` |
| Convert | Make code block / Change language…; Assign as header | `Ed:8391-8395` |
| Delete | Delete lane; Delete table; **Delete block(s)** | `Ed:8398-8403` |

| Detail | Rule | Ref |
|---|---|---|
| Turn into | Only "Make code block". No general "turn into heading / list / quote" in this menu (types change through markdown prefixes and the toolbar) | `Ed:8391-8393` |
| Menu Copy / Cut scope | The selection if the click landed inside it; otherwise the clicked block whole | `Ed:3114-3125` |
| Menu Paste | Text block: the caret goes to the block's end (unless already in it), then normal paste. Media/Divider: an empty paragraph is added below and pasted into. Run menu: replaces the selection | `Ed:2872-2884` |
| Menu Move up/down | `moveRun(row, 1, row ± 1)` with the default lane — it does **not** go through `moveTarget` | `Ed:966-972` |
| Duplicate run | The copy becomes the selection, so a second duplicate / move / delete keeps acting on blocks | `Ed:2889-2899` |
| Delete | Grouped with the focused row's markdown commit as one undo step; the caret lands on the row that takes the slot | `Ed:3174-3207` |
| Menu target tint | Hovering a menu row washes its scope (block / row / column / table), red for destructive rows | `Ed:743-745` |
| Code block chip | A Copy chip in a code block's corner copies its text (plain text only) | `Ed:6622-6624` |

---

## 8. Known bugs, stale spots and awkward corners (from code and comments)

| # | Observation | Ref |
|---|---|---|
| K1 | **List children do not travel.** Depth is a per-row number; drag, keyboard move, duplicate and indent all act on exactly the rows given. Moving or indenting a parent alone re-parents its former children by position | `BM:3462-3468`, `BM:6511-6516` |
| K2 | Menu "Move up / Move down" bypasses `moveTarget`. A top-level block moved up past a split row is inserted **inside** the split row's last lane (it joins the lane above the gap) instead of stepping over it as ⌥⌘↑ does; a run carrying a partial split row is silently refused **(inference from `moveRun` + `laneAt`)** | `Ed:966-972`, `BM:2265-2271`, `BM:6514` |
| K3 | Model refusals are silent (`void`), while the drop line is computed separately in the view; a shown line can lead to no move **(inference)** | `BM:6462-6485`, `Ed:892-901` |
| K4 | Blocks inside lanes and table cells have no rail handle: they can be moved only by ⌥⌘↑/↓ within their lane, by the menu, or by cut/paste — never dragged out | `Ed:7617-7618` |
| K5 | ⌘X with a caret in text and no selection copies the block but removes nothing | `Ed:3107-3111` |
| K6 | Cut is copy + delete as two separate steps; nothing ties the clipboard to the deletion | `Ed:3107-3110` |
| K7 | Ordinary text copies carry no HTML flavour, so formatting is lost in other applications | `Ed:2957-2972` |
| K8 | Plain-text paste always interprets markdown, drops blank lines, and flattens list depth to 0; there is no plain-paste escape | `BM:5764`, `BM:5848` |
| K9 | HTML paste of more than one block never splits the caret row (lands after it), unlike internal and plain-text paste — three paste paths, three landing rules | `BM:5963`, `BM:6784`, `BM:5878` |
| K10 | The payload's per-block `ink` field and the "ink laid per row" comments are stale: nothing fills `Payload::ink` on copy and `applyPaste` never reads it | `BCh:44`, `BM:6609-6616`, `CP:93-153` |
| K11 | Dead code: the tab-merge drag's aiming functions and indicator state remain in the editor with no caller | `Ed:1479-1498` |
| K12 | The drag chip's labels `block` / `N blocks` are untranslated string literals | `Ed:7696` |
| K13 | The file-drop indicator "had no width since forever" (an undefined property), fixed late — the two drop indicators are separate implementations | `Ed:7773-7775` |
| K14 | Order-sensitive undo: the group must close before the caret is written, or the entry records the wrong caret (commented at three call sites) | `Ed:956`, `Ed:3163-3166`, `Ed:3180-3182` |
| K15 | The rail vanishes (and with it every drag handle) when the window is narrow enough for it to overlap the page | `Ed:7594-7602` |
| K16 | A press on a handle replaces the selection even when no drag follows (there is no way to grab a handle without selecting); a run in the menu is whole rows even when the selection's ends are mid-text | `Ed:7646-7657`, `Ed:8179-8182` |
| K17 | A paste holding a table or split row with the caret in a lane relocates the **whole** paste below the split row, explained only by a toast | `BM:6755-6764`, `Ed:1605-1606` |
| K18 | `duplicateBlock` (single) copies the row in place and refuses a record; `duplicateBlocks` (run) goes through specs and the copy grain — two code paths with different edge behaviour | `BM:6319-6346`, `BM:6862-6881` |

No `TODO` / `FIXME` markers were found in `BlockModel.cpp`, the clipboard sources or `Editor.qml`.

---

## 9. Not read / limits of this spec

- `Importer` (the HTML walker behind `pasteHtml`), `Exporter` (`copyMarkdown`, `htmlFragment`, `tableCellsHtml`) and
  `AssetTransfer.cpp` were read only through their headers and call sites.
- Not specified here: table row/column moves, board card drags, canvas element drags, and selection painting
  (`BlockView.qml` was not read, so how a selected opaque block is drawn is not stated).
- Nothing was run: behaviours marked **(inference)** follow from reading two or more functions together.

## Open questions the code does not answer

1. Was moving a list item **without** its deeper items intended, or simply what a flat model gave? No comment says.
2. Why the handle rail sits on the right, far from the text start, and whether the retired left grip was dropped for
   performance (the hover cost is documented) or for looks.
3. Whether dragging a block out of a lane or a table cell was ever wanted; the code offers no handle and no comment.
4. What the intended outcome is when a top-level gap computed by the view falls where the model refuses (K3) — the
   tests cover the model's refusals, not the view's aim.
5. Whether "Move up / Move down" in the menu entering a split row's lane (K2) is intended or an oversight left from
   before `moveTarget` existed.
6. Why ordinary copies write no HTML when the exporter can already produce a fragment for structured ranges.
7. Whether blank lines in pasted text were meant to be dropped in all cases (an empty paragraph can never be pasted).
8. Whether ⌘X on a collapsed caret was meant to cut the block (it does for media/dividers) or to do nothing.
9. How a block link or cross-note reference would have addressed a block: ids exist and survive moves, but every
   paste, duplicate and merge mints new ones, and nothing exposes an id to the user.
10. Whether moving (not copying) blocks between documents was ever planned; merge and paste both copy.
