// The OCIO half of qcv-color (canon rows 183 / 198 / 200 / 204): a C ABI over OpenColorIO 2.6 for the native-shader
// engine. Grown from S1's spike shim (spikes/s1-present/src/ocio_shim.cpp) and QCView's OcioChainBuilder.
//
// ONE DISPLAY, BUILT IN CODE (row 183): every config gets a display "Linear Screen" whose colourspace is display-linear
// Rec.709 (1.0 = 100 nits, the extended-linear surface's encoding) and whose views are the config's own views, gathered
// from its displays (the sRGB display first, then the rest; first name wins). The user never picks a display; the
// patch is derived from the config itself, so a config upgrade needs no hand-made patch (§18.1's upkeep).
#include <OpenColorIO/OpenColorIO.h>
#include <algorithm>
#include <cstdio>
#include <cstring>
#include <set>
#include <string>
#include <vector>
namespace OCIO = OCIO_NAMESPACE;

static const char *kDisplay = "Linear Screen";
static const char *kDisplayCs = "Linear Screen (Rec.709, 1.0 = 100 nits)";

struct OcConfig {
    OCIO::ConstConfigRcPtr cfg;
    std::vector<std::string> inputs, looks, views;
    std::string warning;   // validate()'s complaint, if any (OCIO 2.6 is stricter than the 2.5 the configs were patched under)
};
struct OcChain {
    OCIO::ConstProcessorRcPtr proc;
    OCIO::ConstCPUProcessorRcPtr cpu;
    OCIO::GpuShaderDescRcPtr desc;
    std::string shader;
};

static void put(char *err, size_t n, const char *m) { if (err && n) snprintf(err, n, "%s", m); }

static bool contains(const std::string &s, const char *sub) { return s.find(sub) != std::string::npos; }

