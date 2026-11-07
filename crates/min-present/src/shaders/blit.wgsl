// The post-chain blit: the working texture (linear, or encoded with `src_transfer`) → stage 3 with the fit.
@group(0) @binding(0) var samp: sampler;
@group(0) @binding(1) var tex: texture_2d<f32>;
@group(0) @binding(2) var<uniform> u: U;
@vertex fn vs(@builtin(vertex_index) i: u32) -> VsOut { return fullscreen(i, u.fit, u.off); }
@fragment fn fs(in: VsOut) -> @location(0) vec4<f32> {
  let c = textureSample(tex, samp, in.uv).rgb;
  var lin = c;
  if (u.src_transfer < 3.5) { lin = eotf(c, u.src_transfer, u.white); }
  return stage3(lin, u.out_mode, u.white);
}
