# minNotes document core — behavioural spec (Phase 3, step 1)

Source read: the minNotes repository at commit `59d1129` (2026-11-10, branch `min-paths`), read only. Nothing in it was
built or run. The worked rank examples in §2.2 come from a line-for-line transcription of `rankBetween` run outside the
repository; every other statement is read from code. References are `path:line` relative to the minNotes repository.
`BM` = `app/core/BlockModel.cpp`, `BMh` = `app/core/BlockModel.h`, `IT` = `app/core/InlineText.cpp`,
`ITh` = `app/core/InlineText.h`, `Doc` = `app/core/Document.cpp`, `Canvas` = `app/core/CanvasDocument.cpp`,
`T` = `tests/regression_test.cpp`. This document does not repeat `minnotes-survey.md`; where the two differ, §8 says so.

---

## 1. Inline text (`InlineText.{h,cpp}` and its callers)

### 1.1 Model

| Item | Rule | Ref |
|---|---|---|
| Text | One `QString` per block | `ITh:4` |
| Span | `{s, e, kind, href}`, half-open `[s, e)` | `ITh:22` |
| Offset unit | **UTF-16 code units.** Every clamp is against `QString::size()`; nothing checks surrogate pairs, so an offset may fall inside one | `ITh:7`, `IT:121`, `IT:185-190` |
| Container | `std::vector<Span>`, unordered. Order is whatever the operation history left (§1.7) | `ITh:23` |
| Kinds | 1 bold, 2 italic, 3 code, 4 strike, 5 underline, 6 link, 7 color, 8 highlight, 10 choice. 9 is reserved (old "comment"); it has no name and `kindFromString("comment")` is 0 | `ITh:27-34`, `IT:11-37`, `T:3753-3759` |
| Payload kinds | link, color, highlight, choice (`hasPayload`). `href` holds the URL, the colour string, or the chip JSON | `ITh:42-44` |
| Overlap | Different kinds overlap freely. Nothing in the engine forbids two same-kind spans overlapping; the operations below only avoid creating them | — |

### 1.2 Offset bookkeeping

`shiftSpansInsert(at, len)` (`IT:99-104`), per span:

