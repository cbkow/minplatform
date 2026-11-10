# Spec: Open in minNotes · the "A / B level mixer" · note markers on the timeline

Read 2026-11-10, read-only. 2.x = `QCView-Player` at tag `v2.7.3` (all 2.x line numbers are `git show v2.7.3:<file>`).
3.0 = `minplatform` at `8b01ff0`. minNotes = its checkout on branch `min-paths` (`d3f1e8e`); the code cited is also on `main`.
Each claim is marked **[code]** (read in the source at the line given) or **[inferred]**.

---

## A. "Open in minNotes"

### A.1 What 2.x does

**Where the command lives [code]**
- One place only: a `FlatButton` in the Notes panel's **Export notes** panel, on the bottom row beside Cancel —
  `src/ui/qml/NotesPanel.qml:517-541`.
- `visible: WindowManager.activeNotesAreStream` (`:524`) — it exists **only for a live / stream item**. A file's notes never
  show it.
- `enabled: WindowManager.minNotesAvailable && notesList.length > 0` (`:528-530`).
- No menu item, no shortcut, no key-filter entry: `git grep -i minnotes v2.7.3 -- src` finds the command nowhere else
  (no hit in `Main.qml`, no key map).
- Strings, context `NotesPanel` (verbatim):
  - label `Open in minNotes` (`:526`)
  - tooltip, installed: `Open this stream's notes folder in minNotes` (`:532`)
  - tooltip, absent: `minNotes is not installed` (`:533`)
- One more string, context `qcv::WindowManager`: toast `minNotes is not installed` (`src/window/window_manager.cpp:9292`).
- User doc: `docs/annotations.md:69-71` ("Sending notes to minNotes" — "For a stream item, the Export panel gains Open in
  minNotes. It hands the notes folder to minNotes 1.0.3 or later…").

**What it hands over [code]** — `WindowManager::openNotesInMinNotes()`, `window_manager.cpp:9275-9300`
- The **stream notes folder** itself (`m_annotationManager->streamDir()`), i.e.
  `<project folder>/.qcview/streams/<sanitised name>__<id8>/` holding `notes.json` + `images/`. Not an export, not the
  `notes.json` path, not a media file.
- As a URL: `QUrl::fromLocalFile(absolute dir)` with the scheme swapped to `minnotes` → `minnotes:///<abs path>`
  (`:9283-9284`), opened with `QDesktopServices::openUrl` (`:9286`).
- Returns false silently (log line only) when the active item has no stream folder (`:9278-9282`).

**How it finds / launches minNotes [code]** — `minNotesLocation()`, `window_manager.cpp:9246-9267`
- macOS: `/Applications/minNotes.app`, then `~/Applications/minNotes.app` (path probe only).
- Windows: `%ProgramFiles%/minNotes/minNotes.exe`; else `HKEY_LOCAL_MACHINE\Software\Classes\minnotes` holding
  `URL Protocol` → the sentinel `"minnotes:"`.
- Other OSes: empty (never available).
- `minNotesAvailable()` caches the answer in a `static const bool` — evaluated **once per process** (`:9269-9273`);
  installing minNotes while QCView runs does not enable the button until restart.
- Launch order: (1) the URL through the OS scheme handler; (2) if `openUrl` fails, start the app with the folder as an
  argument — macOS `/usr/bin/open -a <app> <dir>`, Windows `<exe> <dir>` (`:9287-9299`).

**When minNotes is not installed [code]**
- The button is drawn disabled with the tooltip `minNotes is not installed` (`NotesPanel.qml:528-533`).
- If the call is reached anyway and both paths fail: toast `minNotes is not installed` (`window_manager.cpp:9291-9293`).

**2.x gaps seen on the way (for the owner to rule on, not fixed silently)**
1. Windows detection misses a **Store / MSIX minNotes**: minNotes' MSIX registers the scheme through
   `uap:Protocol Name="minnotes"` (`minNotes/packaging/windows/msix/AppxManifest.xml.in:71-74`), which is neither
   `%ProgramFiles%\minNotes\minNotes.exe` nor `HKLM\Software\Classes\minnotes` → the button stays greyed although the
   URL would open. **[code for both halves; the conclusion inferred — not run]**
2. The Inno setup writes the scheme under `HKA` (`minNotes/packaging/windows/minNotes.iss:124-127`) — a per-user install
   lands in HKCU, which 2.x does not look at. **[code / inferred as above]**
3. macOS detection is two fixed paths; a copy elsewhere is "not installed". **[code]**
4. The docs call the timeline note mark a "diamond" (`docs/annotations.md:19`); the code draws a pennant (see C). Unrelated
   to A, listed once here.

### A.2 minNotes' side of the contract [code]

- Scheme registration: macOS `CFBundleURLSchemes` = `minnotes` (`minNotes/packaging/macos/Info.plist.in:89-100`); Windows
  Inno `Software\Classes\minnotes` + `shell\open\command "<exe>" "%1"` (`minNotes.iss:124-127`); MSIX `uap:Protocol`
  (`AppxManifest.xml.in:71-74`).
- Delivery: macOS `QFileOpenEvent` captured by `MinNotesApplication::event` — a non-`file` URL is forwarded as the URL
  string, queued until the engine is ready (`app/MinNotesApplication.cpp:11-30`). Windows: a single-instance gate (lock
  file + local socket `minNotes-singleton`) forwards a second launch's arguments to the running instance
  (`app/main.cpp:150-175`).
- `resolveAndOpen` (`app/main.cpp:44-68`): strips `minnotes:` → local path; **a directory holding `notes.json`, or a
  `notes.json` file itself, → `DocumentManager::openStreamFolder(dir)`**; anything else → `openTab(path)` (a `.mnd`).
- `openStreamFolder` (`app/core/DocumentManager.cpp:147-177`, contract comment `DocumentManager.h:68-73`):
  - refuses with "Not a stream notes folder (no notes.json): %1" when the folder has none;
  - document = `<folder>/<name>.mnd`, `<name>` = the folder name with the `__<id8>` suffix cut;
  - first time: a new tab, `VideoNotesModel::insertNotesFromDir(model, dir, 0)` inserts every note as blocks (captured frame
    as an image, strokes as ink, text under it), then `saveAs` **inside the stream folder**; later times: opens that `.mnd`;
  - the tab remembers the folder (`activeStreamDir`), and the toolbar's stream chip offers the notes QCView adds later
    (`app/qml/MinNotes/App/ChromeToolbar.qml:206-213`).
- History: landed in minNotes `205da7d` (2026-10-13, "QCView stream notes (plan C)"), tags `v1.0.3` and every `v2.0.x`
  contain it. 2.x's button landed the same day (QCView `593ed862`, plan B3).

**Consequence worth knowing [inferred from the code above]:** minNotes writes `<name>.mnd` into the stream folder, so
that folder is not QCView-only. 3.0's stream-folder code must keep ignoring files it does not own (it reads `notes.json`
and `images/` only — `crates/qcv-notes/src/io.rs:29-58`).

