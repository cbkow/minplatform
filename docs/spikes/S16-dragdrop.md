# S16 - real OS drag and drop of files from a winit window (opened 2026-11-15)

**Status: Part A PASS on macOS (2026-11-15) - 50 launches of 50, every check green. Part B (the owner's hand, both
systems) and all of Windows are OPEN: no real drag has been performed by anyone yet. The Windows code type-checks for
the msvc target and has never run.**

## Why

UFB (the file browser, canon row 459) lives on dragging files between itself and Finder / Explorer / other
applications; the Qt application spent a year hardening it (`docs/phase5/step0-browser-ui.md`, sections 3.2 - 3.4 and
the "hard-won fixes" table). None of the family's Rust applications drags a file OUT to the system, and the platform
has no layer for it.

## The question

From a winit 0.31 window drawn by us, can we do real OS drag and drop of files, both ways, on macOS and Windows:

- **(a) out** - start the system's drag for one or many paths from our own pointer handling, with a picture, offering
  copy / move / link, accepted as files by Finder, the Desktop, the Dock, Mail, a browser's upload field; and learn
  afterwards what the target did;
- **(b) in** - be a drop target for files, and for what browsers and mail clients drag (promised files, URLs, text):
  enter / over / leave with the pointer's place, the operation the modifiers choose, the paths at the drop (or the
  promised files once delivered);
- **(c)** drags between two of our windows and within one - the source can tell "this drag is mine";
- **(d)** beside winit's own drag handling, without breaking the pointer or the keys.

## Pass bar

**Part A, by script** (`s16 --selftest`, no hand on the machine, no synthetic input, the general pasteboard never
touched): 50 launches of 50 exit 0 with every check passing -

- two windows are made, each becomes a drop target, each draws;
- the view still is winit's (answers `mouseDown:` / `keyDown:`) and now answers the dragging-destination methods;
- the very dragging items `start_drag` builds for the 9 generated paths, written to a pasteboard of our own, come back
  as 9 items and the same 9 files; the drag picture is made from our own pixels;
- the destination methods, called as AppKit calls them with a dragging info of our own, deliver enter / over / leave /
  drop with 9 paths, the place within 1 px, and the operation the mask leaves;
- a press and a release handed to the view in-process still arrive as winit pointer events; winit's own drag events: 0.

**Part B, by the owner's hand**: every line of the checklist below, on each system.

## Time box

Ninety minutes for Part A, the Windows code and this page.

## How to run

```
CARGO_TARGET_DIR=target/s16 cargo build --release -p s16-dragdrop
target/s16/release/s16                # from the repository's root: two windows (Part B)
target/s16/release/s16 --selftest     # Part A: prints SELFTEST lines, exits 0 / 1 (3: its own watchdog)
```

- The log is on screen (right pane of each window) AND in `target/s16-files/s16.log` (appended; `--log <file>` moves it).
  Long lines are cut on screen; the file has them whole.
- The files to drag out are generated under `target/s16-files/out` (five plain ones, one with accents and `#` `&`, an
  empty one, a very long name, a folder with a file in it). Promised files that arrive land in
  `target/s16-files/promised`. Key **R** makes the files again after a move took them away.
- Keys: **1** offer copy+move+link (the default), **2** copy only, **3** move only; **C** clears the on-screen log.
- A row is chosen by a click; Cmd / Ctrl-click adds, Shift-click extends. A press moved 6 px starts the drag: the
  chosen rows if the pressed row is among them, else that row alone.
- During a drag over a window: a green outline (red: refused), a cross at the pointer with the operation a drop would
  be, and the row under the pointer lit (rows are drop targets too).
- The spike **copies and moves nothing** on a drop: it logs what it would do. See "what is reported back" below before
  dragging a move INTO it from Finder / Explorer - use throwaway files.

## Part A results (macOS, 2026-11-15)

50 launches, 50 passes, no process left behind.

