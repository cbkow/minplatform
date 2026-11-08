# The project crate — `qcv-project` (canon row 141, BUILT 2026-11-07 night)

QCView's `project/` read first (`media_item.h`, `project_io.cpp`, `project_manager.cpp` 1.7 k, `media_extensions.h`),
then the crate written as a frozen contract: nothing here invents, every rule names its QCView line.

## What it is
- `model.rs` — `media_item.h` field for field: `MediaType` (7), `TransferOverride`, `VideoRange` (string form `auto / full /
  limited`), `AudioRoutingMode`, `PixelAspectMode`, `InOutRange` (−1 / −1), `PlaylistEntry` / `PlaylistData` (24 fps, 1920 × 1080,
  gap 10), `DualPairData` (clip maps opaque), `MediaItem` with its overrides (rotation −1 = Auto, custom PAR 1:1), `ProjectBin`,
  `seed_bins()` = the six (Videos open · Audio · Images · Playlists · Dual Views · Live). `VideoMetadata` / `AdobeMetadata` /
  `ImageSequenceData` are `qcv-meta`'s — the sequence data gained `cache_stride` (persisted) + `missing_frames` (memory only),
  and `VideoMetadata::default()` now carries QCView's struct defaults (bitDepth 8, SAR 1:1) so an unprobed item serialises alike.
- `extensions.rs` — the one table: 32 video (webp / gif / dpx in), 13 audio, 6 image; `.r3d / .braw / .ari` absent on purpose.
- `io.rs` — `.qcvproj` v2 BYTE-FOR-BYTE. Qt's `QJsonDocument::toJson(Indented)` as QCView's files show it: keys sorted, ONE
  space of indentation per level, `"key": value`, empty containers inline, integral doubles as integers, no trailing newline.
  `${PROJECT_DIR}` collapse (prefix + separator, `://` untouched) / expand. Save = temp + rename (QSaveFile's commit). Load:
  the schema check (`qcview.project.v2`), the migrations (no bins → the six; pre-2.2.3 → the Live bin appended; dangling
  `ui_state` ids cleared), the sanitising defaults (rotation −1 off the quarter set, SAR ≤ 0 → 1:1 with `isAnamorphic`
  recomputed, `isRgb` re-derived from the pixel-format name for pre-field caches, `transferOverride` absent = Auto).
- `manager.rs` — `ProjectManager` without signals (`revision()` instead): `new_project` keeps the bin shells; `add_media_file`
  = the extension gate, the Recents bump (also on a dedupe hit), images through `min-paths`' sequence detector (dedupe by
  folder + pattern; a lone still = a 1-frame sequence with `%` doubled), dedupe by absolute path, default name = the file
  name, routing to the bin that accepts the type (ImageSequence → Images); `add_live_stream` (dedupe by exact URL; host:port
  name); `create_playlist` (`Playlist-yyyyMMdd-HHmmss`, clips through `add_media_file`, audio skipped); `next_playlist_auto_name`
  (`Playlist_NN`); `add_dual_pair`; `remove_media_items` + `restore_last_removed` (a one-step undo in exact pool and bin
  positions, back to front); the override setters; the metadata results (`on_video_metadata` whole, `on_rotation_probed` /
  `on_container_probed` field patches that do NOT dirty — a migration on open must not read as an edit); the open-time
  probe schedule (`Probe::Rotation` for −1, `Container` for an empty container, `FullVideo` for old ProRes RAW caches).

## Tests
Unit: the `${PROJECT_DIR}` rules, the writer's shape, a round trip with the migrations and remove / restore. The real one:
`QCV_PROJECT_FIXTURES_DIR=<dir> cargo test -p qcv-project real_projects` loads every `.qcvproj` under the directory, writes
it back and compares bytes — **33 of 33 of the user's projects byte-identical** (2026-11-07). The fixtures stay on the
machine (the env var, never a path in the repo).

