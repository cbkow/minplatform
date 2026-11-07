// The viewport background (canon row 153; QCView's D3D11Compositor `backgroundColor` / `fillColor`, retuned 2026-09-15,
// carried verbatim): drawn FIRST over the whole viewport, the picture composited over it by its alpha. Modes (u.bg.x):
// 0 Black · 1 DarkGray #1f1f1f · 2 DarkChecker #1f1f1f / #2e2e2e · 3 LightChecker #b3b3b3 / #cccccc; the checker is
// 32 DEVICE pixels a tile (u.bg.z = 32 × the window scale). Outside the media rect (u.media, px, y down) the fill is
// mixed toward Theme.bg #161616 by u.bg.y (0 / 1 / 0.7 / 0.7 per mode; `QCV_BOUNDS_MIX` overrides) so a transparent
// clip still shows its footprint. The constants are sRGB-encoded; they are decoded and placed at the display's SDR
// reference white like the UI layer, so the fill never passes through the colour chain and never blazes on EDR / PQ.
@group(0) @binding(0) var<uniform> u: U;
@vertex fn vs(@builtin(vertex_index) i: u32) -> VsOut { return fullscreen(i, vec2(1.0, 1.0), vec2(0.0, 0.0)); }
fn checker(px: vec2<f32>, a: f32, b: f32) -> vec3<f32> {
  let cell = floor(px / max(u.bg.z, 1.0));
  let odd = ((i32(cell.x) + i32(cell.y)) & 1) != 0;
  let v = select(a, b, odd);
  return vec3(v, v, v);
}
fn fill(px: vec2<f32>) -> vec3<f32> {
  let m = u.bg.x;
  if (m < 0.5) { return vec3(0.0, 0.0, 0.0); }
  if (m < 1.5) { return vec3(0.1216, 0.1216, 0.1216); }
  if (m < 2.5) { return checker(px, 0.122, 0.180); }
  return checker(px, 0.702, 0.800);
}
@fragment fn fs(in: VsOut) -> @location(0) vec4<f32> {
  let px = in.uv * u.vp.xy;
  var f = fill(px);
  let inside = px.x >= u.media.x && px.x < u.media.x + u.media.z && px.y >= u.media.y && px.y < u.media.y + u.media.w;
  if (!inside) { f = mix(f, vec3(22.0 / 255.0), clamp(u.bg.y, 0.0, 1.0)); }
  let lin = srgb_eotf(f);
  var enc: vec3<f32>;
  if (u.out_mode < 0.5) { enc = lin; }
  else if (u.out_mode < 2.5) { enc = pq_oetf(lin * u.white); }
  else if (u.out_mode < 3.5) { enc = lin; }
  else { enc = srgb_oetf(lin); }
  return vec4(enc, 1.0);
}
