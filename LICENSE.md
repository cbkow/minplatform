# Licences

minplatform is **GPL-3.0-or-later** (`LICENSE`): every app under `apps/` and every crate under `crates/`, with one
exception — `crates/min-paths` and `crates/min-paths-ffi` are **MIT OR Apache-2.0**, because they shipped that way and a
permissive consumer exists (see `crates/min-paths/README.md`). The ring wire contract vendored under `vendor/qcbae` stays
**MIT OR GPL-3.0** so that QCBridgeN (MIT, because of the NDI SDK) can carry the byte layouts without linking anything here.
Rationale and the ruling: `docs/rust-wgpu-port.md` row 116 (revised 2026-11-07).
