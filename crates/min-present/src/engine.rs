//! The engine: the fused graph, the stage-2 slot, the shown frame, the held ring, snapshots. Surface-free — the
//! `Presenter` owns the surface; tests run it headless against an offscreen target.
use crate::{frame::*, import::{Imported, Imports}, readback, Error};
use std::{any::Any, collections::VecDeque, sync::Arc, time::Instant};

#[repr(C)] #[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct U { fit: [f32; 2], yscale: f32, black: f32, lscale: f32, czero: f32, cscale: f32, src_transfer: f32, src_matrix: f32, src_primaries: f32, out_mode: f32, white: f32 }

/// What a colour chain consumes and produces: linear BT.709 working RGB, or display-encoded RGB with a transfer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Space { Linear, Encoded(Transfer) }

/// Stage 2 — the slot (canon §11.2, row 91): OCIO route A, the baked-LUT route, minColor's WGSL. Runs at the video size on
/// `Rgba16Float` textures the engine owns; `src` is stage 1's output in `input()`'s space, `dst` what the blit reads.
pub trait ColorPass {
    fn input(&self) -> Space;
    fn output(&self) -> Space;
    fn run(&mut self, enc: &mut wgpu::CommandEncoder, src: &wgpu::Texture, dst: &wgpu::Texture);
}
/// The identity chain: exercises the three-pass route (stage 1 → slot → blit) with no colour change.
pub struct IdentityColorPass;
impl ColorPass for IdentityColorPass {
    fn input(&self) -> Space { Space::Linear }
    fn output(&self) -> Space { Space::Linear }
    fn run(&mut self, enc: &mut wgpu::CommandEncoder, src: &wgpu::Texture, dst: &wgpu::Texture) {
        enc.copy_texture_to_texture(src.as_image_copy(), dst.as_image_copy(), src.size());
    }
}

/// Stage 3's job on the target (see `stages.wgsl`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum OutMode { /// extended linear, 1.0 = white (EDR surfaces)
    Linear, /// the source's own encoding, untouched (a PQ master on a PQ surface; the snapshot)
    Encoded, /// PQ-encode `white`-relative linear light (PQ surfaces)
    Pq, /// linear clipped to [0, 1] — an sRGB surface encodes in the ROP
    SdrLinear, /// sRGB-encoded here (a plain 8-bit surface)
    SdrEncode }
impl OutMode { fn sel(self) -> f32 { match self { OutMode::Linear => 0.0, OutMode::Encoded => 1.0, OutMode::Pq => 2.0, OutMode::SdrLinear => 3.0, OutMode::SdrEncode => 4.0 } } }
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DisplayMode { pub out: OutMode, /// nits at 1.0 (the SDR reference white; 100 by default)
    pub white: f32 }
impl Default for DisplayMode { fn default() -> Self { DisplayMode { out: OutMode::Linear, white: 100.0 } } }

#[derive(Clone, Copy, Debug)] pub enum Snapshot { /// the source's R'G'B' after the matrix and range only — QCView's `QCV_CAPTURE_PNG` equivalent (identity chain)
    Encoded, /// linear BT.709, 1.0 = white nits
    Linear }

#[derive(Clone, Copy, PartialEq, Eq, Debug)] enum Variant { Planar, Biplanar }
const N_U: usize = 4;   // 0 display · 1 chain-in (1:1) · 2 encoded 1:1 · 3 linear 1:1

#[derive(Clone, Copy, PartialEq)] struct Base { yscale: f32, full_range: bool, /// the bit depth of the SAMPLES (the buffer's, not the codec's)
    depth: u32, meta: ColorMeta }
/// Exact range constants for `d`-bit codes sampled as code/(2^d−1): (black, luma scale, chroma neutral, chroma scale).
fn range_constants(d: u32, full_range: bool) -> (f32, f32, f32, f32) {
    let m = ((1u64 << d) - 1) as f64; let k = (1u64 << (d - 8).max(0)) as f64;   // d ≥ 8
    let czero = ((1u64 << (d - 1)) as f64 / m) as f32;
    if full_range { (0.0, 1.0, czero, 1.0) } else { ((16.0 * k / m) as f32, (m / (219.0 * k)) as f32, czero, (m / (224.0 * k)) as f32) }
}
struct Shown { variant: Variant, binds: Vec<wgpu::BindGroup> }
struct Held { _imp: Imported, _keep: Arc<dyn Any + Send + Sync> }
struct Inter { a: wgpu::Texture, b: wgpu::Texture, bind: wgpu::BindGroup, size: (u32, u32) }

