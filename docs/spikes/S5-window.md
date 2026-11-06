# S5 — window: winit 0.31 on both OSes — DnD in (typed data transfer) + drag-out, HiDPI, two windows, fullscreen, the menu bar

**Question (§21.3, §14):** does winit 0.31's window + typed data-transfer layer, plus a thin native layer where it stops,
cover what the family's windows need — so `min-ui` builds on winit rather than owning its NSView / HWND?
**Pass bar:** §14.2's OS-specific edge list reproduces or is ruled out PER ITEM on macOS (Windows on the box): multi-file
drops arrive as one item per file; non-file URLs and text round-trip untouched; refusals are visible (the OS shows "no drop");
the drop target is recomputed at drop time; drop-onto-self is detectable; actions follow modifiers at drop; drag-OUT of real
files lands in Finder; HiDPI scale is reported and honoured across a monitor change; two windows share one event loop;
fullscreen round-trips; a native menu bar with working items exists. Plus the S3 carry-over: does 0.31 fix the text-input
client (it does not — `selectedRange` is still `{NSNotFound, 0}`, the `replacementRange` TODO stands), so the vendored patch
moves forward with the pin or the port owns the view.
**Time box:** 1 week from 2026-11-06. **Status: IN PROGRESS 2026-11-06 — windows / fullscreen / menu bar / clipboard / receive-side types verified by script; drag-out needs a real mouse (AppKit's drag session does not complete on injected events); Windows owed.**

## Facts from reading winit 0.31.0-beta.4 (before running anything)
- The API is split into `winit-core` / `winit-appkit` / `winit-win32` (Windows crate not in the local registry yet) with
  `&dyn ActiveEventLoop`, `Box<dyn Window>`, `can_create_surfaces` instead of `resumed`, `PointerButton` / `PointerMoved`
  instead of mouse events, `SurfaceResized`.
- **Receive**: `DragEntered { id }` → `data_transfer(id).available_types()` / `has_type(&TypeHint)` → `set_valid_dnd_actions`
  (default = REJECT, so refusals are visible by construction) → `fetch_data_transfer(id, &type)` → `DataTransferReceived {
  value, serial }` with `try_as_uris()` / `try_as_string()` / `try_as_bytes()`; `DragPosition { position, proposed_action }`,
  `DragDropped { proposed_action }`, `DragLeft`. `TypeHint`: Plaintext, UriList, Html, Rtf, Audio, Image; macOS maps
  `NSPasteboardTypeFileURL` + legacy `NSFilenamesPboardType` → UriList (read with `readObjectsForClasses` NSURL — §14.2's
  multi-file rule), String → Plaintext, HTML → Html.
- **Drag-OUT EXISTS (§14.3 said "not seen" — corrected):** `start_drag(window, DataTransferSendBuilder.with_type(..).build(),
  &[DndAction], Some(DragIcon))` → `OutgoingDragDropped { action }` / `OutgoingDragCanceled`; the clipboard is the same
  builder through `set_clipboard` / `clipboard()`. macOS writes URIs as ONE `public.file-url` pasteboard item per URI.
- The AppKit backend implements `NSDraggingDestination` on its WindowDelegate (periodic updates on) — one OS drop target per
  window, as §14.3 wanted; no overlay subclass needed.
- `winit-appkit/src/menu.rs` only installs a default menu — the app's own menu bar is objc2 `NSMenu` work (the spike does it).
- Fullscreen: `set_fullscreen(Some(Fullscreen::Borderless(None)))`; HiDPI: `scale_factor()` + `ScaleFactorChanged`.

## What the spike is
`spikes/s5-window` (winit `=0.31.0-beta.4` — the only 0.31 user in the workspace; the 0.30 line stays patched for S1–S3):
a wgpu window drawing nested ZONES (window → panel → cell) as coloured rects; the innermost zone under the pointer wins
(the §14.4 hit-test rule); the "source" zone starts a drag-out of real temp files on press; drops log per zone what arrived
(types, URIs, text, action, position, self-drop); `--dual` opens a second window on the same loop; `F` toggles fullscreen;
`C`/`V` copy/paste; the menu bar has S5 › Toggle Fullscreen / Quit wired to AppKit actions. `tools/s5/mouse.swift` drives
drags (down / move / up at screen points) so the OS DnD path runs end to end without hands: window A → window B, window A →
the Desktop (a real file must appear), Finder → window.

## Results so far (macOS, 2026-11-06, scripted — `tools/s5/mouse.swift` + `tools/s3/keys.swift`)
| Item | Result |
|---|---|
| Two windows on one loop (`--dual`, or `D`) | both created, drawn, addressed by `WindowId`; events route per window; `Focused` flips as expected |
| HiDPI | scale 2.0 reported on both displays and honoured (surface 1440×960 for a 720×480 logical window); a different-scale monitor is not available here, so `ScaleFactorChanged` across monitors is unexercised |
| Fullscreen | `set_fullscreen(Borderless)` round-trips (window moves to 0,0 and back); the native menu's "Enter Full Screen" does the same through AppKit's `toggleFullScreen:` |
| Native menu bar (objc2 `NSMenu`) | installed; the AX tree shows "s5-window" › "Enter Full Screen" / "Quit S5" — AppKit renames the item per state; ⌘F works |
| Clipboard | `set_clipboard` + `clipboard()` + fetch round-trip; the type surface for text = `public.utf8-plain-text` + legacy `NSStringPboardType`; an `srt://` URL inside the text survives untouched |
| Receive-side type names (from the backend source) | file URLs → `TypeHint::UriList` via `readObjectsForClasses` NSURL (+ legacy `NSFilenamesPboardType`), String → Plaintext, HTML → Html; refusal = not calling `set_valid_dnd_actions` (default reject) |
| Innermost-zone hit test | presses land in window / panel / cell / source as designed (the §14.4 rule in 6 lines) |
| **Drag-out (`start_drag`)** | the call enters AppKit's `beginDraggingSessionWithItems:event:source:` and **never returns under injected mouse events** (a modal drag loop runs until the deadline; no drag image appears); winit's plumbing is inline on the main thread, so this is AppKit's session not engaging with synthetic HID events. **Needs the real mouse** — the hand test below. |
| Drag-in from Finder, multi-file, drop-onto-self, actions at drop | pending the same hand test (window A → window B, A → Desktop, Finder → A) |
**The hand test:** `cargo run --release -p s5-window -- --dual`, then drag the orange box (bottom-right of window A) into window B's green cell, then onto the Desktop (three `s5 drag N.txt` files must appear), then drag any two files from Finder into window A's blue panel. The log prints `s5: …` lines for every step: `drag-out started`, `DRAG ENTERED … types`, zone transitions, `DROPPED … zone … action`, `RECEIVED … N URIs`, `OUTGOING … dropped with action`, `[OUR OWN DRAG — drop-onto-self]` when it lands back on A.

## Remaining plan
1. ~~Scaffold runs~~; the §14.2 list as a checklist with a result per item (macOS) — the drag items by hand.
2. The HiDPI + monitor-change + two-window + fullscreen items; the menu bar.
3. Windows on the box (the same list; `with_drag_and_drop` opt-out question; OLE edges).
4. Verdict: winit (+ patches) vs own view/HWND for the port.
