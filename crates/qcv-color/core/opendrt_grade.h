/* SPDX-License-Identifier: GPL-3.0-only
 * minColorAE. Copyright (C) 2026 cbkow. Part of a work derived from OpenDRT v1.1.0 by Jed Smith (GPLv3); see NOTICE.
 */
/* minColor Grade — a colour corrector whose luminance zones are defined through
 * the OpenDRT tonescale. Not part of upstream OpenDRT.
 *
 * Sits between Input and Output in the linear working gamut. Order: everything
 * that SELECTS pixels is measured on the source pixel (the six zone weights and
 * the secondary's key), then everything that CHANGES them, global before local:
 *   temperature / tint -> global exposure -> contrast about a pivot -> six zones
 *   (Black, Dark, Shadow, Light, Highlight, Specular) -> global saturation ->
 *   HSL secondary.
 * So no dial can move another dial's target: Exposure does not shift a pixel into
 * the next zone, a zone's tint does not push a colour out of the secondary's hue
 * window, and Contrast does not re-shape a zone's push (2026-10-03; until then the
 * zones were measured after Exposure and the key after the zones).
 * A zone is a range of SCENE STOPS over mid grey: each pixel's tonescale norm (the
 * quantity the DRT tonescales) is measured in stops relative to the norm of 0.18
 * grey, and six windows over that axis are the zone weights. The contrast pivot
 * and the secondary's level window are on the same axis (stops over grey), so the
 * Grade's maths does not depend on the DRT reference's peak or look at all; the
 * reference serves only the hosts' eyedroppers. A zone has full
 * weight across its band and feathers smoothly to zero over `zfalloff` stops
 * beyond each boundary, and the six weights are then normalised to sum to one:
 * the same push on every zone is exactly that push applied globally, and a zone
 * owns its own band (76 % at its centre, 50 % at a boundary with the default
 * 1-stop falloff). Boundaries default to -3, -1.5, 0, +1.5, +3 stops. The top
 * zones split real highlight range instead of the last few nits of an SDR
 * shoulder. (Until 0.1.4 the weights were not normalised and the falloff was 2
 * stops: a mid-band pixel carried ~2.4 in total, its neighbours 68 % each.)
 *
 * Same dialect rules as opendrt_kernel.h, which must be included first (with
 * opendrt_params.h). DrtGradeParams is a flat 4-byte block like DrtParams, so the
 * host uploads it as a second uniform buffer. The DrtParams `p` is the DRT
 * reference: the host sets p.in_gamut AND p.working_gamut to the working gamut
 * (so in_m** is working -> XYZ and wk_m** its inverse; the tint uses both).
 */
#ifndef OPENDRT_GRADE_H
#define OPENDRT_GRADE_H

#define DRT_ZONE_COUNT 6   /* Black, Dark, Shadow, Light, Highlight, Specular */

struct DrtGradeParams {
  /* ---- switches : 4 ---- */
  int sec_enable;
  int sec_show_mask;
  int reserved_i0;
  int reserved_i1;

  /* ---- global : 6 ---- */
  float exposure;        /* stops */
  float contrast;        /* 1 = none; power about the pivot, applied on the norm */
  float pivot_nits;      /* RETIRED (hidden row keeps its id): display nits the contrast pivoted on */
  float saturation;      /* 1 = none */
  float temperature;     /* -1 .. 1, warm positive */
  float tint;            /* -1 .. 1, magenta positive */

  /* ---- zones, 4 per zone, Black first : 24 ---- */
  float z0_exposure; float z0_saturation; float z0_hue; float z0_tint;
  float z1_exposure; float z1_saturation; float z1_hue; float z1_tint;
  float z2_exposure; float z2_saturation; float z2_hue; float z2_tint;
  float z3_exposure; float z3_saturation; float z3_hue; float z3_tint;
  float z4_exposure; float z4_saturation; float z4_hue; float z4_tint;
  float z5_exposure; float z5_saturation; float z5_hue; float z5_tint;

