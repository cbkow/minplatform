// Dual view's Difference mode (QCView's compositor): per pixel |A − B| on the display-encoded values (sRGB curve, where a
// change reads as it looks), × gain, back to linear light for the extended-linear surface (× the output's SDR-white gain).
// Matching areas are black at every gain; the sides arrive already fitted to the same content rect.
struct D { rect: vec4<f32>, vp: vec4<f32>, p: vec4<f32> };   // rect px (x, y, w, h); viewport px; x = gain, y = output gain
@group(0) @binding(0) var<uniform> d: D;
@group(0) @binding(1) var smp: sampler;
@group(0) @binding(2) var ta: texture_2d<f32>;
@group(0) @binding(3) var tb: texture_2d<f32>;
struct V { @builtin(position) pos: vec4<f32>, @location(0) uv: vec2<f32> };
@vertex fn vs(@builtin(vertex_index) i: u32) -> V {
  let q = array<vec2<f32>, 6>(vec2(0.0, 0.0), vec2(1.0, 0.0), vec2(0.0, 1.0), vec2(0.0, 1.0), vec2(1.0, 0.0), vec2(1.0, 1.0));
  let uv = q[i]; let px = d.rect.xy + uv * d.rect.zw;
  var o: V; o.pos = vec4(px.x / d.vp.x * 2.0 - 1.0, 1.0 - px.y / d.vp.y * 2.0, 0.0, 1.0); o.uv = uv; return o;
}
fn oetf(c: vec3<f32>) -> vec3<f32> { let x = clamp(c, vec3(0.0), vec3(1.0)); return select(1.055 * pow(x, vec3(1.0 / 2.4)) - 0.055, 12.92 * x, x <= vec3(0.0031308)); }
fn eotf(c: vec3<f32>) -> vec3<f32> { let x = clamp(c, vec3(0.0), vec3(1.0)); return select(pow((x + 0.055) / 1.055, vec3(2.4)), x / 12.92, x <= vec3(0.04045)); }
@fragment fn fs(v: V) -> @location(0) vec4<f32> {
  let a = textureSample(ta, smp, v.uv); let b = textureSample(tb, smp, v.uv);
  let e = abs(oetf(a.rgb) - oetf(b.rgb)) * d.p.x;
  return vec4(eotf(e) * select(1.0, d.p.y, d.p.y > 0.0), 1.0);
}
