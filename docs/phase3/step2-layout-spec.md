# minNotes — layout and typography spec (step 2 input)

Read from the Qt application, branch `min-paths` at `59d1129` (2026-11-10), read only. Every number is in page units
(logical px at 100 % zoom). File abbreviations:

| Abbr. | File (under the minNotes repo) |
|---|---|
| BV | `app/qml/MinNotes/App/BlockView.qml` |
| ED | `app/qml/MinNotes/App/Editor.qml` |
| TH | `app/qml/MinNotes/App/Theme.qml` |
| MB | `app/qml/MinNotes/App/MediaBlock.qml` |
| BM.cpp / BM.h | `app/core/BlockModel.cpp` / `.h` |
| LI | `app/core/LayoutIndex.h` |
| IMH | `app/core/InlineMarkdownHighlighter.cpp` |
| CD | `app/core/CanvasDocument.h` / `.cpp` |

Old-model terms used below: a *record* (type 10) is one split row or one table row; a *lane* is a column of a split row
or a table cell; `colLeft` is the left x of a block's content column; `measure` is that column's width; `te` is the
block's text box.

Derived values that every section uses (BV:49-52, 71, 553):

```
lane      = { x, w }            page-relative; the page (x 0, w pageWidth) for a top-level block
cellInset = 8 in a table cell, else 0
colLeft   = leftEdge + lane.x + cellInset
measure   = lane.w − 2 × cellInset
```

---

## 1. Fonts

### 1.1 Families and loading

| Role | Family in use | Token | Source |
|---|---|---|---|
| Body, headings, quotes, lists, table cells | Geist | `Theme.font.body` | TH:171 |
| Chrome | Geist | `Theme.font.family` | TH:170 |
| Code block, inline code, code chips | Geist Mono | `Theme.font.mono` | TH:172 |
| Icons | Phosphor (regular / fill / duotone / thin) | `Theme.icon.family*` | TH:158-161 |

- All faces are `.ttf` files under `app/qml/MinNotes/App/fonts/`, compiled into the QML module's resources
  (`app/CMakeLists.txt:259-…`) and loaded with one `FontLoader` each (TH:99-155). Qt files every Geist cut under the one
  family name, so a weight number picks the cut (TH:117).
- Geist cuts bundled: Light 300, Regular 400, Medium 500, SemiBold 600, Bold 700, and Italic / LightItalic /
  MediumItalic / BoldItalic. Geist Mono: Regular, Bold, Italic, BoldItalic.
- `Theme.geist` is a constant `true` (TH:107). With it false the app falls back to Inter (chrome), Aspekta (body),
  PT Serif (paragraphs only, 15 px), JetBrains Mono (TH:170-176). That branch is dead in the shipped build; the numbers
  below are for Geist.
- Developer switches, no UI: `--heading-weight=N`, `--body-weight=N`, `--serif-font=`, `--serif-size=` (TH:119-120,
  133, 146).
- No application default font is set (`app/main.cpp`, `MinNotesApplication.cpp`: no `setFont`). Three `Text` items in
  the block view name no family and so use the platform default font: the bullet glyph (BV:721-726) and the two check
  marks (BV:755-761, 780-786).
- Font file vertical metrics (measured from the bundled files; all Geist and Geist Mono cuts identical): unitsPerEm
  1000, ascender 1005, descender −295, line gap 0, `USE_TYPO_METRICS` set → natural line height = 1.300 × pixel size.

### 1.2 Sizes and weights per block type

| Block | Pixel size | Weight | Family | Colour token | Source |
|---|---|---|---|---|---|
| Paragraph | 14 (`sizeBody`) | 500 Medium (`bodyWeight`) | body | `text` | BV:625-635, TH:179-182 |
| Heading 1 | 33 | 300 Light (`headingWeight`) | body | `textBright` | BV:615, 630, 632; TH:178 |
| Heading 2 | 28 | 300 | body | `textBright` | BV:615 |
| Heading 3 | 24 | 300 | body | `textBright` | BV:615 |
| Heading 4 | 20 | 300 | body | `textBright` | BV:615 |
| Heading 5 | 18 | 300 | body | `textBright` | BV:615 |
| Heading 6 | 16 | 300 | body | `textBright` | BV:615 |
| Quote | 14 | 500 | body (upright) | `textMuted` | BV:619, 624 |
| Bullet / numbered item | 14 | 500 | body | `text` | BV:624, 635 |
| Task item | 14 | 500; strikeout when done | body | `text`; `textMuted` when done | BV:620, 636 |
| Code block | 13 (`sizeMono`) | 400 | mono | `codeText`, then syntax colours | BV:618, 622, 627, 634; TH:183 |
| Table cell (body row) | as its block type | as its block type | body | cell colour, else as type | BV:616 |
| Table cell (header row) | as its block type | 700 Bold | body | as above | BV:633 |

- The heading size array is `[26, 33, 28, 24, 20, 18, 16]`, indexed by level clamped to 1–6; index 0 is never read
  (BV:615, 630).
