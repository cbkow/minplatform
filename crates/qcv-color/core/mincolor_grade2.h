/* SPDX-License-Identifier: GPL-3.0-only
 * minColorAE. Copyright (C) 2026 cbkow.
 */
/* minColor Grade 2 — a colour corrector with the Primaries and HDR-zone controls a
 * colourist already knows from a dedicated grading application, behaving the way they do
 * there. NOT DERIVED FROM OPENDRT, and no one else's code: every formula and constant here
 * was fitted to renders of a grey ramp and a 16-patch chart through a reference grading
 * application (a colour-managed project, linear in and out, a wide-gamut log timeline;
 * 2026-11-19). The first Grade (opendrt_grade.h) stays as it is.
 *
 * Linear working gamut in and out. Inside, everything runs in a linear wide-gamut
 * grading space (WG), whatever the working gamut, so a control means the same in any comp.
 * Order, as measured in the reference (HDR palette before Primaries inside a node):
 *
 *   HDR  1. zone weights, read from the INCOMING pixel's luminance in stops over
 *           0.18 grey (so no control moves another control's target)
 *        2. exposure: Global + sum(weight x zone Exp), true stops, one gain (hue kept)
 *        3. Temp / Tint (one matrix)
 *        4. colour balance: Global X/Y + sum(weight x zone X/Y), a matrix exponential
 *        5. saturation: Global x product(1 + weight x (zone Sat - 1)), about luminance
 *        6. Hue (matrix)
 *        7. Contrast about a pivot, on luminance (hue kept)
 *        8. Black Offset
 *   SECONDARY (after the Primaries, see 10 below)
 *   PRIMARIES, per channel on the the log grading curve code value:
 *        9. Lift and Gain (one line), Offset, Gamma
 *
 * What matches the reference and how well (worst case over the test images):
 *   - Primaries: exact (1e-5 of code value), alone and combined.
 *   - Exposure, zone weights, their sum, weights-from-input, exposure -> contrast ->
 *     black offset: within 0.01 stop / 0.007 of weight.
 *   - Contrast: 0.5 % either side of 1, on greys and colours.
 *   - Balance: a 3x3 fit; 1.5-5 % on saturated chart patches up to X 0.1, more beyond
 *     (the reference's is mildly non-linear in colour). Temp 0.3-2.7 %, Tint 0.8-8.6 %,
 *     Hue 3.5 % at 10.
 *   - Saturation: the reference holds this luminance exactly and scales chroma about it; a
 *     plain lerp is within 10 % at 0.5 and 1.5 (the reference compresses saturated colours
 *     above 1, this does not).
 *   Near-neutral colours are far closer than those worst cases. Not modelled: the reference fades the
 *   balance out below about -6 stops, and stops Contrast at a ceiling near 16 stops over grey.
 *
 * Zone weight (dark side; the light side is its mirror). With range r and falloff f,
 * in stops over grey: len = max(f (r + 8), 1.37); a linear ramp, 0 at r to 1 at
 * r - len, smoothed by a 1.62-stop box. So the falloff is the FRACTION of the way
 * from the range to the end of the +-8 stop scale, with a floor, and the weight is
 * about 0.15 at the range value itself. The reference's defaults put Shadow and Light
 * crossing at mid grey and summing to one.
 *
 * Same dialect rules as opendrt_kernel.h, which must be included first. The DrtParams
 * `p` given to drt_grade2_derive() carries the working gamut: the host sets p.in_gamut
 * AND p.working_gamut to it (in_m** = working -> XYZ), as for the first Grade.
 * DrtGrade2Params is a flat 4-byte block (uploaded as a uniform buffer).
 */
#ifndef MINCOLOR_GRADE2_H
#define MINCOLOR_GRADE2_H

#define DRT_G2_ZONES 6   /* Black, Dark, Shadow | Light, Highlight, Specular */

struct DrtGrade2Params {
  /* ---- Primaries, the reference's units : 12 ---- */
  float lift_r;   float lift_g;   float lift_b;     /* 0 = none */
  float gamma_r;  float gamma_g;  float gamma_b;    /* 0 = none */
  float gain_r;   float gain_g;   float gain_b;     /* 1 = none */
  float offset_r; float offset_g; float offset_b;   /* 25 = none */

