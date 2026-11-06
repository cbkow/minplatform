// Y'CbCr planes → BT.709 R'G'B' → linear light (BT.1886) on an extended-linear surface (1.0 = SDR white).
struct U { fit: vec2<f32>, yscale: f32, full_range: f32, mode: f32, trc: f32, mat: f32, white: f32 };   // mode 1 = encoded R'G'B' out; trc 1 = PQ (else BT.1886); mat 1 = BT.2020 (else BT.709); white = nits at 1.0
@group(0) @binding(0) var samp: sampler;
@group(0) @binding(1) var ty: texture_2d<f32>;
@group(0) @binding(2) var tu: texture_2d<f32>;
@group(0) @binding(3) var tv: texture_2d<f32>;
@group(0) @binding(4) var<uniform> u: U;
fn ycc_to_rgb(y: f32, cb: f32, cr: f32, mat: f32) -> vec3<f32> {
  if (mat > 0.5) { return vec3(y + 1.4746 * cr, y - 0.16455 * cb - 0.57135 * cr, y + 1.8814 * cb); }   // BT.2020 NCL
  return vec3(y + 1.5748 * cr, y - 0.1873 * cb - 0.4681 * cr, y + 1.8556 * cb);                        // BT.709
}
fn pq_eotf(c: vec3<f32>) -> vec3<f32> {   // → nits
  let m1 = 0.1593017578125; let m2 = 78.84375; let c1 = 0.8359375; let c2 = 18.8515625; let c3 = 18.6875;
  let e = pow(c, vec3(1.0 / m2));
  return pow(max(e - c1, vec3(0.0)) / (c2 - c3 * e), vec3(1.0 / m1)) * 10000.0;
}
fn to_linear(rgb: vec3<f32>, trc: f32, mat: f32, white: f32) -> vec3<f32> {   // extended-linear, 1.0 = `white` nits, Rec.709 primaries
  var lin: vec3<f32>;
  if (trc > 0.5) { lin = pq_eotf(rgb) / white; } else { lin = pow(rgb, vec3(2.4)); }
  if (mat > 0.5) { lin = vec3(1.6605 * lin.r - 0.5876 * lin.g - 0.0728 * lin.b, -0.1246 * lin.r + 1.1329 * lin.g - 0.0083 * lin.b, -0.0182 * lin.r - 0.1006 * lin.g + 1.1187 * lin.b); }   // BT.2020 → BT.709 primaries
  return lin;
}
struct VsOut { @builtin(position) pos: vec4<f32>, @location(0) uv: vec2<f32> };
@vertex fn vs(@builtin(vertex_index) i: u32) -> VsOut {
  var p = array<vec2<f32>, 3>(vec2(-1.0, -1.0), vec2(3.0, -1.0), vec2(-1.0, 3.0));
  var o: VsOut; o.pos = vec4(p[i] * u.fit, 0.0, 1.0); o.uv = vec2((p[i].x + 1.0) * 0.5, 1.0 - (p[i].y + 1.0) * 0.5); return o;
}
@fragment fn fs(in: VsOut) -> @location(0) vec4<f32> {
  if (u.mat > 1.5) {   // planar RGB half (EXR, FFmpeg gbrp order G, B, R), linear light already
    let lin = vec3(textureSample(tv, samp, in.uv).r, textureSample(ty, samp, in.uv).r, textureSample(tu, samp, in.uv).r);
    if (u.mode > 0.5) { return vec4(clamp(lin, vec3(0.0), vec3(1.0)), 1.0); }
    return vec4(max(lin, vec3(0.0)), 1.0);
  }
  var y = textureSample(ty, samp, in.uv).r * u.yscale;
  var cb = textureSample(tu, samp, in.uv).r * u.yscale - 0.5;
  var cr = textureSample(tv, samp, in.uv).r * u.yscale - 0.5;
  if (u.full_range < 0.5) { y = (y - 16.0 / 255.0) * (255.0 / 219.0); cb = cb * (255.0 / 224.0); cr = cr * (255.0 / 224.0); }
  let rgb = clamp(ycc_to_rgb(y, cb, cr, u.mat), vec3(0.0), vec3(1.0));
  if (u.mode > 0.5) { return vec4(rgb, 1.0); }
  return vec4(to_linear(rgb, u.trc, u.mat, u.white), 1.0);
}
