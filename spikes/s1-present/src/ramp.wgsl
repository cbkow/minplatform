// Fullscreen triangle; fragment = linear-light ramp. Left half 0→1 (SDR), right half 1→4 (EDR headroom).
// Three horizontal bands: top = grey ramp, middle = red, bottom = blue — clipping shows per channel.
struct VsOut { @builtin(position) pos: vec4<f32>, @location(0) uv: vec2<f32> };
@vertex fn vs(@builtin(vertex_index) i: u32) -> VsOut {
  var p = array<vec2<f32>, 3>(vec2(-1.0, -1.0), vec2(3.0, -1.0), vec2(-1.0, 3.0));
  var o: VsOut; o.pos = vec4(p[i], 0.0, 1.0); o.uv = (p[i] + 1.0) * 0.5; return o;
}
@fragment fn fs(in: VsOut) -> @location(0) vec4<f32> {
  let x = in.uv.x; let y = 1.0 - in.uv.y;
  let v = select(1.0 + (x - 0.5) * 6.0, x * 2.0, x < 0.5);       // 0..1 | 1..4 nits-of-white multiples
  let tick = select(0.0, 1.0, fract(v) < 0.02 && v >= 1.0);      // tick every integer multiple
  var c = vec3(v);
  if (y > 0.333 && y <= 0.666) { c = vec3(v, 0.0, 0.0); }
  if (y > 0.666) { c = vec3(0.0, 0.0, v); }
  if (y < 0.05) { c = vec3(tick); }
  return vec4(c, 1.0);
}
