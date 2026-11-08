//! Dual view in the presenter (canon rows 150 / 173 / 209): two SIDE engines — each the whole single-picture machinery
//! (upload, decode imports, its own colour chain, geometry) — draw A and B; the main engine keeps the window's layers
//! (background, the UI, the waveform). QCView's four compare modes:
//!   Single        A alone, in the whole content rect;
//!   Side-by-side  A in the left half, B in the right, each fitted to its half;
//!   Split wipe    both fitted to the whole rect, A left of the seam, B right (a scissor per side);
//!   Difference    both rendered to textures at the content size, then |A − B| × gain on the encoded values.
use crate::engine::{DisplayMode, Engine, ScopeView, ViewerAids};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum CompareMode { #[default] Single, SideBySide, Split, Difference }

struct Diff { pipeline: wgpu::RenderPipeline, layout: wgpu::BindGroupLayout, sampler: wgpu::Sampler, ubuf: wgpu::Buffer, tex: Option<(wgpu::Texture, wgpu::Texture, (u32, u32))> }

pub struct Dual {
    pub a: Engine,
    pub b: Engine,
    pub mode: CompareMode,
    /// Split wipe: the seam, 0..1 across the content rect
    pub seam: f32,
    /// Difference: the gain (QCView's overlay slider; 1 = plain |A − B|)
    pub gain: f32,
    device: wgpu::Device, queue: wgpu::Queue, format: wgpu::TextureFormat,
    content: (u32, u32, u32, u32), viewport: (u32, u32), out_gain: f32,
    diff: Option<Diff>,
}

impl Dual {
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue, format: wgpu::TextureFormat) -> Dual {
        Dual { a: Engine::new(device.clone(), queue.clone(), format), b: Engine::new(device.clone(), queue.clone(), format), mode: CompareMode::SideBySide, seam: 0.5, gain: 1.0,
            device: device.clone(), queue: queue.clone(), format, content: (0, 0, 1, 1), viewport: (1, 1), out_gain: 1.0, diff: None }
    }
    /// The content rect's split for the current mode → each side's viewport + content (called by the main engine on any change).
    pub(crate) fn layout(&mut self, viewport: (u32, u32), content: Option<(u32, u32, u32, u32)>, display: DisplayMode, aids: ViewerAids, out_gain: f32, scope: Option<ScopeView>) {
        let c = content.unwrap_or((0, 0, viewport.0, viewport.1));
        self.content = c; self.viewport = viewport; self.out_gain = out_gain;
        let (x, y, w, h) = c;
        let diff = self.mode == CompareMode::Difference;
        for e in [&mut self.a, &mut self.b] {
            e.set_display(display);
            // Difference renders the sides plain (the aids and the SDR-white gain apply once, after the difference)
            e.set_viewer_aids(if diff { ViewerAids::default() } else { aids });
            e.set_output_gain(if diff { 1.0 } else { out_gain });
            // each side taps its own waveform (QCView: one grid per side); the main engine draws them overlaid
            e.set_scope(scope);
        }
        match self.mode {
            CompareMode::SideBySide => {
                let wa = w / 2;
                for (e, r) in [(&mut self.a, (x, y, wa.max(1), h)), (&mut self.b, (x + wa, y, (w - wa).max(1), h))] { e.set_viewport(viewport.0, viewport.1); e.set_content_rect(Some(r)); }
            }
            CompareMode::Difference => { for e in [&mut self.a, &mut self.b] { e.set_viewport(w.max(1), h.max(1)); e.set_content_rect(None); } }
            _ => { for e in [&mut self.a, &mut self.b] { e.set_viewport(viewport.0, viewport.1); e.set_content_rect(Some(c)); } }
        }
    }
    /// The seam's x in viewport px (Split wipe), for the shell's handle.
    pub fn seam_x(&self) -> f64 { self.content.0 as f64 + self.seam.clamp(0.0, 1.0) as f64 * self.content.2 as f64 }
    pub(crate) fn encode(&mut self, enc: &mut wgpu::CommandEncoder, target: &wgpu::TextureView) {
        let (x, y, w, h) = self.content;
        match self.mode {
            CompareMode::Single => self.a.encode_picture(enc, target, None),
            CompareMode::SideBySide => { self.a.encode_picture(enc, target, None); self.b.encode_picture(enc, target, None); }
            CompareMode::Split => {
                let sx = (self.seam_x().round() as u32).clamp(x, x + w);
                if sx > x { self.a.encode_picture(enc, target, Some([x, y, sx - x, h])); }
                if sx < x + w { self.b.encode_picture(enc, target, Some([sx, y, x + w - sx, h])); }
            }
            CompareMode::Difference => self.encode_difference(enc, target, (x, y, w.max(1), h.max(1))),
        }
    }
    fn encode_difference(&mut self, enc: &mut wgpu::CommandEncoder, target: &wgpu::TextureView, r: (u32, u32, u32, u32)) {
        if self.diff.is_none() {
            let module = self.device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("dual difference"), source: wgpu::ShaderSource::Wgsl(include_str!("shaders/dual.wgsl").into()) });
            let tex_entry = |b| wgpu::BindGroupLayoutEntry { binding: b, visibility: wgpu::ShaderStages::FRAGMENT, ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: true }, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false }, count: None };
            let layout = self.device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: Some("dual difference"), entries: &[
                wgpu::BindGroupLayoutEntry { binding: 0, visibility: wgpu::ShaderStages::VERTEX_FRAGMENT, ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None }, count: None },
                wgpu::BindGroupLayoutEntry { binding: 1, visibility: wgpu::ShaderStages::FRAGMENT, ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering), count: None },
                tex_entry(2), tex_entry(3)] });
            let pl = self.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: None, bind_group_layouts: &[Some(&layout)], immediate_size: 0 });
            let pipeline = self.device.create_render_pipeline(&wgpu::RenderPipelineDescriptor { label: Some("dual difference"), layout: Some(&pl),
                vertex: wgpu::VertexState { module: &module, entry_point: Some("vs"), buffers: &[], compilation_options: Default::default() },
                fragment: Some(wgpu::FragmentState { module: &module, entry_point: Some("fs"), targets: &[Some(wgpu::ColorTargetState { format: self.format, blend: None, write_mask: wgpu::ColorWrites::ALL })], compilation_options: Default::default() }),
                primitive: Default::default(), depth_stencil: None, multisample: Default::default(), multiview_mask: None, cache: None });
            let sampler = self.device.create_sampler(&wgpu::SamplerDescriptor { label: Some("dual"), mag_filter: wgpu::FilterMode::Linear, min_filter: wgpu::FilterMode::Linear, ..Default::default() });
            let ubuf = self.device.create_buffer(&wgpu::BufferDescriptor { label: Some("dual difference"), size: 48, usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
            self.diff = Some(Diff { pipeline, layout, sampler, ubuf, tex: None });
        }
        let d = self.diff.as_mut().unwrap();
        if d.tex.as_ref().map(|t| t.2) != Some((r.2, r.3)) {
            let mk = |l: &str| self.device.create_texture(&wgpu::TextureDescriptor { label: Some(l), size: wgpu::Extent3d { width: r.2, height: r.3, depth_or_array_layers: 1 }, mip_level_count: 1, sample_count: 1, dimension: wgpu::TextureDimension::D2, format: self.format, usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_SRC, view_formats: &[] });
            d.tex = Some((mk("dual A"), mk("dual B"), (r.2, r.3)));
        }
        let (ta, tb, _) = d.tex.as_ref().unwrap();
        let (va, vb) = (ta.create_view(&Default::default()), tb.create_view(&Default::default()));
        for v in [&va, &vb] { let _ = enc.begin_render_pass(&wgpu::RenderPassDescriptor { label: Some("dual clear"), color_attachments: &[Some(wgpu::RenderPassColorAttachment { view: v, depth_slice: None, resolve_target: None, ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT), store: wgpu::StoreOp::Store } })], depth_stencil_attachment: None, timestamp_writes: None, occlusion_query_set: None, multiview_mask: None }); }
        self.a.encode_picture(enc, &va, None); self.b.encode_picture(enc, &vb, None);
        let u: [f32; 12] = [r.0 as f32, r.1 as f32, r.2 as f32, r.3 as f32, self.viewport.0 as f32, self.viewport.1 as f32, 0.0, 0.0, self.gain, self.out_gain, 0.0, 0.0];
        self.queue.write_buffer(&d.ubuf, 0, bytemuck::cast_slice(&u));
        let bind = self.device.create_bind_group(&wgpu::BindGroupDescriptor { label: Some("dual difference"), layout: &d.layout, entries: &[
            wgpu::BindGroupEntry { binding: 0, resource: d.ubuf.as_entire_binding() }, wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&d.sampler) },
            wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(&va) }, wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::TextureView(&vb) }] });
        let mut p = enc.begin_render_pass(&wgpu::RenderPassDescriptor { label: Some("dual difference"), color_attachments: &[Some(wgpu::RenderPassColorAttachment { view: target, depth_slice: None, resolve_target: None, ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store } })], depth_stencil_attachment: None, timestamp_writes: None, occlusion_query_set: None, multiview_mask: None });
        p.set_pipeline(&d.pipeline); p.set_bind_group(0, &bind, &[]); p.draw(0..6, 0..1);
    }
}