| Position of `at` | Result |
|---|---|
| `at <= s` (before, or exactly at the span's start) | `s += len`, `e += len` — the inserted text is **outside** the span |
| `s < at < e` | `e += len` — the span grows |
| `at >= e` (including exactly at the end) | unchanged — typing at a span's end does **not** extend it |

`shiftSpansDelete(from, to)` (`IT:106-118`), `len = to - from`, no-op when `len <= 0`, per span:

| Case | Result |
|---|---|
| `e <= from` | unchanged |
| `s >= to` | both ends `-= len` |
| otherwise | `s' = min(s, from)`; `e' = (e > to) ? e - len : from`; kept only if `e' > s'` |

So: a span inside the cut is dropped; a cut inside a span shrinks it; a cut over the span's head leaves `[from, e-len)`;
a cut over its tail leaves `[s, from)`. Payload is kept. **No merge afterwards:** two bold spans `[0,3)` and `[5,8)` with
`[3,5)` deleted become `[0,3)`, `[3,6)` and stay two spans.

### 1.3 Text edits

| Function | Rule | Ref |
|---|---|---|
| `insertText(text, spans, at, ins)` | `at` clamped to `[0, len]`; text inserted; `shiftSpansInsert` (skipped when there are no spans); returns the clamped `at`. An empty `ins` changes nothing but still returns | `IT:120-125` |
| `deleteRange(from, to)` | Both clamped to `[0, len]`; returns false when `from >= to` (also for a reversed range — it is not swapped); else removes the text and calls `shiftSpansDelete` | `IT:127-135` |
| `setText(with)` | Replaces the whole text. Every span has both ends clamped to the new length; empty ones drop. Spans are **not** remapped — they keep their old offsets over the new text | `IT:159-169` |
| `replaceRange(s, e, with)` | See the table below | `IT:137-157` |

`replaceRange`: `s` clamped to `[0, len]`, `e` clamped to `[s, len]`. Returns false only when `s == e` and `with` is empty
(replacing text with identical text returns true). With `n = len(with)`, `delta = n - (e - s)`, each span is tested in
this order and takes the first matching row:

| # | Test | Result |
|---|---|---|
| 1 | `sp.e <= s` | unchanged (a span ending exactly at `s` is not extended, also for a pure insertion) |
| 2 | `sp.s >= e` | shifted by `delta` (a span starting exactly at an insertion point moves right) |
| 3 | `sp.s <= s && sp.e >= e` (covers, including an exact match) | `sp.e += delta`; dropped if now empty. A bold or linked word stays bold / linked after a spelling fix |
| 4 | `sp.s < s` (overlaps the start of the range) | clipped: `sp.e = s` |
| 5 | else (starts inside the range) | `sp.s = s + n`, `sp.e += delta`; dropped if empty. A span wholly inside the range, or ending exactly at `e`, is dropped; one running past `e` keeps only the part after the replacement |

This is not delete-then-insert: rule 3 is the difference. (The header comment at `BMh:526-527` still says "delete-then-insert".)

### 1.4 Same-kind interval operations (non-payload design)

| Function | Rule | Ref |
|---|---|---|
| `spansCover(v, start, end, kind)` | True for an empty range. Otherwise sorts the kind's spans by `s` and walks: a gap before the cursor fails; touching spans count as continuous | `IT:39-51` |
| `addSpan(v, start, end, kind)` | No-op for an empty range. Adds `{start, end, kind}` (empty `href`), sorts that kind's spans by `s`, merges every pair with `next.s <= prev.e` (**overlapping or touching**). The vector becomes: all other kinds in their previous order, then the merged kind sorted | `IT:53-68` |
| `removeSpan(v, start, end, kind)` | No-op for an empty range. Each overlapping span of the kind is replaced by its left remainder `[s, start)` and / or right remainder `[end, e)`; remainders keep the `href`. Works for payload kinds too | `IT:88-97` |
| `hasFormat` | false for kind 0 or an empty clamped range; else `spansCover` | `IT:193-196` |
| `setFormat(on)` | Range clamped to the text; false (nothing done) for kind 0 or an empty range; else `addSpan` / `removeSpan`; returns true even when nothing changed | `IT:198-203` |
| `toggleFormat` | Same guards. If the kind covers the **whole** range → `removeSpan` over the range; otherwise `addSpan` (a partly covered range becomes fully covered) | `IT:205-210` |
| `clearFormat` | false if the vector is empty or the range is empty; else `removeSpan` for bold, italic, code, strike, underline, link, color, highlight. **Choice is never removed** | `IT:212-217`, `T:3851-3857` |

`addSpan` does not look at `href`. Called with a payload kind it creates a payload-less span and merges same-kind spans
with different payloads, keeping the left-most one's `href`. `BlockModel::setFormat` / `toggleFormat` pass
`spanKindFromString(kind)` through unchecked (`BM:3937-3942`, `BM:5257-5262`), so the names `link`, `color`, `highlight`,
`choice` are accepted. See §8.

Multi-block formatting (the editor's toggle): one decision for the whole selection — covered everywhere → remove, else
add — applied per row with `setFormat` inside one group (`app/qml/MinNotes/App/Editor.qml:2662-2665`).

### 1.5 Payload spans — two different rules exist

| Path | Used by | Rule | Ref |
|---|---|---|---|
| `mn::inl::applyPayloadRun` | only `applyTypingAttributes` (the armed colour / highlight pens while typing) | No-op for an empty range **or an empty payload**. `removeSpan` of that kind over the range (clipping, remainders keep their payload), add the run, sort the kind by `s`, merge pairs that touch or overlap **and** have a byte-identical `href` | `IT:70-86`, `IT:171-181`, `T:3776-3781` |
| `BlockModel::setPayloadSpan` | `setLink`, `setTextColor`, `setHighlight` (menus, palette, link dialog) | Range clamped; no-op when empty. Every same-kind span that **overlaps** the range is dropped **whole** (not clipped). If the payload is non-empty, `{start, end, kind, payload}` is appended. No merge with neighbours. An empty payload therefore removes every overlapping span of the kind entirely | `BM:4922-4939` |

`payloadCovers(start, end, kind, value)`: clamped; false for an empty range; true when spans of that kind whose `href`
equals `value` exactly cover the range without a gap (`IT:219-234`, `T:3858-3861`).

`applyTypingAttributes(spans, s, e, marks, fg, bg)`: bit 1 bold, 2 italic, 4 code, 8 strike, 16 underline → `addSpan`;
non-empty `fg` / `bg` → `applyPayloadRun` (`IT:171-181`). `BlockModel::insertText` calls it over the inserted run after
the insert, inside the same transaction (`BM:5565-5575`).

### 1.6 Choice chips

A chip is a `choice` span whose text is the selected option's label and whose `href` is compact JSON
`{"o":[{"id","l","c"?}…],"v":"<selected id>"}` (`ITh:98-101`).

| Function | Rule | Ref |
|---|---|---|
| `sanitizeChoiceLabel` | Removes every `` ` * _ ~ [ ] \ `` and `\n`, `\r`; trims; an empty result becomes the literal `Option` | `IT:252-260` |
| `encodeChoicePayload` | `QJsonDocument::toJson(Compact)` — keys come out sorted: `o` before `v`; inside an option `c`, `id`, `l` | `IT:281-283` |
| `choiceAt(col)` | The chip with `s <= col < e`, else none | `IT:285-289` |
| `insertChoice(col, payload)` | `col` clamped; if strictly inside a chip (`s < col < e`) it moves to that chip's `e`. Label = label of option `payload.v` (empty if unknown — a zero-length chip is then pushed). `shiftSpansInsert`, insert the label, push the chip. Returns the chip's start | `IT:291-300`, `T:3899-3905` |
| `editChoice(spanStart, edit)` | Finds the chip with `s == spanStart` exactly; parses its payload; `edit` changes the payload and names the label, or aborts. The chip is removed, `shiftSpansDelete` over its old range, `shiftSpansInsert` at its start for the new label, text replaced, chip pushed again (it moves to the end of the vector) | `IT:302-317` |
| `selectOption` | false if already selected or the id has no label; else sets `v` | `IT:319-325` |
| `addOption` | Sanitised label, appended `{id, l, c?}` (`c` only when non-empty), **and selected** | `IT:327-338` |
| `setOptions` | false for an empty list. Rebuilds `o` from `[{id?, label, color?}]`, minting missing ids; the selection survives if its id does, else the first option is selected | `IT:340-364` |
| `choiceRanges` | `[{s, e, color}]`, colour of the selected option | `IT:366-378` |

Atomicity is **not** in the engine. `shiftSpansInsert` grows a chip when text is inserted strictly inside it, and
`replaceRange` / `deleteRange` shrink it, leaving text that no longer equals the label. The guards are in the callers:
`splitBlock` snaps a split point strictly inside a chip to the chip's end (`BM:6234-6235`); the editor hops the caret out
before typing and removes the whole chip on Backspace / Delete (`Editor.qml:1736-1741`, `:1753-1758`, `:1768-1771`);
`removeChoiceAt` deletes the chip's whole range (`BM:5081-5085`).

### 1.7 Block-level text operations (in `BlockModel`, over the engine)

| Operation | Rule | Ref |
|---|---|---|
| `insertText(row, col, text, marks, fg, bg)` | Refused on media and divider rows. Engine insert, then typing attributes over `[col, col+len)` | `BM:5554-5585` |
| `replaceText(row, s, e, text)` | Refused on media / divider. Runs `replaceRange` on copies first; a false result opens no transaction | `BM:6884-6904` |
| `setContent(row, text)` | `setText` (spans clamp). **No opaque-row guard** | `BM:5462-5474` |
| `deleteRange(aRow, aCol, fRow, fCol)` | Ends ordered. Refused when the low row is media / divider, or when the two ends are not in the same container (`cell` differs or `splitRowOf` differs). Same row: engine delete. Different rows: see "join" below | `BM:5476-5552` |
| **Split** `splitBlock(row, col)` | Refused on media, divider and split records. `col` snapped out of a chip, then clamped. Left text stays in the block (same id). A new block (new ULID, rank between this block and the next) takes the right text. Spans: every span with `s < col` stays as `[s, min(e, col))`; every span with `e > col` goes to the new block as `[max(s, col) - col, e - col)`; a straddling span appears on both sides with the same `href`. New block type: **paragraph**, except that a list type (`list_item`, `task_item`, `ordered_item`) continues with the same type and `depth` (a new task item starts at state 0). The new block inherits `cell`. Heading level, quote, code language are not inherited | `BM:6227-6282` |
| **Join** (`deleteRange` across rows `lo < hi`) | The low block survives with its id, type and attrs. Text = `lo[0, loCol) + hi[hiCol, end)`; if the high row is media / divider its whole content is dropped (`hiClip = len`). Spans: low spans with `s < loCol` kept as `[s, min(e, loCol))`; high spans with `e > max(s, hiCol)` appended as `[max(s, hiCol) - hiCol + loCol, e - hiCol + loCol)`. No merge of spans that now touch. Rows `lo+1 … hi` are deleted (every block between the ends, whatever it is) | `BM:5498-5544` |
| Backspace at a block's start | With a previous block in the same container that is not a record: previous is media / divider → the caret selects it, nothing merges; else `deleteRange(prev, len(prev), row, 0)` — a join into the previous block | `Editor.qml:2022-2049` |
| Delete at a block's end | Mirror: `deleteRange(row, len, next, 0)` | `Editor.qml:2051-2077` |
| `clearFormat(row, s, e)` | Engine `clearFormat`; refused on media / divider | `BM:4796-4801` |
| `commitMarkdown(row)` / load pass | Typed markers are consumed into spans (§3.6) | `BM:5228-5255`, `BM:5149-5226` |

### 1.8 Existing tests

All tests are in one file, `tests/regression_test.cpp` (a hand-rolled `CHECK` macro, not QtTest).

| Test | Line | Notable cases |
|---|---|---|
| [66] `testInlineEngine` | `T:3746` | Kind names round-trip, 0 and 9 nameless; `addSpan` merges touching spans `[2,5)+[5,8)`; `removeSpan` splits in two; same-payload runs coalesce, a different payload takes its range (3 spans); insert at a span's start shifts, inside grows, at the exact end does not extend; a delete over the head collapses it; deleting a whole span drops it; payload survives shifts |
| [67] `testInlineTextOps` | `T:3801` | Insert clamps (`at = 99`); empty delete is a no-op; `replaceRange` keeps a covering span; replace-nothing-with-nothing is false; `setText` clamps, to empty drops all; typing attributes `1 or 16` + fg pen → 3 spans; a model sequence insert / replace / delete keeps bold on the right word; shortening drops spans past the end |
| [68] `testInlineFormatOps` | `T:3840` | Empty range and kind 0 are no-ops; `setFormat` clamps (`end = 99`); toggle removes when covered, adds when not; `clearFormat` removes styles and link, keeps the chip; `payloadCovers` needs the exact value; the QML span feed carries `u` exactly for payload kinds |
| [69] `testInlineChoiceOps` | `T:3887` | Insert at 1 in `"x  y"`; later spans shift; an insert at an offset inside a chip lands after it; select same / unknown id is a no-op; select swaps the label and shifts the next chip; `addOption` sanitises (`"  Bl*ocked  "` → `Blocked`) and selects; `setOptions` keeps a surviving selection, mints ids, falls back to the first, refuses an empty list |
| [70] `testPayloadOnlyEditsAreEdits` | `T:3962` | A link retargeted over the same range, or a code language change, is one undo entry and dirties the document |
| [65] `testReplaceTextSpans` | `T:3702` | Same-length and longer replacement under a link; a later bold span shifts by the delta; two replaces = two entries; refused on a divider |
| [5] `testUndoBranchCoalesce` | `T:243` | §6.3 |
| `testInlineChoice`, `testSpecsForRange` | `T:2313`, `T:3111` | Not read in detail |

No test exists for `rankBetween`, `encode62` or `makeUlid` (searched by name).

---

## 2. Ids and order

### 2.1 `makeUlid` (`Doc:25-38`)

| Item | Rule |
|---|---|
| Length, alphabet | 26 characters of `0123456789ABCDEFGHJKMNPQRSTVWXYZ` (Crockford base-32, upper case, no I L O U) |
| Time part | Characters 0–9: `QDateTime::currentMSecsSinceEpoch()` as unsigned, 5 bits per character, most significant first (50 bits written from a 48-bit value) |
| Random part | Characters 10–25: sixteen independent draws `QRandomGenerator::global()->bounded(32)` |
| Monotonicity | **None.** No state is kept between calls; two ids minted in the same millisecond are in random relative order |
| Parsing / validation | None found. Ids are opaque strings everywhere they are read |

The same generator mints block ids, element ids, choice option ids (`BM:5063`, `BM:1380`), save temp-file names and the
untitled file name. Check-column options use the fixed ids `"0"`, `"1"`, `"2"` (`BM:1157-1168`); tests use ids such as `"a"`.

### 2.2 `rankBetween(a, b)` (`BM:2411-2430`, alphabet `BM:78-79`)

Alphabet: `0-9A-Za-z` (62 digits, ascending ASCII), so plain byte comparison is numeric order. Ranks are variable-length
strings; a shorter string that is a prefix of a longer one sorts first.

```
r = "", i = 0, bInf = (b is empty)
loop:
    ca = (i < len(a)) ? digit(a[i]) : 0                    # a padded with the minimum digit
    cb = (bInf or i >= len(b)) ? 62 : digit(b[i])          # b padded with one past the maximum
    if ca + 1 < cb:  return r + alphabet[(ca + cb) / 2]    # integer division
    r += alphabet[ca]
    if ca < cb: bInf = true                                # now strictly below b: b no longer bounds
    i += 1
```

- Empty `a` = "before everything", empty `b` = "after everything" (`BMh:126-128`).
- A character outside the alphabet gives `indexOf = -1`; not guarded.
- The inputs are not validated: `a >= b` is not detected.
- The last character emitted is always a midpoint ≥ 1, so `rankBetween` **never returns a rank ending in `0`**.

| `a` | `b` | Result | Note |
|---|---|---|---|
| `""` | `""` | `V` | The first block of a new page (`Canvas:542`) |
| `V` | `""` | `k` | Append |
| `""` | `V` | `F` | Prepend |
| `F` | `H` | `G` | Room for a digit |
| `F` | `G` | `FV` | Adjacent digits: keep `a`'s digit, extend |
| `Az` | `B` | `AzV` | Adjacent with a carry: extend |
| `A` | `AV` | `AF` | `a` is a prefix of `b` |
| `0` | `01` | `00V` | |
| `z` | `""` | `zV` | Appending past the last digit extends |
| `""` | `1` | `0V` | Prepending below the first non-zero digit extends |
| `""` | `0` | `0V` | **Not below `b`** — no rank exists below `0`; the function does not notice |
| `A` | `A0` | `A0V` | **Not below `b`** — nothing lies between a string and itself + `0` |
| `V` | `V` | `VV` | Equal inputs: a rank above both |
| `V` | `F` | `VV` | Reversed inputs: `b` ignored |

Growth (no rebalancing pass exists anywhere; searched for other rank writers — only `rankBetween` and the dev-only
`encode62` seeds at `BM:2465`, `BM:2655`):

| Pattern | Sequence |
|---|---|
| Repeated append | `V k s w y z zV zk zs zw zy zz zzV …` — one more character every 6 appends; 6,000 appends → a 1,000-character rank |
| Repeated prepend | `V F 7 3 1 0V 0F 07 03 01 00V …` — one more character every 5 |
| Chain of N between fixed neighbours (`prev = rankBetween(prev, next)`) | Between `V` and `k`: `c g i j jV jk js jw jy jz jzV …`; 6,000 in a chain → the last rank is 1,001 characters, 3.0 MB of rank text in total |

The chain pattern is what bulk writers use: `spliceSpecsAt` for paste / import / merge (`BM:6135-6150`), `replaceBand`
for every table grid edit — which re-ranks **every block of the table** each time (`BM:3048-3051`, `BM:3085`) — and
`insertTableRows` (`BM:2117-2128`).

### 2.3 Other rank helpers

| Helper | Rule | Ref |
|---|---|---|
| `encode62(v, width)` | Fixed-width base-62, zero padded. Used only to seed synthetic documents (`seedSyntheticStore`, `rebuild`); such ranks can end in `0` | `BM:83-87` |
| `Document::updateRank` | `UPDATE blocks SET rank = ?, modified = ?` | `Doc:356-366` |
| In-memory order | The row index in `rows_ / ids_ / ranks_ / content_`. Ranks are never compared in memory; they are only compared by SQLite at load | `BMh:1107-1110` |

---

## 3. The store (`Document.cpp`, the gate in `CanvasDocument.cpp`)

### 3.1 Pragmas and DDL

At every open, in this order (`Doc:109-130`): `PRAGMA journal_mode=WAL`, `PRAGMA synchronous=NORMAL`,
`PRAGMA foreign_keys=ON`, then:

```sql
CREATE TABLE IF NOT EXISTS blocks (
  id       TEXT PRIMARY KEY,
  rank     TEXT NOT NULL,
  depth    INTEGER NOT NULL DEFAULT 0,
  type     TEXT NOT NULL,
  attrs    TEXT,
  content  TEXT NOT NULL DEFAULT '',
  created  INTEGER,
  modified INTEGER
);
CREATE INDEX IF NOT EXISTS blocks_order ON blocks(rank, depth, type);
CREATE TABLE IF NOT EXISTS doc_meta (
  id INTEGER PRIMARY KEY CHECK (id = 1),
  title TEXT, schema_version INTEGER, app_version TEXT,
  created INTEGER, modified INTEGER, last_cursor TEXT,
  page_width INTEGER, format TEXT
);
```

`ensureCanvasSchema()` (`Doc:429-450`), run by `ensurePages()` on every open (`Canvas:165-182`):

```sql
CREATE TABLE IF NOT EXISTS elements (
  id TEXT PRIMARY KEY, kind TEXT NOT NULL,
  x REAL NOT NULL DEFAULT 0, y REAL NOT NULL DEFAULT 0, w REAL NOT NULL DEFAULT 0, h REAL NOT NULL DEFAULT 0,
  z INTEGER NOT NULL DEFAULT 0,
  parent_id TEXT REFERENCES elements(id) ON DELETE SET NULL,
  json TEXT NOT NULL DEFAULT '{}', created INTEGER, modified INTEGER
);
CREATE INDEX IF NOT EXISTS elements_z ON elements(z, created);
CREATE TABLE IF NOT EXISTS canvas_meta (key TEXT PRIMARY KEY, value TEXT);
ALTER TABLE blocks ADD COLUMN page_id TEXT REFERENCES elements(id) ON DELETE CASCADE;   -- only if the column is missing
CREATE INDEX IF NOT EXISTS blocks_page_order ON blocks(page_id, rank, depth, type);
```

`pdf_text` (a search cache) is created on first write: `(key TEXT NOT NULL, page INTEGER NOT NULL, mtime INTEGER, size
INTEGER, text TEXT NOT NULL DEFAULT '', PRIMARY KEY (key, page))` (`Doc:649`). Old files may carry `block_ink`
(1.x; read once then dropped, `Doc:378-401`, `BM:2582-2612`) and inert `comment_threads` / `comment_messages` (`Doc:132-134`).

A sentinel `SELECT count(*) FROM sqlite_master` must succeed or the open fails (`Doc:143-150`).

### 3.2 The open gate (`Canvas:139-161`, called at `Canvas:285`)

The file is first byte-copied to a scratch path (plus a sibling `-wal` if one exists, `Canvas:263-279`) and
`Document::open` runs on the copy — so the two `CREATE TABLE IF NOT EXISTS` above run **before** the gate, on the copy.
Then, with `f = doc_meta.format` (`""` when the row or the column is missing) and `v = doc_meta.schema_version` (0 when
missing or NULL):

| `f` | `v` | Result | Message (`lastOpenError`) |
|---|---|---|---|
| `mnd2` | ≤ 2 (including 0, 1) | Opens read-write | — |
| `mnd2` | > 2 | Refused | "This document was made with a newer minNotes and can't be opened." |
| `mnd` | exactly 1 | Opens **read-only**; converted on the working copy (`Converter.cpp`); Save routes to Save As | — |
| anything else | > 0 | Refused | "This document was made with an earlier minNotes and can't be opened." |
| anything else | 0 | Refused | "This file isn't a minNotes document." |
| (path does not exist, or a new untitled document) | — | Gate skipped | — |
| (not an SQLite file / unusable) | — | `open` returns false | **empty** — only a log line (`Canvas:280-284`) |

Constants: `kSchemaVersion = 2`, `kFormat = "mnd2"`, `kFormatV1 = "mnd"` (`Document.h:68-70`). `Document::open` also
logs a soft "newer format" warning (`Doc:156-159`); it is never the deciding check. Tests: `T:4247-4365`.
(`.mnpkg`: the manifest's `formatVersion` must equal the build's constant, `Canvas:215-216`; not in this step's scope.)

### 3.3 What is written to `doc_meta`

Only `stampMeta()` writes it (`Doc:163-180`), and only on the save paths — Save, Save As, snapshot, and the 1.x converter
(`Canvas:388`, `:443`, `:487`, `Converter.cpp:208`). A working copy that was never saved has **no row**.

| Column | Value |
|---|---|
| `id` | 1 |
| `schema_version` | integer 2 |
| `app_version` | the build's version string (`"0.0.0"` in targets built without the define, `Doc:15-17`) |
| `created` | integer, ms since the Unix epoch; written on INSERT only — the `ON CONFLICT` update leaves it |
| `modified` | integer, ms since the epoch, every stamp |
| `format` | `"mnd2"` |
| `title`, `last_cursor`, `page_width` | never written. `page_width` is read once (`Doc:200-208`, default 760) to size the first page of a file that has none |

### 3.4 What is written to `canvas_meta`

Upsert by key (`Doc:667-677`); a missing key reads as `""`. Every key found (searched for all callers):

| Key | Value form | Written | Read |
|---|---|---|---|
| `focusPage` | A page element id, or `""` when the canvas (not a page) is in view | On every save path (`Canvas:807-808`); when the first page is created and the key is empty (`Canvas:176-177`) | At open; used only if a page with that id exists (`Canvas:325-326`) |
| `camera` | Compact JSON `{"x":<double>,"y":<double>,"z":<double>}` | On save paths, only once a camera is valid (`Canvas:809-815`) | At open; valid only if `z` exists and is > 0 (`Canvas:328-330`) |
| `defaultView` | `"canvas"` | 1.x converter, only if empty (`Converter.cpp:205`) | No reader found in `app/` |
| `pageWidthDefault` | A decimal integer as text (e.g. `"880"`) | 1.x converter (`Converter.cpp:206`) | No reader found in `app/` |
| `convertedFrom` | `"mnd1"` | 1.x converter (`Converter.cpp:207`) | No reader found in `app/` |

### 3.5 Pages and block loading

| Step | Rule | Ref |
|---|---|---|
| Elements | `SELECT … FROM elements ORDER BY z, created, id` | `Doc:471-479` |
| Pages | The elements with `kind = 'page'`, in that order. If there is none, one is created (width = `page_width + 2 × inset`) | `Canvas:165-178`, `:496-507` |
| Orphans | `UPDATE blocks SET page_id = <first page> WHERE page_id IS NULL` — on every open | `Doc:625-633`, `Canvas:180` |
| A page's blocks | `SELECT id, rank, depth, type, attrs FROM blocks WHERE page_id = ? ORDER BY rank` | `Doc:231-234` |
| Collation | None declared: SQLite's default `BINARY` (byte order of the UTF-8 text) — equal to the rank alphabet's order |
| Ties | **No tie-break is specified.** Two blocks with the same rank load in whatever order SQLite returns |
| Content | Fetched eagerly, one `SELECT content FROM blocks WHERE id = ?` per block | `BM:2489`, `Doc:252-260` |
| Verbatim rows (canvas undo of a page removal) | `SELECT id, rank, depth, type, attrs, content, created, modified, page_id … ORDER BY rank` | `Doc:580-602` |

Block timestamps: `created` and `modified` are integers, **milliseconds since the Unix epoch**
(`QDateTime::currentMSecsSinceEpoch`). `appendBlock` sets both to now (`Doc:307-320`); `updateContent`, `updateMeta`
and `updateRank` each set `modified` (`Doc:327-366`). Neither is read back by the page model. `attrs` is bound as SQL
**NULL** when the JSON object is empty (§5.1); `content` is coalesced from NULL to `''` on every write (`Doc:313-318`, `:331-334`).

### 3.6 Per-block parsing at load (`BM:2488-2552`)

| Field | Rule |
|---|---|
| `type` | `heading`, `code`, `media`, `quote`, `list_item`, `task_item`, `ordered_item`, `divider`, `split`; **anything else (including `table`) is a paragraph** (`BM:2383-2394`) |
| `attrs` | Parsed as a JSON object; unparsable or NULL = empty object |
| `level` | Heading only: `attrs.level` (default 1) clamped to 1…6 |
| `state` | `task_item` only: `attrs.state` (default 0) clamped to 0…2 |
| `depth` | List types only: the `depth` **column** clamped to 0…8. Any other type gets 0 whatever the column holds |
| `lang` | Code only: `attrs.lang` string |
| `cell` | Any type: read only if `attrs.cell` is a JSON number; clamped to 0…63 (so a negative number becomes 0). Absent = top level (−1) |
| `ratios`, `header`, `table` | `split` only: `ratios` array of numbers → float32; `header` clamped to 0…255; `table` kept only if it is a non-empty object, re-serialised compact |
| `spans` | Each `{s, e, k, u}`: kept if `k` is a known name and `e > s`; `u` is read for every kind. Then (after the markdown pass) both ends are clamped to the text length and empty spans drop. Spans are not sorted, merged or de-duplicated |
| Markdown pass | For `paragraph`, `quote`, `list_item`, `task_item` only: `convertMarkdown` consumes `` `x` `` → code, `**x**` → bold, `*x*` → italic, `~~x~~` → strike, `[label](url)` → link (first closing marker wins, left to right, no nesting, no escapes; `BM:5149-5226`). Existing spans are remapped; non-payload ones are re-added through `addSpan`. If anything was consumed the block's content and attrs are **rewritten in the working copy** (`BM:2562-2576`) without an undo entry or the dirty flag |
| Media | `fillMediaMeta` reads `w`, `h`, `kind`, `dw` from the descriptor (`BM:2667-2686`) |
| 1.x frame ink | A `block_ink` row with `space = "frame"` for a media block is moved into the descriptor's `ink["0"]` (`BM:2582-2612`) |

After all rows are parsed, `repairStructure` runs over the page (§4.5), and its changes are persisted the same silent way.

---

## 4. Structure of the flat block list

### 4.1 `depth` (list items)

| Rule | Ref |
|---|---|
| Stored in the `depth` column, 0…8 (`kMaxListDepth`), meaningful for `list_item`, `task_item`, `ordered_item` only | `BMh:146-149` |
| It is a free number per item: nothing requires an item's depth to be at most one more than the item above it. Indent / outdent shift every list item in a range by ±1, clamped, skipping non-list rows | `BM:3450-3475` |
| Leaving the list family (type change) sets depth 0 | `BM:4162` |
| Splitting a list item continues type and depth | `BM:6264-6267` |
| The ordered number is computed, never stored: walk upward through the **flat list**; a list item of any type with greater depth is skipped; an `ordered_item` at the same depth counts; anything else stops. Numbering starts at 1 | `BM:3476-3488` |
| The walk does not look at `cell`: it runs across lane boundaries inside a split row (a record stops it) | same |

### 4.2 The `split` record and its lanes

A split row is one `split` block (the **record**) followed, in rank order, by its lanes' blocks.

| Element | Encoding | Ref |
|---|---|---|
| Record | `type = "split"`, `content = ""`, no `cell`. Never a caret target | `BM:2867-2878`, `T:4370-4385` |
| Lane block | Any non-split block with `attrs.cell = <lane index>`, 0-based | `BM:3533` |
| Extent of the split row | The record plus every following block that has a `cell`; it ends at the first block without one (a top-level block or the next record) | `BM:2259-2263` |
| A lane | A maximal run of consecutive blocks with the same `cell`. After repair the runs appear in ascending order `0, 1, 2 …` with no gaps; a lane's blocks are contiguous | `BM:2208-2213` |
| Lane count | The number of runs. Layout rows need ≥ 2; table rows need ≥ 1 | `BM:2214-2216`, `BM:312-328` |
| `ratios` | On the record: one fraction per lane, summing to 1, written rounded to 4 decimals (three equal lanes are stored as `0.3333` each); float32 in memory | `BM:3534-3537` |
| Limits | `cell` is clamped to 0…63 at load. New lanes are refused once a row has 63 (`BM:2347`); a table refuses a new column at 63 (`BM:695`); the plan allows 64 lanes and folds anything beyond into lane 63 (`BM:2212-2214`) |
| Nesting | None. A record never has a `cell`; a block in a lane is never a record | `BM:314` |
| Where a new block lands | A block inserted right after a lane block joins that lane; right after a record, lane 0; otherwise top level | `BM:2265-2271` |

### 4.3 Derived tables

Nothing stores "this is a table". The grouping is derived in one pass over the top-level sequence (`BM:331-359`):

```
cur = none
for each block in order:
    if it has no cell:                                   # a top-level block or a record
        if it is not a split record:      cur = none     # any top-level non-record block ends the table
        elif record.header > 0:           cur = this     # a head starts a table (also directly under another table)
        else:                             keep cur       # a headerless record joins the table above it, if any
    table_of(block) = cur                                # lane blocks take their record's table
```

| Property | Rule | Ref |
|---|---|---|
| Head | A top-level record with `attrs.header > 0` | `BM:341` |
| Extent | The head's split row plus every **adjacent** following split row whose record has no `header`, until a top-level non-record block or the next head. A layout split row placed directly under a table is therefore part of the table | `BM:336-355`, `T:7209-7217` |
| Rows | The records, top to bottom (`tableRecords`) | `BM:2057-2062` |
| Header rows | The first `header` rows (`tableRowIndex < head.header`). The stored count is 0…255 and is not clamped to the row count when stored | `BM:2052-2055`, `BM:583-610` |
| Column count | `max(len(head.table.cols), 1 + the largest cell index in any row)` | `BM:343-352` |
| Cell `(r, c)` | The blocks of record `r` with `cell == c`, in order — one or more ordinary blocks (text, lists, media…) | `BM:532-539` |
| Ragged rows | A row may have fewer lanes than the table has columns; the missing cells are at the **end** of the row and simply have no blocks. They are created (one empty paragraph) when first written (`ensureGridCell`) | `T:7225-7228` |
| Empty cells | Inside the app a cell never has zero blocks: after every mutation, missing interior lanes and fully emptied rows are refilled with an empty paragraph (`refillTableCells`) | `BM:555-581` |
| `ratios` in a table | Ignored by table geometry (widths come from the column spec). Grid edits write equal fractions `1 / cells`; load repair leaves a table record's ratios untouched | `BM:608-609`, `BM:2221` |
| One-lane rows | Valid inside a table; a one-lane layout row is not (it is unwrapped) | `BM:2215-2216`, `T:7279-7285` |

### 4.4 The `table` attr

On the **head** record, `attrs.table.cols` is an array with one object per column (it may be shorter than the column
count; missing entries are `{}`):

| Key | Type | Meaning | Default when absent | Ref |
|---|---|---|---|---|
| `w` | integer px | Manual column width | Auto width (widest text line + padding, clamped 48…360; 160 if nothing measurable) | `BM:407-410`, `BM:2009-2016` |
| `a` | integer 1 or 2 | Alignment: centre, right | 0 = left (the key is removed for 0) | `BM:941-942` |
| `bg` | string | Column background colour | none (removed when set to `""`) | `BM:816-819`, `BM:922` |
| `fg` | string | Column text colour | none | same |
| `k` | integer 1, 2 or 3 | Column kind: choice, check, timecode | 0 = text (the key is removed for 0) | `BM:1385-1386` |
| `o` | array of `{"id": string, "l": string, "c": string}` | Choice options; present only when `k = 1` | — | `BM:1380-1386`, `BM:1457` |
| `fps` | number | Frame rate of a timecode column | read as 24.0 where cells are normalised (`BM:1020`) | `BM:1340` |

On **every** record of a table (head included), in the same `attrs.table` object:

| Key | Type | Meaning | Ref |
|---|---|---|---|
| `bg` | string | Row background | `BM:907`, `BM:1621` |
| `fg` | string | Row text colour | `BM:1622` |
| `cbg` | array of strings, indexed by lane | Cell backgrounds; `""` = none; trailing empty entries are trimmed; the key is removed when the array is empty | `BM:821-828`, `BM:599-604` |
| `cfg` | array of strings, indexed by lane | Cell text colours, same form | same |

`cols` is removed from every record that is not the first row whenever a grid is rebuilt (`BM:597-598`). The whole
`table` key is omitted when the object is empty (`BM:3540`). Colour resolution for a cell: the cell's, else the row's,
else the column's (`BM:854-863`). Colour strings are stored as given (hex strings in every caller read); no validation.

Typed cells: a body cell of a choice or check column is **one paragraph** whose text is one chip (`BM:1192-1209`): a
choice cell's chip payload carries the column's whole option list plus `v`; a check cell uses the fixed options
`{"id":"0","l":"To do","c":"#8A8A8A"}`, `{"id":"1","l":"Doing","c":"#0189F1"}`, `{"id":"2","l":"Done","c":"#58A65C"}`
(`BM:1157-1168`), and **state 0 is the empty cell** — no chip is written (`BM:1568`). An empty paragraph means "no
value". Header rows stay plain text. A timecode cell is plain text normalised to `hh:mm:ss:ff`.

### 4.5 `repairStructure` (`BM:2229-2247`, plan at `BM:2186-2226`, ratios at `BM:2150-2179`)

Run on every load over the whole page; never refuses. The same plan runs after mutations (`normalizeStructure`,
`BM:2284-2307`) and over pasted specs (`sanitizeSpecStructure`, `BM:6033-6052`).

| # | Input | Repair |
|---|---|---|
| 1 | A block with a `cell` that does not follow a record (at the page start, or after a top-level block) | Becomes top level (`cell` removed) |
| 2 | A `split` block that itself has a `cell` | Becomes a top-level record and starts its own row; the lane blocks after it are its lanes |
| 3 | Lane indices with a gap (`0, 2`) | Renumbered in order of appearance (`0, 1`) |
| 4 | A lane index that reappears after another lane (`0, 1, 0`) | Each run is its own lane (`0, 1, 2`) |
| 5 | More than 64 runs | Runs past the 64th share lane 63 |
| 6 | A record with no lane blocks | The record is **deleted** (also a table head: its `header` and `table` are lost) |
| 7 | A layout record (not in a table) with exactly one lane | The record is deleted; its blocks become top level, in order |
| 8 | A record in a table (a head, or a headerless record directly under a table row) with one lane | Kept |
| 9 | Ratios of a **layout** record | Recomputed by the next table. A **table** record's ratios are left exactly as stored |

"In a table" for rules 7–8 is tracked while scanning: a record with `header > 0` that survives turns it on; a surviving
headerless record keeps the current state; a top-level non-record block turns it off; a deleted record does not change it.

Ratios of a surviving layout record (`old` = stored ratios, `runLane` = the original lane number of each surviving lane):

| Condition, tested in this order | Result |
|---|---|
| Two surviving lanes had the same original number | Equal fractions |
| `old` is empty, or shorter than the largest original lane number + 1 | Equal fractions |
| Any stored ratio is not > 0 | Equal fractions |
| The stored ratios' sum differs from 1 by more than 0.01 | Equal fractions |
| Same lane count and every lane kept its number | `old`, untouched (not renormalised) |
| Otherwise | Each surviving lane takes its old ratio; each vanished lane's ratio is added to the nearest surviving lane with a smaller original number (else to the first); then all are divided by the total |

Not repaired: `header` on a non-split block (ignored at load — only read for `split`); a `header` larger than the table;
`cbg` / `cfg` arrays after rule 3 renumbers a table row's lanes; missing cells at the end of a table row; equal ranks.

A changed block has its attrs rewritten and a removed record is deleted in the working copy in one SQL transaction,
without an undo entry and without setting the dirty flag (`BM:2562-2576`). `structureValid()` (`BM:312-328`) states the
resulting invariants: no record in a lane; `header` only on records; a layout row has ≥ 2 lanes and exactly one ratio
per lane; a table row has ≥ 1 lane.

### 4.6 Worked examples (synthetic)

Rows are shown in rank order as `type · attrs · content`. `depth` is 0 unless shown.

**A — a layout row.**

```
1  paragraph  ·  NULL                    ·  "intro"
2  split      ·  {"ratios":[0.4,0.6]}    ·  ""
3  paragraph  ·  {"cell":0}              ·  "left one"
4  list_item  ·  {"cell":1}              ·  "right one"
5  list_item  ·  {"cell":1}  depth 1     ·  "right two"
6  paragraph  ·  NULL                    ·  "outro"
```

Tree: `Paragraph "intro"`; `Columns` (id = block 2's id, widths 0.4 / 0.6) with column 0 = [block 3] and column 1 =
[block 4, block 5 (depth 1)]; `Paragraph "outro"`. The split row is blocks 2–5; it ends at block 6 because block 6 has no `cell`.

**B — a table: one header row, a typed column, a two-block cell, colours, a ragged row.**

```
1   split      ·  {"header":1,"ratios":[0.5,0.5],"table":{"cols":[{"w":200},
                   {"k":1,"o":[{"c":"#58A65C","id":"OPT-A","l":"Open"},{"c":"#D9534F","id":"OPT-B","l":"Closed"}]}]}}  ·  ""
2   paragraph  ·  {"cell":0}             ·  "Item"
3   paragraph  ·  {"cell":1}             ·  "State"
4   split      ·  {"ratios":[0.5,0.5],"table":{"bg":"#222222","cbg":["","#334455"]}}  ·  ""
5   paragraph  ·  {"cell":0}             ·  "first"
6   media      ·  {"cell":0}             ·  {"h":300,"src":".minnotes/example.png","w":400}
7   paragraph  ·  {"cell":1,"spans":[{"e":4,"k":"choice","s":0,"u":"{\"o\":[…the column's two options…],\"v\":\"OPT-A\"}"}]}  ·  "Open"
8   split      ·  {"ratios":[1]}         ·  ""
9   paragraph  ·  {"cell":0}             ·  "second"
10  paragraph  ·  NULL                   ·  "after"
```

Tree: `Table` (head = block 1; header rows 1; 2 columns: column 0 width 200, column 1 kind choice with two options);
row 0 (block 1) = cells [block 2], [block 3]; row 1 (block 4, row background `#222222`) = cell 0 [block 5, block 6],
cell 1 [block 7] with cell background `#334455`; row 2 (block 8) = cell 0 [block 9] and **no** cell 1 (ragged);
then `Paragraph "after"`. Block 8 has no `header`, sits directly under the table and has one lane: it is a table row,
not a layout row. Its ratios `[1]` and the others' `[0.5, 0.5]` carry no information.

**C — a malformed page and its repair** (the fixture of `T:4456-4472`, ten paragraphs `p0 … p9` reshaped by SQL).

```
before                                           after repair
0  paragraph  {"cell":0}          "p0"           paragraph  NULL          "p0"      rule 1
1  split      {"ratios":[0.5,0.5]} ""            (deleted)                          rule 6
2  paragraph  NULL                "p2"           paragraph  NULL          "p2"
3  split      {"ratios":[1]}      ""             (deleted)                          rule 7
4  paragraph  {"cell":0}          "p4"           paragraph  NULL          "p4"
5  paragraph  {"cell":0}          "p5"           paragraph  NULL          "p5"
6  split      {"ratios":[0.2]}    ""             split      {"ratios":[0.5,0.5]}    ratios: too short → equal
7  paragraph  {"cell":0}          "p7"           paragraph  {"cell":0}    "p7"
8  paragraph  {"cell":2}          "p8"           paragraph  {"cell":1}    "p8"      rule 3
9  paragraph  NULL                "p9"           paragraph  NULL          "p9"
```

Eight blocks remain; the document is not marked dirty; reopening finds the same structure.

**D — table boundaries.** `split{header:1}` + cells, `split` + cells, `split{header:1}` + cells, `split` + cells with no
block between them is **two** tables of two rows each (the second head starts a new table). Inserting any top-level
block between a table's second and third rows leaves the rows below it as plain layout rows (and one-lane ones are
unwrapped by the next normalisation of that band); removing the block rejoins them (`T:7209-7217`).

---

## 5. Block `attrs` and media descriptors

### 5.1 `attrs` as written (`attrsJson`, `BM:3525-3555`)

One compact JSON object; `QJsonObject` serialises keys in sorted order. An empty object is written as SQL **NULL**, not `{}`.

| Key | Written when | Value | Omitted when |
|---|---|---|---|
| `level` | `heading` and level > 0 | integer 1…6 | level 0 (loads as 1) |
| `state` | `task_item`, always | integer 0, 1, 2 (todo, doing, done) — written even for 0 | other types |
| `lang` | `code` and non-empty | string | empty |
| `cell` | The block is in a lane | integer ≥ 0 | top level |
| `ratios` | `split` and non-empty | array of numbers rounded to 4 decimals | empty |
| `header` | `split` and > 0 | integer 1…255 | 0 |
| `table` | `split` and non-empty | object (§4.4) | empty |
| `spans` | Any type with at least one span | array of `{"e": int, "k": "<name>", "s": int, "u": "<payload>"?}` in vector order; `u` only for a payload kind **with a non-empty payload** | no spans |

`depth` is a column, not an attr; it is written on every meta update with the row's current value (0 for non-list
types). There is no other key. Code blocks are written with whatever spans the row holds (normally none; see §8).

