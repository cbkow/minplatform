// The post-OCIO (display-encoded) image → the extended-linear surface: fit + the display EOTF (0 sRGB, 1 BT.1886, 2 PQ, 3 none).
struct U { fit: vec2<f32>, eotf: f32, p0: f32 };
@group(0) @binding(0) var samp: sampler;
@group(0) @binding(1) var tex: texture_2d<f32>;
@group(0) @binding(2) var<uniform> u: U;
struct VsOut { @builtin(position) pos: vec4<f32>, @location(0) uv: vec2<f32> };
@vertex fn vs(@builtin(vertex_index) i: u32) -> VsOut {
  var p = array<vec2<f32>, 3>(vec2(-1.0, -1.0), vec2(3.0, -1.0), vec2(-1.0, 3.0));
  var o: VsOut; o.pos = vec4(p[i] * u.fit, 0.0, 1.0); o.uv = vec2((p[i].x + 1.0) * 0.5, 1.0 - (p[i].y + 1.0) * 0.5); return o;
}
fn srgb_eotf(c: vec3<f32>) -> vec3<f32> {
  let lo = c / 12.92; let hi = pow((c + 0.055) / 1.055, vec3(2.4));
  return select(hi, lo, c <= vec3(0.04045));
}
fn pq_eotf(c: vec3<f32>) -> vec3<f32> {
  let m1 = 0.1593017578125; let m2 = 78.84375; let c1 = 0.8359375; let c2 = 18.8515625; let c3 = 18.6875;
  let e = pow(c, vec3(1.0 / m2));
  let n = max(e - c1, vec3(0.0)) / (c2 - c3 * e);
  return pow(n, vec3(1.0 / m1)) * 10000.0 / 100.0;   // nits → SDR-white units (100 nit reference)
}
@fragment fn fs(in: VsOut) -> @location(0) vec4<f32> {
  let c = clamp(textureSample(tex, samp, in.uv).rgb, vec3(0.0), vec3(1.0));
  var lin = c;
  if (u.eotf < 0.5) { lin = srgb_eotf(c); } else if (u.eotf < 1.5) { lin = pow(c, vec3(2.4)); } else if (u.eotf < 2.5) { lin = pq_eotf(c); }
  return vec4(lin, 1.0);
}