  /* ---- zone ranges : 6 ---- */
  float zb0; float zb1; float zb2; float zb3; float zb4;   /* boundaries, scene stops over grey */
  float zfalloff;                                          /* stops, feather beyond each boundary */

  /* ---- HSL secondary : 11 + 1 pad ---- */
  float sec_hue;         /* degrees, red = 0, the DRT's hue scale */
  float sec_hue_width;   /* degrees, full width */
  float sec_sat_min;     /* purity, 0..2 */
  float sec_sat_max;
  float sec_lum_min;     /* RETIRED (hidden rows keep their ids): the window in nits */
  float sec_lum_max;
  float sec_softness;    /* RETIRED 0.1.4 (hidden row keeps its id): one softness for all three windows */
  float sec_exposure;
  float sec_saturation;
  float sec_hue_shift;   /* tint hue, degrees */
  float sec_tint;        /* tint amount */
  float pivot_stops;     /* contrast pivot, scene stops over grey (replaces pivot_nits, 2026-10-03) */
  float sec_lev_min;     /* secondary level window, stops over grey (replaces sec_lum_min / max) */
  float sec_lev_max;
  float reserved_f1;
  float reserved_f2;

  /* ---- HSL secondary key rows (0.1.4) : 8 ---- */
  int   sec_hue_on;      /* each window on (1) or ignored (0) */
  int   sec_sat_on;
  int   sec_lev_on;
  float sec_hue_soft;    /* degrees past each end of the hue window to zero */
  float sec_sat_soft;    /* purity past each end of the purity window */
  float sec_lev_soft;    /* stops past each end of the level window */
  float sec_blur;        /* key blur radius in pixels at full resolution (0 = none); the hosts blur the key
                            between drt_grade_key() and drt_grade_keyed(), this core never reads it */
  float reserved_f3;

  /* ---- derived by drt_grade_derive() : 18 ---- */
  float d_pivot_scene;   /* pivot as a scene norm value */
  float d_lb0; float d_lb1; float d_lb2; float d_lb3; float d_lb4;   /* boundaries, log2 of the norm */
  float d_wb_m00; float d_wb_m01; float d_wb_m02;                    /* temperature / tint: a Bradford white */
  float d_wb_m10; float d_wb_m11; float d_wb_m12;                    /* shift in the working gamut (rows), */
  float d_wb_m20; float d_wb_m21; float d_wb_m22;                    /* grey keeps its luminance */
  float d_lum_min_l; float d_lum_max_l;                             /* secondary luminance bounds, log2 nits */
  float d_grey_l;        /* log2 of the norm of 0.18 grey: the zones' zero */
};

#define DRT_GRADE_SCALARS 82   /* 4 + 6 + 24 + 6 + 16 + 8 + 18; sizeof == 328 on every dialect */

/* ------------------------------------------- neutral tonescale, forward */
/* Scene norm -> display-linear fraction, exactly the achromatic path of
   drt_render_linear(): the twin of drt_tonescale_neutral_inv(). */
__DEVICE__ float drt_tonescale_neutral_fwd(DRT_PARAMS_ARG p, float t) {
  if (p.tn_lcon_enable != 0) {
    t *= p.lcon_cnst_sc;
    t = drt_compress_toe_cubic(t, p.lcon_m, p.lcon_w, 0);
  }
  if (p.tn_hcon_enable != 0) t = drt_contrast_high(t, p.hcon_p, p.tn_hcon_pv, p.tn_hcon_st, 0);
  t = drt_compress_hyperbolic_power(t, p.ts_s, p.ts_p);
  t *= p.ts_m2;
  t = drt_compress_toe_quadratic(t, p.tn_toe, 0);
  t *= p.ts_dsc;
  return t;
}

/* Display-linear fraction (the forward's own convention) -> nits. */
__DEVICE__ float drt_display_nits(DRT_PARAMS_ARG p, float t) {
  if (p.eotf == 4) return t*10000.0f;   /* PQ: ts_dsc put 100 nits at 0.01 */
  if (p.eotf == 5) return t*1000.0f;    /* HLG: 1000-nit system, ts_dsc 0.1 */
  return t*p.tn_Lp;
}
__DEVICE__ float drt_nits_to_display(DRT_PARAMS_ARG p, float nits) {
  if (p.eotf == 4) return nits/10000.0f;
  if (p.eotf == 5) return nits/1000.0f;
  return nits/p.tn_Lp;
}