  /* ---- HDR zones, 6 per zone, Black first : 36 ---- */
  float z0_exp; float z0_sat; float z0_x; float z0_y; float z0_range; float z0_falloff;   /* Black     */
  float z1_exp; float z1_sat; float z1_x; float z1_y; float z1_range; float z1_falloff;   /* Dark      */
  float z2_exp; float z2_sat; float z2_x; float z2_y; float z2_range; float z2_falloff;   /* Shadow    */
  float z3_exp; float z3_sat; float z3_x; float z3_y; float z3_range; float z3_falloff;   /* Light     */
  float z4_exp; float z4_sat; float z4_x; float z4_y; float z4_range; float z4_falloff;   /* Highlight */
  float z5_exp; float z5_sat; float z5_x; float z5_y; float z5_range; float z5_falloff;   /* Specular  */

  /* ---- HDR global wheel and adjustments : 10 ---- */
  float g_exp; float g_sat; float g_x; float g_y;
  float temp;            /* the reference's units, 0 = none, warm positive */
  float tint;            /* 0 = none, magenta positive */
  float hue;             /* 0 = none */
  float contrast;        /* 1 = none */
  float pivot;           /* stops over mid grey */
  float blk;             /* Black Offset, 0 = none */

  /* ---- HSL secondary: its correctors : 16 ---- */
  /* The key itself (hue / saturation / level windows, blur, show mask) is the first Grade's:
     the host computes it with drt_grade_key() on the source pixel and hands it to
     drt_grade2_keyed(). One wheel (s1) or three, the key split by the pixel's brightness. */
  float sec_three;       /* 0 = one wheel (s1), 1 = three: Shadows (s0), Midtones (s1), Highlights (s2) */
  float s0_exp; float s0_sat; float s0_x; float s0_y;
  float s1_exp; float s1_sat; float s1_x; float s1_y;
  float s2_exp; float s2_sat; float s2_x; float s2_y;
  int   sec_on;          /* set by the host from the key's Enable and Show Mask */
  int   sec_show;
  int   d_sec_ball;      /* derived: a secondary wheel has X / Y set */

  /* ---- derived by drt_grade2_derive() : 62 ---- */
  int   d_neutral;       /* 1 = every control at its default: the pixel function returns its input untouched */
  int   d_prim;          /* 1 = some Primaries control is off its default */
  int   d_ball_pp;       /* 1 = a zone has X/Y set, so the balance matrix is built per pixel */
  int   d_pad;
  float d_to_m00; float d_to_m01; float d_to_m02;   /* working -> WG, rows */
  float d_to_m10; float d_to_m11; float d_to_m12;
  float d_to_m20; float d_to_m21; float d_to_m22;
  float d_fr_m00; float d_fr_m01; float d_fr_m02;   /* WG -> working */
  float d_fr_m10; float d_fr_m11; float d_fr_m12;
  float d_fr_m20; float d_fr_m21; float d_fr_m22;
  float d_tt_m00; float d_tt_m01; float d_tt_m02;   /* Temp / Tint, then the Global balance when d_ball_pp == 0 */
  float d_tt_m10; float d_tt_m11; float d_tt_m12;
  float d_tt_m20; float d_tt_m21; float d_tt_m22;
  float d_hue_m00; float d_hue_m01; float d_hue_m02;
  float d_hue_m10; float d_hue_m11; float d_hue_m12;
  float d_hue_m20; float d_hue_m21; float d_hue_m22;
  float d_z0_len; float d_z1_len; float d_z2_len; float d_z3_len; float d_z4_len; float d_z5_len;   /* ramp length, stops */
  float d_con_m;         /* contrast exponent */
  float d_con_g;         /* contrast pivot + a, linear */
  float d_con_a;         /* a: the offset contrast works over */
  float d_blk;           /* black offset, linear, at black */
  float d_pa_r; float d_pa_g; float d_pa_b;         /* Primaries: code' = pa + pb * code (Lift and Gain, Offset folded in) */
  float d_pb_r; float d_pb_g; float d_pb_b;
  float d_pe_r; float d_pe_g; float d_pe_b;         /* then code'' = code' ^ pe (Gamma) */
  float d_pad2; float d_pad3; float d_pad4;
};

#define DRT_GRADE2_SCALARS 136   /* 12 + 36 + 10 + 16 (secondary) user; 4 + 36 + 6 + 4 + 9 + 3 (pad) derived; sizeof == 544 on every dialect */

