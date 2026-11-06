//! S2 scrub — a Sequencer (rational time, two streaming decoder slots, a latest-wins scrub decoder) on the S1 present path.
//! Modes: --play | --seeks t,t,… | --scrub a-b:ms | --shuttle | --cut A@in-out,B@in-out. Every mode prints its numbers;
//! `--play` and `--cut` print PER-VIDEO-FRAME LATENESS (the number P0 could not take). Verdict: docs/spikes/S2-scrub.md.
#[path = "../../s1-present/src/decode.rs"] mod decode;
#[cfg(target_os = "macos")] #[path = "../../s1-present/src/vt.rs"] mod vt;
#[cfg(windows)] #[path = "../../s1-present/src/d3d.rs"] mod vt;
mod timeline; mod scrub;
use std::{collections::BTreeMap, sync::Arc, time::{Duration, Instant}};
use timeline::{Rate, Timeline, Clip};
use winit::{application::ApplicationHandler, event::WindowEvent, event_loop::{ActiveEventLoop, EventLoop}, window::{Window, WindowId}};

pub fn pct(v: &mut Vec<f32>, p: f32) -> f32 { if v.is_empty() { return f32::NAN } v.sort_by(|a, b| a.partial_cmp(b).unwrap()); v[((v.len() - 1) as f32 * p / 100.0).round() as usize] }
fn ms(d: Duration) -> f32 { d.as_secs_f32() * 1000.0 }

#[repr(C)] #[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Uniforms { fit: [f32; 2], yscale: f32, full_range: f32, mode: f32, trc: f32, mat: f32, white: f32 }

// ---------- a streaming decoder slot: its own thread, decodes ahead into a bounded channel; takes seek commands ----------
enum Cmd { Seek { t: f64, serial: u32 }, Quit }
struct Slot { clip: usize, cmd: crossbeam_channel::Sender<Cmd>, rx: crossbeam_channel::Receiver<(u32, decode::Frame, Instant)>, serial: u32, fps: f64, start: f64, src_out: i64 }
impl Slot {
    fn open(path: &str, clip: usize, hw: bool, src_in: i64, src_out: i64) -> Slot {
        let mut dec = decode::Decoder::open(path, 0, hw).expect("open");
        dec.loop_at_eof = false;
        let (fps, start) = (dec.fps, dec.start);
        let (ctx, crx) = crossbeam_channel::unbounded::<Cmd>();
        let (tx, rx) = crossbeam_channel::bounded::<(u32, decode::Frame, Instant)>(4);
        std::thread::Builder::new().name(format!("slot{clip}")).spawn(move || {
            let mut serial = 0u32; let mut eof = false;
            let mut pending: Option<(decode::Frame, Instant)> = None;
            // position on the clip's in-point first
            if let Some((f, _)) = dec.decode_to(src_in as f64 / fps + start) { pending = Some((f, Instant::now())); }
            loop {
                match crx.try_recv() {
                    Ok(Cmd::Quit) | Err(crossbeam_channel::TryRecvError::Disconnected) => return,
                    Ok(Cmd::Seek { t, serial: s }) => { serial = s; eof = false; pending = dec.decode_to(t).map(|(f, _)| (f, Instant::now())); }
                    Err(crossbeam_channel::TryRecvError::Empty) => {}
                }
                if let Some((f, t)) = pending.take() {
                    match tx.send_timeout((serial, f, t), Duration::from_millis(2)) { Ok(()) => {}, Err(crossbeam_channel::SendTimeoutError::Timeout(v)) => { pending = Some((v.1, v.2)); continue; }, Err(_) => return }
                }
                if eof { std::thread::sleep(Duration::from_millis(2)); continue; }
                match dec.next() { Some(f) => { if dec.frame_no(f.pts) >= src_out { eof = true; } else { pending = Some((f, Instant::now())); } }, None => eof = true }
            }
        }).unwrap();
        Slot { clip, cmd: ctx, rx, serial: 0, fps, start, src_out }
    }
    fn seek(&mut self, src_frame: i64) { self.serial += 1; self.cmd.send(Cmd::Seek { t: src_frame as f64 / self.fps + self.start, serial: self.serial }).ok(); }
}
impl Drop for Slot { fn drop(&mut self) { self.cmd.send(Cmd::Quit).ok(); } }