/* Where the DRT would put this working-gamut pixel: its tonescale norm (the same
   quantity drt_render_linear() computes before the ratios are taken). */
__DEVICE__ float drt_grade_norm(DRT_PARAMS_ARG p, float3 rgb) {
  drt_mat3 wk_to_xyz = drt_make_mat3(
    make_float3(p.in_m00, p.in_m01, p.in_m02),
    make_float3(p.in_m10, p.in_m11, p.in_m12),
    make_float3(p.in_m20, p.in_m21, p.in_m22));
  float3 p3 = drt_vdot(drt_matrix_xyz_to_p3d65, drt_vdot(wk_to_xyz, rgb));
  float sat_L = p3.x*p.rs_wr + p3.y*p.rs_wg + p3.z*p.rs_wb;
  p3 = sat_L*p.rs_sa + p3*(1.0f - p.rs_sa);
  p3 += p.tn_off;
  return drt_hypotf3(p3)/DRT_SQRT3;
}

__DEVICE__ float drt_log2_nits(DRT_PARAMS_ARG p, float norm) {
  float n = drt_display_nits(p, drt_tonescale_neutral_fwd(p, _fmaxf(norm, 0.0f)));
  return _log2f(_fmaxf(n, 1e-4f));
}

/* Smooth step over [-0.5, 0.5] in units of the falloff (the secondary's luminance window). */
__DEVICE__ float drt_zone_step(float x) {
  float u = drt_clampf(x + 0.5f, 0.0f, 1.0f);
  return u*u*(3.0f - 2.0f*u);
}

/* Zone feather: 1 at x <= 0 (inside the band), smooth to 0 at x >= 1 (one falloff past the boundary). */
__DEVICE__ float drt_zone_edge(float x) {
  float u = drt_clampf(x, 0.0f, 1.0f);
  return 1.0f - u*u*(3.0f - 2.0f*u);
}

/* Zero-sum RGB direction for a hue on the DRT's hue scale (red = 0). Inverse of
   drt_opponent(): opp = (R - B, G - (R + B)/2), hue = atan2(opp.x, opp.y) + PI + 1.107. */
/* sin by Taylor to the 7th power: error < 2e-4 on [-PI/2, PI/2], where drt_hue_dir
   keeps its arguments; far better than a tint direction needs. sin and cos are not in the shim's intrinsic
   list (the DCTL never needed them), so this keeps the dialect contract intact. */
__DEVICE__ float drt_sin_pi(float x) {
  float x2 = x*x;
  return x*(1.0f - x2*(1.0f/6.0f) + x2*x2*(1.0f/120.0f) - x2*x2*x2*(1.0f/5040.0f));
}

__DEVICE__ float3 drt_hue_dir(float hue_deg) {
  float a = hue_deg*(DRT_PI/180.0f) - DRT_PI - 1.10714931f;
  /* reduce to [-PI, PI] */
  a = _fmod(a + DRT_PI, 2.0f*DRT_PI);
  if (a < 0.0f) a += 2.0f*DRT_PI;
  a -= DRT_PI;
  /* the series is only good near zero (at +-PI it is off by 0.075, several degrees of hue,
     until 0.1.4): fold sin into [-PI/2, PI/2] with sin(PI - a) = sin(a); the cos argument
     PI/2 - |a| is already in range */
  float as = a > 0.5f*DRT_PI ? DRT_PI - a : (a < -0.5f*DRT_PI ? -DRT_PI - a : a);
  float ox = drt_sin_pi(as);
  float oy = drt_sin_pi(DRT_PI*0.5f - (a < 0.0f ? -a : a));   /* cos(a) = sin(PI/2 - |a|) */
  float g = (2.0f/3.0f)*oy;
  float r = (ox - (2.0f/3.0f)*oy)*0.5f;
  float b = (-ox - (2.0f/3.0f)*oy)*0.5f;
  float3 d = make_float3(r, g, b);
  float len = drt_hypotf3(d);
  return len > 1e-6f ? d/len : make_float3(0.0f, 0.0f, 0.0f);
}

