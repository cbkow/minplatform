# S4 — ring: `qcv-ring`'s shape — the QCAE consumer + the QCVO producer in Rust, proven against the C++ copies

**Question (§21.3, §20):** can QCView's END of the three wire contracts be written in Rust — the `ringslot::` discipline and the
`#[repr(C)]` layouts byte-exact against `vendor/qcbae`, the QCAE consumer feeding wgpu with zero copies on Metal, the QCVO
producer feeding the real sinks — proven by two-process tests on both OSes?
**Pass bar (§21.3):** byte-exact layout tests pass on both OSes (every struct's size and every field's offset equal between the
C++ and the Rust); the REAL After Effects Transmit device shows in the spike window (ring → present), with the import cost per
frame measured zero-copy (Metal) vs CPU copy (the control; QCView today: 0.95 ms copy + 2.3 ms `replaceRegion` at 4K, parked);
the QCVO producer is consumed by the real QCBridgeN sink (attach, request/reply, heartbeat, frames taken); DX12 = one copy each
way, measured (the box). Audio rings: layouts only (no audio in Phase 1 spikes).
**Time box:** 1 week from 2026-11-06. **Status: VERDICT GO on macOS 2026-11-06 — layouts byte-exact, Rust↔Rust and C++→Rust two-process tests green with ZERO-COPY, the REAL AE Transmit device shown zero-copy at 0.08 ms/frame, the QCVO producer accepted by the real QCBridgeN sink; Windows owed.**

