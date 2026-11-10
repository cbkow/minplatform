//! The output WIRE (canon row 227 step 3; QCView's output pass, the QCVO slot formats): the engine's output canvas
//! (`Engine::render_output`, already in the wire's encoding) quantised into the slot's pixel format — `Bgra8Unorm` (the bus's
//! BGRA8: B, G, R, A in memory) or `Rgb10a2Unorm` (its memory layout IS the bus's RGB10: R in bits 0-9, G 10-19, B 20-29, the
//! top two bits unused — QCView's note) — and read back without stalling: a small ring of staging buffers mapped
//! asynchronously; the caller takes a finished frame on a later tick and copies it into the ring slot (rows already padded to
//! 256 bytes, the bus's alignment and wgpu's copy pitch).
//!
//! The OVERLAY (canon row 238; QCView's output layers): the safety guides and the note strokes, a vello texture at the canvas's
//! size (sRGB-encoded, straight alpha), laid over the canvas in linear light before the encoding — at `gain` × the canvas's
//! white: 1.0 on an SDR wire, 2.03 on PQ / HLG (QCView's 203-nit graphics white = HLG 75 %).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WireFormat { Bgra8, Rgb10 }
impl WireFormat { pub fn texture_format(self) -> wgpu::TextureFormat { match self { WireFormat::Bgra8 => wgpu::TextureFormat::Bgra8Unorm, WireFormat::Rgb10 => wgpu::TextureFormat::Rgb10a2Unorm } } }

/// The wire's ENCODING of the output canvas (linear light in Rec.709 primaries, 1.0 = `white` nits — `Engine::render_output`
/// with `OutMode::Linear`): SDR = the sRGB curve on [0, 1] (the as-stored round trip QCView's SDR capture gives); PQ = the
/// output's primaries (Rec.2020 or P3-D65, QCVO's `OutputPrimaries`) then ST 2084 of nits; HLG = Rec.2020 then BT.2100 HLG
/// (1000-nit reference display, system gamma 1.2, the inverse OOTF on the Rec.2020 luminance).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum WireEncoding { Sdr, Pq { p3: bool, white: f32 }, Hlg { white: f32 } }

const SHADER: &str = r#"
@group(0) @binding(0) var src: texture_2d<f32>;
struct P { m0: vec4<f32>, m1: vec4<f32>, m2: vec4<f32>, mode: f32, white: f32, ov_gain: f32, pad: f32 };
@group(0) @binding(1) var<uniform> p: P;
@group(0) @binding(2) var ov: texture_2d<f32>;
fn srgb_eotf(c: vec3<f32>) -> vec3<f32> { return select(pow((c + 0.055) / 1.055, vec3(2.4)), c / 12.92, c <= vec3(0.04045)); }
fn pq_oetf(nits: vec3<f32>) -> vec3<f32> {
    let m1 = 0.1593017578125; let m2 = 78.84375; let c1 = 0.8359375; let c2 = 18.8515625; let c3 = 18.6875;
    let y = pow(clamp(nits / 10000.0, vec3(0.0), vec3(1.0)), vec3(m1));
    return pow((c1 + c2 * y) / (1.0 + c3 * y), vec3(m2));
}
fn srgb_oetf(l: vec3<f32>) -> vec3<f32> { return select(1.055 * pow(l, vec3(1.0 / 2.4)) - 0.055, l * 12.92, l <= vec3(0.0031308)); }
fn hlg_oetf(e: vec3<f32>) -> vec3<f32> {
    let a = 0.17883277; let b = 0.28466892; let c = 0.55991073;
    return select(a * log(max(12.0 * e - b, vec3(1e-6))) + c, sqrt(3.0 * e), e <= vec3(1.0 / 12.0));
}
@vertex fn vs(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    let p = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    return vec4<f32>(p * 2.0 - 1.0, 0.0, 1.0);
}
@fragment fn fs(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    var lin = max(textureLoad(src, vec2<i32>(pos.xy), 0).rgb, vec3(0.0));   // over black (the canvas was cleared black)
    if (p.mode < 0.5) { lin = min(lin, vec3(1.0)); }
    if (p.ov_gain > 0.0) { let o = textureLoad(ov, vec2<i32>(pos.xy), 0); lin = mix(lin, srgb_eotf(o.rgb) * p.ov_gain, o.a); }   // the guides / strokes over the picture
    if (p.mode < 0.5) { return vec4<f32>(srgb_oetf(clamp(lin, vec3(0.0), vec3(1.0))), 1.0); }
    let w = max(vec3(dot(p.m0.xyz, lin), dot(p.m1.xyz, lin), dot(p.m2.xyz, lin)), vec3(0.0));   // Rec.709 → the output's primaries
    if (p.mode < 1.5) { return vec4<f32>(pq_oetf(w * p.white), 1.0); }
    // HLG: display light (1000-nit reference) → scene light by the inverse OOTF on Rec.2020 luminance, then the OETF
    let fd = clamp(w * p.white / 1000.0, vec3(0.0), vec3(1.0));
    let yd = dot(vec3(0.2627, 0.6780, 0.0593), fd);
    let es = select(fd * pow(yd, (1.0 / 1.2) - 1.0), vec3(0.0), yd <= 0.0);
    return vec4<f32>(hlg_oetf(clamp(es, vec3(0.0), vec3(1.0))), 1.0);
}
"#;