/* The white a Temperature / Tint pair shifts D65 to, XYZ with Y = 1. Temperature
   moves along the Planckian locus by mireds (warm positive: +1 = +100 mired, D65's
   6504 K to ~3940 K; -1 to the 15000 K end of the fit), Tint across it by Delta-uv
   (magenta positive: +1 = 0.02 below the locus), both as offsets from D65 itself so
   0, 0 is exactly D65. Locus: Krystek's rational fit in CIE 1960 uv (1000-15000 K). */
__DEVICE__ float2 drt_planck_uv(float T) {
  float u = (0.860117757f + 1.54118254e-4f*T + 1.28641212e-7f*T*T)/(1.0f + 8.42420235e-4f*T + 7.08145163e-7f*T*T);
  float v = (0.317398726f + 4.22806245e-5f*T + 4.20481691e-8f*T*T)/(1.0f - 2.89741816e-5f*T + 1.61456053e-7f*T*T);
  return make_float2(u, v);
}
__DEVICE__ float3 drt_wb_white(float temperature, float tint) {
  float m = _fmaxf(1.0e6f/6504.0f + 100.0f*temperature, 1.0e6f/15000.0f);
  float2 p0 = drt_planck_uv(6504.0f), p1 = drt_planck_uv(1.0e6f/m);
  float2 pa = drt_planck_uv(6404.0f), pb = drt_planck_uv(6604.0f);   /* the locus' direction at D65 */
  float du = pb.x - pa.x, dv = pb.y - pa.y, dl = _sqrtf(du*du + dv*dv);
  /* D65 in uv, moved along the locus, then along its normal (rotated toward the magenta side) */
  float u = 0.197839f + (p1.x - p0.x) - 0.02f*tint*(dv/dl);
  float v = 0.312262f + (p1.y - p0.y) + 0.02f*tint*(du/dl);
  float d = 2.0f*u - 8.0f*v + 4.0f;
  float x = 3.0f*u/d, y = 2.0f*v/d;
  return make_float3(x/y, 1.0f, (1.0f - x - y)/y);
}

/* Tint strength: a zone at Tint 1 is pushed by this many times its own luminance
   along the hue (opponent purity), luminance held. */
#define DRT_GRADE_TINT_K 0.5f   /* provisional, to calibrate against Lumetri; at 1 a full yellow push takes grey's blue below zero */