### A.3 The original design reason (outside the code)

- `~/.claude/plans/nifty-scribbling-walrus.md` (approved 2026-10-12): Part B = notes on streams, step **B3** = "stream-aware
  exports, Open in minNotes"; Part C = minNotes' stream mode. The plan's draft URL was `minnotes://stream?path=…`
  (line 138); what shipped is `minnotes:///<dir>` (the code above) — the code wins.
- Memory `…QCBridgeAE/memory/stream-annotations-plan.md:26`: the owner chose minNotes for **reordering / editing stream
  notes instead of adding a drag handle to QCView's panel**. That is why the button is stream-only: a file's notes already
  reach minNotes through the media itself (minNotes reads `<media_dir>/.qcview/<name>/notes.json` for any video block),
  a stream has no media file to drop into minNotes, so the folder hand-off is the only door.

### A.4 The canon

- Row 254 (`docs/rust-wgpu-port.md:2552`), 2026-11-10: "The same day the user ruled four 2.x features BACK INTO THE PLAN,
  each to be specified from QCView's code before it is built: the Scene LUT / CDL controls and Export LUT (…), **Open in
  minNotes (minNotes is the next app ported)**, a live source as a dual-view side (…). The A / B level mixer is to be
  checked against 2.x first (the user does not remember it as a live feature)."
