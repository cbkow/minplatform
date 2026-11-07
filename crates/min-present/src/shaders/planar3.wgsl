// Three planes: Y, Cb, Cr — or G, B, R for planar RGB. Chroma sampled bilinearly at the luma pixel centre (QCView).
@group(0) @binding(0) var samp: sampler;
@group(0) @binding(1) var t0: texture_2d<f32>;
@group(0) @binding(2) var t1: texture_2d<f32>;
@group(0) @binding(3) var t2: texture_2d<f32>;
@group(0) @binding(4) var<uniform> u: U;
@vertex fn vs(@builtin(vertex_index) i: u32) -> VsOut { return fullscreen(i, u.fit); }
@fragment fn fs(in: VsOut) -> @location(0) vec4<f32> {
  if (u.is_rgb > 0.5) { return finish(rgb_levels(vec3(textureSample(t2, samp, in.uv).r, textureSample(t0, samp, in.uv).r, textureSample(t1, samp, in.uv).r))); }
  let yc = textureSample(t0, samp, in.uv).r * u.code_max;
  let uc = textureSample(t1, samp, in.uv).r * u.code_max;
  let vc = textureSample(t2, samp, in.uv).r * u.code_max;
  return finish(ycc_to_rgb(levels(yc, uc, vc)));
}
