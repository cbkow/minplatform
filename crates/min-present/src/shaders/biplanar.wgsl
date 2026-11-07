// Y + interleaved CbCr (VideoToolbox / D3D12VA / NV12 / P010).
@group(0) @binding(0) var samp: sampler;
@group(0) @binding(1) var ty: texture_2d<f32>;
@group(0) @binding(2) var tuv: texture_2d<f32>;
@group(0) @binding(3) var<uniform> u: U;
@vertex fn vs(@builtin(vertex_index) i: u32) -> VsOut { return fullscreen(i, u.fit); }
@fragment fn fs(in: VsOut) -> @location(0) vec4<f32> {
  let yc = textureSample(ty, samp, in.uv).r * u.code_max;
  let c = textureSample(tuv, samp, in.uv).rg * u.code_max;
  return finish(ycc_to_rgb(levels(yc, c.x, c.y)));
}
