# S4 — ring: `qcv-ring`'s shape — the QCAE consumer + the QCVO producer in Rust, proven against the C++ copies

**Question (§21.3, §20):** can QCView's END of the three wire contracts be written in Rust — the `ringslot::` discipline and the
`#[repr(C)]` layouts byte-exact against `vendor/qcbae`, the QCAE consumer feeding wgpu with zero copies on Metal, the QCVO
producer feeding the real sinks — proven by two-process tests on both OSes?
**Pass bar (§21.3):** byte-exact layout tests pass on both OSes (every struct's size and every field's offset equal between the
C++ and the Rust); the REAL After Effects Transmit device shows in the spike window (ring → present), with the import cost per
frame measured zero-copy (Metal) vs CPU copy (the control; QCView today: 0.95 ms copy + 2.3 ms `replaceRegion` at 4K, parked);
the QCVO producer is consumed by the real QCBridgeN sink (attach, request/reply, heartbeat, frames taken); DX12 = one copy each
way, measured (the box). Audio rings: layouts only (no audio in Phase 1 spikes).
**Time box:** 1 week from 2026-11-06. **Status: SCAFFOLD 2026-11-06.**

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

## Remaining plan
1. Layouts + the C++ check; the discipline; the mapping. `--layout` green on macOS.
2. Two processes: Rust producer → Rust consumer; C++ producer (the vendored tearing-test shape) → Rust consumer; the numbers.
3. The real AE Transmit device (After Effects 2026 + the installed QCBridgeAE device) in the window; zero-copy vs copy.
4. The QCVO producer + the real QCBridgeN sink.
5. Windows on the box (layouts, `Local\` sections, one-copy DX12). Verdict.
