// min-present — the stage functions every variant shares (canon §11.2). Stage 1 is QCView's `yuv_planar_to_rgba`
// (metal_yuv_renderer.mm): levels in CODE values for any depth / alignment, the matrix from Kr / Kb, UNCLAMPED.
struct U {
  fit: vec2<f32>,        // NDC scale of the fullscreen triangle: the letterbox fit (1,1 = fill)
  code_max: f32,         // sample (0..1 UNORM) × code_max = the stored code (255 or 65535; 1 for float RGB)
  level_k: f32,          // 2^(code_bits − 8)
  full_max: f32,         // 2^code_bits − 1
  chroma_mid: f32,       // 2^(code_bits − 1)
  full_range: f32,       // 1 = full range, 0 = video (limited) range
  kr: f32,
  kb: f32,
  src_transfer: f32,     // 0 sRGB, 1 BT.1886, 2 PQ, 3 HLG, 4 linear
  src_primaries: f32,    // 0 BT.709, 1 P3-D65, 2 BT.2020 — converted to BT.709 (the working primaries)
  out_mode: f32,         // 0 linear extended (EDR), 1 encoded passthrough, 2 PQ-encode, 3 linear clipped (sRGB surface), 4 sRGB-encoded
  white: f32,            // nits at 1.0
  is_rgb: f32,           // planar RGB (G, B, R planes): no matrix
  rgb_expand: f32,       // legal-range RGB → full (rgb_range.h's rule)
  rot: f32,              // display rotation, quarter turns CW; the sampler inverse-rotates (QCView's rotatedSrcUv)
  off: vec2<f32>,        // NDC offset of the fit: the content rect inside the viewport (stage 4's bands take the rest)
  pad1: vec2<f32>,       // x = the extended-linear output's gain (Windows scRGB: SDR white ÷ 80 nits; 0 = none), y —
  bg: vec4<f32>,         // stage 0 (background.wgsl): mode, bounds-mix strength, checker tile px, —
  media: vec4<f32>,      // the media rect in viewport px (x, y, w, h; y down)
  vp: vec4<f32>,         // the viewport size in px
  aids: vec4<f32>,       // the viewer aids (screen pass only): gain, 1/gamma, channel (0 RGB 1 R 2 G 3 B 4 A 5 Y), active
};
// Levels in code values (QCView yuv_planar.h): limited Y = (code − 16K)/(219K), C = (code − 128K)/(224K); full Y = code/fullMax, C = (code − mid)/fullMax.
fn levels(yc: f32, uc: f32, vc: f32) -> vec3<f32> {
  if (u.full_range > 0.5) { return vec3(yc / u.full_max, (uc - u.chroma_mid) / u.full_max, (vc - u.chroma_mid) / u.full_max); }
  return vec3((yc - 16.0 * u.level_k) / (219.0 * u.level_k), (uc - 128.0 * u.level_k) / (224.0 * u.level_k), (vc - 128.0 * u.level_k) / (224.0 * u.level_k));
}
// The matrix from Kr / Kb (601 / 709 / 2020 / 240M / FCC all decode as swscale did). UNCLAMPED: super-whites, sub-blacks
// and out-of-gamut triplets ride into the colour chain; stage 3's SDR modes clamp for the display.
fn ycc_to_rgb(ycc: vec3<f32>) -> vec3<f32> {
  let r = ycc.x + 2.0 * (1.0 - u.kr) * ycc.z;
  let b = ycc.x + 2.0 * (1.0 - u.kb) * ycc.y;
  let g = (ycc.x - u.kr * r - u.kb * b) / (1.0 - u.kr - u.kb);
  return vec3(r, g, b);
}
fn rgb_levels(c: vec3<f32>) -> vec3<f32> {   // planar RGB codes → 0..1 (legal expansion when asked); float RGB has code_max = full_max = 1
  if (u.rgb_expand > 0.5) { return (c * u.code_max - 16.0 * u.level_k) / (219.0 * u.level_k); }
  return c * u.code_max / u.full_max;
}
fn srgb_eotf(c: vec3<f32>) -> vec3<f32> {
  let lo = c / 12.92; let hi = pow((c + 0.055) / 1.055, vec3(2.4));
  return select(hi, lo, c <= vec3(0.04045));
}
fn srgb_oetf(c: vec3<f32>) -> vec3<f32> {
  let lo = c * 12.92; let hi = 1.055 * pow(c, vec3(1.0 / 2.4)) - 0.055;
  return select(hi, lo, c <= vec3(0.0031308));
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
fn hlg_inverse_oetf(e: vec3<f32>) -> vec3<f32> {
  let a = 0.17883277; let b = 0.28466892; let c = 0.55991073;
  let lo = e * e / 3.0; let hi = (exp((e - c) / a) + b) / 12.0;
  return select(hi, lo, e <= vec3(0.5));
}
// Source encoding → linear light with 1.0 = `white` nits, still in the source primaries. Encoded values above 1.0 (super-whites)
// pass through the power curves unclamped where the maths allows; PQ / HLG clamp their domain.
fn eotf(rgb: vec3<f32>, trc: f32, white: f32) -> vec3<f32> {
  if (trc < 0.5) { return sign(rgb) * srgb_eotf(abs(rgb)); }
  if (trc < 1.5) { return sign(rgb) * pow(abs(rgb), vec3(2.4)); }
  if (trc < 2.5) { return pq_eotf_nits(clamp(rgb, vec3(0.0), vec3(1.0))) / white; }
  if (trc < 3.5) { let s = hlg_inverse_oetf(clamp(rgb, vec3(0.0), vec3(1.0))); let ys = dot(s, vec3(0.2627, 0.6780, 0.0593)); return s * pow(max(ys, 1e-6), 0.2) * 1000.0 / white; }
  return rgb;
}
fn to_709(lin: vec3<f32>, prim: f32) -> vec3<f32> {
  if (prim < 0.5) { return lin; }
  if (prim < 1.5) { return vec3(1.2249 * lin.r - 0.2247 * lin.g, -0.0420 * lin.r + 1.0419 * lin.g, -0.0197 * lin.r - 0.0786 * lin.g + 1.0983 * lin.b); }
  return vec3(1.6605 * lin.r - 0.5876 * lin.g - 0.0728 * lin.b, -0.1246 * lin.r + 1.1329 * lin.g - 0.0083 * lin.b, -0.0182 * lin.r - 0.1006 * lin.g + 1.1187 * lin.b);
}
// QCView's viewer aids on the linear output (linear_stage.h applyViewer, `Linear` encoding): gain, c^(1/γ) sign-kept, the
// channel view (alpha is the callers': they know it). Inspection only — the screen's uniform set carries them, no other.
fn viewer_aids(lin: vec3<f32>) -> vec3<f32> {
  if (u.aids.w < 0.5) { return lin; }
  var c = lin * u.aids.x;
  if (abs(u.aids.y - 1.0) > 1e-5) { c = sign(c) * pow(abs(c), vec3(u.aids.y)); }
  let ch = u.aids.z;
  if (ch > 0.5 && ch < 1.5) { return vec3(c.r); }
  if (ch > 1.5 && ch < 2.5) { return vec3(c.g); }
  if (ch > 2.5 && ch < 3.5) { return vec3(c.b); }
  if (ch > 4.5) { return vec3(dot(c, vec3(0.2126, 0.7152, 0.0722))); }
  return c;
}
fn alpha_view() -> bool { return u.aids.w > 0.5 && u.aids.z > 3.5 && u.aids.z < 4.5; }
// Stage 3 — the display transform. 0 EDR linear · 2 PQ encode · 3 SDR linear clipped (sRGB surface) · 4 SDR sRGB-encoded.
fn stage3(lin_in: vec3<f32>, mode: f32, white: f32) -> vec4<f32> {
  let lin = viewer_aids(lin_in);
  // the extended-linear surface: on Windows scRGB 1.0 is 80 nits, so SDR white (1.0 here) is lifted to the user's SDR brightness
  if (mode < 0.5) { return vec4(max(lin, vec3(0.0)) * select(1.0, u.pad1.x, u.pad1.x > 0.0), 1.0); }
  if (mode < 2.5) { return vec4(pq_oetf(max(lin, vec3(0.0)) * white), 1.0); }
  if (mode < 3.5) { return vec4(clamp(lin, vec3(0.0), vec3(1.0)), 1.0); }
  return vec4(srgb_oetf(clamp(lin, vec3(0.0), vec3(1.0))), 1.0);
}
// Stage 1's tail: encoded passthrough (mode 1, unclamped) or stages 1b + 3.
fn finish(rgb: vec3<f32>) -> vec4<f32> {
  if (u.out_mode > 0.5 && u.out_mode < 1.5) { return vec4(rgb, 1.0); }
  return stage3(to_709(eotf(rgb, u.src_transfer, u.white), u.src_primaries), u.out_mode, u.white);
}
struct VsOut { @builtin(position) pos: vec4<f32>, @location(0) uv: vec2<f32> };
// The picture as a QUAD (two triangles, 6 vertices), not the fullscreen triangle: once the fit scales the triangle
// below the viewport its overhang lands INSIDE the window and smears the texture's edge diagonally into the
// letterbox (seen 2026-11-07 at odd window aspects). A quad covers exactly the fit rect.
fn fullscreen(i: u32, fit: vec2<f32>, off: vec2<f32>) -> VsOut {
  var p = array<vec2<f32>, 6>(vec2(-1.0, -1.0), vec2(1.0, -1.0), vec2(-1.0, 1.0), vec2(1.0, -1.0), vec2(1.0, 1.0), vec2(-1.0, 1.0));
  var o: VsOut; o.pos = vec4(p[i] * fit + off, 0.0, 1.0);
  let d = vec2((p[i].x + 1.0) * 0.5, 1.0 - (p[i].y + 1.0) * 0.5);   // display-orientation uv
  o.uv = rotated_src_uv(d, u.rot);
  return o;
}
// Display → stored uv for the quarter-turn rotation (QCView `rotatedSrcUv`): the fit was computed from swapped dims
// for odd quarters, so the inverse rotation here lands back on the stored texture upright.
fn rotated_src_uv(p: vec2<f32>, rot: f32) -> vec2<f32> {
  if (rot > 0.5 && rot < 1.5) { return vec2(p.y, 1.0 - p.x); }
  if (rot > 1.5 && rot < 2.5) { return vec2(1.0 - p.x, 1.0 - p.y); }
  if (rot > 2.5) { return vec2(1.0 - p.y, p.x); }
  return p;
}
