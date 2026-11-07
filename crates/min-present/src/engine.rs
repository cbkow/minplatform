//! The engine: the fused graph, the stage-2 slot, the shown frame, the held ring, snapshots. Surface-free — the
//! `Presenter` owns the surface; tests run it headless against an offscreen target.
use crate::{frame::*, import::{Imported, Imports}, readback, Error};
use std::{any::Any, collections::VecDeque, sync::Arc, time::Instant};

#[repr(C)] #[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct U { fit: [f32; 2], code_max: f32, level_k: f32, full_max: f32, chroma_mid: f32, full_range: f32, kr: f32, kb: f32, src_transfer: f32, src_primaries: f32, out_mode: f32, white: f32, is_rgb: f32, rgb_expand: f32, /// display rotation in quarter turns CW (the sampler inverse-rotates; QCView's `rotatedSrcUv`)
    rot: f32, off: [f32; 2], pad1: [f32; 2],
    /// stage 0, the background (row 153): mode / bounds-mix strength / checker tile px; the media rect in px (y down); the viewport size
    bg: [f32; 4], media: [f32; 4], vp: [f32; 4] }

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
    Linear, /// what the screen gets: the display path 1:1 (the transfer override, bypass-as-stored and stage 3 applied) in the surface's encoding
    Display }

/// QCView's per-clip `TransferOverride` (the Inspector's Transfer chips): how the clip is READ when its tags are missing or wrong.
/// A decode-level fact, not an interpretation — in bypass it only decides SDR vs PQ / HLG / linear decoding (and the scopes); the
/// curve question (sRGB 2.2 vs BT.1886 2.4) belongs to the chain's Input (canon §18.6, 2026-11-07 night).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)] pub enum TransferChoice { #[default] Auto, Sdr709, Pq2020, PqP3, Hlg, Linear }

/// The stage-1 pipelines: QCView's three kernels (planar, biplanar, packed 4444) plus the alpha-plane planar form.
#[derive(Clone, Copy, PartialEq, Eq, Debug)] enum Variant { Planar3, Planar4, Biplanar, Packed }
impl Variant { fn textures(self) -> u32 { match self { Variant::Planar3 => 3, Variant::Planar4 => 4, Variant::Biplanar => 2, Variant::Packed => 1 } } }
const N_U: usize = 5;   // 0 display · 1 chain-in (1:1) · 2 encoded 1:1 · 3 linear 1:1 · 4 display 1:1 (the display snapshot)

/// QCView's `YuvPlanarDesc` constants for the shader: levels in code values for any depth / alignment, the matrix as Kr / Kb.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Levels { pub code_max: f32, pub level_k: f32, pub full_max: f32, pub chroma_mid: f32, pub full_range: bool, pub kr: f32, pub kb: f32, pub is_rgb: bool, pub rgb_expand: bool }
impl Levels {
    /// `code_bits` = depth + shift (8, 10, 12, 16 …); `bytes` 1 or 2 selects the texture's code_max (255 / 65535).
    pub fn new(code_bits: u32, bytes: u32, full_range: bool, matrix: Matrix) -> Levels {
        let (kr, kb) = matrix.kr_kb();
        Levels { code_max: if bytes == 2 { 65535.0 } else { 255.0 }, level_k: (1u32 << code_bits.saturating_sub(8)) as f32, full_max: ((1u64 << code_bits) - 1) as f32, chroma_mid: (1u32 << (code_bits - 1)) as f32, full_range, kr, kb, is_rgb: matrix == Matrix::Identity, rgb_expand: false }
    }
    /// Float planar RGB (EXR): the samples are already 0..1-ish linear values.
    pub fn float_rgb() -> Levels { Levels { code_max: 1.0, level_k: 1.0, full_max: 1.0, chroma_mid: 0.0, full_range: true, kr: 0.2126, kb: 0.0722, is_rgb: true, rgb_expand: false } }
}
#[derive(Clone, Copy, PartialEq)] struct Base { levels: Levels, meta: ColorMeta }
struct Shown { variant: Variant, binds: Vec<wgpu::BindGroup> }
struct Held { _imp: Imported, _keep: Arc<dyn Any + Send + Sync> }
struct Inter { a: wgpu::Texture, b: wgpu::Texture, bind: wgpu::BindGroup, size: (u32, u32) }

pub struct Engine {
    /// No presenter will submit after `show` (a headless engine): flush + maintain per show.
    pub standalone: bool,
    pub device: wgpu::Device, pub queue: wgpu::Queue,
    pub target_format: wgpu::TextureFormat,
    sampler: wgpu::Sampler,
    layouts: Vec<wgpu::BindGroupLayout>, blit_layout: wgpu::BindGroupLayout,   // per Variant
    targets: Vec<wgpu::RenderPipeline>, inters: Vec<wgpu::RenderPipeline>, blit_target: wgpu::RenderPipeline,
    ubufs: Vec<wgpu::Buffer>, blit_ubuf: wgpu::Buffer,
    /// Stage 4: the UI layer's pipeline (blended), its uniform, and the bind group for the overlay texture when one is set.
    overlay_pipeline: wgpu::RenderPipeline, overlay_ubuf: wgpu::Buffer, overlay: Option<wgpu::BindGroup>,
    /// Stage 0: the viewport background under the picture's alpha (row 153): QCView's four modes, the bounds mix per mode, the checker tile in device px.
    bg_pipeline: wgpu::RenderPipeline, bg_ubuf: wgpu::Buffer, bg_bind: wgpu::BindGroup, bg_mode: u8, bg_mix: [f32; 4], ui_scale: f32,
    geometry: Geometry,
    transfer_override: TransferChoice,
    video: (u32, u32), viewport: (u32, u32), display: DisplayMode, base: Option<Base>,
    /// Where the video fits inside the viewport (physical px; `None` = the whole viewport): the shell's bands take the rest.
    content: Option<(u32, u32, u32, u32)>,
    planar_tex: Vec<wgpu::Texture>,
    shown: Option<Shown>, held: VecDeque<Held>,
    color_pass: Option<Box<dyn ColorPass>>, inter: Option<Inter>,
    imports: Imports, imports_n: u32,
    /// The last `show`'s import / upload cost on the calling thread, ms.
    pub last_show_ms: f32,
    pub frames_shown: u64,
}

/// The pixel-aspect choice (QCView's Aspect pill, per clip): Square renders the stored pixels untouched (the default —
/// QC-safe), Detected applies the file's SAR, Custom a given ratio.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Par { #[default] Square, Detected, Custom(u32, u32) }
/// Geometry applied in the fit (QCView `effDim` + the rotation swap): PAR widens the larger-pixel axis in STORED space,
/// then the display rotation (`None` = the file's display matrix, `Some(deg)` = forced) swaps the dims for 90 / 270 and
/// the sampler inverse-rotates.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Geometry { pub par: Par, pub rotation: Option<u16> }

const HELD: usize = 3;   // the GPU is ≤ 2 frames behind (frame latency 2); S1's ring

