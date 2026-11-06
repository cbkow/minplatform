// Route B: the baked chain as ONE 3D LUT (edge³, Rgba16Float, trilinear) over the encoded input — the WGSL stage every backend runs.
struct P { edge: f32, p0: f32, p1: f32, p2: f32 };
@group(0) @binding(0) var src: texture_2d<f32>;
@group(0) @binding(1) var dst: texture_storage_2d<rgba16float, write>;
@group(0) @binding(2) var samp: sampler;
@group(0) @binding(3) var lut: texture_3d<f32>;
@group(0) @binding(4) var<uniform> p: P;
@compute @workgroup_size(16, 16, 1) fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
  let dim = textureDimensions(dst);
  if (gid.x >= dim.x || gid.y >= dim.y) { return; }
  let c = clamp(textureLoad(src, vec2<i32>(gid.xy), 0).rgb, vec3(0.0), vec3(1.0));
  let uvw = c * (p.edge - 1.0) / p.edge + vec3(0.5 / p.edge);
  let o = textureSampleLevel(lut, samp, uvw, 0.0);
  textureStore(dst, vec2<i32>(gid.xy), vec4(o.rgb, 1.0));
}