// ---------- GPU ----------
struct Shown { textures: Vec<wgpu::Texture>, _vt: Option<(decode::VtFrame, vt::Imported)>, bind: wgpu::BindGroup, bi: bool }
struct Gpu { window: Arc<Window>, surface: wgpu::Surface<'static>, device: wgpu::Device, queue: wgpu::Queue, config: wgpu::SurfaceConfiguration,
    layout: wgpu::BindGroupLayout, bi_layout: wgpu::BindGroupLayout, pipeline: wgpu::RenderPipeline, bi_pipeline: wgpu::RenderPipeline, sampler: wgpu::Sampler, ubuf: wgpu::Buffer,
    #[cfg(any(target_os = "macos", windows))] vt: Option<vt::VtImport>, shown: Option<Shown>, size: (u32, u32), uniforms_written: bool, imports: u32 }

impl Gpu {
    /// Upload / import a frame and make it the one the next present shows.
    fn show(&mut self, f: &decode::Frame) {
        if let Some(vtf) = f.vt.as_ref() {
            #[cfg(any(target_os = "macos", windows))]
            {
                let imp = self.vt.as_mut().expect("vt").import(&self.device, vtf.pixel_buffer());
                if !self.uniforms_written { self.write_uniforms(if imp.ten_bit { 65535.0 / 65472.0 } else { 1.0 }, imp.full_range || f.full_range, 0.0); }
                let views: Vec<_> = imp.textures.iter().map(|t| t.create_view(&Default::default())).collect();
                let bind = self.bind(true, &views);
                self.shown = Some(Shown { textures: vec![], _vt: Some((vtf.clone(), imp)), bind, bi: true });
                self.imports += 1; if self.imports % 60 == 0 { self.vt.as_ref().unwrap().flush(); }
            }
            return;
        }
        let float = f.layout == decode::Layout::RgbPlanarF16;
        let fmt = if float { wgpu::TextureFormat::R16Float } else if f.bytes_per_sample == 2 { wgpu::TextureFormat::R16Unorm } else { wgpu::TextureFormat::R8Unorm };
        let reuse = self.shown.as_ref().map(|s| !s.bi && s.textures.len() == f.planes.len() && s.textures[0].format() == fmt).unwrap_or(false);
        let textures = if reuse { self.shown.take().unwrap().textures } else {
            f.planes.iter().map(|p| self.device.create_texture(&wgpu::TextureDescriptor { label: Some("plane"), size: wgpu::Extent3d { width: p.width, height: p.height, depth_or_array_layers: 1 }, mip_level_count: 1, sample_count: 1, dimension: wgpu::TextureDimension::D2, format: fmt, usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST, view_formats: &[] })).collect()
        };
        for (tex, p) in textures.iter().zip(&f.planes) {
            self.queue.write_texture(wgpu::TexelCopyTextureInfo { texture: tex, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All }, &p.data, wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(p.pitch), rows_per_image: Some(p.height) }, wgpu::Extent3d { width: p.width, height: p.height, depth_or_array_layers: 1 });
        }
        if !self.uniforms_written { self.write_uniforms(if float { 1.0 } else if f.bit_depth == 10 { 65535.0 / 1023.0 } else { 1.0 }, f.full_range, if float { 2.0 } else { 0.0 }); }
        let views: Vec<_> = textures.iter().map(|t| t.create_view(&Default::default())).collect();
        let bind = self.bind(false, &views);
        self.shown = Some(Shown { textures, _vt: None, bind, bi: false });
    }
    fn write_uniforms(&mut self, yscale: f32, full_range: bool, mat: f32) {
        let (vw, vh, ww, wh) = (self.size.0 as f32, self.size.1 as f32, self.config.width as f32, self.config.height as f32);
        let fit = if vw / vh > ww / wh { [1.0, (ww / wh) / (vw / vh)] } else { [(vw / vh) / (ww / wh), 1.0] };
        self.queue.write_buffer(&self.ubuf, 0, bytemuck::bytes_of(&Uniforms { fit, yscale, full_range: full_range as u8 as f32, mode: 0.0, trc: 0.0, mat, white: 100.0 }));
        self.uniforms_written = true;
    }
    fn bind(&self, bi: bool, views: &[wgpu::TextureView]) -> wgpu::BindGroup {
        let mut e = vec![wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::Sampler(&self.sampler) }];
        for (i, v) in views.iter().enumerate() { e.push(wgpu::BindGroupEntry { binding: 1 + i as u32, resource: wgpu::BindingResource::TextureView(v) }); }
        e.push(wgpu::BindGroupEntry { binding: if bi { 3 } else { 4 }, resource: self.ubuf.as_entire_binding() });
        self.device.create_bind_group(&wgpu::BindGroupDescriptor { label: None, layout: if bi { &self.bi_layout } else { &self.layout }, entries: &e })
    }
    /// One present. Returns false when the surface had nothing to give (occluded).
    fn present(&mut self) -> bool {
        let frame = match self.surface.get_current_texture() { wgpu::CurrentSurfaceTexture::Success(f) | wgpu::CurrentSurfaceTexture::Suboptimal(f) => f, wgpu::CurrentSurfaceTexture::Occluded => { std::thread::sleep(Duration::from_millis(16)); return false }, other => { log::warn!("{other:?}"); self.surface.configure(&self.device, &self.config); return false } };
        let view = frame.texture.create_view(&Default::default());
        let mut enc = self.device.create_command_encoder(&Default::default());
        {
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor { label: Some("present"), color_attachments: &[Some(wgpu::RenderPassColorAttachment { view: &view, depth_slice: None, resolve_target: None, ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store } })], depth_stencil_attachment: None, timestamp_writes: None, occlusion_query_set: None, multiview_mask: None });
            if let Some(s) = self.shown.as_ref() { pass.set_pipeline(if s.bi { &self.bi_pipeline } else { &self.pipeline }); pass.set_bind_group(0, &s.bind, &[]); pass.draw(0..3, 0..1); }
        }
        self.queue.submit([enc.finish()]);
        self.window.pre_present_notify();
        self.queue.present(frame);
        true
    }
}

