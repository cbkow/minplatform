// Nested zones as coloured rects; `hot` = the zone under the pointer / drag (brightened); rects in physical px.
struct U { size: vec2<f32>, hot: f32, n: f32, rects: array<vec4<f32>, 4>, colors: array<vec4<f32>, 4> };
@group(0) @binding(0) var<uniform> u: U;
struct VsOut { @builtin(position) pos: vec4<f32> };
@vertex fn vs(@builtin(vertex_index) i: u32) -> VsOut { var p = array<vec2<f32>, 3>(vec2(-1.0, -1.0), vec2(3.0, -1.0), vec2(-1.0, 3.0)); var o: VsOut; o.pos = vec4(p[i], 0.0, 1.0); return o; }
@fragment fn fs(in: VsOut) -> @location(0) vec4<f32> {
  let p = in.pos.xy;
  var c = vec3(0.09, 0.09, 0.10);
  for (var k = 0u; k < u32(u.n); k = k + 1u) {
    let r = u.rects[k];
    if (p.x >= r.x && p.y >= r.y && p.x < r.z && p.y < r.w) { c = u.colors[k].rgb; if (f32(k) == u.hot) { c = c * 1.6 + vec3(0.08); } }
  }
  return vec4(c, 1.0);
}
