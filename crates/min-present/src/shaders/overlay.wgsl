// Stage 4 — the UI layer (canon §25, the build order; §11.2 "composite"). The overlay texture is vello's output at the
// viewport size: sRGB-encoded, STRAIGHT alpha (vello's fine stage stores separated alpha). Each texel is decoded to
// linear light, placed at the display's SDR reference white (UI white = 1.0 = `white` nits, so chrome never blazes on
// an EDR or PQ surface), encoded the way stage 3 encoded the video, and premultiplied; the pipeline blends it over the
// finished video with One / OneMinusSrcAlpha. Exact per pixel where alpha is 0 or 1; on anti-aliased edges over a
// non-linear target (PQ, sRGB-encoded 8-bit) the blend happens in the encoded space, as every UI toolkit does.
@group(0) @binding(0) var samp: sampler;
@group(0) @binding(1) var tex: texture_2d<f32>;
@group(0) @binding(2) var<uniform> u: U;
@vertex fn vs(@builtin(vertex_index) i: u32) -> VsOut { return fullscreen(i, vec2(1.0, 1.0), vec2(0.0, 0.0)); }
@fragment fn fs(in: VsOut) -> @location(0) vec4<f32> {
  let s = textureSample(tex, samp, in.uv);
  let lin = srgb_eotf(clamp(s.rgb, vec3(0.0), vec3(1.0)));
  var enc: vec3<f32>;
  if (u.out_mode < 0.5) { enc = lin * select(1.0, u.pad1.x, u.pad1.x > 0.0); }   // SDR white at the user's SDR brightness (Windows scRGB, row 184)
  else if (u.out_mode < 2.5) { enc = pq_oetf(lin * u.white); }
  else if (u.out_mode < 3.5) { enc = lin; }
  else { enc = srgb_oetf(lin); }
  return vec4(enc * s.a, s.a);
}