fn make_pipeline(device: &wgpu::Device, shader: &wgpu::ShaderModule, layout: &wgpu::PipelineLayout, format: wgpu::TextureFormat) -> wgpu::RenderPipeline {
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor { label: None, layout: Some(layout),
        vertex: wgpu::VertexState { module: shader, entry_point: Some("vs"), buffers: &[], compilation_options: Default::default() },
        fragment: Some(wgpu::FragmentState { module: shader, entry_point: Some("fs"), targets: &[Some(wgpu::ColorTargetState { format, blend: None, write_mask: wgpu::ColorWrites::ALL })], compilation_options: Default::default() }),
        primitive: Default::default(), depth_stencil: None, multisample: Default::default(), multiview_mask: None, cache: None })
}

// ---------- the harness ----------
#[derive(Clone, Debug)]
enum Mode { Play, Seeks(Vec<f64>), Scrub { a: i64, b: i64, ms: f32 }, Shuttle, Cut }

struct Stat { lateness_ms: Vec<f32>, cadence: BTreeMap<i64, u32>, dropped: u32, black: u32, late: u32, shown: u32, last_shown: Option<i64>, refreshes_on_current: i64, present_iv: Vec<f32>, last_present: Option<Instant> }
impl Stat { fn new() -> Self { Stat { lateness_ms: vec![], cadence: BTreeMap::new(), dropped: 0, black: 0, late: 0, shown: 0, last_shown: None, refreshes_on_current: 0, present_iv: vec![], last_present: None } } }

struct Harness {
    mode: Mode, tl: Timeline, hw: bool, scrub_hw: bool, slots: Vec<Slot>, active: usize, scrub: Option<scrub::ScrubDecoder>,
    ready: BTreeMap<i64, (usize, decode::Frame, Instant)>,   // timeline frame → decoded frame (+ when it was decoded) waiting to be shown
    t0: Option<Instant>, frame0: i64, refresh: f32, stat: Stat, started: Instant,
    // seeks / scrub / shuttle bookkeeping
    seek_i: usize, seek_next: Instant, seek_outstanding: Option<(u64, Instant, i64)>, seek_rows: Vec<String>,
    scrub_reqs: Vec<(u64, Instant, i64)>, scrub_lag: Vec<f32>, scrub_behind: Vec<i64>, scrub_hits: u32, scrub_work: Vec<f32>, scrub_shown: u32, scrub_last_req: Option<Instant>,
    shuttle_rate: f64, shuttle_t: Option<Instant>, shuttle_frame: f64, shuttle_rows: Vec<(f64, u32, Vec<i64>)>, shuttle_last_rate_change: Option<Instant>,
    done: bool, seconds: f32,
}