/* --------------------------------------------------------------- constants */
/* the wide grading gamut, D65 (published primaries: R 0.8000 0.3130, G 0.1682 0.9877, B 0.0790 -0.1155). */
#define drt_g2_wg_to_xyz drt_make_mat3(make_float3(0.70062239f, 0.14877482f, 0.10105872f), make_float3(0.27411851f, 0.8736319f, -0.14775041f), make_float3(-0.098962913f, -0.13789533f, 1.325916f))
#define drt_g2_xyz_to_wg drt_make_mat3(make_float3(1.516672f, -0.28147805f, -0.14696363f), make_float3(-0.4649171f, 1.2514238f, 0.17488461f), make_float3(0.064849047f, 0.10913934f, 0.76141462f))

/* Generators in linear WG: the control's matrix is exp(value x G). Fitted (matrix log of a
   least-squares 3x3 over the chart): GX from Global X 0.05 and 0.1, GY from Y -0.1, TEMP from
   1000, TINT from 25, HUE from 10; each per unit of its control. */
#define drt_g2_gen_x    drt_make_mat3(make_float3(-1.1473525f, -1.6294251f, -0.59984259f), make_float3(0.35056693f, 0.4512545f, -0.068133287f), make_float3(1.3230644f, 2.8909381f, 6.1505377f))
#define drt_g2_gen_y    drt_make_mat3(make_float3(2.6636201f, 3.6777158f, 0.73256081f), make_float3(-0.95132202f, -1.3123824f, -0.2297458f), make_float3(-0.086569742f, -0.14390417f, -0.71620356f))
#define drt_g2_gen_temp drt_make_mat3(make_float3(1.9791733e-05f, 2.7110459e-05f, 8.1065824e-06f), make_float3(-6.6339206e-06f, -8.7452181e-06f, -4.5464031e-07f), make_float3(-1.0079958e-05f, -2.1230059e-05f, -5.3270103e-05f))
#define drt_g2_gen_tint drt_make_mat3(make_float3(0.002198809f, 0.0029256896f, 0.00026432889f), make_float3(-0.00083906144f, -0.0011607078f, -0.00033304467f), make_float3(0.0010959171f, 0.0024191414f, 0.0051726372f))
#define drt_g2_gen_hue  drt_make_mat3(make_float3(0.00087981518f, 0.012047349f, -0.01278401f), make_float3(-0.0030632609f, -0.0059976148f, 0.0089623918f), make_float3(0.0095262582f, -0.013424877f, 0.0038412983f))

/* Two luminances, both linear, both measured, WG weights. The zones and Contrast read ZL (fitted
   to the chart under Shadow Exp +1 and under Contrast 1.25 together: 0.3 % of weight, 0.4 % of
   contrast); Saturation holds SL (where Sat 0 lands). They are not the same in the reference, so they
   are not the same here. */
#define DRT_G2_ZL_R 0.2560f
#define DRT_G2_ZL_G 0.7120f
#define DRT_G2_ZL_B 0.0320f
#define DRT_G2_SL_R 0.379870f
#define DRT_G2_SL_G 0.495166f
#define DRT_G2_SL_B 0.124964f

#define DRT_G2_ZONE_MIN_LEN 1.37f    /* stops: the ramp's length when the falloff asks for less */
#define DRT_G2_ZONE_BOX     1.62f    /* stops: the box that rounds the ramp's two corners */
#define DRT_G2_SCALE_END    8.0f     /* the zone scale runs -8 .. +8 stops over grey */
#define DRT_G2_CON_A_UP     0.001f   /* contrast works on value + this: above 1 ... */
#define DRT_G2_CON_A_DOWN   0.01f    /* ... and below 1 (measured at 0.9, 0.8, 0.65, 0.5, 0.2: each 0.0099) */
#define DRT_G2_BLK_K        0.0112f  /* linear lift at black per unit of Black Offset */
#define DRT_G2_BLK_TOP      0.36f    /* ... fading to nothing here (one stop over grey) */
#define DRT_G2_OFFSET_UNIT  0.00733f /* log code per unit of Offset */
#define DRT_G2_SEC_SPLIT    1.0f     /* stops: the three-wheel secondary's Shadows reach down from -this, Highlights up from +this */

/* --------------------------------------------- the log grading curve, per value */
/* A published log curve; checked against the reference to 1.2e-7 over the ramp. Code 0 is
   linear 0, 0.18 -> 0.3363, code 1 is linear 100; a linear toe below 0.00262409 carries
   zero and negatives. */
