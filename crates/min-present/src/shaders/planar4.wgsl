// Four planes: Y, Cb, Cr, A — or G, B, R, A. Alpha rides to the working texture (stage 4 composites it); the surface ignores it.
@group(0) @binding(0) var samp: sampler;
@group(0) @binding(1) var t0: texture_2d<f32>;
@group(0) @binding(2) var t1: texture_2d<f32>;
@group(0) @binding(3) var t2: texture_2d<f32>;
@group(0) @binding(4) var t3: texture_2d<f32>;
@group(0) @binding(5) var<uniform> u: U;
@vertex fn vs(@builtin(vertex_index) i: u32) -> VsOut { return fullscreen(i, u.fit); }
@fragment fn fs(in: VsOut) -> @location(0) vec4<f32> {
  let a = textureSample(t3, samp, in.uv).r * u.code_max / u.full_max;
  var o: vec4<f32>;
  if (u.is_rgb > 0.5) { o = finish(rgb_levels(vec3(textureSample(t2, samp, in.uv).r, textureSample(t0, samp, in.uv).r, textureSample(t1, samp, in.uv).r))); }
  else {
    let yc = textureSample(t0, samp, in.uv).r * u.code_max;
    let uc = textureSample(t1, samp, in.uv).r * u.code_max;
    let vc = textureSample(t2, samp, in.uv).r * u.code_max;
    o = finish(ycc_to_rgb(levels(yc, uc, vc)));
  }
  return vec4(o.rgb, a);
}