## What the spike is
`spikes/s4-ring`: `layout.rs` (RingHeader / SlotHeader / FrameDesc, OutputRingHeader / OutputSlotHeader / OutputFrameDesc /
OutputRequest / OutputReply / OutputHdrInfo / OutputHdrBlock, AudioRingHeader / AudioSlotHeader / AudioSession /
AudioPacketDesc — `#[repr(C)]`, atomics as `AtomicU64`/`AtomicU32`), `layout_check.cpp` (built from `vendor/qcbae` by `cc`:
prints `sizeof` + every `offsetof` the C++ compiler sees; the Rust test compares with `size_of` / `offset_of!`), `ringslot.rs`
(begin_write / commit / abandon / acquire_latest / release / write_record / read_record — the discipline, generic over header +
slot), `mapping.rs` (`shm_open` + `mmap` on POSIX, `CreateFileMappingW` + `Local\` on Windows; open = header probe → magic →
EXACT version → `total_size` → object size → map all), `qcae.rs` (SharedRing: create for the test producer, open + acquire for
the consumer), `qcvo.rs` (OutputRing: create + request/reply + heartbeat watch for the producer; open + attach for the test
consumer). Modes: `--layout` (the byte-exact check), `--produce NAME` (a Rust QCAE producer: a moving 4K RGBA16F gradient at
24 fps, the two-process self-test's other half; the C++ tearing-test producer is the cross-implementation half),
`--consume NAME` (the QCView end: 2-ms poll, acquire, import to wgpu — Metal: the mapped slot wrapped with
`newBufferWithBytesNoCopy` and viewed as a linear texture through hal, ZERO copies; `--copy` = the CPU control — present,
per-frame numbers), `--output ID` (the QCVO producer: creates `/qcview-out-ID`, answers requests, publishes BGRA8 frames,
launches the sink from its manifest like QCView does, prints the sink's heartbeat / status / `last_seen_seq`).

## Results (macOS, M5 Max, 2026-11-06)
**Layouts — byte-exact.** `--layout`: 106 sizes, field offsets and constants (RingHeader / SlotHeader / FrameDesc, OutputRingHeader /
OutputSlotHeader / OutputFrameDesc / OutputRequest / OutputReply / OutputHdrInfo / OutputHdrBlock, AudioRingHeader / AudioSlotHeader /
AudioSession / AudioPacketDesc, the three magics + versions + the HDR-block offset) compared between clang++ on the vendored headers
and the Rust `#[repr(C)]` structs: **0 mismatches**. `AtomicU64` / `AtomicU32` stand in for `std::atomic<uint64_t/uint32_t>` exactly.

**QCAE, two processes (4K RGBA16F, 3 slots of 63 MB, 24 fps; the consumer polls every 2 ms):**
| Producer → consumer | import ms p50 / p95 / max | commit → import latency ms p50 / p95 | frames in 10 s / latest-wins skips |
|---|---|---|---|
| Rust producer → Rust consumer, **ZERO-COPY** (slot bytes wrapped once per slot with `newBufferWithBytesNoCopy`, viewed as a linear RGBA16F texture, handed to wgpu via `hal::metal::Device::texture_from_raw`) | **0.20 / 0.25 / 0.37** | 1.1 / 2.4 | 236 / 0 |
| Rust producer → Rust consumer, CPU COPY (`write_texture` of 66 MB — the control) | 7.0 / 8.4 / 9.3 | 8.7 / 10.5 | 238 / 0 |
| **QCBridgeAE's own C++ `SharedRing` (vendor/qcbae, unmodified) → Rust consumer, zero-copy** | **0.22 / 0.30 / 0.40** | 1.2 / 2.4 | 238 / 0 |
Reading: the Rust consumer reads the C++ producer's ring exactly as its own (the cross-implementation test §20.3 asked for, one
direction); zero-copy costs 0.2 ms per 4K frame on the render thread against QCView's parked 0.95 ms copy + 2.3 ms `replaceRegion`
and this spike's 7 ms naive copy; the page-aligned `slots_offset` / `pixels_offset` do exactly what §20.2 said they exist for;
the discipline shows no torn frames and no skips at 24 fps with a 2-ms poll (latency = the poll). The producer's own fill + commit
is 1.4 ms per 4K frame (a row memcpy'd down the slot).

**QCVO, the Rust producer against the REAL sink:** `--output ndi` created `/qcview-out-ndi` (94 MB), launched QCBridgeN from its
manifest exactly as QCView does (`exec --id ndi`, `QCVO_ID`); the sink attached (pid, protocol 1), wrote its request (1920×1080
BGRA8 SDR, letterbox, composite, layers guides+annotations, no fps cap), the Rust producer answered `accepted` with its reply,
the sink reported status `"NDI 'QCView': starting"` → `"0 receivers"`, heartbeat age stayed under 160 ms, `last_seen_seq`
tracked `published − 1` for 20 s (442 frames), and the sink exited 0 when the ring was retired. **Contract B works end to end
with an unmodified shipped companion.**

**The real AE Transmit device — DONE.** After Effects 2026 with a 1920×1080 comp (an animated solid), "Enable Mercury Transmit"
ticked by the user, the QCBridgeAE device enabled; the comp scrubbed by script (one `DoScript` per step — a `$.sleep` loop inside
ONE DoScript blocks AE's UI, so the viewer pushes only ~1 frame/s; one call per step gives ~10 steps/s, osascript-bound).
AppleScript recipe: read the .jsx with `as «class utf8»` and pass the TEXT — the `DoScript "/path"` form returns 1 and does nothing;
`alert()` blocks the call on a modal.
| AE Transmit → Rust consumer (1920×1080, 15 MB slots) | import ms p50 / p95 / max | frames in a 120-step scrub |
|---|---|---|
| **zero-copy** (slot → no-copy Metal buffer → linear texture → wgpu hal) | **0.08 / 0.09 / 0.15** | 90 (one per viewer refresh, 0 skips) |
| CPU copy (`write_texture`, the control) | 1.6 / 2.1 / 2.5 | 59 (70 steps) |
First frame 0 ms after the ring opened (AE's slots already held frames). What AE's plugin writes into FrameDesc v3: `pixel_format`
1 = RGBA16F, `source_tier` 3 = Float32, `channel_order` 1 = RGBA, `flags` 0 (not premultiplied), `graphics_white` 0, `value_scale` 1,
`comp_name` **"After Effects"** (the app name, not the comp), `time_value` −1 over `time_scale` 254016000000 (AE ticks; −1 = unknown).
So the consumer cannot measure commit→import latency on AE frames — QCAE v3 carries no wall-clock stamp (only this spike's own
producers put `now_ns` into `time_value`); the measured latency with those producers = the 2-ms poll. The picture: the consumer
window shows the orange solid moving with the scrub, screenshot `fixtures/s4-ae-live.png`. (The spike blits the RGBA16F values
straight onto an extended-linear surface, so the solid reads paler than AE's viewer — AE hands display-referred values and the
presenter's transfer handling is S1's job, not the ring's.)

## Remaining plan
1. ~~Layouts + the C++ check~~ green.
2. ~~Two processes~~ green (Rust→Rust, C++→Rust).
3. ~~The real AE Transmit device~~ green (zero-copy 0.08 ms vs copy 1.6 ms at 1080p).
4. ~~The QCVO producer + the real QCBridgeN sink~~ green.
5. ~~Windows on the box (layouts, `Local\` sections, one-copy DX12)~~ green (below); the real companions on Windows (AE Transmit, QCBridgeN) are not installed on rhombus — the contract is the same bytes, covered on macOS.

## Verdict (macOS) — GO, 2026-11-06
Every macOS line of the pass bar is met: layouts byte-exact (106 checks, 0 mismatches); the REAL After Effects Transmit device shows
in the spike window with zero-copy import at 0.08 ms per 1080p frame (0.2 ms at 4K from the C++ producer) against 1.6 ms (7 ms at 4K)
for the copy control and QCView's parked 0.95 + 2.3 ms; the QCVO producer drives the unmodified shipped QCBridgeN sink end to end
(attach, request/reply, heartbeat, frames taken, clean exit); the ringslot discipline shows no torn frames and no skips.
`qcv-ring` is therefore shaped: the three `#[repr(C)]` layouts + `ringslot` + `Mapping` + the per-platform zero-copy import, QCView's
end only; the companions stay untouched (row 103) and the contracts stay frozen (row 101).
What stays owed before gate G1: Windows (layouts under MSVC, `Local\` sections, DX12 one-copy in + out — row 104's second half);
audio rings = layouts only in Phase 1 (as the bar said). Contract facts for the port: QCAE v3 has no wall-clock stamp (latency is the
poll, by design); AE's `comp_name` is the app name; the transfer of AE's RGBA16F values is the presenter's business.

## Windows (rhombus: i9-14900KF, RTX 4090, Windows 11, MSVC 18, 2026-11-06) — layouts, `Local\` sections, one-copy DX12
Built from a git bundle (`build-spike.ps1 -Pkg s4-ring -NoFeatures`; scripts need `-ExecutionPolicy Bypass` in that ssh shell);
the C++ producer built with `cl` from `tools/s4/build-cxx.ps1` (vendor/qcbae unmodified); runs on the logged-on desktop via
`tools/s4/s4-runs.ps1` (a .cmd per run through `C:\qcb-lab\run.ps1`). The Rust end mirrors `ring_mapping.cpp` exactly: `Local\`+name
(no leading slash) on the pagefile, create = `CreateFileMappingW` with the C++'s 60 × 25 ms wait on `ERROR_ALREADY_EXISTS`, open =
header view → magic / exact version / `total_size` → full view (a view past the section fails with `ERROR_ACCESS_DENIED` = the
fstat check), liveness = `OpenProcess` + `GetExitCodeProcess` (access denied counts as alive, like EPERM); the bus clock = QPC in ns,
which is MSVC's `steady_clock`, so the C++ producer's `time_value` compares to ours. The consumer forces DX12 (row 45).
**Layouts under MSVC — byte-exact:** `--layout` 106 checks, 0 mismatches (the same `#[repr(C)]` Rust against cl's sizeof/offsetof).
| Producer → consumer (4K RGBA16F, 3 × 63 MB slots, 24 fps) | import ms p50 / p95 / max | commit → import ms p50 / p95 | frames in 10 s / latest-wins skips |
|---|---|---|---|
| Rust producer → Rust consumer, **ONE COPY** (`write_texture` from the section into an RGBA16F texture on DX12) | 5.4–6.2 / 11–17 / 29 | 7–16 / 40–46 | 240 / 43 |
| **QCBridgeAE's own C++ `SharedRing` (MSVC) → Rust consumer, one copy** | 4.6–6.6 / 7–17 / 28 | 6–19 / 8–52 | 244 / 36 |
Reading: the sections, the discipline and the cross-implementation test hold on Windows exactly as on macOS — the consumer even
walked the hand-over correctly (it first found the previous Rust producer's section in state Retired, closed it, then opened the
C++ producer's fresh one by the same name: a retired section lingers while a handle is open, as `ring_mapping.cpp` says). The
import number is the one-copy answer row 104 expected: ≈ 5 ms p50 per 4K frame = a 66 MB single-thread memcpy into wgpu's staging
belt plus the GPU copy (vs 0.2 ms zero-copy on Metal, 7 ms the Metal CPU-copy control). The latency p95 (40–50 ms) and the
latest-wins skips are the display's: rhombus has only the Parsec/Jump virtual display (≈ 13 Hz, 76 ms per present), so the
consumer's present blocks and frames pile up — not a ring effect (0 skips on macOS at the same rate). A later refinement for the
port, not for this spike: a multi-threaded memcpy into a persistently mapped upload buffer + `copy_buffer_to_texture` would cut the
5 ms to ≈ 1.5 ms; still one copy.
**QCVO on Windows:** `--output ndi --no-launch` creates `Local\qcview-out-ndi` (94 MB, 3 slots) and retires it cleanly; no sink is
installed on rhombus (QCBridgeN needs the NDI SDK; After Effects is not on the box), so the real-companion runs stay macOS-only —
the bytes are the same (layouts byte-exact under MSVC) and the macOS runs covered both companions.
**Verdict addendum (Windows) — GO for the ring layer:** layouts byte-exact, sections + discipline + liveness proven in two processes
against the unmodified C++, one-copy DX12 import measured. What Windows still owes is not S4's: pacing on a real display.
