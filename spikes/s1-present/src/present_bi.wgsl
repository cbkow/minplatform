// Biplanar (VideoToolbox) Y + CbCr → BT.709 R'G'B' → BT.1886 linear on an extended-linear surface.
struct U { fit: vec2<f32>, yscale: f32, full_range: f32 };
@group(0) @binding(0) var samp: sampler;
@group(0) @binding(1) var ty: texture_2d<f32>;
@group(0) @binding(2) var tuv: texture_2d<f32>;
@group(0) @binding(3) var<uniform> u: U;
struct VsOut { @builtin(position) pos: vec4<f32>, @location(0) uv: vec2<f32> };
@vertex fn vs(@builtin(vertex_index) i: u32) -> VsOut {
  var p = array<vec2<f32>, 3>(vec2(-1.0, -1.0), vec2(3.0, -1.0), vec2(-1.0, 3.0));
  var o: VsOut; o.pos = vec4(p[i] * u.fit, 0.0, 1.0); o.uv = vec2((p[i].x + 1.0) * 0.5, 1.0 - (p[i].y + 1.0) * 0.5); return o;
}
@fragment fn fs(in: VsOut) -> @location(0) vec4<f32> {
  var y = textureSample(ty, samp, in.uv).r * u.yscale;
  let c = textureSample(tuv, samp, in.uv).rg * u.yscale - vec2(0.5);
  var cb = c.x; var cr = c.y;
  if (u.full_range < 0.5) { y = (y - 16.0 / 255.0) * (255.0 / 219.0); cb = cb * (255.0 / 224.0); cr = cr * (255.0 / 224.0); }
  let rgb = clamp(vec3(y + 1.5748 * cr, y - 0.1873 * cb - 0.4681 * cr, y + 1.8556 * cb), vec3(0.0), vec3(1.0));
  return vec4(pow(rgb, vec3(2.4)), 1.0);
}
