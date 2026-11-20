# Vendored from minColorAE

The minColor engine's maths, copied unmodified from `github.com/cbkow/minColorAE` `core/` at commit **ae9f922**
(2026-11-19, 0.1.7, branch `camera-logs-0.1.7`), GPL-3.0-only, © 2026 cbkow. The rendering is derived from OpenDRT
v1.1.0 by Jed Smith (GPL-3.0), modified in minColorAE — see `CHANGES-FROM-OPENDRT.md`; the AgX port's provenance is in
`CHANGES-AGX.md`. Not affiliated with or endorsed by OpenDRT, darktable or Blender.

Keep in sync by re-copying, never by editing here. Before this copy the crate held 57eeebb (0.1.5). What 0.1.7 brings
that runs here: five camera transfers (Apple Log, Canon Log 2, Canon Log 3, Blackmagic Film Gen 5, DJI D-Log, ids
22-26) and four camera gamuts (Apple Wide Gamut, Canon Cinema Gamut, Blackmagic Wide Gamut Gen 5, DJI D-Gamut, ids
15-18), appended after everything else so no stored id moves; "an inverse entry" is now the range
`DRT_OETF_IS_INVERSE`. What it brings that does not run here: 0.1.6's new Grade (`mincolor_grade2.h`, included by
`opendrt.h`, so it is copied too) - neither QCView nor this crate runs a Grade.

How it runs here (canon row 204): the GPU kernel is the core through minColorAE's own GLSL shim
(`opendrt_shim_glsl.h`), compiled at build time by naga's GLSL front end into WGSL — one shader for every wgpu
backend; the host derive (`drt_resolve`, `drt_agx_derive`, the preset tables) is the C++ core through
`opendrt_shim_cpp.h`, compiled by `build.rs` and called through `src/ffi.cpp`. The order the gamut and transfer lists are
SHOWN in is `src/ffi.cpp`'s (a project stores the id): the 0.1.7 cameras sit with the other cameras. Not vendored: the CUDA / MSL /
HLSL shims (unused), the presets JSON.