/* ----------------------------------------------------------------- derive */
__DEVICE__ DrtGradeParams drt_grade_derive(DrtGradeParams g, DRT_PARAMS_ARG p) {
  g.d_grey_l = _log2f(_fmaxf(drt_grade_norm(p, make_float3(0.18f, 0.18f, 0.18f)), 1e-6f));
  g.d_pivot_scene = _exp2f(g.d_grey_l + g.pivot_stops);
  g.d_lb0 = g.d_grey_l + g.zb0;
  g.d_lb1 = g.d_grey_l + g.zb1;
  g.d_lb2 = g.d_grey_l + g.zb2;
  g.d_lb3 = g.d_grey_l + g.zb3;
  g.d_lb4 = g.d_grey_l + g.zb4;
  /* temperature / tint: a white-balance shift, D65 to the white below, Bradford, as
     one working-gamut matrix (working -> XYZ -> cone space -> scaled -> back) */
  drt_mat3 wk_to_xyz = drt_make_mat3(
    make_float3(p.in_m00, p.in_m01, p.in_m02),
    make_float3(p.in_m10, p.in_m11, p.in_m12),
    make_float3(p.in_m20, p.in_m21, p.in_m22));
  drt_mat3 xyz_to_wk = drt_mat3_inverse(wk_to_xyz);
  float3 wd = drt_wb_white(0.0f, 0.0f), wt = drt_wb_white(g.temperature, g.tint);
  drt_mat3 brad = drt_make_mat3(make_float3(0.8951f, 0.2664f, -0.1614f), make_float3(-0.7502f, 1.7135f, 0.0367f), make_float3(0.0389f, -0.0685f, 1.0296f));
  float3 ld = drt_vdot(brad, wd), lt = drt_vdot(brad, wt);
  float3 gain = make_float3(lt.x/ld.x, lt.y/ld.y, lt.z/ld.z);
  drt_mat3 ibrad = drt_mat3_inverse(brad);
  float3 c0 = drt_vdot(xyz_to_wk, drt_vdot(ibrad, gain*drt_vdot(brad, drt_vdot(wk_to_xyz, make_float3(1.0f, 0.0f, 0.0f)))));
  float3 c1 = drt_vdot(xyz_to_wk, drt_vdot(ibrad, gain*drt_vdot(brad, drt_vdot(wk_to_xyz, make_float3(0.0f, 1.0f, 0.0f)))));
  float3 c2 = drt_vdot(xyz_to_wk, drt_vdot(ibrad, gain*drt_vdot(brad, drt_vdot(wk_to_xyz, make_float3(0.0f, 0.0f, 1.0f)))));
  g.d_wb_m00 = c0.x; g.d_wb_m01 = c1.x; g.d_wb_m02 = c2.x;
  g.d_wb_m10 = c0.y; g.d_wb_m11 = c1.y; g.d_wb_m12 = c2.y;
  g.d_wb_m20 = c0.z; g.d_wb_m21 = c1.z; g.d_wb_m22 = c2.z;
  g.d_lum_min_l = g.d_grey_l + g.sec_lev_min;   /* log2 of the norm */
  g.d_lum_max_l = g.d_grey_l + g.sec_lev_max;
  return g;
}

/* --------------------------------------------------------------- helpers */
__DEVICE__ float drt_grade_luma(DRT_PARAMS_ARG p, float3 rgb) {
  return p.in_m10*rgb.x + p.in_m11*rgb.y + p.in_m12*rgb.z;   /* Y of working -> XYZ */
}

/* Apply exposure / saturation / tint with a weight. */
__DEVICE__ float3 drt_grade_apply(DRT_PARAMS_ARG p, float3 rgb, float w, float exposure, float saturation, float hue_deg, float tint) {
  if (w <= 0.0f) return rgb;
  rgb = rgb*_powf(2.0f, w*exposure);
  float y = drt_grade_luma(p, rgb);
  float s = 1.0f + w*(saturation - 1.0f);
  rgb = make_float3(y + (rgb.x - y)*s, y + (rgb.y - y)*s, y + (rgb.z - y)*s);
  if (tint != 0.0f) {
    /* the hue's direction on the secondary's scale (P3-D65 opponent, as drt_secondary_measure
       reads it) taken into the working gamut, then its luminance taken out: adding grey leaves
       the opponent hue and purity alone, so the push changes colour and not brightness */
    float3 d = drt_vdot(drt_make_mat3(
      make_float3(p.wk_m00, p.wk_m01, p.wk_m02),
      make_float3(p.wk_m10, p.wk_m11, p.wk_m12),
      make_float3(p.wk_m20, p.wk_m21, p.wk_m22)), drt_vdot(drt_matrix_p3d65_to_xyz, drt_hue_dir(hue_deg)));
    float dy = drt_grade_luma(p, d);
    d = make_float3(d.x - dy, d.y - dy, d.z - dy);
    rgb += d*(_fmaxf(y, 0.0f)*DRT_GRADE_TINT_K*w*tint);
  }
  return rgb;
}

/* -------------------------------------------------- secondary measure */
/* What the HSL secondary qualifies on: the DRT's opponent hue (degrees, red = 0,
   the same scale as the tint hues), its purity, and the log2 of its tonescale
   norm (stops over grey once d_grey_l is taken off). The hosts' eyedroppers use
   this too, so a pick lands exactly where the mask looks. */