- A heading ignores bold spans (`ignoreBold`, BV:677; IMH:100).
- Letter spacing: none set anywhere in the block view or the editor. Kerning / hinting: Qt defaults.
- A paragraph inside a table is the same 14 px / 500 as outside (the serif exception at BV:623, 628 is off under Geist).

### 1.3 Line height

- Rule (BV:645-646): `lineH = round(pixelSize × 1.35)`; the text box's height is pinned to
  `max(1, lineCount) × lineH`. Verified. One rule for every text type.
- Resulting values:

| Type | px | `lineH` | Natural (1.30 ×) |
|---|---|---|---|
| Body, quote, lists | 14 | 19 | 18.2 |
| Code | 13 | 18 | 16.9 |
| H1 | 33 | 45 | 42.9 |
| H2 | 28 | 38 | 36.4 |
| H3 | 24 | 32 | 31.2 |
| H4 | 20 | 27 | 26.0 |
| H5 | 18 | 24 | 23.4 |
| H6 | 16 | 22 | 20.8 |

- **`lineH` sizes the box; it does not space the lines.** `TextEdit` has no line-height property (BV:637-644), so the
  wrapped lines inside a block sit at the font's natural pitch from the box's top, and the difference
  (`lineCount × (lineH − natural)`) is left as slack at the bottom of the box. The caret and the selection rects are the
  natural line's rect (§10). The exact per-line pitch Qt produces after its own rounding: not determined (the comment
  at BV:637-639 records 19 or 20 px single-line heights for the earlier body font).
- A line that mixes fonts (inline code in Geist Mono) has the same metrics as the body font, so it does not change the
  line's height. Bold / italic cuts likewise.

### 1.4 Scaling

- No text-size setting exists. The only scale is the view zoom (ED:128-198): a `Scale` transform on the whole page
  layer (ED:6139-6142). Steps `[0.5, 0.67, 0.75, 1, 1.25, 1.5, 2]` (ED:159); the slider is continuous between 0.5
  and 2. Per session and per document tab; never written to the file.
- Zoom reflows: the page width in page units is recomputed from `viewport / zoom` (§2), 150 ms after the zoom value
  settles (ED:141, 181-191). All metrics in this document stay in page units and are multiplied by the zoom at paint.

---

## 2. The page

