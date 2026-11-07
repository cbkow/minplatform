// min-present — the stage functions every variant shares (canon §11.2). Appended in front of each variant body.
struct U {
  fit: vec2<f32>,        // NDC scale of the fullscreen triangle: the letterbox fit (1,1 = fill)
  yscale: f32,           // sample → code/(2^d−1): 65535/1023 for 10-bit in R16Unorm, 65535/65472 for 10-bit left-justified, 1 for 8-bit / float
  black: f32,            // luma black in code/(2^d−1): 16·2^(d−8)/(2^d−1) (0 for full range)
  lscale: f32,           // luma scale: (2^d−1)/(219·2^(d−8)) (1 for full range)
  czero: f32,            // chroma neutral: 2^(d−1)/(2^d−1)
  cscale: f32,           // chroma scale: (2^d−1)/(224·2^(d−8)) (1 for full range)
  src_transfer: f32,     // 0 sRGB, 1 BT.1886, 2 PQ, 3 HLG, 4 linear
  src_matrix: f32,       // 0 BT.601, 1 BT.709, 2 BT.2020 NCL, 3 identity (planar RGB)
  src_primaries: f32,    // 0 BT.709, 1 P3-D65, 2 BT.2020 — converted to BT.709 (the working primaries)
  out_mode: f32,         // 0 linear extended (EDR, 1.0 = white), 1 encoded passthrough (no EOTF), 2 PQ-encode, 3 linear clipped (sRGB surface), 4 sRGB-encoded
  white: f32,            // nits at 1.0
};
fn ycc_to_rgb(y: f32, cb: f32, cr: f32, m: f32) -> vec3<f32> {
  if (m < 0.5) { return vec3(y + 1.402 * cr, y - 0.344136 * cb - 0.714136 * cr, y + 1.772 * cb); }           // BT.601
  if (m < 1.5) { return vec3(y + 1.5748 * cr, y - 0.1873 * cb - 0.4681 * cr, y + 1.8556 * cb); }             // BT.709
  return vec3(y + 1.4746 * cr, y - 0.16455 * cb - 0.57135 * cr, y + 1.8814 * cb);                           // BT.2020 NCL
}
fn srgb_eotf(c: vec3<f32>) -> vec3<f32> {
  let lo = c / 12.92; let hi = pow((c + 0.055) / 1.055, vec3(2.4));
  return select(hi, lo, c <= vec3(0.04045));
}
fn pq_eotf_nits(c: vec3<f32>) -> vec3<f32> {
  let m1 = 0.1593017578125; let m2 = 78.84375; let c1 = 0.8359375; let c2 = 18.8515625; let c3 = 18.6875;
  let e = pow(max(c, vec3(0.0)), vec3(1.0 / m2));
  return pow(max(e - c1, vec3(0.0)) / (c2 - c3 * e), vec3(1.0 / m1)) * 10000.0;
}
fn pq_oetf(nits: vec3<f32>) -> vec3<f32> {
  let m1 = 0.1593017578125; let m2 = 78.84375; let c1 = 0.8359375; let c2 = 18.8515625; let c3 = 18.6875;
  let y = pow(clamp(nits / 10000.0, vec3(0.0), vec3(1.0)), vec3(m1));
  return pow((c1 + c2 * y) / (1.0 + c3 * y), vec3(m2));
}
fn hlg_inverse_oetf(e: vec3<f32>) -> vec3<f32> {   // signal → scene-linear (0..1 per channel, 12 at the nominal peak)
  let a = 0.17883277; let b = 0.28466892; let c = 0.55991073;
  let lo = e * e / 3.0; let hi = (exp((e - c) / a) + b) / 12.0;
  return select(hi, lo, e <= vec3(0.5));
}
// Source encoding → linear light with 1.0 = `white` nits, still in the source primaries.
fn eotf(rgb: vec3<f32>, trc: f32, white: f32) -> vec3<f32> {
  if (trc < 0.5) { return srgb_eotf(rgb); }
  if (trc < 1.5) { return pow(max(rgb, vec3(0.0)), vec3(2.4)); }
  if (trc < 2.5) { return pq_eotf_nits(rgb) / white; }
  if (trc < 3.5) {   // HLG: scene-linear × the OOTF (system gamma 1.2 at the nominal 1000-nit display), 1000 nits at the peak
    let s = hlg_inverse_oetf(clamp(rgb, vec3(0.0), vec3(1.0)));
    let ys = dot(s, vec3(0.2627, 0.6780, 0.0593));
    return s * pow(max(ys, 1e-6), 0.2) * 1000.0 / white;
  }
  return rgb;   // linear
}
fn to_709(lin: vec3<f32>, prim: f32) -> vec3<f32> {
  if (prim < 0.5) { return lin; }
  if (prim < 1.5) { return vec3(1.2249 * lin.r - 0.2247 * lin.g, -0.0420 * lin.r + 1.0419 * lin.g, -0.0197 * lin.r - 0.0786 * lin.g + 1.0983 * lin.b); }   // P3-D65 → BT.709
  return vec3(1.6605 * lin.r - 0.5876 * lin.g - 0.0728 * lin.b, -0.1246 * lin.r + 1.1329 * lin.g - 0.0083 * lin.b, -0.0182 * lin.r - 0.1006 * lin.g + 1.1187 * lin.b);   // BT.2020 → BT.709
}
fn srgb_oetf(c: vec3<f32>) -> vec3<f32> {
  let lo = c * 12.92; let hi = 1.055 * pow(c, vec3(1.0 / 2.4)) - 0.055;
  return select(hi, lo, c <= vec3(0.0031308));
}
// Stage 3 — the display transform. `lin` = linear BT.709, 1.0 = white nits.
//   0 EDR: extended linear · 2 PQ surface: encode · 3 SDR on an sRGB surface: linear clipped (the ROP encodes) · 4 SDR on a
//   plain 8-bit surface: sRGB-encoded here. (1 = encoded passthrough is decided before stage 1 and never reaches here.)
fn stage3(lin: vec3<f32>, mode: f32, white: f32) -> vec4<f32> {
  if (mode < 0.5) { return vec4(max(lin, vec3(0.0)), 1.0); }
  if (mode < 2.5) { return vec4(pq_oetf(max(lin, vec3(0.0)) * white), 1.0); }
  if (mode < 3.5) { return vec4(clamp(lin, vec3(0.0), vec3(1.0)), 1.0); }
  return vec4(srgb_oetf(clamp(lin, vec3(0.0), vec3(1.0))), 1.0);
}
struct VsOut { @builtin(position) pos: vec4<f32>, @location(0) uv: vec2<f32> };
fn fullscreen(i: u32, fit: vec2<f32>) -> VsOut {
  var p = array<vec2<f32>, 3>(vec2(-1.0, -1.0), vec2(3.0, -1.0), vec2(-1.0, 3.0));
  var o: VsOut; o.pos = vec4(p[i] * fit, 0.0, 1.0); o.uv = vec2((p[i].x + 1.0) * 0.5, 1.0 - (p[i].y + 1.0) * 0.5); return o;
}
