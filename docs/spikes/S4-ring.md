# S4 — ring: `qcv-ring`'s shape — the QCAE consumer + the QCVO producer in Rust, proven against the C++ copies

**Question (§21.3, §20):** can QCView's END of the three wire contracts be written in Rust — the `ringslot::` discipline and the
`#[repr(C)]` layouts byte-exact against `vendor/qcbae`, the QCAE consumer feeding wgpu with zero copies on Metal, the QCVO
producer feeding the real sinks — proven by two-process tests on both OSes?
**Pass bar (§21.3):** byte-exact layout tests pass on both OSes (every struct's size and every field's offset equal between the
C++ and the Rust); the REAL After Effects Transmit device shows in the spike window (ring → present), with the import cost per
frame measured zero-copy (Metal) vs CPU copy (the control; QCView today: 0.95 ms copy + 2.3 ms `replaceRegion` at 4K, parked);
the QCVO producer is consumed by the real QCBridgeN sink (attach, request/reply, heartbeat, frames taken); DX12 = one copy each
way, measured (the box). Audio rings: layouts only (no audio in Phase 1 spikes).
**Time box:** 1 week from 2026-11-06. **Status: IN PROGRESS 2026-11-06 — layouts byte-exact, Rust↔Rust and C++→Rust two-process tests green with ZERO-COPY, the QCVO producer accepted by the real QCBridgeN sink; the AE Transmit device run next; Windows owed.**

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

**The real AE Transmit device (in progress):** After Effects 2026 launched by script; a comp with an animated solid created and
scrubbed through `DoScript` (AppleScript: read the .jsx with `as «class utf8»` and pass the TEXT — the `DoScript "/path"` form
returns 1 and does nothing; `alert()` blocks the call on a modal). The QCBridgeAE device is enabled in AE's MC prefs
(`…QCBridgeAE-Transmit.bundle0EnabledVideo true`) **but Mercury Transmit itself is OFF (`TransmitEnabledKey false`)** — AE ›
Settings › Video Preview › "Enable Mercury Transmit" is a UI preference; once on, scrubbing / preview pushes frames into
`/qcbae-ae` and `--consume /qcbae-ae` shows them. Owed until that box is ticked.

## Remaining plan
1. ~~Layouts + the C++ check~~ green.
2. ~~Two processes~~ green (Rust→Rust, C++→Rust).
3. The real AE Transmit device — blocked on AE's "Enable Mercury Transmit" preference (see above); then zero-copy vs copy on AE's frames.
4. ~~The QCVO producer + the real QCBridgeN sink~~ green.
5. Windows on the box (layouts, `Local\` sections, one-copy DX12). Verdict.
