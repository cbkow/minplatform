# minplatform — working notes for Claude

- **State (2026-11-11):** Phase 2 — `apps/qcv` is the QCView port (binary `QCView`). **QCView 3.0.2 is RELEASED on macOS**
  (tag `v3.0.2`; the DMG, the site and the appcast live in QCView-Player, whose `main` is the home page and whose `2.x`
  branch holds QCView 2's source — read QCView's code from `2.x`). **Windows 3.0.2 is PACKED for the Microsoft Store**
  (the submission is the user's; it supersedes the 3.0.1 that was pending certification). 3.0.2 = audio files (single
  view, playlists, dual), the playbar's waveform following the timeline, and four port fixes (rows 277–280). The Stream
  card was built and REMOVED (row 244) — do not re-propose it. **Read `docs/phase2/handoff-2026-11-10.md` first** (its
  LAST sections are where things stand — the 3.0.2 section and its "Open after this release" list), then
  `handoff-2026-11-09.md` (how to run things) and the canon `docs/rust-wgpu-port.md` (rows are the rulings, each dated;
  the newest are at the end of the last Decisions table).
- **Phase 3 (minNotes) is the active work** (opened 2026-11-10): a rethink, not a port - the Qt app is read for
  purpose and frozen contracts, and "QCView's behaviour wins ties" below does NOT apply to it (row 268). As of the
  end of 2026-11-11 there is an EDITOR: `apps/mn` (binary `minNotes`) on `crates/mn-doc`, `mn-store`, `mn-layout`,
  `mn-edit` - collections opened and saved by hand (row 293), the tree, the menu bar, the toolbar / footer / sidebar
  header (rows 297-300), colours (301), whole-block selection, drags, the blocks' menu, moves to other notes and the
  clipboard's first half (303-308). Read `docs/phase3/handoff-2026-11-10.md` - its LAST addenda are where things
  stand (the LAST addendum is the place to resume) - and rows 268-337.
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
- `git push` and merges are the user's: commit locally, say how far `main` is ahead.
- Spike protocol (§21.3): one question, one pass bar with a number, one time box, one written
  verdict in `docs/spikes/`. No verdict = not done. Spikes die; they are never renamed into crates.
- `docs/spikes/P0-baseline.md` holds the measured Qt numbers every pass bar is compared against.
- PRIVACY (this repo is PUBLIC): `fixtures/` (client footage, derived fixtures, traces, logs, `sources.env`) is gitignored and
  must stay so; no tracked file names a client, a job, a source filename (a `.blend`, a clip) or a path under `~/Sync`.
  Spikes and tools take media paths from `fixtures/sources.env` or the command line, never as literals. Traces/logs/screens go
  under `fixtures/` or `docs/spikes/**/out/` (both ignored). Machine-specific recipes (the Windows box) live in the local
  Claude memory, not here. Tests on the Windows box use synthetic fixtures generated there only.
