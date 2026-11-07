// Three planes (Y, Cb, Cr — or G, B, R half floats for planar RGB) → stage 1 → stage 3, fused.
@group(0) @binding(0) var samp: sampler;
@group(0) @binding(1) var t0: texture_2d<f32>;
@group(0) @binding(2) var t1: texture_2d<f32>;
@group(0) @binding(3) var t2: texture_2d<f32>;
@group(0) @binding(4) var<uniform> u: U;
@vertex fn vs(@builtin(vertex_index) i: u32) -> VsOut { return fullscreen(i, u.fit); }
@fragment fn fs(in: VsOut) -> @location(0) vec4<f32> {
  if (u.src_matrix > 2.5) {   // planar RGB (FFmpeg gbrp order: G, B, R), the transfer says what it is (EXR: linear)
    let rgb = vec3(textureSample(t2, samp, in.uv).r, textureSample(t0, samp, in.uv).r, textureSample(t1, samp, in.uv).r);
    if (u.out_mode > 0.5 && u.out_mode < 1.5) { return vec4(clamp(rgb, vec3(0.0), vec3(1.0)), 1.0); }
    return stage3(to_709(eotf(rgb, u.src_transfer, u.white), u.src_primaries), u.out_mode, u.white);
  }
  let y = (textureSample(t0, samp, in.uv).r * u.yscale - u.black) * u.lscale;
  let cb = (textureSample(t1, samp, in.uv).r * u.yscale - u.czero) * u.cscale;
  let cr = (textureSample(t2, samp, in.uv).r * u.yscale - u.czero) * u.cscale;
  let rgb = clamp(ycc_to_rgb(y, cb, cr, u.src_matrix), vec3(0.0), vec3(1.0));
  if (u.out_mode > 0.5 && u.out_mode < 1.5) { return vec4(rgb, 1.0); }   // encoded passthrough
  return stage3(to_709(eotf(rgb, u.src_transfer, u.white), u.src_primaries), u.out_mode, u.white);
}
