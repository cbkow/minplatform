// Biplanar (VideoToolbox / D3D12VA): Y + interleaved CbCr → stage 1 → stage 3, fused.
@group(0) @binding(0) var samp: sampler;
@group(0) @binding(1) var ty: texture_2d<f32>;
@group(0) @binding(2) var tuv: texture_2d<f32>;
@group(0) @binding(3) var<uniform> u: U;
@vertex fn vs(@builtin(vertex_index) i: u32) -> VsOut { return fullscreen(i, u.fit); }
@fragment fn fs(in: VsOut) -> @location(0) vec4<f32> {
  let y = (textureSample(ty, samp, in.uv).r * u.yscale - u.black) * u.lscale;
  let c = (textureSample(tuv, samp, in.uv).rg * u.yscale - vec2(u.czero)) * u.cscale;
  let cb = c.x; let cr = c.y;
  let rgb = clamp(ycc_to_rgb(y, cb, cr, u.src_matrix), vec3(0.0), vec3(1.0));
  if (u.out_mode > 0.5 && u.out_mode < 1.5) { return vec4(rgb, 1.0); }
  return stage3(to_709(eotf(rgb, u.src_transfer, u.white), u.src_primaries), u.out_mode, u.white);
}
