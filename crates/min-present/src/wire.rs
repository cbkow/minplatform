//! The output WIRE (canon row 227 step 3; QCView's output pass, the QCVO slot formats): the engine's output canvas
//! (`Engine::render_output`, already in the wire's encoding) quantised into the slot's pixel format — `Bgra8Unorm` (the bus's
//! BGRA8: B, G, R, A in memory) or `Rgb10a2Unorm` (its memory layout IS the bus's RGB10: R in bits 0-9, G 10-19, B 20-29, the
//! top two bits unused — QCView's note) — and read back without stalling: a small ring of staging buffers mapped
//! asynchronously; the caller takes a finished frame on a later tick and copies it into the ring slot (rows already padded to
//! 256 bytes, the bus's alignment and wgpu's copy pitch).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WireFormat { Bgra8, Rgb10 }
impl WireFormat { pub fn texture_format(self) -> wgpu::TextureFormat { match self { WireFormat::Bgra8 => wgpu::TextureFormat::Bgra8Unorm, WireFormat::Rgb10 => wgpu::TextureFormat::Rgb10a2Unorm } } }

const SHADER: &str = r#"
@group(0) @binding(0) var src: texture_2d<f32>;
@vertex fn vs(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    let p = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    return vec4<f32>(p * 2.0 - 1.0, 0.0, 1.0);
}
@fragment fn fs(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let c = textureLoad(src, vec2<i32>(pos.xy), 0);
    return vec4<f32>(clamp(c.rgb, vec3<f32>(0.0), vec3<f32>(1.0)), 1.0);   // over black (the canvas was cleared black)
}
"#;

struct Staging { buf: wgpu::Buffer, w: u32, h: u32, bpr: u32, state: std::sync::Arc<std::sync::atomic::AtomicU8>, tag: u64 }
// state: 0 free · 1 copying / mapping · 2 mapped (ready) · 3 failed

pub struct WireEncoder { layout: wgpu::BindGroupLayout, pipes: Vec<(wgpu::TextureFormat, wgpu::RenderPipeline)>, staging: Vec<Staging> }

impl WireEncoder {
    pub fn new(device: &wgpu::Device) -> WireEncoder {
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: Some("wire"), entries: &[wgpu::BindGroupLayoutEntry { binding: 0, visibility: wgpu::ShaderStages::FRAGMENT, ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: false }, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false }, count: None }] });
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("wire"), source: wgpu::ShaderSource::Wgsl(SHADER.into()) });
        let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some("wire"), bind_group_layouts: &[Some(&layout)], immediate_size: 0 });
        let pipes = [wgpu::TextureFormat::Bgra8Unorm, wgpu::TextureFormat::Rgb10a2Unorm].into_iter().map(|f| (f, device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("wire"), layout: Some(&pl),
            vertex: wgpu::VertexState { module: &module, entry_point: Some("vs"), buffers: &[], compilation_options: Default::default() },
            fragment: Some(wgpu::FragmentState { module: &module, entry_point: Some("fs"), targets: &[Some(wgpu::ColorTargetState { format: f, blend: None, write_mask: wgpu::ColorWrites::ALL })], compilation_options: Default::default() }),
            primitive: Default::default(), depth_stencil: None, multisample: Default::default(), multiview_mask: None, cache: None,
        }))).collect();
        WireEncoder { layout, pipes, staging: vec![] }
    }
    /// Quantise `canvas` into `fmt` and start its copy-out; `tag` comes back with the bytes. False when every staging buffer
    /// is still busy (QCView: the frame is skipped — "lossy by design").
    pub fn submit(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, canvas: &wgpu::Texture, fmt: WireFormat, tag: u64) -> bool {
        use std::sync::atomic::Ordering::*;
        let (w, h) = (canvas.width(), canvas.height());
        let bpr = (w * 4).div_ceil(256) * 256;
        let i = match self.staging.iter().position(|s| s.state.load(Acquire) == 0 && s.w == w && s.h == h) {
            Some(i) => i,
            None => {
                if self.staging.len() >= 3 { if let Some(j) = self.staging.iter().position(|s| s.state.load(Acquire) == 0 || s.state.load(Acquire) == 3) { self.staging.remove(j); } else { return false } }
                let buf = device.create_buffer(&wgpu::BufferDescriptor { label: Some("wire staging"), size: (bpr * h) as u64, usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ, mapped_at_creation: false });
                self.staging.push(Staging { buf, w, h, bpr, state: std::sync::Arc::new(std::sync::atomic::AtomicU8::new(0)), tag: 0 });
                self.staging.len() - 1
            }
        };
        let tf = fmt.texture_format();
        let out = device.create_texture(&wgpu::TextureDescriptor { label: Some("wire"), size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 }, mip_level_count: 1, sample_count: 1, dimension: wgpu::TextureDimension::D2, format: tf, usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC, view_formats: &[] });
        let view = out.create_view(&Default::default());
        let bind = device.create_bind_group(&wgpu::BindGroupDescriptor { label: Some("wire"), layout: &self.layout, entries: &[wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&canvas.create_view(&Default::default())) }] });
        let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("wire") });
        {
            let mut p = enc.begin_render_pass(&wgpu::RenderPassDescriptor { label: Some("wire"), color_attachments: &[Some(wgpu::RenderPassColorAttachment { view: &view, resolve_target: None, depth_slice: None, ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store } })], depth_stencil_attachment: None, timestamp_writes: None, occlusion_query_set: None, multiview_mask: None });
            p.set_pipeline(&self.pipes.iter().find(|(f, _)| *f == tf).unwrap().1); p.set_bind_group(0, &bind, &[]); p.draw(0..3, 0..1);
        }
        let s = &mut self.staging[i];
        enc.copy_texture_to_buffer(out.as_image_copy(), wgpu::TexelCopyBufferInfo { buffer: &s.buf, layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(s.bpr), rows_per_image: Some(h) } }, wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 });
        queue.submit([enc.finish()]);
        s.state.store(1, Release); s.tag = tag;
        let st = s.state.clone();
        s.buf.slice(..).map_async(wgpu::MapMode::Read, move |r| st.store(if r.is_ok() { 2 } else { 3 }, Release));
        true
    }
    /// The oldest finished frame, handed to `f` as (bytes with `bpr`-byte rows, w, h, bpr, tag); its buffer is free again after.
    pub fn take(&mut self, device: &wgpu::Device, f: &mut dyn FnMut(&[u8], u32, u32, u32, u64)) -> bool {
        use std::sync::atomic::Ordering::*;
        let _ = device.poll(wgpu::PollType::Poll);
        let mut any = false;
        let mut ready: Vec<usize> = (0..self.staging.len()).filter(|&i| self.staging[i].state.load(Acquire) == 2).collect();
        ready.sort_by_key(|&i| self.staging[i].tag);
        for i in ready {
            let s = &self.staging[i];
            if let Ok(data) = s.buf.slice(..).get_mapped_range() { f(&data, s.w, s.h, s.bpr, s.tag); }
            s.buf.unmap(); s.state.store(0, Release); any = true;
        }
        for s in &self.staging { if s.state.load(Acquire) == 3 { s.state.store(0, Release); } }
        any
    }
}
