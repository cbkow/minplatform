// S1 step 2 — a C ABI over OpenColorIO 2.5 for the spike: build ONE display chain (input cs → display/view),
// expose its generated GPU shader (MSL 2.0 / HLSL SM5) + LUT inventory, the CPU processor (ground truth)
// and a 3D-LUT bake of the same chain. Dies with the spike (the real binding is `ocio-sys`, canon §18 row 94).
#include <OpenColorIO/OpenColorIO.h>
#include <cstdio>
#include <cstring>
#include <string>
#include <vector>
namespace OCIO = OCIO_NAMESPACE;

struct OcioChain {
    OCIO::ConstConfigRcPtr       cfg;
    OCIO::ConstProcessorRcPtr    proc;
    OCIO::ConstCPUProcessorRcPtr cpu;
    OCIO::GpuShaderDescRcPtr     desc;
    std::string                  shader;
};

extern "C" {

const char *ocio_version() { return OCIO::GetVersion(); }

OcioChain *ocio_chain_open(const char *config, const char *input_cs, const char *display, const char *view,
                           int lang, const char *fn_name, char *err, size_t err_len)
{
    auto *c = new OcioChain;
    try {
        c->cfg = OCIO::Config::CreateFromFile(config);
        OCIO::DisplayViewTransformRcPtr dvt = OCIO::DisplayViewTransform::Create();
        dvt->setSrc(input_cs);
        dvt->setDisplay(display);
        dvt->setView(view);
        c->proc = c->cfg->getProcessor(dvt);
        c->cpu  = c->proc->getDefaultCPUProcessor();
        OCIO::ConstGPUProcessorRcPtr gpu = c->proc->getDefaultGPUProcessor();
        c->desc = OCIO::GpuShaderDesc::CreateShaderDesc();
        c->desc->setLanguage(lang == 1 ? OCIO::GPU_LANGUAGE_HLSL_SM_5_0 : OCIO::GPU_LANGUAGE_MSL_2_0);
        c->desc->setFunctionName(fn_name);
        c->desc->setResourcePrefix("ocio");
        gpu->extractGpuShaderInfo(c->desc);
        c->shader = c->desc->getShaderText();
        return c;
    } catch (const std::exception &e) {
        snprintf(err, err_len, "%s", e.what());
        delete c;
        return nullptr;
    }
}

void ocio_chain_close(OcioChain *c) { delete c; }
const char *ocio_chain_shader(const OcioChain *c) { return c->shader.c_str(); }
unsigned ocio_chain_num_uniforms(const OcioChain *c) { return c->desc->getNumUniforms(); }
unsigned ocio_chain_num_textures(const OcioChain *c) { return c->desc->getNumTextures(); }

// dim: 1 or 2; rgb: 1 = RGB (3 floats/texel), 0 = red only.
int ocio_chain_texture(const OcioChain *c, unsigned i, const char **name, unsigned *w, unsigned *h,
                       int *rgb, int *dim, const float **values)
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

unsigned ocio_chain_num_3d(const OcioChain *c) { return c->desc->getNum3DTextures(); }
int ocio_chain_texture3d(const OcioChain *c, unsigned i, const char **name, unsigned *edge, const float **values)
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

// Ground truth: the CPU processor over packed RGBA f32, in place.
void ocio_chain_apply_rgba(const OcioChain *c, float *px, long w, long h)
{
    OCIO::PackedImageDesc img(px, w, h, 4);
    c->cpu->apply(img);
}

// Route B's data: the chain sampled on an edge³ lattice over [0,1]³ (x = R fastest, then G, then B); RGBA f32 out.
void ocio_chain_bake3d(const OcioChain *c, int edge, float *out)
{
    const long n = (long)edge * edge * edge;
    for (int b = 0; b < edge; ++b)
        for (int g = 0; g < edge; ++g)
            for (int r = 0; r < edge; ++r) {
                float *p = out + 4 * ((long)r + (long)edge * ((long)g + (long)edge * b));
                p[0] = (float)r / (edge - 1); p[1] = (float)g / (edge - 1); p[2] = (float)b / (edge - 1); p[3] = 1.0f;
            }
    OCIO::PackedImageDesc img(out, n, 1, 4);
    c->cpu->apply(img);
}

} // extern "C"
