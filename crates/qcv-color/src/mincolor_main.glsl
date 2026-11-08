// QCView's minColor kernel main (metal_ocio_renderer.mm kMinColorMain), as GLSL for naga: Input → Knee → AgX → Output,
// the flags' output scale. The blocks are read-only storage (std430): the core's structs carry scalar arrays, which a
// uniform block would pad to 16 bytes each; the viewer aids are NOT here (they run in the presenter's screen pass, canon row 204).
layout(std430, set = 0, binding = 0) readonly buffer McIn { DrtParams pIn; };
layout(std430, set = 0, binding = 1) readonly buffer McOut { DrtParams pOut; };
layout(std430, set = 0, binding = 2) readonly buffer McAgx { float agxRaw[DRT_AGX_SCALARS]; };
// build.rs puts `DrtAgxParams qcv_agx()` here, filled field by field from agxRaw (generated from mincolor_agx.h):
// a struct with scalar arrays cannot be read straight out of a buffer and passed by value in naga's GLSL.
// @AGX_LOADER@
layout(std430, set = 0, binding = 3) readonly buffer McFlags { int kneeOn; int agxOn; float outScale; int flagsPad; };
layout(set = 0, binding = 4) uniform texture2D srcTex;
layout(set = 0, binding = 5) uniform sampler srcSmp;
layout(set = 0, binding = 6, rgba16f) uniform writeonly image2D dst;
// the waveform's tap (canon row 203, QCView's scope tiers): the source through the Input — linear Rec.2020, 1.0 = 100 nits —
// written when flagsPad = 1 (a 1×1 stand-in is bound otherwise)
layout(set = 0, binding = 7, rgba16f) uniform writeonly image2D scopeTap;
layout(local_size_x = 8, local_size_y = 8) in;
void main() {
    ivec2 g = ivec2(gl_GlobalInvocationID.xy);
    ivec2 size = imageSize(dst);
    if (g.x >= size.x || g.y >= size.y) { return; }
    vec4 color = texelFetch(sampler2D(srcTex, srcSmp), g, 0);
    vec3 c = color.rgb;
    c = drt_input_transform(pIn, c);
    if (flagsPad != 0) { imageStore(scopeTap, g, vec4(c, 1.0)); }
    if (kneeOn != 0) { c = drt_knee(pIn, c); }
    if (agxOn != 0) { DrtAgxParams agx = qcv_agx(); c = drt_agx(agx, c); }
    c = drt_transform(pOut, c);
    imageStore(dst, g, vec4(c * outScale, color.a));
}