pub struct Engine {
    pub device: wgpu::Device, pub queue: wgpu::Queue,
    pub target_format: wgpu::TextureFormat,
    sampler: wgpu::Sampler,
    planar_layout: wgpu::BindGroupLayout, biplanar_layout: wgpu::BindGroupLayout, blit_layout: wgpu::BindGroupLayout,
    planar_target: wgpu::RenderPipeline, biplanar_target: wgpu::RenderPipeline, planar_inter: wgpu::RenderPipeline, biplanar_inter: wgpu::RenderPipeline, blit_target: wgpu::RenderPipeline,
    ubufs: Vec<wgpu::Buffer>, blit_ubuf: wgpu::Buffer,
    video: (u32, u32), viewport: (u32, u32), display: DisplayMode, base: Option<Base>,
    planar_tex: Vec<wgpu::Texture>,
    shown: Option<Shown>, held: VecDeque<Held>,
    color_pass: Option<Box<dyn ColorPass>>, inter: Option<Inter>,
    imports: Imports, imports_n: u32,
    /// The last `show`'s import / upload cost on the calling thread, ms.
    pub last_show_ms: f32,
    pub frames_shown: u64,
}

const HELD: usize = 3;   // the GPU is ≤ 2 frames behind (frame latency 2); S1's ring

fn make_pipeline(device: &wgpu::Device, shader: &wgpu::ShaderModule, layout: &wgpu::PipelineLayout, format: wgpu::TextureFormat, label: &str) -> wgpu::RenderPipeline {
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor { label: Some(label), layout: Some(layout),
        vertex: wgpu::VertexState { module: shader, entry_point: Some("vs"), buffers: &[], compilation_options: Default::default() },
        fragment: Some(wgpu::FragmentState { module: shader, entry_point: Some("fs"), targets: &[Some(wgpu::ColorTargetState { format, blend: None, write_mask: wgpu::ColorWrites::ALL })], compilation_options: Default::default() }),
        primitive: Default::default(), depth_stencil: None, multisample: Default::default(), multiview_mask: None, cache: None })
}
fn attach(view: &wgpu::TextureView) -> wgpu::RenderPassColorAttachment<'_> { wgpu::RenderPassColorAttachment { view, depth_slice: None, resolve_target: None, ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store } } }
fn pass<'a>(enc: &'a mut wgpu::CommandEncoder, label: &'static str, view: &'a wgpu::TextureView) -> wgpu::RenderPass<'a> {
    enc.begin_render_pass(&wgpu::RenderPassDescriptor { label: Some(label), color_attachments: &[Some(attach(view))], depth_stencil_attachment: None, timestamp_writes: None, occlusion_query_set: None, multiview_mask: None })
}

/// The features the presenter wants when it can have them: 16-bit norm planes, f32-filterable LUTs (stage 2), timestamps, passthrough shaders (OCIO).
pub(crate) fn wanted_features() -> wgpu::Features { wgpu::Features::TEXTURE_FORMAT_16BIT_NORM | wgpu::Features::FLOAT32_FILTERABLE | wgpu::Features::TIMESTAMP_QUERY | wgpu::Features::PASSTHROUGH_SHADERS }
pub(crate) fn request(instance: &wgpu::Instance, surface: Option<&wgpu::Surface<'_>>) -> Result<(wgpu::Adapter, wgpu::Device, wgpu::Queue), Error> {
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions { power_preference: wgpu::PowerPreference::HighPerformance, compatible_surface: surface, force_fallback_adapter: false, apply_limit_buckets: false })).map_err(|_| Error::NoAdapter)?;
    let required_features = adapter.features() & wanted_features();
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor { label: Some("min-present"), required_features, ..Default::default() })).map_err(|e| Error::Device(e.to_string()))?;
    Ok((adapter, device, queue))
}

