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
5. VoiceOver reads by character, word and line and follows the caret. **DROPPED by the user, 2026-11-11** ("I am not worried about voice over"): not a
   condition of the verdict; Narrator on the Windows leg goes with it.
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

### Step 3f, the hand-written element INSIDE the stack (`spikes/s3-text`, `S9_NATIVE=1`; `src/native_ax.rs`; 2026-11-11)
**How it joins.** winit owns the NSView; AccessKit's subclassing adapter has put a dynamic subclass on it that answers
`accessibilityChildren`, `accessibilityFocusedUIElement` and `accessibilityHitTest:`. One more dynamic subclass goes
on top of AccessKit's: its three methods add our element to AccessKit's children and answer focus and hit-test with
it. No patch to winit or to AccessKit. The element is an `NSAccessibilityElement` subclass built at run time; it
answers from the document as one flat UTF-16 text over the blocks (block start offsets; geometry from parley's
cursor boxes); AccessKit keeps the window node. From outside (`axcheck`, 5,016 words): reads 70 of 70, selections 68
of 70 (the two across paragraphs — the scaffold's selection lives in one block), as fast as the Swift reference.

**What it took before Grammarly would use it — three things the reference window had without our knowing:**
1. **The whole text protocol must be DECLARED, not only the calls a tool makes.** With only value, selection (get /
   set), selected text, string / frame for range and the frame, Grammarly polled the focused element (1,607 times in
   one session) and never read its value. With `insertionPointLineNumber`, `lineForIndex:`, `rangeForLine:`,
   `rangeForIndex:`, `attributedStringForRange:`, `setSelectedText:` and `setValue:` also present it attached within
   two seconds. Which of the seven it looks for is NOT isolated (`S9_OMIT=<selector,…>` leaves single ones off).
   Announcing the element on window focus, the element's parent (the view or the window) and where its frame starts
   made no difference.
2. **The S3 scaffold pulled the view back to the caret on every draw**, so a scroll could not leave it — ours, fixed
   (follow the caret only after it moved).
3. **A scroll area around the text.** With the element directly under the window the user saw the underlines trail
   again ("the lag is back"), although the log shows correct service: the frame read within milliseconds of nearly
   every wheel step, the frame exactly on the drawn text (first character against the frame's top: 0 px), a full
   re-measure in 45 ms (the reference: 43 ms). With the text inside an `AXScrollArea` element (its frame the
   viewport; children the text and an `AXScrollBar` that posts `AXValueChanged` at every scroll step — the tree an
   `NSScrollView` shows) the user: "better"; the frame was read at every wheel step (29 of 29, 25 of 25, 19 of 19),
   and four corrections landed on the intended ranges. Grammarly never read the bar's value and read the area's frame
   once. WHY the scroll area matters is not established — the logs of the two runs differ little; the evidence is
   the user's eye, once each way.

**A view that scrolls WITHOUT a wheel event** (the app scrolling itself, 55 steps in a second; `S9_AUTOSCROLL=1`):
Grammarly read the frame 0–3 times during the scroll whatever was announced (nothing, `AXMoved` on the text,
`AXValueChanged` on a bar without a scroll area); it re-measured by itself within about 2.5 s after the scroll ended
in two runs of three. `AXLayoutChanged` and the scroll-area tree were not measured this way (the first lost focus
twice). So wheel scrolling is followed because Grammarly watches the wheel; keyboard paging, a dragged scroll bar and
jumps are followed late unless a selection or value change accompanies them. Open.

**Reading:** the route of the leaning works — a hand-written text element beside AccessKit, in the real window, serves
Grammarly: attach, underline, 14 corrections accepted by the user in this step, all on the intended ranges, undo not
re-tested. What the element must be is now a list: the full text protocol declared; a flat UTF-16 text; the frame of
the whole document moving with the scroll; a scroll area and scroll bar around it; no layout notification on scroll.

### Step 4, the OS checker called directly (`tools/s9/spellcheck.swift`, `NSSpellChecker`; macOS, 2026-11-11)
| Text | Found | Whole text | One paragraph |
|---|---|---|---|
| The flawed sample (911 units, about 20 deliberate mistakes) | 4 spelling, 4 grammar, 1 correction | 17 ms | 1.1 ms |
| Realistic, 5,016 words | 10 spelling, 9 grammar, 3 corrections | 132 ms | 1.1 ms |
| 50,000 words, a third of the sentences wrong | 966 spelling, 1,053 grammar, 8 corrections | 1,026 ms | 1.0 ms |

It finds misspellings (with guesses) and a thin layer of grammar: "Their are" → There, "should of" → have, a doubled
word, one agreement error. On the flawed sample it missed "Each one are", "We was", "the deadline were", "plane
prose", "more then", "disagrees", "were the word used to be" — the class of error the user keeps Grammarly for (on
the same sample the user accepted 9 Grammarly corrections in step 1). British spellings are flagged under "en"
("colour"): the language must follow the user's setting.
Two traps: `.grammar` and `.correction` asked WITHOUT `.spelling` return nothing; an `.orthography` option silences
the grammar pass.
**Reading:** as the guaranteed floor it is cheap enough to run per edited paragraph (1 ms) and on a whole note at
open (0.13 s at 5,000 words, off the main thread at 50,000) — red underlines for spelling without any third party.
It does not replace a writing tool. Apple's Writing Tools is step 4b.

### Step 3g, the two hands-off tests (the working window `S9_NATIVE=1 S9_SCROLLNOTE=area`, 5,016 words, 12–13 s frontmost each; 2026-11-11)
**Which of the seven added methods Grammarly requires** (`S9_OMIT`; "attached" = it read `accessibilityValue`):
| Left off | Attached | First value read | `frameForRange` calls |
|---|---|---|---|
| none (control) | yes | 0.94 s | 751 |
| all seven | **no** — it polled the focused element and read nothing | never | 0 |
| `insertionPointLineNumber` | yes | 0.09 s | 6,360 |
| `lineForIndex:` | yes | 0.09 s | 660 |
| `rangeForLine:` | yes | 0.09 s | 1,310 |
| `rangeForIndex:` | yes | 0.07 s | 656 |
| `attributedStringForRange:` | yes | 0.09 s | 4,416 |
| `setAccessibilitySelectedText:` | yes | 0.10 s | 660 |
| `setAccessibilityValue:` | **no** | never | 0 |
| the six others (only `setAccessibilityValue:` kept) | yes | 0.09 s | 656 |

**`setAccessibilityValue:` alone is the qualifier**: without it the element is not treated as editable text, with it
alone Grammarly attaches. It asks `respondsToSelector:` for it among the first ten selectors, right after
`accessibilityIsAttributeSettable:`; it never calls it (corrections arrive as selection + typed keys, step 1). The other
six are not needed to attach — they stay declared (a text element should answer them; other tools may ask). One run
each; the differing `frameForRange` counts are how far a checking pass got in 12 s, not a finding.

**A view that scrolls WITHOUT wheel events** (`S9_AUTOSCROLL=1`: 55 steps down during second 4, back up during second 7):
| Announced on scroll | Frame positions Grammarly read during the two scrolls | Re-measure of the flagged ranges |
|---|---|---|
| the scroll area + its bar's `AXValueChanged` (`area`), run 1 | 27 | starts at 8.01 s |
| the same, run 2 | 28 | starts at 8.00 s |
| `AXLayoutChanged` on the text (`layout`) | 9 | starts at 8.07 s |
| nothing | 0 | starts at 7.91 s |

With the scroll area Grammarly follows a self-scroll: about 13 frame reads a second while it moves (a wheel scroll gets
one per wheel step), against none with no announcement — so keyboard paging, a dragged bar and jumps are served by the
same tree, more coarsely than the wheel. `AXLayoutChanged` gets a third of that and is the announcement step 3e
dropped. In all four runs the re-measure began within a tenth of a second of the scroll's end; the 2.5 s lag of step
3f's two runs did not recur, so that number is not stable. What the eye sees in each case was NOT judged — nobody was
at the screen.

### Step 4b, Apple's Writing Tools on the custom view (`tools/s9/writingtools.swift`, `S9_WT=limited|complete`; macOS 27.2, 2026-11-11)
An `NSWritingToolsCoordinator` set on the reference window's view (a plain `NSView`), the system's menu items
(`NSMenuItem.writingToolsItems`) under Edit, every delegate call logged. The window asked for Proofread by itself
(`S9_WT_DO=Proofread`: it selects the first two paragraphs of the flawed sample and sends the menu item's action —
`showWritingTools:` with tag 1; no input is sent). `isWritingToolsAvailable` was true and the preferred behaviour was
granted in both modes.

| Mode | What the system called | How the text changed |
|---|---|---|
| Limited (the panel) | `contextsFor: userSelection`, once, 0.14 s after the action. Nothing else of the delegate. | Nothing by itself (22 s, run 3). In run 2 the whole selection arrived at 14.7 s as ONE `insertText:replacementRange:{0,294}` through `NSTextInputClient` — taken to be the panel's Replace button pressed by the user, who was at the screen; not confirmed. |
| Complete (inline) | `contextsFor`, state inactive → interactive streaming, previews and prepare / finish for the animations, then **10 `replaceRange:` calls within 2 ms, 0.8 s after the action**; afterwards bounding and underline paths for each changed range, and `selectRanges:` + a same-text `replaceRange:` as the suggestions were stepped through. | Through the delegate, one call per correction. |

Both modes corrected all ten mistakes of the two paragraphs, among them every one `NSSpellChecker` missed in step 4
("Each one are", "We was", "the deadline were", "plane prose", "should of"). The final text was right in both.
- A range in `replaceRange:` is in the context's text AS ALREADY CHANGED by the earlier calls of the same delivery
  ("are" was at 74, asked at 73 after "have" → "has"), so the delegate applies them in order against its live text.
- The context given was the paragraphs holding the selection (295 units for a 294-unit selection); the limited
  mode's replacement kept the context's trailing line break, which left one blank line too many — the replacement
  range and the context's edges need care.
- Limited mode needs one delegate method to answer and the ordinary input path to take the text; the other eight
  required methods were never called. Complete mode needs geometry (paths per range) and snapshots of the drawn
  text for its animations; the snapshots here were the view's own cached drawing, good enough for the calls to
  complete — how the animation LOOKS on a wgpu surface is not judged (this window draws with AppKit).
- Not tried: Rewrite and the tone actions, a long selection, the full-document scope, rich text results, the macOS 27
  grammar presentation (`requestsGrammarResultsFor…`, `showGrammarPresentation…`), and the coordinator on winit's
  view in the stack.

**Reading:** Writing Tools is a real second writing tool for a custom-drawn editor, with no third party: the limited
mode costs almost nothing (hand over the text, accept an insert), the complete mode is a bounded piece of work. With
`NSSpellChecker` (step 4) it is the floor on macOS when Grammarly is absent.

### Step 3h, undo of a typed burst in the stack (`S9_COALESCE`, default 300 ms; the user, 50,000 words, 2026-11-11)
Across the earlier logs the chunks of one correction arrive 1–86 ms apart; a person's keys 36–230 ms apart. Time
alone does not tell them apart, and need not: a pure insert at the caret the previous edit left, within the window,
joins that edit's undo step — the rule ordinary typing wants anyway.
First build (joining by the diffed position of each edit): the user accepted 8 corrections on the 50,000-word text,
all 8 on the intended ranges; ⌘Z took 5 back in one step and 3 in two or three. The misses were ours: the edit's
position was derived by diffing the text, and an insert that repeats its neighbour (" version" typed before
" against") diffs one character late, so it did not look contiguous. Second build: the step is keyed on the caret and
re-derived from the text before the burst. **NOT yet re-tried by the user.**
The user's own sentence (27 keys, gaps under 300 ms) became one step — a real editor breaks at words; not this
spike's question.

### Step 3i, key to picture at 50,000 words with Grammarly attached (the stack, `S9_NATIVE=1 S9_SCROLLNOTE=area`; the user typing; 2026-11-11)
The same session as step 3h (about 9 minutes, 951 blocks, 275k UTF-16 units, a third of the sentences flagged):
**key → present p50 2.51 ms, p95 3.12 ms (n = 209 keys)**; edit + relayout per key p50 160 µs, p95 234 µs; frame (scene
+ render + present, CPU side) p50 1.38 ms, p95 2.18 ms, max 3.54 ms — while Grammarly made 1,415,872 `frameForRange`
calls. Pass bar 4 (16.7 ms p95) is met with a wide margin. The figure is from the key event's arrival to the
`present` call, not to photons.
Scrolling at this size, the user: "it seemed decent enough as far as catching up — as fast as other apps". So the
moving frame and the scroll area hold on the 50,000-word worst case; step 3b's seconds of catch-up are gone.

### Step 5a, Windows: the stack as it is, read from outside (the box, Grammarly installed 2026-11-11; `S9_ALL_LINES=1`)
`spikes/s3-text` builds unchanged on Windows; AccessKit's adapter answers UI Automation. A PowerShell probe through
`System.Windows.Automation` in the desktop session (no input): the window's one descendant is a focused `Edit`
"Editor", framework "AccessKit", with `TextPattern` and `ValuePattern` (not read-only); the document range returns the
whole text; 20 of 20 six-character reads at offsets match; a five-character range returns one rectangle; selection is
"Single" with one range. `GetVisibleRanges` returns none.
**The user at the box: "there was no grammarly recognition yet"** — one look, at a window that closed itself after 75 s.
Grammarly's own page for integrators (found by search, not readable by fetch: HTTP 403) asks for the UI Automation
Text pattern, no parent of zero width or height, and the Scroll pattern where the field scrolls.
**Not known yet:** what Grammarly asks of a window on Windows and how a correction arrives. Nothing logs Grammarly's calls there yet (the reference-window step is not built).

### Step 5b, the Windows reference window (`spikes/s9-uia`; the box, Grammarly for Windows; the user accepting by hand; 2026-11-11)
A top-level HWND drawn by hand (fixed-pitch GDI text, our own wrap) whose `WM_GETOBJECT` returns ONE hand-written
provider: `IRawElementProviderSimple` (control type Document) + `ITextProvider` + `IValueProvider` +
`IScrollProvider`, ranges as `ITextRangeProvider` over flat UTF-16 offsets; every provider call and input message logged.

**Grammarly attached and corrected: 10 suggestions accepted by the user ("seems to be working"), each first edit on
exactly the selected range.**
- **Attach.** 0.3 s after launch the text was read once, paragraph by paragraph, and nothing followed for 52 s. The
  window had been opened by a scheduled task. When it lost and regained focus (our outside probe did that), Grammarly
  read the properties, took the Text and Scroll patterns and within 0.6 s asked `GetBoundingRectangles` for the ranges
  it flags on the Mac ({219,7} "speling", {199,2} "of", …). So it engages on a real focus change — which puts step 5a's
  "no recognition" in doubt: that window was also opened by a task and may never have been clicked into.
- **What it asks.** Properties: ControlType, Name, ClassName, AutomationId, FrameworkId, HasKeyboardFocus,
  IsControlElement, IsOffscreen, IsPassword, and ids 30020 / 30002. Patterns asked: Value, Scroll, Text, and (not
  given) Text2 (10024), LegacyIAccessible (10018), 10029. Reading: `DocumentRange`, then a walk with
  `MoveEndpointByUnit(Paragraph)` / `(Character)` and `GetText` per paragraph (the line break as its own read),
  `GetChildren`, `GetSelection`, `ExpandToEnclosingUnit(Paragraph)` at the caret; after each edit it re-reads the
  paragraphs. Geometry: `GetBoundingRectangles` per flagged range, reached by moving endpoints by character counts.
  Not called in this run: `Value` (ours only), `SetValue`, `RangeFromPoint`, `FindText`, `ScrollIntoView`, any `EM_` /
  `WM_SETTEXT` message, `WM_PASTE`.
- **How a correction arrives — the same model as macOS:** `ITextRangeProvider::Select()` on the span to change, about
  100 ms later the replacement TYPED: one `WM_KEYDOWN` with `VK_PACKET` (0xE7, scan 0) + one `WM_CHAR` per UTF-16
  unit, about 0.3 ms apart ("of been checked for speling" → 30 characters in 15 ms). Nothing is set through
  accessibility. One unit per event, so the undo joining of step 3h is REQUIRED on Windows (this window's own undo
  takes one character per Ctrl+Z; not exercised by the user).
- Not judged or not reached: where the underlines sat (not asked of the user in numbers), scrolling (the text fits
  the window), Ctrl+Z, which of the properties and patterns Grammarly requires (`S9_OFF`, `S9_CT=edit` exist), Narrator.

**Reading:** on Windows too a hand-written text provider over a flat UTF-16 text is found, read, underlined and
corrected by Grammarly, and the write path is again ordinary typing after a selection set from outside. Whether
AccessKit's own Windows adapter (step 5a) is enough is the open question — if it is, Windows needs no hand-written
element at all.

### Step 5c, why Grammarly ignores the AccessKit window on Windows (hands-off, the box, 2026-11-11)
The user clicked into the AccessKit window (step 5a's stack) a second time: "grammarly didn't show up".
**Attach test without eyes:** the reference window opened by a task, a focus change 8 s later (the outside probe's
start causes one), then the log is searched for `GetBoundingRectangles` on the ranges Grammarly flags. One run each.
| Shape of the reference window (`S9_CT`, `S9_OFF`, `S9_TREE`) | Geometry asked for flagged ranges |
|---|---|
| Document, Text + Value + Scroll (control, twice) | yes |
| control type Edit | yes |
| no Scroll pattern | yes |
| no Value pattern | yes |
| Edit, no Scroll | yes |
| no change / selection events | yes |
| the text as a CHILD fragment of the window (as a toolkit's tree) — also with Edit and no Scroll | yes |
| no class name, no automation id, framework id "AccessKit" | yes |
| **`ITextProvider::GetVisibleRanges` returns no range** | **no** |
| everything AccessKit's tree differs in at once (child, Edit, no Scroll, no ids, no visible range) | no |

**Grammarly for Windows does not engage with a text element that reports no visible range.** AccessKit's Windows
adapter (accesskit_windows 0.35.2, `node.rs`) answers `GetVisibleRanges` with a null array on purpose ("TBD: do we
need this?"). That is the one difference that matters among those tested.
The same walk Grammarly makes, replayed from outside on both windows (`s9-walk.ps1`): paragraphs and character moves
agree; the AccessKit window differs in the visible range (none) and in `Select()` — the selection read back right
after is empty there (the scaffold applies AccessKit's action request on its next event-loop turn, or not at all; not
separated). The sample file is checked out with CR LF on the box and the stack passes the CRs through — ours.

**Reading:** on Windows AccessKit is ONE function short of being found by Grammarly; whether its text ranges then
carry a correction (selection set from outside, then typed characters) is untested. The choice for Windows is a
patch to accesskit_windows (ours to carry or to send upstream) or the hand-written provider of step 5b.

### Step 5d, AccessKit with the one function patched (the box, hands-off, 2026-11-11)
`docs/spikes/s9/accesskit_windows-0.35.2-visible-ranges.patch`: `GetVisibleRanges` returns the document range (and
three calls are logged to `S9_AK_LOG`). Built on the box only, through `cargo build --config <file with
[patch.crates-io]>` against a patched copy of the crate — the repository's manifest and lock are untouched.
The attach test of step 5c on the AccessKit stack: with the patch switched off (`S9_AK_VISIBLE=0`) 5
`GetBoundingRectangles` calls in the run, with it on **49** — Grammarly engages. One run each.
Whether a correction lands through AccessKit's ranges (a `Select()` that becomes an action request, then typed
characters) needs the user's hands.

## Verdict
(pending)
