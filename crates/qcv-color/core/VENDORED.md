# Vendored from minColorAE

The minColor engine's maths, copied unmodified from `github.com/cbkow/minColorAE` `core/` at commit **57eeebb**
(2026-10-25, 0.1.5), GPL-3.0-only, © 2026 cbkow. The rendering is derived from OpenDRT v1.1.0 by Jed Smith
(GPL-3.0), modified in minColorAE — see `CHANGES-FROM-OPENDRT.md`; the AgX port's provenance is in
`CHANGES-AGX.md`. Not affiliated with or endorsed by OpenDRT, darktable or Blender.

Keep in sync by re-copying, never by editing here. Against QCView's copy (e824933, 0.1.3) the kernel, params,
knee and AgX files are byte-identical; only the Grade (`opendrt_grade.h`, the Grade defaults in
`opendrt_presets.cpp`) moved, and neither QCView nor this crate runs the Grade.

How it runs here (canon row 204): the GPU kernel is the core through minColorAE's own GLSL shim
(`opendrt_shim_glsl.h`), compiled at build time by naga's GLSL front end into WGSL — one shader for every wgpu
backend; the host derive (`drt_resolve`, `drt_agx_derive`, the preset tables) is the C++ core through
`opendrt_shim_cpp.h`, compiled by `build.rs` and called through `src/ffi.cpp`. Not vendored: the CUDA / MSL /
HLSL shims (unused), the presets JSON.
