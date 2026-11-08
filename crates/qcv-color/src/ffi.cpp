// The host half of minColor: QCView's mincolor::resolve (src/color/mincolor_chain.cpp) without Qt, against the vendored
// core. The display is fixed (canon rows 179 / 181): the linear Rec.709 hand-off for the one extended-linear surface,
// 1.0 = 100 nits; a capture resolves the sRGB 2.2 / Rec.709 preset at peak 100 (QCView's sdrCapture rule).
#include "opendrt.h"
#include <algorithm>
#include <cstddef>

struct McChain {
    int in_gamut, in_transfer, in_limited;
    int knee_on; float knee_src, knee_tgt, knee_start;
    int agx_on, agx_target; float agx_peak, agx_white_ev, agx_black_ev, agx_contrast, agx_toe, agx_shoulder, agx_hue_restore, agx_hdr_purity, agx_outset;
    int open_drt, look, tonescale, cwp; float cwp_limit; int surround; float peak_nits, grey_boost, hdr_purity, grey_nits;
};
struct McFlags { int knee_on, agx_on; float out_scale; int pad; };
struct McGpu { drt::DrtParams in, out; drt::DrtAgxParams agx; McFlags flags; };

namespace {
int clamp_index(int v, int count) { return (v >= 0 && v < count) ? v : 0; }
int agx_target_gamut(int t) { switch (t) { case 0: return DRT_IN_REC2020; case 2: return DRT_IN_P3D65; default: return DRT_IN_REC709; } }
}

extern "C" std::size_t mc_gpu_size() { return sizeof(McGpu); }
extern "C" std::size_t mc_chain_size() { return sizeof(McChain); }

extern "C" void mc_resolve(const McChain *c, int capture, McGpu *g)
{
    drt::DrtParams in = drt::drt_stickshift_defaults();
    in.in_gamut = clamp_index(c->in_gamut, DRT_IN_GAMUT_COUNT);
    in.in_oetf = clamp_index(c->in_transfer, DRT_OETF_INVERSE_FIRST);
    in.in_range = c->in_limited ? 1 : 0;
    in.working_gamut = DRT_IN_REC2020;
    in.kn_src = std::max(c->knee_src, 1.0f);
    in.kn_tgt = std::max(c->knee_tgt, 1.0f);
    in.kn_auto = c->knee_start < 0.0f ? 1 : 0;
    in.kn_start = std::clamp(c->knee_start, 0.0f, 0.99f);
    in = drt::drt_derive(in);
    in = drt::drt_knee_derive(in);
    g->in = in;

    drt::DrtAgxParams agx = drt::drt_agx_defaults();
    agx.working_gamut = float(DRT_IN_REC2020);
    agx.target = float(clamp_index(c->agx_target, 3));
    agx.peak = capture ? 100.0f : std::max(c->agx_peak, 100.0f);
    agx.white_ev = c->agx_white_ev; agx.black_ev = c->agx_black_ev; agx.contrast = c->agx_contrast;
    agx.toe_power = c->agx_toe; agx.shoulder_power = c->agx_shoulder; agx.hue_restore = c->agx_hue_restore;
    agx.hdr_purity = c->agx_hdr_purity; agx.outset = c->agx_outset;
    g->agx = drt::drt_agx_derive(agx);

    const bool agx_on = c->agx_on != 0;
    const bool open_drt = c->open_drt != 0 && !agx_on;
    drt::DrtSettings s;
    s.in_gamut = agx_on ? agx_target_gamut(c->agx_target) : DRT_IN_REC2020;
    s.in_oetf = DRT_OETF_LINEAR;
    s.tn_Lp = capture ? 100.0f : std::clamp(c->peak_nits, 48.0f, 10000.0f);
    s.tn_gb = c->grey_boost; s.pt_hdr = c->hdr_purity; s.tn_Lg = c->grey_nits;
    s.look = clamp_index(c->look, drt::kLookCount);
    s.tonescale = std::clamp(c->tonescale, 0, drt::kTonescaleCount);
    s.cwp = std::clamp(c->cwp, 0, drt::kCwpCount);
    s.cwp_lm = c->cwp_limit;
    const int linear709 = drt::drt_display_preset_index(DRT_DG_REC709, DRT_EOTF_LINEAR);
    s.display = capture ? 1 : (linear709 >= 0 ? linear709 : 0);
    drt::DrtParams out = drt::drt_resolve(s);
    out.out_view = open_drt ? 0 : 1;
    if (out.tn_su != c->surround) { out.tn_su = std::clamp(c->surround, 0, 2); out = drt::drt_derive(out); }
    g->out = out;

    g->flags.knee_on = c->knee_on ? 1 : 0;
    g->flags.agx_on = agx_on ? 1 : 0;
    // OpenDRT's linear hand-off puts its peak at 1.0 (ts_dsc = 100 / Lp): scale by peak / 100 so 1.0 = 100 nits;
    // un-tone-mapped and AgX already put 100 nits at 1.0 (QCView's edrLinear rule).
    g->flags.out_scale = (!capture && open_drt) ? s.tn_Lp / 100.0f : 1.0f;
    g->flags.pad = 0;
}

extern "C" void mc_apply(const McGpu *g, float *rgb)
{
    drt::float3 c = drt::make_float3(rgb[0], rgb[1], rgb[2]);
    c = drt::drt_input_transform(g->in, c);
    if (g->flags.knee_on) c = drt::drt_knee(g->in, c);
    if (g->flags.agx_on) c = drt::drt_agx(g->agx, c);
    c = drt::drt_transform(g->out, c);
    rgb[0] = c.x * g->flags.out_scale; rgb[1] = c.y * g->flags.out_scale; rgb[2] = c.z * g->flags.out_scale;
}

extern "C" int mc_look_count() { return drt::kLookCount; }
extern "C" const char *mc_look_name(int i) { return (i >= 0 && i < drt::kLookCount) ? drt::kLooks[i].name : ""; }
extern "C" const char *mc_in_gamut_name(int i) { return (i >= 0 && i < DRT_IN_GAMUT_COUNT) ? drt::kInGamutNames[i] : ""; }
extern "C" const char *mc_in_transfer_name(int i) { return (i >= 0 && i < DRT_OETF_INVERSE_FIRST) ? drt::kInOetfNames[i] : ""; }
extern "C" int mc_in_gamut_count() { return DRT_IN_GAMUT_COUNT; }
extern "C" int mc_in_transfer_count() { return DRT_OETF_INVERSE_FIRST; }

// The Highlight Knee alone (the OCIO split's middle, canon row 204): the vendored core's knee fields, derived.
extern "C" void mc_knee_params(float src_nits, float tgt_nits, float start, drt::DrtParams *out)
{
    drt::DrtParams p = drt::drt_stickshift_defaults();
    p.kn_src = std::max(src_nits, 1.0f);
    p.kn_tgt = std::max(tgt_nits, 1.0f);
    p.kn_auto = start < 0.0f ? 1 : 0;
    p.kn_start = std::clamp(start, 0.0f, 0.99f);
    *out = drt::drt_knee_derive(p);
}
extern "C" void mc_knee_apply(const drt::DrtParams *p, float *rgb)
{
    drt::float3 c = drt::drt_knee(*p, drt::make_float3(rgb[0], rgb[1], rgb[2]));
    rgb[0] = c.x; rgb[1] = c.y; rgb[2] = c.z;
}