- Row 233 (`:2531`), 2026-11-09, last sentence: "**Owed: "Open in minNotes" (packaging).**"
- Handoff `docs/phase2/handoff-2026-11-09.md:94`, item 4 (Packaging) lists `"Open in minNotes"`.
- Row 215 (`:2513`): the sidecar is carried "byte-for-byte (minNotes reads it — 1:1 forever)".

### A.5 What 3.0 already has

- **The folder, byte-for-byte [code]:** `crates/qcv-notes/src/io.rs:29-58` — `streams_root`, `stream_folder_name`
  (`<sanitised name>__<id8>`), `find_stream_folder`, `stream_dir`, `stream_header` (stream / stream_id / stream_url). Row 233
  records it built and walked on both OSes. `crates/qcv-notes/src/lib.rs:1-5`: written as Qt's indented JSON, "No schema
  extensions, ever". So **3.0's stream folder is still exactly what `openStreamFolder` reads** — nothing to convert.
  (Verified by reading both sides; not verified by opening a 3.0-written folder in minNotes — that is the walk.)
- **The folder at run time [code]:** `apps/qcv/src/annotate.rs:74` `dir()` returns the stream folder when the active notes
  are a stream's.
- **The link builder [code]:** `crates/min-paths/src/links.rs:34-37` `minnotes_link(path)` → `minnotes:///…` (tests at
  `:93`). Not yet a dependency of the notes drawer path, but `min-paths` is already used by `os_actions.rs`.
- **OS open [code]:** `apps/qcv/src/os_actions.rs:6-14` `open(path)` — macOS `open <arg>` accepts a URL as is. The Windows
  arm runs `explorer "<to_native(path)>"`, which rewrites separators: **not safe for a URL**; a separate `open_url` is
  needed on Windows (ShellExecuteW "open", or `explorer.exe "minnotes:///…"` unmapped). **[inferred — not run]**
- **The surface [code]:** `apps/qcv/src/notes_drawer.rs:57,123,199,290` — 3.0's Export notes is a **tile that opens a
  menu** (Markdown folder… / HTML (single file)… / PDF… / DOCX…), not 2.x's panel with a Cancel row. Stream mode is known
  to the drawer (`stream: Option<StreamMode>`, `:59`, `:83`).
- **Strings [code]:** `Open in minNotes`, its two tooltips and the toast are already in the 3.0 catalogs with QCView's
  contexts and translations (`apps/qcv/i18n/qcview_*.po`, e.g. `qcview_de.po:2501-2503`) — no new drafts needed if the
  wording is kept.
- **Toast [code]:** `apps/qcv/src/main.rs:267`.
- **Missing:** any detection of minNotes; any `Cmd` for it; a URL-safe open on Windows.

### A.6 Build outline (small: about half a day + one walk per OS)

1. `os_actions.rs`: `open_url(url)` (macOS `open`, Windows ShellExecuteW / unmapped `explorer`), and
   `minnotes_available()` per ruling A-2.
2. `notes_drawer.rs`: in stream mode the export menu gains a separator + `Open in minNotes`
   (`trc("NotesPanel", …)`), disabled when unavailable or when there are no notes; the disabled row's tooltip =
   `minNotes is not installed`; a new `Cmd::OpenInMinNotes`.
3. `main.rs`: on the command — `annot.dir()` → `min_paths::links::minnotes_link` → `open_url`; on failure the process
   fallback (macOS `open -a minNotes <dir>`, Windows `<exe> <dir>` when an exe path is known); else the toast
   (`trc("qcv::WindowManager", "minNotes is not installed")`).
4. Tests: the link for a stream folder with spaces / non-ASCII on both OS forms (`links.rs` already covers the codec);
   the menu shows the row only in stream mode.
5. Walk (release build, both OSes): a stream with two notes → Open in minNotes → the `.mnd` appears beside `notes.json`
   with both notes; add a third note in QCView → minNotes' stream chip offers it; minNotes absent → greyed + tooltip.

### A.7 Rulings needed

