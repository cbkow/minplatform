# minplatform — working notes for Claude

- **State (2026-11-09, evening):** Phase 2 — `apps/qcv` is the QCView port (binary `QCView`). The QCBridge round (canon row 227) is
  built: Adobe in, SRT in, the Blender card, the NDI / AJA outputs. The Stream card (step 4) was built and then REMOVED (row 244) —
  do not re-propose it. What remains is walks (AJA hardware, Premiere audio), translations and packaging. **Read
  `docs/phase2/handoff-2026-11-09.md` first**, then the canon `docs/rust-wgpu-port.md` (rows are the rulings, each dated; the newest
  are at the end of the last Decisions table).
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
