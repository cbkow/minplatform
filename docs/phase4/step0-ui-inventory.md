# Phase 4, step 0 — minRender: the UI inventory and the widget gap

Written 2026-11-14 from a read of the minRender working tree (branch `min-paths`, release 0.5.11 plus a small change) and of
this repository. Nothing was built or run. Paths under `src/…` and `docs/…` in sections 1, 2, 5 and 6 are relative to the
minRender repository; paths under `crates/…` and `apps/…` are relative to this one.

The port is a parity port ("with less creativity"): what farm operators use today, on `min-ui`.

Three findings that change the brief before anything else:

- **There are no monitor graphs.** `src/monitor` (21 files) is the farm coordinator's backend (`MonitorApp`, the database,
  dispatch, peers, the render coordinator, templates, the submission watcher), not machine statistics. Nothing samples CPU,
  GPU, memory, temperature or network load, and nothing draws a line graph. The only machine facts shown are static: host
  name, node id, GPU name, CPU core count, RAM size. `src/ui/painters` holds ONE painter, the frame grid.
- **There is no first-run flow and no template editor.** A node is pointed at the shared folder in Settings; templates are
  JSON files edited by hand in the shared folder.
- **The whole UI is one window** with modal cards over it, plus a tray icon. No second top-level window exists.

---

## 1. Inventory

### 1.1 Window, chrome, process behaviour

| Piece | What it is today | Source |
|---|---|---|
| Main window | 1440 × 900 default, title "minRender Monitor" (task bar / Dock label only) | `src/ui/qml/Main.qml` |
| Chrome, macOS | Chromeless: content under a transparent title bar, native traffic lights kept, 70 px inset on the left of the title row (0 in fullscreen), AppKit's title text hidden. The title row drags the window (after 6 px of movement, so clicks stay clicks) and a double-click zooms (maximise / restore). `--native-frame` keeps the OS title bar (dev aid). | `src/ui/chrome/*`, `components/DragMouseArea.qml` |
| Chrome, Windows | Native frame and title bar, forced DARK whatever the OS scheme; the title row is an ordinary toolbar under it | `src/monitor/main_qt.cpp`, `WindowChrome.h` |
| Colour | Surface pinned to sRGB (so tokens are not oversaturated on P3 displays) | `main_qt.cpp`, `src/ui/platform/color_space*` |
| Close | Closing the window HIDES it; the app keeps running (tray). | `Main.qml` `onClosing` |
| Start hidden | `--minimized` starts with the window hidden (used by the Windows startup task and the restart helper) | `main_qt.cpp` |
| Single instance | A second launch signals the first (which shows, raises and activates its window) and exits | `src/core/single_instance.*` |
| Dock (macOS) | Clicking the Dock icon while the window is hidden shows it again | `main_qt.cpp` |
| Session end (Windows) | A hidden top-level window receives the end-of-session message (Store update, sign-out) and force-exits, aborting the current render; the app registers to be restarted with `--minimized` after servicing | `src/ui/platform/app_restart.*` |
| Pump | A 50 ms timer runs the backend's `update()`, `AppBridge::refresh()`, the tray labels, and quits when the backend says so | `main_qt.cpp` |
| Persisted UI state | Split sizes, panel visibility, active bottom tab, column widths, grid height, preview height (OS settings store, UI state only) | `Settings {}` blocks in the QML |
| Fonts / icons | Geist, Geist Mono, Phosphor (Material Symbols bundled as an unused fallback) | `main_qt.cpp`, `Theme.qml` |
| Accent | One fixed blue `#0189f1`; the OS accent reader exists (`accent_color.cpp`) but the theme does not use it | `Theme.qml` |

### 1.2 Tray / menu-bar icon

`src/ui/platform/tray.cpp` (Qt `QSystemTrayIcon`). macOS: a template (auto-tinted) image; Windows: the colour icon.

