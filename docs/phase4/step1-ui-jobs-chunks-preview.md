# Phase 4, step 1 — minRender: jobs, chunks and preview frames, read closely

Written 2026-11-14 from a line-by-line read of the minRender working tree (branch `min-paths`, release 0.5.11 plus the
path-library change). Nothing was built, run or edited. All `src/…` and `docs/…` references are into the minRender
repository; `file:line` numbers are for that checkout. This document refines `step0-ui-inventory.md` for the job list,
the job detail (chunks, frame grid), the preview, the submission form and the peer list as it relates to jobs; sections
8 and 9 list what the first inventory got wrong and what looks broken today.

Vocabulary used below: **leader** = the node that owns the database; **follower** = any other node (the code says
"worker"); **current job** = the job whose detail is showing (`AppBridge::currentJobId`); **chunk** = a contiguous run of
frames rendered by one process on one node. The UI never uses the word "task" for a chunk except in "Task Output".

A note on the manual's pictures: `docs/images/mr001.jpg`, `mr009`–`mr011` show an OLDER layout (a separate "Jobs" header
row over the strip, no "Pri" and no "Duration" column, no drag handle, an empty State column, the log beside the detail
instead of in a tab, the New Job form inside the detail pane, raw node ids in the chunk table, no preview). The
`midrender_*.png` files are older still (the previous toolkit) and are not referenced by any page. **The code is the
reference for parity, not the pictures.**

---

## 1. The job list (`src/ui/qml/panels/JobListPanel.qml`)

### 1.1 Data and refresh

| Step | What happens | Where |
|---|---|---|
| Leader | Every 2 s (or at once when forced) the backend rebuilds its job cache from the database: all jobs, `archived` left out, with per-state chunk counts and two timestamps | `src/monitor/monitor_app.cpp:116`, `641-719`; `database_manager.cpp:176-236` |
| Follower | The same 2 s tick does a **blocking** `GET /api/jobs` on the leader (500 ms connect, 1 s read). On any failure the old cache is kept and no request is made for 5 s | `monitor_app.cpp:720-801` |
| Push to the model | A 50 ms timer calls `AppBridge::refresh()`, which hands the cache to `JobsModel::setJobs` (skipped while a reorder drag is in flight) | `src/monitor/main_qt.cpp:356-358`; `app_bridge.cpp:837-838` |
| Diff | Same ids in the same order → `dataChanged` only for rows whose state, priority, counts or the two timestamps changed. Any change of membership or order → a full model reset | `models/jobs_model.cpp:133-158` |
| Forced refresh | After a move, a priority change or a chunk stop: at once on the leader; on a follower when the leader answers 200 | `monitor_app.cpp:927`, `942`, `962`, `970`, `1204`, `1212` |

There is no push from the leader; everything is polling. Pause, resume, cancel, requeue, archive, retry and delete do
NOT force a refresh: their effect shows on the next 2 s tick (up to 2 s on the leader, up to about 4 s on a follower:
the post, then the next poll).

What a follower's cache holds per job is only the summary (`job_id`, `template_id`, `submitted_by`, `submitted_at_ms`,
range, chunk size, state, priority, the counts, the two timestamps, `output_dir`, `output_stem`); it has no flags and no
manifest (`monitor_app.cpp:745-775`).

### 1.2 The strip (always shown)

`JobListPanel.qml:219-322`. Left to right: "Jobs" (bold) · "· <count>" and, with a selection, " · %1 selected" (muted
with no selection, primary with one; the colour ramps over 100 ms) · buttons · a filler · "Cancel" · "Delete" (danger).

| Button | Icon (Phosphor) | Enabled when | Does |
|---|---|---|---|
| "Edit" | pencil-simple | exactly one selected AND the leader has the `job_edit` capability | `openJobEditor(id)` |
| "Pause" | pause | ≥ 1 selected | `pauseJob` per id, then clears the selection |
| "Resume" | play | ≥ 1 selected | `resumeJob` per id, clears |
| "Retry failed" | arrow-clockwise | ≥ 1 selected | `retryFailedChunks` per id, clears |
| "Requeue" | arrows-counter-clockwise | ≥ 1 selected | `requeueJob` per id, clears. **No confirmation** although it discards all progress |
| "Archive" | archive | ≥ 1 selected | `archiveJob` per id, clears. No confirmation |
| "Cancel" | x | ≥ 1 selected | the confirm card |
| "Delete" | trash | ≥ 1 selected | the confirm card |

The strip buttons are not state-aware: Pause is offered for a completed job, Resume for an active one. The leader's
state changes are unconditional (step0-contracts §4.5), so "Resume" on a cancelled or completed job sets it `active`.

"Clears the selection" (`applyToChecked`, lines 49-54) empties the selected set but leaves the cursor, so the detail
panel keeps showing the job while the strip's buttons go disabled and the row loses its fill and accent rule.

### 1.3 Columns

Header row 22 px on `Theme.border`; rows 32 px. Left margin 8, 8 px between columns, right margin 16 (padding + the
scroll-bar gutter). Widths persist under the settings category `jobsTableColumns` (lines 134-144).