impl Engine {
    pub fn new(device: wgpu::Device, queue: wgpu::Queue, target_format: wgpu::TextureFormat) -> Engine {
        let stages = include_str!("shaders/stages.wgsl");
        let module = |label: &str, body: &str| device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some(label), source: wgpu::ShaderSource::Wgsl(format!("{stages}\n{body}").into()) });
        let planar = module("planar", include_str!("shaders/planar.wgsl"));
        let biplanar = module("biplanar", include_str!("shaders/biplanar.wgsl"));
        let blit = module("blit", include_str!("shaders/blit.wgsl"));
        let tex = |b| wgpu::BindGroupLayoutEntry { binding: b, visibility: wgpu::ShaderStages::FRAGMENT, ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: true }, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false }, count: None };
        let smp = wgpu::BindGroupLayoutEntry { binding: 0, visibility: wgpu::ShaderStages::FRAGMENT, ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering), count: None };
        let ub = |b| wgpu::BindGroupLayoutEntry { binding: b, visibility: wgpu::ShaderStages::VERTEX_FRAGMENT, ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None }, count: None };
        let planar_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: Some("planar"), entries: &[smp, tex(1), tex(2), tex(3), ub(4)] });
        let biplanar_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: Some("biplanar"), entries: &[smp, tex(1), tex(2), ub(3)] });
        let blit_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: Some("blit"), entries: &[smp, tex(1), ub(2)] });
        let pl = |l: &wgpu::BindGroupLayout| device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: None, bind_group_layouts: &[Some(l)], immediate_size: 0 });
        let (ppl, bpl, lpl) = (pl(&planar_layout), pl(&biplanar_layout), pl(&blit_layout));
        let inter_fmt = wgpu::TextureFormat::Rgba16Float;
        let planar_target = make_pipeline(&device, &planar, &ppl, target_format, "planar→target");
        let biplanar_target = make_pipeline(&device, &biplanar, &bpl, target_format, "biplanar→target");
        let planar_inter = make_pipeline(&device, &planar, &ppl, inter_fmt, "planar→working");
        let biplanar_inter = make_pipeline(&device, &biplanar, &bpl, inter_fmt, "biplanar→working");
        let blit_target = make_pipeline(&device, &blit, &lpl, target_format, "blit→target");
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor { label: Some("video"), mag_filter: wgpu::FilterMode::Linear, min_filter: wgpu::FilterMode::Linear, ..Default::default() });
        let ubuf = |label: &str| device.create_buffer(&wgpu::BufferDescriptor { label: Some(label), size: std::mem::size_of::<U>() as u64, usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
        let ubufs = (0..N_U).map(|i| ubuf(&format!("u{i}"))).collect();
        let blit_ubuf = ubuf("u-blit");
        let imports = Imports::new(&device, &queue);
        Engine { device, queue, target_format, sampler, planar_layout, biplanar_layout, blit_layout, planar_target, biplanar_target, planar_inter, biplanar_inter, blit_target, ubufs, blit_ubuf,
            video: (0, 0), viewport: (1, 1), display: DisplayMode::default(), base: None, planar_tex: vec![], shown: None, held: VecDeque::new(), color_pass: None, inter: None, imports, imports_n: 0, last_show_ms: 0.0, frames_shown: 0 }
    }
    /// An engine with no surface (tests, offscreen renders): the first high-performance adapter.
    pub fn headless(target_format: wgpu::TextureFormat) -> Result<Engine, Error> {
        let instance = crate::presenter::Presenter::instance();   // DX12 on Windows (the hal the zero-copy import needs), the platform default elsewhere
        let (_, device, queue) = request(&instance, None)?;
        Ok(Engine::new(device, queue, target_format))
    }
    pub fn zero_copy_available(&self) -> bool { self.imports.available() }
    pub fn set_viewport(&mut self, w: u32, h: u32) { self.viewport = (w.max(1), h.max(1)); self.write_uniforms(); }
    pub fn viewport(&self) -> (u32, u32) { self.viewport }
    pub fn video_size(&self) -> (u32, u32) { self.video }
    pub fn set_display(&mut self, d: DisplayMode) { self.display = d; self.write_uniforms(); }
    pub fn display(&self) -> DisplayMode { self.display }
    pub fn set_color_pass(&mut self, cp: Option<Box<dyn ColorPass>>) { self.color_pass = cp; self.inter = None; self.write_uniforms(); }
    pub fn has_color_pass(&self) -> bool { self.color_pass.is_some() }
    pub fn has_frame(&self) -> bool { self.shown.is_some() }
    #[cfg(windows)]
    /// The `ID3D12Device` for a D3D12VA decoder context (one reference added for the caller).
    pub fn dx12_device_raw(&self) -> Option<*mut std::ffi::c_void> { crate::import::dx12::Dx12Import::raw_device(&self.device) }

    /// Upload / import `frame` and make it the one every `encode` draws until the next `show`.
    pub fn show(&mut self, frame: &Frame) -> Result<(), Error> {
        let t = Instant::now();
        if self.video != (frame.width, frame.height) { self.video = (frame.width, frame.height); self.base = None; self.inter = None; }
        match &frame.pixels {
            Pixels::Native(nf) => {
                let imp = self.imports.import(&self.device, nf.handle)?;
                let yscale = if imp.ten_bit { 65535.0 / 65472.0 } else { 1.0 };   // 10-bit left-justified in 16
                let full_range = imp.full_range.unwrap_or(frame.meta.range == Range::Full);
                self.set_base(Base { yscale, full_range, depth: if imp.ten_bit { 10 } else { 8 }, meta: frame.meta });
                let views: Vec<_> = imp.textures.iter().map(|t| t.create_view(&Default::default())).collect();
                let binds = self.binds(Variant::Biplanar, &views);
                self.shown = Some(Shown { variant: Variant::Biplanar, binds });
                self.held.push_back(Held { _imp: imp, _keep: nf.keep.clone() });
                while self.held.len() > HELD { self.held.pop_front(); }
                self.imports_n += 1; if self.imports_n % 60 == 0 { self.imports.flush(); }
            }
            Pixels::Planar { planes, layout } => {
                let (fmt, yscale) = match layout {
                    PlanarLayout::RgbF16 => (wgpu::TextureFormat::R16Float, 1.0),
                    PlanarLayout::Ycc { bytes_per_sample: 2 } => (wgpu::TextureFormat::R16Unorm, if frame.meta.bit_depth >= 9 && frame.meta.bit_depth <= 16 { 65535.0 / ((1u32 << frame.meta.bit_depth) - 1) as f32 } else { 1.0 }),
                    PlanarLayout::Ycc { .. } => (wgpu::TextureFormat::R8Unorm, 1.0),
                };
                let same = self.planar_tex.len() == planes.len() && self.planar_tex.iter().zip(planes).all(|(t, p)| t.format() == fmt && t.width() == p.width && t.height() == p.height);
                if !same {
                    self.planar_tex = planes.iter().map(|p| self.device.create_texture(&wgpu::TextureDescriptor { label: Some("plane"), size: wgpu::Extent3d { width: p.width, height: p.height, depth_or_array_layers: 1 }, mip_level_count: 1, sample_count: 1, dimension: wgpu::TextureDimension::D2, format: fmt, usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST, view_formats: &[] })).collect();
                }
                for (tex, p) in self.planar_tex.iter().zip(planes) {
                    self.queue.write_texture(wgpu::TexelCopyTextureInfo { texture: tex, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All }, &p.data, wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(p.pitch), rows_per_image: Some(p.height) }, wgpu::Extent3d { width: p.width, height: p.height, depth_or_array_layers: 1 });
                }
                let depth = match layout { PlanarLayout::RgbF16 => 16, PlanarLayout::Ycc { bytes_per_sample: 2 } => frame.meta.bit_depth.clamp(9, 16), _ => 8 };
                self.set_base(Base { yscale, full_range: frame.meta.range == Range::Full, depth, meta: frame.meta });
                if !same || self.shown.as_ref().map(|s| s.variant != Variant::Planar).unwrap_or(true) {
                    let views: Vec<_> = self.planar_tex.iter().map(|t| t.create_view(&Default::default())).collect();
                    let binds = self.binds(Variant::Planar, &views);
                    self.shown = Some(Shown { variant: Variant::Planar, binds });
                }
            }
        }
        self.frames_shown += 1;
        self.last_show_ms = t.elapsed().as_secs_f32() * 1000.0;
        Ok(())
    }
    fn set_base(&mut self, b: Base) {
        let changed = self.base != Some(b);
        if changed { self.base = Some(b); self.write_uniforms(); }
    }
    fn fit(&self) -> [f32; 2] {
        let (vw, vh, ww, wh) = (self.video.0.max(1) as f32, self.video.1.max(1) as f32, self.viewport.0 as f32, self.viewport.1 as f32);
        if vw / vh > ww / wh { [1.0, (ww / wh) / (vw / vh)] } else { [(vw / vh) / (ww / wh), 1.0] }
    }
    fn write_uniforms(&self) {
        let Some(b) = self.base else { return };
        let (trc, mat, prim) = b.meta.selectors();
        let (black, lscale, czero, cscale) = range_constants(b.depth.max(8), b.full_range);
        let u = |fit: [f32; 2], out: OutMode| U { fit, yscale: b.yscale, black, lscale, czero, cscale, src_transfer: trc, src_matrix: mat, src_primaries: prim, out_mode: out.sel(), white: self.display.white };
        // a PQ master on a PQ surface passes through encoded — bit-exact, no EOTF round trip
        let display_out = if self.display.out == OutMode::Pq && b.meta.transfer == Transfer::Pq { OutMode::Encoded } else { self.display.out };
        let chain_in = match self.color_pass.as_ref().map(|c| c.input()) { Some(Space::Encoded(_)) => OutMode::Encoded, _ => OutMode::Linear };
        let sets = [u(self.fit(), display_out), u([1.0, 1.0], chain_in), u([1.0, 1.0], OutMode::Encoded), u([1.0, 1.0], OutMode::Linear)];
        for (buf, v) in self.ubufs.iter().zip(sets.iter()) { self.queue.write_buffer(buf, 0, bytemuck::bytes_of(v)); }
        // the blit reads the chain's output space; `src_matrix` 3 = RGB, no range expansion
        let in_trc = match self.color_pass.as_ref().map(|c| c.output()) { Some(Space::Encoded(t)) => ColorMeta { transfer: t, ..Default::default() }.selectors().0, _ => 4.0 };
        let blit_out = if display_out == OutMode::Encoded { OutMode::Pq } else { display_out };
        self.queue.write_buffer(&self.blit_ubuf, 0, bytemuck::bytes_of(&U { fit: self.fit(), yscale: 1.0, black: 0.0, lscale: 1.0, czero: 0.0, cscale: 1.0, src_transfer: in_trc, src_matrix: 3.0, src_primaries: 0.0, out_mode: blit_out.sel(), white: self.display.white }));
    }
    fn binds(&self, variant: Variant, views: &[wgpu::TextureView]) -> Vec<wgpu::BindGroup> {
        let (layout, ub) = match variant { Variant::Planar => (&self.planar_layout, 4u32), Variant::Biplanar => (&self.biplanar_layout, 3u32) };
        self.ubufs.iter().map(|u| {
            let mut e = vec![wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::Sampler(&self.sampler) }];
            for (i, v) in views.iter().enumerate() { e.push(wgpu::BindGroupEntry { binding: 1 + i as u32, resource: wgpu::BindingResource::TextureView(v) }); }
            e.push(wgpu::BindGroupEntry { binding: ub, resource: u.as_entire_binding() });
            self.device.create_bind_group(&wgpu::BindGroupDescriptor { label: Some("video"), layout, entries: &e })
        }).collect()
    }
    fn target_pipeline(&self, v: Variant) -> &wgpu::RenderPipeline { match v { Variant::Planar => &self.planar_target, Variant::Biplanar => &self.biplanar_target } }
    fn inter_pipeline(&self, v: Variant) -> &wgpu::RenderPipeline { match v { Variant::Planar => &self.planar_inter, Variant::Biplanar => &self.biplanar_inter } }
    fn ensure_inter(&mut self) {
        if self.inter.as_ref().map(|i| i.size == self.video).unwrap_or(false) { return }
        let mk = |label: &str| self.device.create_texture(&wgpu::TextureDescriptor { label: Some(label), size: wgpu::Extent3d { width: self.video.0.max(1), height: self.video.1.max(1), depth_or_array_layers: 1 }, mip_level_count: 1, sample_count: 1, dimension: wgpu::TextureDimension::D2, format: wgpu::TextureFormat::Rgba16Float, usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::COPY_SRC | wgpu::TextureUsages::COPY_DST, view_formats: &[] });
        let (a, b) = (mk("working-in"), mk("working-out"));
        let bv = b.create_view(&Default::default());
        let bind = self.device.create_bind_group(&wgpu::BindGroupDescriptor { label: Some("blit"), layout: &self.blit_layout, entries: &[wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::Sampler(&self.sampler) }, wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&bv) }, wgpu::BindGroupEntry { binding: 2, resource: self.blit_ubuf.as_entire_binding() }] });
        self.inter = Some(Inter { a, b, bind, size: self.video });
    }
    /// Draw the shown frame into `target` (the surface or any `target_format` texture): one fused pass, or stage 1 →
    /// the chain → the blit when a `ColorPass` is bound. Black when nothing is shown.
    pub fn encode(&mut self, enc: &mut wgpu::CommandEncoder, target: &wgpu::TextureView) {
        let Some(variant) = self.shown.as_ref().map(|s| s.variant) else { drop(pass(enc, "clear", target)); return };
        if self.color_pass.is_some() {
            self.ensure_inter();
            let shown = self.shown.as_ref().unwrap(); let inter = self.inter.as_ref().unwrap();
            let av = inter.a.create_view(&Default::default());
            { let mut p = pass(enc, "stage 1", &av); p.set_pipeline(self.inter_pipeline(variant)); p.set_bind_group(0, &shown.binds[1], &[]); p.draw(0..3, 0..1); }
            let Inter { a, b, bind, .. } = self.inter.as_ref().unwrap();
            self.color_pass.as_mut().unwrap().run(enc, a, b);
            let mut p = pass(enc, "stage 3", target); p.set_pipeline(&self.blit_target); p.set_bind_group(0, bind, &[]); p.draw(0..3, 0..1);
        } else {
            let shown = self.shown.as_ref().unwrap();
            let mut p = pass(enc, "present", target); p.set_pipeline(self.target_pipeline(variant)); p.set_bind_group(0, &shown.binds[0], &[]); p.draw(0..3, 0..1);
        }
    }
    /// Render the shown frame 1:1 at the video size into an `Rgba16Float` texture and read it back (RGBA f32).
    pub fn snapshot(&self, which: Snapshot) -> Result<(Vec<f32>, u32, u32), Error> {
        let shown = self.shown.as_ref().ok_or(Error::NoFrame)?;
        let (w, h) = self.video;
        let tex = self.device.create_texture(&wgpu::TextureDescriptor { label: Some("snapshot"), size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 }, mip_level_count: 1, sample_count: 1, dimension: wgpu::TextureDimension::D2, format: wgpu::TextureFormat::Rgba16Float, usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC, view_formats: &[] });
        let view = tex.create_view(&Default::default());
        let mut enc = self.device.create_command_encoder(&Default::default());
        { let mut p = pass(&mut enc, "snapshot", &view); p.set_pipeline(self.inter_pipeline(shown.variant)); p.set_bind_group(0, &shown.binds[match which { Snapshot::Encoded => 2, Snapshot::Linear => 3 }], &[]); p.draw(0..3, 0..1); }
        self.queue.submit([enc.finish()]);
        Ok((readback::read_rgba16f(&self.device, &self.queue, &tex, w, h), w, h))
    }
    /// Render exactly what `encode` would draw into an offscreen `target_format` texture and read it back as RGBA f32
    /// (`Rgba16Float` targets) — the headless route for tests.
    pub fn render_offscreen(&mut self, w: u32, h: u32) -> Result<Vec<f32>, Error> {
        if self.target_format != wgpu::TextureFormat::Rgba16Float { return Err(Error::NoFormat) }
        self.set_viewport(w, h);
        let tex = self.device.create_texture(&wgpu::TextureDescriptor { label: Some("offscreen"), size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 }, mip_level_count: 1, sample_count: 1, dimension: wgpu::TextureDimension::D2, format: self.target_format, usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC, view_formats: &[] });
        let view = tex.create_view(&Default::default());
        let mut enc = self.device.create_command_encoder(&Default::default());
        self.encode(&mut enc, &view);
        self.queue.submit([enc.finish()]);
        Ok(readback::read_rgba16f(&self.device, &self.queue, &tex, w, h))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    /// A synthetic 8-bit 4:2:2 planar frame: a horizontal luma ramp, flat chroma — enough to exercise every stage.
    fn ramp_frame(w: u32, h: u32) -> Frame {
        let plane = |pw: u32, ph: u32, f: &dyn Fn(u32, u32) -> u8| { let pitch = pw.div_ceil(256) * 256; let mut data = vec![0u8; (pitch * ph) as usize]; for y in 0..ph { for x in 0..pw { data[(y * pitch + x) as usize] = f(x, y); } } Plane { data, width: pw, height: ph, pitch } };
        let planes = vec![plane(w, h, &|x, _| (16 + (x * 219) / (w - 1)) as u8), plane(w / 2, h, &|_, _| 128), plane(w / 2, h, &|_, _| 128)];
        Frame { width: w, height: h, pixels: Pixels::Planar { planes, layout: PlanarLayout::Ycc { bytes_per_sample: 1 } }, meta: ColorMeta { bit_depth: 8, ..Default::default() } }
    }
    #[test]
    fn identity_chain_matches_the_fused_route() {
        let Ok(mut e) = Engine::headless(wgpu::TextureFormat::Rgba16Float) else { eprintln!("no GPU — skipped"); return };
        let f = ramp_frame(256, 16);
        e.set_display(DisplayMode { out: OutMode::Linear, white: 100.0 });
        e.show(&f).unwrap();
        let fused = e.render_offscreen(256, 16).unwrap();
        e.set_color_pass(Some(Box::new(IdentityColorPass)));
        let chained = e.render_offscreen(256, 16).unwrap();
        // the working texture is fp16: ≤ 1 code at 8 bit after the sRGB encode, and ≤ 1/2048 in linear
        let worst = fused.iter().zip(&chained).map(|(a, b)| (a - b).abs()).fold(0f32, f32::max);
        assert!(worst <= 1.0 / 2048.0, "fused vs chained worst diff {worst}");
        // the ramp is a ramp: linear light rises monotonically with x, black at code 16, 1.0 at code 235
        let row: Vec<f32> = (0..256).map(|x| fused[x * 4]).collect();
        assert!(row.windows(2).all(|p| p[1] >= p[0] - 1e-4), "not monotonic");
        assert!(row[0].abs() < 2e-3 && (row[255] - 1.0).abs() < 2e-3, "ends {} {}", row[0], row[255]);
        // the encoded snapshot returns the matrix+range output: 0 at code 16, 1 at code 235
        let (enc, _, _) = e.snapshot(Snapshot::Encoded).unwrap();
        assert!(enc[0].abs() < 1e-3 && (enc[255 * 4] - 1.0).abs() < 1e-3, "encoded ends {} {} (len {})", enc[0], enc[255 * 4], enc.len());
        // grey stays grey: neutral chroma (128) gives R = G = B to within fp16 at every code
        for x in 0..256 { let p = &enc[x * 4..x * 4 + 3]; assert!((p[0] - p[1]).abs() < 1.5e-3 && (p[1] - p[2]).abs() < 1.5e-3, "tint at {x}: {p:?}"); }
    }
    #[test]
    fn ten_bit_range_is_exact() {
        // 10-bit limited: code 64 → 0, 940 → 1, chroma 512 neutral — the constants S1 approximated with 8-bit numbers
        let (black, lscale, czero, cscale) = range_constants(10, false);
        let s = |v: f32| v / 1023.0;
        assert!(((s(64.0) - black) * lscale).abs() < 1e-6 && (((s(940.0) - black) * lscale) - 1.0).abs() < 1e-6);
        assert!((s(512.0) - czero).abs() < 1e-7 && (((s(960.0) - czero) * cscale) - 0.5).abs() < 1e-6, "chroma {} {}", s(512.0) - czero, (s(960.0) - czero) * cscale);
        let (b8, l8, c8, _) = range_constants(8, false);
        assert!(((16.0 / 255.0 - b8) * l8).abs() < 1e-6 && (((235.0 / 255.0 - b8) * l8) - 1.0).abs() < 1e-6 && (c8 - 128.0 / 255.0).abs() < 1e-7);
    }
    #[test]
    fn sdr_modes_clip_and_encode() {
        let Ok(mut e) = Engine::headless(wgpu::TextureFormat::Rgba16Float) else { return };
        let f = ramp_frame(64, 4);
        e.set_display(DisplayMode { out: OutMode::SdrEncode, white: 100.0 }); e.show(&f).unwrap();
        let encoded = e.render_offscreen(64, 4).unwrap();
        e.set_display(DisplayMode { out: OutMode::SdrLinear, white: 100.0 });
        let linear = e.render_offscreen(64, 4).unwrap();
        // sRGB-encoded ≥ linear everywhere in (0, 1); both end at 1.0
        for x in 1..63 { assert!(encoded[x * 4] >= linear[x * 4] - 1e-3); }
        assert!((encoded[63 * 4] - 1.0).abs() < 2e-3 && (linear[63 * 4] - 1.0).abs() < 2e-3);
    }
}