__DEVICE__ float drt_g2_log_enc(float x) {
  return x > 0.00262409f ? (_log2f(x + 0.0075f) + 7.0f)*0.07329248f : x*10.44426855f;
}
__DEVICE__ float drt_g2_log_dec(float c) {
  return c > 0.02740668f ? _exp2f(c/0.07329248f - 7.0f) - 0.0075f : c/10.44426855f;
}

/* ------------------------------------------------------------ small matrix kit */
__DEVICE__ drt_mat3 drt_g2_mul(drt_mat3 a, drt_mat3 b) {
  return drt_make_mat3(
    make_float3(a.x.x*b.x.x + a.x.y*b.y.x + a.x.z*b.z.x, a.x.x*b.x.y + a.x.y*b.y.y + a.x.z*b.z.y, a.x.x*b.x.z + a.x.y*b.y.z + a.x.z*b.z.z),
    make_float3(a.y.x*b.x.x + a.y.y*b.y.x + a.y.z*b.z.x, a.y.x*b.x.y + a.y.y*b.y.y + a.y.z*b.z.y, a.y.x*b.x.z + a.y.y*b.y.z + a.y.z*b.z.z),
    make_float3(a.z.x*b.x.x + a.z.y*b.y.x + a.z.z*b.z.x, a.z.x*b.x.y + a.z.y*b.y.y + a.z.z*b.z.y, a.z.x*b.x.z + a.z.y*b.y.z + a.z.z*b.z.z));
}
/* a*ka + b*kb */
__DEVICE__ drt_mat3 drt_g2_mix(drt_mat3 a, float ka, drt_mat3 b, float kb) {
  return drt_make_mat3(a.x*ka + b.x*kb, a.y*ka + b.y*kb, a.z*ka + b.z*kb);
}
/* exp(G): halve G six times (a balance at the edge of its range has a norm near 2, so the
   scaled one is under 0.04), a 4th-order series there (error ~1e-9), square back up. Branch-
   free and plain arithmetic, so it can run per pixel when a zone's balance is in use. */
__DEVICE__ drt_mat3 drt_g2_expm(drt_mat3 g) {
  drt_mat3 one = drt_identity();
  drt_mat3 a = drt_g2_mix(g, 1.0f/64.0f, one, 0.0f);
  drt_mat3 a2 = drt_g2_mul(a, a);
  drt_mat3 a3 = drt_g2_mul(a2, a);
  drt_mat3 a4 = drt_g2_mul(a2, a2);
  drt_mat3 t = drt_g2_mix(drt_g2_mix(one, 1.0f, a, 1.0f), 1.0f, drt_g2_mix(a2, 0.5f, a3, 1.0f/6.0f), 1.0f);
  t = drt_g2_mix(t, 1.0f, a4, 1.0f/24.0f);
  t = drt_g2_mul(t, t); t = drt_g2_mul(t, t); t = drt_g2_mul(t, t);
  t = drt_g2_mul(t, t); t = drt_g2_mul(t, t); t = drt_g2_mul(t, t);
  return t;
}
/* The balance matrix for a push of (x, y). */
__DEVICE__ drt_mat3 drt_g2_ball(float x, float y) {
  return drt_g2_expm(drt_g2_mix(drt_g2_gen_x, x, drt_g2_gen_y, y));
}

/* -------------------------------------------------------------- zone weight */
/* Integral of clamp(x, 0, 1). */
__DEVICE__ float drt_g2_ramp_int(float x) {
  return x <= 0.0f ? 0.0f : (x < 1.0f ? 0.5f*x*x : x - 0.5f);
}
/* How far a zone reaches: `dist` = stops INTO the zone from its range value (positive inside),
   `len` its ramp length. 0 well outside, 1 well inside, about 0.15 at the range itself. */
__DEVICE__ float drt_g2_zone_w(float dist, float len) {
  float u = dist/len, b = DRT_G2_ZONE_BOX/len;
  return (drt_g2_ramp_int(u + 0.5f*b) - drt_g2_ramp_int(u - 0.5f*b))/b;
}
/* Ramp length for a range and falloff; `to_end` is the distance from the range to the end of the
   scale on the zone's own side. */
__DEVICE__ float drt_g2_zone_len(float falloff, float to_end) {
  return _fmaxf(falloff*_fmaxf(to_end, 0.0f), DRT_G2_ZONE_MIN_LEN);
}