| # | Header | Default / min px | Source field | Cell |
|---|---|---|---|---|
| 0 | (none) | 22 fixed | — | "dots-six-vertical" icon, 14 px, muted; the drag handle; open-hand cursor, closed hand during a drag |
| 1 | "Pri" | 36 / 28 | `priority` (the database value) | mono 11, secondary colour, not elided |
| 2 | "Name" | 300 / 100 | `job_id` (the slug; there is no display name, `jobs_model.cpp:64-67`) | mono 12, primary colour, elided in the middle |
| 3 | "State" | 90 / 50 | `current_state`, with `stopped_chunks` | the raw state word in lower case, 11 px bold, elided right; see 1.4 |
| 4 | "Progress" | 280 / 80 | `completed_chunks / total_chunks` | a bar (10 px tall, radius 3, accent on `#252525`) of width column − 64, and "%1/%2" (done / total chunks) right-aligned, 10 px, secondary. **Chunks, not frames.** Stopped chunks are in neither number |
| 5 | "Duration" | 80 / 50 | `first_assigned_ms`, `last_completed_ms`, the state | mono 11, secondary; see 1.5 |
| 6 | "Created" | 140 / 60 | `submitted_at_ms` (the submitter's clock) | `yyyy-MM-dd HH:mm` in local time, 11 px, secondary; "—" when ≤ 0 |

Header labels are 11 px, secondary colour. Each has an 8 px grip at its right edge: a 1 px line (muted at 45 % opacity
at rest; accent at full opacity while hovered or dragged, ramping over 100 ms), the horizontal-split cursor, a drag
resizes down to the minimum, a double-click restores the default (`components/ResizableHeaderLabel.qml`). There is no
maximum width and no horizontal scroll: columns wider than the panel are simply cut off.

Not shown anywhere in the list (and nowhere in the detail panel either): the template, the frame range, the chunk
size, the submitting node, the counts of rendering / failed / pending chunks.

### 1.4 State words and colours

`JobListPanel.qml:84-101`. The word shown is the wire state, except that `completed` with `stopped_chunks > 0` is shown
as "partial".

| Shown | Colour token | Value |
|---|---|---|
| active | `success` | `#4cb050` |
| paused | `warn` | `#f5a623` |
| cancelled | `error` | `#c04040` |
| completed | `info` | `#9cc9ff` |
| partial | `warn` | `#f5a623`; tooltip "%1 stopped chunk(s) excluded" after 400 ms |
| failed, and anything else | `textSecondary` | `#888888` |

`failed` is a real job state (a job whose chunks are all finished with at least one failed, step0-contracts §4.5); the
list gives it no colour of its own. There is no icon or badge in the list; the state is coloured text only.

### 1.5 Duration

`jobDuration` (lines 122-127) and `Format.duration` (`src/ui/qml/Format.qml`):

- not started (`first_assigned_ms ≤ 0`) → "—";
- state `active` → now − first assignment, re-evaluated by a 1 s timer that runs while the panel is visible;
- any other state → last completion − first assignment when that is positive, else "—";
- format: "42s" below a minute, "3m 12s" below an hour, "1h 4m" above; no days.

`first_assigned_ms` is `MIN(assigned_at_ms)` over the job's chunks and `last_completed_ms` is `MAX(completed_at_ms)`
(`database_manager.cpp:221-233`). Both are the leader's clock; "now" is the viewer's clock.

### 1.6 Rows, selection, keys

- Row fill: selected → `selection` (`#10395b`); else alternating `bgAlt` `#181818` / `surface` `#1a1a1a`; the change
  ramps over 100 ms. No hover highlight.
- A 3 px accent rule at the left edge of the current job's row, only while the selected set is not empty (lines 453-461).
- The dragged row is at 45 % opacity.
- Selection model: `components/ListSelection.qml`, keyed by job id. Click = select only; Ctrl / Cmd-click = toggle;
  Shift-click = range from the anchor; right-click keeps a selection that contains the row, else selects it.
- Keys while the list has focus (focus comes from a click on a row or on the empty area, or Tab):
  Up / Down / Home / End / PageUp / PageDown move the cursor and select it (with Shift: extend from the anchor); a page
  is the visible rows minus one; Ctrl / Cmd+A selects all; Escape clears the selection (only when one exists); Return /
  Enter opens Edit Job when exactly one job is selected and the leader supports it; Delete, or Ctrl / Cmd+Backspace,
  opens the Delete confirmation (lines 405-413).
- Double-click with no modifier: Edit Job, as Return (lines 578-582).
- A click on the empty area under the rows clears the selection (lines 417-424).
- The cursor row is the current job: moving the cursor sets `currentJobId`, which starts the chunk fetch (2.1), clears a
  preview pin and closes the New Job card if open (`app_bridge.cpp:290-335`).
- No type-ahead, no column sort, no filter, no search, no grouping headers between priorities.

### 1.7 Order and drag

The order is the leader's dispatch order: `priority ASC, COALESCE(sort_key, submitted_at_ms) ASC, submitted_at_ms ASC`
(`database_manager.cpp:185`). Drag from the handle only (lines 589-604): press selects that row alone and pauses model
pushes; the drop gap is `round(y / 32)`; a 2 px accent line marks it (none for the row's own two gaps or outside the
list); release calls `moveJob(id, target, before)` — before the row under the line, or after the last row. Across
priority groups the moved job takes the target's priority (leader side). The list does not auto-scroll during a drag,
and the row does not follow the pointer. On release the model catches up at once from the cache
(`app_bridge.cpp:345-354`), but the cache itself is only fresh at once on the leader; on a follower the old order shows
until the leader's answer forces a poll.

### 1.8 Context menu

Lines 628-698. With n > 1 selected each label gets " %1 jobs" (for example "Pause 3 jobs").

| Item | Enabled | Does |
|---|---|---|
| "Pause" | n > 1, or the clicked job is `active` | per id, clears the selection |
| "Resume" | n > 1, or the clicked job is `paused` | per id, clears |
| — | | |
| "Edit Job…" / "Edit Job… (needs leader upgrade)" | n ≤ 1 and the capability | `openJobEditor(clicked id)` |
| — | | |
| "Retry failed chunks" | always | per id, clears |
| "Requeue" | always | per id, clears; no confirmation |
| — | | |
| "Cancel" | always | the confirm card |
| "Archive" | always | per id, clears; no confirmation |
| "Delete" | always | the confirm card |

The clicked job's state is captured when the menu opens; it does not follow a refresh while the menu is up.

### 1.9 The confirm card

Lines 703-769: 460 px wide, a stock dialog (not the house card: no title strip, no scrim colour of its own), no close on
Escape or an outside click.

| Action | Heading | Body | Buttons |
|---|---|---|---|
| Cancel | "Cancel %1 job%2?" (%2 = "" or "s") | "This stops dispatch and immediately kills in-flight renders on all nodes. The jobs can be requeued later." | "Cancel" (closes) · "Cancel jobs" (danger, x icon) |
| Delete | "Delete %1 job%2?" | "Removes job records and SQLite history. Output files on disk are NOT deleted. This cannot be undone." | "Cancel" · "Delete" (danger, trash icon) |

The card acts on the selection as it is when the danger button is pressed, not as it was when the card opened.

### 1.10 What each action does behind the UI

| Action | Leader | Follower | Feedback on failure |
|---|---|---|---|
| Pause / Resume | state → `paused` / `active` | `POST /api/jobs/<id>/pause` / `/resume` | none |
| Cancel | state → `cancelled`; aborts the local render of that job | `POST …/cancel`; aborts the local render | none |
| Requeue | every chunk → `pending`, counters and finished frames cleared, state → `active` | `POST …/requeue` | a log line only |
| Archive | state → `archived` (drops out of the list) | `POST …/archive` | none |
| Retry failed | failed chunks → `pending`, state → `active` | `POST …/retry-failed` | none |
| Delete | cancel first, delete the rows, remove `<farm>/jobs/<id>` (from the node that clicked, retried once after 250 ms) | `DELETE /api/jobs/<id>`, then the same folder removal | a log line only |
| Move | reorder (+ adopt priority) | `POST …/move` | a log line only |
| Set priority | update | `POST …/priority` | none |

(`monitor_app.cpp:887-1105`.) Only the editor paths raise a toast; every plain job action fails silently in the window.

### 1.11 Empty, error and follower states

- No jobs: the header row and an empty list. No text.
- Farm not configured or not running: the same. No text.
- Follower that cannot reach the leader: the last list stays, frozen, with no indication that it is stale.
- Leader versus follower in this panel: the Edit gating only (plus the latencies above).

---

## 2. Job detail (`src/ui/qml/panels/JobDetailPanel.qml`)

Two states: "empty" when `currentJobId` is empty, else "detail" (line 13). The detail is built from
`appBridge.currentJob`, a map re-emitted every 50 ms: `jobId`, `name` (= id), `state`, `priority`, `totalChunks`,
`doneChunks`, `failedChunks`, `renderingChunks`, `progress`, `createdAt`, `frameStart`, `frameEnd`
(`app_bridge.cpp:356-385`). It does NOT carry the stopped count, the chunk size, the template, the submitter or the two
duration timestamps. When the id is no longer in the cache the map is empty (see 9, item 1).

### 2.1 Chunk data

- `ChunksModel` holds one job's chunks ordered by first frame. Fetched at once when the current job changes, then every
  3 s by a timer that runs while a job is current — whether or not the Job Detail tab is in front
  (`app_bridge.cpp:119-121`, `310-320`, `387-411`).
- Leader: a database read. Follower: a **blocking** `GET /api/jobs/<id>` on the UI thread (500 ms connect, 1 s read);
  on failure, or during the 5 s cooldown after any failed contact with the leader, it returns an empty list
  (`monitor_app.cpp:573-637`) and the model is reset to no rows.
- Fields per chunk: `id`, `frame_start`, `frame_end`, `state`, `assigned_to` (node id), `assigned_at_ms`,
  `completed_at_ms`, `retry_count`, `completed_frames`; `failed_on` (the nodes that failed it) is fetched and not shown.
- Node names: a map of node id → host name rebuilt from the peer list (plus this node) on every refresh; an id that is
  not in the map (a forgotten or never-seen node) is shown raw (`chunks_model.cpp:62-66`).
- Diff: as the jobs model; any change of the id list resets the model (requeue keeps ids; "Start over" replaces them).

### 2.2 Chunk states as the UI can see them

The database has five: `pending`, `assigned`, `completed`, `failed`, `stopped` (`database_manager.h:22`,
`database_manager.cpp:733`). **There is no `rendering` chunk state**; the frame grid's code tests for it
(`frame_grid.cpp:225`) but it never arrives. "Rendering" in the header and in the job counts means "chunks in state
`assigned`" (`database_manager.cpp:218`).

Transitions the operator can cause or watch (full table: step0-contracts §4.4):

| Seen change | Cause |
|---|---|
| pending → assigned | dispatch (the leader's 2 s cycle) |
| assigned → completed | the node's report |
| assigned → pending, Retries + 1 | a failure with retries left; the node goes on the chunk's blacklist |
| assigned → failed, Retries + 1 | the last allowed failure; the Node cell keeps the failing node |
| assigned → pending, Retries unchanged | the node died, an edit with "Stop chunks & apply", or "Reassign to another node" |
| failed → pending, Retries 0 | "Retry failed" |
| anything → pending, all cleared | "Requeue" |
| pending / assigned → stopped | "Stop chunk" |
| stopped / failed / assigned → pending | "Reassign…" / "Requeue (resume stopped chunk)"; a finished job goes back to `active` |

### 2.3 Header

Lines 205-361, on `surface`, 10 px margins, 6 px between lines.

1. The job id — mono 14 bold, elided in the middle — and a state badge: a filled pill (radius 3), the state in UPPER
   CASE, 11 px bold, text colour `bg` (`#161616`), fill by `stateColor` (lines 189-198): active green, paused amber,
   cancelled red, completed light blue, everything else (including `failed`) grey; "—" when unknown. No "partial".
2. A full-width progress bar (done chunks / total) and "%1 / %2" in mono 12.
3. "● %1 rendering" (light blue, only when > 0) · "● %1 failed" (red, only when > 0) · "priority" · a number field 1–999
   (typed or stepped; `setJobPriority` on every modification; it follows the backend only while it does not have focus;
   50 when unknown) · filler · the creation time `yyyy-MM-dd HH:mm` (muted, 11 px).
4. Actions: "Pause" (only when `active`) or "Resume" (primary, only when `paused`); "Retry failed" (enabled when the
   failed count > 0); "Requeue"; "Open output" (tooltip in section 6); filler; "Cancel"; "Delete" (danger). In any other
   state neither Pause nor Resume is shown. **None of these confirms.**

"Open output" (`app_bridge.cpp:1202-1248`): the job's `output_dir` translated through the path mappings; if the folder
does not exist on this machine, or the job has none, a log line and nothing else.

### 2.4 Frame grid (`src/ui/painters/frame_grid.{h,cpp}`)

In a vertical split above the chunk table: preferred height 220 (persisted, `jobDetailLayout/gridRowHeight`), minimum
100; the table keeps at least 120. The grid sits in a scroll area with 4 px margins.

- One cell per frame from the job's `frame_start` to `frame_end`, in reading order; `cells per row = floor(width / 10)`;
  each cell is drawn 8 × 8 inside its 10 × 10 slot. The content height is `rows × 10`; taller content scrolls.
- Paint order (`frame_grid.cpp:162-271`): background; every cell in the "unclaimed" colour; then per chunk in list order.

| Cell | Colour | When |
|---|---|---|
| background | `#0d0d0d` | also every frame of a `stopped` chunk (it reads as a hole) |
| unclaimed | `#303030` | `pending`, and frames covered by no chunk |
| assigned | `#0189f1` (accent) | chunk `assigned` |
| rendered | `#2A8228` | a frame listed in `completed_frames` while its chunk is NOT `completed` (assigned, pending again after a failure, or failed) |
| completed | `#46C846` | every frame of a `completed` chunk, whatever `completed_frames` says |
| failed | `#c04040` | chunk `failed` (its finished frames still show dark green on top) |
| outline | white, 1 px, on the full 10 px slot | the pinned frame, only for a single-frame pin |

- Hover: a tooltip after 300 ms — "Frame %1", then "Chunk %1  (%2-%3)" (the chunk's 1-based position in the list and
  its range), then the raw state and, when a node is assigned, "  ·  <host name>" (lines 444-461).
- Click: `pinPreview(f, f)` (section 3.5). No selection of frames, no drag, no context menu, no keyboard.
- Repaint only on a model signal, a property change or a resize; never on a timer.
- A chunk is not outlined or separated in the grid; chunk borders are visible only through colour changes.

### 2.5 Chunk table

Header 22 px, rows 24 px, alternating `bg` / `bgAlt`, selected rows `selection`; a `stopped` row at 45 % opacity. Widths
persist under `chunkTableColumns`.

| Header | Default / min | Cell | Source |
|---|---|---|---|
| "State" | 80 / 50 | the raw state word, 11 px bold; colour by the JOB state map (lines 189-198): `completed` light blue, `stopped` muted, **everything else — pending, assigned, failed — the same grey** | `state` |
| "Frames" | 120 / 60 | "12" or "12-20", mono 12 | `frame_start`, `frame_end` |
| "Node" | 160 / 60 | the host name, the raw node id when unknown, "—" when unassigned; mono 12, elided in the middle; tooltip (600 ms) = the raw node id | `assigned_to` |
| "Progress" | 120 / 60 | a bar, column − 6 wide: frames in `completed_frames` / frames in the chunk. No number | `completed_frames` |
| "Duration" | 70 / 50 | `completed_at − assigned_at` formatted as in 1.5, only when both are set and positive; else "—". A running chunk shows "—": there is no live elapsed time per chunk | timestamps |
| "Retries" | 60 / 40 | the count; amber when > 0, muted when 0 | `retry_count` |

No chunk number column, no start time, no end time, no per-frame time, no failure reason, no list of the nodes that
failed it.

Selection: `ListSelection` keyed by chunk id, with the same mouse and key gestures as the job list (no Return action, no
Delete). A left click of any kind, and every keyboard move, also calls `pinPreview(first, last)` for that row
(lines 98-102, 585-590). The selection is cleared when the current job changes.

Context menu (lines 124-187), computed when it opens:

| Item | Label | Enabled | Does |
|---|---|---|---|
| 1 | "Reassign to another node", or "Requeue (resume stopped chunk)" when every selected chunk is `stopped`; with several chunks a suffix " (%1 chunks)" | always | `reassignChunk(id, "")` for every selected chunk |
| 2 | "Stop chunk"; with several selected "Stop %1 chunk(s)" (%1 = how many are pending or assigned); "Stop chunk (needs leader upgrade)" without the `chunk_stop` capability | capability AND at least one pending or assigned | `stopChunk(id)` for the pending and assigned ones |
| — | | | |
| 3 | "Submit as separate job…"; with several "Submit frames %1–%2 as separate job…" (the span from the lowest first frame to the highest last frame, gaps included) | always | `openChunkResubmitEditor(job, lo, hi)` |

What "Reassign" really does (`database_manager.cpp:1021-1066`, `monitor_app.cpp:1107-1122`): chunks in `assigned`,
`failed` or `stopped` go back to `pending` with the assignment cleared; `pending` and `completed` chunks are untouched,
silently. The node that was rendering is not told to stop and is not put on the chunk's blacklist, so the dispatcher may
hand the chunk to the same node again, and the first render keeps running until it ends by itself. Nothing in the UI
picks a target node (the `target_node` parameter exists and is never used).

"Stop chunk" (`monitor_app.cpp:1155-1219`): state → `stopped`, the node rendering it gets an abort; the chunk leaves the
job's totals; the job can then complete as "partial".

No action here confirms.

### 2.6 Logs per chunk ("Task Output", `LogPanel.qml:323-494`, `app_bridge.cpp:483-602`, `666-694`)

The chunk table and the log view are NOT linked: selecting a chunk row does not select its log, and there is no
"show log" item on a chunk.

- Source: the Logs tab with the source "Task Output". Files are read straight from the share:
  `<farm>/jobs/<job id>/stdout/<node id>/<range>_<timestamp ms>.log`, one file per render attempt.
- The list (left pane, 220 px, min 140; rows 24 px, mono 11): "f<range>  HH:MM:SS" where the range is the file's own
  text — zero-padded to six digits each side ("f000001-000010") — and the time is the file's timestamp in local time.
  Tooltip (400 ms): "<node id> — <range>". Order: by range text, then by time (older first). The node's host name is
  not shown.
- Single selection; click, or Up / Down / Home / End once the list has focus. Nothing is selected at first.
- The right pane shows the whole file, one row per line, wrapped, alternating fills; a line containing "[ERROR]" is red,
  "[WARN]" amber.
- Refresh: the folder is listed and the selected file is re-read in full every 3 s, on the UI thread, while the source
  is "Task Output" (whether or not the Logs tab is in front). The selection is kept by file path across refreshes.
- Empty texts: see section 6.

---

## 3. Preview frames

### 3.1 Where it is and when it exists

The preview is NOT in Job Detail. It is at the bottom of the Node panel (`NodePanel.qml:443-492`): a 32 px header
("Preview" + "· <job id>") and the pane at a fixed height (240; the value is read from the settings key
`nodePanel/previewHeight` but nothing in the UI changes it). It is shown only when the build has the image libraries
AND a job is current. Hiding the Node panel (View > Node Panel) hides the preview with it and stops its timer.

### 3.2 Where the picture comes from

There is no HTTP route and no thumbnail file on the share. Each node opens the rendered frame itself
(`AppBridge::previewInfoForRange`, `app_bridge.cpp:1569-1727`), **synchronously on the UI thread**:

1. The job's `output_dir` from the cache, translated through this node's path mappings. Not a directory here → nothing.
2. A file-name pattern from the first output flag's value (on a follower: `output_stem` from the summary): the part
   after the last separator, regex-escaped, each run of `#` → `(\d+)`, `[####]` unwrapped to digits, and when there is
   no `#` at all `(\d+)\.\w+` is appended. Case-insensitive, whole-name match.
3. One pass over the directory, **not recursive**, regular files with the extensions `.exr .png .jpg .jpeg .tif .tiff
   .bmp .webp .gif .tga`:
   - with a pattern, a matching file's frame number is the first digit group;
   - with no usable pattern, the frame number is the digit run just before the extension;
   - **the file with the HIGHEST frame number wins** — not the most recently written;
   - only when no file yields a frame number does the newest modification time decide (and never for a pinned range).
4. Result: `{found, file, frame, isExr}`.

Consequences: sub-folders (per-pass or per-layer folders) are never searched; a second output of the same job is never
shown; with chunks rendering out of order "latest" is the highest-numbered frame on disk, which can sit still while
lower frames arrive.

### 3.3 Refresh

`JobPreview.qml:29-67`: the lookup runs on creation, when the job changes, when the pin changes, and every 3 s while the
pane is visible and a job is set. The picture is addressed as `image://thumb/<encoded path>?layer=<encoded layer>`
(lines 69-73) and requested at 960 px wide, asynchronously. The picture changes only when that address changes — a new
file name or a new layer. **A file rewritten under the same name (a requeue, a re-render) is not reloaded**: the address
is the same and the decoder's cache key has no modification time (3.4).

### 3.4 Decoding (`src/preview`, vendored from QCView; `thumbnail_image_provider.cpp`)

An asynchronous provider with its own pool of 4 threads. It decodes a "master" per (path, layer) and keeps it in a
least-recently-used cache of 64 MB (cost = bytes of the 8-bit image); each answer is the master scaled DOWN to the
request with smooth filtering, never up (lines 141-156, 221-230, 272-341). Requests can be cancelled while queued.

| Format | Code | How the master is made | Size of the master |
|---|---|---|---|
| EXR, single part | `EXRImageLoader::loadThumbnailsAllLayers` (OpenEXR) | ONE sweep reads every `skip`-th scanline and keeps every `skip`-th pixel (`skip = max(1, long edge / 480)`, nearest sample, no filter) for EVERY layer at once; all layers are cached; other requests for the same file wait for the sweep | `full / skip`: between 480 and 959 px on the long edge; not clamped |
| EXR, multi-part | `EXRImageLoader::loadThumbnail` | the same sampling, one part per request; the part is chosen by header name, the first part when no name matches | then clamped to 480 |
| PNG | `PNGImageLoader::loadThumbnail` (libpng) | the whole image is decoded, then pixels are picked every `skip`; 16-bit is reduced by a shift; palette, grey and transparency expanded | clamped to 480 |
| JPEG | `JPEGImageLoader` (libjpeg-turbo) | a FULL-resolution decode (there is no scaled-decode path in this copy); RGB and grey only, anything else fails | scaled to 480, smooth |
| TIFF | `TIFFImageLoader` (libtiff) | a FULL-resolution decode; 8-bit through libtiff's RGBA reader; 16-bit integer per scanline; 16-bit float and 32-bit float are read as if they were integers (wrong picture); other channel counts fail | scaled to 480, smooth |
| `.bmp .webp .gif .tga` and anything else | Qt's image reader | scaled on read to 480; depends on which Qt image plug-ins are shipped | ≤ 480 |

Colour and alpha (`thumbnail_image_provider.cpp:57-137`):

- EXR (half float): each of R, G, B goes through the sRGB transfer curve and is clamped to 0–1. No exposure control, no
  tone mapping (anything above 1.0 is white), no colour management (primaries and chromaticities ignored), no view
  transform. Infinities clamp, NaN → 0.
- 8-bit and 16-bit integer sources are shown as stored (16-bit by a shift); no ICC profile, no gamma chunk.
- Alpha is DROPPED for every format: the picture is opaque, the colour values as stored. For premultiplied EXR this is
  "over black"; a straight-alpha PNG shows whatever colour lies under its transparent pixels. No checkerboard.
- The pane's background is `#0d0d0d`; the picture is fitted with its aspect kept and 4 px margins, and is scaled UP by
  the view (smoothly, by the toolkit) when the pane is larger than the master.

EXR layers and channels (`exr_image_loader.cpp:262-340`, `430-492`, `495-665`):

- Layer list: multi-part → each part's name ("default (part N)" for a nameless part); single part → the distinct
  channel prefixes (the text before the LAST dot), with "default" first when bare channels exist; any name containing
  "crypto" is left out; "default" alone when nothing else is found.
- A layer is shown from `<layer>.R`, `.G`, `.B` (and `.A` if present, then dropped). A layer without those three
  channels (a depth layer with only `Z`, a single-channel matte, `X/Y/Z` vectors) **falls back to the bare R, G, B of
  the file** — its tile shows the beauty picture. With no layer chosen and no bare RGB, the first prefixed layer that
  has RGB is used. A file with no RGB triple at all (luminance-only, `Y`) produces no picture.
- The display window sizes the picture; a data window that differs is handled.
- No single-channel view, no channel picker, no false colour.

Not present at all: zoom, pan, a full-size view, a compare or A/B mode, a pixel readout, frame stepping, playback,
"open in viewer", "reveal file". The file's name in the footer cannot be copied.

The constant `kBrokenFileThresholdBytes` (15 KB, `image_loader.h:66`) is defined and never used in minRender's copy: no
size test is made.

### 3.5 Pinning

`AppBridge::pinPreview(first, last)` (`app_bridge.cpp:1740-1766`): runs the lookup of 3.2 restricted to frames in
`[first, last]`; if a file is found the pin is set (and the 60 s timer restarts even when the range is unchanged); if
none is found **nothing changes** — an earlier pin stays as it was. A pinned preview shows the highest-numbered frame on
disk inside the range and keeps re-running that lookup every 3 s.

| Gesture | Pin |
|---|---|
| Click a frame cell | that frame alone (the cell gets the white outline) |
| Click a chunk row (any modifier), or move the chunk cursor with the keys | that chunk's range |
| "Latest" in the footer | cleared |
| 60 s after the last pin action | cleared |
| The current job changes | cleared |

A pin needs a frame number in the file name; a job whose files carry none can only follow "latest".

### 3.6 States of the pane

| Situation | What shows |
|---|---|
| No file found (not rendered yet; output folder unreachable from this machine; no path mapping; job without `output_dir`) | "No frames rendered yet" centred; the footer reads "Preview" |
| Decoding | a 28 px spinner over the previous picture |
| Decoded | the picture |
| The file is found but cannot be decoded (half-written, truncated, unsupported variant, a path the loader cannot open) | **nothing**: an empty dark pane with the file's name in the footer, no message; it is not retried until the address changes |
| A frame not yet rendered is clicked | nothing happens (the pin is refused) |
| A huge file | EXR and PNG are sampled (EXR reads only the needed scanlines; PNG still decodes everything); JPEG and TIFF are decoded at full size in memory first. No size limit, no time-out, no progress |
| "No job selected" | the text exists (`JobPreview.qml:94`) but cannot be reached: the pane is hidden when no job is current |

Footer (32 px): the amber pin tag "◉ f%1" or "◉ %1-%2" when pinned · the file's name (mono 11, elided in the middle)
plus "  ·  <layer>" when a layer is chosen · "Latest" (broadcast icon, only when pinned) · "Layers" (stack icon, only
when the file is an EXR).

### 3.7 The layers card

`JobPreview.qml:172-272`. A modal popup 560 wide, up to 620 tall, 12 px padding, on the scrim; it closes on Escape or an
outside click (default policy) and when the job changes. Title "EXR layers". A two-column scrolling grid, cells 168 px
tall: a framed tile (accent 2 px border when chosen, else 1 px) holding that layer's picture requested at 240 px wide,
and the layer's name under it (11 px, centred, elided in the middle). The layer list is read from the file when the
card opens (a header read on the UI thread). A click sets the layer for the preview, remembers it for that job until
the application quits, and closes the card. With no layer chosen the tile named "default" is the highlighted one. There
is no "none / reset" entry: to go back, pick "default".

---

## 4. The submission form (`src/ui/qml/panels/SubmissionForm.qml`, `Main.qml:505-571`)

One form in three cards (house card, 640 wide; New Job up to 720 tall, the other two up to 760; none closes on Escape or
an outside click; the form is rebuilt on every open):

| Card title | Opened by |
|---|---|
| "New Job" | the title row's "New Job", Jobs > "New Job…", the empty detail's "New Job" |
| "Edit Job — %1" | "Edit", "Edit Job…", Return, double-click (the manifest is fetched first, off the UI thread) |
| "Resubmit chunk as new job — %1" | the chunk menu's "Submit … as separate job…" |

Selecting any job while New Job is open closes it (`app_bridge.cpp:298-300`).

### 4.1 Fixed fields

| Section | Control | New Job | Edit | Resubmit |
|---|---|---|---|---|
| "Template" | dropdown | every non-plug-in template in the cache, by name; nothing chosen at first; invalid templates are not filtered out here | the name as text; "%1 (template missing — editing raw values)" in amber when gone | as Edit |
| "Job name" | text, placeholder "e.g. shot_010_v003" | free text | read-only (the id) | free text, pre-filled "<job id>_f<first>-<last>" |
| "Range" | "Start" 0–1 000 000 (initial 1) · "End" 0–1 000 000 (250) · "Chunk" 1–10 000 (1) · "Priority" 1–999 (50); each typed or stepped | set from the template's `job_defaults` when a template is chosen (choosing another template overwrites what was typed) | the stored values | the chunk's range; Chunk = the whole range; the job's priority |
| "Job settings" | "Max retries" 0–99 · "Timeout (s, 0 = none)" 0–1 000 000 | not shown: the template's defaults are used | shown | not shown |

Negative frame numbers cannot be entered. In New Job the retries and the timeout cannot be changed.

### 4.2 Generated fields ("Flags")

The section heading shows when the template has at least one flag. One row per flag of the template, in order, **only
when `editable` or `required` is true** (line 258). Hidden flags still occupy their position in the value list.

| Part | Rule |
|---|---|
| Label (140 px, 11 px) | `info`, else the flag text, else "(positional)"; **amber when `required`** (no asterisk) |
| Field | a single-line text field holding the value; read-only when not `editable`. This is the only field kind: no number, no checkbox, no choice list, no multi-line |
| "Browse" (folder-open icon, neutral button) | only when `type` is `"file"`: a native OPEN-file dialog, filters "<filter> (*.<filter>)" and "All files (*)" (only the second when no filter), started at the field's current value; the choice replaces the field |
| Help | `help` in muted 10 px under the field, indented 148 px, wrapped |

- `type: "output"` gets no picker and no special control; it marks the flag whose folder becomes `output_dir` and whose
  name pattern feeds the preview.
- The template's `default_pattern` on a flag is parsed and never used by this form; no token is expanded in the form.
- The template's executable (`cmd`, with its `label` and `editable`) has no field; the stored path for this system is
  used.
- Defaults: the flag's `value`; on macOS shown translated through the path mappings and turned back on submit.
- Edit and Resubmit with the template present: the template's rows with the job's stored values laid over them
  (matched in order, then by flag name; `app_bridge.cpp:1901-1964`). Template gone: one editable row per stored flag,
  labelled with the flag text or "(positional)", no pickers, no help.

### 4.3 Validation and what is sent

Client side the only gate is the button: "Submit" needs a non-empty name, a chosen template and no submission in
flight. Everything else is checked on press, and only the FIRST failure is shown, in a red box at the END of the
scrolled form (it can be below the fold).

On "Submit" (`AppBridge::submitJob`, `app_bridge.cpp:1336-1451`):

1. Checks (`template_manager.cpp:319-355`): "Executable path is empty", "Job name is empty", "Job name produces an empty
   slug", "Frame start must be <= frame end", "Chunk size must be >= 1", "Required field is empty: <info>" (editable
   required flags only). No check that a path exists, that the range fits the scene, or that the name is unused.
2. The id: the name lower-cased, every character outside `a-z 0-9 _` → "-", runs of "-" collapsed, trimmed, cut to 64;
   if `<farm>/jobs/<id>` exists, "-2" … "-99" is appended (`template_manager.cpp:357-417`). The operator is not shown
   the id before submitting; the job appears under it.
3. The manifest is baked from the template and the values (an empty optional positional is dropped together with the
   flag before it), `output_dir` = the parent of the first output flag's value, retries and timeout from the template.
4. macOS: every flag value and `output_dir` are turned into the canonical form and `submitted_os` is set to "windows".
5. Leader: queued locally and reported as succeeded AT ONCE (no spinner). Follower: `POST /api/jobs`
   `{manifest, priority}`; a 22 px spinner beside the button until the answer; failure text = the leader's body, or
   "Leader did not respond".
6. Success: the card closes and the new job becomes the current job (its row may not exist yet for up to 2 s).

Resubmit: with the template present it goes through the same `submitJob` with the template's id; with the template
gone, `submitManifestResubmit` copies the stored manifest, replaces the values, range and chunk size, and gives it a new
id, submitter and time.

### 4.4 Edit

Footer: "Cancel" · spinner while busy · "Apply to next chunks" (primary, play icon; mode `continue`) · "Stop chunks &
apply" (neutral; mode `restart`; confirms) · "Start over" (danger; mode `startover`; confirms). A change of Start, End
or Chunk shows the amber line "Frame range / chunk size changed — only Start over can apply this." and disables the
first two. The two confirmations are stock OK / Cancel dialogs 380 wide (texts in section 6). The manifest is fetched
again before applying; the card closes only when the leader acknowledges; outcomes come as a toast ("Edit applied to
%1", or the queued message) or as the red box.

---

## 5. Nodes and jobs (`src/ui/qml/panels/NodePanel.qml`, `Main.qml:293-325`)

Who renders what is visible in three places only:

| Place | Shows |
|---|---|
| Peer row, line 2 | "rendering · %1" with the JOB id the node reports. Not the chunk, not the frames, not a percentage (the peer record carries `active_chunk`; the model drops it) |
| Chunk table, "Node" | the host name per chunk; frame grid tooltip the same |
| Tray tooltip | this node's own job |

This node's own card does not say what it is rendering. There is no "jump to the job" from a peer and no "show this
node's chunks".

Peer row (52 px): a 10 px dot — red when suspended, grey when offline, else green `ok` / amber `reconnecting` / red
`needs_attention`; line 1 the host name (node id when empty; muted when offline), "LEADER" (accent, 9 px bold) on the
leader, a red warning icon when suspended; line 2 by precedence: "suspended · %1 failures · no work assigned" (red) →
"offline" → "rendering · %1" → "idle" → "stopped". Order: alive first; then rendering, active, stopped; then host name,
then id (`nodes_model.cpp:124-141`). This node is not in the list. Refresh: every 50 ms from the peer snapshot, with the
leader's suspension list (polled every 3 s) merged in.

Stop / resume:

| Control | Effect |
|---|---|
| This node's "Stop" / "Resume" button; the tray's item | toggles this node between active and stopped at once |
| Peer menu "Stop" / "Start" (+ " (%1)" with several) | sent to each live peer it applies to; no feedback but a log line |
| Peer menu "Restart App" | sent to each live peer; **no confirmation**, even while that peer is rendering |
| Peer menu "Unsuspend" | clears the leader's suspension for each suspended target |
| Offline peers: "Write restart signal (filesystem)" | a file on the share |

Suspension: the title-row dot (red, "Suspended by the leader"), the banner in this node's card with "Unsuspend this
node", and the peer row. Leader badge: "LEADER" pill in the title row on the leader itself; "LEADER" text on the
leader's peer row.

---

## 6. Strings, shortcuts, tooltips (verbatim)

`%1`, `%2`, `%3` are substituted. "…" is the single ellipsis character U+2026; "—" is U+2014; "·" is U+00B7; "●" is
U+25CF; "◉" is U+25C9; "–" in the resubmit label is U+2013.

### 6.1 Job list

| String | Where |
|---|---|
| "Jobs" | strip |
| "· " + count; " · " + "%1 selected" | strip |
| "Edit", "Pause", "Resume", "Retry failed", "Requeue", "Archive", "Cancel", "Delete" | strip buttons |
| "The leader is running an older version without job editing" | tooltip of "Edit" when the capability is missing |
| "Pri", "Name", "State", "Progress", "Duration", "Created" | headers |
| "%1/%2" | progress count |
| "%1 stopped chunk(s) excluded" | tooltip of "partial" |
| "—" | no duration / no date |
| "active", "paused", "cancelled", "completed", "failed", "partial" | state cell (wire words; "partial" is display only and is NOT wrapped for translation) |
| "Pause", "Resume", "Edit Job…", "Edit Job… (needs leader upgrade)", "Retry failed chunks", "Requeue", "Cancel", "Archive", "Delete" | context menu |
| " %1 jobs" | context-menu suffix |
| "Cancel %1 job%2?", "Delete %1 job%2?" | confirm headings |
| "This stops dispatch and immediately kills in-flight renders on all nodes. The jobs can be requeued later." | confirm body |
| "Removes job records and SQLite history. Output files on disk are NOT deleted. This cannot be undone." | confirm body |
| "Cancel", "Cancel jobs", "Delete" | confirm buttons |
| "42s" / "3m 12s" / "1h 4m" shapes (letters s, m, h unwrapped) | duration |

### 6.2 Job detail

| String | Where |
|---|---|
| "No job selected"; "Pick one in the Jobs list, or click New Job."; "New Job" | empty state |
| "%1 / %2" | header progress |
| "● %1 rendering", "● %1 failed", "priority" | header line |
| "Pause", "Resume", "Retry failed", "Requeue", "Open output", "Cancel", "Delete" | actions |
| "Reveal the job's render folder in the file manager. Uses Path Mappings to translate canonical paths." | tooltip of "Open output" |
| "Frame %1"; "\nChunk %1  (%2-%3)" (two spaces); "  ·  %1" | grid tooltip |
| "State", "Frames", "Node", "Progress", "Duration", "Retries" | chunk headers |
| "pending", "assigned", "completed", "failed", "stopped" | chunk state cell, badge (upper-cased) and grid tooltip |
| "Reassign to another node", "Requeue (resume stopped chunk)", " (%1 chunks)" | chunk menu |
| "Stop chunk", "Stop %1 chunk(s)", "Stop chunk (needs leader upgrade)" | chunk menu |
| "Submit as separate job…", "Submit frames %1–%2 as separate job…" | chunk menu |

### 6.3 Preview

| String | Where |
|---|---|
| "Preview" | header in the Node panel; also the footer when nothing is found |
| "· " + job id | header |
| "No job selected", "No frames rendered yet" | pane |
| "◉ f%1", "◉ %1-%2" | pin tag |
| "  ·  " + layer | footer suffix (unwrapped) |
| "Latest", "Layers" | footer buttons |
| "EXR layers" | card title |
| "default", "default (part %1)" | layer names produced by the decoder (unwrapped) |

### 6.4 Submission and edit

| String | Where |
|---|---|
| "New Job", "Edit Job — %1", "Resubmit chunk as new job — %1", "Edit Job" | card titles |
| "Template", "Job name", "Range", "Job settings", "Flags" | section headings |
| "%1 (template missing — editing raw values)" | template line |
| "e.g. shot_010_v003" | placeholder |
| "Start", "End", "Chunk", "Priority", "Max retries", "Timeout (s, 0 = none)" | field labels |
| "Frame range / chunk size changed — only Start over can apply this." | warning |
| "(positional)" | label of an unnamed flag |
| "Browse" | file button |
| "%1 (*.%1)" built as filter + " (*." + filter + ")"; "All files (*)" | file dialog filters (unwrapped) |
| "Cancel", "Submit", "Submit as new job" | footer |
| "Apply to next chunks", "Stop chunks && apply" (shown "Stop chunks & apply"), "Start over" | edit footer |
| "In-flight chunks finish with the old settings; every chunk dispatched from now on uses the new ones." | tooltip |
| "Kills in-flight renders and requeues them with the new settings. Completed chunks keep their output." | tooltip |
| "Discards ALL progress — every chunk, including completed frames, re-renders with the new settings." | tooltip |
| "Stop in-flight chunks?"; "In-flight renders are killed and requeued with the new settings. Chunks that already completed keep the output they rendered with the old settings." | confirm |
| "Start over?"; "All chunks — including completed frames — are reset and re-rendered with the new settings. In-flight renders are killed immediately." | confirm |
| "OK", "Cancel" | the stock dialog's buttons (the toolkit's own text) |
| "Backend not ready", "Unknown template: %1", "Failed to generate job slug", "Leader did not respond" | red box, submit |
| "Executable path is empty", "Job name is empty", "Job name produces an empty slug", "Frame start must be <= frame end", "Chunk size must be >= 1", "Required field is empty: %1" | red box, validation (unwrapped, from `template_manager.cpp`) |
| "Invalid frame range or chunk size", "No source manifest to resubmit from", "Stored manifest is invalid: %1", "Job changed while editing — reopen the editor" | red box, resubmit |
| "No job is being edited", "Frame start must be <= frame end", "Chunk size must be at least 1", "Template no longer available: %1", "Template changed while editing — reopen the editor", "Required flag is empty: %1", "Template has no executable for this OS" | red box, edit |
| "The leader is running an older version without job editing — update the leader node", "No response from the leader — check the farm connection and try again", "The job no longer exists on the leader", "The leader's database isn't ready — try again shortly", "Edit rejected: %1", "Leader error: %1" | red box, edit answer |
| "Leader unreachable — check the farm connection", "The job no longer exists on the leader", "Couldn't fetch the job manifest: %1", "The stored job manifest is invalid" | toast, opening the editor |
| "Edit applied to %1"; "Leader unreachable — edit queued to the farm and applies when the leader reconnects" | toast |

Note the two wordings for one condition: "Chunk size must be >= 1" (submit) and "Chunk size must be at least 1" (edit);
"Required field is empty: %1" (submit) and "Required flag is empty: %1" (edit).

### 6.5 Task Output and peers

| String | Where |
|---|---|
| "Task Output" | log source |
| "f%1  %2" (two spaces; unwrapped) | chunk log label |
| node id + " — " + range | chunk log tooltip (unwrapped) |
| "%1 chunks" | toolbar count |
| "Select a job in the Jobs list to view its task output." | no job |
| "This job hasn't written any chunk output yet. Output appears once a node starts rendering a chunk of this job." | no files |
| "Select a chunk on the left", "(Empty)" | right pane |
| "Peers", "%1 online" | header |
| "LEADER" | peer row, title row |
| "suspended · %1 failures · no work assigned", "offline", "rendering · %1", "idle", "stopped" | peer line 2 |
| "%1 nodes", "Stop", "Start", "Restart App", "Unsuspend", " (%1)" | live-peer menu |
| "%1 offline nodes", "Write restart signal (filesystem)" | offline-peer menu |
| "Stop", "Resume" | this node's button |
| "SUSPENDED"; "The leader benched this node after %1 chunk failures within 5 minutes. It will not be given work until unsuspended, and the suspension does not clear on its own."; "Unsuspend this node" | banner |
| "Accepting work", "Stopped — not accepting work", "Suspended by the leader" | title-row dot tooltip |

### 6.6 Keyboard

| Key | Where | Effect |
|---|---|---|
| Up, Down, Home, End, PageUp, PageDown (+ Shift) | jobs, chunks, peers | move / extend |
| Ctrl / Cmd+A | jobs (also the menu item "Select All Jobs", live only while the job list has focus), chunks, peers | select all |
| Escape | jobs, chunks, peers | clear the selection |
| Return / Enter | jobs | Edit Job (one selected, capable leader) |
| Delete; Ctrl / Cmd+Backspace | jobs | the Delete confirmation |
| Up, Down, Home, End | Task Output's file list | move the single selection |
| Tab | — | each list takes focus on Tab; inside the cards the toolkit's default order |

No shortcut exists for New Job, Pause, Resume, Cancel, Requeue, Retry, Archive, the tabs, the preview, the layers card
or any chunk action. The confirm cards have no default button and no Escape.

### 6.7 Tooltips

Default 600 ms delay, 5 s time-out, fade in 80 ms. Exceptions: "partial" 400 ms; frame grid 300 ms; Task Output file
row 400 ms. All tooltips in these panels are listed in the tables above (Edit, "partial", Open output, the frame grid,
the chunk Node cell, the three edit buttons, the title-row dot, the Task Output row).

---

## 7. Timers, animations, timing dependencies

| What | Interval | Runs when | Where |
|---|---|---|---|
| Application pump: backend `update()`, bridge `refresh()` (jobs, current job, peers, templates, log sources) | 50 ms | always | `main_qt.cpp:356` |
| Backend job cache (and templates) | 2 s, or at once when forced | always | `monitor_app.cpp:116` |
| Chunk list fetch | 3 s; at once on a job change | a job is current (any tab) | `app_bridge.cpp:119` |
| Remote log / Task Output re-read | 3 s; at once on a source change | source ≠ Monitor Log (any tab) | `app_bridge.cpp:126` |
| Preview lookup (a directory scan) | 3 s; at once on job / pin change | the preview pane is visible | `JobPreview.qml:55` |
| Preview pin auto-return | 60 s, single shot, restarted by every pin action | pinned | `JobPreview.qml:63` |
| Duration column tick | 1 s | the job list is visible | `JobListPanel.qml:112` |
| Leader contact cooldown | 5 s after a failed call | follower | `monitor_app.cpp:602`, `789` |
| Follower HTTP time-outs | 500 ms connect, 1 s read (blocking the UI thread for the chunk fetch and the jobs fetch) | follower | `monitor_app.cpp:595`, `734` |
| Delete: retry of the folder removal | 250 ms sleep on the calling thread | on error | `monitor_app.cpp:1064` |
| Toast | 6 s; fades 80 ms | | `Main.qml:620` |
| Decoder threads' idle expiry | 5 s | | `thumbnail_image_provider.cpp:229` |
| Suspension list | 3 s poll of the leader | follower | step0-contracts §9 |

Animations: row fill, button fill, text colour, the header grip — a 100 ms colour ramp (`Theme.hoverMs`); popups,
cards, tooltips, the toast — an 80 ms fade IN only (`Theme.popupMs`; no exit fade by design); the two spinners (toolkit
default). Nothing else moves: no row reordering animation, no progress animation, no grid transitions.

Timing dependencies to keep in mind for the port:

- Selection keys survive refreshes because they are ids; the drag relies on model pushes being paused.
- The priority field must not be overwritten while it has focus.
- A newly submitted job is made current before its row exists.
- On a follower the UI thread is blocked by the two HTTP reads and, on every node, by the preview's directory scan, the
  Task Output re-read and the EXR header read of the layers card. A port that moves these off the UI thread changes
  nothing visible except the stalls.

---

## 8. What the first inventory missed or got wrong

Wrong:

| # | First inventory | What the code does |
|---|---|---|
| 1 | "Files under 15 KB are treated as broken" (§5) | The constant exists and is never used in this copy; no size test is made |
| 2 | JPEG: "scaled decoding for thumbnails" (§5) | No thumbnail path for JPEG: a full-resolution decode, then a scale. TIFF likewise |
| 3 | "the preview is at most 480 px on its long edge" (§5, finding k) | True for PNG, JPEG, TIFF, multi-part EXR and the fallback formats. A single-part EXR master is `full / floor(long edge / 480)`: 480 to 959 px |
| 4 | Chunk states "pending, assigned, rendering, completed, failed, stopped" (1.8) | `rendering` is never a chunk state. The "rendering" counts are chunks in `assigned` |
| 5 | The job states were given as coloured or "others grey" without naming them (1.7) | `failed` is a real, common job state and it is one of the grey ones, in the list and in the badge |
| 6 | The frame grid's "rendered" colour was tied to "a chunk that is not yet complete" (1.8) | It also shows on failed chunks and on chunks back in `pending` after a failure: `completed_frames` survives those |
| 7 | "Shows the newest rendered frame" (1.9) | It shows the HIGHEST-numbered frame on disk; modification time is only the last fallback |
| 8 | The chunk table "States shown as text" with no word on colour (1.8) | The chunk state is coloured with the job-state map, so only `completed` and `stopped` get a colour; `failed` is not red |
| 9 | The manual's pictures were taken as showing the UI's shape | They show an earlier layout (see the note at the top) |

Missed:

1. After Delete or Archive the detail panel keeps the removed job as the current job (section 9, item 1).
2. The preview lives and dies with the Node panel; "No job selected" in the pane is unreachable.
3. The preview's lookup is a synchronous, non-recursive directory scan on the UI thread every 3 s, and once per chunk
   click or arrow key; the picture cache has no modification time, so a re-rendered file keeps its old picture.
4. A file that is found but cannot be decoded shows an empty pane with no message.
5. EXR details: nearest-sample thumbnails; alpha dropped; no exposure; "crypto" layers hidden; a layer without R, G, B
   shows the beauty picture; multi-part versus single-part paths; a luminance-only file shows nothing.
6. "Reassign to another node" does not abort the running render, does not exclude the node, and is a silent no-op on
   pending and completed chunks.
7. Task Output is not linked to the chunk table; its labels are six-digit zero-padded ranges with no host name; one file
   per attempt.
8. On a follower a failed chunk fetch empties the chunk table and greys the whole grid for up to 5 s; the job list goes
   stale with no sign; the fetches block the UI thread.
9. Requeue (list, menu, detail) and Archive never confirm; Restart App on a peer never confirms.
10. Strip actions clear the selection but leave the job in the detail panel.
11. The form: the id is a slug the operator never sees before submitting; only the first validation error shows, at the
    end of the scroll; the leader path reports success at once; retries and timeout cannot be set in New Job; choosing
    another template overwrites typed numbers; invalid templates are not filtered from the picker; `default_pattern` is
    unused; two wordings for the same errors.
12. Information that has no place in today's UI: a job's template, frame range, chunk size and submitter (shown by the
    previous toolkit's UI, visible in the old pictures); a chunk's failure history (`failed_on`); the chunk a peer is
    rendering (`active_chunk`).
13. The current-job map lacks the stopped count — the reason the detail badge cannot say "partial".
14. The plain job actions give no feedback on failure (a log line at most); only the editor paths toast.
15. The 2 s cache is not forced after pause, resume, cancel, requeue, archive, retry or delete, so their effect lags.
16. The chunk "Duration" has no live value while a chunk renders; the job's does.

---

## 9. Things that look broken or inconsistent today

For the owner to rule on; listed, not fixed. Letters a–z of the first inventory stand; these are new or sharpened.

| # | What | Where |
|---|---|---|
| 1 | **A ghost detail after Delete / Archive** (and when another node removes the job): `currentJobId` is never cleared, so the detail shows the bare id, a "—" badge, "0 / 0", priority 50, an empty table, a one-cell grid, and live buttons that act on a job that is gone. The "No job selected" state returns only by selecting something else | `JobListPanel.qml:49-54`, `ListSelection.qml:95-98`, `app_bridge.cpp:356-365` |
| 2 | **Chunk state colours use the job-state map**: `failed` chunks are grey, not red; `assigned` is grey; `completed` is the light blue of a completed job | `JobDetailPanel.qml:189-198`, `602` |
| 3 | **The job state `failed` has no colour** in the list or the badge, while `cancelled` is red | `JobListPanel.qml:84-93` |
| 4 | **"Reassign to another node" does not reassign to another node**: it returns the chunk to the pool without stopping the render or excluding the node; nothing happens for pending or completed chunks, with no message | `database_manager.cpp:1021-1045`, `monitor_app.cpp:1107-1122` |
| 5 | The chunk menu's " (%1 chunks)" shows the menu's item count (first inventory, b) — confirmed: `count` there is the menu's own property | `JobDetailPanel.qml:149` |
| 6 | **"Latest" is the highest frame number**, so with out-of-order chunks the preview does not follow what was just rendered | `app_bridge.cpp:1694-1703` |
| 7 | **A re-rendered frame keeps its old preview** (no modification time in the address or the cache key) | `JobPreview.qml:69-73`, `thumbnail_image_provider.cpp:158-163` |
| 8 | **An undecodable or half-written frame shows an empty pane** with no text and no retry | `JobPreview.qml:90-103` |
| 9 | A refused pin leaves the PREVIOUS pin in place: clicking an unrendered chunk after a rendered one keeps showing the first, with its tag | `app_bridge.cpp:1748-1751` |
| 10 | A data layer (depth, mattes, vectors) shows the beauty picture in the layers card and in the preview | `exr_image_loader.cpp:312-333`, `555-570` |
| 11 | 16-bit-float and 32-bit-float TIFF are decoded as integers; non-RGB JPEG fails; both end as item 8 or a wrong picture | `tiff_image_loader.cpp:62-73`, `137-143`; `jpeg_image_loader.cpp:118-122` |
| 12 | The preview scans the output folder on the UI thread every 3 s and on every chunk click or arrow key; on a large folder over a slow share this stalls the window | `app_bridge.cpp:1669`, `JobDetailPanel.qml:98-102` |
| 13 | On a follower a failed chunk fetch blanks the table and the grid instead of keeping the last data | `monitor_app.cpp:581-582`, `599-605`; `chunks_model.cpp:107-126` |
| 14 | The detail's Cancel, Delete (first inventory, c) AND Requeue act at once; Requeue never confirms anywhere though it discards finished work; the edit form's "Start over", which does the same, confirms | `JobDetailPanel.qml:336-358`, `JobListPanel.qml:293-299` |
| 15 | The strip offers every action for every state (Resume on a completed or cancelled job reopens it); the context menu and the detail are state-aware; the three places disagree | `JobListPanel.qml:272-306`, `641-658`; `JobDetailPanel.qml:317-329` |
| 16 | The selected count can include jobs that no longer exist (keys are never pruned); bulk actions are then sent for them | `ListSelection.qml:36-46` |
| 17 | Hiding the Node panel hides the preview; the preview's height setting has no control (first inventory, j) | `NodePanel.qml:443-492` |
| 18 | The layer remembered for a job is still shown in the footer ("· <layer>") when the current frame is not an EXR or has no such layer | `JobPreview.qml:141-148` |
| 19 | The submit error box is at the bottom of the scrolled form and can be out of sight; only one error is shown | `SubmissionForm.qml:327-343` |
| 20 | The wording pairs in 6.4; "partial" and several built strings are not wrapped for translation | — |
| 21 | Task Output labels show six-digit padded ranges ("f000001-000010") and the node only as an id in a tooltip; the chunk table shows "1-10" and host names | `app_bridge.cpp:525-546` |
| 22 | "Peers · %1 online" counts offline peers (first inventory, i); the peer row names the job but not the chunk although the chunk is in the peer record | `NodePanel.qml:193`, `nodes_model.cpp:60-95` |
| 23 | The manual: its pictures show an older layout; it says "New Job in the Jobs panel" (it is in the title row), that a peer's log is "streamed from the agent" (it is read from the share), that a drop across priority groups is refused, that required fields carry an asterisk, that selecting jobs "reveals" the strip | `docs/monitor.md`, `docs/submit.md` |
| 24 | The frame grid tests for a chunk state `rendering` that does not exist (dead branch); harmless | `frame_grid.cpp:224-226` |
| 25 | To verify on Windows before relying on it: the PNG and JPEG loaders open files with the narrow C `fopen` on a UTF-8 path; an output path with non-ASCII characters may fail to preview unless the process runs with the UTF-8 code page. Not proven from the source alone | `png_image_loader.cpp:182-187`, `jpeg_image_loader.cpp:62` |
| 26 | To verify: a chunk's Progress bar is `completed_frames / frames`, independent of its state; a `completed` chunk of a template that reports no per-frame output would show an empty bar next to "completed" (the grid paints it full) | `chunks_model.cpp:67-74`, `frame_grid.cpp:245-246` |

---

## 10. Open questions for the owner

Each with a proposed default, to be ruled and dated in the canon before anything is built.

1. **Ghost detail (9.1).** Carry it, or return to "No job selected" when the current job disappears? [proposed: fix —
   clear the current job when it leaves the list]
2. **Chunk and job state colours (9.2, 9.3).** Carry the greys, or give chunks their own map (failed red, assigned
   accent, completed green as in the grid) and the job state `failed` a colour? [proposed: fix both, using the frame
   grid's colours for chunks so the table and the grid agree]
3. **"Reassign to another node" (9.4).** Keep the behaviour and the label, rename it to what it does ("Return to
   queue"), or make it do what it says (abort the render and exclude the node)? The third changes farm behaviour and
   touches the contract. [proposed: carry the behaviour, rename the label; the real reassign is a later ruling]
4. **Confirmations (9.14).** Which of Requeue, Archive, the detail's Cancel / Delete, peer "Restart App" and the chunk
   actions confirm? [proposed: Cancel, Delete and Requeue confirm everywhere; the rest stay immediate]
5. **Strip state-awareness (9.15).** One rule for the three places? [proposed: the context menu's rule everywhere —
   enabled when it applies to at least one selected job]
6. **Preview "latest" (9.6).** Highest frame number (today) or most recently written? [proposed: carry the highest
   number; it is stable and cheap. Say so in the manual]
7. **Preview staleness and failures (9.7, 9.8, 9.9).** Add the modification time to the cache key; show a text for an
   undecodable file and retry it on the next tick; clear the pin when a pin is refused? [proposed: yes to the first two;
   for the third keep the old pin but say nothing new]
8. **Preview quality.** Step 0's question 13 stands (480 px, fixed sRGB curve). Adding to it: is alpha to stay dropped,
   are data layers to stay as they are (9.10), and is anything of zoom / full-size / compare wanted? Today there is
   none. [proposed: parity — no zoom, no compare; alpha dropped; a data layer with a single channel shown as grey
   instead of the beauty picture, since `min-image` reads by layer]
9. **Preview placement (9.17).** Keep it under the peers in the Node panel at a fixed 240 px, or give it a divider, or
   move it? [proposed: keep the place, add the divider the saved setting already implies]
10. **Where the directory scan runs (9.12).** Off the UI thread in the port, with the same 3 s cadence? [proposed: yes;
    no visible change]
11. **Follower blanking (9.13).** Keep the last chunk data on a failed fetch, and mark stale data (jobs and chunks) in
    some way? [proposed: keep the last data; a small "leader unreachable" mark in the strip — a new string, so a ruling]
12. **Chunk logs.** Link the chunk table to Task Output (a "Show log" item, or selecting the newest log of the selected
    chunk)? Show host names and unpadded ranges in its list? [proposed: parity first; the label clean-up (9.21) with the
    port, the link later]
13. **Missing job facts (8, missed 12).** Template, frame range, chunk size, submitter in the detail header; the chunk a
    peer renders in its row; a chunk's failed-on nodes in a tooltip? [proposed: not in the parity port; list for after]
14. **The form.** Show the resulting id under the name field; move the error box above the footer; one wording per
    error; filter invalid templates out of the picker; retries / timeout in New Job? [proposed: error box by the footer
    and one wording now (they are fixes); the rest later]
15. **Strings.** The strings in section 6 are carried verbatim. Three built in code are not translatable today
    ("partial", the file-dialog filter, the Task Output label): wrap them in the port? [proposed: yes, through `trc`,
    text unchanged]
16. **Shortcuts.** None exists for New Job, Pause / Resume or the tabs. Carry none, or add a small set? [proposed: carry
    none; a new shortcut is a design change]
17. **Scale.** The chunk fetch returns every chunk with its full frame list every 3 s, and the grid paints one
    rectangle per frame. What is the largest job in real use (frames, chunks)? It sizes the table, the grid and whether
    the follower's fetch needs to stay whole. (Step 0's question 14, now with a concrete consumer.)
18. **The manual's pictures and text (9.23).** Retake and correct with the port, or before? [proposed: with the port's
    release, as for the other two applications]