fn make_pipeline(device: &wgpu::Device, shader: &wgpu::ShaderModule, layout: &wgpu::PipelineLayout, format: wgpu::TextureFormat, label: &str) -> wgpu::RenderPipeline {
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor { label: Some(label), layout: Some(layout),
        vertex: wgpu::VertexState { module: shader, entry_point: Some("vs"), buffers: &[], compilation_options: Default::default() },
        fragment: Some(wgpu::FragmentState { module: shader, entry_point: Some("fs"), targets: &[Some(wgpu::ColorTargetState { format, blend: None, write_mask: wgpu::ColorWrites::ALL })], compilation_options: Default::default() }),
        primitive: Default::default(), depth_stencil: None, multisample: Default::default(), multiview_mask: None, cache: None })
}
fn make_pipeline_blended(device: &wgpu::Device, shader: &wgpu::ShaderModule, layout: &wgpu::PipelineLayout, format: wgpu::TextureFormat, label: &str) -> wgpu::RenderPipeline {
    // premultiplied source over: out = src + dst × (1 − src.a)
    let blend = wgpu::BlendState { color: wgpu::BlendComponent { src_factor: wgpu::BlendFactor::One, dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha, operation: wgpu::BlendOperation::Add }, alpha: wgpu::BlendComponent { src_factor: wgpu::BlendFactor::One, dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha, operation: wgpu::BlendOperation::Add } };
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor { label: Some(label), layout: Some(layout),
        vertex: wgpu::VertexState { module: shader, entry_point: Some("vs"), buffers: &[], compilation_options: Default::default() },
        fragment: Some(wgpu::FragmentState { module: shader, entry_point: Some("fs"), targets: &[Some(wgpu::ColorTargetState { format, blend: Some(blend), write_mask: wgpu::ColorWrites::ALL })], compilation_options: Default::default() }),
        primitive: Default::default(), depth_stencil: None, multisample: Default::default(), multiview_mask: None, cache: None })
}
fn pass_load<'a>(enc: &'a mut wgpu::CommandEncoder, label: &'static str, view: &'a wgpu::TextureView) -> wgpu::RenderPass<'a> {
    let att = wgpu::RenderPassColorAttachment { view, depth_slice: None, resolve_target: None, ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store } };
    enc.begin_render_pass(&wgpu::RenderPassDescriptor { label: Some(label), color_attachments: &[Some(att)], depth_stencil_attachment: None, timestamp_writes: None, occlusion_query_set: None, multiview_mask: None })
}
fn attach(view: &wgpu::TextureView) -> wgpu::RenderPassColorAttachment<'_> { wgpu::RenderPassColorAttachment { view, depth_slice: None, resolve_target: None, ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store } } }
fn pass<'a>(enc: &'a mut wgpu::CommandEncoder, label: &'static str, view: &'a wgpu::TextureView) -> wgpu::RenderPass<'a> {
    enc.begin_render_pass(&wgpu::RenderPassDescriptor { label: Some(label), color_attachments: &[Some(attach(view))], depth_stencil_attachment: None, timestamp_writes: None, occlusion_query_set: None, multiview_mask: None })
}