/* ----------------------------------------------------------------- derive */
__DEVICE__ float drt_g2_prim_a(float lift, float gain) {
  float l = 2.0f*lift;
  return gain*l/(1.0f - l + l*gain);
}
__DEVICE__ float drt_g2_prim_b(float lift, float gain) {
  float l = 2.0f*lift;
  return gain*(1.0f - l)/(1.0f - l + l*gain);
}

__DEVICE__ DrtGrade2Params drt_grade2_derive(DrtGrade2Params g, DRT_PARAMS_ARG p) {
  /* working <-> WG through XYZ */
  drt_mat3 wk_to_xyz = drt_make_mat3(
    make_float3(p.in_m00, p.in_m01, p.in_m02),
    make_float3(p.in_m10, p.in_m11, p.in_m12),
    make_float3(p.in_m20, p.in_m21, p.in_m22));
  drt_mat3 to = drt_g2_mul(drt_g2_xyz_to_wg, wk_to_xyz);
  drt_mat3 fr = drt_mat3_inverse(to);
  g.d_to_m00 = to.x.x; g.d_to_m01 = to.x.y; g.d_to_m02 = to.x.z;
  g.d_to_m10 = to.y.x; g.d_to_m11 = to.y.y; g.d_to_m12 = to.y.z;
  g.d_to_m20 = to.z.x; g.d_to_m21 = to.z.y; g.d_to_m22 = to.z.z;
  g.d_fr_m00 = fr.x.x; g.d_fr_m01 = fr.x.y; g.d_fr_m02 = fr.x.z;
  g.d_fr_m10 = fr.y.x; g.d_fr_m11 = fr.y.y; g.d_fr_m12 = fr.y.z;
  g.d_fr_m20 = fr.z.x; g.d_fr_m21 = fr.z.y; g.d_fr_m22 = fr.z.z;

  /* a zone with its own balance needs the matrix per pixel; otherwise the Global one is folded in here */
  g.d_ball_pp = (g.z0_x != 0.0f || g.z0_y != 0.0f || g.z1_x != 0.0f || g.z1_y != 0.0f || g.z2_x != 0.0f || g.z2_y != 0.0f ||
                 g.z3_x != 0.0f || g.z3_y != 0.0f || g.z4_x != 0.0f || g.z4_y != 0.0f || g.z5_x != 0.0f || g.z5_y != 0.0f) ? 1 : 0;
  drt_mat3 tt = drt_g2_expm(drt_g2_mix(drt_g2_gen_temp, g.temp, drt_g2_gen_tint, g.tint));
  if (g.d_ball_pp == 0) tt = drt_g2_mul(drt_g2_ball(g.g_x, g.g_y), tt);   /* balance after Temp / Tint */
  g.d_tt_m00 = tt.x.x; g.d_tt_m01 = tt.x.y; g.d_tt_m02 = tt.x.z;
  g.d_tt_m10 = tt.y.x; g.d_tt_m11 = tt.y.y; g.d_tt_m12 = tt.y.z;
  g.d_tt_m20 = tt.z.x; g.d_tt_m21 = tt.z.y; g.d_tt_m22 = tt.z.z;
  drt_mat3 hm = drt_g2_expm(drt_g2_mix(drt_g2_gen_hue, g.hue, drt_identity(), 0.0f));
  g.d_hue_m00 = hm.x.x; g.d_hue_m01 = hm.x.y; g.d_hue_m02 = hm.x.z;
  g.d_hue_m10 = hm.y.x; g.d_hue_m11 = hm.y.y; g.d_hue_m12 = hm.y.z;
  g.d_hue_m20 = hm.z.x; g.d_hue_m21 = hm.z.y; g.d_hue_m22 = hm.z.z;

  /* dark-side zones measure the way down to -8, light-side ones the way up to +8 */
  g.d_z0_len = drt_g2_zone_len(g.z0_falloff, g.z0_range + DRT_G2_SCALE_END);
  g.d_z1_len = drt_g2_zone_len(g.z1_falloff, g.z1_range + DRT_G2_SCALE_END);
  g.d_z2_len = drt_g2_zone_len(g.z2_falloff, g.z2_range + DRT_G2_SCALE_END);
  g.d_z3_len = drt_g2_zone_len(g.z3_falloff, DRT_G2_SCALE_END - g.z3_range);
  g.d_z4_len = drt_g2_zone_len(g.z4_falloff, DRT_G2_SCALE_END - g.z4_range);
  g.d_z5_len = drt_g2_zone_len(g.z5_falloff, DRT_G2_SCALE_END - g.z5_range);

  /* Contrast: the exponent is the value below 1 and its fourth power above (1.1 -> 1.46,
     1.25 -> 2.44, 1.5 -> 5.06 measured) */
  float c = _fmaxf(g.contrast, 0.0f);
  g.d_con_m = c > 1.0f ? c*c*c*c : c;
  g.d_con_a = c > 1.0f ? DRT_G2_CON_A_UP : DRT_G2_CON_A_DOWN;
  g.d_con_g = 0.18f*_exp2f(g.pivot) + g.d_con_a;
  g.d_blk = DRT_G2_BLK_K*g.blk;

  /* Primaries. Lift and Gain are one line on the code value: with l = 2 Lift,
     code' = Gain (l + (1 - l) code) / (1 - l + l Gain): each alone is the familiar
     "pin white, move black" and "pin black, scale", and together white lands where the
     measured renders put it. Then Offset adds, then Gamma is a plain power. */
  g.d_pa_r = drt_g2_prim_a(g.lift_r, g.gain_r) + DRT_G2_OFFSET_UNIT*(g.offset_r - 25.0f);
  g.d_pa_g = drt_g2_prim_a(g.lift_g, g.gain_g) + DRT_G2_OFFSET_UNIT*(g.offset_g - 25.0f);
  g.d_pa_b = drt_g2_prim_a(g.lift_b, g.gain_b) + DRT_G2_OFFSET_UNIT*(g.offset_b - 25.0f);
  g.d_pb_r = drt_g2_prim_b(g.lift_r, g.gain_r);
  g.d_pb_g = drt_g2_prim_b(g.lift_g, g.gain_g);
  g.d_pb_b = drt_g2_prim_b(g.lift_b, g.gain_b);
  g.d_pe_r = 1.0f/_fmaxf(1.0f + 4.0f*g.gamma_r, 0.05f);
  g.d_pe_g = 1.0f/_fmaxf(1.0f + 4.0f*g.gamma_g, 0.05f);
  g.d_pe_b = 1.0f/_fmaxf(1.0f + 4.0f*g.gamma_b, 0.05f);
  g.d_prim = (g.d_pa_r != 0.0f || g.d_pa_g != 0.0f || g.d_pa_b != 0.0f || g.d_pb_r != 1.0f || g.d_pb_g != 1.0f || g.d_pb_b != 1.0f ||
              g.d_pe_r != 1.0f || g.d_pe_g != 1.0f || g.d_pe_b != 1.0f) ? 1 : 0;

  int hdr = (g.g_exp != 0.0f || g.g_sat != 1.0f || g.g_x != 0.0f || g.g_y != 0.0f || g.temp != 0.0f || g.tint != 0.0f ||
             g.hue != 0.0f || g.contrast != 1.0f || g.blk != 0.0f || g.d_ball_pp != 0 ||
             g.z0_exp != 0.0f || g.z1_exp != 0.0f || g.z2_exp != 0.0f || g.z3_exp != 0.0f || g.z4_exp != 0.0f || g.z5_exp != 0.0f ||
             g.z0_sat != 1.0f || g.z1_sat != 1.0f || g.z2_sat != 1.0f || g.z3_sat != 1.0f || g.z4_sat != 1.0f || g.z5_sat != 1.0f) ? 1 : 0;
  /* the secondary: with one wheel only s1 counts */
  g.d_sec_ball = (g.sec_on != 0 && (g.s1_x != 0.0f || g.s1_y != 0.0f ||
                                    (g.sec_three != 0.0f && (g.s0_x != 0.0f || g.s0_y != 0.0f || g.s2_x != 0.0f || g.s2_y != 0.0f)))) ? 1 : 0;
  /* at the defaults the effect is an exact pass-through, not a matrix round trip */
  g.d_neutral = (hdr == 0 && g.d_prim == 0 && g.sec_on == 0) ? 1 : 0;
  g.d_pad = 0; g.d_pad2 = 0.0f; g.d_pad3 = 0.0f; g.d_pad4 = 0.0f;
  return g;
}

