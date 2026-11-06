# S3 — text: the editor long pole (parley + vello + IME + AccessKit)

**Question (§21.3, §3, §16):** can a from-scratch editor on parley/vello handle real text input — IME on both
OSes, accessibility, selection/clipboard/undo — at minNotes' numbers, so `min-ui`'s text node is viable?
**Pass bar (vs P0):** CJK IME commits correctly on macOS (marked text) and Windows (TSF); VoiceOver and
Narrator read the text and caret; per-key ≤ 1 ms p95 in paragraphs (minNotes 2.0.6: 0.98 ms); key→swap p50
≤ 3.4 ms; 60 fps at 10k lines.
**Time box:** runs the whole of Phase 1 (starts day 1). **Status: SCAFFOLD (2026-11-06) — the spike has
NOT started.**

## What the scaffold is
`spikes/s3-text`: a parley 0.12 `PlainEditor<Brush>` drawn by vello 0.11 in a winit 0.30 window (vello's
`util::RenderContext` → Rgba8 target → `TextureBlitter` → swapchain). Typing, Backspace/Delete, arrows,
Home/End, Shift-selection, click and Shift-click to place/extend, IME preedit (`set_compose`) and commit,
and `set_ime_cursor_area` so the candidate window follows the caret. `cargo run -p s3-text` · Esc quits
and prints the final text.

## Facts the scaffold already settled
- parley 0.12 ships the editor (`parley::PlainEditor` + `PlainEditorDriver`, ~70 operations incl. compose,
  word/line motion, selection geometry, `ime_cursor_area`); vello draws its glyph runs directly
  (`draw_glyphs` + `positioned_glyphs`, variable-font coords via `to_bits`). No Masonry needed (§16).
- `accesskit_winit` 0.34 pins winit 0.30 (see S1 notes).

## Remaining plan
1. AccessKit: `accesskit_winit::Adapter` + a two-node tree (document + text run) with the caret/selection
   reported; VoiceOver walk.
2. IME proof: macOS Japanese/Chinese input with candidate window placement; the press-and-hold accent
   popover; dictation. Windows TSF on the box.
3. Clipboard (plain + the block MIME later), undo as ops (§17's op model), a 10k-line document, the timing
   harness (per-key and key→swap, mirroring minNotes' probes) → numbers → verdict here.