/// The letterbox fit of `video` inside `content` (or the whole `viewport`) as the fullscreen triangle's NDC scale and
/// offset: `pos = p × scale + offset`. Pure, so the maths is testable without a GPU.
pub fn fit_in(video: (u32, u32), viewport: (u32, u32), content: Option<(u32, u32, u32, u32)>) -> ([f32; 2], [f32; 2]) {
    let (vw, vh) = (video.0.max(1) as f32, video.1.max(1) as f32);
    let (ww, wh) = (viewport.0.max(1) as f32, viewport.1.max(1) as f32);
    let (cx, cy, cw, ch) = match content { Some((x, y, w, h)) => (x as f32, y as f32, w.max(1) as f32, h.max(1) as f32), None => (0.0, 0.0, ww, wh) };
    let s = if vw / vh > cw / ch { [1.0, (cw / ch) / (vw / vh)] } else { [(vw / vh) / (cw / ch), 1.0] };
    let scale = [s[0] * cw / ww, s[1] * ch / wh];
    let off = [(cx + cw / 2.0) / ww * 2.0 - 1.0, 1.0 - (cy + ch / 2.0) / wh * 2.0];
    (scale, off)
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
        let bodies = [include_str!("shaders/planar3.wgsl"), include_str!("shaders/planar4.wgsl"), include_str!("shaders/biplanar.wgsl"), include_str!("shaders/packed.wgsl")];
        let variants = [Variant::Planar3, Variant::Planar4, Variant::Biplanar, Variant::Packed];
        let tex = |b| wgpu::BindGroupLayoutEntry { binding: b, visibility: wgpu::ShaderStages::FRAGMENT, ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: true }, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false }, count: None };
        let smp = wgpu::BindGroupLayoutEntry { binding: 0, visibility: wgpu::ShaderStages::FRAGMENT, ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering), count: None };
        let ub = |b| wgpu::BindGroupLayoutEntry { binding: b, visibility: wgpu::ShaderStages::VERTEX_FRAGMENT, ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None }, count: None };
        let inter_fmt = wgpu::TextureFormat::Rgba16Float;
        let (mut layouts, mut targets, mut inters) = (vec![], vec![], vec![]);
        for (v, body) in variants.iter().zip(bodies) {
            let n = v.textures();
            let mut entries = vec![smp]; for i in 0..n { entries.push(tex(1 + i)); } entries.push(ub(1 + n));
            let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: Some("stage 1"), entries: &entries });
            let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: None, bind_group_layouts: &[Some(&layout)], immediate_size: 0 });
            let sh = module(&format!("{v:?}"), body);
            targets.push(make_pipeline_blended(&device, &sh, &pl, target_format, "stage 1+3 → target"));   // over the background by the frame's alpha (row 153)
            inters.push(make_pipeline(&device, &sh, &pl, inter_fmt, "stage 1 → working"));
            layouts.push(layout);
        }
        let blit = module("blit", include_str!("shaders/blit.wgsl"));
        let blit_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: Some("blit"), entries: &[smp, tex(1), ub(2)] });
        let lpl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: None, bind_group_layouts: &[Some(&blit_layout)], immediate_size: 0 });
        let blit_target = make_pipeline_blended(&device, &blit, &lpl, target_format, "blit→target");
        let bg_sh = module("background", include_str!("shaders/background.wgsl"));
        let bg_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: Some("background"), entries: &[ub(0)] });
        let bg_pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: None, bind_group_layouts: &[Some(&bg_layout)], immediate_size: 0 });
        let bg_pipeline = make_pipeline(&device, &bg_sh, &bg_pl, target_format, "background→target");
        let overlay_sh = module("overlay", include_str!("shaders/overlay.wgsl"));
        let overlay_pipeline = make_pipeline_blended(&device, &overlay_sh, &lpl, target_format, "overlay→target");   // the blit's layout: sampler, one texture, U
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor { label: Some("video"), mag_filter: wgpu::FilterMode::Linear, min_filter: wgpu::FilterMode::Linear, ..Default::default() });
        let ubuf = |label: &str| device.create_buffer(&wgpu::BufferDescriptor { label: Some(label), size: std::mem::size_of::<U>() as u64, usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
        let ubufs = (0..N_U).map(|i| ubuf(&format!("u{i}"))).collect();
        let blit_ubuf = ubuf("u-blit");
        let overlay_ubuf = ubuf("u-overlay");
        let bg_ubuf = ubuf("u-background");
        let bg_bind = device.create_bind_group(&wgpu::BindGroupDescriptor { label: Some("background"), layout: &bg_layout, entries: &[wgpu::BindGroupEntry { binding: 0, resource: bg_ubuf.as_entire_binding() }] });
        // QCView's media-bounds policy: outside the media rect the fill mixes toward Theme.bg by these (black / darkgray / darkChecker / lightChecker); `QCV_BOUNDS_MIX` = one value for all four, or four comma-separated
        let mut bg_mix = [0.0f32, 1.0, 0.7, 0.7];
        if let Ok(env) = std::env::var("QCV_BOUNDS_MIX") { let parts: Vec<f32> = env.split(',').filter_map(|p| p.trim().parse().ok()).map(|v: f32| v.clamp(0.0, 1.0)).collect(); if parts.len() == 1 { bg_mix = [parts[0]; 4]; } else { for (i, v) in parts.iter().take(4).enumerate() { bg_mix[i] = *v; } } log::info!("background: QCV_BOUNDS_MIX override → {bg_mix:?}"); }
        let imports = Imports::new(&device, &queue);
        let e = Engine { device, queue, target_format, sampler, layouts, blit_layout, targets, inters, blit_target, ubufs, blit_ubuf, overlay_pipeline, overlay_ubuf, overlay: None, bg_pipeline, bg_ubuf, bg_bind, bg_mode: 0, bg_mix, ui_scale: 1.0, geometry: Geometry::default(), transfer_override: TransferChoice::Auto,
            video: (0, 0), viewport: (1, 1), display: DisplayMode::default(), base: None, content: None, planar_tex: vec![], shown: None, held: VecDeque::new(), color_pass: None, inter: None, imports, imports_n: 0, last_show_ms: 0.0, frames_shown: 0, standalone: false };
        e.write_overlay_uniform(); e.write_bg_uniform();
        e
    }
    /// An engine with no surface (tests, offscreen renders): the first high-performance adapter.
    pub fn headless(target_format: wgpu::TextureFormat) -> Result<Engine, Error> {
        let instance = crate::presenter::Presenter::instance();   // DX12 on Windows (the hal the zero-copy import needs), the platform default elsewhere
        let (_, device, queue) = request(&instance, None)?;
        let mut e = Engine::new(device, queue, target_format); e.standalone = true;
        Ok(e)
    }
    pub fn zero_copy_available(&self) -> bool { self.imports.available() }
    pub fn set_viewport(&mut self, w: u32, h: u32) { self.viewport = (w.max(1), h.max(1)); self.write_uniforms(); }
    pub fn viewport(&self) -> (u32, u32) { self.viewport }
    /// The rect the video is letterboxed into (x, y, w, h in physical px within the viewport); `None` = the whole viewport.
    pub fn set_content_rect(&mut self, r: Option<(u32, u32, u32, u32)>) { self.content = r; self.write_uniforms(); }
    pub fn content_rect(&self) -> Option<(u32, u32, u32, u32)> { self.content }
    pub fn video_size(&self) -> (u32, u32) { self.video }
    pub fn set_display(&mut self, d: DisplayMode) { self.display = d; self.write_uniforms(); }
    pub fn display(&self) -> DisplayMode { self.display }
    pub fn set_color_pass(&mut self, cp: Option<Box<dyn ColorPass>>) { self.color_pass = cp; self.inter = None; self.write_uniforms(); }
    pub fn has_color_pass(&self) -> bool { self.color_pass.is_some() }
    pub fn has_frame(&self) -> bool { self.shown.is_some() }
    /// Stage 4: composite `view` (an `Rgba8Unorm` texture at the viewport size — vello's output: sRGB-encoded, straight
    /// alpha) over every `encode` until the next call; `None` removes it. UI white lands on the display's SDR reference
    /// white in every output mode. Call again after the texture is re-created (a resize).
    pub fn set_overlay(&mut self, view: Option<&wgpu::TextureView>) {
        self.overlay = view.map(|v| self.device.create_bind_group(&wgpu::BindGroupDescriptor { label: Some("overlay"), layout: &self.blit_layout, entries: &[
            wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::Sampler(&self.sampler) },
            wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(v) },
            wgpu::BindGroupEntry { binding: 2, resource: self.overlay_ubuf.as_entire_binding() }] }));
    }
    pub fn has_overlay(&self) -> bool { self.overlay.is_some() }
    /// The clip's geometry choices (QCView's Aspect + Rotation pills).
    pub fn set_geometry(&mut self, g: Geometry) { if self.geometry != g { self.geometry = g; self.write_uniforms(); } }
    /// The clip's Transfer override (QCView's chips). `Sdr709` reads the pixels as display-referred SDR in 709; the PQ / HLG choices
    /// carry their primaries; `Linear` keeps the tagged primaries.
    pub fn set_transfer_override(&mut self, t: TransferChoice) { if self.transfer_override != t { self.transfer_override = t; self.write_uniforms(); } }
    pub fn transfer_override(&self) -> TransferChoice { self.transfer_override }
    fn effective_meta(&self, m: ColorMeta) -> ColorMeta {
        match self.transfer_override {
            TransferChoice::Auto => m,
            TransferChoice::Sdr709 => ColorMeta { transfer: Transfer::Bt1886, primaries: Primaries::Bt709, ..m },
            TransferChoice::Pq2020 => ColorMeta { transfer: Transfer::Pq, primaries: Primaries::Bt2020, ..m },
            TransferChoice::PqP3 => ColorMeta { transfer: Transfer::Pq, primaries: Primaries::P3D65, ..m },
            TransferChoice::Hlg => ColorMeta { transfer: Transfer::Hlg, primaries: Primaries::Bt2020, ..m },
            TransferChoice::Linear => ColorMeta { transfer: Transfer::Linear, ..m },
        }
    }
    pub fn geometry(&self) -> Geometry { self.geometry }
    /// The rotation in quarter turns CW that applies now (forced, else the frame's display matrix).
    pub fn rotation_quarters(&self) -> u8 {
        let deg = self.geometry.rotation.unwrap_or(self.base.map(|b| b.meta.rotation).unwrap_or(0));
        match deg { 90 => 1, 180 => 2, 270 => 3, _ => 0 }
    }
    /// The dims the picture is fitted with: QCView's `effDim` (PAR widens the larger-pixel axis) then the rotation swap.
    pub fn display_dims(&self) -> (u32, u32) {
        let (w, h) = self.video;
        let (n, d) = match self.geometry.par { Par::Square => (1, 1), Par::Detected => self.base.map(|b| b.meta.sar).unwrap_or((1, 1)), Par::Custom(n, d) => (n, d) };
        let (ew, eh) = if w == 0 || h == 0 || n == 0 || d == 0 || n == d { (w, h) } else if n > d { ((w as f64 * n as f64 / d as f64).round() as u32, h) } else { (w, (h as f64 * d as f64 / n as f64).round() as u32) };
        if self.rotation_quarters() & 1 == 1 { (eh, ew) } else { (ew, eh) }
    }
    /// The viewport background mode (QCView `display/backgroundMode`: 0 Black, 1 DarkGray, 2 DarkChecker, 3 LightChecker).
    pub fn set_background(&mut self, mode: u8) { self.bg_mode = mode.min(3); self.write_bg_uniform(); }
    pub fn background(&self) -> u8 { self.bg_mode }
    /// The window's scale factor: the checker tile is 32 DEVICE pixels (QCView's Metal path passes the contentsScale-aware size).
    pub fn set_ui_scale(&mut self, scale: f32) { self.ui_scale = scale.max(0.5); self.write_bg_uniform(); }
    /// The media rect in viewport px (x, y, w, h; y down) from the fit; the whole viewport when nothing is shown (QCView: no layout → everything "inside").
    fn media_rect_px(&self) -> [f32; 4] {
        let (vw, vh) = (self.viewport.0 as f32, self.viewport.1 as f32);
        if self.base.is_none() { return [0.0, 0.0, vw, vh] }
        let (fit, off) = fit_in(self.display_dims(), self.viewport, self.content);
        let x0 = (off[0] - fit[0] + 1.0) * 0.5 * vw; let x1 = (off[0] + fit[0] + 1.0) * 0.5 * vw;
        let y0 = (1.0 - (off[1] + fit[1])) * 0.5 * vh; let y1 = (1.0 - (off[1] - fit[1])) * 0.5 * vh;
        [x0.round(), y0.round(), (x1 - x0).round(), (y1 - y0).round()]
    }
    fn write_bg_uniform(&self) {
        let out = if self.display.out == OutMode::Encoded { OutMode::Pq } else { self.display.out };
        let m = self.bg_mode as usize;
        self.queue.write_buffer(&self.bg_ubuf, 0, bytemuck::bytes_of(&U { fit: [1.0, 1.0], off: [0.0, 0.0], pad1: [0.0, 0.0], code_max: 1.0, level_k: 1.0, full_max: 1.0, chroma_mid: 0.0, full_range: 1.0, kr: 0.2126, kb: 0.0722, src_transfer: 0.0, src_primaries: 0.0, out_mode: out.sel(), white: self.display.white, is_rgb: 0.0, rgb_expand: 0.0, rot: 0.0,
            bg: [self.bg_mode as f32, self.bg_mix[m], 32.0 * self.ui_scale, 0.0], media: self.media_rect_px(), vp: [self.viewport.0 as f32, self.viewport.1 as f32, 0.0, 0.0] }));
    }
    fn write_overlay_uniform(&self) {
        // the UI never passes through "encoded": a PQ-master-on-PQ-surface passthrough still PQ-encodes the UI's linear light
        let out = if self.display.out == OutMode::Encoded { OutMode::Pq } else { self.display.out };
        self.queue.write_buffer(&self.overlay_ubuf, 0, bytemuck::bytes_of(&U { fit: [1.0, 1.0], off: [0.0, 0.0], pad1: [0.0, 0.0], code_max: 1.0, level_k: 1.0, full_max: 1.0, chroma_mid: 0.0, full_range: 1.0, kr: 0.2126, kb: 0.0722, src_transfer: 0.0, src_primaries: 0.0, out_mode: out.sel(), white: self.display.white, is_rgb: 0.0, rgb_expand: 0.0, rot: 0.0, bg: [0.0; 4], media: [0.0; 4], vp: [0.0; 4] }));
    }
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
                let mut meta = frame.meta;
                if let Some(m) = imp.matrix { meta.matrix = m; }   // the buffer's attachments win over the stream tags (QCView's bridge)
                if let Some(t) = imp.transfer { meta.transfer = t; }
                let full_range = if frame.meta.range_forced { frame.meta.range == Range::Full } else { imp.full_range.unwrap_or(frame.meta.range == Range::Full) };
                self.set_base(Base { levels: Levels::new(imp.code_bits, if imp.code_bits > 8 { 2 } else { 1 }, full_range, meta.matrix), meta });
                let variant = if imp.packed { Variant::Packed } else { Variant::Biplanar };
                let views: Vec<_> = imp.textures.iter().map(|t| t.create_view(&Default::default())).collect();
                let binds = self.binds(variant, &views);
                self.shown = Some(Shown { variant, binds });
                self.held.push_back(Held { _imp: imp, _keep: nf.keep.clone() });
                while self.held.len() > HELD { self.held.pop_front(); }
                self.imports_n += 1; if self.imports_n % 60 == 0 { self.imports.flush(); }
            }
            Pixels::Planar { planes, layout } => {
                let (levels, variant, fmts): (Levels, Variant, Vec<wgpu::TextureFormat>) = match *layout {
                    PlanarLayout::Rgb { bytes_per_sample, code_bits, float, alpha, legal_expand } => {
                        let mut l = if float { Levels::float_rgb() } else { Levels::new(code_bits, bytes_per_sample, true, Matrix::Identity) };
                        l.rgb_expand = legal_expand && !float;
                        let f = if float { wgpu::TextureFormat::R16Float } else if bytes_per_sample == 2 { wgpu::TextureFormat::R16Unorm } else { wgpu::TextureFormat::R8Unorm };
                        (l, if alpha { Variant::Planar4 } else { Variant::Planar3 }, vec![f; if alpha { 4 } else { 3 }])
                    }
                    PlanarLayout::Ycc { bytes_per_sample, code_bits, interleaved_chroma, alpha } => {
                        let l = Levels::new(code_bits, bytes_per_sample, frame.meta.range == Range::Full, frame.meta.matrix);
                        let (one, two) = if bytes_per_sample == 2 { (wgpu::TextureFormat::R16Unorm, wgpu::TextureFormat::Rg16Unorm) } else { (wgpu::TextureFormat::R8Unorm, wgpu::TextureFormat::Rg8Unorm) };
                        if interleaved_chroma { (l, Variant::Biplanar, vec![one, two]) }   // an alpha plane with interleaved chroma has no FFmpeg format
                        else if alpha { (l, Variant::Planar4, vec![one; 4]) } else { (l, Variant::Planar3, vec![one; 3]) }
                    }
                };
                if planes.len() != fmts.len() { return Err(Error::Import(format!("{} planes for {variant:?}", planes.len()))) }
                let same = self.planar_tex.len() == planes.len() && self.planar_tex.iter().zip(planes).zip(&fmts).all(|((t, p), f)| t.format() == *f && t.width() == p.width && t.height() == p.height);
                if !same {
                    self.planar_tex = planes.iter().zip(&fmts).map(|(p, f)| self.device.create_texture(&wgpu::TextureDescriptor { label: Some("plane"), size: wgpu::Extent3d { width: p.width, height: p.height, depth_or_array_layers: 1 }, mip_level_count: 1, sample_count: 1, dimension: wgpu::TextureDimension::D2, format: *f, usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST, view_formats: &[] })).collect();
                }
                for (tex, p) in self.planar_tex.iter().zip(planes) {
                    self.queue.write_texture(wgpu::TexelCopyTextureInfo { texture: tex, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All }, &p.data, wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(p.pitch), rows_per_image: Some(p.height) }, wgpu::Extent3d { width: p.width, height: p.height, depth_or_array_layers: 1 });
                }
                self.set_base(Base { levels, meta: frame.meta });
                if !same || self.shown.as_ref().map(|s| s.variant != variant).unwrap_or(true) {
                    let views: Vec<_> = self.planar_tex.iter().map(|t| t.create_view(&Default::default())).collect();
                    let binds = self.binds(variant, &views);
                    self.shown = Some(Shown { variant, binds });
                }
            }
        }
        // `write_texture` queues its copy (and a wgpu-owned staging buffer) on the queue's pending-writes encoder, which only a
        // `submit` flushes and only a maintain reclaims. A window's draw submits every refresh; a headless engine showing
        // without drawing never did (4 GB after 120 software 4K shows). An empty submit + poll here keeps staging at one frame.
        if self.standalone { self.queue.submit(std::iter::empty()); let _ = self.device.poll(wgpu::PollType::Poll); }
        self.frames_shown += 1;
        self.last_show_ms = t.elapsed().as_secs_f32() * 1000.0;
        Ok(())
    }
    fn set_base(&mut self, b: Base) {
        let changed = self.base != Some(b);
        if changed { self.base = Some(b); self.write_uniforms(); }
    }
    fn fit(&self) -> [f32; 2] { fit_in(self.display_dims(), self.viewport, self.content).0 }
    fn offset(&self) -> [f32; 2] { fit_in(self.display_dims(), self.viewport, self.content).1 }
    fn write_uniforms(&self) {
        self.write_overlay_uniform(); self.write_bg_uniform();
        let Some(b) = self.base else { return };
        let meta = self.effective_meta(b.meta);
        let (trc, _, prim) = meta.selectors();
        let l = b.levels;
        let u = |fit: [f32; 2], off: [f32; 2], out: OutMode| U { fit, off, pad1: [0.0, 0.0], code_max: l.code_max, level_k: l.level_k, full_max: l.full_max, chroma_mid: l.chroma_mid, full_range: l.full_range as u8 as f32, kr: l.kr, kb: l.kb, src_transfer: trc, src_primaries: prim, out_mode: out.sel(), white: self.display.white, is_rgb: l.is_rgb as u8 as f32, rgb_expand: l.rgb_expand as u8 as f32, rot: 0.0, bg: [0.0; 4], media: [0.0; 4], vp: [0.0; 4] };
        // a PQ master on a PQ surface passes through encoded — bit-exact, no EOTF round trip
        let display_out = if self.display.out == OutMode::Pq && meta.transfer == Transfer::Pq { OutMode::Encoded } else { self.display.out };
        let chain_in = match self.color_pass.as_ref().map(|c| c.input()) { Some(Space::Encoded(_)) => OutMode::Encoded, _ => OutMode::Linear };
        let rot = self.rotation_quarters() as f32;
        let mut sets = [u(self.fit(), self.offset(), display_out), u([1.0, 1.0], [0.0, 0.0], chain_in), u([1.0, 1.0], [0.0, 0.0], OutMode::Encoded), u([1.0, 1.0], [0.0, 0.0], OutMode::Linear), u([1.0, 1.0], [0.0, 0.0], display_out)];
        sets[0].rot = rot;
        // BYPASS = AS STORED (canon row 180, QCView's rule): with no chain engaged, display-referred SDR (sRGB / BT.1886 tags) is
        // linearised with the sRGB EOTF so the surface's encode is a no-op and the screen shows the stored values — on the EDR
        // layer as on an 8-bit sRGB surface. NOT a transfer opinion: the 2.2-vs-2.4 question is the chain's Input, which still
        // receives the tagged transfer (set 1). PQ / HLG / linear sources decode to nits as before (the open point, §18.6).
        if self.color_pass.is_none() && matches!(meta.transfer, Transfer::Srgb | Transfer::Bt1886) { sets[0].src_transfer = 0.0; sets[4].src_transfer = 0.0; }
        for (buf, v) in self.ubufs.iter().zip(sets.iter()) { self.queue.write_buffer(buf, 0, bytemuck::bytes_of(v)); }
        // the blit reads the chain's output space (linear, or encoded with a transfer)
        let in_trc = match self.color_pass.as_ref().map(|c| c.output()) { Some(Space::Encoded(t)) => ColorMeta { transfer: t, ..Default::default() }.selectors().0, _ => 4.0 };
        let blit_out = if display_out == OutMode::Encoded { OutMode::Pq } else { display_out };
        let rotq = self.rotation_quarters() as f32;
        self.queue.write_buffer(&self.blit_ubuf, 0, bytemuck::bytes_of(&U { fit: self.fit(), off: self.offset(), pad1: [0.0, 0.0], code_max: 1.0, level_k: 1.0, full_max: 1.0, chroma_mid: 0.0, full_range: 1.0, kr: 0.2126, kb: 0.0722, src_transfer: in_trc, src_primaries: 0.0, out_mode: blit_out.sel(), white: self.display.white, is_rgb: 1.0, rgb_expand: 0.0, rot: rotq, bg: [0.0; 4], media: [0.0; 4], vp: [0.0; 4] }));
    }
    fn binds(&self, variant: Variant, views: &[wgpu::TextureView]) -> Vec<wgpu::BindGroup> {
        let (layout, ub) = (&self.layouts[variant as usize], 1 + variant.textures());
        self.ubufs.iter().map(|u| {
            let mut e = vec![wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::Sampler(&self.sampler) }];
            for (i, v) in views.iter().enumerate() { e.push(wgpu::BindGroupEntry { binding: 1 + i as u32, resource: wgpu::BindingResource::TextureView(v) }); }
            e.push(wgpu::BindGroupEntry { binding: ub, resource: u.as_entire_binding() });
            self.device.create_bind_group(&wgpu::BindGroupDescriptor { label: Some("video"), layout, entries: &e })
        }).collect()
    }
    fn target_pipeline(&self, v: Variant) -> &wgpu::RenderPipeline { &self.targets[v as usize] }
    fn inter_pipeline(&self, v: Variant) -> &wgpu::RenderPipeline { &self.inters[v as usize] }
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
        self.encode_video(enc, target);
        if let Some(bg) = &self.overlay {
            let mut p = pass_load(enc, "stage 4 overlay", target); p.set_pipeline(&self.overlay_pipeline); p.set_bind_group(0, bg, &[]); p.draw(0..6, 0..1);
        }
    }
    fn encode_video(&mut self, enc: &mut wgpu::CommandEncoder, target: &wgpu::TextureView) {
        // stage 0: the background over the whole viewport (clears it); the picture passes load and blend over it
        { let mut p = pass(enc, "stage 0 background", target); p.set_pipeline(&self.bg_pipeline); p.set_bind_group(0, &self.bg_bind, &[]); p.draw(0..6, 0..1); }
        let Some(variant) = self.shown.as_ref().map(|s| s.variant) else { return };
        if self.color_pass.is_some() {
            self.ensure_inter();
            let shown = self.shown.as_ref().unwrap(); let inter = self.inter.as_ref().unwrap();
            let av = inter.a.create_view(&Default::default());
            { let mut p = pass(enc, "stage 1", &av); p.set_pipeline(self.inter_pipeline(variant)); p.set_bind_group(0, &shown.binds[1], &[]); p.draw(0..6, 0..1); }
            let Inter { a, b, bind, .. } = self.inter.as_ref().unwrap();
            self.color_pass.as_mut().unwrap().run(enc, a, b);
            let mut p = pass_load(enc, "stage 3", target); p.set_pipeline(&self.blit_target); p.set_bind_group(0, bind, &[]); p.draw(0..6, 0..1);
        } else {
            let shown = self.shown.as_ref().unwrap();
            let mut p = pass_load(enc, "present", target); p.set_pipeline(self.target_pipeline(variant)); p.set_bind_group(0, &shown.binds[0], &[]); p.draw(0..6, 0..1);
        }
    }
    /// Render the shown frame 1:1 at the video size into an `Rgba16Float` texture and read it back (RGBA f32).
    pub fn snapshot(&self, which: Snapshot) -> Result<(Vec<f32>, u32, u32), Error> {
        let shown = self.shown.as_ref().ok_or(Error::NoFrame)?;
        let (w, h) = self.video;
        let tex = self.device.create_texture(&wgpu::TextureDescriptor { label: Some("snapshot"), size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 }, mip_level_count: 1, sample_count: 1, dimension: wgpu::TextureDimension::D2, format: wgpu::TextureFormat::Rgba16Float, usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC, view_formats: &[] });
        let view = tex.create_view(&Default::default());
        let mut enc = self.device.create_command_encoder(&Default::default());
        { let mut p = pass(&mut enc, "snapshot", &view); p.set_pipeline(self.inter_pipeline(shown.variant)); p.set_bind_group(0, &shown.binds[match which { Snapshot::Encoded => 2, Snapshot::Linear => 3, Snapshot::Display => 4 }], &[]); p.draw(0..6, 0..1); }
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
        Frame { width: w, height: h, pixels: Pixels::Planar { planes, layout: PlanarLayout::Ycc { bytes_per_sample: 1, code_bits: 8, interleaved_chroma: false, alpha: false } }, meta: ColorMeta { bit_depth: 8, ..Default::default() } }
    }
    #[test]
    fn fit_in_places_the_video_inside_the_content_rect() {
        let near = |a: [f32; 2], b: [f32; 2]| (a[0] - b[0]).abs() < 1e-5 && (a[1] - b[1]).abs() < 1e-5;
        // 16:9 video in a 16:9 viewport: fills it
        assert!(near(fit_in((1920, 1080), (1920, 1080), None).0, [1.0, 1.0]));
        // the bottom 180 px taken by bands: the video letterboxes into the top 900 → scaled down and centred there
        let (s, o) = fit_in((1920, 1080), (1920, 1080), Some((0, 0, 1920, 900)));
        assert!(near(s, [1600.0 / 1920.0, 900.0 / 1080.0]), "{s:?}");
        assert!(near(o, [0.0, 1.0 - 450.0 / 1080.0 * 2.0]), "{o:?}");
        // a pillarboxed 4:3 video in the whole viewport: width scales, no offset
        let (s, o) = fit_in((1440, 1080), (1920, 1080), None);
        assert!(near(s, [0.75, 1.0]) && near(o, [0.0, 0.0]));
    }
    #[test]
    fn the_letterbox_stays_black_at_odd_aspects() {
        // A 16×16 picture in a 64×16 target: the pillarbox on both sides must be black and the picture centred — the
        // fullscreen triangle's overhang used to smear the edge column diagonally into the box.
        let Ok(mut e) = Engine::headless(wgpu::TextureFormat::Rgba16Float) else { eprintln!("no GPU — skipped"); return };
        e.set_display(DisplayMode { out: OutMode::Linear, white: 100.0 });
        e.show(&ramp_frame(16, 16)).unwrap();
        let out = e.render_offscreen(64, 16).unwrap();
        let px = |x: usize, y: usize| out[(y * 64 + x) * 4];
        for y in [0, 8, 15] {
            assert!(px(2, y) == 0.0 && px(22, y) == 0.0 && px(41, y) == 0.0 && px(61, y) == 0.0, "black outside the picture at row {y}: {} {} {} {}", px(2, y), px(22, y), px(41, y), px(61, y));
            assert!(px(25, y) > 0.0 && px(38, y) > 0.0, "picture inside at row {y}");
        }
        // and with a content rect taking the right half: the picture centres in that half
        e.set_content_rect(Some((32, 0, 32, 16)));
        let out = e.render_offscreen(64, 16).unwrap();
        let px = |x: usize| out[(8 * 64 + x) * 4];
        assert!(px(16) == 0.0 && px(38) == 0.0 && px(57) == 0.0 && px(47) > 0.0, "{} {} {} {}", px(16), px(38), px(47), px(57));
    }
    /// As `ramp_frame`, with an alpha plane: opaque on the left half, transparent on the right.
    fn alpha_frame(w: u32, h: u32) -> Frame {
        let plane = |pw: u32, ph: u32, f: &dyn Fn(u32, u32) -> u8| { let pitch = pw.div_ceil(256) * 256; let mut data = vec![0u8; (pitch * ph) as usize]; for y in 0..ph { for x in 0..pw { data[(y * pitch + x) as usize] = f(x, y); } } Plane { data, width: pw, height: ph, pitch } };
        let planes = vec![plane(w, h, &|x, _| (16 + (x * 219) / (w - 1)) as u8), plane(w / 2, h, &|_, _| 128), plane(w / 2, h, &|_, _| 128), plane(w, h, &|x, _| if x < w / 2 { 255 } else { 0 })];
        Frame { width: w, height: h, pixels: Pixels::Planar { planes, layout: PlanarLayout::Ycc { bytes_per_sample: 1, code_bits: 8, interleaved_chroma: false, alpha: true } }, meta: ColorMeta { bit_depth: 8, ..Default::default() } }
    }
    #[test]
    fn the_background_fills_under_alpha_and_mixes_outside_the_picture() {
        // Row 153 / QCView's bounds policy: DarkGray = #1f1f1f under the picture (where its alpha is 0 the fill shows),
        // #161616 outside the media rect (strength 1.0); Black = black everywhere; the checkers alternate 32-px tiles.
        let Ok(mut e) = Engine::headless(wgpu::TextureFormat::Rgba16Float) else { eprintln!("no GPU — skipped"); return };
        e.set_display(DisplayMode { out: OutMode::Linear, white: 100.0 });
        let lin = |c: f32| if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) };
        let (g1f, bg) = (lin(31.0 / 255.0), lin(22.0 / 255.0));
        e.set_background(1);
        e.show(&alpha_frame(16, 16)).unwrap();   // a 16×16 picture centred in 64×16: x 24..40, opaque 24..32, transparent 32..40
        let out = e.render_offscreen(64, 16).unwrap();
        let px = |x: usize| out[(8 * 64 + x) * 4];
        assert!((px(2) - bg).abs() < 2e-3 && (px(60) - bg).abs() < 2e-3, "outside the media: Theme.bg — {} {} (want {bg})", px(2), px(60));
        assert!(px(30) > 0.05, "the opaque half shows the ramp: {}", px(30));
        assert!((px(36) - g1f).abs() < 2e-3, "the transparent half shows the fill #1f1f1f: {} (want {g1f})", px(36));
        e.set_background(0);
        let out = e.render_offscreen(64, 16).unwrap();
        let px = |x: usize| out[(8 * 64 + x) * 4];
        assert!(px(2) == 0.0 && px(36) == 0.0 && px(30) > 0.05, "Black: black outside and under the alpha — {} {} {}", px(2), px(36), px(30));
        e.set_background(3); e.set_ui_scale(1.0);
        let out = e.render_offscreen(64, 16).unwrap();
        let px = |x: usize| out[(8 * 64 + x) * 4];
        assert!(px(2) > 0.03 && px(50) > 0.03 && (px(2) - px(50)).abs() > 0.01, "LightChecker (mixed 0.7 toward Theme.bg outside): two tiles 32 px apart differ — {} {}", px(2), px(50));
    }
    #[test]
    fn geometry_rotates_the_picture_and_the_par_widens_the_fit() {
        // A 32×16 ramp (dark left → bright right) in a 64×64 target. Rotated 90° CW it stands 16 wide × 32 tall, fitted
        // to 32×64 at the centre, and the ramp runs top → bottom (the stored left edge is now the top). With a 2:1 PAR
        // (Detected) and no rotation the picture is 64 wide × 16 tall.
        let Ok(mut e) = Engine::headless(wgpu::TextureFormat::Rgba16Float) else { eprintln!("no GPU — skipped"); return };
        e.set_display(DisplayMode { out: OutMode::Linear, white: 100.0 });
        let mut f = ramp_frame(32, 16); f.meta.rotation = 90; f.meta.sar = (2, 1);
        e.show(&f).unwrap();
        assert_eq!(e.display_dims(), (16, 32), "Auto rotation swaps the dims; Square ignores the SAR");
        let out = e.render_offscreen(64, 64).unwrap();
        let px = |x: usize, y: usize| out[(y * 64 + x) * 4];
        assert!(px(4, 32) == 0.0 && px(60, 32) == 0.0 && px(32, 32) > 0.0, "a 32-wide column of picture at the centre: {} {} {}", px(4, 32), px(60, 32), px(32, 32));
        assert!(px(32, 4) < px(32, 60), "90° CW: the stored left (dark) edge is on top — top {} bottom {}", px(32, 4), px(32, 60));
        e.set_geometry(Geometry { par: Par::Detected, rotation: Some(0) });
        assert_eq!(e.display_dims(), (64, 16), "Detected PAR 2:1 doubles the width; a forced 0° keeps it landscape");
        let out = e.render_offscreen(64, 64).unwrap();
        let px = |x: usize, y: usize| out[(y * 64 + x) * 4];
        assert!(px(32, 20) == 0.0 && px(32, 32) > 0.0 && px(2, 32) < px(61, 32), "a 16-tall band across the centre with the ramp left → right: {} {} {} {}", px(32, 20), px(32, 32), px(2, 32), px(61, 32));
    }
    #[test]
    fn overlay_composites_at_sdr_white_in_linear_light() {
        // Stage 4 over a cleared (black) target: a straight-alpha sRGB texel lands as linear light × alpha in EDR-linear
        // output, and as its own encoding in SDR-encoded output.
        let Ok(mut e) = Engine::headless(wgpu::TextureFormat::Rgba16Float) else { eprintln!("no GPU — skipped"); return };
        let tex = e.device.create_texture(&wgpu::TextureDescriptor { label: Some("ui"), size: wgpu::Extent3d { width: 4, height: 1, depth_or_array_layers: 1 }, mip_level_count: 1, sample_count: 1, dimension: wgpu::TextureDimension::D2, format: wgpu::TextureFormat::Rgba8Unorm, usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST, view_formats: &[] });
        let px: [u8; 16] = [255, 255, 255, 128,  128, 128, 128, 255,  255, 0, 0, 0,  0, 0, 0, 255];
        e.queue.write_texture(wgpu::TexelCopyTextureInfo { texture: &tex, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All }, &px, wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(16), rows_per_image: None }, tex.size());
        let view = tex.create_view(&Default::default());
        e.set_overlay(Some(&view));
        e.set_display(DisplayMode { out: OutMode::Linear, white: 100.0 });
        let out = e.render_offscreen(4, 1).unwrap();
        let near = |a: f32, b: f32| (a - b).abs() < 0.01;
        assert!(near(out[0], 128.0 / 255.0) && near(out[1], 128.0 / 255.0), "white at α .5 over black → .502 linear: {:?}", &out[0..4]);
        assert!(near(out[4], 0.2140), "sRGB 128 → linear .214: {:?}", &out[4..8]);
        assert!(near(out[8], 0.0) && near(out[9], 0.0), "α 0 → nothing: {:?}", &out[8..12]);
        assert!(near(out[12], 0.0), "black: {:?}", &out[12..16]);
        e.set_display(DisplayMode { out: OutMode::SdrEncode, white: 100.0 });
        let out = e.render_offscreen(4, 1).unwrap();
        assert!(near(out[0], 128.0 / 255.0), "encoded: white × α: {:?}", &out[0..4]);
        assert!(near(out[4], 128.0 / 255.0), "encoded: sRGB 128 stays 128: {:?}", &out[4..8]);
        e.set_overlay(None);
        let out = e.render_offscreen(4, 1).unwrap();
        assert!(near(out[0], 0.0), "overlay removed");
    }
    #[test]
    fn identity_chain_matches_the_fused_route() {
        let Ok(mut e) = Engine::headless(wgpu::TextureFormat::Rgba16Float) else { eprintln!("no GPU — skipped"); return };
        // an sRGB-tagged source: bypass (as stored = the sRGB EOTF) and the chain's input (the tagged transfer) agree, so the two
        // routes must be the same plumbing to within fp16
        let mut f = ramp_frame(256, 16); f.meta.transfer = Transfer::Srgb;
        e.set_display(DisplayMode { out: OutMode::Linear, white: 100.0 });
        e.show(&f).unwrap();
        let fused = e.render_offscreen(256, 16).unwrap();
        e.set_color_pass(Some(Box::new(IdentityColorPass)));
        let chained = e.render_offscreen(256, 16).unwrap();
        // the working texture is fp16: ≤ 1 code at 8 bit after the sRGB encode, and ≤ 1/2048 in linear
        let worst = fused.iter().zip(&chained).map(|(a, b)| (a - b).abs()).fold(0f32, f32::max);
        assert!(worst <= 1.0 / 2048.0, "fused vs chained worst diff {worst}");
        // a BT.1886-tagged source DIFFERS by design (canon row 180): bypass shows it as stored, an engaged chain receives gamma 2.4
        e.set_color_pass(None); let f2 = ramp_frame(256, 16); e.show(&f2).unwrap();
        let bypass = e.render_offscreen(256, 16).unwrap();
        e.set_color_pass(Some(Box::new(IdentityColorPass))); let via_chain = e.render_offscreen(256, 16).unwrap();
        let mid = 128 * 4; assert!(bypass[mid] > via_chain[mid] + 0.02, "bypass {} should sit above the 2.4 read {}", bypass[mid], via_chain[mid]);
        e.set_color_pass(None); e.show(&f).unwrap();
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
    fn levels_match_qcview_constants() {
        // 10-bit LSB-aligned: 64 → 0, 940 → 1, chroma 512 neutral; 16-bit MSB-aligned (P010 / x422): 4096 / 60160 / 32768 — QCView's biplanar kernel constants
        let l = Levels::new(10, 2, false, Matrix::Bt709);
        assert!((l.level_k - 4.0).abs() < 1e-6 && (l.full_max - 1023.0).abs() < 1e-6 && (l.chroma_mid - 512.0).abs() < 1e-6 && (l.code_max - 65535.0).abs() < 1e-3);
        let l = Levels::new(16, 2, false, Matrix::Bt2020);
        assert!((l.level_k - 256.0).abs() < 1e-6 && (l.chroma_mid - 32768.0).abs() < 1e-6 && (l.kr - 0.2627).abs() < 1e-6);
        assert!(((16.0 * 256.0 - 4096.0f32).abs()) < 1e-3 && (((219.0 * 256.0) - 56064.0f32).abs()) < 1e-3 && (((224.0 * 256.0) - 57344.0f32).abs()) < 1e-3);
        let l = Levels::new(8, 1, false, Matrix::Bt601); assert!((l.kr - 0.299).abs() < 1e-6 && (l.chroma_mid - 128.0).abs() < 1e-6);
    }
    #[test]
    fn stage1_is_unclamped() {
        // code 250 luma in 8-bit limited = 1.07 encoded: survives into the linear snapshot (QCView: a range chart reads 109 %, not 100 %)
        let Ok(mut e) = Engine::headless(wgpu::TextureFormat::Rgba16Float) else { return };
        let plane = |pw: u32, ph: u32, v: u8| { let pitch = pw.div_ceil(256) * 256; Plane { data: vec![v; (pitch * ph) as usize], width: pw, height: ph, pitch } };
        let f = Frame { width: 64, height: 4, pixels: Pixels::Planar { planes: vec![plane(64, 4, 250), plane(32, 4, 128), plane(32, 4, 128)], layout: PlanarLayout::Ycc { bytes_per_sample: 1, code_bits: 8, interleaved_chroma: false, alpha: false } }, meta: ColorMeta { bit_depth: 8, ..Default::default() } };
        e.show(&f).unwrap();
        let (enc, _, _) = e.snapshot(Snapshot::Encoded).unwrap(); assert!(enc[0] > 1.05, "encoded {}", enc[0]);
        let (lin, _, _) = e.snapshot(Snapshot::Linear).unwrap(); assert!(lin[0] > 1.1, "linear {}", lin[0]);
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

    fn oetf(x: f32) -> f32 { let x = x.clamp(0.0, 1.0); if x <= 0.0031308 { 12.92 * x } else { 1.055 * x.powf(1.0 / 2.4) - 0.055 } }
    #[test]
    fn bypass_shows_sdr_as_stored_on_the_linear_surface() {
        // canon row 180: no chain → the EDR layer receives sRGB-EOTF-linearised values, so the OS's sRGB encode returns the stored code values
        let mut e = Engine::headless(wgpu::TextureFormat::Rgba16Float).unwrap();
        let f = ramp_frame(256, 1);
        e.set_display(DisplayMode { out: OutMode::Linear, white: 100.0 }); e.show(&f).unwrap();
        let (enc, _, _) = e.snapshot(Snapshot::Encoded).unwrap();
        let (disp, _, _) = e.snapshot(Snapshot::Display).unwrap();
        for x in 0..256 { let back = oetf(disp[x * 4]); assert!((back - enc[x * 4]).abs() < 1.0 / 255.0, "code {x}: stored {} → screen {}", enc[x * 4], back); }
        // the chain-in set still carries the tagged transfer (BT.1886 → gamma 2.4), so an engaged chain is not affected
        let (lin, _, _) = e.snapshot(Snapshot::Linear).unwrap();
        let mid = enc[128 * 4]; assert!((lin[128 * 4] - mid.powf(2.4)).abs() < 4e-3, "linear set at mid: {} vs {}", lin[128 * 4], mid.powf(2.4));
    }
    #[test]
    fn the_transfer_override_changes_how_the_clip_is_read() {
        let mut e = Engine::headless(wgpu::TextureFormat::Rgba16Float).unwrap();
        let f = ramp_frame(256, 1);
        e.set_display(DisplayMode { out: OutMode::Linear, white: 100.0 }); e.show(&f).unwrap();
        let (enc, _, _) = e.snapshot(Snapshot::Encoded).unwrap();
        let (auto, _, _) = e.snapshot(Snapshot::Display).unwrap();
        // SDR 709 reads as the tagged SDR did (as stored)
        e.set_transfer_override(TransferChoice::Sdr709); let (sdr, _, _) = e.snapshot(Snapshot::Display).unwrap();
        assert!((sdr[128 * 4] - auto[128 * 4]).abs() < 1e-3);
        // PQ P3 decodes the same code values to nits / 100 — far brighter at mid-ramp than the sRGB read — and converts P3 → 709 (grey stays grey)
        e.set_transfer_override(TransferChoice::PqP3); let (pq, _, _) = e.snapshot(Snapshot::Display).unwrap();
        assert!(pq[128 * 4] > auto[128 * 4] * 2.0, "pq {} vs auto {}", pq[128 * 4], auto[128 * 4]);
        let p = &pq[128 * 4..128 * 4 + 3]; assert!((p[0] - p[1]).abs() < 3e-3 && (p[1] - p[2]).abs() < 3e-3, "tint {p:?}");
        // Linear passes the stored values straight to the linear surface
        e.set_transfer_override(TransferChoice::Linear); let (linr, _, _) = e.snapshot(Snapshot::Display).unwrap();
        for x in (0..256).step_by(17) { assert!((linr[x * 4] - enc[x * 4]).abs() < 2e-3, "linear override at {x}: {} vs {}", linr[x * 4], enc[x * 4]); }
        e.set_transfer_override(TransferChoice::Auto); assert_eq!(e.transfer_override(), TransferChoice::Auto);
    }
}
