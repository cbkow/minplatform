# Phase 2 — the reference app `apps/qcv`, where `min-ui` is born

Canon: `docs/rust-wgpu-port.md` §21.4 (the slice), rows 127–130 (ruled 2026-11-06). One spike-grade step at a time, in
the §21.4 order, each with its own evidence note in this folder (question · pass bar with numbers · time box · verdict).
The spikes' proven pieces become real crates only as a step needs them, REWRITTEN as crates (row 112: never renamed); a
spike is deleted when its crate meets the spike's own numbers — every number, not the convenient ones.

| Step | Note | Crates born / grown | Status |
|---|---|---|---|
| 1 | [step1-open-present.md](step1-open-present.md) — open a file or an image-sequence frame → the fused presenter at the display's cadence | `min-present`, `qcv-player`, `qcv-timeline` (rational time only), `apps/qcv` | **DONE 2026-11-07 — GO** (carried: H.264 first frame, the software upload's staging blit, the Windows display rows) |
| 1a | [step1-qcview-audit.md](step1-qcview-audit.md) — row 131: step 1 held against QCView's player + renderer; the ports to make before step 2 | `qcv-player`, `min-present` | audit written 2026-11-07; ports on a "go" |
| 2 | transport + timeline surface (S2: the scrub decoder, two slots, the cut, shuttle, J/K/L) | `qcv-player` (Transport), `qcv-timeline`, `min-ui` (first nodes) | — |
| 3 | rails / panels / popups / shortcut registry / focus model (§16) | `min-ui` | — |
| 4 | scopes + guides (§19) | `min-present` (the working-buffer tap), `min-ui` overlays | — |
| 5 | drag and drop (§14; S5's native layer items) | `min-ui`, `min-paths` | — |
| 6 | annotations over the frozen sidecar (`qcv-notes` + `qcv-ink`) | `qcv-notes`, `qcv-ink` | — |
| 7 | the Adobe ring (S4) | `qcv-ring` | — |
| — | the colour chain (§18, S1 step 2: OCIO route A in `min-present`'s stage-2 slot; minColor WGSL) lands with scopes (step 4), the first step that needs the working buffer | `min-present` | — |

Rules carried from Phase 1: the window layer is winit 0.31 (row 129; the text layer stays on the vendored 0.30 until
accesskit_winit follows), DX12 on Windows (row 45), the Windows display numbers wait for a real HDR monitor on rhombus
(row 130). Gate G2 (§21.4): the slice plays the user's real QC sessions side by side with Qt QCView for a week.
