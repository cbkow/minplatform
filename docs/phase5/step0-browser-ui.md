# Phase 5, step 0 — UFB's file browser front end, as built (audit)

Audited: the `ufb` repository, branch `main` @ `9e197f0`, version 1.4.2. Read only; nothing was built or run, so every
statement below is from the source. Where behaviour depends on Qt's runtime event routing it is marked **not verified**.

Scope: the browser as a user meets it (QML). The Rust file operations, projects / metadata / tracker, mesh sync, mounts and
media decoding are other readers' areas; they appear here only as touch points.

Citation legend (all under `app/qml/Ufb/App/` unless a full path is given):

| Tag | File |
|---|---|
| `FB` | `FileBrowser.qml` (3987 lines) |
| `Main` | `Main.qml` (2393) |
| `SB` | `Sidebar.qml` |
| `DBV` | `DualBrowserView.qml` |
| `FTV` | `FolderTabView.qml` |
| `PL` | `PreviewLightbox.qml` |
| `TQ` | `TaskQueue.qml` |
| `LS` | `ListingState.js` |

---

## 1. Window layout and navigation model

### 1.1 Window

- One `ApplicationWindow`, chromeless: the tab strip **is** the title bar on both systems (`Main:19-34`, `Main:1216-1225`).
  macOS keeps the native traffic lights (70 px inset, `Main:1213`); on Windows Qt draws the caption buttons
  (`Main:1214`). Empty strip area drags the window; double-click zooms (`Main:1250-1255`). A 120 px "drag reserve" at the
  right can never be covered by tabs (`Main:1239-1240`, `Main:1299-1301`).
- **No menu bar on either system** — no `MenuBar` and no `Shortcut` element exists in any audited QML file (grep over the
  directory: zero hits). Every command lives in a context menu, a tool strip button or a pane-local `Action`.
- Closing the window hides to a tray icon; Quit is the tray menu or Cmd-Q (`Main:104-108`, `Main:147-209`,
  `app/main.cpp:517`). Minimum size 900 x 540 (`Main:58-59`).
- Layout: `SplitView { Sidebar | (tab content) }` under the title row, plus a 22 px status strip as the window footer
  (`Main:1825-1830`, `Main:1870-1875`, `Main:2015-2016`).
- Qt Quick Controls style is forced to FluentWinUI3 on both systems (`app/main.cpp:408`, `app/main.cpp:432`); menus are
  therefore QML-drawn, never native (`UfbMenu.qml:21-24`).

### 1.2 Tabs (title row)

| Tab kind | Count | Label | Close | Source |
|---|---|---|---|---|
| Files (a dual browser) | 1..n, the last one cannot be closed | always the literal "Files" | x when more than one | `Main:1333-1434`, `Main:780-781` |
| Job (project view) | 0..n | job name, middle-elided, max 180 px | x | `Main:1437-1543` |
| All Tracked | 0 or 1 | fixed | x | `Main:1732-1823` |
| Tasks (queue) | 0 or 1, with an active-count chip | fixed | x | `Main:1626-1730` |
| "+" | — | new Files tab | — | `Main:1566-1588` |
| Close all (x-circle) | — | confirm dialog, keeps the first Files tab | — | `Main:1594-1621`, `Main:2348-2371` |

- Overflowing tabs scroll horizontally; the vertical wheel is translated to horizontal scroll (`Main:1285-1323`).
- Right-click a tab: one item, "Close Other Tabs" (`Main:2376-2384`).
- A new Files tab starts with the right pane collapsed and **both panes at the home folder**
  (`Main:637-651`, `DBV:59-64`); "Open in New Tab" then navigates the active pane (`Main:645-648`).
- Tab reordering, tab drag-out, drop-on-tab: **not found**. A drag on a tab moves the window (`Main:1185-1207`).
- Tab keyboard shortcuts (new / close / next / previous): **not found**.

### 1.3 Dual browser (one per Files tab)