| Quantity | Value | Source |
|---|---|---|
| Side margin each side of the page (`pageMargin`) | 48 | ED:71 |
| Minimum page width (`minPageWidth`) | 320 (= 2 × 120 + 24) | ED:72 |
| Reading measure cap (`view/measureCap`) | default 760; lower bound 320; no upper bound in the setter | ED:54, 114; TH:83 |
| Cap on / off (`view/capMeasure`) | default **off** (the page is full width) | ED:55-57, 77 |
| Top padding above block 0 | 0 (the first block's own 6 px inset is all there is) | ED:6108, 6141; `Theme.dim.docTopPad` 28 is defined (TH:84) and used nowhere |
| Bottom padding below the last block | 0 (content height = Σ block heights) | ED:6114 |
| Page inset inside a canvas page card | 24 on every side | CD.h:59; `CanvasEditorFacade.qml:14` |
| Canvas page card measure | default 760; range 400 … 4000; card height min 400, max measure × √2 | CD.h:61-65, 291-293 |

Width rule (ED:75-76, 101-112):

```
availPageWidth  = max(320, floor(viewportWidth / zoom) − 2 × 48)
targetPageWidth = capMeasure ? min(availPageWidth, measureCap) : availPageWidth
pageWidth       = round(targetPageWidth)        committed 120 ms after the last change; never during a pointer gesture
```

Horizontal placement (ED:248, 259):

```
leftEdge = 48 + max(0, floor((viewportWidth / zoom − 2 × 48 − pageWidth) / 2))
```

- Capped with room: the page is centred. Full width: the bracket is 0, so the page starts 48 from the left and ends 48
  from the right.
- The edge is latched for the duration of any pointer gesture (ED:254-259).
- Content wider than the page (a wide table, a widened image) extends the scrollable span to the right:
  `contentSpan = max(viewportWidth, leftEdge + max(pageWidth, widest block, widest table + its shift) + 48)`
  (ED:124, 288-290). The page then scrolls horizontally; prose never moves.
- The viewport narrower than 320 + 96: the page stays 320 and the view scrolls horizontally.
- Background: one tone for the whole field, `bgAlt` (ED:976). The "sheet" (page + margins) is computed but not painted
  (ED:984-999), except by the focus fill (§10.4).
- The delegate of every block spans the whole field width (`x 0`, width `contentSpan`, BV:199-202); content is placed
  at `colLeft` inside it.

---

## 3. Vertical rhythm

There are no margins between blocks and nothing collapses. Each block is a box with fixed insets; boxes stack with no
gap. All of it is in one expression (BV:221-227) plus the text box's `y` (BV:586).

| Block | Box height | Content top inset | Bottom inset |
|---|---|---|---|
| Paragraph, heading, quote, list / task / numbered item | `12 + lineCount × lineH` | 6 | 6 |
| Code | `24 + lineCount × 18` | 12 | 12 |
| Divider | 30 (`12 + 18`) | — | — |
| Media | `12 + frame height (+ 22 video strip, + 40 PDF strip)` | 6 | 6 (below the strip) |
| Split row / table row (record) | its tallest lane (+ table pockets, §9) | 0 | 0 |

Resulting single-line heights: paragraph / quote / list 31; H1 57, H2 50, H3 44, H4 39, H5 36, H6 34; code 42.

- Empty paragraph: `lineCount` is clamped to 1 → 31 (BV:646).
- Paragraph gap: the distance between the last line box of one paragraph and the first of the next is 12 (6 + 6) plus
  the bottom slack of §1.3.
- Headings carry no extra space above or below, at any level; neither do list items (consecutive items are 12 apart,
  the same as paragraphs) nor a change of block type.
- Divider (BV:788-793): a 1 px line, colour `divider`, at `y = height / 2 − 1` (= 14), from `colLeft` across the whole
  `measure`; no side insets.
- Lanes: blocks stack from the lane's top with the same boxes; a short lane leaves empty space below (LI:14-15, 64-66).
- Model-side constants that are **estimates only** (used before a block is measured, replaced by the measured height):
  22 px a line + 16 pad for text, 40 + 16 for a heading, 24 for a divider (BM.cpp:62-64, 3388-3400), and the width-aware
  estimate at BM.cpp:5407-5429. They are not layout rules; note the divider estimate (24) disagrees with the rendered 30.

---

## 4. Lists

`depth` 0 … 8 (BM.h:149). `deco` is the left indent of the text box (BV:575-577, 581, 585):

| Type | `deco` | At depth 8 |
|---|---|---|
| Bullet, task | `22 + 24 × depth` | 214 |
| Numbered | `34 + 24 × depth` | 226 |
| Quote | 22 | — |

- Text box: `x = colLeft + deco`, `width = measure − deco`. Wrapped lines therefore hang: every line starts at the
  same x as the first.
- All markers are top-aligned to the text box (`y = te.y`, i.e. 6 from the block top); none is positioned by baseline.

| Marker | Geometry | Drawing | Source |
|---|---|---|---|
| Bullet | `x = colLeft + 6 + 24 × depth`, `y = te.y` | `•` at even depths, `◦` at odd; 14 px; `textMuted`; platform default font | BV:721-726 |
| Number | box `x = colLeft + 24 × depth`, width 28, right-aligned; 6 px gap to the text | `<n>.` decimal at every depth; 14 px body font, default weight (400); `textMuted` | BV:727-737 |
| Task box | 14 × 14 at `x = colLeft + 2 + 24 × depth`, `y = te.y + round((lineH − 14) / 2)` (= +3) | see below | BV:738-762 |

- Number value (BM.cpp:3476-3488): 1 + the count of preceding numbered items at the same depth, scanning upward;
  deeper list items of any kind are skipped; anything else (a non-list block, a shallower item, a bullet or task at the
  same depth) ends the run. Never stored. Numbers wider than the 28 px slot (four digits) overflow to the left.
- Task states (BV:742-761; radius `radiusSmall` = 3):

| State | Box | Mark | Text |
|---|---|---|---|
| 0 todo | transparent; 1.5 px border `textMuted` | — | normal |
| 1 doing | transparent; 1.5 px border `accent` | centred 7 × 2 bar, `accent` | normal |
| 2 done | fill `accent`, no border | `✓` 11 px bold, `textBright`, centred | `textMuted` + strikeout |

- Task box hit zone: x 0 … 20 from `colLeft` (does **not** add the depth indent), y −2 … `lineH` from the text top
  (ED:1940-1950).

---

## 5. Quote and code

### 5.1 Quote (BV:715-720, 577, 619)

| Quantity | Value |
|---|---|
| Bar | `x = colLeft + 4`, width 3, `y = te.y`, height = the text's natural height (`implicitHeight`, not the pinned height), square ends, colour `quoteBar` |
| Text inset | 22 from `colLeft` (15 from the bar's right edge) |
| Text | 14 px body font, upright, `textMuted` |
| Nesting | none (flattened in the model) |

### 5.2 Code block (BV:492-551, 582-591)

| Quantity | Value |
|---|---|
| Text box | `x = colLeft`, `y = 12`, `width = measure − 76` (`codeToolsW`) |
| Wrapping | wraps at a word, else anywhere (`TextEdit.Wrap`); no horizontal scroll (ruling 2026-10-23 in the source) |
| Background | the text box grown by 8 on every side and by a further 76 on the right: x `colLeft − 8` … `colLeft + measure + 8`, y 4 … `box height − 4` |
| Background fill | the syntax theme's editor background when it has alpha, else `codeBg` (BV:497). The theme is KSyntaxHighlighting's default dark theme (`CodeHighlighter.cpp:16-18`); its colour value is the library's — not determined here |
| Background radius / border | `radius` 6; 1 px `border` |
| Language chip | right edge at `colLeft + measure + 2`; `y = 7`; height 18; width = label + 24; radius 3; fill `surface` (`surfaceHover` on hover); 1 px `border`; label mono 11 px at x 7, `textMuted` (or `plain` in `textSubtle`); a 9 px caret-down icon 6 from the right |
| Copy chip | 22 × 18, 6 px left of the language chip, same chrome; 11 px copy icon |
| Chip hit slop | 2 px (ED:1961, 1974) |
| Inline styles | none inside code (no spans; the Markdown highlighter is detached, BV:668) |
| Spell underlines | none in code (BV:327) |

---

## 6. Inline styles

Text attributes are character formats on the text (IMH:93-143). Backgrounds are overlay rectangles built from the
per-line rects of §10.2 and drawn under the glyphs. **No overlay reserves width; none changes line height.**

| Style | Glyphs | Overlay | Advance change |
|---|---|---|---|
| Bold | weight 700 (ignored in headings) | — | the bold cut's own |
| Italic | italic cut | — | the italic cut's own |
| Underline, strikethrough | standard decorations | — | none |
| Inline code | Geist Mono at the block's pixel size (families only; size unchanged), colour `inlineCodeText` | fill `inlineCodeBg`, radius 3, `x − 2`, `width + 4`, the full line rect's height (BV:375-388) | the mono font's own |
| Link | underline + `accent` (code colour wins when both) | — | none |
| Text colour | the span's colour; wins over code / link / highlight contrast (IMH:140) | — | none |
| Highlight | auto-contrast: `#1c1c1c` when the highlight's luma > 150, else `#f0f0f0` (hard-coded, IMH:134-137); not applied under an explicit text colour or in a coloured cell | fill = the span's colour, exactly the line rects, no radius, no padding (BV:303-315) | none |
| Choice chip | the block's own text (the selected option's label) | fill = option colour at α 0.28, 1 px border at α 0.55, radius 3, `x − 4`, `width + 8`, line rect height; option without a colour uses `textMuted` (BV:408-428) | none — the 4 px pads ride under the neighbouring glyphs |
| Spelling / grammar | — | a 2 px-high dotted tile (2 on, 2 off) at `rect bottom − 3`, min width 4 (BV:348-358) | none |
| Typed Markdown markers | `accent` while typed (IMH:59-64) | — | none |

- Luma = `(299 R + 587 G + 114 B) / 1000` (IMH:135).
- A coloured table cell (`cellFg`) recolours its code and link runs too (IMH:51-52; BV:671).
- Paint order inside a block, back to front (BV z values and declaration order): code-block background (−1) →
  multi-block band (−0.5) → highlight → spelling → inline-code chips → choice chips → selection → text → opaque-row
  wash (1) → caret (2).
- A chip or code pill at a wrap point is split into one rect per visual line, each with its own rounded ends.

---

## 7. Media blocks

Placement: `x = colLeft`, `y = 6`; always left-aligned in its lane; no caption; no alignment attribute (BV:455-482).

Display width (BM.cpp:2693-2698; `laneW` = `measure`, i.e. the lane less a cell's 16):

| Case | Width |
|---|---|
| `dw` set in the descriptor | `dw`, verbatim — may exceed the lane and the page |
| PDF, no `dw` | `laneW` |
| Raster / video with intrinsic width `w`, no `dw` | `min(laneW, w)` (never upscaled) |
| No intrinsic dims | `laneW` |

Height (BM.cpp:3380-3386): `floor(width × h / w + 0.5)`; without intrinsic dims `laneW × aspect / 100` (the stored
aspect; the view's own fallback is 0.5, MB:65). The model's value is authoritative; the delegate never measures back.

| Quantity | Value | Source |
|---|---|---|
| `dw` on write | clamped 80 … 65535; ≤ 0 removes it | BM.cpp:5383-5384 |
| Drag minimum | 80 | ED:7883 |
| Aspect | locked; the image is fitted (`PreserveAspectFit`) | MB:184 |
| Video transport strip under the frame | 22 | ED:1071; BM.cpp:66 |
| PDF page-nav strip under the frame | 40 | ED:1072; BM.cpp:68 |
| File attachment chip | height 56, width `min(360, laneW)`, radius 6, fill `surfaceHover`, 1 px `border`; 24 px icon, name 14 px `text`, path 12 px `textSubtle`, 12 px side padding, 10 px spacing | BM.cpp:67; MB:106-136 |
| Loading / missing placeholder | fill `surfaceRecess`, radius 6, 1 px `border`, 14 px `textMuted` label | MB:217-228 |
| Video poster backdrop | `bgAlt`, radius 6, clipped; duration badge 8 from the bottom-right, black at α 0.6, 12 px `textBright` | MB:231-267 |
| PDF page | white (hard-coded), radius 6, 1 px `border`, clipped | MB:142-149 |
| Preview-format pill | 6 from the top-right, height 18, mono 10 px bold | MB:192-206 |
| Resize outline | 1 px `accent` around the frame, whole pixels | ED:7837-7845 |
| Resize handles | four corners; 9 × 9 `accent` squares, radius 3, centred on the corner; 19 × 19 hit area | ED:7846-7862 |
| Resize ghost | 1 px `accent` outline, radius 3, size label 10 from the top-right (black α 0.7, mono 12 px) | ED:7900-7931 |

- The image itself has square corners; only the placeholders and backdrops are rounded.
- Resize is proportional from any corner and anchored at the lane's left edge (ED:7810-7814); the model is written on
  release only.
- The page width, a lane ratio or a column width changing re-derives every media height (BM.cpp:2760-2766, 5431-5458).

---

## 8. Columns (split rows)

| Quantity | Value | Source |
|---|---|---|
| Gap between lanes (`kLaneGap`) | 24 | BM.h:201 |
| Minimum lane width (`kMinLaneWidth`) | 120 | BM.h:385 |
| Lanes per row | ≥ 2; lane index max 63 | BM.cpp:325; survey §2.2 |
| Nesting | none (no record inside a lane) | BM.cpp:314 |
| Ratios | one float per lane, sum 1, stored to 4 dp | BM.cpp:2881-2901; survey §2.2 |

Geometry (BM.cpp:2703-2714; the view recomputes the same formula, ED:301-316):

```
avail   = max(0, pageWidth − 24 × (n − 1))
w[k]    = avail × ratio[k]
x[0]    = 0;   x[k] = x[k−1] + w[k−1] + 24
divider[k] = x[k] + w[k] + 12              (the middle of the gap)
```

- Ratio clamp, applied **when ratios are written** (BM.cpp:2881-2901): `minShare = 120 / avail`; if
  `minShare × n ≥ 1` the lanes become equal; otherwise ratios are normalised, lanes under `minShare` are raised to it,
  and the wider lanes pay the deficit in proportion to their excess over `minShare`.
- Narrow page: nothing is re-clamped at layout time. Lanes always stay side by side (no stacking); with the page at its
  320 minimum two lanes are 148 each. A stored ratio can therefore render a lane under 120 on a page narrower than the
  one it was set on.
- Vertical alignment: lanes are top-aligned; the row's height is its tallest lane; no padding above or below the row
  (LI:78-82).
- Divider (BV:75-89): a 1 px line, colour `border`, centred in the gap (`x = divider − 0.5`), the full height of the
  row; on hover 2 px `accent` (`x = divider − 1`).
- Divider hit zone: the whole gap, ± 12 from its centre (ED:2126-2127). `⌘`/Ctrl held means "pull", never resize
  (ED:6605).
- Drag (ED:2148-2178, 7708-7720; BM.cpp:2962-2993): a 2 px `accent` preview line; the model is written on release. The
  x snaps to ¼, ⅓, ½, ⅔, ¾ **of the page width** within 8 px (ED:2142-2147, 2167). Neither neighbour goes under 120.
  Dividers at the same x (± 1 px) in directly adjacent split rows above and below move together as one chain; the
  chain stops at a table (BM.cpp:2930-2960).
- Pull-to-split (ED:775, 2132-2213; BV:95-104): an 8 px hot band inside each block's left and right edge, shown as a
  3 px `accent` bar at opacity 0.55; a pull shorter than 12 px is a click; the new divider snaps to the same fractions
  of the block's lane and keeps both sides ≥ 120.

---

## 9. Tables

A table row is a record whose lanes are the cells; a cell holds one or more ordinary blocks laid out as in §3.

### 9.1 Column widths

Per column, left to right from the table's left edge (BM.cpp:367-416):

```
manual  = head spec cols[c].w  (integer px)
width   = manual > 0 ? manual
        : column has a measurable block ? clamp(round(widest + 2 × 8 + 6), 48, 360)
        : 160
```

`widest` for a column = the maximum over every block in that column, in every row of the table, of:

- skipped: media blocks, dividers, blocks with empty text (BM.cpp:395);
- `natW` = the widest hard line (split on `\n`) of the block's plain text, measured with `QFontMetricsF` of
  `QFont("Geist")` at 14 px, **default weight (400)**, rounded to an integer (BM.cpp:369-378, 396). Inline styles,
  the block's type (a heading in a cell), its list indent, its chip padding and the rendered weight (500, or 700 in a
  header row) are not considered;
- plus 18 when the block is in the table's **first** row (BM.cpp:398) — the comment calls it the sort-glyph slot.

So auto width = widest line + 22, clamped 48 … 360; a longer line wraps inside the 360 column.

When it is recomputed:

| Event | Effect | Source |
|---|---|---|
| A cell block's text changes | that block's `natW` is dropped; all tables' geometry is marked dirty and rebuilt lazily on the next read | BM.cpp:2441-2447 |
| Undo / redo restores different text | `natW` dropped; geometry dirty | BM.cpp:3755, 3778 |
| A table is rebuilt (row / column insert, move, delete, sort, paste) | `natW` dropped for its blocks; geometry dirty | BM.cpp:618-625 |
| The head's column spec changes (width, kind, alignment, colour) | geometry dirty | BM.cpp:1346, 1401, 3564 |
| Table membership changes (a header set or cleared, rows joining) | geometry dirty | BM.cpp:357 |

Auto width follows the text live, on every keystroke in a cell. It does not depend on the page width.

Manual width: `setTableColumnWidth` clamps to 48 … 4000; ≤ 0 returns the column to auto (BM.cpp:2005-2028). Dragging
a column's right border (hit zone ± 4 px, every column including the last; ED:2117-2123) previews a 2 px `accent` line
across the table and never goes under 48 (ED:2163-2165); no fraction snaps. A column beyond the stored spec, or beyond
the geometry, is 160 (BM.h:1229-1231).

### 9.2 Cell and grid

| Quantity | Value | Source |
|---|---|---|
| Cell horizontal padding (`cellInset`) | 8 each side | BV:71; BM.cpp:2725-2731 |
| Cell vertical padding | none of its own: each block's 6 / 6 (12 / 12 for code) | BV:221-227 |
| Text wrap width in a cell | column width − 16 (less the block's own indent) | BV:52, 585 |
| Row height | the tallest cell = Σ of that cell's block heights; minimum 31 | LI:78-82 |
| Cell vertical alignment | top | LI:31-33 |
| Pocket above the first row / below the last row (`kTablePocket`) | 32 each; part of the row record's height | BM.h:253; BM.cpp:236-240 |
| Grid lines | 1 px, colour `border`: each column's right edge, each row's bottom edge, the table's left edge, the first row's top edge. Lines are drawn inside the cell rect | BV:152-170 |
| Column border on hover | 2 px `accent` | BV:152-156 |
| Header row | fill `surfaceHover`; text weight 700; otherwise as body | BV:121, 633 |
| Cell fill | cell colour > row colour > column colour (model), else header tint, else transparent | BV:116-121 |
| Cell text colour | `cellFg` when set (also recolours code and links) | BV:67-68, 616 |
| Alignment (`a` in the column spec) | 0 left, 1 centre, 2 right; applies to the text of every block in the column, header rows included | BV:66, 592-593; BM.cpp:874-877 |
| Row grip | 8 wide, full row height, at `tableX − 13`; fill `surfaceHover` / `accentMuted` live; 1 px `border` / `accent`; hit zone x −18 … −2 | BV:173-182; ED:2226 |
| Column grip | 8 high, the column's width, at 13 above the first row (inside the pocket); hit zone y −18 … −2 | BV:183-195; ED:2228 |
| Columns | max 63 | BM.cpp:2006 |

### 9.3 Horizontal placement and overflow

```
tableWidth = Σ column widths                        (independent of the page)
shift      = −min(round(max(0, (tableWidth − pageWidth) / 2)), max(0, leftEdge − 24))
tableX     = leftEdge + shift
```
(ED:274-279.)

- A table narrower than the page is left-aligned at the page's left edge; it is not stretched and not centred.
- A table wider than the page is centred under it by moving left by half its overhang, but never closer than 24 to
  the viewport's left; the rest overflows to the right, extends `contentSpan` and makes the whole page scroll
  horizontally. There is no per-table scroll box.
- Sticky header (ED:6296-6364): once the header rows pass the top of the view they stay pinned there until the table's
  bottom pushes them out. Frozen first column (ED:6201-6290): once the view scrolls sideways past the table's left
  edge, a mirror of column 0 is pinned at the viewport's left for each visible row, with the corner cell over both.
  Mirrors draw the cell's joined plain text at `x 8, y 6`, 14 px, bold in header rows.
- In a canvas page card a table starts at the card's inset and a wide one is clipped by the card
  (`CanvasEditorFacade.qml:54`).

### 9.4 Typed cells

| Kind | Stored as | Drawn as | Source |
|---|---|---|---|
| 0 text | any blocks | as §3–§6 | — |
| 1 choice | a paragraph holding one chip span | the chip pill of §6 behind the label; follows the column alignment because it is text | BV:395-428 |
| 2 check | a paragraph holding one chip | the text is hidden (opacity 0, still laid out); a 14 × 14 box at `x = colLeft + 2`, `y = te.y + 3`, the task box's three states (§4). It does not follow the column alignment | BV:398, 559, 763-787 |
| 3 timecode | a paragraph of normalised text | plain text; no special drawing in the block view | BV (no `colKind === 3` branch) |

Header-row cells are always kind 0 for drawing (BV:64-65).

---

## 10. Caret and selection

### 10.1 Caret (BV:697-713; ED:3524)

| Quantity | Value |
|---|---|
| Width | 2; its left edge is at the insertion x (not centred on it) |
| Height | the caret rect of the text layout (the natural line height of the font at that position); 18 when the layout returns 0 |
| y | the top of that line's rect |
| Colour | `accent` |
| Blink | toggles every 530 ms; not reset on typing (a free-running timer) |
| Hidden | while a selection exists; on media, dividers and records; when the view has no caret focus |

### 10.2 Selection rects inside one block (ED:1883-1898; BV:431-453)

For a range `[sp, ep)` with `rs`, `re` the caret rects at its ends and `lh = rs.height` (18 if 0):

| Case | Rects (in the text box's coordinates) |
|---|---|
| Same visual line (`|rs.y − re.y| < lh / 2`) | one: `x = rs.x`, `width = max(2, re.x − rs.x)`, height `lh` |
| First line of several | `rs.x` → the text box's right edge (`te.width`), not the last glyph |
| Middle lines | `0` → `te.width`, stepping y by `lh` |
| Last line | `0` → `re.x` (min width 2) |

- Height is the natural line height, so rects of consecutive lines touch but do not include the block's 6 px insets,
  the pinned-height slack, or the gap to the next block.
- The rects do not cover the marker column (bullet, number, task box, quote bar).
- An empty block inside a range has no rect.
- A hard line break inside a block (code, Shift-Enter text) is treated like a wrap.
- The same function produces the rects for highlights, inline-code pills, chips and spelling underlines.
- Colour: `selectionBg`, opaque, under the glyphs.

### 10.3 Across blocks, columns and cells

| Situation | Drawing | Source |
|---|---|---|
| Range inside one block | the rects only | BV:431-440 |
| Range over several blocks | each block draws its slice (first: from the start column to its end; middle: whole text; last: up to the end column) **plus** a band, `selectionBand` (α 0.16), over the block's full box height. The band runs the whole field width (page and margins) for a top-level block, and only the lane's measure for a block in a lane | BV:794-814 |
| Media in a range | `selectionWash` (α 0.45) over the frame rect | BV:815-828 |
| Divider in a range | `selectionWash`, 16 px high, centred on the line, the measure wide | BV:825-827 |
| Range whose ends are in different top-level rows | it widens to take any split row or table it reaches whole (`rowRangeSpan`, ED:2086-2100); such a row's lane blocks carry no band of their own — the record's full-field band covers them | BV:802-805 |
| Range inside one split row, across lanes | flat-order range: each block its slice + a lane-wide band | BV:810-812 |
| Cell rectangle in a table (drag across cells, or a grip-picked row / column set) | every cell in the rectangle is filled with opaque `selectionBg` over its whole cell rect (between the pockets); text rects and bands are suppressed for that table | BV:126-151, 436, 803 |
| Text range inside one cell | as inside one block | — |

### 10.4 Focus fill (ED:6144-6184)

The focused block's row is filled across the whole field: `bgAlt2` over the sheet (page + its two 48 margins, grown by
wide content), `bgAlt` beyond; height = the block's box, min 16. For a block in a lane or a cell the fill is the whole
split row / table row. It is the only "you are here" mark on media and dividers.

---

## 11. Colour tokens

One dark theme only (`Theme.qml`); there is no light theme and no system-appearance following, so there are no light
token values to map. Token names used by the page:

| Use | Token |
|---|---|
| Field / page background | `bgAlt` |
| Focus fill over the sheet | `bgAlt2` |
| Body text | `text` |
| Headings; done-task check mark; badge text | `textBright` |
| Quote text; markers (bullet, number, todo box border); done-task text; chip without a colour; chip labels | `textMuted` |
| Empty-language label; attachment path; small icons | `textSubtle` |
| Code block text before syntax colours | `codeText` |
| Code block fill (fallback) | `codeBg` |
| Inline code text / pill | `inlineCodeText` / `inlineCodeBg` |
| Caret; links; Markdown markers; doing / done task box; hover and drag lines; resize outline and handles; pull band | `accent` |
| Live grip fill | `accentMuted` |
| Quote bar | `quoteBar` |
| Divider line; placeholder frame while resizing | `divider` |
| Grid lines; lane dividers; code block border; chip borders; media borders | `border` |
| Header row fill; grips; chip hover; attachment chip | `surfaceHover` |
| Code chips' fill; frozen-column cell fill | `surface` |
| Image bed | `surfaceRecess` |
| Selection rects; cell-rectangle fill | `selectionBg` |
| Multi-block band | `selectionBand` |
| Media / divider in a range | `selectionWash` |
| Destructive menu-target tint | `error` |
| Spelling / grammar underline tiles | `squiggleSpelling` / `squiggleGrammar` |
| Preview-format pill | `affordanceIdle`, `borderStrong` |

Radii: `radiusSmall` 3 (chips, pills, task box, handles), `radius` 6 (code block, media frames, attachment chip)
(TH:76-78).

Hard-coded in the block view and its helpers (not tokens):

| Value | Where |
|---|---|
| `#1c1c1c` / `#f0f0f0` highlighted-glyph contrast colours | IMH:136-137 |
| Chip fill α 0.28, border α 0.55 | BV:415-417 |
| Cell wash α 0.14 (menu target / column drag), α 0.16 + 2 px border (file drop) | BV:147-149 |
| Pull band opacity 0.55 | BV:96-97 |
| `white` PDF page | MB:146 |
| Black at α 0.6 / 0.7 badges | MB:259; ED:7925 |
| The code block's fill and token colours (KSyntaxHighlighting default dark theme) | `CodeHighlighter.cpp:16-18` |
| User colours: text colour, highlight, cell / row / column fills, option colours | document data (hex) |

---

## 12. For the user to rule on

Layout bugs and inconsistencies found while reading. None is fixed here.

1. **Line pitch is not the line-height rule.** `round(size × 1.35)` sizes the box only; lines sit at the font's natural
   pitch (1.30 × for Geist) and the remainder collects under the last line (≈ 0.8 px a line for body text, ≈ 2 px a line
   for H1). A long paragraph gets a visibly larger gap below than a short one. Keep the look (natural pitch + bottom
   slack) or apply the rule as a true line height? (BV:637-646)
2. **Default measure.** The cap is off by default: a new install shows a full-width page; 760 applies only after
   "Full width" is turned off. (ED:55-57)
3. **No top or bottom page padding.** `docTopPad` (28) is defined and unused; the first block starts 6 px under the top
   of the view and the last block ends at the bottom edge. (TH:84; ED:6114)
4. **Code block background overhangs its column by 8 px on each side** (`colLeft − 8` … `colLeft + measure + 8`): past
   the page edge at top level, into the gap in a lane, and exactly onto the grid lines in a table cell. (BV:493-500)
5. **Code block vertical inset is 12, but the background starts 4 px inside the box**, so two code blocks in a row are
   8 px apart while their text is 24 apart. (BV:227, 495, 586)
6. **The chip slot is a fixed 76 px**; the language chip's width follows its label (label + 24) plus the 22 px copy
   chip and gaps, so a long language name runs over the text. (BV:492, 513-515, 543)
7. **Auto column width is measured in Regular 400 at 14 px, plain text only**, while cells render at 500 (700 in header
   rows) and may hold headings, list indents (22 +), inline code in the mono font and chips (± 4 px). Only the 6 px of
   slack covers the difference, so a column can wrap a line it was sized for. (BM.cpp:369-378, 409)
8. **The 18 px "sort glyph" reserve** is added to the first row's blocks, but no sort glyph is drawn anywhere; it is
   also applied to the first row only, not to every header row. (BM.cpp:398)
9. **A check cell ignores the column alignment** (the box stays at the left) and still reserves its hidden text's
   height. (BV:559, 763-765)
10. **Timecode cells have no distinct drawing** (proportional body font, not tabular or mono). (BV)
11. **A table narrower than the page is left-aligned, a wider one is centred** — two placement rules; and the centring
    limit depends on the window (24 from its left edge). (ED:274-279)
12. **Min lane width is enforced only when ratios are written**, not at layout: on a narrower page a lane can render
    under 120, and lanes never stack. (BM.cpp:2881-2901, 2703-2707)
13. **Divider drag snaps to fractions of the page width**, not to the divider positions that equal
    ratios give (`avail × f + 12` for two lanes): ½ coincides, but a ⅓ snap lands 4 px and a ¼ snap 6 px away from the
    true third / quarter split, and with three or more lanes the offsets grow. (ED:2167; BM.cpp:2923-2928)
14. **A media `dw` is honoured verbatim in a lane or a cell**: a width set at top level overflows a narrower lane and
    draws over its neighbour. (BM.cpp:2693-2694)
15. **The live video surface ignores `dw` and the cell inset**: it is placed at the lane's x with `min(lane, intrinsic)`
    width, so a resized or in-cell video's playing surface does not match its poster. (ED:6380-6388 vs MB:60-64)
16. **Markers are top-aligned, not baseline-aligned**, and the bullet and check glyphs use the platform default font
    while the text is Geist: their vertical fit depends on the OS font. The number marker is weight 400 beside
    weight-500 text. (BV:721-737, 755-761)
17. **Numbers past 999 overflow the 28 px slot to the left**; every depth is decimal (no letters / roman). (BV:727-736)
18. **The task box hit zone does not follow the depth indent** (always the first 20 px of the column), so a nested
    task's box is not where the click is tested. (ED:1946-1948 vs BV:740)
19. **The quote bar's height is the text's natural height**, the box is the pinned height: the bar ends short of the
    box by the slack of item 1. The comment above it still describes a serif quote; the code draws the body font.
    (BV:587-589, 715-719)
20. **Selection rects on the first and middle lines run to the text box's right edge**, not to the line's end, and in a
    centred / right-aligned cell they start at the box's left edge. (ED:1892-1894)
21. **Two selection languages**: a multi-block band spans the whole field for a top-level block but only the lane for a
    lane block; a table cell rectangle is opaque `selectionBg` with no text rects. (BV:794-814, 143-151)
22. **Overlay pills cannot reserve width**: a chip's ± 4 px and an inline-code pill's ± 2 px sit under adjacent glyphs,
    and at the start of a line they hang outside the text box (into the cell padding or the marker column).
    (BV:383-385, 420-425)
23. **Divider block estimate (24) ≠ rendered height (30)**; general text estimates (22 a line + 16) ≠ measured
    (19 a line + 12). Harmless once measured, but any consumer of the estimates (exports, first paint) is off.
    (BM.cpp:62-64, 3396; BV:226)
24. **The caret blink timer free-runs** (no reset on a keystroke or a move), so the caret can be invisible right after
    typing. (ED:3524)
25. **One theme only.** A light theme has no source values; they would be new design. (TH)
26. **Headings have no space above**; level is carried by size alone, and H6 (16 px, weight 300) is close to body
    (14 px, weight 500). (BV:615, 632)

## 13. Not determined

- The per-line pitch and caret-rect height Qt actually produces for Geist at each size after rounding (needs a
  measurement in the running app; the font's nominal value is 1.300 × size).
- The KSyntaxHighlighting default dark theme's background and token colours (library data, not in this repo's source).
- Whether the platform default font's `•` / `◦` / `✓` glyphs align with Geist's baseline on each OS.
- The right-side block rail's geometry (row numbers, drag handles): outside the page column; not read for this spec.
