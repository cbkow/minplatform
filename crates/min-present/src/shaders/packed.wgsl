// Y416 / Y408 layout (VideoToolbox 'y416', ProRes 4444 alpha): A, Y, Cb, Cr packed in one RGBA16Unorm texture (QCView's
// yuv_interleaved_to_rgba). 16-bit video-range levels: Y 4096..60160, C 4096..61440, mid 32768.
@group(0) @binding(0) var samp: sampler;
@group(0) @binding(1) var tex: texture_2d<f32>;
@group(0) @binding(2) var<uniform> u: U;
@vertex fn vs(@builtin(vertex_index) i: u32) -> VsOut { return fullscreen(i, u.fit, u.off); }
@fragment fn fs(in: VsOut) -> @location(0) vec4<f32> {
  let s = textureSample(tex, samp, in.uv);
  let o = finish(ycc_to_rgb(levels(s.g * u.code_max, s.b * u.code_max, s.a * u.code_max)));
  return vec4(o.rgb * s.r, s.r);   // premultiplied: the pipeline blends One / OneMinusSrcAlpha over the background (row 153)
}
