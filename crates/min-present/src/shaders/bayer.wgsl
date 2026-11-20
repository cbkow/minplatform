// One 16-bit plane holding a camera's raw MOSAIC (ProRes RAW through FFmpeg: bayer_rggb16le) — red at even x / even y,
// blue at odd / odd, green at the other two. The colour is built HERE (QCView 2 did it on the CPU with swscale, 30 ms a
// frame at 4224 x 3024, and named this pass as its follow-up): Malvar, He and Cutler's linear 5 x 5 filter ("High-quality
// linear interpolation for demosaicing of Bayer-patterned color images", 2004), exact integer texel reads. The result is
// the sensor's own linear RGB as the decoder left it: no camera matrix, as in QCView 2.
@group(0) @binding(0) var samp: sampler;
@group(0) @binding(1) var tex: texture_2d<f32>;
@group(0) @binding(2) var<uniform> u: U;
@vertex fn vs(@builtin(vertex_index) i: u32) -> VsOut { return fullscreen(i, u.fit, u.off); }
// A texel by its position; past an edge the picture is mirrored about the edge texel, which keeps the colour of the site.
fn site(p: vec2<i32>, d: vec2<i32>) -> f32 {
  var q = abs(p);
  let m = d - vec2(1, 1);
  q = m - abs(m - q);
  return textureLoad(tex, q, 0).r;
}
fn demosaic(p: vec2<i32>, d: vec2<i32>) -> vec3<f32> {
  let c = site(p, d);
  let n = site(p + vec2(0, -1), d); let s = site(p + vec2(0, 1), d); let e = site(p + vec2(1, 0), d); let w = site(p + vec2(-1, 0), d);
  let n2 = site(p + vec2(0, -2), d); let s2 = site(p + vec2(0, 2), d); let e2 = site(p + vec2(2, 0), d); let w2 = site(p + vec2(-2, 0), d);
  let diag = site(p + vec2(-1, -1), d) + site(p + vec2(1, -1), d) + site(p + vec2(-1, 1), d) + site(p + vec2(1, 1), d);
  let odd_x = (p.x & 1) == 1; let odd_y = (p.y & 1) == 1;
  if (odd_x == odd_y) {
    // a red site (even, even) or a blue site (odd, odd): green from the cross, the other colour from the diagonals
    let g = (4.0 * c + 2.0 * (n + s + e + w) - (n2 + s2 + e2 + w2)) / 8.0;
    let o = (6.0 * c + 2.0 * diag - 1.5 * (n2 + s2 + e2 + w2)) / 8.0;
    if (odd_x) { return vec3(o, g, c); }
    return vec3(c, g, o);
  }
  // a green site: one colour lies left and right of it, the other above and below
  let h = (5.0 * c + 4.0 * (e + w) - diag - (e2 + w2) + 0.5 * (n2 + s2)) / 8.0;
  let v = (5.0 * c + 4.0 * (n + s) - diag - (n2 + s2) + 0.5 * (e2 + w2)) / 8.0;
  if (odd_x) { return vec3(h, c, v); }   // a red row (even y): red beside it, blue above and below
  return vec3(v, c, h);                  // a blue row
}
@fragment fn fs(in: VsOut) -> @location(0) vec4<f32> {
  let d = vec2<i32>(textureDimensions(tex));
  let t = in.uv * vec2<f32>(d) - vec2(0.5, 0.5);
  let b = floor(t); let f = t - b; let p = vec2<i32>(b);
  var rgb: vec3<f32>;
  // drawn 1:1 (the colour chain's input, a snapshot) the sample sits on a texel: one site. Scaled, the four around it
  // are blended as the sampler blends the other layouts.
  if (max(f.x, f.y) < 0.002) { rgb = demosaic(p, d); }
  else if (min(f.x, f.y) > 0.998) { rgb = demosaic(p + vec2(1, 1), d); }
  else { rgb = mix(mix(demosaic(p, d), demosaic(p + vec2(1, 0), d), f.x), mix(demosaic(p + vec2(0, 1), d), demosaic(p + vec2(1, 1), d), f.x), f.y); }
  return finish(rgb_levels(rgb));
}