| Item | Does |
|---|---|
| Tooltip | "minRender - Rendering <job id>" / "minRender - Stopped" / "minRender - No Farm" / "minRender - Idle" (refreshed every 50 ms) |
| Click or double-click on the icon | Show, raise and activate the window |
| "Show Window" | The same |
| "Stop" / "Resume" (label follows the node's state) | Toggles this node between Active and Stopped |
| separator | |
| "Exit" | `MonitorApp::requestExit()`: quits at once when idle; while a render runs it waits for the render to end, with no message |

A status text ("Rendering <chunk>", "Stopped", "No Farm", "Idle") is computed and handed to the tray but shown nowhere.
A status-coloured icon is noted in the code as deferred.

### 1.3 Menus

Actions are shared by the macOS native menu bar and the Windows app menu (a hamburger button at the left of the title row,
one flat list with separators).

| Menu (macOS) | Item | Shortcut | Enabled when | Does |
|---|---|---|---|---|
| File | "Settings…" | | always | opens Settings |
| File | "Farm Cleanup…" | | always | opens Farm Cleanup |
| File | "E&xit" | | always | `Qt.quit()` (immediate) |
| Jobs | "New Job…" | | always | `submissionMode = true` |
| Jobs | "Select All Jobs" | Cmd/Ctrl+A | the job list has keyboard focus | selects every job |
| Jobs | "Edit Job…" | | exactly one job selected and the leader supports job editing | `openJobEditor` |
| Jobs | "Pause" / "Resume" | | at least one job selected | per selected job |
| Jobs | "Cancel…" / "Delete…" | | at least one job selected | the confirm card |
| View | "Node Panel" (checkable) | | | shows / hides the left panel |
| View | "Job List" (checkable) | | | shows / hides the job list |
| About | "minRender v%1" | | never (a label) | |
| About | "Check for &Updates…" | | item removed when no updater is compiled in (always on Windows) | Sparkle's check |
| About | "Docs" | | always | opens the documentation site in the browser |

Windows app menu order: Settings…, Farm Cleanup…, —, New Job…, Edit Job…, Pause, Resume, Cancel…, Delete…, —, Node Panel,
Job List, —, minRender v%1, (Check for Updates… hidden), Docs, —, Exit. "Select All Jobs" is NOT in it (the shortcut still
works).

### 1.4 Title row

Left to right: (macOS) the traffic-light inset · (Windows) the hamburger, 30 px wide, tinted while its menu is open · this
node's host name, bold, elided at 320 px · the "LEADER" badge when this node leads · an 8 px state dot with a tooltip —
green "Accepting work", grey "Stopped — not accepting work", red "Suspended by the leader" · empty drag room · the primary
button "New Job" (plus icon), always reachable.

### 1.5 Layout

A horizontal split: the Node panel (default 28 % of the width, minimum 260) | the right side. The right side is a vertical
split: the Job list (default a third of the height, minimum 120) over a tabbed pane with two tabs, "Job Detail" and "Logs"
(text tabs, a 2 px accent underline on the active one; both panels stay alive when hidden, so their timers keep running).
Split positions, the active tab and the two panels' visibility persist.

### 1.6 Node panel (`panels/NodePanel.qml`)

**This-node card.** A two-column list: "Host", "Node ID" (mono, elided in the middle), "GPU" (or "—"), "CPU / RAM"
("%1 cores · %2" with "x.x GB" / "n MB" / "unknown"), "Tags" (or "—"). Below it one full-width button: "Stop" (pause icon)
while active, "Resume" (play icon, primary) while stopped → `toggleNodeActive()`.

**Suspension banner** (only while the leader has benched this node): warning icon, "SUSPENDED", the text "The leader benched
this node after %1 chunk failures within 5 minutes. It will not be given work until unsuspended, and the suspension does not
clear on its own.", and a danger button "Unsuspend this node" → `unsuspendThisNode()` (proxied to the leader).

**Peers.** Header "Peers" with the subtitle "%1 online" (the count is of ALL listed peers). Rows 52 px, striped:

- a 10 px dot: red when suspended, grey when offline, else by agent health — green `ok`, amber `reconnecting`, red
  `needs_attention`;
- line 1: the host name (the node id when empty; muted when offline), "LEADER" in accent when it leads, a red warning icon
  when suspended;
- line 2: "suspended · %1 failures · no work assigned" / "offline" / "rendering · %1" (the job id) / "idle" / "stopped".

Selection: the shared list selection (1.12), keyed by node id; it only feeds the context menu.

Context menu, live nodes (dead ones in a mixed selection are skipped): a disabled header (the node id in mono, or
"%1 nodes"), —, "Stop", "Start", "Restart App", "Unsuspend"; with several targets each gets " (%1)" with the count it
applies to; items that apply to none are disabled. Calls: `setPeerNodeActive(id, bool)`, `restartPeerApp(id)`,
`unsuspendNode(id)`.

Context menu, an all-offline selection: header (the node id or "%1 offline nodes"), —, "Write restart signal (filesystem)"
→ `writePeerRestartSignal(id)`.

**Preview** (only when the build has the image libraries and a job is selected): a header "Preview" + "· <job id>", then
the preview pane at a fixed 240 px — see 1.9.

### 1.7 Job list (`panels/JobListPanel.qml`)

**Strip** (always present): "Jobs", "· <count>" and, with a selection, " · %1 selected"; then buttons, each disabled with
no selection: "Edit" (one job only and a capable leader; tooltip otherwise "The leader is running an older version without
job editing"), "Pause", "Resume", "Retry failed", "Requeue", "Archive", a gap, "Cancel", "Delete" (danger). Each loops the
single-job call over the selection, then clears the selection.

**Columns** (fixed widths, left aligned, no fill column; each header has an 8 px grip that resizes, a double-click on the
grip restores the default; widths persist):

| Column | Default / min px | Cell |
|---|---|---|
| (handle) | 22, fixed | six-dot drag handle |
| "Pri" | 36 / 28 | the priority, mono 11 |
| "Name" | 300 / 100 | the job id, mono, elided in the middle |
| "State" | 90 / 50 | bold 11, coloured: active green, paused amber, cancelled red, completed light blue, "partial" amber (a completed job with stopped chunks; tooltip "%1 stopped chunk(s) excluded"), others grey |
| "Progress" | 280 / 80 | a bar and "%1/%2" (done / total chunks) |
| "Duration" | 80 / 50 | first assignment → last completion; ticks every second while active; "—" when not started |
| "Created" | 140 / 60 | `yyyy-MM-dd HH:mm` |

Rows are 32 px, striped; selected rows take the selection fill; the row whose detail is showing carries a 3 px accent rule
at its left edge while anything is selected.

**Order.** The list is the dispatch order as the leader stores it (priority ascending, then the queue's sort key, then the
submission time). There is no user sort and no filter. Archived jobs are not listed.

**Mouse and keys.** The shared list selection (1.12). Double-click or Return on a single selection opens Edit Job (leader
permitting). Delete, or Cmd/Ctrl+Backspace, opens the Delete confirm. A click on the empty area under the rows clears the
selection. The cursor row drives the detail panel (`currentJobId`); a job made current elsewhere (a submission, an edit)
moves the cursor.

**Drag to reorder.** Pressing the handle selects that row alone, pauses the model's refreshes and ghosts the row (45 %
opacity; open / closed hand cursor). A 2 px accent line shows the gap; a drop into its own slot shows none. On release:
`moveJob(id, target, before)` — before the row under the line, or after the last row. When the target is in another
priority group the moved job ADOPTS the target's priority. The row does not move under the pointer; the next refresh shows
the new order. The list does not auto-scroll during a drag.

**Context menu** (a right-click inside a multi-selection keeps it, outside selects the row): "Pause", "Resume", —,
"Edit Job…" (or "Edit Job… (needs leader upgrade)"), —, "Retry failed chunks", "Requeue", —, "Cancel", "Archive", "Delete".
With several jobs each label gets " %1 jobs". For one job Pause is enabled only when it is active and Resume only when
paused.

**Confirm card** (Cancel and Delete always confirm): heading "Cancel %1 job%2?" with the body "This stops dispatch and
immediately kills in-flight renders on all nodes. The jobs can be requeued later." and the buttons "Cancel" / "Cancel jobs"
(danger); or "Delete %1 job%2?" with "Removes job records and SQLite history. Output files on disk are NOT deleted. This
cannot be undone." and "Cancel" / "Delete" (danger). No close on Escape or outside click.

### 1.8 Job Detail tab (`panels/JobDetailPanel.qml`)

**Empty** (no job selected): "No job selected", "Pick one in the Jobs list, or click New Job.", a primary "New Job" button.

**Header.** The job id (mono, bold, elided in the middle) and a state badge (upper-cased, coloured as the list's states);
a progress bar with "%1 / %2"; a line with "● %1 rendering" (when > 0), "● %1 failed" (when > 0), "priority" and a number
field 1–999 (editable; `setJobPriority` on every change; it follows the backend only while not focused), and the creation
time at the right. Then the action row: "Pause" (when active) or "Resume" (primary, when paused), "Retry failed" (enabled
with failed chunks), "Requeue", "Open output" (tooltip "Reveal the job's render folder in the file manager. Uses Path
Mappings to translate canonical paths."), a gap, "Cancel", "Delete" (danger). These two do NOT confirm here.

**Frame grid** (the one custom painter, `src/ui/painters/frame_grid.*`), in a vertical split above the chunk table
(default 220 px, minimum 100, persisted; the table keeps at least 120):

- one 10 px cell per frame of the job's range, wrapped to the width, a 1 px gap round each; taller ranges scroll;
- colours: background `#0d0d0d`; unclaimed / pending `#303030`; assigned or rendering = accent; a frame finished inside a
  chunk that is not yet complete `#2A8228`; a completed chunk `#46C846`; failed = the error red; a stopped chunk's frames
  are painted in the background colour (they read as absent);
- a 1 px white outline on the pinned frame (single-frame pins only);
- hover tooltip (300 ms): "Frame %1", then "Chunk %1  (%2-%3)", the chunk's state and "  ·  <node>" when assigned;
- click: pin the preview to that frame (`pinPreview(f, f)`; refused silently when the file is not on disk yet);
- repainted only when the chunk model or a property changes, never on a timer.

**Chunk table.** Columns (resizable, persisted): "State" 80, "Frames" 120 ("12" or "12-20"), "Node" 160 (the host name,
"—" when unassigned; tooltip = the raw node id), "Progress" 120 (a bar), "Duration" 70 (completed chunks only), "Retries"
60 (amber when > 0). Rows 24 px, striped; a stopped chunk's row is at 45 % opacity. States shown as text: pending, assigned,
rendering, completed, failed, stopped.

Selection: the shared list selection keyed by chunk id. A click, or moving the cursor with the keys, pins the preview to
that chunk's range (the newest frame of it on disk).

Context menu on the selection: "Reassign to another node" (or "Requeue (resume stopped chunk)" when every selected chunk is
stopped) → `reassignChunk(id, "")`; "Stop chunk" / "Stop %1 chunk(s)" / "Stop chunk (needs leader upgrade)" → `stopChunk`
on the pending and assigned ones; —; "Submit as separate job…" / "Submit frames %1–%2 as separate job…" →
`openChunkResubmitEditor(job, start, end)`.

### 1.9 Preview pane (`panels/JobPreview.qml`)

- Shows the newest rendered frame of the selected job, found by matching the output folder (translated through the path
  mappings) against the output flag's `#`-padded pattern; asked again every 3 s while visible.
- States: "No job selected"; "No frames rendered yet"; a spinner while the picture decodes; the picture, fitted, aspect
  kept, requested at 960 px wide.
- Footer: when pinned, an amber tag "◉ f%1" or "◉ %1-%2"; the file's name and, when a layer is chosen, "  ·  <layer>"
  (the word "Preview" when nothing is found); "Latest" (only while pinned → `clearPreviewPin()`); "Layers" (only for EXR).
- A pin returns to the latest frame by itself 60 s after the last pin action, and on a change of job.
- **EXR layers card** (modal, 560 wide): the title "EXR layers", a two-column grid of tiles (168 px tall, each a 240 px
  thumbnail of that layer with its name under it, the chosen one outlined in accent). A click chooses the layer for the
  preview and closes the card; the choice is remembered per job for the session only.

### 1.10 Logs tab (`panels/LogPanel.qml`)

Toolbar: a source dropdown (220 px) · level chips "Info", "Warn", "Error" (toggle; coloured when on; disabled for remote
sources) · a checkbox "Autoscroll" · a count ("%1 entries" / "%1 lines" / "%1 chunks") · "Show in Explorer" / "Show in
Finder" (tooltip "Reveal the current log file for use with other tools"; disabled in Task Output until a chunk is picked)
→ `revealCurrentLog()` · "Clear" (local log only) → `logModel.clear()`.

Sources: "Monitor Log", "Task Output", "<host> (this node)", then every live peer by host name.

| Source | View | Refresh |
|---|---|---|
| Monitor Log | Rows of time (`HH:mm:ss.zzz`, 84 px), level (48, coloured, bold), category (120, elided), message (wraps). Striped. Rows of a level that is switched off collapse to zero height. | Pushed live from the logger; a ring of 5000 |
| A node | Plain lines, wrapped; a line containing "[ERROR]" is red, "[WARN]" amber. Empty: "No log data for this node yet" or "Farm not running". | The last 500 lines of that node's log file on the share, every 3 s |
| Task Output | A split: on the left (220 px, min 140) the selected job's chunk log files, labelled "f<range>  HH:MM:SS", tooltip "<node id> — <range>", single selection, Up / Down / Home / End; on the right the chosen file's lines, coloured as above. Empty states: "Select a job in the Jobs list to view its task output.", "This job hasn't written any chunk output yet. Output appears once a node starts rendering a chunk of this job.", "Select a chunk on the left", "(Empty)". | The folder and the chosen file are re-read every 3 s |

Autoscroll: each view stays pinned to its end while the user is at the end; scrolling away unpins, returning to the end
pins again. The text cannot be selected or copied.

Failures of "Show in …" come as toasts: "Farm not running — no log files available", "No log file on disk for this source".

### 1.11 Modal cards

All are a raised card over a 55 % black scrim, with a title strip; they fade in over 80 ms.

**Settings** (`panels/SettingsPanel.qml`, 600 wide; closes only by its buttons). Edits change the configuration in memory;
"Save" writes it, "Cancel" restores the snapshot.

| Section | Control | Notes |
|---|---|---|
| "Sync root" | A text field (placeholder "e.g. \\\\server\\share\\minRender") and "Browse" (a native folder picker titled "Select sync root") | At the right of the heading: "✓ path OK" (green) or "⚠ path not reachable" (amber), when not empty |
| "Tags (comma-separated)" | A text field (placeholder "gpu, gpu-fast, workstation") | |
| "Networking" | "HTTP port" (number 1–65535); "IP override" (text, placeholder "auto-detect"); a checkbox "UDP multicast heartbeat"; "UDP port" (number, disabled while the box is off) | |
| "Farm security" | "Secret fingerprint" and its value in mono, or "not loaded" (amber) | Then ONE of: "No farm secret loaded — this node will reject all API calls. Check that farm.json is readable." / "%n node(s) on this farm report a different secret and cannot exchange work with this one." / "Every node reading the same farm.json shares this secret. Anyone with read access to the sync root can control the farm." |
| "Rendering" | A checkbox "Enable staging" | |
| "Path mappings" | "Map share roots between Windows, macOS, and Linux for cross-OS dispatch." and a button "Edit" | opens Path Mappings |
| "User interface" | A checkbox "Show notifications" | see section 6, findings |
| Footer | "Cancel", "Save" (primary, check icon) | Saving a changed sync root, HTTP port, IP override or UDP setting restarts the farm connection |

**Path Mappings** (`panels/PathMappingsDialog.qml`, 820 wide). The text "minRender stores paths in canonical Windows form.
Mappings translate between Windows, macOS, and Linux roots when jobs cross OSes or when a macOS user submits to the Windows
farm."; a header row "On", "Label", "Windows", "macOS", "Linux"; rows of a checkbox, a label field (placeholder "e.g. Render
share"), three mono path fields (placeholders "Z:\\renders", "/Volumes/renders", "/mnt/renders") and a trash button
(tooltip "Remove"); the empty state "No mappings yet — click Add Mapping below."; the footer "Add Mapping" (primary),
"Cancel", "Save" (primary). It edits a copy; Save drops wholly empty rows and writes the configuration at once (it does not
wait for Settings' Save).

**New Job / Edit Job / Resubmit** — one form, `panels/SubmissionForm.qml`, in three cards: "New Job" (640 wide),
"Edit Job — %1" and "Resubmit chunk as new job — %1". They close only by their buttons; the form is rebuilt on every open.

| Section | New Job | Edit Job | Resubmit |
|---|---|---|---|
| "Template" | A dropdown of the templates (plug-in templates left out); nothing chosen at first | The template's name as text; "%1 (template missing — editing raw values)" in amber when it is gone | as Edit |
| "Job name" | A text field (placeholder "e.g. shot_010_v003") | Read-only (the job id) | Editable, pre-filled "<job>_f<start>-<end>" |
| "Range" | "Start", "End" (0–1 000 000), "Chunk" (1–10 000), "Priority" (1–999); the template's defaults fill them | The job's values | The chunk's range; chunk size = the whole range |
| "Job settings" | — | "Max retries" (0–99), "Timeout (s, 0 = none)" | — |
| "Flags" | generated, see below | the stored values | the stored values |
| Error banner | The failure's text in a red box, inside the scrolled form | | |
| Footer | "Cancel", a spinner while busy, "Submit" (primary; needs a name and a template) | "Cancel", spinner, "Apply to next chunks" (primary), "Stop chunks && apply", "Start over" (danger) | "Cancel", spinner, "Submit as new job" |

*How the fields are generated.* The template's `flags` array is walked in order. A flag is shown only when it is `editable`
or `required`. Each shown flag is one row: a 140 px label (its `info`, else the flag text, else "(positional)"; amber when
required), a text field with the default value (read-only when not editable), and — when `type` is `"file"` — a "Browse"
button opening a native file picker filtered to "<filter> (*.<filter>)" plus "All files (*)", started at the current
value's folder. A `help` text shows small under the row. Every flag is a text field; no other field type exists. On macOS
the default values that are paths are shown translated through the path mappings and turned back on submit. When the
template of an edited job is gone, the stored flags are shown raw, all editable.

*Edit rules.* Changing the range or the chunk size shows "Frame range / chunk size changed — only Start over can apply
this." and disables the first two apply buttons. Tooltips: "In-flight chunks finish with the old settings; every chunk
dispatched from now on uses the new ones." / "Kills in-flight renders and requeues them with the new settings. Completed
chunks keep their output." / "Discards ALL progress — every chunk, including completed frames, re-renders with the new
settings." The last two confirm first: "Stop in-flight chunks?" — "In-flight renders are killed and requeued with the new
settings. Chunks that already completed keep the output they rendered with the old settings."; "Start over?" — "All chunks —
including completed frames — are reset and re-rendered with the new settings. In-flight renders are killed immediately."
(OK / Cancel).

*Outcomes.* A submission succeeds → the card closes and the new job becomes current. A failure stays in the banner. Texts
produced by the bridge (`src/ui/app_bridge.cpp`): "Backend not ready", "Unknown template: %1", "Failed to generate job
slug", "Leader did not respond", "Invalid frame range or chunk size", "No source manifest to resubmit from", "Stored
manifest is invalid: %1", "Job changed while editing — reopen the editor", "No job is being edited", "Frame start must be
<= frame end", "Chunk size must be at least 1", "Template no longer available: %1", "Template changed while editing —
reopen the editor", "Required flag is empty: %1", "Template has no executable for this OS", "The leader is running an older
version without job editing — update the leader node", "No response from the leader — check the farm connection and try
again", "The job no longer exists on the leader", "The leader's database isn't ready — try again shortly", "Edit rejected:
%1", "Leader error: %1", plus the template validator's own messages.

**Farm Cleanup** (`panels/FarmCleanupDialog.qml`, 720 wide). It scans on opening, off the UI thread, with a spinner. Top
line: "You are the leader — DB-backed sections are populated." or "You are a worker — finished/archived jobs need the leader
to scan."; "Rescan". Six sections, each a card with a three-state select-all box (tooltip "Select all in this section"),
"<title> · <count>", an action button (enabled with a ticked row and no scan running), and rows of a checkbox, a label
(mono) and a detail:

| Section | Action | Empty text |
|---|---|---|
| "Finished jobs (completed / cancelled)" | "Archive selected" | "No finished jobs." / "Worker can't list finished jobs — only the leader has DB access." |
| "Archived jobs" | "Delete selected" | "No archived jobs." / "Worker can't list archived jobs." |
| "Orphaned job directories (no matching record)" | "Delete selected" | "No orphaned job directories." |
| "Offline peers" | "Remove selected" | "No offline peers." |
| "Empty staging directories" | "Delete selected" | "No empty staging directories." |
| "Non-empty staging copies (failed or in-progress)" | "Delete selected" | "No lingering staging copies." |

Each action runs off the UI thread and rescans. Footer: "Close". No confirmation before a deletion.

### 1.12 Shared behaviours

**List selection** (`components/ListSelection.qml`; jobs, chunks, peers). Keys are stable identities, so a selection
survives the model's refreshes.

| Gesture | Effect |
|---|---|
| Click | Select only this row; it becomes the anchor and the cursor |
| Cmd / Ctrl-click | Toggle this row; it becomes the anchor |
| Shift-click | The range from the anchor to this row |
| Right-click | Keep a selection that contains the row, else select only it; then the menu |
| Up / Down / Home / End / PageUp / PageDown | Move the cursor and select it; with Shift, extend from the anchor; the row is scrolled into view |
| Cmd / Ctrl+A | Select all |
| Escape | Clear the selection (the cursor stays) |
| Return / Enter | Activate the cursor row (jobs: Edit Job) |

**Toast** (`Main.qml`). One at a time, bottom centre, 24 px up, wraps; a red border for an error, accent for information;
6 s; a click dismisses it; a new message restarts the clock. Sources: a failed job action (texts in 1.10 and "The stored job
manifest is invalid", "Leader unreachable — check the farm connection", "The job no longer exists on the leader", "Couldn't
fetch the job manifest: %1"); "Leader unreachable — edit queued to the farm and applies when the leader reconnects";
"Edit applied to %1".

**Tooltips.** 600 ms delay (400 or 300 on some cells), 5 s timeout, flat, wrapped.

**Buttons** (`components/FlatButton.qml`): variants default (no fill at rest), primary (accent), danger (red), neutral
(grey); an icon, a label or both; checkable with an accent border, or "subtle" (colour only — the log chips).

**Scroll bars**: an 8 px thumb with no track, shown only when the content overflows; rows keep a gutter for it.

**Updater.** macOS: Sparkle, started after launch (daily check from the bundle's settings) and "Check for Updates…";
Sparkle's own windows. Windows: none (the Store updates the app).

**Notifications.** None are sent. See section 6.

**Leader / follower differences in the UI.** The "LEADER" badge; Farm Cleanup's two database sections are empty on a
worker; "Edit" and "Stop chunk" are disabled, with their "needs leader upgrade" wording, when the leader lacks the
capability or cannot be reached; on a worker the chunk list is fetched from the leader over HTTP (blocking, up to about
1.5 s) and job actions are sent to the leader.

**Offline / not configured.** With no sync root the farm does not start: the lists are empty, the tray says "No Farm", a
remote log says "Farm not running". The window itself shows no message and no prompt to open Settings.

---

## 2. The data models (`src/ui/models`, `src/ui/app_bridge.*`)

| Model | Fed from | Fields the UI uses | Order | Cadence | Update method |
|---|---|---|---|---|---|
| Jobs | The backend's cached job list | id (also shown as the name), state, progress (done / total chunks), total, done, failed, rendering, stopped chunks, created, priority, first assignment, last completion | The leader's dispatch order; not sortable, not filterable; archived left out | The backend refreshes its cache every 2 s (at once after a move or a priority change); the bridge pushes it every 50 ms | Same ids in the same order → only changed rows are signalled; otherwise a full reset. Pushes stop during a drag. |
| Nodes | The peer snapshot, this node left out, with the leader's suspension state merged in | node id, host name, leader, active, alive, agent health, render state, active job, suspended, failure count (also carried but not shown: alert reason, tags, priority, last seen, endpoint) | Alive first; then rendering, active, stopped; then by host name, then id | Every 50 ms | As Jobs |
| Chunks | One job's chunks (the leader's database, or HTTP on a worker) | id, first / last frame, state, node id, node name, progress (frames done / frames), assigned at, completed at, retries, the list of finished frames | By first frame | Every 3 s while a job is selected; at once on selection | As Jobs. Also `chunkForFrame(frame)` and `rowAt(row)`. |
| Log | The logger's callback (any thread → queued to the UI thread) | time, level, category, message | Arrival | Live | Appends; the oldest row is dropped past 5000 |
| Templates | The backend's template cache, plug-in templates left out | id, name (also carried: executable path for this OS, flag count, valid, validation error, example) | The cache's | With the job cache | A full reset when anything differs |
| Log sources, node log lines, task chunk files, task lines | Built by the bridge | plain lists | | Sources every 50 ms (rebuilt only on change); the rest every 3 s | Replaced whole when different |
| Current job | A map of the selected job | as Jobs, plus the frame range | | Signalled every 50 ms while a job is selected | |

**Selection** lives in the view, not the models (1.12): a set of keys, an anchor and a cursor per list. The jobs list's
cursor is the application's current job.

**Size and performance.** No limits and no paging. The lists are Qt's virtualised list views (only visible rows exist).
The frame grid is one painted surface: every repaint fills one rectangle per frame, then per chunk; the code's own note
calls it cheap "at 10 k frames", and the form allows ranges up to a million frames. The finished-frame list of every chunk
is converted on each repaint. The selection's `rowOf` is a linear search, and the menus' and the strip's enabled states
call it per row. The whole job list and the current job's map are re-pushed 20 times a second and diffed. The Monitor Log
keeps 5000 rows with wrapped, variable-height rows; a node's log is capped at 500 lines; a task log file is loaded whole.
Expect tens to a few hundred jobs, up to tens of nodes, and chunk counts from one to thousands (chunk size 1 on a long
range).

---

## 3. The map to `min-ui`

`min-ui` today (`crates/min-ui/src/lib.rs`): `ActionButton`, `ActionGroup`, `TextButton`, `TextField`, `TextArea`,
`NumberField`, `Picker`, `Switch`, `Slider`, `Scrubber`, `Section`, `Rail`, `Dialog`, `Menu`, `MenuBar`, `ListColumn`,
`ScrollBar`, `Toast`, `Tooltips`, `Popover`, `ColorPicker`, and the drawn-not-held helpers `checkbox`, `progress`, `paint`.
Both applications are ONE window; what they call windows (`apps/mn/src/gather_win.rs`, `mappings_win.rs`, `site_win.rs`)
are cards over the page assembled from these parts.

| minRender piece | Status | With what / what is missing |
|---|---|---|
| Buttons with icon and / or label, default / primary / danger, tooltips | exists | `ActionButton` (icon, optional label, selected), `TextButton` (`Primary`, `Secondary`, `Negative`). The looks differ (pills, Spectrum greys); the port takes the platform's look. |
| Toggle chip (log levels) | exists | `ActionButton` with `selected`; the per-level colour when on is not a property today — a small addition or a ruling to drop it |
| Text field, secret field | exists | `TextField` |
| Number field with steppers, range, wheel | exists | `NumberField` |
| Dropdown (template, log source) | exists | `Picker` (quiet form). To check: a long list of options (many peers, many templates) — the context `Menu` scrolls when taller than the window; `Picker`'s own popover was not seen to. |
| Checkbox | exists (drawn helper) | `min_ui::checkbox::paint_box`, three states; the owner holds the state and the hit test, as `gather_win.rs` does. minNotes' mappings table uses `Switch` for "On". |
| Progress bar | exists (drawn helper) | `min_ui::progress::bar` |
| Context menus, check items, disabled header row, separators | exists | `Menu` (`open_at`, `open_checked_at`; scrolls when too tall) |
| Menu bar on Windows / native menu on macOS | exists | `MenuBar` + `apps/mn/src/menu.rs` (muda on macOS). minRender's Windows build uses a hamburger list instead — see the questions. |
| Tooltips | exists | `Tooltips` (the same 600 ms / 5 s) |
| Toast | exists | `Toast` (one at a time; top right, 3.4 s; kinds positive / notice / negative). Position and duration differ from minRender's bottom-centre 6 s. |
| Scroll bar | exists | `ScrollBar` (overlay) |
| Confirm card (heading, body, buttons) | exists | `Dialog` |
| Left panel with a draggable width, collapsible | exists | `Rail` (covers the Node panel's horizontal split) |
| Section heading in a form | exists | `Section` (collapsible — more than is needed) or a plain label |
| Path Mappings table (rows of text fields, a switch, remove, add) | exists in an app, should move to `min-ui` or be shared | `apps/mn/src/mappings_win.rs`, on `min_paths::map::Mapping` — the same table. minRender's rows also have a Linux column and a label; minNotes' has a coverage hint. |
| Form card with a scrolled body and a pinned button row (Settings, New Job, Farm Cleanup) | exists in an app, should move to `min-ui` | The card + scrolled body + footer pattern is rebuilt in `gather_win.rs`, `mappings_win.rs`, `site_win.rs`; `Dialog` itself only stacks labelled text fields. A shared "form card" would serve all of minRender's cards. |
| Itemised list with a tick per row and per group, a busy state (Farm Cleanup) | exists in an app, should move to `min-ui` | `apps/mn/src/gather_win.rs` (rows under kinds, a tick per row and per kind) |
| Rows with multi-selection (Ctrl toggle, Shift range, Select All) and drag to reorder | exists in an app, should move to `min-ui` | `apps/qcv/src/project_panel.rs` (multi-select, range, reorder by drag); `apps/mn/src/sidebar.rs` (drop line, single selection). Neither is a reusable widget, neither has columns. |
| Native file / folder pickers | exists (in the apps) | `rfd`, as both applications use it |
| **Data table** (jobs, chunks, the Monitor Log's columns) | **missing** | See 3.1 |
| **Vertical split with a draggable divider** (job list / detail; frame grid / chunk table; task chunk list / text) | **missing** | See 3.2 |
| **Tabs** (Job Detail / Logs) | **missing** | See 3.3 |
| **Log view** | **missing** | See 3.4 |
| **Frame grid** | **missing** (an application widget, or `min-ui` if the family wants it) | See 3.5 |
| **Generated form** (flags) | **missing** as a widget; its parts exist | See 3.6 |
| **Picture view with a thumbnail grid** (preview, EXR layers) | **missing** in `min-ui`; the pieces exist in the applications | See 3.7 |
| **Spinner** (busy indicator) | **missing** | See 3.8 |
| **Status badge / pill, state dot** | **missing** as named parts | Trivial: a rounded rect with upper-case bold text in a state colour; an 8–10 px disc. `paint::rounded` covers the drawing. |
| **Inline banner** (error box in a form, the suspension warning) | **missing** as a named part | A tinted rounded box with an icon, text that wraps and an optional button |
| **Tray icon and its menu** | **missing** from the platform | See section 4 |
| Line graph / sparkline | not needed | minRender draws none |
| Tree | not needed | minRender has no tree |

### 3.1 Data table (the largest gap)

Required behaviour, all taken from the QML:

- columns with a header row; each column a fixed width in px with a minimum and a default; a grip at the right edge of each
  header drags the width (the resize cursor; an accent line while hovered or dragged); a double-click on the grip restores
  the default; widths are handed back to the owner to persist; no fill column;
- cell kinds: text (sans or mono, a colour per cell, bold, elided at the right or in the middle), a progress bar with an
  optional right-aligned count, an icon, a two-line cell (the peers' rows are two lines at 52 px);
- a fixed row height per table (32 / 24 / 52), zebra striping, the selection fill, a per-row opacity (ghosted while
  dragged, dimmed when stopped), a 3 px accent rule on the "primary" row;
- virtualised rows (only the visible ones laid out and painted), thousands of rows;
- the selection model of 1.12, keyed by a stable id supplied by the owner, surviving a replacement of the rows; scroll the
  cursor into view; a click on the empty area clears;
- keyboard focus per table (Cmd/Ctrl+A must act on the focused table only, and not while a text field has focus);
- right-click reports the row and the point (the owner opens a `Menu`); double-click and Return report activation;
- a per-cell tooltip supplied by the owner;
- optional drag to reorder from a handle column: a drop line between rows, the dragged row ghosted, the hand cursors, the
  result reported as (moved id, target id, before / after); the owner pauses its data updates meanwhile. Auto-scroll at the
  edges is absent today (a ruling);
- updates without flicker: a row replaced in place when only its values changed;
- NOT required: sorting by a header click, filtering, column reordering, column hiding, horizontal scrolling, cell editing.

`ListColumn` is not a base for this: one text column, a single pick, no header, no multi-selection.

### 3.2 Split view

Two panes, horizontal or vertical, a divider with a hot zone and the resize cursor; a preferred size for one pane, a
minimum for each; the position reported for persistence; a pane can be hidden (View > Node Panel / Job List), the other then
takes the room. `Rail` covers only the left-edge horizontal case.

### 3.3 Tabs

A strip of text tabs at toolbar height: the active one bright with a 2 px accent underline, the others muted and brighter
on hover; a click switches; the index persists. Both pages keep running when hidden.

### 3.4 Log view

- a virtualised list of lines in the mono face that WRAP to the width (variable row heights), zebra striped;
- structured rows: fixed-width columns for time, level (coloured) and category, then the wrapping message;
- plain rows with a per-line colour chosen by the owner;
- filtering by level (hidden rows take no room);
- stick-to-end: pinned while the user is at the end, released by scrolling away, pinned again on return; it must hold when
  the whole content is replaced (the 3 s re-reads) and when it grows;
- a count for the toolbar; an empty-state text;
- capacity: 5000 structured rows; files of any length for task output (today the whole file is loaded — a cap is a ruling);
- text selection and copy are NOT in today's UI (they were named in the brief as a likely need; a ruling).

### 3.5 Frame grid

As 1.8: fixed 10 px cells wrapped to the width, six state colours, a pinned-frame outline, `frame_at(x, y)`, a hover
tooltip, a click; scrolls when taller than its box; repaints only on change. In vello this is at most one rectangle per
frame; for very large ranges, runs of equal colour in a row should be merged into one rectangle, or the grid drawn into an
image, so that a million-frame job does not put a million shapes in the scene.

### 3.6 Generated form

Not a new widget so much as a layout: label column (140 px) + `TextField` + optional "Browse" button + a help line, built
from a list of field descriptions; a scrolled body; values read back as a list of strings in the template's order. Only
text fields and file pickers exist today. The four number fields and the edit-only pair sit above it.

### 3.7 Picture view and thumbnails

A box that shows one decoded picture fitted with its aspect kept, a centred message when there is none, a spinner while
decoding; and a card with a scrolled two-column grid of thumbnails, one selected. minNotes draws pictures and has a large
view (`apps/mn/src/studio.rs`, `media.rs`); QCView has thumbnail strips (`apps/qcv/src/hover_thumbs.rs`,
`crates/qcv-player/src/thumbnail_cache.rs`). None of these is a `min-ui` widget; the picture box is small enough to write
for minRender and offer to `min-ui`.

### 3.8 Spinner

A small rotating arc (20–28 px) for: a submission in flight, an edit in flight, a cleanup scan, a decoding preview.
`min-ui` has `anim::Ramp` but no indefinite spinner; the redraw-on-demand rule means the spinner must ask for frames while
it is shown.

---

## 4. Platform integration

| Need | minRender today | minplatform | Gap |
|---|---|---|---|
| Tray / menu-bar icon with a menu, a tooltip, click to show | Qt's tray icon; a template image on macOS | Nothing (no tray code in `apps/` or `crates/`) | **Missing.** A tray crate that works beside a winit event loop on both systems (the macOS menu already comes from `muda`; a tray from the same family of crates is the obvious candidate — to be spiked). |
| Close hides the window; the app lives on | Yes | Both applications quit on close | **Missing**: hide instead of close, show again from the tray / the Dock / a second launch; on Windows the hidden app must still receive the end-of-session message |
| Start hidden (`--minimized`) | Yes | No | **Missing**, small |
| Dock icon click reopens a hidden window (macOS) | Yes | minNotes handles documents handed over by the system (`apps/mn/src/mac_open.rs`); a reopen of a hidden window was not seen | To add |
| System notifications | None sent (a dead setting) | None | No gap unless the owner wants them (see the questions) |
| Start at login | Windows: the Store package's startup task, through a helper that adds `--minimized`. macOS: nothing found. | Nothing | Packaging on Windows (the MSIX manifest), not UI; macOS stays as it is unless ruled otherwise |
| Restart after a Store update; force exit on session end | Yes (`src/ui/platform/app_restart.*`) | Nothing | **Missing**, Windows only: register for restart, and a hidden top-level window for the end-of-session message |
| Remote "Restart App" of a node | A restart helper program launched by the backend | — | Backend scope; the UI only sends the request |
| Single instance, a second launch raises the first | Yes | QCView: `apps/qcv/src/single_instance.rs` (a per-user local socket / named pipe); minNotes hands documents to a running instance | Exists; lift QCView's |
| Native file and folder pickers | Qt's | `rfd` in both applications | Exists |
| Open a folder / reveal a file | `explore` on Windows, `open` on macOS | `apps/mn/src/open.rs` (`file`, `reveal`, `url`), with the Explorer quoting faults already fixed | Exists in an app; should be shared (QCView has `os_actions.rs` too) |
| Open a web address | Yes | `open::url` | Exists |
| Dark title bar on Windows | Qt's dark colour scheme | `with_theme(Some(Theme::Dark))` on the window (both applications) | Exists |
| macOS chromeless title row with native traffic lights, drag and double-click zoom | Yes | To check against how the applications build their windows on macOS (both carry AppKit code: `apps/qcv/src/mac.rs`, minNotes' `objc2-app-kit` use) | Likely exists; confirm when the shell is written |
| Menu: native on macOS, in-window on Windows | Native on macOS; a hamburger list on Windows | `muda` on macOS + `min_ui::MenuBar` on Windows (`apps/mn/src/menu.rs`) | Exists; a ruling on hamburger versus menu bar |
| High DPI | Qt's scaling; sizes in px | Every widget takes the window's scale factor | Exists |
| sRGB surface | Pinned | The presenter's concern (`min-present`) | Exists |
| Drag and drop of files into the window | None | Both applications take dropped files | Not needed for parity |
| Clipboard | None (nothing can be copied) | `arboard`, `min_paths::clipboard` | Not needed for parity |
| Localisation | Every string is wrapped for translation, but no translation files and no translator are in the tree: English only | `min-i18n` (`trc`), 11 languages in QCView; minNotes ships English only | Exists; a ruling on whether minRender's strings get drafts |
| Accessibility | Whatever Qt Quick provides by default; nothing explicit | `accesskit` is a workspace dependency, but no use of it was found in `apps/` or `crates/` | No parity gap; neither side does it deliberately |
| Updater | Sparkle on macOS; the Store on Windows | Sparkle loaded at run time (`apps/qcv/src/sparkle.rs`, `apps/mn/src/sparkle.rs`); MSIX packaging done twice | Exists |
| Settings store | The OS store for UI state; the farm configuration in its own JSON file | `min-settings` (one JSON file per app, typed keys) | Exists for UI state; the farm configuration file is a frozen contract for the backend port |
| Path translation between systems | Its own mappings (`win`, `mac`, `lin`, `enabled`, `label`) | `min-paths` (`map::Mapping`) — minRender's branch is itself named for an adoption of it | Exists; confirm the two table shapes are the same |

---

## 5. Image decoding for previews

**minRender today** (`src/preview`, vendored from QCView; optional at build time — without the libraries the preview pane
is absent):

| Format | Library | Notes |
|---|---|---|
| EXR | OpenEXR 3 + Imath | Half float; multi-layer: the layer list is read from the file, a thumbnail path reads every Nth scanline, and one sweep can fill every layer's thumbnail; data window ≠ display window handled; alpha added when absent |
| PNG | libpng | 8 and 16 bit, palette / grey / transparency expanded |
| JPEG | libjpeg-turbo | RGB and grey; scaled decoding for thumbnails; CMYK refused |
| TIFF | libtiff | 8 and 16 bit integer |
| Anything else (BMP, WebP, GIF, …) | Qt's own readers | scaled on read |

Pipeline: an asynchronous provider with its own pool of 4 threads; each request decodes a MASTER of 480 px on the long edge,
kept in a 64 MB least-recently-used cache keyed by path and layer; each answer is scaled down from the master (never up).
EXR pixels are converted linear → sRGB, clamped, drawn over black, with no exposure control and no colour management.
Requests for sibling layers of one file wait for the one sweep. Files under 15 KB are treated as broken. Note: the pane
asks for 960 px but the master is 480 px, so the preview is at most 480 px on its long edge, scaled up by the view.

**minplatform** already has more than this:

- `crates/min-image`: EXR (through the OpenEXR binding in `sys.rs`; `exr::layers`, `exr::load` by layer), PNG and TIFF
  (pure Rust), JPEG (TurboJPEG), DPX, TGA / BMP; `load`, `load_thumbnail(path, layer, max_size)`, `dimensions`; the same
  gap / broken sentinels.
- `crates/qcv-player/src/thumbnail_cache.rs`: a threaded thumbnail cache with texture hand-over, written for timelines.
- `crates/min-ffmpeg` and `apps/mn/src/media.rs`: video and further still formats (GIF and WebP through the `image` crate,
  HEIC and camera RAW through the system on macOS).
- QCView's colour stage (`crates/qcv-color`) can show an EXR properly, where minRender uses a fixed sRGB curve.

What the port needs: a small "decode a thumbnail off the UI thread, cache it by (path, layer), hand it to the scene"
service on `min_image::load_thumbnail`, with one-sweep decoding of all layers for the layers card if it is fast enough
without it (to measure). No new decoder. The preview would no longer be optional at build time.

---

## 6. Parity checklist

Each line is one testable behaviour of today's application.

**Window and process**

1. The window opens at 1440 × 900 with the title "minRender Monitor" in the task bar / Dock.
2. Closing the window hides it and the node keeps working.
3. Launching with `--minimized` starts with no window shown.
4. Launching a second time shows and raises the first instance's window and starts nothing new.
5. On macOS, clicking the Dock icon shows a hidden window.
6. On macOS the title row drags the window and a double-click on it zooms; the traffic lights are in the row.
7. On Windows the title bar is dark whatever the system's colour scheme.
8. On Windows, the end of the session (a Store update, sign-out) stops the node and it is started again, hidden, after an update.
9. Split positions, panel visibility, the active bottom tab, column widths, the grid's height survive a restart.

**Tray**

10. A tray / menu-bar icon is present; its tooltip reads "minRender - Rendering <job>", "- Stopped", "- No Farm" or "- Idle".
11. A click on the icon, or "Show Window", shows the window.
12. The tray's "Stop" / "Resume" toggles the node and its label follows the state.
13. The tray's "Exit" quits when idle, and waits for the current render when one runs.

**Menus and title row**

14. macOS has the menus File, Jobs, View, About with the items of 1.3; Windows has the same actions (1.3's order).
15. Cmd/Ctrl+A selects all jobs only while the job list has the keyboard.
16. "Edit Job…" is enabled only with exactly one job selected and a leader that supports editing.
17. "Node Panel" and "Job List" show and hide their panels and keep their tick.
18. "Check for Updates…" is absent on Windows and present on macOS.
19. "Docs" opens the documentation site.
20. The title row shows this node's host name, "LEADER" when it leads, and a dot: green / grey / red with the three tooltips.
21. "New Job" in the title row opens the New Job card.

**Node panel**

22. The card lists Host, Node ID, GPU, CPU / RAM and Tags with "—" for missing values.
23. "Stop" / "Resume" toggles this node and changes its label, icon and weight.
24. A suspended node shows the banner with the failure count, and "Unsuspend this node" clears it.
25. Peers are ordered alive first, then rendering, active, stopped, then by name.
26. A peer row shows the dot's colour and the second line's text for each of: suspended, offline, rendering, idle, stopped.
27. Peers can be multi-selected with the gestures of 1.12.
28. A right-click on live peers offers Stop, Start, Restart App, Unsuspend with counts, each disabled when it applies to none.
29. A right-click on offline peers offers "Write restart signal (filesystem)".
30. The preview section appears only while a job is selected.

**Job list**

31. Jobs are listed in dispatch order with the seven columns of 1.7.
32. A column's width changes by dragging its header's grip, returns to the default on a double-click, and persists.
33. An active job's Duration ticks every second.
34. A completed job with stopped chunks reads "partial" with its tooltip.
35. The strip shows the count and the number selected; its buttons are disabled with nothing selected.
36. Pause, Resume, Retry failed, Requeue, Archive act on every selected job and then clear the selection.
37. Cancel and Delete ask first, with the two texts of 1.7, and act only on confirmation.
38. Selecting a job shows it in Job Detail; several can be selected with Ctrl / Shift / the keys.
39. A double-click or Return opens Edit Job for a single selection.
40. Delete (or Cmd/Ctrl+Backspace) opens the Delete confirmation.
41. A click under the last row clears the selection.
42. Dragging a row by its handle shows a drop line and, on release, moves the job there; across a priority group it takes the target's priority.
43. The list does not change under the pointer during a drag.
44. The context menu offers the nine actions of 1.7, with " %1 jobs" on a multi-selection.

**Job Detail**

45. With no job selected the tab shows "No job selected", the hint and a "New Job" button.
46. The header shows the job's name, state badge, progress with "%1 / %2", the rendering and failed counts, and the creation time.
47. Editing the priority number changes the job's priority at once and the job moves in the list.
48. Pause shows for an active job and Resume for a paused one; Retry failed is enabled only with failed chunks.
49. "Open output" opens the job's output folder, translated through the path mappings.
50. The frame grid shows one cell per frame in the six colours of 1.8 and follows the chunks' progress.
51. Hovering a frame shows its number, chunk, range, state and node.
52. Clicking a rendered frame pins the preview to it and outlines the cell; clicking an unrendered one does nothing.
53. The divider between the grid and the chunk table drags and persists.
54. The chunk table shows State, Frames, Node, Progress, Duration, Retries; stopped chunks are dimmed.
55. Selecting a chunk pins the preview to that chunk's newest frame on disk.
56. The chunk menu offers reassign (or requeue for stopped chunks), stop, and "Submit … as separate job…".
57. "Submit as separate job…" opens the form pre-filled with the chunk's range and a suggested name.

**Preview**

58. The preview shows the newest rendered frame of the selected job and updates within about 3 s of a new frame.
59. It reads "No frames rendered yet" until one exists.
60. A pinned preview shows the amber tag and a "Latest" button, and returns to the latest by itself after 60 s.
61. For an EXR, "Layers" opens a grid of every layer's thumbnail; choosing one changes the preview and is remembered per job until the application closes.
62. EXR, PNG, JPEG and TIFF frames are shown.

**Logs**

63. The source dropdown lists "Monitor Log", "Task Output", "<host> (this node)" and every live peer.
64. The Monitor Log shows time, level, category and message, live, keeping the last 5000.
65. The Info / Warn / Error chips hide and show their rows; they are disabled for the other sources.
66. A node's log shows its last 500 lines, re-read every 3 s, with error and warning lines coloured.
67. Task Output lists the selected job's chunk logs on the left and the chosen one's text on the right, following the file as it grows.
68. With Autoscroll on, a view follows its end until the user scrolls away, and again once they return to the end.
69. "Show in Explorer / Finder" reveals the current log's file, or shows a toast when there is none.
70. "Clear" empties the Monitor Log view.

**Settings and Path Mappings**

71. Settings shows the sections and controls of 1.11 with the current values.
72. The sync root can be typed or picked with a folder dialog, and "✓ path OK" / "⚠ path not reachable" follows it.
73. The UDP port is disabled while the multicast box is off.
74. The fingerprint line and exactly one of the three security texts are shown.
75. "Save" writes the configuration; "Cancel" discards every edit; neither Escape nor a click outside closes the card.
76. Saving a changed sync root or a changed network setting reconnects the farm without a restart of the application.
77. Path Mappings lists the rows, adds, removes, and saves at once; empty rows are dropped; Cancel discards.

**Submission and editing**

78. New Job lists the templates; choosing one fills the range, chunk size, priority and the flag fields.
79. Only editable or required flags are shown; required ones are marked; read-only ones cannot be changed.
80. A flag of type file has "Browse", filtered to its extension, and the choice lands in the field.
81. "Submit" is disabled without a name or a template, and while a submission is in flight.
82. A successful submission closes the card and selects the new job; a failure shows its reason in the form and keeps it open.
83. Edit Job shows the job's stored values, the name locked, plus Max retries and Timeout.
84. Changing the range or chunk size in Edit Job shows the warning and leaves only "Start over" enabled.
85. "Stop chunks & apply" and "Start over" ask first; "Apply to next chunks" does not.
86. A job whose template is gone can still be edited, with every stored flag shown raw and the amber note.
87. An edit that reaches the leader shows "Edit applied to %1"; one that is queued shows the queued message.

**Farm Cleanup**

88. Farm Cleanup scans on opening and shows the six sections with counts, or their empty texts.
89. On a worker the two job sections say that the leader is needed.
90. A section's box selects or clears all its rows and shows the mixed state.
91. A section's action is enabled only with a ticked row, runs, and the list refreshes.
92. A spinner shows and the buttons are disabled while a scan or an action runs.

**General**

93. A failed job action shows a toast with its reason for 6 s; a click dismisses it.
94. Controls that carry a tooltip show it after the delay.
95. The leader's missing capabilities show as disabled items with the "needs leader upgrade" wording.
96. Nothing in the interface redraws while nothing changes (the original's stated rule for the frame grid).

### Things in today's UI that look broken, unfinished or inconsistent

To be carried as they are unless ruled otherwise.

| # | What | Where |
|---|---|---|
| a | **"Show notifications" does nothing.** The setting is saved and served over HTTP, but no code sends a notification. | `SettingsPanel.qml`, `src/core/config.h` |
| b | **The chunk menu's count is wrong.** The suffix " (%1 chunks)" is filled with the menu's own item count, not the number of selected chunks, so a multi-selection always reads the same number. | `JobDetailPanel.qml`, `chunkMenu.suffix()` |
| c | **Cancel and Delete in Job Detail do not confirm**, while the same actions in the list, the menu and the menu bar always do. | `JobDetailPanel.qml` |
| d | **File > Exit quits at once** (`Qt.quit()`), while the tray's Exit waits for the running render; the two paths differ. | `Main.qml`, `main_qt.cpp` |
| e | **The detail header never says "partial"** and gives it no colour; the list does. | `JobDetailPanel.qml` |
| f | **The manual and the code disagree on dragging across priorities**: the manual says the drop is refused; the code moves the job and gives it the target's priority. | `docs/monitor.md` vs `JobListPanel.qml` |
| g | The manual says required fields carry an asterisk; they are amber, with no asterisk. | `docs/submit.md` |
| h | The manual says selecting jobs "reveals" the action strip; it is always shown. | `docs/monitor.md` |
| i | "Peers · %1 online" counts every listed peer, offline ones included. | `NodePanel.qml` |
| j | The preview's height is read from the settings but nothing in the UI changes it (no divider). | `NodePanel.qml` |
| k | The preview asks for 960 px but the decoder's master is 480 px. | `JobPreview.qml`, `thumbnail_image_provider.cpp` |
| l | With level chips off, the Monitor Log's count still counts hidden rows, and the striping follows the hidden rows. | `LogPanel.qml` |
| m | Log text cannot be selected or copied; the only way out is "Show in …". | `LogPanel.qml` |
| n | The tray's status text is computed and never shown; a status-coloured icon is noted as deferred. | `tray.cpp` |
| o | "Select All Jobs" is missing from the Windows app menu. | `Main.qml` |
| p | No message in the window when the farm is not configured or not running; no first-run prompt. | — |
| q | Farm Cleanup deletes without a confirmation. | `FarmCleanupDialog.qml` |
| r | Settings and New Job close only by their buttons; Path Mappings and Farm Cleanup also close on Escape or an outside click (losing edits in Path Mappings). The two stock confirm dialogs (edit apply; cancel / delete) are styled differently from the house card. | `Main.qml`, the dialogs |
| s | The Settings card is not scrolled; on a short window its lower sections can be cut off. | `SettingsPanel.qml` |
| t | Path Mappings saves at once, outside Settings' Save / Cancel. | `PathMappingsDialog.qml` |
| u | The template's `cmd` block (`editable`, `label`) and the flag type `"output"` have no control in the form; only `"file"` gets a picker. | `SubmissionForm.qml`, `docs/templates.md` |
| v | "Open output" fails silently (a log line only) when the folder does not exist on this machine. | `app_bridge.cpp` |
| w | Dead or unused: `forgetPeer`, `requestRestart`, `priorityAt`, the OS accent reader, the `ToolStrip` component, the Material Symbols font, and the node model's alert reason / tags / endpoint / last-seen fields (carried, never shown). | `app_bridge.h`, `components/` |
| x | The job "name" is the job id; a display name was planned and never added. | `jobs_model.cpp` |
| y | The job list does not scroll by itself during a reorder drag. | `JobListPanel.qml` |
| z | macOS has no start-at-login; Windows gets it from the Store package. | `installer/` |

---

## 7. A build order for the UI

Each slice is useful by itself and adds the fewest new widgets. The backend (the coordinator in Rust, or a bridge to the
existing one) is another step's subject; the slices assume a data source exists and name what they read.

| Slice | What the operator gets | New `min-ui` / platform pieces |
|---|---|---|
| 0 | A window in the family's shell: title row (host name, LEADER, state dot), the menus, Settings with only the sync root, single instance. Joins the farm. | A form card (lifted from the minNotes cards); status badge; state dot. Single instance lifted from QCView. |
| 1 | The Node panel, read-only: this node's card and the peers list with its states. | **Data table** (first cut: rows, two-line cells, striping, virtualisation); `Rail` for the panel |
| 2 | The job list, read-only, with the detail header and the chunk table. | Data table: columns, resizable headers, progress cells; **split view**; **tabs** |
| 3 | The frame grid. | **Frame grid** |
| 4 | Selection and actions: multi-selection, the strip, context menus on jobs, chunks and peers, the confirm cards, the priority field, this node's Stop / Resume and unsuspend, toasts. | Data table: the selection model, keyboard, right-click. Banner. |
| 5 | Drag to reorder. | Data table: the handle column and the drop line |
| 6 | Logs: Monitor Log, node logs, Task Output; "Show in …". | **Log view**; the shared open / reveal helper |
| 7 | New Job, then Edit Job and Resubmit. | The generated form; **spinner**; `rfd` file picker |
| 8 | The rest of Settings, Path Mappings, Farm Cleanup. | The shared mappings table; the ticked list (lifted from `gather_win.rs`) |
| 9 | Tray, close-to-tray, `--minimized`, Dock reopen, the Windows session-end and restart registration. | **Tray icon and menu**; hide-on-close in the shell |
| 10 | Preview and the EXR layers card. | Picture box, thumbnail grid, the thumbnail service on `min-image` |
| 11 | Updater, packaging, the manual's pictures. | Lifted from QCView / minNotes |

Slice 9 could move up to right after slice 1 if the first Rust build is meant to run on real nodes unattended: a render node
without the tray and close-to-tray is not usable day to day. The tray is also the piece with the most unknowns (no code
for it exists in the platform), so a spike before slice 0 would take the risk out early.

---

## Open questions for the owner

1. **Graphs.** minRender has no CPU / GPU / memory / temperature / network graphs and samples none of it. Is machine
   monitoring wanted in the port (a new feature), or is the brief's mention of it a mix-up with the `src/monitor` folder's
   name? [proposed: not in the port]
2. **Look.** The platform's widgets are Spectrum-style (pill buttons, grey selection, blue rationed); minRender is flat with
   a blue accent, blue primary buttons and a blue selection fill. Parity of behaviour with the platform's look, or
   minRender's look carried? [proposed: the platform's look and tokens, as the house rule says]
3. **Windows menu.** The hamburger list of today, or the in-window `MenuBar` the other two applications use on Windows?
   [proposed: `MenuBar`, with "Select All Jobs" in it]
4. **Toast.** Bottom centre for 6 s (today) or the platform's top-right `Toast` with its 3.4 s? [proposed: the platform's
   `Toast`; a longer time for errors if wanted]
5. **Checkbox or switch** for the on / off settings and the mappings' "On" column? [proposed: `Switch` in forms, as
   minNotes' mappings table does; checkboxes where rows are ticked for an action]
6. **"Show notifications".** Remove the dead setting, keep it dead, or make it send system notifications (job finished,
   job failed, node suspended)? [proposed: keep the setting's key in the file, hide the control until notifications exist]
7. **The findings in section 6** — which to fix in the port rather than carry: [proposed: fix b (chunk count), c (confirm
   in Job Detail), d (one Exit path), e ("partial" in the header), i (count live peers only), o; carry the rest and list
   them for later]
8. **Dragging across priority groups**: the code's behaviour (adopt the target's priority) or the manual's (refuse)?
   [proposed: the code's; correct the manual]
9. **Log text**: keep it unselectable (parity) or make it selectable with copy, since the widget is new anyway?
   [proposed: parity first; selection as a later addition to the log view]
10. **First run.** Keep "nothing happens until Settings is opened", or show a line in the empty window that points at
    Settings? [proposed: one line and a button in the empty job area — the smallest help; no wizard]
11. **Localisation.** minRender ships English only though its strings are wrapped. Strings through `min-i18n` with English
    only, or drafts in the 11 languages as QCView has? [proposed: through `trc` from the start, English only at first, as
    minNotes]
12. **Path mappings.** Are minRender's table (`win`, `mac`, `lin`, `enabled`, `label`, in the farm configuration) and
    minNotes' (`min_paths::map::Mapping`, per machine) to become one control and one file format, or stay two?
13. **Preview quality.** Carry the 480 px master and the fixed sRGB curve, or use QCView's colour stage and a larger size
    now that the decoder is the family's? [proposed: carry the behaviour, lift the size to what the pane shows]
14. **Scale to design for.** How many jobs, nodes and frames per job do the largest farms in use have? The data table and
    the frame grid are sized from this. [assumed: hundreds of jobs, tens of nodes, tens of thousands of frames]
15. **Linux.** The original builds a tray fallback and path columns for Linux; is a Linux build of the port in scope?
    [proposed: no — Windows and macOS, as shipped]
16. **Order.** Tray and close-to-tray early (slice 9 moved up, with a spike first), or last?
17. **Where the new widgets live.** The data table, split view, tabs, log view and spinner are general — `min-ui`. The
    frame grid is minRender's own: in `min-ui` or in the application? [proposed: the application]
