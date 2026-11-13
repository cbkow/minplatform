# S11 — can the note's own layout be paginated onto paper as a PDF?

**Question (§21.3; canon rows 328 ruling 10, 329):** minNotes' PDF export should look like the note. Can the layout the
editor already makes (`mn-layout`: parley's shaped lines, one box per block) be cut into pages and written as a VECTOR
PDF whose text can be selected and searched - instead of a second, separate layout written for paper, as the Qt app
had?
**Pass bar:** a synthetic note comes out as Letter pages with (1) no line of text cut by a page's edge, (2) the text
selectable - read back whole by another program, accents and typographic quotes included, (3) the fonts embedded, and
(4) a note of 20 pages written in **under 2 s**.
**Time box:** one session, 2026-11-13. **Status: VERDICT - PASSES all four (2026-11-13).**

## What the spike was
`spikes/s11-notepdf` (dies with this verdict): a synthetic note (headings, paragraphs, nested bullets, tasks, a quote,
code, a table) is laid out by `NoteLayout::build` 640 units wide with a paper `Style` (dark text), every line of every
text block becomes an "atom" with its top and bottom in the laid-out note, the atoms are sorted by their top, and a
page is closed when the next atom would cross its bottom (the new page starts at that atom's top). Each line's glyph
runs are written with krilla 0.8.2's `draw_glyphs` - parley's own glyph ids and advances, cluster by cluster, each
glyph carrying the byte range of the block's text it stands for - in the run's own font (the bundled Geist), scaled so
that 640 units are the page's column (Letter, 0.75 in margins: body text comes out at 10.2 pt).
Read back with poppler (`pdftotext`, `pdffonts`, `pdfinfo`, `pdftoppm`). Apple M5 Max, release build.

## Findings

| | 192 blocks | 596 blocks |
|---|---|---|
| Lines / pages | 371 / 12 | 1,211 / 37 |
| Layout, ms | 30 | 36 |
| Pagination, ms | 1 | 5 |
| All, to the bytes, ms | 35 | 49 |
| File, KB | 39 | 89 |
| Lines crossing a page's edge | 0 | 0 |

- **Text:** `pdftotext` gives every paragraph back in reading order (67 of 67 body paragraphs in the 12-page file), with
  `Café, naïve, “quotes”` intact; cells of the table read as their own lines.
- **Fonts:** Geist-Regular and GeistMono-Regular, embedded, subset, with a Unicode map (`pdffonts`: emb yes, sub yes,
  uni yes).
- **Look:** page 1 drawn by poppler matches the note's own line breaks and spacing (headings, indents of nested list
  items, the code block's face).
- **Speed:** 37 pages in 49 ms - forty times inside the bar. Pictures will dominate a real export, not text.

## What the spike did NOT do (the production work)
- What the editor's PAINTER draws and the layout does not hold: list markers and numbers, task boxes, a quote's bar, a
  code block's ground, a table's grid and fills, highlights and chips behind text, underlines and strike-throughs.
  Each is a rectangle or a short path at a place the layout already gives; they need a paper version of `paint.rs`'s
  block pass, not new measurement.
- Pictures, PDF pages, video stills and their drawings (`export::Source` with `Mode::Paper` already gives them as PNG /
  JPEG; krilla takes both). A picture is an atom of its own and moves whole to the next page.
- Links as clickable annotations (krilla has link annotations; a run's rectangle is known).
- A table wider than the page: ruled in row 328 to be scaled to the page in the first version.
- Side-by-side things (columns, a table's row) are cut wherever a line ends, each lane by itself - correct for text,
  but a row can straddle a page. Keeping a short row whole is a refinement.
- A4 when the system is metric (the Qt app's rule); page numbers; the note's name as a running head.

## Verdict and recommendation
**Pass.** Build the PDF export on the editor's own layout with krilla - the same library QCView's notes export uses, so
no new dependency in the family. It gives a PDF that looks like the note for the price of a paper painter, with real,
searchable text and embedded Geist, and it is fast enough that no progress display is needed for text alone.
The alternative - a second layout written for paper, as the Qt app had (its column-band packer for tables) - is not
needed.