/* ------------------------------------------------------------------ pixel */
/* One Primaries channel: encode, the line, Gamma, decode. A negative code keeps its sign
   through the power (the toe is linear, so it stays continuous through zero). */
__DEVICE__ float drt_g2_prim_ch(float x, float pa, float pb, float pe) {
  float c = pa + pb*drt_g2_log_enc(x);
  if (pe != 1.0f) c = c < 0.0f ? -_powf(-c, pe) : _powf(c, pe);
  return drt_g2_log_dec(c);
}

/* The grade of one working-gamut pixel with the secondary's key given (drt_grade_key() on the
   same source pixel, blurred or not; 0 when there is no secondary); g through drt_grade2_derive(). */
__DEVICE__ float3 drt_grade2_keyed(DrtGrade2Params g, float3 rgb, float mask) {
  if (g.sec_on != 0 && g.sec_show != 0) return make_float3(mask, mask, mask);
  if (g.d_neutral != 0) return rgb;
  float3 d = drt_vdot(drt_make_mat3(make_float3(g.d_to_m00, g.d_to_m01, g.d_to_m02),
                                    make_float3(g.d_to_m10, g.d_to_m11, g.d_to_m12),
                                    make_float3(g.d_to_m20, g.d_to_m21, g.d_to_m22)), rgb);

  /* 1. select on the source: where the pixel sits on the zone scale, and each zone's reach */
  float L = DRT_G2_ZL_R*d.x + DRT_G2_ZL_G*d.y + DRT_G2_ZL_B*d.z;
  float s = _log2f(_fmaxf(L, 1e-10f)/0.18f);
  float w0 = drt_g2_zone_w(g.z0_range - s, g.d_z0_len);
  float w1 = drt_g2_zone_w(g.z1_range - s, g.d_z1_len);
  float w2 = drt_g2_zone_w(g.z2_range - s, g.d_z2_len);
  float w3 = drt_g2_zone_w(s - g.z3_range, g.d_z3_len);
  float w4 = drt_g2_zone_w(s - g.z4_range, g.d_z4_len);
  float w5 = drt_g2_zone_w(s - g.z5_range, g.d_z5_len);

  /* 2. exposure: the zones add to the Global, in stops */
  float e = g.g_exp + w0*g.z0_exp + w1*g.z1_exp + w2*g.z2_exp + w3*g.z3_exp + w4*g.z4_exp + w5*g.z5_exp;
  d = d*_exp2f(e);

  /* 3, 4. Temp / Tint, then the balance: a zone's push is scaled by its reach BEFORE the
     exponential (Shadow X at half reach is Global X / 2, as measured) */
  drt_mat3 tt = drt_make_mat3(make_float3(g.d_tt_m00, g.d_tt_m01, g.d_tt_m02),
                              make_float3(g.d_tt_m10, g.d_tt_m11, g.d_tt_m12),
                              make_float3(g.d_tt_m20, g.d_tt_m21, g.d_tt_m22));
  d = drt_vdot(tt, d);
  if (g.d_ball_pp != 0) {
    float bx = g.g_x + w0*g.z0_x + w1*g.z1_x + w2*g.z2_x + w3*g.z3_x + w4*g.z4_x + w5*g.z5_x;
    float by = g.g_y + w0*g.z0_y + w1*g.z1_y + w2*g.z2_y + w3*g.z3_y + w4*g.z4_y + w5*g.z5_y;
    d = drt_vdot(drt_g2_ball(bx, by), d);
  }

  /* 5. saturation: the zones multiply (Dark 0.5 with Shadow 0.5 leaves a quarter) */
  float sat = g.g_sat*(1.0f + w0*(g.z0_sat - 1.0f))*(1.0f + w1*(g.z1_sat - 1.0f))*(1.0f + w2*(g.z2_sat - 1.0f))
                     *(1.0f + w3*(g.z3_sat - 1.0f))*(1.0f + w4*(g.z4_sat - 1.0f))*(1.0f + w5*(g.z5_sat - 1.0f));
  if (sat != 1.0f) {
    float y = DRT_G2_SL_R*d.x + DRT_G2_SL_G*d.y + DRT_G2_SL_B*d.z;
    d = make_float3(y + (d.x - y)*sat, y + (d.y - y)*sat, y + (d.z - y)*sat);
  }

  /* 6. Hue */
  if (g.hue != 0.0f)
    d = drt_vdot(drt_make_mat3(make_float3(g.d_hue_m00, g.d_hue_m01, g.d_hue_m02),
                               make_float3(g.d_hue_m10, g.d_hue_m11, g.d_hue_m12),
                               make_float3(g.d_hue_m20, g.d_hue_m21, g.d_hue_m22)), d);

  /* 7. Contrast: a power on (luminance + a) about the pivot; all three channels, each
     with the same a added, take the luminance's ratio, so hue holds and a colour in the
     deep blacks runs to neutral as the reference's does. Above 1 that floor is a hair below
     zero (-a), which the reference also lets through. */
  if (g.contrast != 1.0f) {
    float la = DRT_G2_ZL_R*d.x + DRT_G2_ZL_G*d.y + DRT_G2_ZL_B*d.z + g.d_con_a;
    if (la > 1e-10f) {
      float k = g.d_con_g*_powf(la/g.d_con_g, g.d_con_m)/la;
      d = make_float3((d.x + g.d_con_a)*k - g.d_con_a, (d.y + g.d_con_a)*k - g.d_con_a, (d.z + g.d_con_a)*k - g.d_con_a);
    }
  }

  /* 8. Black Offset: a lift pinned one stop over grey, nothing above it */
  if (g.d_blk != 0.0f)
    d = make_float3(d.x + g.d_blk*_fmaxf(0.0f, 1.0f - d.x/DRT_G2_BLK_TOP),
                    d.y + g.d_blk*_fmaxf(0.0f, 1.0f - d.y/DRT_G2_BLK_TOP),
                    d.z + g.d_blk*_fmaxf(0.0f, 1.0f - d.z/DRT_G2_BLK_TOP));

  /* 9. Primaries, per channel on the log code value */
  if (g.d_prim != 0)
    d = make_float3(drt_g2_prim_ch(d.x, g.d_pa_r, g.d_pb_r, g.d_pe_r),
                    drt_g2_prim_ch(d.y, g.d_pa_g, g.d_pb_g, g.d_pe_g),
                    drt_g2_prim_ch(d.z, g.d_pa_b, g.d_pb_b, g.d_pe_b));

  /* 10. the HSL secondary, last, through the key from the source pixel. Each wheel is an
     HDR-style corrector (exposure, balance, saturation) whose reach is the key; with three
     wheels the key is shared out by the source's brightness: Shadows below about -1 stop,
     Highlights above about +1, on the zones' own ramp, and Midtones is what is left. */
  if (g.sec_on != 0 && mask > 0.0f) {
    float k0 = 0.0f, k1 = mask, k2 = 0.0f;
    if (g.sec_three != 0.0f) {
      float ws = drt_g2_zone_w(-DRT_G2_SEC_SPLIT - s, DRT_G2_ZONE_MIN_LEN);
      float wh = drt_g2_zone_w(s - DRT_G2_SEC_SPLIT, DRT_G2_ZONE_MIN_LEN);
      k0 = mask*ws; k2 = mask*wh; k1 = mask*(1.0f - ws - wh);
    }
    d = d*_exp2f(k0*g.s0_exp + k1*g.s1_exp + k2*g.s2_exp);
    if (g.d_sec_ball != 0)
      d = drt_vdot(drt_g2_ball(k0*g.s0_x + k1*g.s1_x + k2*g.s2_x, k0*g.s0_y + k1*g.s1_y + k2*g.s2_y), d);
    /* the three share one key, so their saturations add by share (three alike are exactly one
       wheel), where the zones, which overlap, multiply */
    float ss = 1.0f + k0*(g.s0_sat - 1.0f) + k1*(g.s1_sat - 1.0f) + k2*(g.s2_sat - 1.0f);
    if (ss != 1.0f) {
      float y = DRT_G2_SL_R*d.x + DRT_G2_SL_G*d.y + DRT_G2_SL_B*d.z;
      d = make_float3(y + (d.x - y)*ss, y + (d.y - y)*ss, y + (d.z - y)*ss);
    }
  }

  return drt_vdot(drt_make_mat3(make_float3(g.d_fr_m00, g.d_fr_m01, g.d_fr_m02),
                                make_float3(g.d_fr_m10, g.d_fr_m11, g.d_fr_m12),
                                make_float3(g.d_fr_m20, g.d_fr_m21, g.d_fr_m22)), d);
}

/* The grade with no secondary. */
__DEVICE__ float3 drt_grade2(DrtGrade2Params g, float3 rgb) {
  return drt_grade2_keyed(g, rgb, 0.0f);
}

#endif /* MINCOLOR_GRADE2_H */
