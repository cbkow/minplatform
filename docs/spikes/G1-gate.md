# Gate G1 — end of Phase 1: go / go-with-LUT / no-go (evidence assembled 2026-11-06; the ruling is the user's)

**The gate (§21.3, row 113):** Go = passthrough works and the numbers hold → Phase 2 (the reference app `apps/qcv`, where `min-ui`
is born). Go-with-LUT = passthrough fails but the baked LUT path matches → Phase 2 with route B as the display path. No-go = the
HDR/present story does not hold on wgpu → §10's FFI media platform without a UI toolkit.

## The five spikes against their bars
| Spike | Bar (§21.3) | macOS (M5 Max) | Windows (rhombus, RTX 4090) | Verdict |
|---|---|---|---|---|
| **S1 present** | pixel-diff vs Qt QCView within tolerance; EDR + PQ correct; OCIO passthrough compiles unmodified; latency ≤ P0 | 55.9 dB / max 2 per 255 vs QCView (identity); EDR engages (headroom 8.5–8.8); PQ exact to the file; OCIO MSL passthrough EXACT (≤ 0.13/255; ACES / AgX / Filmic; baked LUT 8-bit-exact only); first frame 123–195 ms vs QCView 181, present p95 17 ms, VT zero-copy 0.1 ms/frame; footprint 0.4–0.66 GB vs 1.36 | DX12 forced; D3D12VA zero-copy 0.27 ms/frame; OCIO HLSL passthrough exact (FXC); PQ/EDR/pacing NOT measurable (virtual 13 Hz display) | **GO** (macOS); Windows display runs owed |
| **S2 scrub** | seek-to-display ≤ P0; gapless cut; shuttle ±32× | seeks 16 / 7 ms vs QCView 60–130; scrubs 5.6 / 3.5 vs 30–40; cut GAPLESS; shuttle to 32× one frame per refresh; 0 drops | hw scrub decoder (D3D12VA tolerates flushes) 7.5 ms; two hw contexts cut gaplessly; ProRes software on NVIDIA | **GO** (both, pacing on Windows owed) |
| **S3 text** | composition commits (dead keys, press-and-hold, dictation; TSF on Windows); VoiceOver + Narrator read it; 60 fps at 10k lines | one editor per block: 24 µs/key at 10k lines, key→present 2.3 ms with AX live; AccessKit tree verified via the AX C API; dead keys + press-and-hold confirmed by the user on a real keyboard (winit 0.30 patched, vendored) | TSF + Narrator owed (needs the user at the box) | **GO** (macOS); Windows + dictation = the user's walk |
| **S4 ring** | layouts byte-exact on both OSes; the real AE Transmit device in the spike window; DX12 one copy measured | 106 checks exact; AE Transmit zero-copy 0.08 ms/1080p (copy 1.6); C++ producer → Rust 0.2 ms/4K (copy 7); Rust QCVO producer drives the shipped QCBridgeN sink end to end | 106 checks exact under MSVC; `Local\` sections + MSVC-built QCBridgeAE producer → Rust green; one-copy DX12 ≈ 5 ms/4K | **GO** (both) |
| **S5 window** | §14's edge list reproduces or is ruled out per item | winit 0.31 beta: typed receive (Finder multi-file), clipboard, single-item drag-out lands a real file, self-drop, actions, two windows, fullscreen, HiDPI, objc2 menu bar; native-layer items: multi-item drag-out (beta hang), the text patch carried to 0.31 | owed (needs the user at the box: DnD is interaction) | **GO** (macOS); Windows = the user's walk |

**P0 baseline:** QCView's own numbers measured first on both OSes (`P0-baseline.md`), every spike compared against them.

## What the evidence says
- **Passthrough works** on both backends (MSL on Metal, HLSL→FXC on DX12) with OCIO 2.5's generated shaders through QCView's own
  kernel wrapper; the baked-LUT route is the measured control, not the fallback we need.
- **The HDR/present story holds** on macOS: EDR extended-linear and PQ surfaces (the latter through the wgpu-hal workaround, bug not
  reported by the user's choice), zero-copy VideoToolbox, pacing at the refresh. On Windows the pieces (DX12, D3D12VA zero-copy,
  DXIL/FXC) all work; what is unmeasured is the display itself — pacing, EDR, PQ on a real HDR monitor.
- **The numbers hold or beat P0** everywhere they could be measured: first frame, present, seeks, scrubs, text latency, ring import.
- **The text long pole** (S3) resolved into a rule with teeth: one editor per block; `min-ui`'s text node is a block editor.

## What is owed, and whether it blocks the gate
| Item | Blocks G1? | Why |
|---|---|---|
| Windows pacing / EDR / PQ on a real HDR monitor (S1, S2) | **the one real open** — go-with-a-condition | every component works; only the display-facing number is missing; needs a monitor on rhombus or the spikes run on another Windows box with one |
| S3 Windows (TSF, Narrator), S5 Windows (DnD, drag-out) | no | interaction walks for the user; the libraries are the same crates |
| Multi-item drag-out (winit 0.31 beta hang) | no | native-layer item in Phase 2 (objc2 `NSDraggingSession` directly) |
| Dictation on macOS | no | the user's own check, same text-input client path as press-and-hold |
| QCVO zero-copy out, DX12 one-copy out | no | belongs to the output pass (row 105) in Phase 2 |
| The wgpu-hal PQ-surface bug upstream | no | worked around; the user chose not to post |

## Proposed ruling [proposed]
**GO**, with one condition carried into Phase 2: the Windows display numbers (pacing / EDR / PQ) are measured on a real HDR
monitor before the presenter in `apps/qcv` is called done on Windows. Route A (native passthrough) is the display path; route B stays
as a test control. The crates to shape from the spikes' findings (the spikes themselves die, row 112): `qcv-ring` (S4: layouts,
ringslot, Mapping, per-platform import), the presenter core (S1: decode → zero-copy import → OCIO passthrough → HDR surface),
`qcv-timeline` + the scrub decoder policy (S2), the block text node + AccessKit tree (S3), the window layer on winit 0.31 (S5).

## Rows this ruling touches (all open, the user's)
109 (QCView-shaped first), 111 (no `min-ui` repo), 112 (spike protocol — followed; spikes die), 113 (this gate), 114 (reference-app
scope + G2), 115 (second consumer / first to ship at G2), 116 (licences), 117 (pins: wgpu 30.0.1, winit 0.31 for the window
layer once the text patch is carried, accesskit 0.25, parley 0.12, vello 0.11), 118 (`min-paths` now, optional). Row 110 is ruled;
"public after G1" (§21.2) waits on the fixture/privacy sweep: nothing tracked names a client, and `fixtures/` stays ignored.