struct Staging { buf: wgpu::Buffer, w: u32, h: u32, bpr: u32, state: std::sync::Arc<std::sync::atomic::AtomicU8>, tag: u64, /// the submit order (`take` hands frames out oldest first) and when it was submitted
    seq: u64, at: std::time::Instant, /// the copy's submission (a `Readback` waits for exactly it)
    index: Option<wgpu::SubmissionIndex>, /// handed to a `Readback`: `take` leaves it alone
    detached: bool }

/// One frame's copy-out, detached from the encoder (`WireEncoder::detach_last`) so ANOTHER THREAD can wait for the GPU and
/// take the bytes the moment they exist — the app's tick only comes round once a refresh (spike S8: 33 ms from a frame shown
/// to its ring commit on the tick, 3–6 ms this way). Dropping it without `collect` leaves the buffer to be reclaimed by a
/// later `submit`.
pub struct Readback { buf: wgpu::Buffer, pub w: u32, pub h: u32, pub bpr: u32, pub tag: u64, pub seq: u64, pub at: std::time::Instant, state: std::sync::Arc<std::sync::atomic::AtomicU8>, index: wgpu::SubmissionIndex }
impl Readback {
    /// Wait (at most `timeout`) for the GPU to finish this frame's copy, hand the bytes (rows `bpr` apart) to `f`, free the
    /// buffer. False when the copy failed or did not finish in time.
    pub fn collect(self, device: &wgpu::Device, timeout: std::time::Duration, f: &mut dyn FnMut(&[u8])) -> bool {
        use std::sync::atomic::Ordering::*;
        let t0 = std::time::Instant::now();
        let _ = device.poll(wgpu::PollType::Wait { submission_index: Some(self.index.clone()), timeout: Some(timeout) });
        // the map's callback runs on a poll after the submission is done — usually that one
        while self.state.load(Acquire) == 1 && t0.elapsed() < timeout { let _ = device.poll(wgpu::PollType::Poll); if self.state.load(Acquire) == 1 { std::thread::sleep(std::time::Duration::from_micros(200)); } }
        match self.state.load(Acquire) {
            2 => { let ok = match self.buf.slice(..).get_mapped_range() { Ok(data) => { f(&data); true } Err(_) => false }; self.buf.unmap(); self.state.store(0, Release); ok }
            3 => { self.state.store(0, Release); false }
            _ => false,   // still mapping: `submit` reclaims the buffer once the map lands and this handle is gone
        }
    }
}

/// What the copy-out did since the counters were last taken (`QCV_TRACE_OUTPUT`): frames submitted, refused with every
/// staging buffer busy, handed out; takes that handed out two or more at once; takes whose buffers finished in another order
/// than they were submitted; each frame's submit → handed-out time.
#[derive(Default, Debug)]
pub struct WireStats { pub submitted: u32, pub busy: u32, pub taken: u32, pub multi: u32, pub disorder: u32, pub latency_ms: Vec<f32> }
// state: 0 free · 1 copying / mapping · 2 mapped (ready) · 3 failed

pub struct WireEncoder { layout: wgpu::BindGroupLayout, pipes: Vec<(wgpu::TextureFormat, wgpu::RenderPipeline)>, staging: Vec<Staging>, ubuf: wgpu::Buffer, /// bound when a frame has no overlay
    none: wgpu::TextureView, seq: u64, /// keep `stats` (off: nothing is counted)
    pub trace: bool, pub stats: WireStats }

/// QCView's graphics white on an HDR wire: 203 nits (PQ) = 75 % HLG, as a gain on the canvas's 100-nit white.
pub const OVERLAY_HDR_GAIN: f32 = 2.03;

