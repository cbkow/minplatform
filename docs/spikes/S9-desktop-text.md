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

Undo (the user, 12 × ⌘Z): every chunk came back — but a correction typed in three 16-unit chunks took three undos. **The
editor must coalesce a burst of typed chunks into one undo step.** The user: "everything seemed to work perfectly".

### Step 2, the stack (macOS, 2026-11-10): `spikes/s3-text` (parley + vello, AccessKit 0.25 through accesskit_winit) as `S9Stack.app`
Four defects stood between the S3 scaffold and a writing tool; all four were ours, three of one family:
1. **A crash on the first outside query**: an empty paragraph's line reads the range 0..1 → a slice past the end (`ax.rs`).
2. **Every rectangle half-size and half-offset on a 2x screen**: AccessKit's macOS adapter reads BACKING pixels; the tree
   was built in points. One `Affine::scale(scale)` on the root (a 12-character range: 48 px → 96 px, as drawn).
3. **The runs did not add up to the value**: the document value joins blocks with "\n", the per-line runs left it out, so
   every offset after the first paragraph was off by one per paragraph. The block's last line now carries the joiner.
4. **A selection that crossed a line wrap was REPORTED as starting at the next line** (anchor and focus were derived from
   one line). Grammarly reads the selection back before it types; in the first run one correction then landed 28
   characters late ("of been checked for spe" → " but nobody did. We wer"). With anchor and focus placed independently the
   same sequence landed on bytes 46..69, as meant. (Why a wrong read-back moved the NEXT target by exactly the caret's
   offset was not established; the symptom went with the fix.)

**`tools/s9/axcheck.swift`** — an outside check through the AX API, no typing: `AXStringForRange` against the value, and a
set of `AXSelectedTextRange` read back as `AXSelectedText`. On the fixed build, 302 positions: reads 0 mismatches;
selections 11 mismatches, ALL of them selections spanning two paragraphs (the scaffold's selection lives in one block).

**Second run by the user on the fixed build: 9 corrections, 9 on the intended range** (the log names each range's text
before the typed chunks arrive). Grammarly's own behaviour was the same as against the reference window: it attached,
underlined, selected, typed in chunks of ≤ 16.

**Reading:** the stack CAN serve Grammarly through AccessKit, read side and write side. But the reference window (one
flat UTF-16 text, geometry straight from the layout) was right the first time, and every defect here was a conversion
between "offset in the document" and "run + character index" or between coordinate spaces. Cross-block selection, rich
spans and off-screen blocks (the scaffold exposes visible lines only, so offsets would not add up in a long document)
are still ahead on this route. Leaning: the document's text element is written directly against the platform's text
protocol over a flat text + block map; AccessKit serves the rest of the interface. Not yet a verdict.

### Step 3a, long documents, probed from outside (`axcheck.swift`; 2026-11-10; synthetic text, 95 and 950 paragraphs)
| Window | Document | Reads wrong | Selections wrong | 69 probes took | Tree build per update |
|---|---|---|---|---|---|
| Stack, visible lines only (the S3 design) | 5,000 words | **68 of 69** (nil off screen) | **67 of 69** | 3.3 s | — |
| Stack, every line exposed (`S9_ALL_LINES=1`) | 5,000 words (315 lines) | 0 | 2 (both across paragraphs) | 3.3 s | **27–31 ms** |
| Stack, every line exposed | 50,000 words (3,066 lines, 275k units) | 0 | 0 | **20.2 s** | **272 ms** |
| Reference window (flat text, TextKit geometry) | 50,000 words | 0 | 0 | 3.4 s | none (no tree) |

**Reading:** exposing only what is on screen cannot work — an outside tool addresses the document by offset and the
offsets stop adding up. Exposing everything is correct but costs O(document) twice in this stack: the scaffold rebuilds
the whole tree on every edit and selection change (272 ms at 50,000 words against a 16.7 ms bar; a per-block cache and
incremental updates would cut that), and AccessKit's consumer side walks the runs for each query (about 245 ms per
probe more than the reference at this size — and Grammarly made 2,000 geometry calls in a minute on a 900-character
text). The second cost is inside the library, not ours to cache.

### Step 3b, the user on the reference window at 50,000 words (2026-11-10)
The user: typing "seem ok", "the right words are changing", "a noticeable lag between scrolling and then grammarly
catching up to the viewport". The log: **515,953 `frameForRange` calls** in the session. After EVERY scroll Grammarly
re-asks the geometry of every flagged range in the WHOLE document (offsets 0..274,955), 24,000–130,000 calls over about
8 s, answered at 25–30 thousand a second — the wait is the number of round trips, not our answers. It never asks
`visibleCharacterRange`. The text was hostile on purpose (about a third of its sentences are wrong: thousands of flags).

