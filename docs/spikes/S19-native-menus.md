# S19 - a native menu with our rows and the system's rows for files (opened 2026-11-15)

**Status: VERDICT (macOS): Part A PASS; Part B and Windows OPEN (2026-11-15). In a winit 0.31 window a native NSMenu
holding our own rows and the system's rows for a set of files is built from public API, shown at a point of our view by
our code, closed by our code, and tells us which of OUR rows was chosen. TWO rules came out of it: the menu must be
shown from the run loop, never from inside a winit callback (winit 0.31 panics), and a menu closes by itself when
another application comes forward. Not seen by script: what the Share and Services rows show on screen (Part B). The
Windows code is written and type-checked for the MSVC target, never run.**

## Why

UFB's rebuild rule is "stay native as often as possible": the application is a window onto the file system. A file's
context menu must offer what the SYSTEM offers for that file - Open With, Get Info, Share, Services, tags on macOS;
the shell's own menu (Open with, Send to, Properties, installed extensions) on Windows - together with our own rows
(Rename, Copy to Other Pane, Mark as Tracked ...). The old application does part of it as a second step ("Show Finder
Menu" / "Show Shell Menu", `docs/phase5/step0-browser-ui.md` section 4.3). Our windows are drawn by us on winit; a
native menu is a nested, modal thing of the system's. Whether the two live together was not known.

## The question

In a winit 0.31 window drawn by us, can we show at a given point a NATIVE menu that combines our own rows with the
system's rows for a set of files, and learn which of OUR rows was chosen - on macOS and on Windows?

## Pass bar

Part A, by script (`S19_SCRIPT=all`, no hand, nothing launched, no input sent, the clipboard untouched), macOS:

1. an NSMenu for one file and for several: three rows of ours (one disabled, one with a submenu), a separator, Open
   With as a submenu of real applications with icons and the default marked, Get Info / Show in Finder, and Share and
   Services if public API allows; a statement of what public API can and cannot reproduce of Finder's menu;
2. shown at a point of our view by our code and closed by our code after about a second, **20 times**, no crash, the
   winit loop alive afterwards; how menu tracking and winit's loop live together;
3. every row logs to a file when chosen; the rows' actions invoked by program reach our callback; Open With is
   verified by the URL and the application it WOULD use;
4. the applications the system offers for a .txt, a .png, a .pdf, a .mov-named dummy and a folder, printed.

Windows: the code written (IShellFolder / IContextMenu, our ids in their own range of the same HMENU,
TrackPopupMenuEx on our window, InvokeCommand for the shell's rows, the IContextMenu2 / 3 forwarding), with the
thread and apartment stated and the old module's gaps listed. Run later on the Windows box.

Part B, by the owner's hand: the list at the end.

## Time box

About an hour.

## How to run

`CARGO_TARGET_DIR=target/s19 cargo build --release -p s19-native-menus`, then from the repository's root
`S19_SCRIPT=all target/s19/release/s19` (about 35 s; a small window shows and menus open and close in it; other
values: `enumerate`, `menus`, `plumbing`, `popups`, `inhandler`). With no `S19_SCRIPT` the window stays and a RIGHT
CLICK in it shows the menu (`S19_HAND=one|many|mixed`; `S19_HOW=positioning` for the other call; `S19_LIVE=1` makes
Open With and Show in Finder real). Test files are made under `target/s19-files`; chosen rows are appended to
`target/s19-files/s19.log`.

## Part A results (macOS 27.2, 2026-11-15)

### 1. The menu, built

For one .txt file (two .txt files give the same with "Rename Items…"):

```
Rename                      [OURS id 1]
Mark as Tracked             [OURS id 2, disabled]
Copy to >                   Other Pane [OURS id 3] | Bookmarks… [OURS id 4]
--------
Open With >                 TextEdit (default) [icon] | -------- | 16 more applications, each with its icon | -------- | Other…
Show in Finder
Get Info
Quick Look                  (a row only; not built)
--------
Share…                      [icon]   (AppKit's own row: NSSharingServicePicker.standardShareMenuItem)
Tags >                      Gray | Green | Purple | Blue | Yellow | Red | Orange
```

For three files of three types (.txt, .png, .pdf) Open With holds the seven applications that open all three and no
default (our rule in the spike: the applications common to every file; a default only when all files share it. What
Finder shows for a mixed selection is for the owner to compare).

Two things the first build got wrong: the display name of an application carries ".app" when Finder is set to show
all extensions (stripped now), and two copies of one application have the same name (Finder tells them apart by
version; the spike appends the bundle's short version, which here is the same for three copies - the product needs
the path as the last resort).

### 2. Shown and closed by program

Every scripted popup is `popUpMenuPositioningItem:atLocation:inView:` on winit's own NSView (20 of them), or
`NSMenu.popUpContextMenu:withEvent:forView:` with an NSEvent object made for the call (3), closed by
`cancelTracking` one second later; one more has its first row chosen by program while it is up. **24 popups a run.**

| Runs | Popups | Closed by us after 1 s | Crash | The winit loop afterwards |
|---|---|---|---|---|
| five runs (one of the popups alone, four of the whole script) | 120 | 120 | none | alive |
| two more runs of the whole script | 48 | 43 - five closed early, see below | none | alive |

(The script's printing changed between some of these runs; the popup code did not.)

- A menu closed by `cancelTracking` at 1.00 s is gone at **1.24 s** (the fade); the call that showed it returns then.
- **The early closes** (5 popups in two runs; 0.01 to 0.74 s instead of 1.24 s): each time another application had
  come forward - another spike of ours was running on the same desktop and was the frontmost application when the
  spike looked. A menu is closed by the system when its application stops being the active one. The call returns as
  for a cancel; nothing else happens. The script counts these as failures of its own timing; they are not crashes
  and the loop went on. (One earlier run ended at once with "no native view": the script's own mistake, a wake-up
  that arrived before the window was made; fixed.)

### 3. Which row was chosen

- No menu on screen, `performActionForItemAtIndex:`: Rename reaches our callback with id 1; the DISABLED row does
  not fire; the submenu's row reaches it with id 3; Open With's first row WOULD open the file with
  `/System/Applications/TextEdit.app`, which is what `URLForApplicationToOpenURL:` names as the default; another
  row would use its own application; Get Info and Show in Finder fire. Six lines in the log file. Nothing launched.
- Menu on screen, the first row chosen by program at 0.5 s: our callback got id 1 **while the menu was still up**.
  `performActionForItemAtIndex:` does not close a tracked menu, and `popUpMenuPositioningItem` still returned
  `false` when we closed it. So: the row's ACTION is how we learn the choice (its tag is our command id); the
  return value of the popup call says nothing about our rows. Whether a hand's choice sends the action before the
  popup call returns is in Part B (the spike prints both).

### 4. The applications the system offers

`URLsForApplicationsToOpenURL:` and `URLsForApplicationsToOpenContentType:` (the UniformTypeIdentifiers route) gave
the same count for every file. The default first, the rest by name:

| File | Type | Default | Offered |
|---|---|---|---|
| .txt | public.plain-text | TextEdit | 17: TextEdit, Adobe Illustrator 2026, Adobe Illustrator 2027, Claude, Firefox, Google Chrome, Helium, Instruments, Microsoft Excel, Microsoft Word, Notes, Qt Creator, Safari, Script Editor, Typora, Visual Studio Code, Xcode |
| .png | public.png | Preview | 19: Preview, Adobe Acrobat, Adobe Illustrator 2026, Adobe Illustrator 2027, Adobe Photoshop 2026, Adobe Photoshop 2027, Claude, CleanShot X, ColorSync Utility, Firefox, Google Chrome, Helium, Image Playground, QCView (three copies), QuickTime Player, Safari, Topaz Gigapixel |
| .pdf | com.adobe.pdf | Preview | 18: Preview, Adobe Acrobat, Adobe Illustrator 2026, Adobe Illustrator 2027, Adobe InDesign 2026, Adobe InDesign 2027, Adobe Photoshop 2026, Adobe Photoshop 2027, Books, Claude, ColorSync Utility, Droplet, Firefox, Google Chrome, Helium, Microsoft Word, Safari, Save as Adobe PDF |
| .mov (a dummy) | com.apple.quicktime-movie | QuickTime Player | 9: QuickTime Player, Adobe After Effects 2027, Adobe Premiere Pro 2026, Adobe Premiere Pro 2027, CleanShot X, QCView (three copies), TV |
| a folder | public.folder | Finder | 7: Finder, Archive Utility, Books, Claude, GitHub Desktop, Typora, Visual Studio Code |

The system lists every registered copy of an application (three of QCView on this machine). Whether Finder shows
fewer, or in another order, is for the owner's eye (Part B).

## What public API can and cannot reproduce of Finder's menu

"Seen" = this spike did it; "known" = the API is public but was not exercised here.

| Finder's row | Public API | State |
|---|---|---|
| Open With: the list, the default, icons | `NSWorkspace` `URLForApplicationToOpenURL:`, `URLsForApplicationsToOpenURL:`, `iconForFile:`; opening is `openURLs:withApplicationAtURL:configuration:completionHandler:` | seen (the open itself only as "would") |
| Open With > Other… | our own NSOpenPanel on the Applications folder, as the old application does | known |
| Always Open With (changing the default) | `setDefaultApplicationAtURL:toOpenContentType:` changes it for the TYPE; Finder's per-file choice has no public API | known / cannot |
| Show in Finder | `activateFileViewerSelectingURLs:` | seen as a row (real under `S19_LIVE`) |
| Get Info | **none.** Only an Apple event to Finder ("open information window"), which needs the user's Automation consent once; the old application does that | cannot, except by asking Finder |
| Share | `NSSharingServicePicker.standardShareMenuItem` (macOS 13+): AppKit's own row; the picker must outlive the menu | seen as a row; what it opens is Part B |
| Services | not a row we add: AppKit appends them to a menu shown with `popUpContextMenu:withEvent:forView:` when the view answers `validRequestorForSendType:returnType:` and writes the selection on request. **The application must first register file types** (`registerServicesMenuSendTypes:returnTypes:`): without it AppKit asked our view about text types only (seen); with it AppKit asked about `public.file-url` and `NSFilenamesPboardType` and took the files onto a private pasteboard of its own ("CFPasteboardUnique-…", not the clipboard) each time the menu opened (seen) | the machinery is seen working; the rows on screen are Part B (our NSMenu's own item list did not change while it was tracked) |
| Tags | names and colours: `NSWorkspace.fileLabels` / `fileLabelColors`; reading and writing: `NSURLTagNamesKey` | seen as plain rows; Finder's row of coloured dots is a view of Finder's, ours to draw if wanted |
| Quick Look | `QLPreviewPanel` (public, in Quartz); it wants methods on an object in the window's responder chain | known; not built (not cheap) |
| Quick Actions (Finder's submenu and the preview pane's) | no API lists them. Those that are Services arrive through Services | cannot |
| Third-party Finder extensions (sync badges and their menu rows) | none: Finder Sync extensions are loaded by Finder only | cannot |
| Finder's own verbs (Move to Trash, Duplicate, Make Alias, Compress, Copy, Rename, New Folder with Selection) | not "the system's rows": ordinary file operations, ours to build | - |
| Automatic rows AppKit may add to a context menu (Services, and whatever a later system adds) | `allowsContextMenuPlugIns` (on by default) | follows the Services row above |

## Menu tracking and winit's loop

Measured with a redraw asked for at every wake-up, a second thread waking the loop 100 times a second, and two AppKit
timers at 100 Hz (one in the default run-loop mode, one in the common modes).

| | winit redraws delivered | winit loop turns | wake-ups from the thread | timer, default mode | timer, common modes |
|---|---|---|---|---|---|
| No menu, 1.00 s | 82 | 205 | 82 | 100 | 100 |
| A menu tracked, 1.24 s (the 23 popups of the last run, least to most) | 99 to 103 | 282 to 325 | as the redraws | **0** | 122 to 125 |
| After the 24 popups, 1.00 s | 82 | 212 | 82 | 101 | 101 |

1. **Shown from inside a winit callback, winit 0.31 panics.** The popup call does not return until the menu is gone;
   it runs the run loop itself, the next wake-up arrives, and winit refuses to enter its handler twice:
   `winit-common … event_handler.rs:138: tried to handle event while another event is currently being handled`
   (`S19_SCRIPT=inhandler`; the process ends with code 101). In 0.31 this is a panic, not a dropped event.
2. **Shown from the run loop, everything goes on.** The spike hands the popup to
   `performSelector:withObject:afterDelay:0` on an object of its own; the call then runs from the run loop with no
   winit callback on the stack. While the menu is tracked winit's loop turns, redraws are delivered at the rate they
   were before, and wake-ups from other threads arrive. The window keeps drawing under an open menu.
3. **Default-mode timers stop** while a menu is tracked (0 ticks); timers in the common modes fire. Anything of ours
   that must run under a menu belongs in the common modes. NOT measured: winit's own `ControlFlow::WaitUntil`.
4. The popup call blocks ITS caller for as long as the menu is up, so the product's "show a menu" is a request that
   completes later: the chosen command comes back as a message to the loop, as the tray menu's do.
5. A menu closes when another application comes forward (above).

The rule for the product: `min-ui` asks for a native menu; the shell queues it to the run loop; the choice (our
command id, or nothing) is sent back through the event-loop proxy.

## The Windows plan (written, type-checked with `cargo check --target x86_64-pc-windows-msvc`, NOT run)

`spikes/s19-native-menus/src/win.rs`:

- **One menu.** `SHParseDisplayName` for each path, `SHBindToParent` of the first, `IShellFolder::GetUIObjectOf`
  with every child item for ONE `IContextMenu` (all the paths in one folder). Our rows are appended first to a
  `CreatePopupMenu` (ids 1 to 0x0FFF, one greyed, one with a submenu), a separator, then
  `QueryContextMenu(menu, position, 0x1000, 0x7FFF, CMF_NORMAL | CMF_CANRENAME [| CMF_EXTENDEDVERBS with Shift])`
  puts the shell's rows into the SAME menu in their own id range.
- **Tracked on our window**: `TrackPopupMenuEx(TPM_RETURNCMD)` with the winit window as owner. The number returned
  is ours (handed back to the application) or the shell's (`GetCommandString(GCS_VERBW)` for its name, then
  `InvokeCommand` with a `CMINVOKECOMMANDINFOEX` carrying the point, the Unicode verb and the Shift / Control
  state). The shell's "rename" is handed back to US: only the host can start a rename in its own list.
- **The forwarding**: the owner window is subclassed (`SetWindowSubclass`); while a menu is tracked,
  `WM_INITMENUPOPUP`, `WM_DRAWITEM`, `WM_MEASUREITEM` and `WM_MENUCHAR` go to `IContextMenu3::HandleMenuMsg2` (or
  `IContextMenu2::HandleMenuMsg`). This is what fills "Send to", "Open with" and "New" when they open and paints
  owner-drawn rows.
- **Thread and apartment**: the window's own thread, which winit has put in a single-threaded apartment
  (`OleInitialize`); `TrackPopupMenuEx` must be called by the thread that owns the owner window, and shell
  extensions expect that apartment. As on macOS the menu is not shown from inside a winit callback: a private
  message is posted to the window and the menu is shown when the subclass procedure receives it. The menu's modal
  loop dispatches the window's messages, so winit should go on drawing; the spike prints the counts (Part B).
  `QueryContextMenu` loads every installed extension on that thread; the spike prints how long it took.
- **Closed by program** for the scripted runs: a posted private message whose handler calls `EndMenu`.

How the old module does it (`core/src/shell_context_menu.rs` in the Qt application) and what it leaves out:

| The old module | What that leaves out |
|---|---|
| ONE path (`SHCreateItemFromParsingName`, `BindToHandler(BHID_SFUIObject)`) | the selection: several files get the menu of the row under the pointer only |
| its own thread with a hidden zero-size window whose procedure is `DefWindowProc` | **no IContextMenu2 / 3 forwarding**: lazily filled submenus and owner-drawn rows are not served; the menu's owner is not the application's window |
| the thread destroys its window and leaves its apartment as soon as `InvokeCommand` returns | a verb that goes on working on that thread or with that owner window loses both (read from the code, not observed) |
| the shell's rows only, ids 1 to 0x7FFF | none of our rows; it can only be a second step |
| `CMF_NORMAL | CMF_EXPLORE` | no extended verbs with Shift, no `CMF_CANRENAME` |
| `CMINVOKECOMMANDINFO` with the verb's number only | no invoke point, no Unicode verb, no Shift / Control state |
| no `GetCommandString` | "rename" chosen in the shell's menu cannot become our rename |
| the pointer's position read on the calling thread, a "one menu at a time" flag | - (kept in spirit) |

It was put on its own thread because joining it froze the Qt window. Here the menu is tracked on the window's thread
and the loop is expected to go on; if Part B shows it does not, the old shape (own thread, own owner window) with the
forwarding added is the fallback.

Not known until it is run: whether the menu obtained this way on Windows 11 holds the rows of extensions that exist
only for the new-style menu (it is the classic menu, the one behind "Show more options"); how a Win32 popup menu
looks beside a dark window.

## Replace our drawn menu, or a second step?

What was found, and what follows from how each system works:

| | One native menu holding our rows (replace) | Our drawn menu, with a row that opens the system's (second step) |
|---|---|---|
| macOS: who builds it | we do, row by row; the system's rows are ordinary NSMenuItems beside ours (seen). There is no "Finder's menu" to open as a second step - a second step would be a second menu of OUR making | the same rows, one click further away |
| macOS: look | the system's menu: its font, icons, vibrancy, submenu behaviour; Share and Services exist ONLY as native rows | ours matches the application's theme; Open With could be drawn by us (it is data), Share and Services could not |
| macOS: keys | arrows, type-to-select, Return, Escape, VoiceOver come with it; shortcuts can be shown on our rows | ours to write and keep right |
| macOS: a long menu | scrolls by itself; long lists sit in submenus (Open With held 20 rows here) | our drawn menu needs its own scrolling |
| macOS: cost | the popup is a request that completes later (above); a per-platform path in `min-ui` | none new |
| Windows: who builds it | the shell and every installed extension fill their part; we do not choose its rows | our rows are ours; the shell's menu appears only when asked for |
| Windows: content | the shell's part repeats ours (its own Open, Cut, Copy, Delete, Rename) unless we filter by verb, and extensions add what they like: the menu gets long and its order is not ours | our menu stays short; the system's is whole and untouched, as Explorer shows it |
| Windows: cost | every right click pays for `QueryContextMenu` on the window's thread (to be measured in Part B) | paid only when the row is chosen |
| Windows: look | a classic Win32 menu; Windows 11's own Explorer shows a short modern menu first and this one behind "Show more options" | the same two steps Explorer itself has |

**Recommendation [proposed], for the owner to rule:**

- **macOS: replace.** For files and folders the context menu IS one native NSMenu: our rows first, then Open With,
  Show in Finder, Get Info, Share, Tags, with Services appended by AppKit. It costs nothing in function (our rows
  report through their actions), it is the only place Share and Services can live, and it is what "stay native"
  asks for. The pane's background menu should then be native too, so that one window does not show two kinds of
  menu.
- **Windows: second step.** Our drawn menu, with a last row "Show more options" (Shift+F10 and Shift+right-click go
  straight to it) that opens the shell's menu for the whole selection on our window, with the forwarding. This is
  the two-step shape Windows 11's Explorer has, it keeps our menu fast and short, and nothing of the system's is
  lost. The spike's merged menu (ours and the shell's in one) stays available as the alternative if Part B shows
  `QueryContextMenu` to be quick and the merged menu to read well.

## Part B - by the owner's hand

macOS (`target/s19/release/s19`, right-click in the window; `S19_HAND=many` or `mixed` for several files):

1. The menu appears at the pointer and reads as a system menu; "Mark as Tracked" is grey; "Copy to" opens.
2. Open With: compare the list and its order with Finder's for `target/s19-files/sample.txt`, `.png`, `.pdf`,
   `.mov` (does Finder show three copies of one application? which name does it give the default?).
3. Share…: does it open the system's share list (AirDrop, Mail, Messages …)?
4. Services: is there a Services row (or service rows) at the end of the menu, for a file and for the folder? The
   terminal prints what AppKit asked.
5. Choose "Rename": the terminal prints `chosen: OURS id 1` - before or after the popup's own line?
6. Arrow keys, typing a row's first letters, Escape; a click outside the menu; a right click while a menu is up.
7. With `S19_LIVE=1`: Open With an application opens the file in it; Show in Finder shows it.

Windows (on the Windows box; build there, synthetic files only):

1. `S19_SCRIPT=popups`: the menus open and close by themselves; winit redraws are counted while a menu is up.
2. By hand: right-click; our four rows above the shell's; "Send to" and "Open with" fill when opened; Properties
   opens; an installed extension's rows are there and work; two files selected (`S19_HAND=many`).
3. The printed `QueryContextMenu` time, first and later right clicks.
4. Choosing our row prints `Ours(1)`; choosing the shell's Rename prints `ShellForUs("rename")`.
5. How the menu looks beside a dark window; whether new-style-only extensions are in it.

## VERDICT (macOS): Part A PASS; Part B and Windows OPEN

A native NSMenu with our rows and the system's rows for one or several files is built from public API, shown at a
point of our winit view by our code and closed by it (five clean runs, 120 popups; in two more runs 5 menus of 48 were
closed early by another application coming forward, with no crash), the winit loop turns and draws while it is up, and our
rows report their ids through their actions. It must be shown from the run loop, not from a winit callback. Public
API does not give Get Info (only by asking Finder), Quick Actions, per-file "Always Open With" or third-party Finder
extensions. The spike's code dies with this verdict.