impl Harness {
    /// Open the scrub decoder (scrub / shuttle / seeks) and the two streaming slots (play / seeks / cut). Called once the GPU exists.
    fn open_decoders(&mut self) {
        if matches!(self.mode, Mode::Scrub { .. } | Mode::Shuttle | Mode::Seeks(_)) {
            self.scrub = Some(scrub::ScrubDecoder::open(&self.tl.clips[0].path, self.scrub_hw, 512 << 20).expect("scrub decoder"));
            println!("scrub decoder: {}", if self.scrub_hw { "hardware (VideoToolbox / D3D12VA)" } else { "software" });
        }
        if !matches!(self.mode, Mode::Scrub { .. } | Mode::Shuttle) {
            // slot A on clip 0; slot B prewarmed on clip 1 (the gapless pair)
            for (i, c) in self.tl.clips.iter().enumerate().take(2) { self.slots.push(Slot::open(&c.path, i, self.hw, c.src_in, c.src_out)); }
        }
    }
    fn now_frame(&self, now: Instant) -> i64 { self.frame0 + ((now - self.t0.unwrap()).as_secs_f64() * self.tl.rate.as_f64()).floor() as i64 }

    /// Pull every decoded frame the active (and prewarmed) slots have produced into `ready`, keyed by TIMELINE frame.
    fn pump(&mut self) {
        for si in 0..self.slots.len() {
            let s = &self.slots[si];
            let clip = &self.tl.clips[s.clip];
            while let Ok((serial, f, t)) = s.rx.try_recv() {
                if serial != s.serial { continue }   // stale, from before a seek
                let src = ((f.pts - s.start) * s.fps).round() as i64;
                let tlf = clip.start + timeline::RationalTime::new(src - clip.src_in, clip.src_rate).rescaled_to(self.tl.rate).value;
                self.ready.insert(tlf, (si, f, t));
            }
        }
    }

    /// Playback step: show the newest ready frame ≤ the clock frame; account lateness / drops / cadence.
    fn step_play(&mut self, g: &mut Gpu, now: Instant) {
        let target = self.now_frame(now);
        self.pump();
        let candidate = self.ready.range(..=target).next_back().map(|(k, _)| *k);
        match candidate {
            Some(k) if Some(k) != self.stat.last_shown => {
                // frames skipped between the last shown and this one were never shown = dropped
                if let Some(last) = self.stat.last_shown { for f in last + 1..k { if self.tl.resolve(f).is_some() { self.stat.dropped += 1; } } *self.stat.cadence.entry(self.stat.refreshes_on_current).or_insert(0) += 1; }
                let (_, f, _) = self.ready.remove(&k).unwrap();
                g.show(&f);
                let ideal = self.t0.unwrap() + Duration::from_secs_f64((k - self.frame0) as f64 * self.tl.rate.frame_seconds());
                let late = if now >= ideal { ms(now - ideal) } else { -ms(ideal - now) };
                self.stat.lateness_ms.push(late); if late > self.refresh { self.stat.late += 1; }
                self.stat.shown += 1; self.stat.last_shown = Some(k); self.stat.refreshes_on_current = 1;
                self.ready.retain(|f, _| *f > k);
            }
            Some(_) => { self.stat.refreshes_on_current += 1; }
            None => { if self.stat.last_shown.is_none() { self.stat.black += 1; } else { self.stat.refreshes_on_current += 1; } }
        }
        if target >= self.tl.duration() { self.done = true; }
    }

    fn report_play(&mut self, label: &str) {
        let s = &mut self.stat;
        let n = s.lateness_ms.len();
        println!("{label}: shown {} frames, dropped {}, late (> 1 refresh) {}, black refreshes {}; lateness ms vs the ideal clock p50 {:.1} p95 {:.1} max {:.1} min {:.1}; cadence (refreshes per frame → count) {:?}; present interval ms p50 {:.1} p95 {:.1} max {:.1}",
            s.shown, s.dropped, s.late, s.black, pct(&mut s.lateness_ms, 50.0), pct(&mut s.lateness_ms, 95.0), pct(&mut s.lateness_ms, 100.0), pct(&mut s.lateness_ms, 0.0), s.cadence, pct(&mut s.present_iv, 50.0), pct(&mut s.present_iv, 95.0), pct(&mut s.present_iv, 100.0));
        let _ = n;
    }
}

struct App { gpu: Option<Gpu>, h: Harness, screen: Option<usize>, t_app: Instant }