**Answering off-screen ranges with an empty rectangle (`S9_OFFSCREEN=zero`) did not help** (the user: "as slow or
slower — a matter of seconds"): about 110,000 calls in the first 30 s, 9,600 of them for on-screen ranges; the catch-up
still took 9 s. The cost is one visit per flagged range, wherever it is.

**Reading:** Grammarly's catch-up after a scroll (and presumably after any layout change) grows with the number of
flagged ranges in the element it is attached to. The only lever on our side is HOW MUCH TEXT ONE ELEMENT HOLDS.
Typing and corrections are unaffected at this size.

### Step 3c, one text element per paragraph (`S9_MODE=blocks`; the reference window, 5,000 words, 96 elements; 2026-11-10)
The user's instinct ("I do believe each block is its own run"), tested. From outside: the application's focused element
is an `AXTextArea` "Block N" (one paragraph), its parent an `AXGroup` with 96 children. Grammarly followed the caret:
on each change of paragraph it read that paragraph's value (200–330 characters) and asked geometry for it alone — 277
`frameForRange` calls in a whole session, against 516,000 on the one-element 50,000-word run. Corrections landed on the
right ranges (local offsets mapped back to the document).
**The cost, which the user felt at once:** Grammarly checks only the paragraph the caret is in; the rest of the document
shows nothing. The user: "so i understand the limitations here and maybe our block model is not helpful at all for what
we want in this app. time to reassess?"
**Answer given, and where the day ended:** the block model (storage, editing, merge) and the SHAPE SHOWN TO THE DESKTOP
are independent — the same blocks can be described as one field, as several, or as one per paragraph. Per-paragraph is
the most fragmented shape and the wrong trade for a writing tool. The whole-note shape was only ever tried on a
worst case (50,000 words, a third of the sentences wrong).

**NEXT, agreed with the user for the following session:** the whole-note shape on a REALISTIC text —
`docs/spikes/s9/out/real-5k.txt` (5,016 words, 95 paragraphs, 15 flawed sentences; regenerate with the generator in
this log's history if the ignored folder is gone): `open -n target/spikes/s9/S9Text.app --env S9_LOG=<file> --env
S9_FILE=<that file>` (no `S9_MODE`). The user scrolls and says whether Grammarly keeps up; the log gives the call
counts after each scroll. If it holds: one field for the whole note, split only when a note is enormous. Then the
middle shape (one field per run of text between media blocks) if it does not.

### Step 3d, the whole note as one field on a realistic text (the reference window, 5,016 words, no `S9_MODE`; 2026-11-11)
The user, 110 s, 7 scrolls, 7 corrections accepted by hand (all 7 on the intended range, two of them typed in two
chunks): "it lags, but maybe this is just how it has to be".
The log: 65,758 `frameForRange` calls, but in PASSES — one pass over every flagged range of the note is about 460
calls and is answered in about 40 ms. While the view scrolls Grammarly makes a pass every 0.1–0.45 s; its last pass
comes **0.50–0.52 s after the scroll stops** on six of the seven scrolls (0.93 s on the seventh), whether the scroll
cost 2,300 calls or 10,500.
**Reading:** at this size the lag is Grammarly's own cadence (a settle timer of about half a second), not the number
of round trips and not our answers. The field shape cannot shorten it. This window has no blocks at all (one flat
text), so none of the lag is the block model's.
**Control, owed by the user:** the same text in TextEdit (`NSTextView`). Same trail there = Grammarly's behaviour
everywhere, and the whole-note shape stands. Tighter there = a native text view tells Grammarly something we do not
(a scroll or layout notification); find it in the reference window.

**The control (the user, TextEdit, the same text):** clearly tighter there. So the trail was ours.

### Step 3e, what an `NSTextView` tells the outside while it scrolls, and the same from our window (2026-11-11)
`tools/s9/axwatch.swift` — an outside watcher: every accessibility notification of the application, and the attributes
a tool could poll (the text area's frame, its visible range, the scroll bar's value, the bounds of range {0,1}). It
sends no input; `--drive` steps the vertical scroll bar's `AXValue`. Forty steps on each window:
| | TextEdit (`NSTextView`) | The reference window as built on 2026-11-10 |
|---|---|---|
| The text area's frame | the WHOLE document (656×4979); its position moves at every step | the visible part (843×600); never changes |
| Notifications | `AXValueChanged` on the scroll bar, nothing on the text | the same, plus `AXLayoutChanged` on the text area at every step |

Two switches make the reference window behave as the text view does: `S9_FRAME=full` (frame = the document view's
bounds on screen) and `S9_SCROLLNOTE=off` (no `AXLayoutChanged` on scroll). **The user, both on, the same 5,016-word
text: "this is much better."** The log: Grammarly reads `accessibilityFrame` once per scroll event (9 of 9, 7 of 7,
15 of 15, 25 of 26) and now sees it move; `frameForRange` falls to 450–2,700 calls per scroll (2,300–15,000 before),
and a checking pass still comes about 0.5 s after the scroll stops — no longer what the eye waits for. Three more
corrections, all on the intended range.
**Reading:** Grammarly follows a scroll by the MOVEMENT OF THE ELEMENT'S FRAME and slides its underlines; where the
frame stands still it can only re-measure every flagged range. The element must report the frame of the whole
document, in screen coordinates, moving with the scroll — and must not announce a scroll as a layout change. (The two
switches were not separated: both are what the native view does, and both are adopted.) Step 3b's seconds of catch-up
at 50,000 words were measured with the still frame and are to be re-read with the moving one.
**Consequence for the shape:** the whole note as ONE field holds on a realistic note. Proposed to the user as the ruling.

## Verdict
(pending)