struct drt_sec_measure {
  float hue_deg;
  float purity;
  float log2_norm;
};

__DEVICE__ drt_sec_measure drt_secondary_measure(DRT_PARAMS_ARG p, float3 rgb) {
  float n = drt_grade_norm(p, rgb);
  drt_mat3 wk_to_xyz = drt_make_mat3(
    make_float3(p.in_m00, p.in_m01, p.in_m02),
    make_float3(p.in_m10, p.in_m11, p.in_m12),
    make_float3(p.in_m20, p.in_m21, p.in_m22));
  float3 p3 = drt_vdot(drt_matrix_xyz_to_p3d65, drt_vdot(wk_to_xyz, rgb));
  float3 ratios = drt_sdivf3f(p3 + p.tn_off, _fmaxf(n, 1e-8f));
  float2 opp = drt_opponent(ratios);
  drt_sec_measure m;
  m.purity = drt_hypotf2(opp)/2.0f;
  m.hue_deg = _fmod(_atan2f(opp.x, opp.y) + DRT_PI + 1.10714931f, 2.0f*DRT_PI)*(180.0f/DRT_PI);
  m.log2_norm = _log2f(_fmaxf(n, 1e-6f));
  return m;
}

/* ------------------------------------------------------------------ grade */
/* The secondary's key for a source pixel: 1 inside all its windows, 0 outside, 0 when the
   secondary is off. The hosts may blur it (Key Blur) before drt_grade_keyed() uses it. */
__DEVICE__ float drt_grade_key(DrtGradeParams g, DRT_PARAMS_ARG p, float3 rgb) {
  if (g.sec_enable == 0) return 0.0f;
  drt_sec_measure sm = drt_secondary_measure(p, rgb);
  float purity = sm.purity;
  float hue = sm.hue_deg*(DRT_PI/180.0f);
  float Ls = sm.log2_norm;
  /* each window is full across its range and feathers to zero over its own softness past
     each end (0.1.4: the key bars draw it that way; before, one Softness straddled the ends) */
  float mh = 1.0f, mp = 1.0f, ml = 1.0f;
  if (g.sec_hue_on != 0) {
    float hc = g.sec_hue*(DRT_PI/180.0f);
    float dh = _fabs(drt_hue_offset(hue, hc));
    float hw = 0.5f*g.sec_hue_width*(DRT_PI/180.0f);
    float he = _fmaxf(g.sec_hue_soft, 0.01f)*(DRT_PI/180.0f);
    mh = drt_zone_edge((dh - hw)/he);
  }
  if (g.sec_sat_on != 0) {
    float pe = _fmaxf(g.sec_sat_soft, 1e-4f);
    /* Saturation Min 0 means no lower bound, so greys sit fully inside */
    float mp_lo = g.sec_sat_min <= 0.0f ? 1.0f : drt_zone_edge((g.sec_sat_min - purity)/pe);
    mp = mp_lo*drt_zone_edge((purity - g.sec_sat_max)/pe);
  }
  if (g.sec_lev_on != 0) {
    float le = _fmaxf(g.sec_lev_soft, 1e-3f);
    ml = drt_zone_edge((g.d_lum_min_l - Ls)/le)*drt_zone_edge((Ls - g.d_lum_max_l)/le);
  }
  return mh*mp*ml;
}

/* One tap of the key blur: a Gaussian whose 3 sigma is the radius r (pixels), unnormalised. */
__DEVICE__ float drt_key_blur_weight(float k, float r) {
  float s = _fmaxf(r, 1e-3f)*(1.0f/3.0f);
  return _expf(-0.5f*(k*k)/(s*s));
}