Keys per stored type:

| `type` | Possible keys |
|---|---|
| `paragraph`, `quote` | `spans`, `cell` |
| `heading` | `level`, `spans`, `cell` |
| `list_item`, `ordered_item` | `spans`, `cell` (+ `depth` column) |
| `task_item` | `state`, `spans`, `cell` (+ `depth` column) |
| `code` | `lang`, `cell` |
| `media`, `divider` | `cell` |
| `split` | `ratios`, `header`, `table` |

### 5.2 Media descriptors (a media block's `content`)

Compact JSON, built by `MediaStore` (`app/core/MediaStore.cpp`). Which key is present decides the kind: an object
without `kind` is an image.

| Kind | Keys and types | Ref |
|---|---|---|
| image | `src`, `w`: int, `h`: int, `name`: string (optional, only when known) — **no `kind` key** | `MediaStore.cpp:70-77` |
| remote image (before download) | `src`: `http(s)` URL string, `w`: 480, `h`: 300 (placeholders); replaced by `{src, w, h}` of the local copy when the download lands | `MediaStore.cpp:96-102`, `BM:6203-6206` |
| preview (an image rendered from another file) | image keys with `name` = the source file's name, plus `preview`: `{"src", "format": string, "mtime": number (ms), "size": number (bytes)}` | `MediaStore.cpp:195-208` |
| video | `src`, `w`: int, `h`: int, `kind`: `"video"`, `durMs`: integer, `frames`: int, `fps`: number | `MediaStore.cpp:41-51` |
| pdf | `src`, `w`: int, `h`: int (page 0 in PDF points), `kind`: `"pdf"`, `pages`: int | `MediaStore.cpp:52-60` |
| audio | `src`, `kind`: `"audio"`, `name`: string, `durMs`: number, `sampleRate`: int, `channels`: int — no `w` / `h` | `MediaStore.cpp:156-165` |
| file | `src`, `kind`: `"file"`, `name`: string, `ext`: string (lower case, no dot) | `MediaStore.cpp:61-69` |
| folder | `src`, `kind`: `"folder"`, `name`: string, `count`: int (capped at 10000), `countCapped`: true (only when capped), `entries`: array of up to 6 names, `scanned`: integer ms | `MediaStore.cpp:78-95` |
| bookmark | `kind`: `"bookmark"`, `src`: URL string, `host`: string, `title`: string, `icon`: string (a `src`, `""` until fetched), `fetched`: bool | `Canvas:1186-1191`, `:1237-1253` |
| sketch (1.x leftover) | `kind`: `"sketch"`, `images`: array of `{"src", …}`; shown as a file chip until the converter turns it into canvas ink | `BM:2678`, `AssetTransfer.cpp:66-68` |