extern "C" {

const char *oc_version() { return OCIO::GetVersion(); }
const char *oc_display() { return kDisplay; }

OcConfig *oc_config_open(const char *path, char *err, size_t errn)
{
    try {
        OCIO::ConstConfigRcPtr src = OCIO::Config::CreateFromFile(path);
        OCIO::ConfigRcPtr cfg = src->createEditableCopy();
        // the display colourspace: display reference (CIE XYZ D65 in every OCIO v2 config) → linear Rec.709
        if (!cfg->hasRole("cie_xyz_d65_interchange")) { put(err, errn, "this config has no display reference (cie_xyz_d65_interchange role): an OCIO v1 config is not supported"); return nullptr; }
        OCIO::ColorSpaceRcPtr cs = OCIO::ColorSpace::Create(OCIO::REFERENCE_SPACE_DISPLAY);
        cs->setName(kDisplayCs);
        cs->setFamily("Display");
        cs->setEncoding("display-linear");
        cs->setDescription("minplatform: the extended-linear surface (Apple EDR / Windows scRGB), Rec.709 primaries, 1.0 = 100 nits SDR white");
        OCIO::MatrixTransformRcPtr m = OCIO::MatrixTransform::Create();
        const double x2r[16] = { 3.2409699419045226, -1.5373831775700939, -0.4986107602930034, 0,
                                -0.9692436362808796,  1.8759675015077202,  0.0415550574071756, 0,
                                 0.0556300796969936, -0.2039769588889765,  1.0569715142428786, 0,
                                 0, 0, 0, 1 };
        m->setMatrix(x2r);
        cs->setTransform(m, OCIO::COLORSPACE_DIR_FROM_REFERENCE);
        cfg->addColorSpace(cs);
        // the views: the config's own, sRGB display first
        std::vector<std::string> displays;
        for (int i = 0; i < src->getNumDisplays(); ++i) displays.push_back(src->getDisplay(i));
        std::stable_partition(displays.begin(), displays.end(), [](const std::string &d) { return contains(d, "sRGB") && !contains(d, "EDR") && !contains(d, "Linear"); });
        std::set<std::string> seen;
        auto *oc = new OcConfig;
        for (const std::string &d : displays) {
            for (int i = 0; i < src->getNumViews(d.c_str()); ++i) {
                const std::string v = src->getView(d.c_str(), i);
                if (seen.count(v)) continue;
                const char *vt = src->getDisplayViewTransformName(d.c_str(), v.c_str());
                const char *looks = src->getDisplayViewLooks(d.c_str(), v.c_str());
                const char *rule = src->getDisplayViewRule(d.c_str(), v.c_str());
                const char *desc = src->getDisplayViewDescription(d.c_str(), v.c_str());
                if (vt && *vt) {
                    cfg->addDisplayView(kDisplay, v.c_str(), vt, kDisplayCs, looks ? looks : "", rule ? rule : "", desc ? desc : "");
                } else {
                    // a view straight to a colourspace: only data views (Raw) mean the same on our display
                    const char *vcs = src->getDisplayViewColorSpaceName(d.c_str(), v.c_str());
                    OCIO::ConstColorSpaceRcPtr c = vcs ? src->getColorSpace(vcs) : nullptr;
                    if (!c || !c->isData()) continue;
                    cfg->addDisplayView(kDisplay, v.c_str(), vcs, looks ? looks : "");
                }
                seen.insert(v);
                oc->views.push_back(v);
            }
        }
        std::string act = src->getActiveDisplays();
        if (!act.empty()) cfg->setActiveDisplays((act + ", " + kDisplay).c_str());
        oc->cfg = cfg;
        // inputs: every scene / display colourspace that is not data and not our own
        for (int i = 0; i < cfg->getNumColorSpaces(); ++i) {
            const char *n = cfg->getColorSpaceNameByIndex(i);
            OCIO::ConstColorSpaceRcPtr c = cfg->getColorSpace(n);
            if (!c || c->isData() || std::strcmp(n, kDisplayCs) == 0) continue;
            oc->inputs.push_back(n);
        }
        for (int i = 0; i < cfg->getNumLooks(); ++i) oc->looks.push_back(cfg->getLookNameByIndex(i));
        try { cfg->validate(); } catch (const std::exception &e) { oc->warning = e.what(); }
        return oc;
    } catch (const std::exception &e) { put(err, errn, e.what()); return nullptr; }
}

void oc_config_free(OcConfig *c) { delete c; }
const char *oc_config_warning(const OcConfig *c) { return c->warning.c_str(); }
const char *oc_config_role(const OcConfig *c, const char *role) { if (!c->cfg->hasRole(role)) return ""; const char *n = c->cfg->getRoleColorSpace(role); return n ? n : ""; }
int oc_config_count(const OcConfig *c, int what) { return what == 0 ? (int)c->inputs.size() : what == 1 ? (int)c->looks.size() : (int)c->views.size(); }
const char *oc_config_item(const OcConfig *c, int what, int i) {
    const auto &v = what == 0 ? c->inputs : what == 1 ? c->looks : c->views;
    return (i >= 0 && i < (int)v.size()) ? v[i].c_str() : "";
}
const char *oc_config_family(const OcConfig *c, const char *cs) { OCIO::ConstColorSpaceRcPtr s = c->cfg->getColorSpace(cs); return s ? s->getFamily() : ""; }
const char *oc_config_encoding(const OcConfig *c, const char *cs) { OCIO::ConstColorSpaceRcPtr s = c->cfg->getColorSpace(cs); return s ? s->getEncoding() : ""; }

// input → [look] → [scene LUT / CDL] → our display + view. lang: 0 MSL 2.0, 1 HLSL SM 5.0.
OcChain *oc_chain_build(const OcConfig *c, const char *input, const char *look, const char *view, const char *scene_lut,
                        int lang, const char *fn_name, char *err, size_t errn)
{
    try {
        OCIO::GroupTransformRcPtr g = OCIO::GroupTransform::Create();
        std::string src = input;
        if (look && *look) {
            const char *res = OCIO::LookTransform::GetLooksResultColorSpace(c->cfg, c->cfg->getCurrentContext(), look);
            if (res && *res) {
                OCIO::LookTransformRcPtr lt = OCIO::LookTransform::Create();
                lt->setSrc(input); lt->setDst(res); lt->setLooks(look);
                g->appendTransform(lt); src = res;
            }
        }
        if (scene_lut && *scene_lut) {
            OCIO::FileTransformRcPtr ft = OCIO::FileTransform::Create();
            ft->setSrc(scene_lut); ft->setInterpolation(OCIO::INTERP_BEST); ft->setDirection(OCIO::TRANSFORM_DIR_FORWARD);
            g->appendTransform(ft);
        }
        OCIO::DisplayViewTransformRcPtr dvt = OCIO::DisplayViewTransform::Create();
        dvt->setSrc(src.c_str()); dvt->setDisplay(kDisplay); dvt->setView(view);
        g->appendTransform(dvt);
        auto *ch = new OcChain;
        ch->proc = c->cfg->getProcessor(g);
        ch->cpu = ch->proc->getDefaultCPUProcessor();
        OCIO::ConstGPUProcessorRcPtr gpu = ch->proc->getDefaultGPUProcessor();
        ch->desc = OCIO::GpuShaderDesc::CreateShaderDesc();
        ch->desc->setLanguage(lang == 1 ? OCIO::GPU_LANGUAGE_HLSL_SM_5_0 : OCIO::GPU_LANGUAGE_MSL_2_0);
        ch->desc->setFunctionName(fn_name);
        ch->desc->setResourcePrefix("ocio");
        gpu->extractGpuShaderInfo(ch->desc);
        ch->shader = ch->desc->getShaderText();
        return ch;
    } catch (const std::exception &e) { put(err, errn, e.what()); return nullptr; }
}

// QCView's split (OcioChainBuilder::buildSplitTransforms): pre = [look] [scene LUT] → the interchange role; post =
// interchange → the display's view. The Knee runs between, in linear light. side: 1 scene (aces_interchange, AP0 D60),
// 2 display (cie_xyz_d65_interchange); both put 1.0 at 100 nits.
int oc_chain_build_split(const OcConfig *c, const char *input, const char *look, const char *view, const char *scene_lut,
                         int lang, OcChain **pre_out, OcChain **post_out, char *err, size_t errn)
{
    try {
        std::string src = input;
        OCIO::GroupTransformRcPtr pre = OCIO::GroupTransform::Create();
        if (look && *look) {
            const char *res = OCIO::LookTransform::GetLooksResultColorSpace(c->cfg, c->cfg->getCurrentContext(), look);
            if (res && *res) { OCIO::LookTransformRcPtr lt = OCIO::LookTransform::Create(); lt->setSrc(input); lt->setDst(res); lt->setLooks(look); pre->appendTransform(lt); src = res; }
        }
        if (scene_lut && *scene_lut) { OCIO::FileTransformRcPtr ft = OCIO::FileTransform::Create(); ft->setSrc(scene_lut); ft->setInterpolation(OCIO::INTERP_BEST); pre->appendTransform(ft); }
        OCIO::ConstColorSpaceRcPtr scs = c->cfg->getColorSpace(src.c_str());
        if (!scs) { put(err, errn, "unknown input colourspace"); return 0; }
        if (scs->isData()) { put(err, errn, "the input is a data colourspace: nothing for the Knee to act on"); return 0; }
        const char *vcs = c->cfg->getDisplayViewColorSpaceName(kDisplay, view);
        OCIO::ConstColorSpaceRcPtr v = vcs ? c->cfg->getColorSpace(vcs) : nullptr;
        if (v && v->isData()) { put(err, errn, "a data view (Raw): nothing for the Knee to act on"); return 0; }
        const bool scene = scs->getReferenceSpaceType() == OCIO::REFERENCE_SPACE_SCENE;
        const char *role = scene ? "aces_interchange" : "cie_xyz_d65_interchange";
        if (!c->cfg->hasRole(role)) { put(err, errn, scene ? "the config has no aces_interchange role" : "the config has no cie_xyz_d65_interchange role"); return 0; }
        const char *ic = c->cfg->getRoleColorSpace(role);
        OCIO::ColorSpaceTransformRcPtr to = OCIO::ColorSpaceTransform::Create(); to->setSrc(src.c_str()); to->setDst(ic); pre->appendTransform(to);
        OCIO::DisplayViewTransformRcPtr dvt = OCIO::DisplayViewTransform::Create(); dvt->setSrc(ic); dvt->setDisplay(kDisplay); dvt->setView(view);
        auto mk = [&](OCIO::ConstTransformRcPtr t, const char *fn, const char *prefix) {
            auto *ch = new OcChain;
            ch->proc = c->cfg->getProcessor(t);
            ch->cpu = ch->proc->getDefaultCPUProcessor();
            ch->desc = OCIO::GpuShaderDesc::CreateShaderDesc();
            ch->desc->setLanguage(lang == 1 ? OCIO::GPU_LANGUAGE_HLSL_SM_5_0 : OCIO::GPU_LANGUAGE_MSL_2_0);
            ch->desc->setFunctionName(fn); ch->desc->setResourcePrefix(prefix);
            ch->proc->getDefaultGPUProcessor()->extractGpuShaderInfo(ch->desc);
            ch->shader = ch->desc->getShaderText();
            return ch;
        };
        *pre_out = mk(pre, "OCIOPre", "ocio_pre_");
        *post_out = mk(dvt, "OCIOPost", "ocio_post_");
        return scene ? 1 : 2;
    } catch (const std::exception &e) { put(err, errn, e.what()); return 0; }
}

// QCView's OCIOScope: the input colourspace → the interchange role, nothing else (the waveform reads the source through the
// Input — canon row 203). side: 1 scene (aces_interchange), 2 display (cie_xyz_d65_interchange); 0 = failed.
OcChain *oc_chain_build_scope(const OcConfig *c, const char *input, int lang, int *side, char *err, size_t errn)
{
    try {
        OCIO::ConstColorSpaceRcPtr scs = c->cfg->getColorSpace(input);
        if (!scs || scs->isData()) { put(err, errn, "no scope for a data / unknown input"); return nullptr; }
        const bool scene = scs->getReferenceSpaceType() == OCIO::REFERENCE_SPACE_SCENE;
        const char *role = scene ? "aces_interchange" : "cie_xyz_d65_interchange";
        if (!c->cfg->hasRole(role)) { put(err, errn, "the config has no interchange role"); return nullptr; }
        OCIO::ColorSpaceTransformRcPtr t = OCIO::ColorSpaceTransform::Create(); t->setSrc(input); t->setDst(c->cfg->getRoleColorSpace(role));
        auto *ch = new OcChain;
        ch->proc = c->cfg->getProcessor(t);
        ch->cpu = ch->proc->getDefaultCPUProcessor();
        ch->desc = OCIO::GpuShaderDesc::CreateShaderDesc();
        ch->desc->setLanguage(lang == 1 ? OCIO::GPU_LANGUAGE_HLSL_SM_5_0 : OCIO::GPU_LANGUAGE_MSL_2_0);
        ch->desc->setFunctionName("OCIOScope"); ch->desc->setResourcePrefix("ocio_scope_");
        ch->proc->getDefaultGPUProcessor()->extractGpuShaderInfo(ch->desc);
        ch->shader = ch->desc->getShaderText();
        *side = scene ? 1 : 2;
        return ch;
    } catch (const std::exception &e) { put(err, errn, e.what()); return nullptr; }
}

void oc_chain_free(OcChain *c) { delete c; }
const char *oc_chain_shader(const OcChain *c) { return c->shader.c_str(); }
unsigned oc_chain_num_uniforms(const OcChain *c) { return c->desc->getNumUniforms(); }
unsigned oc_chain_num_textures(const OcChain *c) { return c->desc->getNumTextures(); }
int oc_chain_texture(const OcChain *c, unsigned i, const char **name, unsigned *w, unsigned *h, int *rgb, int *dim, const float **values)
{
    const char *tn = nullptr, *sn = nullptr;
    OCIO::GpuShaderDesc::TextureType chan = OCIO::GpuShaderDesc::TEXTURE_RGB_CHANNEL;
    OCIO::GpuShaderDesc::TextureDimensions d = OCIO::GpuShaderDesc::TEXTURE_1D;
    OCIO::Interpolation interp = OCIO::INTERP_DEFAULT;
    c->desc->getTexture(i, tn, sn, *w, *h, chan, d, interp);
    const float *v = nullptr;
    c->desc->getTextureValues(i, v);
    if (!tn || !v) return 0;
    *name = tn; *rgb = chan == OCIO::GpuShaderDesc::TEXTURE_RGB_CHANNEL ? 1 : 0;
    *dim = d == OCIO::GpuShaderDesc::TEXTURE_1D ? 1 : 2; *values = v;
    return 1;
}
unsigned oc_chain_num_3d(const OcChain *c) { return c->desc->getNum3DTextures(); }
int oc_chain_texture3d(const OcChain *c, unsigned i, const char **name, unsigned *edge, const float **values)
{
    const char *tn = nullptr, *sn = nullptr;
    OCIO::Interpolation interp = OCIO::INTERP_DEFAULT;
    c->desc->get3DTexture(i, tn, sn, *edge, interp);
    const float *v = nullptr;
    c->desc->get3DTextureValues(i, v);
    if (!tn || !v) return 0;
    *name = tn; *values = v;
    return 1;
}
void oc_chain_apply_rgba(const OcChain *c, float *px, long w, long h) { OCIO::PackedImageDesc img(px, w, h, 4); c->cpu->apply(img); }

} // extern "C"