| # | Question | [proposed] default |
|---|---|---|
| A-1 | Where the command sits, since 3.0 has an export **menu**, not 2.x's panel | **[proposed]** a last row of the Export notes menu, under a separator, stream items only — the nearest thing to 2.x's "bottom row of the Export panel". No shortcut (2.x has none) |
| A-2 | Detection | **[proposed]** ask the OS who handles `minnotes:` (macOS `NSWorkspace URLForApplicationToOpenURL`; Windows `AssocQueryString` for the scheme, which sees HKCU, HKLM and packaged apps), keep 2.x's path probes as the fallback. A written divergence: fixes 2.x gaps 1–3. Re-checked each time the menu opens, not once per process |
| A-3 | Stream items only, or files too | **[proposed]** stream only, as 2.x. For a file's sidecar folder minNotes would treat it as a stream folder and write a `.mnd` inside `.qcview/<media>/` — not a designed path. Files wait for the minNotes port |
| A-4 | Not installed | **[proposed]** 2.x's: the row greyed with `minNotes is not installed`; the toast only if the launch itself fails. No download link |
| A-5 | The row 233 / handoff wording files this under "packaging" | **[proposed]** it is an app feature with no packaging work on QCView's side (QCView registers nothing; macOS needs no `LSApplicationQueriesSchemes` for `open`). Move it out of the packaging list when built |

---

## B. The "A / B level mixer"

### Verdict: **documented but never built.** The owner's memory is right.

**The only claim [code]:** `docs/dualview.md:39` — "Per-side mute and an A/B level mixer live in the Inspector's Audio
routing controls. … The right side is muted by default." The sentence arrived in commit `74795be4` (v2.0.1, 2026-05-17),
the same commit that added `src/dual/dual_audio_mixer.{h,cpp}`; the docs before it read "Toggle both sources together to
mix all audio, or solo one side. The right side is muted by default."

**The code has never had a per-side level [code]:**
- `src/dual/dual_audio_mixer.h:5-7` (v2.7.3): "independent per-side mutes, and a master volume/mute mirrored from the
  transport bar **(no per-side gains — sides mix at unity)**".
- The class's whole per-side surface: `Q_PROPERTY(bool mutedA …)`, `mutedB` (`:59-60`), `setRoutingModeA/B` (`:189-190`),
  per-side peaks. No gain / volume / level / balance setter.
- The mix is a plain sum: `output[i] += sideBuf[i]` (`dual_audio_mixer.cpp:738-740`); a muted side is skipped
  (`:735`), still consuming so unmute is in sync.
- At v2.0.1 the header's comment said "independent mute / volume controls per side" (`74795be4:src/dual/dual_audio_mixer.h:6`)
  but that version's API was the same two mutes — no volume member, setter or property. The comment was corrected in
  `68b24716` (2026-07-07, which added the **master** volume / mute to dual).
- `git log --all -S` for `gainA`, `setGainA`, `sideGain`, `m_gainA`, `balanceAB`: no commit. `volumeA`: two commits
  (v0.7.1 / v0.7.6), both false positives inside a vendored tag library's headers. `level mixer`: only `74795be4`, the
  docs line.
- QML: `git grep` for mixer / crossfade / side volume / levelA / listen in `src/ui/qml` at v2.7.3 finds no control. The
  only dual audio controls are the two mute pills below.
- Memories / plans: nothing describing a per-side level control (the QCView-Player memories that mention
  `DualAudioMixer` are about the servo, pops and races).

**The docs line is wrong twice:** there is no level mixer, and the per-side mutes are **not in the Inspector** — they
are on the viewport's source bars.

### What 2.x actually does for dual-view audio [code]

- Both sides are decoded; each is resampled to 48 kHz stereo float and **summed at unity**
  (`dual_audio_mixer.cpp:716-744`).
- Which side is heard = **per-side mute**, defaults **A on, B muted** (`dual_audio_mixer.h:262`, `:267`:
  `m_mutedA{false}`, `m_mutedB{true}`). Both may be on at once (a straight sum), or both off.
