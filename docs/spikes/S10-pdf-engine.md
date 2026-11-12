# S10 — which engine draws minNotes' PDF pages and gives their text?

**Question (§21.3; canon rows 310, 313):** the PDF block needs pages drawn at any size AND the text of a page with the
place of every character (selection, copy, search), the same on macOS and Windows, beside GPL-3 in a public
repository. Two candidates were left by the survey of 2026-11-12: **pdfium** (through `pdfium-render`, a bundled
dynamic library) and **hayro** (pure Rust). Does hayro do both well enough on the user's own files to spare the bundled
library?
**Pass bar (for hayro, against pdfium as the reference):** on the user's files - (1) every file opens and its first and
a middle page are drawn; (2) the first page differs from pdfium's by a mean of **less than 4** of 255 per colour value;
(3) a page at 2000 px wide takes **under 100 ms**; (4) the characters with a place number **within 2 %** of pdfium's.
**Time box:** one session, 2026-11-12. **Status: VERDICT - hayro PASSES all four (2026-11-12). The choice between the
two is the user's; the recommendation is at the end.**

## What the spike was
`spikes/s10-pdf` (dies with this verdict): for each file and engine, open, count pages, draw the first and a middle page
at 2000 px wide, write PNGs, and collect the first page's characters. pdfium 158.0.8125.0 (the prebuilt macOS arm64
library, loaded from a folder outside the repository) through `pdfium-render` 0.9.5; hayro 0.9.0. hayro has no call
for text: its interpreter hands every glyph run to a `Device`, and a small device of our own (about 15 lines in the
spike) keeps each glyph's character (`Glyph::as_unicode`) and place (`PositionedGlyph::transform`).
Files: 15 PDFs (presentation decks, 4 to 61 MB, 14 to 210 pages, full-page pictures with text over them) and 5
Illustrator files (0.4 to 0.7 MB, 1 to 4 artboards, saved with PDF compatibility), read where they are on a network
share. Apple M5 Max. They are named here by their order only.

## Findings

| | pdfium | hayro |
|---|---|---|
| Files opened and drawn | 20 of 20 | 20 of 20 |
| Open, ms (min / median / max) | 3 / 20 / 66 | 0 / 32 / 147 |
| First page at 2000 px, ms (max) | 7 | 5 |
| A middle page at 2000 px, ms (median / max) | 22 / 49 | 20 / 33 |
| Characters with a place, first page (the 15 PDFs) | 37 to 1284 | 37 to 1280: equal on 3 files, 1 to 5 fewer on 12 (at most 1.2 %) |
| Glyphs with no character | - | 0 |
| First page against pdfium's: mean difference per colour value | - | 0.05 to 1.92 of 255 |
| First page against pdfium's: pixels more than 24 apart | - | 0.03 to 3.0 % |
| The Illustrator files | drawn, no text (outlined artwork) | the same |

- Two pairs of pages were also looked at side by side (a deck's picture page with small type; an Illustrator icon):
  no difference to the eye. The measured differences sit on the edges of type and shapes (the two smooth edges
  differently) and in a one-pixel difference of the pictures' widths (hayro truncates 1999.x, pdfium rounds).
- The few characters pdfium has more of were not traced; pdfium is known to add spaces and line ends of its own
  between words, which hayro's glyphs do not carry. Whatever the engine, words and lines must be made from the
  glyphs' places.
- hayro opens slower on the larger files (it was given the whole file read into memory; pdfium reads from the file).

## Not tested
- Windows (pdfium: its DLL; hayro: nothing to ship). An Illustrator file saved WITHOUT PDF compatibility. Encrypted
  files. Pages with knockout groups, CMYK blending or odd blend modes, which hayro's own notes list as incomplete.
  Right-to-left and vertical text. Middle pages were drawn and timed but compared only by eye on one file.
- hayro is 0.x and changes its interface about monthly; pdfium-render needs its library pinned to the API it binds.

## Verdict
Both engines do the job on these files; hayro passes every bar set for it. What separates them is not quality here:

| | pdfium | hayro |
|---|---|---|
| Ships as | a dynamic library per system and architecture (about 8 MB each), re-signed for notarisation, inside the MSIX | compiled in |
| Text with places | a ready, mature call | a device of our own over its interpreter (small) |
| Known gaps | none that matter here | some blending features; young |
| Threads | one at a time (a global lock) | no lock of its own |

**Recommendation: hayro**, with the drawing kept behind one small interface in `apps/mn` (open, page count, page
size, draw a page at a size, a page's characters with places) so that pdfium can take its place without touching the
block if a real file turns up that hayro draws wrong.