Keys added to a block's descriptor after creation:

| Key | Type | Meaning | Ref |
|---|---|---|---|
| `dw` | integer 80…65535 | Per-block display width; removed to reset | `BM:5380-5387` |
| `ink` | object keyed by page number as a string (`"0"` for images and video posters) → `{"version": "2.0", "coordinate_system": "normalized", "shapes": […]?, "texts": […]?}` | Annotation ink. A page with neither shapes nor texts loses its key; an empty `ink` leaves the root | `BM:4635-4651`, `BM:2590-2603` |

`src` has two forms (`app/core/PathMap.h:36-48`, `PathMap.cpp:47-55`):

| Form | Meaning |
|---|---|
| JSON **string** | `.minnotes/<file>` (relative to the document's folder — the only form that is ever copied between documents, `AssetTransfer.cpp:83`, `:112`), or an absolute path of the machine that wrote it, or an `http(s)` URL |
| JSON **object** `{"vol": "<mapping id>", "rel": ["segment", …]}` | A path under a mapped volume: `vol` is the id of an application-level path mapping, `rel` the path below that volume's root as separator-free segments |

`toRef` returns the object form when the absolute path falls under an enabled mapping's root for the current OS, else
the string. The decision and the exact segmenting are made in the `min-paths` library (`mp_to_ref`), which was not read:
**not determined** beyond the description above. A consumer must test `src` for string vs object before use
(`AssetTransfer.cpp:81`, `:110`). Which kinds may appear as page blocks rather than only as canvas elements was not
traced kind by kind (bookmark and audio builders are called from the canvas code): **not determined**.

---

## 6. Undo (`BM:3623-4125`, declarations `BMh:935-967`, `BMh:1256-1267`)

### 6.1 What a record holds

| Field | Content |
|---|---|
| `BlockSnap` | One block in full: `id, rank, content, lang, type, level, taskState, depth, spans, cell, ratios, header, table`. No timestamps, no page id |
| `lo` | The first **row index** of the touched band at the time of the edit. Entries are positional, not keyed by id |
| `before`, `after` | Band form: the snaps of rows `[lo, lo + n)` on each side (sizes may differ) |
| `patches` | Sparse form: `[{row, before, after}]` for the rows that changed |
| `permBefore`, `permAfter` | Permutation form: the band's `(id, rank)` pairs in order, each side |
| Caret | Focus row / col and anchor row / col, before and after |
| `parent` | Index of the entry this one follows (−1 = the opened state) |
| `coalesce` | The coalescing key, or empty |
| `ts` | Wall clock (ms) when pushed; a coalesced entry keeps its first |

Exactly one of the three forms is filled. All entries live in one vector that only grows; nothing is pruned or capped
(the History list shows the newest 200 of the active path, `BM:4047-4100`). In memory only; cleared on load and detach.

### 6.2 Transactions (`BM:3786-3905`)

| Step | Rule |
|---|---|
| `beginTxn(lo, hi)` at depth 0 | Remember `lo`, `hi`, the row count, and snapshot rows `[lo, hi]`. `hi = lo − 1` is an empty band (a pure insertion) |
| Nested `beginTxn` | Only increments the depth; its band is ignored — the outermost band must cover everything the group touches |
| `endTxn(key)` at depth 0 | `delta = rows now − rows then`; snapshot `[lo, hi + delta]` as `after` |
| No change | If `before` and `after` are equal field by field (spans compared **in vector order**), nothing is recorded and the document is not dirtied |
| Otherwise | Mark dirty; coalesce (§6.3) or push a new entry whose parent is the current node |
| Form chosen | (1) both sides the same size > 1 with the same id at every position → **sparse**: only rows that differ; (2) else, same size > 1, the same id set, and every block identical except its rank → **permutation**; (3) else **band** (always for a single row and for any insert or delete) |
| During undo / redo | `applying_` is set: no recording |

Applying: band → rows `[lo, lo + count)` are replaced by the other side's snaps; in SQL, ids that left are deleted, ids
on both sides get content, meta and rank updates, new ids are inserted again (`BM:3646-3706`). Sparse → each patch is
written in place if the id at that row still matches (`BM:3738-3774`). Permutation → rows reordered and only ranks
written (`BM:1971-1997`). Undo sets the current node to the entry's parent; redo follows the **newest child** of the
current node (`BM:3951-3984`). The caret of the corresponding side is restored through a signal. Undo and redo always
set the dirty flag. `undoJumpTo` walks undos to an ancestor or redos along the newest-child chain (`BM:4103-4116`).

Caret: the editor mirrors every caret change into the model (`noteCaret`, `Editor.qml:1670-1675`). A new entry starts
with caret-after = caret-before; the first `noteCaret` after the push stamps the real caret-after (`BM:4118-4125`).

### 6.3 Coalescing

An edit merges into the current entry when **all** of these hold (`BM:3837-3847`):

1. The edit passed a non-empty key.
2. There is a current entry and it has no children (nothing can be redone).
3. The current entry's key is equal.
4. Same `lo`; the current entry's `after`, this edit's `before` and `after` are each exactly one block; same block id.
5. The current entry's caret-after (focus row and column only) equals the caret mirrored before this edit.

Effect: the entry's `after` is replaced; its `before`, `ts` and caret-before stay. **There is no timer** and no word or
whitespace boundary: a run ends only when one of the five conditions fails (the caret moved, another block, another
kind of edit, an undo, a grouped edit).

| Key | Passed by | Condition |
|---|---|---|
| `type` | `insertText` | The inserted string is exactly one UTF-16 code unit (`BM:5583`) |
| `del` | `deleteRange` | Same row and exactly one code unit removed (`BM:5548`) — Backspace and forward Delete alike |
| `imgw:<block id>` | `setMediaWidth` | Always (`BM:5400`) |
| `fgcolor`, `bgcolor` | The editor's colour picker drag | Single-row selection with the coalesce flag (`Editor.qml:2745-2749`) |
| the translated string "Refresh preview" | The preview refresh | Always (`BM:4439`) |

`tcimgw:` and `tcell-del:` are tested by the History labels (`BM:4035-4040`) but no caller passes them. Typing then
Backspace are different keys: two entries. Test [5] (`T:243-280`): after an undo the current node has a child, so the
next edit with the same key branches instead of merging.

### 6.4 What is one step

Each model mutator opens and closes its own transaction, so each call is one entry unless coalesced. Several calls are
grouped by `beginGroup(lo, hi)` / `endGroup()` (the same begin / end, `BM:3912-3913`); the editor does this for:
replacing a selection by typing or pasting, multi-row formatting and colouring, multi-row type changes, deletes that
also move the caret's block, and drops (`Editor.qml:930-957`, `:1315-1323`, `:2662-2665`, `:2751-2770`, `:2806-2894`,
`:3143-3246`). Model-side groups: every table operation (one transaction over the whole table band), a document merge,
a paste, an import. The markdown commit when the caret leaves a block is its own step (`BM:5246-5254`), as is the
prefix trigger fired by a typed space — which also ends a typing run.

