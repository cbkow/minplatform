# Phase 2 — the reference app `apps/qcv`, where `min-ui` is born

Canon: `docs/rust-wgpu-port.md` §21.4 (the slice), rows 127–130 (ruled 2026-11-06). One spike-grade step at a time, in
the §21.4 order, each with its own evidence note in this folder (question · pass bar with numbers · time box · verdict).
The spikes' proven pieces become real crates only as a step needs them, REWRITTEN as crates (row 112: never renamed); a
spike is deleted when its crate meets the spike's own numbers — every number, not the convenient ones.

| Step | Note | Crates born / grown | Status |
|---|---|---|---|
| 1 | [step1-open-present.md](step1-open-present.md) — open a file or an image-sequence frame → the fused presenter at the display's cadence | `min-present`, `qcv-player`, `qcv-timeline` (rational time only), `apps/qcv` | **DONE 2026-11-07 — GO** (carried: H.264 first frame, the software upload's staging blit, the Windows display rows) |
| 1a | [step1-qcview-audit.md](step1-qcview-audit.md) — row 131: step 1 held against QCView's player + renderer; the ports to make before step 2 | `qcv-player`, `min-present` | audit written 2026-11-07; ports DONE as step 1b |
| 1b | [step1b-qcview-decoder.md](step1b-qcview-decoder.md) — `qcv-player` re-derived from QCView's `src/decode` file by file; stage 1 from `yuv_planar.h`; the CV bridge table | `qcv-player`, `min-present` | **DONE 2026-11-07 — GO** |
| 2 | [step2-transport-timeline.md](step2-transport-timeline.md) — the scrub decoder + cache, QCView's Transport verbatim, the timeline edits + hover cache, `qcv-meta`, `min-ui`'s first nodes | `qcv-player`, `qcv-timeline`, `qcv-meta`, `min-ui`, `min-theme` | IN PROGRESS — rows 154–159 ruled 2026-11-07; **the scrub decoder + GOP cache LANDED 2026-11-07** (ProRes seeks 5.5 / 2.8 ms, sweeps 4.1 / 2.3, H.264 first frame 17.5 ms; a headless-engine leak fixed); **the Transport LANDED 2026-11-07** (WindowManager's transport slice + PlaybackTimer + the key map; `Player` retired; the stream's own seeks timed: ProRes 5.9 ms, H.264 339); **the timeline model + edits (rational time, ops with inverses, policy enforced) + the hover-thumbnail cache LANDED 2026-11-07** (a QCView move-undo bug found → §17.6); **`qcv-meta` LANDED 2026-11-07** (the extractor + probes verbatim, the XMP reader EQUAL to exiftool on the masters, the timecode formatter, the service); **the Sequencer LANDED 2026-11-07** (every source a timeline, two decoder slots + prewarm: gapless cuts at 1080p / 4K incl. mid-GOP H.264, the loop wrap in one refresh, playlist seeks exact; S2 retired); the mouse-hover pass measured (nearest frame 2–3 away on every sweep position, exact 0–6 ms after rest); next `min-ui`'s first nodes |
| 2 · 6a | [step2-transport-timeline.md](step2-transport-timeline.md) §6a — `min-theme`: the tokens as data (canon §25) | `min-theme` | BUILT 2026-11-07 (396 tokens, 7 tests) |
| 2b | audio — QCView's chain whole (row 139) | `qcv-player` (audio) | — |
| 2c | image sequences — `min-image` pulled forward (row 148) | `min-image`, `qcv-player` (the sequence cache, the upload thread) | — |
| 2d | the MXF scanner (row 137) | `qcv-meta` | — |
| 3 | rails / panels / popups / shortcut registry / focus model (§16) | `min-ui` | — |
| 4 | scopes + guides (§19) | `min-present` (the working-buffer tap), `min-ui` overlays | — |
| 5 | drag and drop (§14; S5's native layer items) | `min-ui`, `min-paths` | — |
| 6 | annotations over the frozen sidecar (`qcv-notes` + `qcv-ink`) | `qcv-notes`, `qcv-ink` | — |
| 7 | the Adobe ring (S4) | `qcv-ring` | — |
| — | the colour chain (§18, S1 step 2: OCIO route A in `min-present`'s stage-2 slot; minColor WGSL) lands with scopes (step 4), the first step that needs the working buffer | `min-present` | — |

Rules carried from Phase 1: the window layer is winit 0.31 (row 129; the text layer stays on the vendored 0.30 until
accesskit_winit follows), DX12 on Windows (row 45), the Windows display numbers wait for a real HDR monitor on rhombus
(row 130). Gate G2 (§21.4): the slice plays the user's real QC sessions side by side with Qt QCView for a week.
