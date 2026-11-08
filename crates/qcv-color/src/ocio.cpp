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