## Deviations, written
No signals (the shell polls `revision()`); timestamps from `std::time` (Qt's ISODate UTC form, `yyyyMMdd-HHmmss` local with
the caller's offset); `.lnk` shortcut resolution waits for a Windows session (QCView: `QFileInfo::symLinkTarget`); EXR layer
discovery at import (`populateExrImportMetadata`) waits for `min-image` (2c); the live-stream display name uses the URL's
authority (QCView adds the host-bridge labels for `qcbae://`).

## Next
Wire it into the shell: the left rail's Add Media section (the six bins as QCView's tree, drop to add, click to load), the
per-clip pickers reading and writing the active media item (Range / Mix / Origin / Aspect / Rotation persist), Open / Save /
Save As with QCView's shortcuts, Recents.

## The shell wiring (2026-11-07 night)
`apps/qcv/src/project_panel.rs` + `main.rs`. QCView's `LeftRail.qml` media card read first (the Project header, the Media
card with its count, the ListView grouped by `sectionLabel`, rows = type glyph + name elided in the middle + badges;
single click selects, Ctrl / Cmd toggles, Shift extends a range, double-click activates — "activation moved to
double-click in prep for drag-and-drop into playlists"; the drop hint "Drop media here to add to the project"). Ours: the
Add Media section at the top of the left rail (QCView's order: Add Media · … · Background · … · Settings), the project's
name with a dot while unsaved, an "Add Media…" size-S pill, the groups Videos · Audio · Images · Image Sequences ·
Playlists · Dual Views · Live with counts in the detail style, the LOADED row in the accent (row 162's "loaded-media
row": a 2-px accent bar + the accent name), selected rows in gray-300. Keys: Ctrl+O / Ctrl+Shift+O / Ctrl+S / Ctrl+Shift+S
(QCView's), Ctrl+N for New Project (no QCView shortcut; stands in for the menu), Delete / Backspace removes the selection,
Ctrl+Z restores the batch while the annotation undo has no owner (QCView: a toast's Undo — ours when toasts land).
Dialogs: `rfd` 0.18 with QCView's `nameFilters` (All media / Video / Audio / Image & sequence). Drops and the command line
go through `add_media_file` → load; a `.qcvproj` drop or argument opens the project. Load (`load_item`, QCView's
`loadRequested` handler): Video / Audio open in the Transport, then the item's range → the decoders, aspect + rotation →
the presenter, routing → the audio; sequences, playlists, dual views and live streams select only until their steps. The
Inspector's Range / Mix / Aspect / Rotation choices write the active item (Origin is per session in QCView too). The
metadata service is keyed by the media id; results feed the manager's cache and the Inspector; the open-time probes run.
Recents: `recents/media` + `recents/projects` in the store (newline-joined, 10) — shown once there is a menu. Owed: the
row context menu (Create Playlist from Selection, Input, Delete), rename, drag-to-playlist, "Can't be played here" badge
+ toast, New playlist, the unsupported-codec gate at load.

## The Add Media section's owed QCView pieces, part 1 (2026-11-08)
Read first (row 131): `LeftRail.qml` (selection model, shortcuts, `rowContextMenu`, `renameClickAway`, the row `Drag` source,
`dropZone`), `TimelinePanel.qml` (`playlistDropArea`), `ViewportOverlay.qml` / `Main.qml` (`dropMediaAt`), `ProjectManager::
renameMediaItem`. Built:
- **`min_ui::Menu`** (context menu at the pointer; disabled rows inert, separators, shortcuts at the right edge; picks on
  release; a press outside closes and is consumed; clamped / flipped inside the window).
- **The row menu**: Create Playlist from Selection (enabled when the selection holds Videos / Images / Image Sequences —
  audio is refused, as `createPlaylist`) · Delete (Del). Right-click outside the selection replaces it, inside keeps the group.
  DEVIATION: Input · Use Clip Chain of · Reset Clip Chain arrive with the colour chain (§18.6) — inert here.
- **Inline rename**: F2 (exactly one row), the new-playlist flow; Enter or a click anywhere else commits (trimmed), Esc
  dismisses, focus loss commits, empty / unchanged commits nothing (`renameMediaItem` refuses empty). IME routed.
- **New Playlist**: the Playlists header is always shown with a "+" (QCView's fallback header); `Playlist_NN` at 24 fps
  1920×1080, gap 0; the new row is selected and its rename opens. QCView also ACTIVATES it — waits for playlist loading.
- **Ctrl/Cmd+A** selects every source row (playlists and dual views skipped); **Shift** ranges stay inside their list (media
  vs Playlists), across them a plain replace.
- **Drag and drop, all four of QCView's behaviours accounted for:**
  1. OS drop on the BIN adds only (was add + load — the deviation is fixed); OS drop on the VIEWPORT adds and loads the
     first (`dropMediaAt` → `openMediaPaths`); over the bands / the Inspector the OS shows a refused drop (no DropArea there,
     `set_valid_dnd_actions` per `DragPosition`); a drop with no position from the OS falls back to the viewport.
  2. Row drag OUT of the bin is an INTERNAL drag (6-px threshold, a ghost chip under the pointer): released over the viewport
     it loads the first row; anywhere else, including outside the window, nothing happens — QCView restricted its OS drag to
     Link precisely so a drop on Finder / Explorer could not move or copy files; never reaching the OS gives that by construction.
  3. The payload is the dragged row, or every selected non-playlist row in bin order when it is part of a multi-selection; a
     playlist row never starts a drag.
  4. **Drag-to-playlist** = rows dropped on the TIMELINE while a playlist is loaded (QCView's `playlistDropArea`: snap to a
     clip edge within 12 px or append; audio refused; `admitMediaPaths` gates types). It needs playlist loading in the shell
     (`load_item` still says "selected only" for playlists) — the next piece.
- Tests: `min_ui::menu` 1, `project_panel` 5 (menu selection rule + eligibility, drag payload, drag threshold + release,
  range rule + select-all, rename commit).
- Still owed in this section: playlist loading + the playlist timeline, drag-to-playlist, the "Can't be played here" badge,
  toasts (incl. QCView's "Removed … · Undo"), the unsupported-codec gate (`admitMediaPaths`) at load and at drop.

## The playlist UI on the timeline (2026-11-08; canon row 167's playlist half)
Proposed and taken ("go"): 1 loading · 2 the bar · 3 gestures · 4 drops · 5 keys · 6 the precision popover · 7 write-back.
- `apps/qcv/src/playlist.rs`: project playlist ↔ `qcv_timeline::Editor` (QCView `startPlaylist` + `TimelineController::
  loadPlaylist`): entries resolved against the pool (missing ones skipped + logged), QCView's SECONDS trims (-1 = untrimmed)
  ↔ source frames on the clip's rate, the project's metadata or a synchronous probe for rate / frame count; `entries_from`
  writes the edited order + trims back (`ProjectManager::set_playlist_items`, the dirty dot).
- The shell: `transport_options()` / `adopt()` factor the open so a file and a playlist open alike; `load_playlist` opens it
  paused (an empty playlist = no player, the bar says "Drop media here to add to the playlist"); `after_playlist_edit`:
  trims / slips re-seat the running transport (`set_timeline`), reorder / insert / delete / split / undo rebuild it at the
  same frame and play state (the slots are clip-indexed).
- `clip_bar.rs` (20 px under the scrubber, on its camera; `frames_per_px` / `x_of_frame` now public on `Scrubber`): QCView's
  `pickEditOp` without the toggle — 8-px ends trim, body reorders, Ctrl/Cmd slips, 5-px threshold, one undoable edit per
  release; hover lights the handle, the cursor is col-resize / grab; selected = gray-800 with dark text, current = gray-400,
  hover gray-300, gaps gray-100 — no accent (row 162). Tests: trims / reorder / slip / click / double-click / drop snapping.
- Drops: `DropTarget::Playlist(x)` over the scrubber or the bar while a playlist is loaded — OS files are added to the project
  then inserted, bin rows are inserted, at the clip edge within 12 px (QCView `snapTolPx`) else appended, audio / playlists /
  dual views refused; the insertion bar follows the OS drag (`DragPosition`) and the row drag.
- Keys while a playlist is loaded: Delete / Backspace removes the selected CLIP (the bin's selection otherwise; selecting one
  clears the other), Ctrl/Cmd+K splits at the playhead, Ctrl/Cmd+Z / Shift+Z undo / redo timeline edits (the bin-restore
  stand-in keeps Ctrl+Z only when no playlist is loaded).
- `clip_popover.rs`: double-click a clip → Source in · Source out · Duration (timecode on the clip's rate from frame 0); Tab
  cycles, Enter or a press outside commits as trims (a changed duration moves the out), Esc discards, a malformed field
  turns red and stays open. Test included.
- Owed: the viewport preview while trimming (QCView's `beginEditScrub`), thumbnails in the bar, QCView's undo toast; the
  user's walk.
- Walk fixes (2026-11-08): the band backing stacks on the clip bar (it was drawn 20 px low); the bar is 28 px (the user: "make
  the tracks a little taller so text fits"); its tokens are `playlist-bar-*`: a `clip-bar-*` set from the §25 sketch (dual's 12-px bars) already
  had `clip-bar-height`, and the LATER duplicate key won, so the bar had really been 12 px, not 20; **cross-clip opens leave the UI thread** — Windows froze 6.5 s / 6.1 s scrubbing
  across 5.7K NAS clips because a non-prewarmed clip opened synchronously (QCView's `playlistAdvanceToClip` does, at ~50–100
  ms; ours took ~4 s there). `Transport::swap_to` now swaps at once only for an open / prewarmed slot; otherwise the clip opens
  on a thread (`pending`), the picture holds, the latest scrub / seek target applies on landing, a return to the active clip
  cancels it, and replaced slots are dropped off-thread (their decoders join). Bench: 12 random cross-clip seeks all land,
  p50 62 ms incl. the 50-ms debounce; playback across cuts unchanged (0 dropped, all prewarmed).

## The Media section finished: toasts, the file-type gate, the unsupported badge (2026-11-08)
- The section is titled **Media** (the user: "rename … to Media or Bins or something more logical" — QCView's own word for the
  list; "Bins" would promise folders that don't exist). The "Add Media…" pill stays.
- **`min_ui::Toast`** (QCView `Toast.qml`): one at a time (a new one replaces), top-right of the content rect, 3.4 s — 5.6 s with
  an action button, a click on it dismisses, presses elsewhere go on; positive / notice / negative = the status colour on the
  icon only; popover surface + elevation; 120-ms fade in; long text wraps at 360 px. Test included.
- **The file-type gate** (QCView `admitMediaPaths`) on every way media comes in — Open Media…, the command line, OS drops on the
  bin / viewport / playlist: one path → "X isn’t a file type QCView opens." or "X is a folder — drop a file, or any frame of an
  image sequence."; a batch → "N of M files skipped — not file types QCView opens (.ext, …)." `.qcvproj` drops still open the
  project. Owed: Windows `.lnk` resolution (refused like any unknown suffix for now).
- **Removal** toasts "Removed X" / "Removed N items" with **Undo** (QCView `undo-media-delete` → "Restored …" / "Nothing to
  restore"); the Ctrl+Z bin-restore stand-in RETIRED (Ctrl+Z belongs to the playlist's timeline undo).
- **Unsupported media** (QCView `reportUnsupportedMedia`): a failed load sets the row badge and toasts once per item per load —
  "X can’t be played here: unsupported format." or the ARRIRAW camera-named line; the row badge (QCView's `unsupported` =
  `video.unsupported_codec || load_failed`) is a warning glyph at the row's right edge with QCView's tooltip.