Not in the undo log: load-time normalisation and repair (§3.6, §4.5), the swap of a downloaded remote image's
descriptor (`BM:6214-6222`), copied media files, the view-only row filter.

---

## 7. `DocumentMerger` (`app/core/DocumentMerger.{h,cpp}`, `AssetTransfer.{h,cpp}`)

It is **not** a merge of two versions of one document and uses **no key**. It copies every block of one open page model
into another at a gap ("tab merge"). There is no three-way merge, no conflict rule and no id matching anywhere in the
application (the only concurrency check is the mtime / size comparison at Save).

| Aspect | Rule | Ref |
|---|---|---|
| Input | Source model, destination model, gap `g` (between rows `g−1` and `g`; clamped to `[0, count]`) | `DocumentMerger.h:44-46` |
| Refusals | A null model → "No document"; source = destination → "A document can't merge into itself"; an asset plan refusal | `DocumentMerger.cpp:29-33`, `:52` |
| Snapshot | `specForRow` for every source row: type, level, task state, depth, lang, text, all spans with payloads, `cell`, `ratios`, `header`, `table`; a media row carries only its descriptor | `BM:5972-5993` |
| No-op | A source of exactly one empty paragraph with no media → success, nothing inserted, result rows −1 / −1 | `DocumentMerger.cpp:40-46` |
| Ids and ranks | Every copied block gets a **new ULID** and a new rank chained between the destination neighbours. Source ids are not kept | `BM:6149-6150` |
| Anchor | The gap is remembered as the id of the destination block above it; at apply time the gap is re-derived from that id, falling back to the planned gap clamped to the current row count if the block is gone | `DocumentMerger.cpp:54-58`, `:82-92` |
| Placement | Always top level: a gap inside a split row, or between two adjacent split rows, moves below the run (`mergeGapFor`) | `DocumentMerger.cpp:93`, `BM:6020-6031` |
| Fold | If the block above the gap is a top-level, empty paragraph / heading / quote / list item, the first copied block replaces it in place (that block keeps its id and rank) | `DocumentMerger.cpp:98-109`, `BM:6103-6130` |
| Structure | Split rows and tables in the source keep their structure, after the same plan as §4.5 (a record the plan would drop becomes an empty paragraph) | `BM:6076-6080`, `BM:6033-6052` |
| Undo | One entry in the destination. The source is never modified | `DocumentMerger.cpp:107-111`, `T:2787-2799` |
| Media | Only `src` strings starting with `.minnotes/` are copied into the destination's sidecar and rewritten in the specs before insertion; absolute paths, `{vol, rel}` objects and URLs pass through unchanged. A destination file with the same name and size is reused; other name clashes get a unique name; a video brings its `.qcview` folder unless one exists. Copied files are outside the undo entry; a cancelled or failed copy removes what it made | `AssetTransfer.h:8-13`, `AssetTransfer.cpp:90-150` |
| After | Remote `http` image descriptors in the inserted range are queued for download | `DocumentMerger.cpp:112` |
| Threading | Plan on the GUI thread, file copy on a worker, apply queued back; if the destination closed meanwhile, the copied files are removed and the result is "Destination closed" | `DocumentMerger.cpp:161-218` |