- Chosen with a **speaker pill on each side's source bar in the viewport overlay** — `src/ui/qml/ViewportOverlay.qml:262-318`
  (A) and `:505-555` (B); shown only when that side has audio (`:270-272`); click toggles `audio.mutedA` / `mutedB`
  (`:305-309`, `:545-549`); tooltips `A audio muted — click to unmute` / `A audio on — click to mute` (and B's). No
  shortcut.
- Master volume and mute (transport bar, `M`) apply to the sum (`dual_audio_mixer.h:124-130`).
- A side inside a timeline gap is silenced (`:103-113`). Each side keeps its own channel routing mode (`:185-190`). A
  separate A/V sync offset is stored for dual (`docs/dualview.md:41`). Live sides are heard only with live playdown on
  (`dual_audio_mixer.cpp:736`).

### What 3.0 does today [code]

- The same model, already ported under row 211: `crates/qcv-player/src/dual.rs:46-47` ("QCView's per-side mutes
  (`DualAudioMixer`: A heard, B muted by default)"), `set_side_muted` / `side_muted` (`:317-318`), a side's gain is 0 or 1
  only — its mute, or silence where it has no picture (`:345-347`); test `b_starts_muted_and_a_side_is_silent_in_its_gap`
  (`:433`).
- The pills: `apps/qcv/src/dual_controls.rs:99-104` (drawn), `:126` (`Cmd::ToggleSideMute`), `:141` (QCView's tooltips);
  wired at `apps/qcv/src/main.rs:726`, `:907-908`. 3.0 places them on the side chips of the status-strip dual controls
  rather than on viewport source bars.
- Master volume / mute go to both sides (`main.rs:1479`, `dual.rs:305`); the dual sync offset is its own setting
  (`settings.rs:19`, `main.rs:1520`).
- One internal difference: each side has its own audio player following the master clock instead of one two-decoder mixer
  (`dual.rs:313`); the audible result is the same sum. **[code for the structure; "same result" inferred]**

### Ruling needed

| # | Question | [proposed] default |
|---|---|---|
| B-1 | The "A / B level mixer" | **[proposed]** close it as **never a feature**: nothing to port, 3.0 already has 2.x's real behaviour. Record a dated canon row; fix the sentence in the docs for 3.0 ("a speaker pill on each side; A on, B muted by default; both on = mixed") |
| B-2 | The wrong sentence in the 2.x docs (`docs/dualview.md:39`) | **[proposed]** a QCView docs bug to list for the owner; correct it only on a branch if 2.x docs are still published |

Build size: **none**.

---

## C. Note markers on the timeline

### C.1 What 2.x does [code]

- **Yes.** `src/ui/qml/TimelinePanel.qml:2277-2374` — "Annotation note pins": one per entry of `WindowManager.notesList`,
  placed at `timeToX(note.timestampSeconds)` on the **bottom rail** of the timeline track area, scrolling and zooming
  with the content (`x: -root.scrollX`), `z: 2` (above clips / cache / in-out, below the playhead).
- **Drawing:** a 1-px stem 7 px tall + a 9 × 8 px upward **pennant (triangle)**.
  - open note: filled `Theme.noteMarker` = `#b48ef0` (violet; `Theme.qml:90-92`: "stays clear of the green in-point,
    amber out-point, and blue accent"), stem opacity 0.55;
  - addressed note: hollow, 1-px outline in `Theme.textMuted`, stem opacity 0.25.
- **Hover:** pointing-hand cursor + a tooltip `<timecode>  —  <note text>` (the timecode alone when the note has no
  text) (`:2355-2372`). Hit area 13 × 14 px, wider than the flag.
- **Click:** `WindowManager.seekToAnnotationNote(timecode)` (`:2362-2363`) → `seekToFrame(note.frame)` for a file
  (`window_manager.cpp:9226-9236`). The area sits above the scrub area, so a click on a pin seeks to the note rather than
  scrubbing.
- **When shown:** hidden when the timeline's `sourceMode === 1` (`:2286-2287`); and `notesList` is empty unless
  annotations are allowed for the current media — single video / image sequence, not dual, not playlist
  (`:2280-2283`, `window_manager.cpp:8876-8881`). A stream has no timeline, so no pins.
- **Setting / shortcut:** none. No toggle to hide them.
- **Next / previous note:** none for files. The only note stepping in 2.x is a stream's Review mode, where `←` / `→` walk
  the notes in capture order (`window_manager.cpp:4881` `stepReviewNote`, `:5463-5464`) — not a timeline feature.
- **Not in Compact Mode:** the compact strip shows the playhead and in / out marks only (`docs/basics.md:75`; no
  `notesList` use outside `NotesPanel.qml` and `TimelinePanel.qml`).
- Family note: minNotes draws the same marks in the same violet on its transports
  (`minNotes/app/qml/MinNotes/App/Theme.qml:68-69`, `CompactTransport.qml:75`).
- Docs: `docs/annotations.md:19` — "A diamond marker appears on the timeline" (the shape is a pennant; see A.1 gap 4).

### C.2 What 3.0 has [code]

- **Ruled, designed, tokenised — not drawn.**
  - Row 166 (`docs/rust-wgpu-port.md:2470`), RULED + BUILT 2026-11-07: the 32-px scrubber = "ruler + playhead + in/out +
    **pennants** + cache underline in one band" — and the build note: "(`min-ui::Scrubber`: camera, gesture, ticks, in/out,
    extent, hover; the waveform waits for the audio probe, **pennants for notes**)".
  - §25.2 (`:2442-2443`): "note pennants on the bottom edge".
  - Tokens exist and are unused: `note-marker-color` → `purple-900` (`crates/min-theme/tokens/color-aliases.json:71`),
    `scrubber-note-marker-color` (`component.json:247`). No `.rs` file references either.
  - `crates/min-ui/src/scrubber.rs:4` (module doc): "the pennants (later: notes)". `ScrubState` (`:18-24`) has no notes
    field. `crates/qcv-timeline` has no note code.
- Notes landed after the scrubber (row 215, 2026-11-08) and the pennants were never picked back up. **No ruling drops
  them** — this is an owed item, not an open question.
- What it hangs on: the notes drawer already holds every card's frame, key, text and addressed state and already emits
  `Cmd::Seek(frame)` (`apps/qcv/src/notes_drawer.rs:39`, `:203`); the scrubber already maps time ↔ x and draws in / out
  flags on the same edge (`scrubber.rs:162`, `:205-210`).

### C.3 Build outline — size **S** (about half a day to a day, one walk)

1. `min-ui::Scrubber`: `ScrubState` gains `notes: &[NoteMark { frame, addressed }]` (or a `set_notes` on the widget so the
   four `ScrubState { … }` call sites stay as they are); draw stem + pennant on the bottom edge with
   `SCRUBBER_NOTE_MARKER_COLOR` — filled for open, hollow + muted for addressed — under the playhead, over in / out,
   clipped to the track.
2. Hit-test before the scrub gesture: a press within the 13 × 14 px area returns a "note clicked" result instead of
   starting a scrub; hover returns the pin so the app shows `<key>  —  <text>` through the existing tooltip path.
3. `apps/qcv`: feed the marks from the annotation model whenever notes change (add, delete, addressed, the sidecar
   reload at `main.rs:885`); a click = the drawer's existing `Cmd::Seek(frame)`.
4. Empty in dual, playlist and stream modes — which is what the annotation model already gives (row 215: annotations in
   single view only).
5. Tests: x of a mark at a known zoom; a press on a mark seeks and does not scrub; an addressed mark draws hollow.
   Snapshot through the existing `QCV_UI_SNAP_*` path. No new strings (the tooltip is data).

### C.4 Rulings needed

| # | Question | [proposed] default |
|---|---|---|
| C-1 | Build the pennants now | **[proposed]** yes — owed by row 166, QCView has them, the token is waiting |
| C-2 | Colour | **[proposed]** row 168's `purple-900` token, not 2.x's `#b48ef0` literal (already ruled; noted because it differs from minNotes' "same violet as QCView") |
| C-3 | Compact strip | **[proposed]** no pins there, as 2.x |
| C-4 | Next / previous note keys for files | **[proposed]** none — 2.x has none for files; a new feature would be its own proposal |
| C-5 | Clip bars / playlist timeline | **[proposed]** none (annotations are single-view only, row 215) |

---

## Summary table

| | Verdict | Build |
|---|---|---|
| A | 2.x: one stream-only button in the Export notes panel; hands the stream notes **folder** as `minnotes:///<dir>`; minNotes creates / opens `<name>.mnd` in it. 3.0 writes the same folder and already has the link builder and the strings | S — ~½ day + a walk per OS; rulings A-1 … A-5 |
| B | **Documented but never built.** 2.x = two mute pills on the viewport source bars, A on / B muted, summed at unity, master volume over the sum. 3.0 already matches | none; rulings B-1, B-2 (docs) |
| C | 2.x: violet pennants on the timeline's bottom rail, hover = timecode — text, click = seek; no setting, no shortcut, no next / previous. 3.0: ruled in row 166, token defined, **not drawn** | S — ½–1 day; rulings C-1 … C-5 |
