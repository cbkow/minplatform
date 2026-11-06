# S3 — text: the editor long pole (parley + vello + IME + AccessKit)

**Question (§21.3, §3, §16):** can a from-scratch editor on parley/vello handle real text input — IME on both
OSes, accessibility, selection/clipboard/undo — at minNotes' numbers, so `min-ui`'s text node is viable?
**Pass bar (vs P0; the IME line REWRITTEN 2026-11-06 — the user types neither CJK nor needs it in the apps):** composition
commits correctly on macOS — dead keys (Option+e, e → é: marked text then commit), press-and-hold accents, dictation — and
through TSF on Windows; CJK rides the same `setMarkedText` / `firstRectForCharacterRange` path and is chased only on a user
report; VoiceOver and Narrator read the text and caret; per-key ≤ 1 ms p95 in paragraphs (minNotes 2.0.6: 0.98 ms); key→swap p50
≤ 3.4 ms; 60 fps at 10k lines.
**Time box:** runs the whole of Phase 1 (starts day 1). **Status: IN PROGRESS — step 1 (AccessKit) DONE 2026-11-06; step 2 (IME): press-and-hold DONE with a vendored winit patch (the user confirmed the symptom on a real keyboard, the fix is verified by injected events), CJK + dictation owed by hand; Windows owed.**

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

## Step 2 — IME: press-and-hold (2026-11-06) — a winit 0.30 gap, FIXED by a vendored patch
**The symptom (the user, real keyboard; reproduced by `tools/s3/keys.swift`, a CGEvent injector with autorepeat — TextEdit
gives "kéy" from the same events, so the injection is valid):** holding "e" in a winit window prints "eeee…"; with a partial
fix the accent popover appears but the repeats still print and the digit that picks the accent prints "2".
**Root cause, five winit 0.30.13 behaviours in `platform_impl/macos/view.rs`:** (1) `selectedRange` returns `{NSNotFound, 0}`
— AppKit never shows the popover then (GTK's macOS backend found the same, gtk!532); (2) `attributedSubstringForProposedRange`
returns nil — the popover needs the held character; (3) repeating letter keyDowns are delivered to the app as text although
AppKit consumed them for the popover; (4) the accent arrives as `insertText:` with a REPLACEMENT RANGE of length 1 and no marked
text, which winit drops (its own TODO: "Use _replacement_range"); (5) a commit that arrives outside a keyDown (the popover's own
window answered the digit) must not set `ImeState::Committed`, or the next ordinary key is swallowed as "IME-related".
**The patch (`vendor/winit`, `[patch.crates-io]` in the workspace, every change marked "minplatform PATCH"):** track the last
typed character; `selectedRange` = a caret after it; `attributedSubstringForProposedRange` = that character; a repeating
letter → `interpretKeyEvents` only; the digit while a letter is held → swallowed (the popover answers it asynchronously); an
`insertText:` with a replacement range → a synthetic Backspace key event + `Ime::Commit` (the spike's stand-in for the
`Ime::Replace` event winit lacks); Escape clears the held letter. The zapfast #723 swizzle was the recipe. The app also sets
`ApplePressAndHoldEnabled` in its own `NSUserDefaults` domain (an unbundled binary; not isolated as necessary).
**Result:** `k`, hold `e`, `2`, `y`, a typed `é`, Escape → final text `kéyé`, the log shows the synthetic Backspace and
`Commit("é")`, and the key after the commit flows. Two harness flakes to know: the injected keys go to whatever is frontmost
(`set frontmost` races — a run that shows only "k" is the harness), and the Japanese-input test cannot be driven here.
**For the canon:** winit's macOS text-input client is incomplete for a real editor; the port carries this patch (or its own
NSView, S5's question) until upstream has `Ime::Replace` + the press-and-hold answers. **Composition confirmed by the user on a real keyboard (2026-11-06):** Option+e, e prints é in the editor — the dead key
is marked text (`Preedit`), the second keystroke commits — the same path a CJK composition uses. **Owed by hand:** dictation;
then Windows TSF on the box. CJK: on report only (the bar above).

## Remaining plan
1. ~~AccessKit~~ DONE (step 1 above).
2. IME: ~~press-and-hold, dead-key composition~~ DONE on macOS; dictation by hand; Windows TSF on the box.
3. Clipboard (plain + the block MIME later), undo as ops (§17's op model), a 10k-line document, the timing
   harness (per-key and key→swap, mirroring minNotes' probes) → numbers → verdict here.