| Check | Result |
|---|---|
| Two windows, each a drop target, each drawn | ok |
| The view's class is `S16Drop_WinitView` over `WinitView`; it answers the destination methods, `mouseDown:`, `keyDown:` | ok |
| Registered on the view: `public.file-url`, `public.url`, `public.utf8-plain-text`, `NSFilenamesPboardType`, the three file-promise types | ok |
| The drag picture from our pixels: 360 x 68 px -> 180 x 34 points; 9 dragging items, the first carrying the picture | ok |
| 9 paths out as 9 pasteboard items (`public.file-url` each; the pasteboard adds `NSFilenamesPboardType` and the URL types by itself) and back as the same 9 files | ok |
| Enter and over: 9 files, the place (240,160 px expected and seen), mask copy+move+link answered "copy" | ok |
| Leave; then a drop with all 9 paths | ok |
| A mask narrowed to "move" (what Command leaves) is answered "move" | ok |
| A file promise (a provider of our own) is recognised as a promise at the drop | ok |
| The promised file is DELIVERED | **not checkable**: AppKit raises "can only be called during -prepareForDragOperation:, -performDragOperation:, and -concludeDragOperation:" outside a real drag. The raise is caught and the loop lives. |
| A press and a release handed to the view arrive through winit (2 button events) | ok |
| winit's own drag events while ours is in place | 0 |

### What Part A could NOT verify (honestly)

- **That a drag starts at all.** `beginDraggingSessionWithItems:event:source:` was never called: it needs a real
  pointer press. Its inputs (items, picture, source object) are built and inspected; the call itself, and that the
  application's current event IS the pointer's event when we are inside winit's `PointerMoved`, are Part B's first line.
- That Finder, the Desktop, the Dock, Mail or a browser ACCEPT the drag; what operation they report back.
- That AppKit, during a real drag, asks OUR view and not winit's window (we registered the view and unregistered the
  window - the documented order - but only a real drag shows it).
- Promised files arriving (above). URLs and text from a browser (the reader is exercised with file URLs only).
- The modifier keys narrowing the mask during a real drag; the picture as seen on screen.
- Everything on Windows.

### Findings on the way

1. **An Objective-C exception that leaves through winit's handler silently kills the application's events.** The first
   promise test raised inside our drop method, which was running inside winit's `about_to_wait`: the run loop went on
   turning, but winit never called the handler again (no redraw, no pointer, no exit) and nothing was printed. Every
   AppKit call of ours that can raise, made from a callback, must be wrapped (`objc2`'s `exception` feature, as
   `receive` in `mac.rs` now is). This is the riskiest thing found.
2. **A path that comes back from a drag is not byte-equal to the path that went out.** AppKit returns names
   decomposed (u + U+0308 for U+00FC): 8 of the 9 generated paths were identical, the accented one was not, though it
   is the same file. "Is this drop onto its own folder", "is this path one of the dragged ones" must compare file
   identity or normalised names, never bytes.