impl WireEncoder {
    pub fn new(device: &wgpu::Device) -> WireEncoder {
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: Some("wire"), entries: &[wgpu::BindGroupLayoutEntry { binding: 0, visibility: wgpu::ShaderStages::FRAGMENT, ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: false }, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false }, count: None },
            wgpu::BindGroupLayoutEntry { binding: 1, visibility: wgpu::ShaderStages::FRAGMENT, ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None }, count: None },
            wgpu::BindGroupLayoutEntry { binding: 2, visibility: wgpu::ShaderStages::FRAGMENT, ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: false }, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false }, count: None }] });
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("wire"), source: wgpu::ShaderSource::Wgsl(SHADER.into()) });
        let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some("wire"), bind_group_layouts: &[Some(&layout)], immediate_size: 0 });
        let pipes = [wgpu::TextureFormat::Bgra8Unorm, wgpu::TextureFormat::Rgb10a2Unorm].into_iter().map(|f| (f, device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("wire"), layout: Some(&pl),
            vertex: wgpu::VertexState { module: &module, entry_point: Some("vs"), buffers: &[], compilation_options: Default::default() },
            fragment: Some(wgpu::FragmentState { module: &module, entry_point: Some("fs"), targets: &[Some(wgpu::ColorTargetState { format: f, blend: None, write_mask: wgpu::ColorWrites::ALL })], compilation_options: Default::default() }),
            primitive: Default::default(), depth_stencil: None, multisample: Default::default(), multiview_mask: None, cache: None,
        }))).collect();
        let ubuf = device.create_buffer(&wgpu::BufferDescriptor { label: Some("wire params"), size: 64, usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
        let none = device.create_texture(&wgpu::TextureDescriptor { label: Some("wire: no overlay"), size: wgpu::Extent3d { width: 1, height: 1, depth_or_array_layers: 1 }, mip_level_count: 1, sample_count: 1, dimension: wgpu::TextureDimension::D2, format: wgpu::TextureFormat::Rgba8Unorm, usage: wgpu::TextureUsages::TEXTURE_BINDING, view_formats: &[] }).create_view(&Default::default());
        WireEncoder { layout, pipes, staging: vec![], ubuf, none, seq: 0, trace: false, stats: WireStats::default() }
    }
    /// Quantise `canvas` into `fmt` and start its copy-out; `tag` comes back with the bytes. False when every staging buffer
    /// is still busy (QCView: the frame is skipped — "lossy by design"). `overlay` = the guides / strokes texture at the
    /// canvas's size (see the module's note), laid over the picture.
    pub fn submit(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, canvas: &wgpu::Texture, overlay: Option<&wgpu::TextureView>, fmt: WireFormat, encoding: WireEncoding, tag: u64) -> bool {
        use std::sync::atomic::Ordering::*;
        let (w, h) = (canvas.width(), canvas.height());
        let bpr = (w * 4).div_ceil(256) * 256;
        // a detached buffer whose `Readback` is gone (dropped, or timed out before the map landed): free again
        for s in &mut self.staging { if s.detached && std::sync::Arc::strong_count(&s.state) == 1 { match s.state.load(Acquire) { 2 => { s.buf.unmap(); s.state.store(0, Release); } 3 => s.state.store(0, Release), _ => {} } } }
        let i = match self.staging.iter().position(|s| s.state.load(Acquire) == 0 && s.w == w && s.h == h) {
            Some(i) => i,
            None => {
                if self.staging.len() >= 3 { if let Some(j) = self.staging.iter().position(|s| s.state.load(Acquire) == 0 || s.state.load(Acquire) == 3) { self.staging.remove(j); } else { if self.trace { self.stats.busy += 1; } return false } }
                let buf = device.create_buffer(&wgpu::BufferDescriptor { label: Some("wire staging"), size: (bpr * h) as u64, usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ, mapped_at_creation: false });
                self.staging.push(Staging { buf, w, h, bpr, state: std::sync::Arc::new(std::sync::atomic::AtomicU8::new(0)), tag: 0, seq: 0, at: std::time::Instant::now(), index: None, detached: false });
                self.staging.len() - 1
            }
        };
        let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("wire") });
        let out = self.draw(device, queue, canvas, overlay, fmt, encoding, &mut enc);
        self.seq += 1; let seq = self.seq; if self.trace { self.stats.submitted += 1; }
        let s = &mut self.staging[i];
        enc.copy_texture_to_buffer(out.as_image_copy(), wgpu::TexelCopyBufferInfo { buffer: &s.buf, layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(s.bpr), rows_per_image: Some(h) } }, wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 });
        s.index = Some(queue.submit([enc.finish()])); s.detached = false;
        s.state.store(1, Release); s.tag = tag; s.seq = seq; s.at = std::time::Instant::now();
        let st = s.state.clone();
        s.buf.slice(..).map_async(wgpu::MapMode::Read, move |r| st.store(if r.is_ok() { 2 } else { 3 }, Release));
        true
    }
    fn draw(&self, device: &wgpu::Device, queue: &wgpu::Queue, canvas: &wgpu::Texture, overlay: Option<&wgpu::TextureView>, fmt: WireFormat, encoding: WireEncoding, enc: &mut wgpu::CommandEncoder) -> wgpu::Texture {
        let (w, h) = (canvas.width(), canvas.height());
        let tf = fmt.texture_format();
        // linear Rec.709 → Rec.2020 / P3-D65 (ITU-R BT.2087; SMPTE EG 432-1)
        const TO_2020: [[f32; 3]; 3] = [[0.6274, 0.3293, 0.0433], [0.0691, 0.9195, 0.0114], [0.0164, 0.0880, 0.8956]];
        const TO_P3: [[f32; 3]; 3] = [[0.8225, 0.1774, 0.0], [0.0332, 0.9669, 0.0], [0.0171, 0.0724, 0.9108]];
        let (mode, m, white) = match encoding { WireEncoding::Sdr => (0.0, TO_2020, 100.0), WireEncoding::Pq { p3, white } => (1.0, if p3 { TO_P3 } else { TO_2020 }, white), WireEncoding::Hlg { white } => (2.0, TO_2020, white) };
        let mut u = [0f32; 16]; for r in 0..3 { u[r * 4..r * 4 + 3].copy_from_slice(&m[r]); } u[12] = mode; u[13] = white; u[14] = if overlay.is_none() { 0.0 } else if mode == 0.0 { 1.0 } else { OVERLAY_HDR_GAIN * 100.0 / white };
        queue.write_buffer(&self.ubuf, 0, bytemuck::cast_slice(&u));
        let out = device.create_texture(&wgpu::TextureDescriptor { label: Some("wire"), size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 }, mip_level_count: 1, sample_count: 1, dimension: wgpu::TextureDimension::D2, format: tf, usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC, view_formats: &[] });
        let view = out.create_view(&Default::default());
        let bind = device.create_bind_group(&wgpu::BindGroupDescriptor { label: Some("wire"), layout: &self.layout, entries: &[wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&canvas.create_view(&Default::default())) }, wgpu::BindGroupEntry { binding: 1, resource: self.ubuf.as_entire_binding() }, wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(overlay.unwrap_or(&self.none)) }] });
        {
            let mut p = enc.begin_render_pass(&wgpu::RenderPassDescriptor { label: Some("wire"), color_attachments: &[Some(wgpu::RenderPassColorAttachment { view: &view, resolve_target: None, depth_slice: None, ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store } })], depth_stencil_attachment: None, timestamp_writes: None, occlusion_query_set: None, multiview_mask: None });
            p.set_pipeline(&self.pipes.iter().find(|(f, _)| *f == tf).unwrap().1); p.set_bind_group(0, &bind, &[]); p.draw(0..3, 0..1);
        }
        out
    }
    /// The frame `submit` last accepted, as a `Readback` for another thread; `take` no longer sees it.
    pub fn detach_last(&mut self) -> Option<Readback> {
        let seq = self.seq;
        let s = self.staging.iter_mut().find(|s| s.seq == seq && !s.detached && s.state.load(std::sync::atomic::Ordering::Acquire) != 0)?;
        s.detached = true;
        Some(Readback { buf: s.buf.clone(), w: s.w, h: s.h, bpr: s.bpr, tag: s.tag, seq: s.seq, at: s.at, state: s.state.clone(), index: s.index.clone()? })
    }
    /// The sequence number of the frame `submit` last accepted — `take` hands it back with the bytes, so the caller can keep
    /// what it knew about the frame when it was drawn.
    pub fn last_seq(&self) -> u64 { self.seq }
    /// The finished frames in the order they were submitted, each handed to `f` as (bytes with `bpr`-byte rows, w, h, bpr,
    /// tag, seq); their buffers are free again after. (The order is the submit sequence, not the tag: one output's frames share a
    /// tag, and two that finish on the same poll would otherwise go out in buffer order — an older frame after a newer one.)
    pub fn take(&mut self, device: &wgpu::Device, f: &mut dyn FnMut(&[u8], u32, u32, u32, u64, u64)) -> bool {
        use std::sync::atomic::Ordering::*;
        let _ = device.poll(wgpu::PollType::Poll);
        let mut any = false;
        let mut ready: Vec<usize> = (0..self.staging.len()).filter(|&i| !self.staging[i].detached && self.staging[i].state.load(Acquire) == 2).collect();
        if self.trace && ready.len() > 1 { self.stats.multi += 1; if ready.windows(2).any(|p| self.staging[p[0]].seq > self.staging[p[1]].seq) { self.stats.disorder += 1; } }
        ready.sort_by_key(|&i| self.staging[i].seq);
        for i in ready {
            let s = &self.staging[i];
            if self.trace { self.stats.taken += 1; self.stats.latency_ms.push(s.at.elapsed().as_secs_f32() * 1000.0); }
            if let Ok(data) = s.buf.slice(..).get_mapped_range() { f(&data, s.w, s.h, s.bpr, s.tag, s.seq); }
            s.buf.unmap(); s.state.store(0, Release); any = true;
        }
        for s in &self.staging { if !s.detached && s.state.load(Acquire) == 3 { s.state.store(0, Release); } }
        any
    }
}
