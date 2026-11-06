# S3 — text: the editor long pole (parley + vello + IME + AccessKit)

**Question (§21.3, §3, §16):** can a from-scratch editor on parley/vello handle real text input — IME on both
OSes, accessibility, selection/clipboard/undo — at minNotes' numbers, so `min-ui`'s text node is viable?
**Pass bar (vs P0):** CJK IME commits correctly on macOS (marked text) and Windows (TSF); VoiceOver and
Narrator read the text and caret; per-key ≤ 1 ms p95 in paragraphs (minNotes 2.0.6: 0.98 ms); key→swap p50
≤ 3.4 ms; 60 fps at 10k lines.
**Time box:** runs the whole of Phase 1 (starts day 1). **Status: IN PROGRESS — step 1 (AccessKit) DONE 2026-11-06 on macOS; step 2 (IME) started; Windows owed.**

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

## Step 1 — AccessKit (macOS, 2026-11-06)
`ax.rs`: a full tree per update (the `EventLoopProxy` adapter path requires it): Window → `MultilineTextInput` (value,
bounds, `SetTextSelection` / `ReplaceSelectedText` / `Focus` actions, the live `TextSelection` as (line node, character
index)) → one `TextRun` per laid-out parley line (value, UTF-8 character lengths, per-character x positions and widths
from caret geometry, bounds from the line metrics; `word_lengths` does not exist in accesskit 0.25). The adapter is created
before the window is shown (`with_visible(false)` → adapter → `set_visible(true)`), `process_event` runs before every
window event, `user_event` handles `InitialTreeRequested` and `ActionRequested`.
**Verified through the macOS AX API (`tools/s3/axdump.swift`, the same tree VoiceOver reads):** the window exposes an
`AXTextArea` titled "Editor" under the content group; `AXValue` = the whole text (187 characters), `AXSelectedText` /
`AXSelectedTextRange` = the editor's selection with correct character counting across a multi-byte "—" ({3,7} for bytes
3–12), `AXInsertionPointLineNumber`, `AXLineForIndex`, `AXRangeForLine`, `AXStringForRange` and `AXBoundsForRange` (screen
coordinates) all answer; **setting `AXSelectedTextRange` {20,5} from outside reaches the app as a `SetTextSelection`
action and selects " Shif" in parley** (the VoiceOver caret-move path). A tree build costs 186 µs p50 / 249 max for this
2-paragraph text (per-character caret geometry; a 10k-line document will need visible-range runs — step 3). The
AppleScript `System Events` route is too slow for a walk (minutes) and chokes on the window buttons; the AX C API is instant.

## Step 2 — IME (started 2026-11-06; needs a real keyboard)
The editor logs every `Ime` event (`ime: Preedit/Commit`). Two automated attempts: AppleScript `keystroke` and a CGEvent
injector with autorepeat (`tools/s3/keys.swift`). Holding "e" produced "eeee…" with NO popover and NO `Ime` event through
either — the press-and-hold popover never engaged for synthetic input, which does not separate "winit's NSView lacks the
press-and-hold hooks" from "the system ignores synthetic repeats for it". This Mac has only U.S. + the character palette
enabled, so a CJK composition could not be driven at all. **Owed, by hand:** (1) hold a vowel in the spike window and pick an
accent; (2) enable Japanese (Hiragana) in System Settings → Keyboard → Input Sources, type `nihongo`, space, return — the
log shows the `Preedit` sequence and the `Commit`, the window shows the candidate list at the caret; (3) dictation. Then
Windows TSF on the box. The marked-text plumbing itself (`set_compose` / `ime_cursor_area`) is in place from the scaffold.

## Remaining plan
1. ~~AccessKit~~ DONE (step 1 above).
2. IME proof — by hand (step 2 above); Windows TSF on the box.
3. Clipboard (plain + the block MIME later), undo as ops (§17's op model), a 10k-line document, the timing
   harness (per-key and key→swap, mirroring minNotes' probes) → numbers → verdict here.
