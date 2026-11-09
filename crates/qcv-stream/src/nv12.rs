//! The NV12 pass (QCView's Metal kernel `metal_player_renderer.mm` 476–503 / the D3D11 plane passes): the output's SDR wire
//! picture (encoded, non-linear RGB) → the encoder surface's two planes, written in place on the GPU. BT.709, LIMITED range,
//! our maths — `Y = dot(rgb, (0.2126, 0.7152, 0.0722)) · 219/255 + 16/255`; chroma from the 2×2 average:
//! `Cb = (B − Y′)/1.8556 · 224/255 + 128/255`, `Cr = (R − Y′)/1.5748 · 224/255 + 128/255` — RGB saturated first.
const SHADER: &str = r#"
@group(0) @binding(0) var src: texture_2d<f32>;
const K: vec3<f32> = vec3<f32>(0.2126, 0.7152, 0.0722);
@vertex fn vs(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    let p = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    return vec4<f32>(p * 2.0 - 1.0, 0.0, 1.0);
}
fn px(x: i32, y: i32) -> vec3<f32> {
    let d = vec2<i32>(textureDimensions(src));
    return clamp(textureLoad(src, clamp(vec2<i32>(x, y), vec2<i32>(0), d - vec2<i32>(1)), 0).rgb, vec3(0.0), vec3(1.0));
}
@fragment fn fs_y(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let y = dot(px(i32(pos.x), i32(pos.y)), K) * (219.0 / 255.0) + 16.0 / 255.0;
    return vec4<f32>(y, 0.0, 0.0, 1.0);
}
@fragment fn fs_c(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let x = i32(pos.x) * 2; let y = i32(pos.y) * 2;
    let c = (px(x, y) + px(x + 1, y) + px(x, y + 1) + px(x + 1, y + 1)) * 0.25;
    let yf = dot(c, K);
    let cb = (c.b - yf) / 1.8556 * (224.0 / 255.0) + 128.0 / 255.0;
    let cr = (c.r - yf) / 1.5748 * (224.0 / 255.0) + 128.0 / 255.0;
    return vec4<f32>(cb, cr, 0.0, 1.0);
}
"#;

pub struct Nv12Pass { layout: wgpu::BindGroupLayout, y: wgpu::RenderPipeline, c: wgpu::RenderPipeline }

impl Nv12Pass {
    pub fn new(device: &wgpu::Device) -> Nv12Pass {
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: Some("nv12"), entries: &[wgpu::BindGroupLayoutEntry { binding: 0, visibility: wgpu::ShaderStages::FRAGMENT, ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: false }, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false }, count: None }] });
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("nv12"), source: wgpu::ShaderSource::Wgsl(SHADER.into()) });
        let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some("nv12"), bind_group_layouts: &[Some(&layout)], immediate_size: 0 });
        let pipe = |entry: &str, format: wgpu::TextureFormat| device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("nv12"), layout: Some(&pl),
            vertex: wgpu::VertexState { module: &module, entry_point: Some("vs"), buffers: &[], compilation_options: Default::default() },
            fragment: Some(wgpu::FragmentState { module: &module, entry_point: Some(entry), targets: &[Some(wgpu::ColorTargetState { format, blend: None, write_mask: wgpu::ColorWrites::ALL })], compilation_options: Default::default() }),
            primitive: Default::default(), depth_stencil: None, multisample: Default::default(), multiview_mask: None, cache: None,
        });
        Nv12Pass { y: pipe("fs_y", wgpu::TextureFormat::R8Unorm), c: pipe("fs_c", wgpu::TextureFormat::Rg8Unorm), layout }
    }
    /// Write `src` (the wire picture at the surface's size) into the surface's Y (`R8Unorm`, full size) and CbCr (`Rg8Unorm`,
    /// half size) planes. The caller submits `enc` and hands the surface over when the GPU is done.
    pub fn encode(&self, device: &wgpu::Device, enc: &mut wgpu::CommandEncoder, src: &wgpu::TextureView, y: &wgpu::TextureView, cbcr: &wgpu::TextureView) {
        let bind = device.create_bind_group(&wgpu::BindGroupDescriptor { label: Some("nv12"), layout: &self.layout, entries: &[wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(src) }] });
        for (view, pipe) in [(y, &self.y), (cbcr, &self.c)] {
            let mut p = enc.begin_render_pass(&wgpu::RenderPassDescriptor { label: Some("nv12"), color_attachments: &[Some(wgpu::RenderPassColorAttachment { view, resolve_target: None, depth_slice: None, ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store } })], depth_stencil_attachment: None, timestamp_writes: None, occlusion_query_set: None, multiview_mask: None });
            p.set_pipeline(pipe); p.set_bind_group(0, &bind, &[]); p.draw(0..3, 0..1);
        }
    }
}
