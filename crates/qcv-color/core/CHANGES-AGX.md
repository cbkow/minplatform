# Changes from darktable's AgX

GPL-3.0 §5(a) notice. `core/mincolor_agx.h` and `core/mincolor_agx.cpp` port
parts of darktable's AgX module (`src/iop/agx.c`, `src/common/custom_primaries.c`,
GPL-3.0-or-later, © 2025-2026 darktable developers, AgX module by István
Kovács). These are the modifications and additions, made by cbkow on
2026-10-04. None of them is part of, or endorsed by, darktable.

## Ported, rewritten

- The lower guard rail (`_compress_into_gamut`), the log2 encoding, the sigmoid
  (`_scaled_sigmoid`, `_scale`) and the HSV hue restore, rewritten in minColor's
  dialect-neutral style (C++ and Metal from one source).
- The rotated / scaled primaries geometry (`dt_rotate_and_scale_primary`), in
  double precision on the host.

## Changed

- Base gamut fixed to Rec.2020 at D65 (darktable works in its D50 profile
  connection space); the working space's matrices are minColor core's.
- Primaries: the inset rotation and amounts and the outset amounts published
  with Blender's AgX LUTs (Rec.2020, D65), instead of darktable's D50-tuned
  "blender-like" values. Both agree with Blender's views within 1e-3.
- Curve fixed to AgX's form: pivot at grey, gamma 2.4, zero-length linear
  section; darktable's look controls (lift, slope, brightness, saturation) and
  auto-gamma are not ported.
- Output in absolute display light, 1.0 = 100 nits.

## Added (not in darktable)

- HDR: the shoulder power scales with the peak as (peak / 100)^log10(2), and a
  second sigmoid in a wide log2 domain darkens mid grey by peak / 100 while
  holding the top, with hue and saturation half restored; reimplemented from the
  method of Blender's AgX (Eary Chow et al.). At peak 100 it is the SDR AgX.
- A target guard rail into Rec.709 or P3-D65 (the same luminance-preserving
  rail with the target's luminance weights), then a clip to the display range.
- minColor AgX, an After Effects effect (`ae/`): the curated controls (working
  and target gamut, peak, white / black relative exposure, contrast, toe and
  shoulder power; hue restore, HDR purity and outset under Advanced), Metal and
  CPU paths from the one source.
- The darkening step uses the sigmoid unguarded (a negative scale with power 1
  is valid there); the main curve keeps darktable's guard.
