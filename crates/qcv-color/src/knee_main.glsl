// The Highlight Knee as its own pass (the OCIO split's middle; canon row 204): QCView's linear stage — the interchange
// space → linear Rec.2020 (QCView's fixed matrices), the gain, the vendored core's drt_knee (QCView's maths: minColorAE
// ported it from linear_stage.h), back. Only the shim, the params and the knee are compiled in, so it stays small.
layout(std430, set = 0, binding = 0) readonly buffer KneeP { DrtParams kp; };
layout(std430, set = 0, binding = 1) readonly buffer KneeM { float to2020[12]; float from2020[12]; float gain; float kneeOn; float kpad0; float kpad1; };
layout(set = 0, binding = 2) uniform texture2D srcTex;
layout(set = 0, binding = 3) uniform sampler srcSmp;
layout(set = 0, binding = 4, rgba16f) uniform writeonly image2D dst;
layout(local_size_x = 8, local_size_y = 8) in;
vec3 rowmul(float m[12], vec3 c) { return vec3(m[0]*c.x + m[1]*c.y + m[2]*c.z, m[4]*c.x + m[5]*c.y + m[6]*c.z, m[8]*c.x + m[9]*c.y + m[10]*c.z); }
void main() {
    ivec2 g = ivec2(gl_GlobalInvocationID.xy);
    ivec2 size = imageSize(dst);
    if (g.x >= size.x || g.y >= size.y) { return; }
    vec4 color = texelFetch(sampler2D(srcTex, srcSmp), g, 0);
    vec3 r = rowmul(to2020, color.rgb) * gain;
    if (kneeOn != 0.0) { r = drt_knee(kp, r); }
    imageStore(dst, g, vec4(rowmul(from2020, r), color.a));
}