Tests: [40] `testMergeEngine` (`T:2714`), [41] `testMergeAssetsAndEdges` (`T:2822`, not read in detail).

---

## 8. For the user to rule on

Observations only. Each is read from code; none was reproduced in a running build.

| # | Area | Observation | Ref |
|---|---|---|---|
| 1 | Ranks | `rankBetween` can return a rank that is **not below `b`**: `("", "0")` → `0V`, `("A", "A0")` → `A0V`. No valid answer exists in those cases and the function does not report it. It cannot produce such neighbours itself (it never emits a trailing `0`), but `encode62` seeds and any outside writer can | `BM:2411-2430`, `BM:83-87` |
| 2 | Ranks | Inputs are not checked: equal or reversed ranks return a rank above both; a character outside the alphabet is not guarded | same |
| 3 | Ranks | Rank length is unbounded and nothing rebalances: +1 character per 6 appends; a chained write of N blocks ends near N / 6 characters. Every table grid edit re-ranks the whole table by such a chain, so every block of the table changes rank (and `modified`) on any row / column operation | `BM:3048-3051`, `BM:6135-6150` |
| 4 | Ranks | Equal ranks have no defined load order (`ORDER BY rank` only) | `Doc:232` |
| 5 | Ids | `makeUlid` is not monotonic within a millisecond although comments call the ids time-sortable; elements use `id` as the last sort key | `Doc:25-38`, `Doc:475` |
| 6 | Spans | Two rules for payload spans: the engine's `applyPayloadRun` clips and coalesces (used only by the typing pens); `BlockModel::setPayloadSpan` (links, text colour, highlight from the UI) drops every overlapping same-kind span **whole** and never coalesces. Colouring 2 characters inside a 10-character coloured run removes the colour from the other 8 | `IT:70-86`, `BM:4922-4939` |
| 7 | Spans | `setFormat` / `toggleFormat` / `hasFormat` accept the payload kind names; `addSpan` then creates a payload-less span and merges same-kind spans with different payloads, keeping the left-most payload | `BM:3937-3942`, `BM:5257-5262`, `IT:53-68` |
| 8 | Spans | No canonical form: nothing merges touching same-kind spans after a delete, a join or a split; vector order depends on history; undo's "no change" test and its sparse diff compare spans positionally, so a reorder alone is an edit | `IT:106-118`, `BM:3808-3819` |
| 9 | Spans | Chip atomicity is enforced only by callers. The engine grows a chip when text is inserted strictly inside it and shrinks it on a partial delete or replace, leaving text ≠ label | `IT:99-104`, `IT:137-157` |
| 10 | Spans | `insertChoice` with a `v` that names no option pushes a zero-length chip span | `IT:295-298` |
| 11 | Text | Opening a file **changes literal text**: the load pass consumes marker pairs in paragraphs, quotes, bullet and task items — a paragraph `2*3*4` becomes `234` with `3` italic; an empty pair (two backticks, `****`) just disappears; markers inside text that already carries a code span are consumed too. The same happens when the caret leaves a block | `BM:2529-2535`, `BM:5149-5226` |
| 12 | Text | The markdown pass skips `ordered_item` (both at load and on leaving a block) while it runs on the other two list types; the load pass lacks the "starts with three backticks" guard that the leave-block pass has | `BM:2529`, `BM:5231`, `BM:5243` |
| 13 | Text | `isOpaqueRow` is media and divider only (its comment also names tables). A split **record** is not refused by `insertText`, `replaceText`, formatting or `deleteRange`'s low end; `setContent` has no guard at all and would overwrite a media descriptor | `BM:6284-6288`, `BM:5462-5474` |
| 14 | Text | A join keeps the earlier block's type and appends the later block's spans: joining into a code block leaves spans on a code block; splitting a heading, quote or code block always makes the remainder a paragraph | `BM:5512-5523`, `BM:6258-6267` |
| 15 | Text | The typing key tests "one UTF-16 code unit": a character outside the BMP typed as one keystroke (2 units) never coalesces | `BM:5583` |
| 16 | Structure | Load repair renumbers lanes in table rows as well (cells `0, 2` → `0, 1`), moving a cell one column left, while `cbg` / `cfg` stay indexed by the old lane. The in-app path refills missing interior cells first; the load path does not | `BM:2208-2213`, `BM:555-581`, `BM:2229-2247` |
| 17 | Structure | A negative `cell` in a file loads as lane 0 (clamp); `cell` > 63 loads as 63, merging lanes | `BM:2504-2505` |
| 18 | Structure | Three different lane limits: 64 (plan), 63 lanes (layout split), 63 columns (table insert) | `BM:2212-2214`, `BM:2347`, `BM:695` |
| 19 | Structure | Table extent is by adjacency only: a headerless layout row placed directly under a table becomes a table row; there is no way to store "table, then columns" without a block between them | `BM:336-355` |
| 20 | Structure | The stored `header` count is not clamped to the number of rows. Some readers clamp (`tableStickyAt`), others do not: `isHeaderRow` then reports every row as a header, and "delete row" on any row deletes the table | `BM:2052-2055`, `BM:661-665`, `BM:454` |
| 21 | Structure | A table head with no lane blocks is deleted by the repair plan, silently dropping `header` and the column spec; the rows below become layout rows (one-lane ones unwrap) | `BM:2216-2218` |
| 22 | Structure | Ordered-list numbering walks the flat list across lanes: the first ordered item of lane 1 continues the count of an ordered item ending lane 0 | `BM:3476-3488` |
| 23 | Structure | A table record's `ratios` are never validated or used, yet always written | `BM:2221`, `BM:608-609` |
| 24 | Store | `blocks.created` is not stable: undoing a delete re-inserts the block with `created = now`. `modified` is touched by rank-only rewrites | `BM:3702`, `Doc:307-320` |
| 25 | Store | `mnd2` with `schema_version` 0, 1 or NULL opens; the gate is skipped when the path does not exist; a non-SQLite file fails with an empty reason | `Canvas:139-161`, `:280-285` |
| 26 | Store | The gate runs after `CREATE TABLE IF NOT EXISTS blocks / doc_meta` on the working copy; harmless to the original, but a refused foreign database has been given two tables in the copy | `Doc:113-130`, `Canvas:280-285` |
| 27 | Store | `canvas_meta` keys `defaultView`, `pageWidthDefault`, `convertedFrom` are written by the converter and read by nothing in `app/` | `Converter.cpp:205-207` |
| 28 | Undo | Entries are addressed by row index, never pruned, and hold full block snapshots (a table edit snapshots the whole table twice unless it compresses). Undo / redo always dirty the document, including a return to the saved state | `BM:3786-3905`, `BM:3962` |
| 29 | Undo | No time or word boundary in coalescing: a paragraph typed without moving the caret is one step | `BM:3837-3847` |
| 30 | Undo | A translated UI string is used as a coalesce key ("Refresh preview"); the keys `tcimgw:` and `tcell-del:` are read but never written | `BM:4439`, `BM:4035-4040` |
| 31 | Merger | The class named `DocumentMerger` copies one document into another; nothing in the application merges versions | §7 |
| 32 | Survey | `minnotes-survey.md` §2.2 says a `split` block's `content` holds a "lane layout descriptor". In the code a record's content is always `""` and the layout is entirely in `attrs`; a comment at `BM:2437` still speaks of a split descriptor in the content | `BM:2867-2878`, `BM:2437` |
| 33 | Survey | The survey's DDL omits `NOT NULL` on `blocks.depth`; and its "typing at a span's exact end does not extend it" should be read with its twin: typing at a span's exact **start** is outside the span too | `Doc:116`, `IT:101` |

---

## 9. Not determined

- The exact `{vol, rel}` production and resolution (inside the `min-paths` library, not read).
- Which media kinds can occur as page blocks as opposed to canvas elements only (bookmark, audio, folder).
- Whether any path writes a non-empty `content` to a `split` record (none was found; the model does not forbid it — §8 #13).
- The order SQLite returns for equal ranks.
- `testInlineChoice`, `testSpecsForRange`, `testMergeAssetsAndEdges` and the clipboard payload were not read in detail.
- The rules of the 1.x converter (`Converter.cpp`) beyond the three `canvas_meta` keys.
