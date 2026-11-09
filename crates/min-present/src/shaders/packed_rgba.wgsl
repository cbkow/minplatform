// One packed RGBA half-float plane — a QCBridge host frame (canon row 228): the host's working-space values exactly as sent
// (unclamped; no levels, no matrix); `u.rgb_expand` = the frame is premultiplied, else straight alpha (premultiplied here).
@group(0) @binding(0) var samp: sampler;
@group(0) @binding(1) var tex: texture_2d<f32>;
@group(0) @binding(2) var<uniform> u: U;
@vertex fn vs(@builtin(vertex_index) i: u32) -> VsOut { return fullscreen(i, u.fit, u.off); }
@fragment fn fs(in: VsOut) -> @location(0) vec4<f32> {
  let s = textureSample(tex, samp, in.uv);
  let a = clamp(s.a, 0.0, 1.0);
  var rgb = s.rgb;
  if (u.rgb_expand > 0.5 && a > 0.0) { rgb = rgb / a; }   // premultiplied in → straight for the display transform
  if (alpha_view()) { return vec4(vec3(a), 1.0); }
  let o = finish(rgb);
  return vec4(o.rgb * a, a);   // premultiplied out, blended over the background (row 153)
}