3. File URLs must be read as `NSURL` objects and asked for their path: Finder puts file-REFERENCE URLs on the
   pasteboard, which a string parse of `public.file-url` cannot resolve. (The old application's "multi-file drop
   truncated to one URL" does not exist here: `readObjectsForClasses:` returns every item.)
4. **winit 0.31.0-beta.4 has a drag-and-drop API of its own** (`start_drag`, `DragEntered` / `DragPosition` /
   `DragDropped`, a data-transfer object, actions, a drag icon; AppKit and Win32 both). Read, not run. Its type list
   (text, URI list, HTML, RTF, audio, image) has no promised / virtual files; whether it reports the performed
   operation the way a file manager needs, and what its Win32 data object carries beyond a file list, was NOT
   examined. It is a beta surface on an exact pin. This spike's layer does not use it and switches it off; a ruling on
   "ours or winit's" should look at it once more.
5. Where winit keeps its destination decides how we sit beside it: on macOS it is the WINDOW's delegate, so the view
   was free for us. A winit update that moves it to the view would collide - the self-test's first check catches that.

## Part B - the checklist (the owner's hand)

One line per case. Tick when the log AND the result agree with "Expect". M = macOS key, W = Windows key.
On Windows run `s16 --selftest` first and note its lines here (it is that system's Part A).

### (a) Drag out

| # | From (window 1's rows) | To | Keys | Expect | macOS | Windows |
|---|---|---|---|---|---|---|
| O1 | one plain row | a Finder / Explorer window, same volume | none | the drag starts with a picture (`the system took the drag`); the file arrives; log `DRAG #n ENDED: ... MOVE`, "1 of 1 source(s) no longer exist" (same volume = move; if the system copies instead, write that down - the rule is its own). Then **R**. | ☐ | ☐ |
| O2 | one plain row | the same folder window | M Option / W Ctrl | arrives as a COPY; log `COPY`, 0 sources gone | ☐ | ☐ |
| O3 | five rows (Cmd / Ctrl-click) | the Desktop | M Option / W Ctrl | all five arrive; the log's `out:` lines list five | ☐ | ☐ |
| O4 | the accented name, the long name and the folder together | a Finder / Explorer window | M Option / W Ctrl | names intact; the folder with its inner file | ☐ | ☐ |
| O5 | one row | the Trash (Dock) / Recycle Bin | none | the file is in the Trash; log `MOVE` (macOS adds "the Trash"); source gone. Then **R**. | ☐ | ☐ |
| O6 | two rows | a new message in Mail / Outlook | none | attached as two files; log `COPY`; sources remain | ☐ | ☐ |
| O7 | one row | a browser's file-upload field (any page with one) | none | taken as a file; sources remain | ☐ | ☐ |
| O8 | one row | Terminal / a console window | none | the path is typed there | ☐ | ☐ |
| O9 | a .txt row | a text editor's Dock icon / its open window | none | the editor opens the file | ☐ | ☐ |
| O10 | one row, after key **2** (copy only) | a folder window, same volume | none | a COPY (the move is not on offer) | ☐ | ☐ |
| O11 | one row, after key **3** (move only) | a folder window on ANOTHER volume, if there is one | none | a MOVE across volumes; then **R**, key **1** | ☐ | ☐ |
| O12 | one row | a folder window | M Control (or Option+Cmd) / W Alt | an alias / a shortcut; log `LINK` | ☐ | ☐ |
| O13 | one row | let go over the menu bar / the taskbar's empty part; and once more ending with Esc | none | log `ENDED: ... NONE` (cancelled); on macOS the picture flies back; nothing moved | ☐ | ☐ |
| O14 | look at the picture | - | - | a small dark card with one white mark per file, sharp on a Retina / scaled screen, the pointer near its top left | ☐ | ☐ |
| O15 | three multi-drags in a row, each with a different choice of rows | a folder window | M Option / W Ctrl | each drag's `out:` lines are THAT choice (the old "stale payload" edge) | ☐ | ☐ |
| O16 | choose three rows; press one of them and let go without moving; then press one and drag | - | none | the click leaves that row alone chosen; the drag carries all three | ☐ | ☐ |
| O17 | a row that is NOT among the chosen ones | a folder window | M Option / W Ctrl | only that row goes | ☐ | ☐ |
| O18 | any drag, over remote desktop if at hand | a folder window | - | the drag still starts (no picture is acceptable; no drag is not) | ☐ | ☐ |

### (b) Drop in

| # | From | To | Keys | Expect | macOS | Windows |
|---|---|---|---|---|---|---|
| I1 | Finder / Explorer: one file | window 1's log pane | none | `ENTER` (types listed), outline + cross following the pointer, `DROP ... COPY not ours`, one `path (file)` line | ☐ | ☐ |
| I2 | Finder / Explorer: 12 or more files and a folder together | window 1 | none | EVERY path is listed (the old "truncated to one" edge), the folder as `path (folder)` | ☐ | ☐ |
| I3 | one file | move slowly down window 1's rows, drop on `[folder] s16 folder` | none | each row lights as the pointer crosses it (`OVER ... on row n`); `DROP ... on row 9 "s16 folder"` | ☐ | ☐ |
| I4 | one file | flick it fast onto a row and let go at once | none | the `DROP` line's place and row are where the button was released, not where the pointer was earlier (the old "stale hover" edge) | ☐ | ☐ |
| I5 | a THROWAWAY file | hold over window 1, press and release each key in turn | M Option, Cmd, Control / W Ctrl, Shift, Alt | `OVER` lines: allowed=copy, then move, then link; the cross's word changes at once | ☐ | ☐ |
| I6 | one file | into window 1 and out again; then in and press Esc | none | `LEAVE` each time; the outline and the lit row go away | ☐ | ☐ |
| I7 | a browser: an image dragged off a page | window 1 | none | macOS: `promised:` then `PROMISED FILE DELIVERED` (the file is in `target/s16-files/promised`) and / or a `url:` line. Windows: `promised:` + delivered, or a `url:` | ☐ | ☐ |
| I8 | a browser: a link, and the address bar's icon | window 1 | none | a `url:` line (http...), `text:` beside it | ☐ | ☐ |
| I9 | Mail / Outlook: an attachment | window 1 | none | `promised:` then `PROMISED FILE DELIVERED` with the right size | ☐ | ☐ |
| I10 | a file from INSIDE an archive (Windows: a .zip opened in Explorer; macOS: any archive tool that drags entries) | window 1 | none | a promised file delivered, or real paths under a temporary folder - note which | ☐ | ☐ |
| I11 | Photos / a picture viewer: a picture | window 1 | none | a promised file delivered, or a path | ☐ | ☐ |
| I12 | a text editor: selected text | window 1 | none | only `text:`; no path, nothing crashes | ☐ | ☐ |
| I13 | Finder / Explorer: a file with accents in its name, and one from a network or external volume | window 1 | none | `path (file)` - never `MISSING` | ☐ | ☐ |
| I14 | one file, while the spike's window is NOT the active window | window 2 | none | the drop arrives all the same | ☐ | ☐ |
| I15 | Windows only: a file dragged with the RIGHT button from Explorer | window 1 | none | ENTER / DROP are logged (a menu at the drop would be the target's own job) | - | ☐ |

### (c) Our own drags

| # | From | To | Keys | Expect | macOS | Windows |
|---|---|---|---|---|---|---|
| C1 | window 1: one row | window 2's log pane | none | window 2: `DROP ... MOVE OURS (drag #n)`; then `DRAG #n ENDED: ... MOVE`; no file touched | ☐ | ☐ |
| C2 | window 1: two rows | window 1's own `[folder] s16 folder` row | none | `OURS`, `on row 9`, MOVE; the row lit while over it | ☐ | ☐ |
| C3 | window 1: one row | window 2 | M Option / W Ctrl | `COPY OURS` and `ENDED: ... COPY` | ☐ | ☐ |
| C4 | window 1: one row | out over a Finder / Explorer window and back into window 2 without letting go | none | a `LEAVE`, then a new `ENTER ... OURS`; the drop is OURS | ☐ | ☐ |
| C5 | Finder / Explorer: a file | window 2, right after C1 | none | `not ours` | ☐ | ☐ |
| C6 | a SECOND running `s16`: one of its rows | the first one's window | none | Windows: `from=another process of ours`. macOS: `not ours` (see limits) | ☐ | ☐ |

### (d) Beside winit

| # | Do | Expect | macOS | Windows |
|---|---|---|---|---|
| D1 | after all of the above, search the log for `winit's OWN drag event` | never there | ☐ | ☐ |
| D2 | the log's first lines | macOS: `view class WinitView -> S16Drop_WinitView ... the window's own registration removed`. Windows: `IDropTarget registered (... a target of winit's was not there ...)` | ☐ | ☐ |
| D3 | right after any drag out (the button was released over another application): click a row, Cmd / Ctrl-click, Shift-click, press R, C, 1, 2, 3, resize, move between windows | everything answers at the first try - no "stuck button", no lost first click | ☐ | ☐ |
| D4 | an own drag held over the spike's own windows | the outline, the cross and the lit row FOLLOW the pointer, and the log scrolls, while the drag is still in hand (on Windows winit's loop is stopped inside `DoDragDrop`: the frames are drawn from the drop target itself) | ☐ | ☐ |
| D5 | thirty drags out and in, without restarting | no crash, no highlight left behind, the log still scrolls | ☐ | ☐ |
| D6 | close either window during idle; and (if you dare) press the window's close button's shortcut while a drag is in hand | the application leaves cleanly | ☐ | ☐ |

## What the real implementation must own

A small platform layer (one crate, say `min-dnd`, two files) with the surface this spike's `dnd` module sketches:
`attach(window, sink)`, `start_drag(window, paths, picture, operations)`, and three notices - a drag over us (enter /
over / leave / drop, the place, what is allowed, whose it is, the payload), a promised file delivered, our own drag
ended (what the target did).

**macOS**
- The destination: a run-time subclass of winit's view adding only the NSDraggingDestination methods (or, if that is
  ruled too clever, a patch to the vendored winit); registration of the types on the view; un-registration of the
  window. A check at start that the view's class did not already answer those methods.
- The reader: `readObjectsForClasses:` for `NSURL` (path asked of the URL) and `NSFilePromiseReceiver`; text; types.
- Promises: `receivePromisedFilesAtDestination:` from inside `performDragOperation:`, into a staging folder, results on
  the main queue; **every call that can raise wrapped**.
- The source: an `NSDraggingSource` object kept alive until `endedAtPoint:operation:`; one `NSDraggingItem` per path
  with a file `NSURL` as its writer; the picture from the row as drawn (a count badge is ours to draw); the offer mask
  per context (inside / outside the application).
- The pointer after a drag: winit never sees the release - the widget layer must end its own press when the drag starts.
- `wantsPeriodicDraggingUpdates` YES when auto-scroll and spring-loading are built (the spike says NO).

**Windows** (as written in `win.rs`, unproven)
- Windows made with `with_drag_and_drop(false)`; `OleInitialize`; one `IDropTarget` per window via `RegisterDragDrop`,
  revoked before the window dies.
- The reader: `CF_HDROP`, `FileGroupDescriptorW` + `FileContents` (stream or memory, read inside `Drop`; a worker and
  `IDataObjectAsyncCapability` for large ones), `UniformResourceLocatorW`, `CF_UNICODETEXT`; the format list.
- The keys: Ctrl copy, Shift move, Ctrl+Shift or Alt link, narrowed to what the source offers; the place from each
  call's own point (`ScreenToClient`), the drop's place from `Drop` itself.
- `IDropTargetHelper` so the source's picture shows over our window.
- The source: the SHELL's data object for the items (`SHParseDisplayName` -> `SHCreateShellItemArrayFromIDLists` ->
  `BHID_DataObject`), our own registered format on it as the "mine" mark (process + serial), `Preferred DropEffect`,
  the picture through `IDragSourceHelper::InitializeFromBitmap`; an `IDropSource`; `DoDragDrop`.
- **What is reported back** - the rule the old application stated as "always report Copy so the system never deletes
  the source": a target that answers MOVE makes the source delete its files. When we are the target and move the
  files ourselves, we answer as the shell does (effect NONE; `Performed DropEffect` = NONE and `Logical Performed
  DropEffect` = MOVE on the data object). When we are the source, we read `DoDragDrop`'s effect AND those two formats;
  we delete only for a plain MOVE with nothing written back, and otherwise only refresh.
- `DoDragDrop` is modal inside winit's event handler: nothing of the application may be borrowed across the call; our
  own windows are drawn from the drop target's methods while it runs; timers and background results wait or are
  pumped by OLE's loop - to be seen in D4.
- Paths: `\\?\` prefixes (what `std::fs::canonicalize` returns) stripped before the shell's parser sees them.

**Both**
- Path comparison by file identity / normalised names (finding 2).
- The list model must not be what the drag holds: the paths are captured when the drag starts (the old crash "a
  refresh during a drag" cannot happen by construction; the old "stale payload" neither).
- A self-test like this spike's, kept as a test of the layer: it caught every mistake made here without a hand.

## Known limits of this spike

- No file is copied or moved on a drop; on Windows a stranger's "move" is therefore answered with NONE at the drop
  (the cross still says "move" while over). On macOS the drop answers "move" under Command; Finder is not expected to
  delete on that answer, but I5 asks for throwaway files for that reason.
- "Mine" on macOS is the drag's sequence number within this process; a second process of ours is a stranger there
  (Windows' mark travels on the data object). A pasteboard type of our own would close that if it matters.
- We never PROMISE files ourselves (a file browser's files exist); dragging out of an archive view would need it.
- No spring-loading, no auto-scroll at the edges, no count badge, no right-button-drag menu, no "drop onto the folder
  the items already live in" guard: behaviour, not capability - rulings for the build, not for this spike.
- The drag picture is a drawn card, not a snapshot of the row; the real one comes from the scene we already draw.
- The in-process press / release in the self-test calls the view's own `mouseDown:` / `mouseUp:` with an event object
  made in the process; nothing goes to the window server.
- Windows: written against the `windows` crate 0.62 and checked with `cargo check --target x86_64-pc-windows-msvc`;
  never linked, never run.
- Building it changed the shared `Cargo.lock`: this crate's own dependency list, and ONE new package,
  `objc2-exception-helper` 0.1.1 (a small C shim built with `cc`), which `objc2`'s `exception` feature brings - the
  real layer needs that feature for finding 1. `block2` 0.6.2 was already there.

## VERDICT

**Part A PASS (macOS): 50 / 50.** The layer sits beside winit without touching its classes, the payload it would hand
to the system is right, the destination path works end to end on a pasteboard of our own.
**Part B and Windows: OPEN** - no verdict on the question until the checklist is ticked on both systems.

## Windows, Part A, run 2026-11-15 (`s16 --selftest`, Windows 11, in the desktop session; exit 0, first try)

- Two windows, each a drop target of our own (OLE), each drawn.
- 9 paths out in the shell's data object and back; formats offered: Shell IDList Array, Preferred DropEffect, our own
  mark, CF_HDROP, FileName, FileNameW, ZoneIdentifier.
- Our own mark and the preferred effect ride on the data object.
- The drag picture made from our own pixels is taken by the shell's helper.
- Enter and over reach us with the files, the place and the operation (our own drag: a move); then leave; then a drop
  with every path (Ctrl: a copy), and the logical effect is written back.

As on macOS, no drag was performed: `DoDragDrop` was never called. Part B on Windows is the owner's hand.

## VERDICT (Windows): Part A PASS; Part B OPEN

## macOS, Part B, a first pass by the owner's hand (2026-11-15)

Not the whole table - the main cases. From the log of that sitting:

- **7 drags OUT, every one taken by the system**; the target's answer came back each time: 3 MOVE, 3 COPY, 1 NONE
  (cancelled: the source still there). A row let go on Terminal typed its path (escaped by Terminal, as it does for
  any dragged file).
- **4 drops IN from other applications**, each with its place and its paths; one promised file DELIVERED; one drag
  from a browser arrived as a `url:`.
- **2 drags between our two windows**, both known as OURS, a move.
- No missing path, no error, nothing that stopped the events.

Not done in this pass: modifier-by-modifier (I5), the Trash, Mail, an upload field, archives, the stale-payload and
stale-hover edges by intent (O15, I4), remote desktop (O18), a second process (C6). Windows: nothing by hand yet.

## VERDICT (macOS): the question is answered YES - a real drag goes out and comes in. The edge cases stay on the list.