impl App {
    fn init(&mut self, el: &ActiveEventLoop) {
        let monitors: Vec<_> = el.available_monitors().collect();
        let mut attrs = Window::default_attributes().with_title("S2 scrub").with_inner_size(winit::dpi::LogicalSize::new(1280.0, 720.0));
        if let Some(m) = self.screen.and_then(|i| monitors.get(i)) { let p = m.position(); attrs = attrs.with_position(winit::dpi::PhysicalPosition::new(p.x + 100, p.y + 100)); self.h.refresh = 1000.0 / (m.refresh_rate_millihertz().unwrap_or(60000) as f32 / 1000.0); }
        let window = Arc::new(el.create_window(attrs).unwrap());
        let mut idesc = wgpu::InstanceDescriptor::new_without_display_handle();
        if cfg!(windows) { idesc.backends = wgpu::Backends::DX12; }   // the canon's Windows backend (and the one d3d12va imports need)
        let instance = wgpu::Instance::new(idesc);
        let surface = instance.create_surface(window.clone()).expect("surface");
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions { power_preference: wgpu::PowerPreference::HighPerformance, compatible_surface: Some(&surface), force_fallback_adapter: false, apply_limit_buckets: false })).expect("adapter");
        let caps = surface.get_capabilities(&adapter);
        let format = if caps.formats.contains(&wgpu::TextureFormat::Rgba16Float) { wgpu::TextureFormat::Rgba16Float } else { caps.formats[0] };
        let required_features = adapter.features() & wgpu::Features::TEXTURE_FORMAT_16BIT_NORM;
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor { label: Some("s2"), required_features, ..Default::default() })).expect("device");
        let size = window.inner_size();
        let config = wgpu::SurfaceConfiguration { usage: wgpu::TextureUsages::RENDER_ATTACHMENT, format, color_space: wgpu::SurfaceColorSpace::ExtendedSrgbLinear, width: size.width.max(1), height: size.height.max(1), present_mode: wgpu::PresentMode::AutoVsync, alpha_mode: caps.alpha_modes[0], view_formats: vec![], desired_maximum_frame_latency: 2 };
        surface.configure(&device, &config);
        let tex = |b| wgpu::BindGroupLayoutEntry { binding: b, visibility: wgpu::ShaderStages::FRAGMENT, ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: true }, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false }, count: None };
        let smp = wgpu::BindGroupLayoutEntry { binding: 0, visibility: wgpu::ShaderStages::FRAGMENT, ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering), count: None };
        let ub = |b| wgpu::BindGroupLayoutEntry { binding: b, visibility: wgpu::ShaderStages::VERTEX_FRAGMENT, ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None }, count: None };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: Some("video"), entries: &[smp, tex(1), tex(2), tex(3), ub(4)] });
        let bi_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: Some("video_bi"), entries: &[smp, tex(1), tex(2), ub(3)] });
        let sh = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("present"), source: wgpu::ShaderSource::Wgsl(include_str!("../../s1-present/src/present.wgsl").into()) });
        let sh_bi = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("present_bi"), source: wgpu::ShaderSource::Wgsl(include_str!("../../s1-present/src/present_bi.wgsl").into()) });
        let pipeline = make_pipeline(&device, &sh, &device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: None, bind_group_layouts: &[Some(&layout)], immediate_size: 0 }), format);
        let bi_pipeline = make_pipeline(&device, &sh_bi, &device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: None, bind_group_layouts: &[Some(&bi_layout)], immediate_size: 0 }), format);
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor { mag_filter: wgpu::FilterMode::Linear, min_filter: wgpu::FilterMode::Linear, ..Default::default() });
        let ubuf = device.create_buffer(&wgpu::BufferDescriptor { label: Some("u"), size: 32, usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
        #[cfg(target_os = "macos")]
        let vt = if self.h.hw || self.h.scrub_hw { vt::VtImport::new(&device) } else { None };
        #[cfg(windows)]
        let vt = if self.h.hw || self.h.scrub_hw { vt::VtImport::new(&device, &queue) } else { None };
        // the decoders open HERE, after the GPU: on Windows the D3D12VA device context must be wgpu's own ID3D12Device
        self.h.open_decoders();
        let vsize = { let d = decode::Decoder::open(&self.h.tl.clips[0].path, 0, false).expect("probe"); (d.width, d.height) };
        self.gpu = Some(Gpu { window, surface, device, queue, config, layout, bi_layout, pipeline, bi_pipeline, sampler, ubuf, #[cfg(any(target_os = "macos", windows))] vt, shown: None, size: vsize, uniforms_written: false, imports: 0 });
        self.h.started = Instant::now();
    }

    fn tick(&mut self) {
        let Some(g) = self.gpu.as_mut() else { return };
        let now = Instant::now();
        let h = &mut self.h;
        match h.mode.clone() {
            Mode::Play | Mode::Cut => {
                if h.t0.is_none() {
                    // start the clock when the first frame is ready (QCView starts the loop when the decoder is ready)
                    h.pump();
                    if h.ready.contains_key(&h.frame0) { h.t0 = Some(now); println!("clock started {:.0} ms after the window; timeline {} frames at {}/{} fps, clips {:?}", ms(now - h.started), h.tl.duration(), h.tl.rate.num, h.tl.rate.den, h.tl.clips.iter().map(|c| format!("{}@{}-{}", c.path.rsplit('/').next().unwrap_or(""), c.src_in, c.src_out)).collect::<Vec<_>>()); }
                }
                if h.t0.is_some() { h.step_play(g, now); }
            }
            Mode::Seeks(times) => {
                // paused: show frame 0, then every 1.5 s seek the ACTIVE STREAMING SLOT (QCView's QCV_START_SECONDS path)
                if h.stat.last_shown.is_none() { h.pump(); if let Some((_, f, _)) = h.ready.remove(&0) { g.show(&f); h.stat.last_shown = Some(0); h.seek_next = now + Duration::from_millis(1500); } }
                else if let Some((_, t_issued, tlf)) = h.seek_outstanding {
                    // the SCRUB decoder answers first (shown as soon as it lands); the streaming slot's own answer is timed too
                    if h.scrub_last_req.is_none() { if let Some(sd) = h.scrub.as_mut() { let mut newest = None; while let Ok(r) = sd.rx.try_recv() { newest = Some(r); } if let Some(r) = newest { g.show(&r.frame); h.scrub_lag.push(ms(r.t_done - t_issued)); h.scrub_last_req = Some(now); h.seek_rows.push(format!("seek → frame {tlf}: SCRUB decoder answered in {:.1} ms (shown +{:.1})", ms(r.t_done - t_issued), ms(now - r.t_done))); } } }
                    h.pump();
                    if let Some((_, f, t_decoded)) = h.ready.remove(&tlf) {
                        if h.scrub_last_req.is_none() { g.show(&f); }
                        let work = ms(t_decoded - t_issued);
                        h.seek_rows.push(format!("                 streaming slot answered in {work:.1} ms (stream resumes from here)"));
                        h.scrub_work.push(work);
                        h.seek_outstanding = None; h.scrub_last_req = None; h.seek_next = now + Duration::from_millis(1500); h.stat.last_shown = Some(tlf); h.ready.clear();
                    }
                } else if now >= h.seek_next {
                    if h.seek_i >= times.len() { h.done = true; } else {
                        let t = times[h.seek_i]; h.seek_i += 1;
                        let tlf = timeline::RationalTime::from_seconds(t, h.tl.rate).value.clamp(0, h.tl.duration() - 1);
                        let (ci, src) = h.tl.resolve(tlf).unwrap();
                        let s = &mut h.slots[h.active]; assert_eq!(s.clip, ci);
                        s.seek(src); h.ready.clear();
                        if let Some(sd) = h.scrub.as_mut() { sd.request(src); }
                        h.seek_outstanding = Some((s.serial as u64, now, tlf));
                    }
                }
            }
            Mode::Scrub { a, b, ms: dur } => {
                // a pointer sweep: one request per refresh from a to b over `dur` ms (QCView: every move issues a request, decoders coalesce)
                let sd = h.scrub.as_mut().unwrap();
                let t_rel = ms(now - h.started) - 1000.0;
                if t_rel >= 0.0 && t_rel <= dur { let f = a + ((b - a) as f32 * (t_rel / dur)).round() as i64; let id = sd.request(f); h.scrub_reqs.push((id, now, f)); h.scrub_last_req = Some(now); }
                let mut newest: Option<scrub::Result_> = None;
                while let Ok(r) = sd.rx.try_recv() { newest = Some(r); }
                if let Some(r) = newest {
                    g.show(&r.frame);
                    let lag = ms(now - r.req.t_issued); h.scrub_lag.push(lag); h.scrub_work.push(ms(r.t_done - r.req.t_issued)); if r.hit { h.scrub_hits += 1; }
                    let behind = h.scrub_reqs.last().map(|(_, _, f)| (f - r.req.frame).abs()).unwrap_or(0); h.scrub_behind.push(behind);
                    h.scrub_shown += 1;
                }
                if t_rel > dur + 500.0 { h.done = true; }
            }
            Mode::Shuttle => {
                // 2× → 32×, doubling each second, 1 s each; the clock leads, the scrub decoder chases (latest wins)
                let sd = h.scrub.as_mut().unwrap();
                if h.shuttle_t.is_none() { h.shuttle_t = Some(now); h.shuttle_rate = 2.0; h.shuttle_last_rate_change = Some(now); h.shuttle_rows.push((2.0, 0, vec![])); }
                let dt = (now - h.shuttle_t.unwrap()).as_secs_f64(); h.shuttle_t = Some(now);
                h.shuttle_frame += dt * h.shuttle_rate * h.tl.rate.as_f64();
                let clock = h.shuttle_frame.floor() as i64;
                if clock >= h.tl.duration() { h.done = true; return }
                sd.request(clock);
                let mut newest = None; while let Ok(r) = sd.rx.try_recv() { newest = Some(r); }
                if let Some(r) = newest { g.show(&r.frame); let row = h.shuttle_rows.last_mut().unwrap(); row.1 += 1; row.2.push(clock - r.req.frame); }
                if now - h.shuttle_last_rate_change.unwrap() >= Duration::from_secs(1) {
                    if h.shuttle_rate >= 32.0 { h.done = true; return }
                    if h.shuttle_rate >= 16.0 && h.shuttle_frame + 32.0 * h.tl.rate.as_f64() >= h.tl.duration() as f64 { println!("  (file too short for the 32× second)"); h.done = true; return }
                    h.shuttle_rate *= 2.0; h.shuttle_last_rate_change = Some(now); h.shuttle_rows.push((h.shuttle_rate, 0, vec![]));
                }
            }
        }
        if g.present() { if let Some(lp) = h.stat.last_present { h.stat.present_iv.push(ms(now - lp)); } h.stat.last_present = Some(now); }
        if h.seconds > 0.0 && ms(now - h.started) > h.seconds * 1000.0 { h.done = true; }
    }

    fn finish(&mut self) {
        let h = &mut self.h;
        match h.mode.clone() {
            Mode::Play => h.report_play("play"),
            Mode::Cut => { h.report_play("cut"); println!("  boundaries at timeline frames {:?}: a black refresh or a cadence entry other than 2/3 at the boundary = a gap or a duplicate", h.tl.boundaries()); }
            Mode::Seeks(_) => { for r in &h.seek_rows { println!("  {r}"); } let mut w = h.scrub_work.clone(); let mut sc = h.scrub_lag.clone(); println!("seeks: n={} — scrub-decoder answer ms p50 {:.1} p95 {:.1} max {:.1} (+ one refresh to show); streaming-slot answer ms p50 {:.1} p95 {:.1} max {:.1}", w.len(), pct(&mut sc, 50.0), pct(&mut sc, 95.0), pct(&mut sc, 100.0), pct(&mut w, 50.0), pct(&mut w, 95.0), pct(&mut w, 100.0)); }
            Mode::Scrub { .. } => { let mut lag = h.scrub_lag.clone(); let mut work = h.scrub_work.clone(); let mut behind: Vec<f32> = h.scrub_behind.iter().map(|b| *b as f32).collect();
                println!("scrub: {} requests, {} frames shown ({} cache hits); request → frame ready ms p50 {:.1} p95 {:.1} max {:.1}; request → shown ms p50 {:.1} p95 {:.1} max {:.1}; frames behind the pointer when shown p50 {:.0} p95 {:.0} max {:.0}",
                    h.scrub_reqs.len(), h.scrub_shown, h.scrub_hits, pct(&mut work, 50.0), pct(&mut work, 95.0), pct(&mut work, 100.0), pct(&mut lag, 50.0), pct(&mut lag, 95.0), pct(&mut lag, 100.0), pct(&mut behind, 50.0), pct(&mut behind, 95.0), pct(&mut behind, 100.0)); }
            Mode::Shuttle => { for (rate, shown, lag) in &h.shuttle_rows { let mut l: Vec<f32> = lag.iter().map(|x| *x as f32).collect(); println!("  shuttle {rate:>4.0}×: {shown} frames shown in 1 s, lag behind the clock (frames) p50 {:.0} p95 {:.0} max {:.0}", pct(&mut l, 50.0), pct(&mut l, 95.0), pct(&mut l, 100.0)); } }
        }
        let mut iv = h.stat.present_iv.clone(); println!("present interval ms p50 {:.1} p95 {:.1} max {:.1} over {} presents", pct(&mut iv, 50.0), pct(&mut iv, 95.0), pct(&mut iv, 100.0), iv.len());
        let wall = h.started.elapsed().as_secs_f64(); let cpu = decode::process_cpu_seconds();
        println!("process CPU time {cpu:.2} s over {wall:.1} s = {:.0} % of one core (decode threads included)", 100.0 * cpu / wall);
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, el: &ActiveEventLoop) { if self.gpu.is_none() { self.init(el); } }
    fn window_event(&mut self, el: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => { self.finish(); el.exit() }
            WindowEvent::Resized(s) => if let Some(g) = self.gpu.as_mut() { g.config.width = s.width.max(1); g.config.height = s.height.max(1); g.surface.configure(&g.device, &g.config); g.uniforms_written = false; },
            WindowEvent::RedrawRequested => { self.tick(); if self.h.done { self.finish(); el.exit(); return } if let Some(g) = &self.gpu { g.window.request_redraw(); } }
            WindowEvent::KeyboardInput { event, .. } if event.state.is_pressed() => { if let winit::keyboard::Key::Named(winit::keyboard::NamedKey::Escape) = event.logical_key { self.finish(); el.exit(); } }
            _ => {}
        }
    }
}

fn main() {
    env_logger::init();
    let args: Vec<String> = std::env::args().collect();
    let arg = |k: &str| args.iter().position(|a| a == k).and_then(|i| args.get(i + 1).cloned());
    let has = |k: &str| args.iter().any(|a| a == k);
    let hw = has("--hw");
    let fixtures = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures");
    // --cut names its own clips; otherwise --file (default: the 4K ProRes fixture). A clip path is absolute when it has a separator or a drive.
    let abs = |p: &str| if p.contains('/') || p.contains('\\') || p.contains(':') { p.to_string() } else { format!("{fixtures}/{p}") };
    let file = arg("--file").unwrap_or_else(|| arg("--cut").and_then(|c| c.split(',').next().map(|part| abs(part.split('@').next().unwrap()))).unwrap_or_else(|| format!("{fixtures}/prores_4k.mov")));
    // probe the file for its rate + length
    let probe = decode::Decoder::open(&file, 0, false).expect("open");
    let rate = Rate::from_f64(probe.fps); let frames = (probe.duration * probe.fps).round() as i64;
    println!("file {} — {} {}x{} {:.3} fps = {}/{}, {} frames, {}", file.rsplit('/').next().unwrap(), probe.codec, probe.width, probe.height, probe.fps, rate.num, rate.den, frames, if probe.is_intra() { "intra" } else { "inter" });
    drop(probe);
    let mode = if let Some(s) = arg("--seeks") { Mode::Seeks(s.split(',').filter_map(|t| t.parse().ok()).collect()) }
        else if let Some(s) = arg("--scrub") { let (ab, m) = s.split_once(':').unwrap_or((&s, "2000")); let (a, b) = ab.split_once('-').unwrap(); Mode::Scrub { a: a.parse().unwrap(), b: b.parse().unwrap(), ms: m.parse().unwrap() } }
        else if has("--shuttle") { Mode::Shuttle } else if arg("--cut").is_some() { Mode::Cut } else { Mode::Play };
    let tl = if let Some(c) = arg("--cut") {
        // A@in-out,B@in-out (source frames; a path without @ = the whole file)
        let clips = c.split(',').map(|part| { let (p, r) = part.split_once('@').unwrap_or((part, "")); let (i, o) = r.split_once('-').map(|(i, o)| (i.parse().unwrap(), o.parse().unwrap())).unwrap_or((0, frames)); Clip { path: abs(p), src_rate: rate, src_in: i, src_out: o, start: 0 } }).collect();
        Timeline::playlist(rate, clips)
    } else { Timeline::playlist(rate, vec![Clip { path: file.clone(), src_rate: rate, src_in: 0, src_out: frames, start: 0 }]) };
    let scrub_hw = has("--scrub-hw");   // default SOFTWARE for the scrub decoder — QCView's choice on Apple (VideoToolbox pays a flush/re-prime per seek)
    let seconds = arg("--seconds").and_then(|v| v.parse().ok()).unwrap_or(0.0);
    // the decoders open in App::init, after the GPU exists (Harness::open_decoders)
    let h = Harness { mode, tl, hw, scrub_hw, slots: vec![], active: 0, scrub: None, ready: BTreeMap::new(), t0: None, frame0: 0, refresh: 16.667, stat: Stat::new(), started: Instant::now(),
        seek_i: 0, seek_next: Instant::now(), seek_outstanding: None, seek_rows: vec![], scrub_reqs: vec![], scrub_lag: vec![], scrub_behind: vec![], scrub_hits: 0, scrub_work: vec![], scrub_shown: 0, scrub_last_req: None,
        shuttle_rate: 2.0, shuttle_t: None, shuttle_frame: 0.0, shuttle_rows: vec![], shuttle_last_rate_change: None, done: false, seconds };
    let el = EventLoop::new().unwrap();
    let mut app = App { gpu: None, h, screen: arg("--screen").and_then(|v| v.parse().ok()), t_app: Instant::now() };
    el.run_app(&mut app).unwrap();
    let _ = app.t_app;
}
