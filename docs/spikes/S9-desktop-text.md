# S9 — desktop text: can a custom-drawn block editor serve Grammarly-class tools?

**Question (canon rows 268, 273; Phase 3):** what, exactly, must a custom-drawn editor answer for a third-party writing
tool (Grammarly Desktop above all) and the OS text services to find its text, underline it in the right place and apply
a correction — and can our stack (winit + wgpu + parley, AccessKit or a hand-written adapter) answer it for a document
that is many blocks?

**Why now:** the answer decides the editor's shape (one text element per document vs. per block; AccessKit vs. our own
adapter) and so comes before the editor is designed. How Grammarly APPLIES a correction is undocumented, and nobody has
publicly shown it working in an AccessKit application (research, 2026-11-10).

**Pass bar (numbers):**
1. Grammarly's widget appears within **2 s** of the editor taking focus.
2. Underlines sit within **2 px** of their words, and still do after a scroll and after a window resize.
3. **20 accepted suggestions in a row** replace exactly the intended range: **0** misplaced, undo restores each.
4. With Grammarly attached, key → picture stays within **one 60 Hz frame (16.7 ms)** p95 on a **50,000-word** document.
5. VoiceOver reads by character, word and line and follows the caret.
Windows (Narrator, the same four with UI Automation) is a second leg, when Grammarly is installed on the box.

**Fail =** the widget never appears, or corrections land at the wrong offset, in our stack AND in the hand-written
adapter. Then: one element per block; and if that fails too, the OS spellcheckers called by the app are the guaranteed path.

**Time box:** one week from 2026-11-10. **Status: step 1 started 2026-11-10 (macOS; Grammarly Desktop 1.207.0).**

## Steps
1. **A native reference window, not our toolkit** (`tools/s9/`, Swift): a custom `NSView` (NOT `NSTextView` — TextKit is
   used for layout and geometry only) that implements the accessibility text protocol and `NSTextInputClient`, with a
   logger on every call. Run Grammarly against it. Reading the log answers: which attributes it asks for, how often,
   and — the unknown — how a correction arrives (a set selection + `setAccessibilitySelectedText`, a paste, typed text,
   `insertText:replacementRange:`, or a whole-value set). Switches turn single answers off to find the minimum.
2. The same content through the stack (accesskit_winit, one multi-line text node, parley's real glyph positions);
   diff against step 1 and patch what the log proves matters.
3. One element for the document vs. one per block, on 50 blocks and on 50,000 words; media blocks as placeholder lines.
4. The OS spellcheckers called directly (`NSSpellChecker`; `ISpellChecker` on the box) and Apple's Writing Tools in its
   limited mode — the baseline that does not depend on any third party.
5. Windows: the same reference window with a UI Automation provider, then the stack.

## Log
(the reference window writes to `docs/spikes/s9/out/`, ignored by git)

### Step 1, first run (macOS, 2026-11-10; Grammarly Desktop 1.207.0; `tools/s9/`, the user accepting suggestions by hand)
**Grammarly attached to an unknown bundle id with a custom `NSView` at once** — no allow-list stood in the way. It read the
value 49 ms after launch and asked for its first underline geometry at 0.6 s.

**What it calls (≈ 55 s, 9 corrections accepted):** `accessibilityFrameForRange` 2043 × · `selectedTextRange` 19 ·
`selectedText` 17 · `value` 11 · `setSelectedTextRange` 9 · `accessibilityFrame` 5. **Never called:** `stringForRange`,
`attributedStringForRange`, `numberOfCharacters`, `visibleCharacterRange`, `rangeForLine`, `lineForIndex`,
`rangeForPosition`, `insertionPointLineNumber`, `setSelectedText`, `setValue`, the shared-text pair. Ranges are UTF-16.

**Underlines:** `frameForRange` for the first character and for the whole flagged range; for a range that wraps it
BISECTS the length (94 → 47 → 23 → 11 → 17 → 20 → 19 → 18 …) to find where the line breaks, then asks per line. That is
where the 2043 calls come from, so `frameForRange` must be cheap and exact. It re-asks after the window moves
(`accessibilityFrame` follows each move).

**How a correction is applied — the unknown, answered:**
1. `setAccessibilitySelectedTextRange` over the span to change (it selects the smallest differing span, mid-word if
   need be: "W[as in a hurry and the the deadline were]" → "ere in a hurry, and the deadline was");
2. ~100 ms later it reads `selectedText` back, to check the selection is what it meant;
3. it TYPES the replacement as synthetic key events — `keyDown` with key code 0 and the text in the event's characters,
   **up to 16 UTF-16 units per event** — which arrive through the input context as `insertText:` with no replacement
   range, replacing the selection;
4. it re-reads `value` and `selectedTextRange`.
A deletion (a doubled word) is the same with the selection replaced by a shorter text. All 9 landed on the right range.

**Consequences:** (a) the write path is our ORDINARY typing path — nothing has to be set through accessibility, so
AccessKit's missing `setSelectedText` does not matter for Grammarly; (b) a key event carrying SEVERAL characters must be
inserted whole (the Qt minNotes accepts only `event.text.length === 1`, so every correction would have been dropped);
(c) the selection set from outside must be honoured exactly, in UTF-16 units, before the keys arrive; (d) the minimum
read surface is value + selectedTextRange (get / set) + selectedText + frameForRange + frame.

## Verdict
(pending)
