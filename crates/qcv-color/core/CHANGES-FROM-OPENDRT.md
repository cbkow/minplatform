# Changes from OpenDRT

GPL-3.0 §5(a) notice. The rendering in minColorAE is derived from OpenDRT v1.1.0
by Jed Smith (`upstream/OpenDRT_v1.1.0.dctl`, unmodified). These are the
modifications and additions, made by cbkow between 2026-09-30 and 2026-10-25.
None of them is part of, or endorsed by, OpenDRT.

## Port (2026-09-30)

- The DCTL body transcribed to one dialect-neutral source (`core/opendrt_kernel.h`)
  that compiles as C++ and Metal (HLSL and GLSL shims exist, untested). Functions
  and constants carry a `drt_` prefix; parameters come from a flat struct
  (`core/opendrt_params.h`) instead of DCTL globals; per-render constants are
  computed once (`drt_derive`). The preset tables are transcribed into
  `core/opendrt_presets.cpp`. The test `tools/probe_core` compares the port with
  the unmodified DCTL compiled as C++ and expects zero difference on the
  upstream paths.
- Dropped: the tonescale overlay (`crv_enable`).

## Additions to the rendering path

- Display-referred input decodes (Rec.1886, sRGB, 2.2 power, BT.709 camera, PQ,
  HLG) alongside upstream's camera log curves; a working-gamut stage for the
  Input effect; limited (video) range expansion.
- Sub-black codes decode to black for every EOTF (upstream only encodes).
- An iterative inverse of the rendering (`drt_inverse_transform`), used by the
  Input effect's "OpenDRT inverse" transfers and the Grade's eyedroppers.
- An "Un-tone-mapped" view: the input conversion reversed, with no rendering.
- An output Mode: View | Render, with a separate render encoding block (removed
  again in minColorAE 0.1.1: the Output has one encoding).
- Display presets beyond upstream's: linear hand-offs to the working gamut,
  ACES 2065-1, ACEScg, Rec.2020 and Rec.709 (with matching display gamuts).
- Peak Luminance range raised to 10000 nits (upstream's slider stops at 1000).
- "Use Look" on the tonescale preset re-applies the look's tonescale.
- Defaults (2026-10-04): Display Encoding sRGB Display - 2.2 Power / Rec.709 and
  Surround Dark (upstream: Rec.1886, Dim). The display presets, the render presets
  and the Input's inverse transfers no longer write Surround; it is the viewing
  room, set by hand, on the Output only; the Input's inverse always assumes
  Dark and has no Surround row. `drt_apply_display` and the preset-mode path the probe
  checks keep upstream's behaviour. The Output's Rendering defaults to
  Un-tone-mapped (a plain gamut and display-curve conversion); the OpenDRT
  rendering is selected on the same popup, unchanged.

## New, not derived from OpenDRT

- The minColor Grade effect (`core/opendrt_grade.h`, `ae/common/drt_ae_wheels.h`):
  luminance zones defined through the tonescale, colour wheels, HSL secondary.
- The minColor macOS Fix effect, the macOS display of `ocio/mincolor.ocio` and
  `ocio/mincolor-unmanaged.ocio` (Set OCIO's Unmanaged): a
  workaround for how After Effects shows colour on macOS.
- `ocio/mincolor.ocio` and `tools/make_ocio.cpp`: an OCIO config for linear
  conversions, generated from the core's gamut matrices (taken from upstream).
- The minColor Knee effect (`core/mincolor_knee.h`): a highlight knee ported from
  the author's QCView, not from OpenDRT.
- The After Effects hosting (`ae/`): upstream ships no After Effects version.

## Controls hidden (minColorAE 0.1.4, 2026-10-25)

- The effects show only the controls that describe the media: gamuts, transfer
  functions, display encodings and peak luminance. The OpenDRT look and
  tonescale presets and parameters, the Output's Rendering choice, Surround and
  Clamp are no longer shown. The rendering is unchanged: hidden controls keep
  their values and are still read, so the Input's inverse transfers, the
  Grade's zones and saved projects work as before.
- The Grade's eyedroppers no longer use the inverse rendering: a pick is
  decoded by the Grade's Display Encoding (the Un-tone-mapped Output in
  reverse), which now defaults to None - Linear / Working Gamut.
