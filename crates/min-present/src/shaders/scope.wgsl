// The waveform (QCView scope_math.h, waveform only — canon row 203): one tapped pixel = one count in a 512² histogram
// (image column × level). The tap is the SOURCE through the clip's Input, in linear light with a matrix to linear
// Rec.2020 (1.0 = 100 nits). SDR scale: Rec.709 primaries, pure 2.4 inverse (BT.1886, black 0), Y′ with −10…110 % room.
// HDR scale: Rec.2020 luminance in nits, 0…peak — above the peak piles up at the top. Sign-preserving curves.
struct S {
  to0: vec4<f32>, to1: vec4<f32>, to2: vec4<f32>,   // tap → linear Rec.2020 rows
  dims: vec4<f32>,      // tap texture w, h; sampled grid w, h
  p: vec4<f32>,         // x = HDR scale (0/1), y = HDR top (nits), z = nits per 1.0, w = —
};
const GRID: u32 = 512u;
@group(0) @binding(0) var tap: texture_2d<f32>;
@group(0) @binding(1) var<uniform> s: S;
@group(0) @binding(2) var<storage, read_write> hist: array<atomic<u32>>;
// QCView's ScopePeaks for the frame: [0] the waveform level (Y′ on SDR, nits on HDR), [1] the brightest channel (encoded 0..1 on
// SDR, nits on HDR) — as f32 bits, ≥ 0, so an unsigned max orders them
@group(0) @binding(3) var<storage, read_write> peaks: array<atomic<u32>, 2>;

fn spow(x: vec3<f32>, k: f32) -> vec3<f32> { return sign(x) * pow(abs(x), vec3(k)); }

@compute @workgroup_size(16, 16)
fn accumulate(@builtin(global_invocation_id) id: vec3<u32>) {
  let gw = u32(s.dims.z); let gh = u32(s.dims.w);
  if (id.x >= gw || id.y >= gh) { return; }
  let tx = i32(f32(id.x) * s.dims.x / s.dims.z); let ty = i32(f32(id.y) * s.dims.y / s.dims.w);
  let c = textureLoad(tap, vec2<i32>(tx, ty), 0).rgb;
  let l = vec3(dot(s.to0.xyz, c), dot(s.to1.xyz, c), dot(s.to2.xyz, c));   // linear Rec.2020
  var lo: f32; var hi: f32; var y: f32;
  if (s.p.x > 0.5) {
    lo = 0.0; hi = s.p.y;
    y = s.p.z * dot(vec3(0.2627, 0.6780, 0.0593), l);
    atomicMax(&peaks[1], bitcast<u32>(max(s.p.z * max(l.r, max(l.g, l.b)), 0.0)));
  } else {
    lo = -0.1; hi = 1.1;
    let l7 = vec3(1.6604910 * l.r - 0.5876411 * l.g - 0.0728499 * l.b, -0.1245505 * l.r + 1.1328999 * l.g - 0.0083494 * l.b, -0.0181508 * l.r - 0.1005789 * l.g + 1.1187297 * l.b);
    let e = spow(l7, 1.0 / 2.4);
    y = dot(vec3(0.2126, 0.7152, 0.0722), e);
    if (e.r == e.r && e.g == e.g && e.b == e.b) { atomicMax(&peaks[1], bitcast<u32>(max(max(e.r, max(e.g, e.b)), 0.0))); }
  }
  if (y != y) { return; }   // NaN
  atomicMax(&peaks[0], bitcast<u32>(max(y, 0.0)));
  let bx = min(u32(f32(id.x) / f32(gw) * f32(GRID)), GRID - 1u);
  let by = u32(clamp(floor((hi - y) / (hi - lo) * f32(GRID)), 0.0, f32(GRID - 1u)));
  atomicAdd(&hist[by * GRID + bx], 1u);
}

// The draw: the histogram over the UI, in the card's rect. Count → 1 − e^(−gain·count) (QCView resolveDraw); bins out of
// the scale's range (SDR above 100 % / below 0 %, HDR at the top row) tinted red.
// q: x = sides (1, or 2 = dual view's A + B overlay, QCView cyan / orange), y = B's gain
struct D { rect: vec4<f32>, vp: vec4<f32>, p: vec4<f32>, q: vec4<f32> };   // rect px (x, y, w, h); viewport px; x = gain, y = HDR, z = HDR top nits, w = the output's SDR-white gain
@group(0) @binding(0) var<uniform> d: D;
@group(0) @binding(1) var<storage, read> h: array<u32>;
@group(0) @binding(2) var<storage, read> h2: array<u32>;
struct V { @builtin(position) pos: vec4<f32>, @location(0) uv: vec2<f32> };
@vertex fn vs(@builtin(vertex_index) i: u32) -> V {
  let q = array<vec2<f32>, 6>(vec2(0.0, 0.0), vec2(1.0, 0.0), vec2(0.0, 1.0), vec2(0.0, 1.0), vec2(1.0, 0.0), vec2(1.0, 1.0));
  let uv = q[i]; let px = d.rect.xy + uv * d.rect.zw;
  var o: V; o.pos = vec4(px.x / d.vp.x * 2.0 - 1.0, 1.0 - px.y / d.vp.y * 2.0, 0.0, 1.0); o.uv = uv; return o;
}
@fragment fn fs(v: V) -> @location(0) vec4<f32> {
  let bx = min(u32(v.uv.x * f32(GRID)), GRID - 1u); let by = min(u32(v.uv.y * f32(GRID)), GRID - 1u);
  let level_frac = (f32(by) + 0.5) / f32(GRID);   // 0 top … 1 bottom
  var out_of_range = false;
  if (d.p.y > 0.5) { out_of_range = by == 0u; } else { let y = 1.1 - level_frac * 1.2; out_of_range = y > 1.0 || y < 0.0; }
  let red = vec3(1.0, 0.33, 0.28);
  let k = select(1.0, d.p.w, d.p.w > 0.0);   // the SDR-white gain (row 184)
  let a0 = 1.0 - exp(-d.p.x * f32(h[by * GRID + bx]));
  if (d.q.x < 1.5) {
    return vec4(select(vec3(0.86, 0.88, 0.86), red, out_of_range) * k * a0, a0);
  }
  // dual (QCView scope_draw, sides == 2): each side tinted, summed, alpha = the brighter; premultiplied
  let a1 = 1.0 - exp(-d.q.y * f32(h2[by * GRID + bx]));
  var col = select(vec3(0.30, 0.85, 1.0), red, out_of_range) * a0 + select(vec3(1.0, 0.62, 0.25), red, out_of_range) * a1;
  let a = max(a0, a1);
  col = min(col, vec3(a));
  return vec4(col * k, a);
}