/* The grade with the secondary's key given (drt_grade_key(), blurred or not). */
__DEVICE__ float3 drt_grade_keyed(DrtGradeParams g, DRT_PARAMS_ARG p, float3 rgb, float mask) {
  /* 1. select on the source: the zone weights (the secondary's key came in) */
  float L = _log2f(_fmaxf(drt_grade_norm(p, rgb), 1e-6f));
  float f = _fmaxf(g.zfalloff, 1e-3f);
  /* zone weights: full across the band, feathered `f` stops past each boundary, free to overlap */
  float w0 = drt_zone_edge((L - g.d_lb0)/f);
  float w1 = drt_zone_edge((g.d_lb0 - L)/f)*drt_zone_edge((L - g.d_lb1)/f);
  float w2 = drt_zone_edge((g.d_lb1 - L)/f)*drt_zone_edge((L - g.d_lb2)/f);
  float w3 = drt_zone_edge((g.d_lb2 - L)/f)*drt_zone_edge((L - g.d_lb3)/f);
  float w4 = drt_zone_edge((g.d_lb3 - L)/f)*drt_zone_edge((L - g.d_lb4)/f);
  float w5 = drt_zone_edge((g.d_lb4 - L)/f);
  float ws = 1.0f/_fmaxf(w0 + w1 + w2 + w3 + w4 + w5, 1e-6f);   /* sum to one (never below 1 before: Black and Specular are open-ended) */
  w0 *= ws; w1 *= ws; w2 *= ws; w3 *= ws; w4 *= ws; w5 *= ws;
  if (g.sec_enable != 0 && g.sec_show_mask != 0) return make_float3(mask, mask, mask);

  /* 2. global: temperature / tint, exposure */
  rgb = drt_vdot(drt_make_mat3(make_float3(g.d_wb_m00, g.d_wb_m01, g.d_wb_m02),
                               make_float3(g.d_wb_m10, g.d_wb_m11, g.d_wb_m12),
                               make_float3(g.d_wb_m20, g.d_wb_m21, g.d_wb_m22)), rgb);
  rgb = rgb*_powf(2.0f, g.exposure);

  /* 3. contrast about the pivot, on the norm so ratios (hue) hold; before the zones so a
        zone's push is judged on the final tone and not re-shaped by contrast */
  if (g.contrast != 1.0f) {
    float n = drt_grade_norm(p, rgb);
    if (n > 1e-8f) {
      float n2 = g.d_pivot_scene*_powf(n/g.d_pivot_scene, g.contrast);
      rgb = rgb*(n2/n);
    }
  }

  /* 4. the zones, with the weights from the source */
  rgb = drt_grade_apply(p, rgb, w0, g.z0_exposure, g.z0_saturation, g.z0_hue, g.z0_tint);
  rgb = drt_grade_apply(p, rgb, w1, g.z1_exposure, g.z1_saturation, g.z1_hue, g.z1_tint);
  rgb = drt_grade_apply(p, rgb, w2, g.z2_exposure, g.z2_saturation, g.z2_hue, g.z2_tint);
  rgb = drt_grade_apply(p, rgb, w3, g.z3_exposure, g.z3_saturation, g.z3_hue, g.z3_tint);
  rgb = drt_grade_apply(p, rgb, w4, g.z4_exposure, g.z4_saturation, g.z4_hue, g.z4_tint);
  rgb = drt_grade_apply(p, rgb, w5, g.z5_exposure, g.z5_saturation, g.z5_hue, g.z5_tint);

  /* 5. global saturation */
  if (g.saturation != 1.0f) {
    float y = drt_grade_luma(p, rgb);
    rgb = make_float3(y + (rgb.x - y)*g.saturation, y + (rgb.y - y)*g.saturation, y + (rgb.z - y)*g.saturation);
  }

  /* 6. the secondary's correctors through the mask from the source */
  if (g.sec_enable != 0) rgb = drt_grade_apply(p, rgb, mask, g.sec_exposure, g.sec_saturation, g.sec_hue_shift, g.sec_tint);
  return rgb;
}

/* The grade of one pixel, its own key unblurred. */
__DEVICE__ float3 drt_grade(DrtGradeParams g, DRT_PARAMS_ARG p, float3 rgb) {
  return drt_grade_keyed(g, p, rgb, drt_grade_key(g, p, rgb));
}

#endif /* OPENDRT_GRADE_H */
