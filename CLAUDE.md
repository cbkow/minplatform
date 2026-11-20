# minplatform — working notes for Claude

- **State (2026-11-19):** this day minRender 1.0.2, QCBridge 0.5.0 and UFB 2.0.7 went out (their lines below). **QCView 3.0.4 is RELEASED on macOS
  and PACKED for the Store (2026-11-19, rows 596-598, tag `v3.0.4`: ProRes RAW plays again - its mosaic made colour on
  the GPU, `bayer.wgsl`, with the clip's own colour data, linear Rec.2020 to the chain - and six camera encodings in
  minColor from minColorAE 0.1.7's core; the LAST section of `docs/phase2/handoff-2026-11-10.md`).** minColorAE 0.1.7
  is RELEASED too (GitHub `v0.1.7`, after the owner's look in After Effects). Phase 2 — `apps/qcv` is the QCView port (binary `QCView`). Before it: **QCView 3.0.3 was RELEASED on macOS
  and PACKED for the Store (2026-11-18, rows 554-583, tag `v3.0.3`: the pen path tool, container pixel aspect and Custom
  aspect, Export to minNotes, the audio tile, Blender 5.3; the last sections of `docs/phase2/handoff-2026-11-10.md`).**
  Before it: **QCView 3.0.2 was RELEASED on macOS**
  (tag `v3.0.2`; the DMG, the site and the appcast live in QCView-Player, whose `main` is the home page and whose `2.x`
  branch holds QCView 2's source — read QCView's code from `2.x`). **Windows 3.0.2 is PACKED for the Microsoft Store**
  (the submission is the user's; it supersedes the 3.0.1 that was pending certification). 3.0.2 = audio files (single
  view, playlists, dual), the playbar's waveform following the timeline, and four port fixes (rows 277–280). The Stream
  card was built and REMOVED (row 244) — do not re-propose it. **Read `docs/phase2/handoff-2026-11-10.md` first** (its
  LAST sections are where things stand — the 3.0.2 section and its "Open after this release" list), then
  `handoff-2026-11-09.md` (how to run things) and the canon `docs/rust-wgpu-port.md` (rows are the rulings, each dated;
  the newest are at the end of the last Decisions table).
- **Phase 3 (minNotes): minNotes 3.0.2 is RELEASED on macOS and PACKED for the Store** (2026-11-18, tag
  `minnotes-v3.0.2`, rows 564-583: saves on storage that refuses a full sync, a video's caption bar, the pen path on
  videos, canvases, pictures and PDF pages, notes handed over by QCView). Before it **minNotes 3.0.0 was RELEASED** (2026-11-14; opened 2026-11-10 as a rethink, not a port - "QCView's
  behaviour wins ties" below does NOT apply to it, row 268). `apps/mn` (binary `minNotes`) on `crates/mn-doc`,
  `mn-store`, `mn-layout`, `mn-edit`. macOS: a notarized DMG on the minNotes repository's release `v3.0.0`, the Sparkle
  feed carries it. Windows: the Microsoft Store only (the Store is the updater; no installer) - the package was
  SUBMITTED by the user; the work resumes on Partner Center's answer or a bug. The minNotes repository's `main` is the
  site, the manual and the feed; its `2.x` branch is minNotes 2's source. **Read `docs/phase3/handoff-2026-11-10.md`:
  its LAST section ("Where minNotes stands after the 3.0.0 release") is the place to resume**, with the runbook just
  above it, and rows 268-497.
- **Phase 4 (minRender): minRender 1.0.2 is RELEASED on macOS and PACKED for the Microsoft Store** (2026-11-19, rows
  593-594, tag `minrender-v1.0.2`: on Windows the kill works from the Store package again, the empty job detail; a kill
  test on Windows must run from INSIDE a package). Before it 1.0.1 (2026-11-15; parity with a live 0.5.x farm, rows 387-433). `apps/mr` (binary `minRender`: node, leader, window, tray and the leash in one
  executable). macOS: a notarized DMG on the minRender repository's release `v1.0.2`, the Sparkle feed carries it.
  Windows: the Store only - the package was built for the owner to submit. The minRender repository's `main` is the
  site, the manual and the feed; its `0.5.x` branch is 0.5.11's source. **Read
  `docs/phase4/step10-store-release-flow.md`** and rows 421-432 before touching packaging; a farm's folder on storage
  that syncs is a FEATURE (rows 429-430).
- **Phase 5 (UFB): UFB 2.0.7 is RELEASED on macOS and PACKED for the Store (2026-11-19, row 595, tag `ufb-v2.0.7`: a job in the sidebar opens as a folder, Open as Job, New Web Link in an entry's menu, the link glyph; read `docs/phase5/handoff-2026-11-17.md`'s LAST section, "Where UFB stands at the end of 2026-11-19").** Before it: **UFB 2.0.6 was RELEASED on macOS and PACKED for the Store (2026-11-18, rows 584-592, tag `ufb-v2.0.6`: links pressed in a Links cell, web links as files, New minNote, the dated folders, the double click as a preference, and on Windows a drive letter the system remembers is not chosen; read `docs/phase5/handoff-2026-11-17.md`'s LAST section, "Where UFB stands at the end of 2026-11-18").** Before it: UFB 2.0.4 was RELEASED on macOS (2026-11-17, row 553: the disk image through `appcast.xml`; nothing is taken over from 1.4.x any more, row 549; tag `ufb-v2.0.4`) and PACKED for the Store. Before it 2.0.3 (row 548; the LAST version with the way up from 1.4.x - rows 546-547: two feeds, `appcast-mac.xml` frozen, the next release ships the disk image through `appcast.xml` and takes nothing over; **read `docs/phase5/handoff-2026-11-17.md`'s LAST section, "Where UFB stands at the end of 2026-11-17" - the release as it is made now, and where the steps for a Store comment are**; Windows is PACKED for the Store, the submission the owner's). Before it: 2.0.2 (row 544: the UFB repository's release `v2.0.2`, its `main` = the site, the manual and the feed; worktree `../ufb-2.0`, branch `2.0-release`; an installed 1.4.x is offered it) and PACKED for the Microsoft Store (the submission is the owner's); tags `ufb-v2.0.2`, `ufb-v2.0.3`, pushed. The local branch `backup/pre-rewrite-2026-11-17` is never pushed. Rows 536-544: the release identity, the installer package, the Store package, tag `ufb-v2.0.1`; the UFB repository's new tree is its branch `2.0-release`, worktree `../ufb-2.0`, its old source the branch `1.4.x`; resume from the LAST sections of `docs/phase5/handoff-2026-11-17.md`).** Its history: open since 2026-11-15, five spikes answered (S15-S19); `apps/ufb` is a browser with thumbnails, a context menu and file operations - new folder, rename, duplicate, trash, copy and move with the conflict question, drag and drop, undo - the lightbox for stills, video, sound, sequences, text, PDF and (macOS) HTML, image sequences as one entry (slices 2a-4c and 5, rows 463-479), optional tabs (rows 480-481), and the first step of the project side: jobs in the sidebar, a job in a tab with its sections and items, New Job and Add from the project type, on a test database with 1.4.2's schema (slice 6a, rows 482-488) and the items' metadata - the star, columns, cells edited in place (slice 6b first walk, rows 489-490), the tracker and All Tracked (second walk, row 491), the board (third walk, row 492), the metadata following renames with Relink and Remove (fourth walk, row 493); shared path identities and the mesh's node on a TEST epoch (slice 7, rows 494-497: the node, the snapshot, the folder alone as the fallback, edits merged by cell, the shared secret); saved shares in the sidebar, found where the system has them, and the Sync entry at its foot (slice 8, rows 498-501); a studio's own project types per jobs folder (slice 6c, row 502); links and copied paths (row 503); the node checked beside a genuine 1.4.2 node in isolation (row 504, `tools/ufb-oldnode`); the tray, the application staying after its last window, `ufb:` and `union:` links (row 505) - behind a safety belt (it changes nothing outside the folders it is started with, row 468); run by script on both systems (rows 470, 477, 478).** `../ufb` (1.4.2) was read into six specs in
  `docs/phase5/`; the owner then ruled the course item by item (rows 434-505): a file browser that works like one,
  "a window, not a file system replacement"; optional tabs (hidden until asked for, row 480), a dual pane, real menus, native file operations with undo; no
  cache layer, no agent, no third-party driver; **resume from `docs/phase5/handoff-2026-11-17.md`** (built, never
  exercised, the agreed order of what is next); mounts are "ask and watch"; metadata and the mesh stay compatible
  with 1.4.2 and are mended additively; a project-type schema. It will live in `apps/ufb`. Read rows 434-505 first.
- **QCBridge (the Blender bridge; its own repository `../QCBridge`, GPL-3): 0.5.0 is RELEASED (2026-11-19, GitHub `v0.5.0`).**
  Its agent's settings window is built on THIS repository's `min-ui`, `min-theme` and `min-present` as path dependencies
  (`../../minplatform/crates/...`, `agent/src/settings/window.rs`): a change to those crates' public API can break the
  agent's build - build `../QCBridge/agent` after one, and a QCBridge release names a pushed commit here. The round's
  record (the start measured and mended within SRT - Kyber was weighed and dropped again; a geometry-nodes bake on the
  replica; the window; the release as it is made) is in the local Claude memory, not in this repository; QCView's part
  of it is the timing lines of a live stream's start in the log (`stream_feed.rs`, `min-ffmpeg`'s live open).
- **Talk before building.** Design changes get a scoped proposal — "rulings needed" with [proposed] defaults — and are built on
  the user's go. Every ruling is dated in the canon. A new step starts with an agent reading QCView's code into a spec.
- **QCView's behaviour wins ties.** Read QCView-Player's code before porting a piece; a divergence needs a written reason (and a
  QCView bug found on the way is listed for the user to rule on, not fixed silently).
- **Carry QCView's shortcuts** with every ported panel (grep its Main.qml, the C++ key filter, `key_map.rs`); the key goes in
  the tooltip.
- **Strings:** every UI string through `min_i18n::trc("<QCView's context>", …)`; a new string gets drafts in all 11 languages via
  `tools/i18n/add_drafts.py` (marked drafted / unreviewed).
- **Walks:** build `--release`, launch the app for the user's walk after each change, never kill an instance the user may be
  using (ask them to quit it), never send synthetic input to the user's desktop.
- The sibling repos (`../QCView-Player`, `../minNotes`, `../ufb`, `../MinRender`, the QCBridge companions) may be
  edited ONLY on a branch that never touches `main` (user 2026-11-07: "you can touch other repos as long as we work in a
  branch for now without affecting main"); pushing the branch is fine, merging is the user's call. Measure and read them
  freely; vendor frozen contract sources with a stamp.
- **Pushing is asked for ONCE PER SESSION** (the owner, 2026-11-19: "request permission once per session ... there might
  be a time where we are experimenting and don't yet want to push"). At the first commit of a session that could be
  pushed, ask whether this session may push as the work goes. With a yes, push your own commits to `main` here and to
  working branches of the sibling repos without asking again that session; with a no, or before the answer, commit
  locally and say how far `main` is ahead. A yes does not carry to the next session. Merging a sibling repo's `main`,
  tags and releases are on the owner's word each time, whatever the answer.
- Spike protocol (§21.3): one question, one pass bar with a number, one time box, one written
  verdict in `docs/spikes/`. No verdict = not done. Spikes die; they are never renamed into crates.
- `docs/spikes/P0-baseline.md` holds the measured Qt numbers every pass bar is compared against.
- PRIVACY (this repo is PUBLIC): `fixtures/` (client footage, derived fixtures, traces, logs, `sources.env`) is gitignored and
  must stay so; no tracked file names a client, a job, a source filename (a `.blend`, a clip) or a path under `~/Sync`.
  Spikes and tools take media paths from `fixtures/sources.env` or the command line, never as literals. Traces/logs/screens go
  under `fixtures/` or `docs/spikes/**/out/` (both ignored). Machine-specific recipes (the Windows box) live in the local
  Claude memory, not here. Tests on the Windows box use synthetic fixtures generated there only.