- A header strip ("Files" + the active pane's path in mono + "Refresh both panes") over a horizontal `SplitView` of two
  `FileBrowser` panes (`DBV:98-157`, `DBV:159-276`).
- Each pane owns its own `Directory` model object (`DBV:53-54`). The **active pane** is whichever pane last emitted
  `activated` (mouse press on a row or empty area, a tool-strip click, a drag entering) (`DBV:233`, `DBV:265`,
  `FB:17-20`). The active pane has a 1 px accent outline; an inactive pane's selection is drawn muted
  (`FB:3973-3986`, `FB:3015`).
- Either pane collapses to a 34 px strip with an expand button; the toggles sit on the inner edge in a footer row
  (`FB:290-308`, `FB:2217-2244`, `FB:3726-3779`, `DBV:184-190`). Collapsing the active pane hands activation to the
  other one (`DBV:238-241`).
- Splitter: 6 px hit zone, 1 px hairline; minimum pane width 280 (`DBV:168-182`, `DBV:228`). The dragged left width is
  saved as ONE shared default for every dual browser (`DBV:196-203`, `Main:438-446`).
- Sidebar clicks navigate the active pane of the last-active Files tab (`Main:1876-1886`).
- Keyboard switch between panes (Tab / F6): **not found**.

### 1.4 Sidebar sections (top to bottom)

| Section | Rows | Left click | Right-click menu | Source |
|---|---|---|---|---|
| BOOKMARKS (+ button) | three fixed system rows (Desktop, Downloads, Documents) then the user's | navigate the active pane | Open, Open in New Tab, Reveal in Explorer, Copy ufb:// Link, Copy Path, Remove (user rows only) | `SB:107-126`, `SB:279-400` |
| SUBSCRIPTIONS | subscribed jobs | open the job tab | Open Job, Reveal Folder in Pane, Reveal in Explorer, Copy ufb:// Link, Copy Path, Unsubscribe | `SB:418-519` |
| TRACKERS | All Tracked, Task Queue | open that tab | none | `SB:536-602` |
| MOUNTS | one row per mount with a state dot | navigate when mounted | Open, Open in New Tab, Reveal in Explorer, Start / Stop / Restart, Drain Cache, Open Agent Log | `SB:614-961` |
| footer rows | Path swaps, Mesh sync | open their dialogs | — | `SB:1031`, `SB:1115` |

- Sidebar width persisted; minimum 160, default 220 (`Main:1864-1874`).
- Bookmarks cannot be reordered or renamed in place, and a folder cannot be dragged in to create one (a drop on a
  bookmark row copies/moves INTO it, section 3.4). Add is by dialog only (`SB:283-291`, `AddBookmarkDialog.qml:55-60`).
- No "Volumes / Drives / Recent / Home / Trash" rows: **not found**.
- The sidebar has no keyboard handling at all (no `Keys` in `SB`).

### 1.5 Pane chrome (top to bottom inside one `FileBrowser`)

1. **Path bar** (`FB:2264-2409`): breadcrumb buttons with a chevron separator; long paths clip on the LEFT so the current
   folder stays visible (`FB:2303-2306`). Click a crumb = navigate (`FB:2329-2332`). Click empty crumb area or the
   reserved 48 px tail, or Ctrl/Cmd+L, or Ctrl/Cmd+click anywhere = switch to an editable mono text field with the whole
   path selected (`FB:247-257`, `FB:2293-2297`, `FB:2351-2381`). Return navigates; Escape or focus loss cancels
   (`FB:2392-2407`). Hidden in project-tab panes (`FB:210-213`, `FTV:342`).
   No path completion, no crumb drop-down of siblings, no drop on crumbs: **not found**.
2. **Tool strip** (`FB:2416-2632`): Back, Forward, Up, Refresh, "+" (opens the background menu), the search field with a
   clear button, a search-backend toggle (lightning), a busy spinner, a 3-way view-mode segmented control, a grid-size
   slider (grid only), a Sort button with menu (grid and tree only).
3. **Error banner**: the model's error string in a red strip (`FB:2635-2654`).
4. **List header** (list mode only) (`FB:2665-2883`).
5. **View area** (`FB:2889-3707`).
6. **Footer** (`FB:3726-3779`): the collapse toggle and, inside a "jobs folder", a full-width "New Job" button.

### 1.6 History

- Per pane, in memory: a stack capped at 50, truncating the future when navigating from the middle (`FB:310-341`).
- Back / Forward are tool-strip buttons only. **No keyboard shortcut and no mouse back/forward button** handling exists
  (grep for `BackButton` / `ForwardButton` / Alt+arrow: zero hits).
- Up: tool-strip button and the Backspace key (`FB:2437-2442`, `FB:3813-3815`). The Up button is never disabled at a root.
- History is not persisted; scroll position and selection are not restored when going back (a navigation resets them,
  `FB:1808-1829`). Going Up does not select the folder you came out of (no code for it).

### 1.7 View modes

| Mode | What it is | Source |
|---|---|---|
| list | a `ListView` of 24 px rows, zebra striped: OS icon 16 px, Name, (Synced tick in a jobs folder), Size, Modified | `FB:2954-3179`, `Theme.qml:138` |
| grid | a `GridView`; cell = size + 16 by size + 36; OS icon or thumbnail; a 2-line middle-elided name | `FB:3182-3442` |
| tree | a flat `ListView` drawn as a tree (indent 18 px per depth, caret, icon, name ONLY — no size or date columns) rooted at the pane's current folder | `FB:3444-3665`, `FB:3586` |

- No Miller columns view, no gallery view, no details pane: **not found**.
- Grid size 48-256 px in steps of 8, default 128 (`FB:54`, `FB:2567-2580`). Thumbnails are requested at device pixels
  (`FB:3325-3331`); the icon stays as a placeholder until a real thumbnail is ready and cross-fades (`FB:3248-3287`).
  Thumbnails exist in grid mode only; list rows never show one (`FB:3305-3319`).
- The mode is remembered **per folder** (section 9).

### 1.8 Sorting, grouping, filtering, hidden files

- Sort keys: name, size, modified. Click cycles ascending, descending, **unsorted** ("insert order" = whatever order the
  backend listed) (`FB:166-181`). The default for a never-visited folder is unsorted (`FB:170-171`).
- Folders always sort before files, in both directions (`FB:186-187`, `FB:1858-1864`).
- Name comparison is a plain lowercase string compare (`FB:196-198`): **not natural/numeric** — `shot_10` sorts before
  `shot_2`. No locale collation.
- List mode sorts by clicking a header label; grid and tree use the Sort menu (`FB:2582-2631`). In **tree** mode the sort
  state is shown and changeable but nothing applies it — `refreshTree` appends the backend's rows as they come
  (`FB:2148-2182`); the tree's own order is the backend's (not verified there).
- Sort by kind / extension / date created: **not found**. Grouping: **not found**.
- Filtering inside the current folder: there is none in practice. The search box is a recursive search (section 7); the
  substring filter left in `refreshEntries` (`FB:1840-1846`) is vestigial (the comment at `FB:151-160` says so).
- Hidden files: four names are always dropped — `.gitkeep`, `.DS_Store`, `Thumbs.db`, `desktop.ini` (`FB:1784-1792`).
  No "show hidden files" toggle exists in the QML (grep: zero hits); whether dotfiles / OS-hidden entries reach the view is
  the backend's decision (not verified here).

### 1.9 Columns

- Fixed set: Name, Size, Modified (+ a 26 px "Synced" tick column inside a jobs folder) (`FB:2672-2882`).
- Name and Size are drag-resizable (min 80 / 40 px); Modified takes the rest (`FB:56-63`, `FB:2744-2756`,
  `FB:2842-2853`). During the drag only a guide line moves; the width commits on release because a live write re-flowed
  every row and stuttered (`FB:65-74`).
- No column chooser, no reordering, no Kind / Created / Dimensions / Duration columns: **not found**.
- Size: binary units labelled KB/MB/GB, folders show a dash (`FB:3095`, `FB:3953-3959`). Date: fixed
  `yyyy-MM-dd hh:mm` (`FB:3961-3965`).
- Header labels "Name" / "Size" / "Modified" are untranslated literals (`FB:2687`, `FB:2789`, `FB:2871`).

---

## 2. Selection model and keyboard

### 2.1 Selection state

- List and grid share one state on the pane: `selectedIndexes` (row indexes), `anchorIndex`, `currentIndex`
  (`FB:458-481`). Tree mode has its own: `treeSelectedPaths` (paths) and `treeCurrentPath` (anchor + cursor)
  (`FB:2109-2117`). Switching mode does not carry the selection across.
- After a navigation the cursor is put on row 0 but **nothing is selected** (`FB:1881-1883`).
- An in-place reload (same folder, changed by an operation) restores selection, cursor and scroll by path (`LS:1-56`,
  `FB:1808-1880`). A navigation resets all three.
- Select-after-load: a path can be queued to be selected when the next listing lands (deep links, a finished archive,
  a new web link); it matches the full path or falls back to the bare file name (`FB:463-468`, `FB:1884-1902`).

### 2.2 Shortcut table

Qt maps `Ctrl` in a key sequence, and `Qt.ControlModifier`, to the Command key on macOS; the source has one binding per
action and no per-OS branch. All pane `Action` shortcuts are live only for the pane that is active AND visible
(`FB:39-45`).

| Action | Windows | macOS | Works in | Source |
|---|---|---|---|---|
| Cut | Ctrl+X | Cmd+X | list, grid, tree | `FB:895-904` |
| Copy | Ctrl+C | Cmd+C | all | `FB:905-914` |
| Paste | Ctrl+V | Cmd+V | all | `FB:915-932` |
| Rename (dialog) | F2 | F2 | all, exactly one item | `FB:933-942` |
| Delete to trash (confirm dialog) | Del | forward-delete only (Fn+Delete on a laptop) | all | `FB:943-952` |
| New Folder (dialog) | Ctrl+Shift+N | Cmd+Shift+N | all | `FB:994-999` |
| Refresh | F5 | F5 | all | `FB:1100-1110` |
| Edit path | Ctrl+L | Cmd+L | panes with a path bar | `FB:1111-1117` |
| Move cursor | Up / Down (grid: by a row of cells) | same | list, grid, tree | `FB:3788-3795`, `FB:3853-3854` |
| Move cursor in a row | Left / Right | same | grid only | `FB:3796-3803` |
| Expand / collapse | Right / Left | same | tree only | `FB:3882-3893` |
| First / last | Home / End | same | all | `FB:3804-3809`, `FB:3855-3856` |
| Extend selection | Shift + any of the moves above | same | all | `FB:3931-3936`, `FB:3859-3860` |
| Open | Return / Enter (tree: toggles a folder's expansion) | same | all | `FB:3810-3812`, `FB:3869-3881` |
| Parent folder | Backspace | Delete (the Mac backspace key) | list, grid — **not tree** | `FB:3813-3815` |
| Clear selection | Escape | Escape | all | `FB:3816-3818`, `FB:3895-3897` |
| Select all | Ctrl+A | Cmd+A | list, grid — **not tree** | `FB:3819-3821`, `FB:3894` |
| Preview (lightbox) | Space on a file (nothing on a folder) | same | all | `FB:3822-3841` |
| Clear search | Escape in the search field | same | — | `FB:2504` |

Not bound anywhere (grep verified): Page Up / Page Down, type-ahead find, Ctrl/Cmd+F (focus search), back / forward
(Alt+Left/Right, Cmd+[ / ]), Cmd+Up / Cmd+Down, Cmd+Backspace (the Mac "move to trash"), Shift+Delete (permanent delete),
Cmd+O, Ctrl/Cmd+D or any Duplicate, Ctrl/Cmd+T / W, Ctrl+Tab, Tab / F6 to switch panes, Ctrl/Cmd+I (properties), Ctrl/Cmd+Z,
Ctrl/Cmd+Shift+C (copy path), Ctrl+Up/Down/Space for a non-contiguous keyboard selection, the Menu key / Shift+F10.

### 2.3 Lightbox keys (while a preview is open)

| Key | Effect | Source |
|---|---|---|
| Space | close | `PL:101-107` |
| Escape | step back from an EXR layer to the layer grid, else close | `PL:86-95` |
| Backspace | back to the layer grid only | `PL:96-100` |
| Left / Right | previous / next non-folder item in the originating pane (the pane's cursor moves with it) | `PL:108-115`, `FB:3900-3929` |
| K, W, S | play / pause (video, audio) | `PL:128` |
| Q / E | step one frame back / forward | `PL:122-123` |
| A or J / D or L (held) | accelerating rewind / fast-forward | `PL:126-127`, `PL:149-158` |
| M, V | mute, loop | `PL:129-130` |
| Up / Down | volume (media) or scroll (PDF, text, HTML) | `PL:131-132`, `PL:138-139` |
| Home / End | jump to ends (media) or scroll to ends | `PL:133-134`, `PL:142-143` |
| Page Up / Page Down | scroll a page (PDF, text, HTML) | `PL:140-141` |

The video transport's play button tooltip still says "Play / Pause (Space)" (`VideoPreview.qml:332`) although Space now
closes the lightbox (`PL:101-107`).

### 2.4 Focus rules

- The three views each declare `focus: visible` (`FB:2960`, `FB:3188`, `FB:3453`); a mouse press on a row or on empty
  area forces focus to the view (`FB:3130`, `FB:3168`, `FB:3395`, `FB:3510`).
- Closing the lightbox returns focus to the originating pane's view (`FB:663-672`, `Main:983-989`, `PL:75-82`).
- Escape in the search field calls `root.forceActiveFocus()` on the pane rectangle, not on the view (`FB:2504`); a
  committed path edit does not refocus the view either (`FB:2392-2396`). Whether arrow keys work afterwards without a
  click: **not verified** (the code suggests not).
- Keyboard handlers (`Keys.onPressed`) need the view to hold focus; the `Action` shortcuts are window-level and gated by
  the active-pane flag, so the two sets can disagree about which pane they act on (not verified at runtime).

### 2.5 Rename

A modal dialog with a text field, the base name preselected up to the last dot (`FB:1605-1651`). **No in-place rename.**
One item only; no batch rename. A failure is `console.warn` only (`FB:1644-1648`).

---

## 3. Mouse

### 3.1 Clicks

| Gesture | List / grid | Tree | Source |
|---|---|---|---|
| Press on an unselected row | select only it (selection changes on PRESS, not release) | same | `FB:3128-3146`, `FB:3531-3541` |
| Press on a selected row | keep the multi-selection (so a drag carries it); a plain click never collapses it to that one row | same | `FB:3141-3145`, `FB:3533-3538` |
| Ctrl/Cmd + press | toggle | toggle | `FB:3139-3140`, `FB:3517-3524` |
| Shift + press | range from the anchor | range along the visible rows; anchor stays | `FB:3137-3138`, `FB:3525-3530` |
| Double-click | folder: navigate; file: open with the OS default app | folder: expand / collapse (never navigates); file: open | `FB:3156`, `FB:563-573`, `FB:3571-3581` |
| Caret click | — | expand / collapse | `FB:3607-3619` |
| Right press on a row | select it if it was not selected, then the item menu | same | `FB:855-868`, `FB:3544-3554` |
| Left click on empty area | clear the selection | clear | `FB:2985-2994`, `FB:3162-3176` |
| Right click on empty area | clear the selection, then the background menu | same | `FB:869-872` |
| Middle click | **not handled** (no `MiddleButton` anywhere) | | |
| Slow second click to rename | **not found** | | |

- Rubber-band (marquee) selection: **not found**. A press-and-drag on empty area does nothing.
- Hover: a row tint; no hover tooltip with the full name for elided names (not found).

### 3.2 Drag OUT of a pane (the source side)

| Edge | How it is handled | Source |
|---|---|---|
| What is dragged | the whole selection if the pressed row is in it, otherwise just that row | `FB:597-606`, `FB:3562-3568` |
| Threshold | 6 px from the press point | `FB:3147-3155` |
| Scroll view stealing the gesture | each row's mouse area sets `preventStealing` so the flickable cannot turn a vertical drag into a scroll | `FB:3121-3125` |
| Payload | `text/uri-list` only, built by the backend from the path list | `FB:633-635` |
| Stale payload on a repeated multi-drag | the MIME map is assigned imperatively right before activation; a declarative binding shipped a stale, often single-path list | `FB:626-632`, `FB:1694-1697` |
| Crash when the listing refreshes mid-drag | the drag is owned by one persistent, invisible 1 x 1 item per pane, not by the row delegate (a recycled delegate holding a live drag aborted the process) | `FB:608-616`, `FB:1682-1703` |
| Model rebuild during a drag | deferred until the drag ends | `FB:585-595`, `FB:1795-1801` |
| Drag image | an async snapshot of the pressed row or cell, hot spot 8,8; the image must be set before activation | `FB:618-623`, `FB:636-646`, `FB:1700-1702` |
| Snapshot slow or never delivered (software rendering, remote desktop) | an 80 ms fallback timer starts the drag with no image; a failed grab starts it at once | `FB:648-656`, `FB:1705-1721` |
| Offered actions | **Copy only** (`Drag.supportedActions: Qt.CopyAction`) — a drag from UFB to Finder / Explorer can never be a move | `FB:1693` |
| Multi-item drag image | the single pressed row only; no count badge (not found) | `FB:3153` |

### 3.3 Drop INTO a pane (the target side)

| Edge | How it is handled | Source |
|---|---|---|
| Accepted format | `text/uri-list` | `FB:2918-2922` |
| One drop area over the whole view (z above the rows) | per-row drop areas underneath never received the drag | `FB:2911-2917` |
| Target folder | the folder row under the pointer, else the pane's folder (tree: the tree root) | `FB:674-684`, `FB:2935-2947` |
| Stale hover target on Windows | the target is recomputed from the drop event's own coordinates; move events are not guaranteed during the OS's modal drag loop | `FB:2939-2946` |
| Feedback | the hovered folder row takes the selection fill; the whole pane gets a 2 px green outline; the pane becomes active | `FB:3007-3013`, `FB:3981-3984`, `FB:2923-2927` |
| Multi-file drop from Finder / Explorer truncated to one URL | read both `drop.urls` and the raw `text/uri-list` string, keep whichever has more entries; URL-to-path conversion is the backend's | `FB:686-720` |
| External vs internal | external = the drop has no source item; **always copies, no prompt** | `FB:736-760` |
| Internal (pane to pane, or within a pane) | a modal "Move or Copy?" dialog with Cancel / Copy / Move, **every time** | `FB:761-764`, `FB:1725-1778` |
| Why no modifier keys | the comment says same-volume inference had "risky multi-URL truncation interactions" and that the QML drag event does not expose modifiers | `FB:1677-1681` |
| Action reported to the source | always Copy, so the OS never deletes the source itself; a chosen Move is done by the app's own move | `FB:741-751` |
| Refresh afterwards | the backend's `dirs_changed` broadcast reloads every pane showing an affected folder; two fallback timers (350 ms, then 1500 ms) remain as a safety net | `FB:1983-2002`, `FB:767-793` |
| Drop onto the folder the items already live in, or onto one of the dragged folders | no guard in the QML; the dialog still opens (backend behaviour not verified) | `FB:726-765` |
| Non-file payloads (text, images, URLs from a browser, promised files / virtual files from mail or an archive tool) | not accepted — only `text/uri-list` with file URLs | `FB:2921` |

### 3.4 Other drop targets

| Target | Behaviour | Source |
|---|---|---|
| Sidebar bookmark row | accepts; **no dialog**: external = copy; internal = Ctrl forces copy, Shift forces move, else move on the same volume and copy across volumes | `SB:65-98`, `SB:330-336` |
| Sidebar subscription (job) row | same, into the job folder | `SB:457-462` |
| Sidebar mount row | same, only while mounted | `SB:668-674` |
| Tabs, breadcrumbs, the collapsed pane strip, the Up button | **not drop targets** | — |
| Project-view item list | other reader's area | — |

- Spring-loading (hover a folder / bookmark / tab to open it): **not found**.
- Auto-scroll when a drag nears the top or bottom edge of the list: **not found**.
- Dropping a folder on the BOOKMARKS header to add a bookmark: **not found**.

---

## 4. Context menus

All menus are `UfbMenu`, a QML menu that instantiates every row up front (section 10, item 9). Rows are hidden by
collapsing their height.

### 4.1 Item menu (`FB:1122-1311`)

| # | Row | Shown / enabled when | Calls | Source |
|---|---|---|---|---|
| 1 | Open | always | folder: navigate; file: `FileOps.open_file` | `FB:874-886` |
| 2 | Open in Other Browser | dual-browser panes | the opposite pane navigates there and becomes active | `FB:1144-1149`, `DBV:242-245` |
| 3 | Open in Left Browser / Open in Right Browser | project-tab panes | `window.openPathInBrowser(side, path)` | `FB:1150-1169`, `Main:659-690` |
| 4 | Open in New Tab | always (a file opens its containing folder) | `Main.addFilesTab` | `FB:1051-1066` |
| 5 | Sync as Job… | folders | name dialog, then `Subscription.subscribe_to_job` | `FB:1173-1178`, `FB:1479-1515` |
| 6 | Cut / Copy | selection not empty | `FileOps.clipboard_cut_paths` / `clipboard_copy_paths` | `FB:1180-1181` |
| 7 | Paste | the clipboard holds paths (probed on each popup) | `clipboard_paste_intent`, then copy or move | `FB:1182`, `FB:1124-1137` |
| 8 | Rename | exactly one selected | rename dialog | `FB:1184` |
| 9 | Delete | selection not empty | confirm dialog, then `FileOps.delete_to_trash` | `FB:1185`, `FB:1653-1675` |
| 10 | Copy Path / Copy Filename / Copy ufb:// Link / Copy union:// Link | always; act on the row under the pointer only | `FileOps.clipboard_copy_text(...)` | `FB:953-984`, `FB:1187-1190` |
| 11 | Transcode to MP4 | the selection contains a video extension | `Transcode.add_jobs` | `FB:1192-1202`, `FB:410-415` |
| 12 | Extract Here / Extract N Archives Here | the selection contains an archive | `Archive.add_extract_jobs` | `FB:1209-1223` |
| 13 | Compress to ZIP / Compress N Items to ZIP | selection not empty | `Archive.add_compress_job` | `FB:1224-1235` |
| 14 | Pin Locally / Unpin (LucidLink) | the whole selection is on one such filespace | `Lucid.set_pinned` | `FB:1240-1261` |
| 15 | Copy LucidLink Link(s) / Create LucidLink Guest Link… | same; guest link for one item | `Lucid.direct_links`, `Lucid.create_guest_link` | `FB:1266-1302` |
| 16 | Reveal in Explorer | always | `FileOps.reveal_in_file_manager` | `FB:887-894`, `FB:1304` |
| 17 | Show Shell Menu (Windows) / Show Finder Menu (macOS) | always | section 4.3 | `FB:1076-1094`, `FB:1305` |
| 18 | New Web Link… | always | dialog, then `FileOps.create_web_link` | `FB:1310`, `FB:1424-1477` |

Not in the item menu: Duplicate, Open With, Get Info / Properties, Quick Look, New Folder with Selection, Make Alias /
Shortcut, Add to Bookmarks, Copy as Pathname variants, Share.

### 4.2 Background menu (`FB:1313-1336`) — also the tool strip's "+" button (`FB:2450-2459`)

New Folder…, New UFB Folder… (a date-prefixed, letter-suffixed name, `FB:1517-1558`), New Web Link…, New Date Folder
(`YYMMDD`, created at once, `FB:1005-1017`), New Time Folder (`HHMM`, `FB:1018-1030`), Add Note… (notes tabs only,
`FB:230-236`, `FB:1560-1603`), Paste, Copy Current Path, Copy ufb:// Link, Refresh.

In tree mode New Folder / Paste / the date and time folders target the selected tree folder, not the root
(`FB:543-557`); New UFB Folder and Add Note always use the pane's folder (`FB:1034`, `FB:1039`).

Not in the background menu: view mode, sort by, show hidden files, Select All, Open in Terminal, Properties, New File.

### 4.3 Native shell menu

| OS | What "Show … Menu" does | Source |
|---|---|---|
| Windows | hosts Explorer's real `IContextMenu` for **one path** (the row under the pointer, not the selection) on its own thread | `FB:1089-1092`, `bindings/src/services/file_ops.rs:148-154` |
| macOS | an app-built native menu for the whole selection from public APIs: Open, Open With, Show in Finder, Get Info, Share, Tags. Third-party Finder extensions and Quick Actions are not reachable | `FB:1082-1088`, `app/MacShellMenu.h:1-17` |

The shell menu is a second step (a row inside the app's menu), never the first menu.

### 4.4 Other menus

Sort menu (`FB:2603-2630`), tab menu (`Main:2376-2384`), sidebar row menus (section 1.4), tray menu (`Main:168-208`).

---

## 5. File operations as the UI drives them

| Operation | Entry points | UI flow | Backend call | Source |
|---|---|---|---|---|
| Copy / move by paste | shortcut, menus | none; lands in the pane's folder (tree: the selected folder) | `FileOps.copy_files` / `move_files` | `FB:915-932` |
| Copy / move by drag | sections 3.3, 3.4 | dialog inside a pane; none on the sidebar | same | `FB:726-765`, `SB:69-98` |
| Delete | Del, menu | confirm "Move N items to the recycle bin?" (the same words on macOS) | `FileOps.delete_to_trash` | `FB:1653-1675` |
| Permanent delete | **not found** | | | |
| Rename | F2, menu | dialog | `FileOps.rename_path` (synchronous, returns an error string) | `FB:1639-1650` |
| New folder | Ctrl/Cmd+Shift+N, menu, "+" | name dialog; the new folder is **not selected** afterwards and not put into rename | `FileOps.create_directory` (synchronous) | `FB:1379-1418` |
| Duplicate | **not found** (the macOS shell-menu header lists it as an existing app row, `app/MacShellMenu.h:15-16`; no such row exists) | | | |
| Compress | item menu | queued job; the result is selected when it lands in the shown folder | `Archive.add_compress_job` | `FB:1224-1235`, `FB:1973-1982` |
| Extract | item menu | queued job, extracts beside the archive | `Archive.add_extract_jobs` | `FB:1209-1223` |
| Transcode | item menu | queued job | `Transcode.add_jobs` | `FB:1192-1202` |
| New web link / note / date-named folders | menus | dialogs | `FileOps.create_web_link`, `create_date_prefixed_note`, `create_ufb_folder`, `find_available_path` | `FB:1005-1040` |
| New job from template | footer button inside a jobs folder | `NewJobDialog` (other reader) | — | `FB:3752-3763` |

### 5.1 Progress, cancel, conflicts, errors

- **Copy / move / delete** progress: a chip per running operation in the window's status strip (icon, done / total,
  a 60 px bar, percent) (`Main:2064-2122`). The chips have no mouse area: **no cancel, no pause, no detail, no click
  through**. The bridge exposes no cancel function for them (`bindings/src/services/file_ops.rs` — only one signal,
  `dirs_changed`, at line 295-296, and no `cancel` method).
- When the last operation ends, "N file(s) copied / moved / deleted" shows for 3.5 s (`Main:824-846`, `Main:809-813`).
  **A failed operation takes the same path**: the bridge logs the error and removes the op from the snapshot
  (`bindings/src/services/file_ops.rs:1182-1188`), so the strip reports success wording. No error dialog, no toast.
- **Name conflicts** (replace / skip / keep both, apply to all): no UI exists in the audited QML (no user-facing string
  about replace / overwrite / conflict / keep both in any browser file). What the backend does on a collision is the file-operations reader's finding.
- Synchronous creates and rename report failure with `console.warn` only (`FB:1411-1415`, `FB:1644-1648`, `FB:1013-1014`).
- **Queued jobs** (transcode, extract, compress): chips in the status strip (max three plus "+N"), failures as a red
  chip that stays until clicked (`Main:2123-2267`); the Tasks tab lists every job with a status pill, progress, detail,
  and Cancel / Open in Browser / Reveal / Remove buttons plus Clear Completed (`TQ:151-158`, `TQ:228-240`, `TQ:376-400`).
  Queuing a job never switches tabs (`Main:1552-1557`).
- The "Task Queue" therefore does **not** contain copy, move or delete.

### 5.2 Undo

**None.** No undo / redo code, shortcut or menu row exists for any operation.

### 5.3 Clipboard

- Cut / Copy put file paths on the OS clipboard; Paste reads paths plus a move-or-copy intent. Per the bridge's notes it
  honours Explorer's Cut and shares cut state across panes and tabs (`bindings/src/services/file_ops.rs:123-133`).
  Finder interop (macOS has no cut flag): not verified here — file-operations reader.
- Paste is enabled from a cheap "has paths" probe taken when the menu opens; the full read once hung the app on Windows
  while the clipboard's owner was busy (`FB:827-836`, `FB:1124-1137`).
- Cut items are not dimmed or marked in the view (no code for it).
- Pasting an image or text from the clipboard as a new file: **not found**.
- Text copies: path, file name, two link schemes, the current folder's path (section 4).

---

## 6. Preview (lightbox)

Space on a file opens a near-opaque overlay over the whole window, title row included (`PL:160-171`, `Main:2386-2392`).
The content loads asynchronously with a delayed spinner (`PL:173-212`); the path shows at the bottom (`PL:268-282`); a
close button sits clear of the OS window buttons (`PL:284-300`). Minimising or hiding the window closes it
(`Main:284-296`). It is a modal layer inside the window, not a separate floating panel.

| Type (by extension, `PL:19-68`) | Renderer | Notes |
|---|---|---|
| Video (mp4, mov, mkv, mxf, …) and animated GIF / WebP | `VideoPreview` | transport bar, frame step, shuttle, loop, mute, volume |
| Audio (wav, mp3, aiff, flac, …) | `AudioPreview` | same transport names |
| PDF and `.ai` | `PdfReader` | continuous scroll, pages rendered lazily and evicted (`PdfReader.qml:1-5`) |
| HTML | `HtmlPreview` (Chromium) when the optional web engine module is built in; otherwise the text renderer (`PL:40-44`) | links open in the system browser |
| minNotes documents, Word documents | rendered to a staged HTML page, same web view (`PL:45-62`) | |
| Text / code / extensionless text | `TextPreview`, the first 1 MB only (`TextPreview.qml:1-6`) | |
| Everything else | `ImagePreview`: full-resolution still (EXR, PSD, TIFF, JPEG, PNG, DPX…), falling back to the OS file icon | multi-layer EXR opens as a layer contact sheet; click a layer to drill in (`ImagePreview.qml:1-13`) |

- Folders: Space does nothing (`FB:3836-3840`). Multi-selection: previews the cursor item only.
- Zoom / pan in the still preview: **not found** (no wheel, pinch or drag handler in `ImagePreview.qml`).
- No inline preview pane or info panel beside the list; no metadata readout in the lightbox (not found).
- Opening the preview by double-click or from a menu: **not found** — Space only. A mouse-only user cannot reach it.

---

## 7. Search

- One field per pane in the tool strip (`FB:2461-2520`). Typing (200 ms debounce) starts a **recursive** search under
  the pane's current folder; results replace the listing; clearing the text returns to it (`FB:151-164`, `FB:361-391`).
- Two backends, toggled by the lightning button: the OS index (Everything's command-line tool on Windows, Spotlight's on
  macOS) or the app's own index; the spinner covers "indexing" and "searching" (`FB:361-366`, `FB:2522-2544`,
  `core/src/search.rs:17`). The toggle is not persisted (`FB:366`).
- Results: folders first, then by name; rows grow to 36 px to show the parent path under the name
  (`FB:1908-1950`, `FB:3001-3004`, `FB:3059-3069`). The header sort does not apply to results (`FB:1939-1944`).
- Result cap: 200 per pass (`core/src/search.rs:12`) — no "more results" affordance in the QML.
- Opening a folder from the results ends the search (`FB:346-349`). "Reveal in enclosing folder" for a file result:
  not found (Open in Other Browser / New Tab open the pane's CURRENT folder for a file, `FB:1057-1062`).
- In **tree** mode results are written to the list/grid model, which the tree does not show (`FB:1913-1917` vs
  `FB:3451`): searching in tree mode shows nothing (not verified at runtime; follows from the code).
- No name filter of the current folder, no kind / date / size criteria, no saved searches, no shortcut to focus the field.

---

## 8. Settings, dialogs, menus reachable from the browser

- **There is no Preferences window** in the audited files. Settings are spread over: the sidebar's Add Bookmark dialog
  (folder field, native folder picker, display name, a "jobs folder" checkbox — `AddBookmarkDialog.qml:55-60`,
  `AddBookmarkDialog.qml:131-145`), the Path swaps dialog (a table mapping a Windows path to a macOS path, with an on/off
  and a label per row — `PathSwapsDialog.qml:36-64`), the Mount manager and Mesh sync dialogs (other readers), the tray
  menu (Open, Mounts, Start at Login on Windows, Quit — `Main:168-208`) and "Check for Updates" in the status strip
  (macOS only, `Main:2319-2342`).
- Browser dialogs (all modal, centred in the PANE, standard OK / Cancel): New Folder, New Web Link, Sync as Job, New UFB
  Folder, Add Note, Rename, Delete, Move or Copy, a LucidLink error box, New Job (`FB:1339-1778`).
- Menu bar: none on macOS, none in-window on Windows (section 1.1).
- Status strip, left to right: "active: left|right", "N selected", operation chips, an agent state dot, the core
  version, the update link (`Main:2030-2343`). No item count, total size or free space.

---

## 9. State persistence

| What | Scope | Stored | Source |
|---|---|---|---|
| View mode, grid size, sort key + direction, Name and Size column widths | **per folder path**; an unvisited folder inherits the pane's current state | `Settings.folder_view_prefs(path)`, merged into the row, 500 ms debounce, flushed on leaving the folder | `FB:47-63`, `FB:2034-2101` |
| Open tabs: per Files tab both panes' paths and collapsed flags; job tabs; tracker / tasks open; the current tab | global | `Settings.set_tab_state` | `Main:298-336`, `Main:338-427` |
| Window size and maximized | global; **position is deliberately not restored** (mixed-DPI multi-monitor trouble) | `Settings.set_window_state` | `Main:236-254`, `Main:856-901` |
| Sidebar width; the dual browser's left width (one shared value) | global | `Settings` | `Main:438-453`, `DBV:196-203` |
| Project-tab pane paths per sub-tab | in-session memory only | `FTV:89-94` | |

Not persisted: navigation history, selection, scroll position, tree expansion (not found in the QML), the search-backend
toggle, the active pane, sidebar section collapse (sections do not collapse).

---

## 10. Rough spots

Ordered roughly by how much a file-browser user would feel them.

### 10.1 Missing or wrong compared with Finder / Explorer

1. **No undo** for anything (section 5.2).
2. **Copy / move / delete cannot be cancelled and failures look like success** (section 5.1;
   `Main:829-839`, `bindings/src/services/file_ops.rs:1182-1188`). No conflict dialog in the UI.
3. **No type-ahead**, no Page Up / Down, no keyboard back / forward, no keyboard pane switch, no tab shortcuts, no
   focus-search shortcut (section 2.2).
4. **macOS key mapping is Windows's**: the Mac delete key goes up a folder (`FB:3813-3815`), trash needs forward-delete
   (`FB:946`), Cmd+Backspace / Cmd+Up / Cmd+Down / Cmd+O / Cmd+[ are unbound.
5. **Windows wording on macOS**: "Reveal in Explorer" in the item menu and all three sidebar menus (`FB:889`, `SB:376`,
   `SB:500`, `SB:923`); "recycle bin" in the delete prompt (`FB:1666-1667`). Only the shell-menu row branches on the OS
   (`FB:1078`).
6. **Every Files tab is labelled "Files"** (`Main:1363`) — several browser tabs are indistinguishable; the folder is only
   in the window title and the header strip.
7. **First arrow press skips the first row**: after a navigation the cursor sits on row 0 with nothing selected
   (`FB:1881-1883`); Down computes `currentIndex + 1` and selects row 1 (`FB:3789-3790`).
8. **Rename is a dialog**, not in place; new folders are neither selected nor put into rename (`FB:1407-1417`).
9. **A plain click on one item of a multi-selection does not reduce the selection to it** — selection changes only on
   press, and a press on a selected row is ignored (`FB:3141-3145`); there is no on-release handling.
10. **No rubber-band selection, no spring-loading, no drag auto-scroll, no middle click** (section 3).
11. **Internal drags always ask "Move or Copy?"** and ignore modifier keys (`FB:1677-1681`), while the same drag onto a
    sidebar row asks nothing and DOES read modifiers and same-volume (`SB:85-89`). The pane's own header comment still
    describes the modifier scheme it no longer implements (`FB:11-15`).
12. **Dragging out can only copy** (`FB:1693`); external drops can only copy (`FB:752-759`).
13. **Sorting is not natural** and can be "unsorted" (`FB:170-198`); sort does nothing in tree mode (`FB:2148-2182`);
    search results ignore it (`FB:1939-1944`).
14. **Tree mode is a second-class citizen**: no Size / Modified columns (`FB:3584-3645`), no Select All (`FB:3894`), no
    Backspace-up, double-click a folder never navigates into it (`FB:3571-3577`), search results invisible (section 7),
    the status strip's "N selected" reads the list-mode array and shows nothing (`Main:2055-2057`).
15. **No show-hidden toggle, no Duplicate, no Open With / Properties in the first menu, no New File** (sections 1.8, 4).
16. **Mouse users cannot open the preview**; the still preview has no zoom (section 6).
17. **Errors go to the console**: create / rename / link / note failures are `console.warn` and the dialog just closes
    (`FB:1411-1415`, `FB:1469-1472`, `FB:1552-1555`, `FB:1594-1597`, `FB:1644-1648`).
18. **The Windows shell menu acts on one path**, not the selection (`FB:1089-1092`).
19. **New tabs open at Home in both panes** and the right pane's path is whatever it was seeded with (`DBV:59-64`).
20. **Window position is never restored** (`Main:856-876`).
21. The status strip shows no item count, sizes or free space (section 8).
22. Up is never disabled; going Up or Back does not reselect where you came from (section 1.6).

### 10.2 Workarounds for Qt behaviour (each is a cost the rebuild does not inherit)

| # | Quirk | Workaround | Source |
|---|---|---|---|
| 1 | Destroying a delegate that owns a live drag aborts the process | a persistent 1 x 1 drag proxy per pane; refreshes deferred during a drag | `FB:608-616`, `FB:1682-1703`, `FB:1795-1801` |
| 2 | Drag MIME binding went stale on repeat drags | imperative assignment before activation | `FB:626-632` |
| 3 | Drag image grab is async and may never arrive under software rendering | 80 ms fallback timer | `FB:1705-1721` |
| 4 | `drop.urls` truncated to one entry for native multi-file drags | parse the raw `text/uri-list` too (code duplicated in the sidebar) | `FB:686-720`, `SB:29-63` |
| 5 | No move events during Windows' modal drag loop | recompute the target at drop time | `FB:2939-2946` |
| 6 | Flickable steals presses and drags | `preventStealing` on rows; **three** layers of click handling per view (a z:-1 mouse area, a tap handler inside the view, a tap handler on the panel) coordinated by a `_menuClickConsumed` flag | `FB:803-810`, `FB:2968-2996`, `FB:3160-3176`, `FB:3682-3706` |
| 7 | Ambiguous shortcuts across panes silently disable all of them | shortcuts bound only while the pane is active and visible | `FB:39-45` |
| 8 | A hidden `GridView` still instantiates delegates, firing thumbnail decodes for a whole folder in list mode (out-of-memory on huge media) | thumbnail loader gated on grid mode | `FB:3305-3319` |
| 9 | A clamped menu with collapsing rows looped forever in layout — the "right-click hangs the app" report | `UfbMenu` instantiates every row (`cacheBuffer: 1000000`) | `UfbMenu.qml:9-27`, `UfbMenu.qml:60-61` |
| 10 | Reading the clipboard for the Paste row blocked on the clipboard's owner (Windows) | a cheap probe for enablement | `FB:827-836` |
| 11 | Menu-item `enabled` bound to a non-reactive call latched at creation | re-evaluate on every popup | `FB:1124-1137` |
| 12 | Live column-width writes re-flow every row | guide line during the drag, commit on release; hover handler because a lost leave event stuck the highlight | `FB:65-74`, `FB:2724-2733` |
| 13 | Image request size is not scaled by the device pixel ratio | multiply by hand | `FB:3325-3331` |
| 14 | An image reports the request size even when the provider declined | gate on `status === Ready` | `FB:3253-3266` |
| 15 | A `Loader` inside a `StackLayout` loads at 0 x 0 | tracker and task views are always instantiated | `Main:1978-1995` |
| 16 | The styled tab bars vanish on the dark background | a hand-built tab strip with an invisible item holding `currentIndex` | `Main:1224-1225`, `Main:1908-1917` |
| 17 | Rectangle borders inset layout and clip at splitter seams | a z:9999 overlay draws the focus / drop outline | `FB:2199-2206`, `FB:3967-3986` |
| 18 | An object-literal append followed by an IIFE parsed as a call | a "load-bearing" semicolon | `Main:376-383` |
| 19 | Refresh timing after operations | `dirs_changed` broadcast plus two leftover timers "as a safety net" | `FB:767-793` |

### 10.3 Structure

- `FileBrowser.qml` is one 3987-line component: model glue, selection, three views, drag and drop, two menus, ten
  dialogs, search, per-folder prefs and jobs-folder logic together.
- The three view delegates repeat the same press / drag / right-click code three times with small differences (e.g. the
  list delegate lacks the right-button guard the grid and tree have in `onPositionChanged`: `FB:3147-3155` vs
  `FB:3410-3418`, `FB:3556-3557`); the two column-resize handles are copies (`FB:2716-2761`, `FB:2814-2859`); the four
  tab pills in `Main.qml` are copies (`Main:1336-1433`, `Main:1440-1542`, `Main:1628-1729`, `Main:1734-1822`); the
  drop-path collector exists twice (`FB:691-720`, `SB:35-63`).
- Two selection models (index-based for list / grid, path-based for tree) with every command branching on the mode
  (`FB:527-562`). Index-based selection is why a rebuild needs the snapshot / resolve helper (`LS:1-56`).
- Data crosses the bridge as whole-folder JSON strings parsed in the UI thread (section 11).
- Internal names lag the product: the Tasks tab is still "transcode" in state keys and index arithmetic
  (`Main:1552-1557`); `AppController.h:12-19` documents an `agentBanner` strip that `Main.qml` does not show.
- Hard-coded colours beside the theme tokens (`#2a2a2a`, `#262626`, `#888`, `#cfd9e6`, `#dddddd` …:
  `FB:3017`, `FB:3096`, `FB:3237`, `FB:1668`).
- No `TODO` / `FIXME` / `HACK` markers in `FileBrowser.qml`; the debt is recorded as prose comments ("first cut",
  "deferred", "safety net", "good enough until someone hits a UNC edge case": `FB:796-801`, `FB:1171-1172`, `FB:767-772`,
  `FB:263-264`).
- Breadcrumbs do not understand UNC roots (`FB:263-264`): a network path splits into server and share crumbs that are
  not navigable folders.

---

## 11. Performance with large folders (as far as the QML shows)

- **Listing**: asynchronous on a worker; `loading` drives the spinner (`bindings/src/services/directory.rs:31-38`,
  `bindings/src/services/directory.rs:117-124`, `FB:2536-2544`). The result arrives as ONE JSON string for the whole
  folder, is parsed on the UI thread, filtered, sorted in script, and appended to a `ListModel` **row by row**
  (`FB:1836-1868`). No paging, no incremental delivery, no cancellation of a superseded listing visible in the QML.
- **Every refresh is clear + rebuild** of the model (`FB:1826`), which recycles all delegates; selection survives only
  through the snapshot helper, which builds a path map over the whole model (`LS:36-44`).
- **Selection cost**: `selectedIndexes` is a plain array; each visible delegate evaluates `indexOf` on it
  (`FB:483`, `FB:3005`); `_setSelection` de-duplicates with a quadratic filter (`FB:484-488`); Select All builds an
  N-element array (`FB:512-516`). The comment calls it "small enough that linear search is fine" (`FB:459-460`).
- **Views are virtualised** by Qt's `ListView` / `GridView` (delegates only for visible rows). Tree mode is a flat list
  rebuilt in full on every expand / collapse (`FB:2148-2182`); lookups by path are linear scans (`FB:2126-2131`).
- **Thumbnails**: asynchronous image provider, grid mode only, gated per file by a "may have a thumbnail" probe; the
  backend declines very large images (`FB:3262-3264`, `FB:3305-3332`). Icons are per-extension OS icons from an async
  provider (`FileTypeIcon.qml:4-14`).
- **File watching: none.** No watcher exists in the app, bindings or core (grep for the usual watcher APIs hits only the
  mesh coordinator). A pane refreshes on: the app's own operations (`FB:1983-2002`), the app regaining focus — visible
  Files tab only (`Main:526-540`), F5 / the refresh buttons, and the fallback timers. A change made by another program
  while UFB stays in front is not seen until one of those.
- **Search**: capped at 200 results, debounced 200 ms, full rebuild per result batch (`FB:403-408`, `FB:1913-1950`).

---

## 12. min-ui: what a file browser needs

From `crates/min-ui/src` in this repository (names only).

**Has** (usable as is or as a base): `ActionButton`, `ActionGroup`, `TextButton`, `Tooltips`, `TextField`, `TextArea`,
`NumberField`, `Checkbox`, `Switch`, `Slider`, `Picker`, `Popover`, `Menu` (context menu), `MenuBar` (in-window, with
submenus and mnemonics), `Dialog` (with text fields), `Toast`, `Progress`, `ScrollBar` (overlay), `Rail` (a collapsible,
resizable side rail with a splitter), `Section` (collapsible header), `ListColumn` (a single-select scrolling list with
section headers, hover, right-click report; paints from the first visible row), `ColorPicker`, `Scrubber`, `UiLayer`,
`Fonts`, `anim` ramps, pointer / key / IME event types.

**Lacks** (to be added to min-ui, not hand-drawn in the app):

- A virtualised multi-column **table / details view**: header row, sortable and resizable (and reorderable) columns,
  multi-selection with anchor and cursor, zebra rows, in-place rename cell.
- A virtualised **icon grid view** with thumbnails and a variable cell size.
- A **tree / outline view** (disclosure, indent, lazy children) — and, if ruled in, Miller columns.
- **Multi-selection** in any list (`ListColumn` holds one selected row), rubber-band selection, type-ahead.
- A **path bar**: breadcrumbs with overflow, crumb menus, edit mode with completion.
- **Tabs**: a tab strip with close, overflow, reorder, drop targets.
- A general **split pane** between two arbitrary views (`Rail` is edge-anchored only).
- **Drag and drop**: an OS drag source (file URLs, drag image with a count badge, copy / move / link negotiation) and
  drop targets with hover feedback, spring-loading and auto-scroll; in-app drags between widgets.
- **Input vocabulary**: wheel / scroll, double-click, middle and back / forward buttons, Page Up / Down, function keys,
  Space as a command, and Ctrl/Alt as separate modifiers are not in `input.rs`'s `Event` / `Named` / `KeyEvent` today.
- A **sidebar source list** (icon + label rows, sections, drop targets, reorder, badges, state dots).
- A **status bar**, a **segmented control**, a **search field** (icon, clear button, scope / mode toggle), a
  **busy spinner**, an **error banner**, an **empty-state** placeholder.
- A **task / job list row** (status pill, progress, actions) and status-strip chips.
- A **lightbox / overlay host** and the viewers behind it (image with zoom and pan, PDF pages, text, media transport).
- File-type **icon** and **thumbnail** image nodes with async loading and caching.
- **Native shell menu** hand-off (Windows `IContextMenu`, the macOS service menu) and clipboard file-list access —
  platform services rather than widgets, listed here because the browser cannot ship without them.

---

## 13. Touch points with other readers' areas

| Area | Where the browser touches it |
|---|---|
| File operations (Rust) | every `FileOps.*` call in sections 4-5; `dirs_changed`; `active_ops_json`; clipboard; URI / path conversion |
| Projects / metadata / tracker | "jobs folder" detection and the Synced column (`FB:76-149`), New Job, Sync as Job, `FTV` hosting one to three panes beside the item list (`FTV:321-354`, `FTV:388-400`, `FTV:476-484`), deep-link routing into job tabs (`Main:1151-1183`) |
| Mounts | sidebar rows, tray menu, jobs-folder lookup via mount state (`FB:100-114`) |
| Mesh sync | sidebar refresh hooks (`SB:243-263`) |
| Media | thumbnails, the lightbox renderers, Transcode jobs |
| LucidLink | menu rows via `Lucid.*` (`FB:1132-1142`, `FB:1236-1302`) |
| Search core | `Search` object, two backends, the 200 cap |
| Deep links | `ufb://` and `union://` schemes, single-instance forwarding, select-after-load (`Main:542-627`, `app/AppController.h:58-63`) |

---

## Open questions for the owner

1. **Dual pane**: keep the two-pane-per-tab model as the default, or one pane per tab with an optional split? Should a new
   tab still start with the right pane collapsed and both panes at Home?
2. **Tree vs columns**: is the tree view (rooted at the current folder) used in practice, or would Miller columns / an
   expandable details view replace it? Today it is the least finished mode.
3. **Drag semantics**: replace the "Move or Copy?" prompt with the platform rules (same volume = move, other volume =
   copy, modifier overrides, a badge on the cursor) everywhere, including drags out to Finder / Explorer? Was the prompt
   a safety decision for shared storage that must stay?
4. **Per-OS keys**: adopt Finder's keys on macOS (Return = rename, Cmd+Down = open, Cmd+Backspace = trash, Cmd+Up =
   parent) or keep one Explorer-style map on both systems?
5. **Undo**: which operations must be undoable in the first release (rename, move, trash, new folder)?
6. **Operation feedback**: should copy / move / delete join the task queue (cancel, pause, per-file errors, conflict
   resolution with "apply to all"), or stay as status chips?
7. **Per-folder view memory**: keep it (Finder's way), or one setting per pane / per tab with an optional per-folder
   override?
8. **Hidden files**: a toggle, and what is the default for dotfiles and OS-hidden entries?
9. **Search**: should the field first filter the current folder and only go recursive on request? Is the 200-result cap
   acceptable? Does the OS-index / own-index toggle need to be a user-facing choice?
10. **Live updates**: is a file watcher wanted on network shares and mounts (cost and reliability there), or refresh on
    focus plus own operations as today?
11. **Tabs**: name each tab after its folder? Reorder, duplicate, reopen closed tab, drop-on-tab?
12. **Shell integration**: must Explorer's full context menu stay reachable on Windows (third-party handlers), and is the
    app-built Finder-style menu on macOS enough?
13. **Menu bar**: add one (native on macOS, in-window on Windows, as the family's other apps do) so commands are
    discoverable and carry shortcuts?
14. **Preview**: keep the full-window lightbox, add an optional side preview / info pane, or both? Zoom and pan for
    stills?
15. **Columns**: which extra columns matter for the work (kind, created, dimensions, duration, frame range of a
    sequence)? Should image sequences collapse into one row?
16. **The date / time / "UFB" folder creators and web links**: all four stay as first-level menu rows?
17. **Sidebar**: add system locations (volumes, home, recent, trash), bookmark reordering and drag-to-add?
