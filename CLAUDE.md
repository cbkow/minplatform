# minplatform — working notes for Claude

- This is the THEORETICAL / spike phase of the Rust + wgpu media platform for QCView-Player,
  minNotes, ufb and MinRender. Read `docs/rust-wgpu-port.md` §13 then §21 before doing anything.
- Standing rule: talk before building. No crate, spike or app is started without an explicit "go"
  from the user; every ruling is dated in the Decisions tables of the canon.
- Never modify the sibling Qt repos (`../QCView-Player`, `../minNotes`, `../ufb`, `../MinRender`)
  from here. Measure them, read them, vendor frozen contract sources with a stamp — never edit.
- Spike protocol (§21.3): one question, one pass bar with a number, one time box, one written
  verdict in `docs/spikes/`. No verdict = not done. Spikes die; they are never renamed into crates.
- `docs/spikes/P0-baseline.md` holds the measured Qt numbers every pass bar is compared against.
