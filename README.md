# minplatform

The shared Rust + wgpu media platform for the family — QCView-Player, minNotes, ufb, MinRender —
in its **theoretical / spike phase**. Nothing here ships. The Qt apps keep shipping untouched; this
repo never modifies them. Interop with them is only through the frozen contracts (the `.qcview`
sidecar, `.mnd`, the three QCBridge rings) and, later, a C ABI.

**Start at [docs/rust-wgpu-port.md](docs/rust-wgpu-port.md) §13 (where we are) and §21 (the path
forward).** The Decisions tables in that document are the agenda; every ruling is dated there.

```
docs/           the canon (rust-wgpu-port.md) + docs/spikes/<id>.md: one verdict per spike
crates/         only crates with a consumer or a frozen contract — never speculative
spikes/         bin crates, numbered; question / pass bar with a number / time box / verdict
apps/           vertical slices (apps/qcv = the QCView-shaped reference app, first)
vendor/         byte-exact C++ reference copies for cross-implementation tests (stamped)
tools/          CI checks: dependency direction, vendor stamps, per-crate licences
```

Rules (§21.2): spikes depend on crates, never the reverse; a spike that passes is never renamed
into `crates/` — the crate is written fresh with the spike as its first integration test.

Licences are **per crate** (row 116, proposed): shared `min-*` / `qcv-*` crates MIT OR Apache-2.0
(`qcv-ring` MIT — the companions vendor the ring sources), `apps/*` GPL-3.0-or-later like their Qt
ancestors. Until the first crate lands there is nothing here to license.
