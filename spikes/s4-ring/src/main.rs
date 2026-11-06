//! S4 ring — QCView's end of the QCBridge contracts in Rust. `--layout` = the byte-exact check vs the C++ compiler;
//! `--produce NAME` = a test QCAE producer (two-process tests); `--consume NAME [--copy]` = the QCView end on wgpu (zero-copy Metal
//! import, or the CPU copy as the control) in a window; `--output ID` = the QCVO producer + the real sink. docs/spikes/S4-ring.md.
mod layout; mod ringslot; mod mapping; mod qcae; mod qcvo;
#[cfg(target_os = "macos")] mod mtl;
use layout::*;
use std::{sync::Arc, time::{Duration, Instant}};
use winit::{application::ApplicationHandler, event::WindowEvent, event_loop::{ActiveEventLoop, EventLoop}, window::{Window, WindowId}};

pub fn pct(v: &mut Vec<f32>, p: f32) -> f32 { if v.is_empty() { return f32::NAN } v.sort_by(|a, b| a.partial_cmp(b).unwrap()); v[((v.len() - 1) as f32 * p / 100.0).round() as usize] }
fn ms(d: Duration) -> f32 { d.as_secs_f32() * 1000.0 }
fn now_ns() -> i64 { qcvo::now_ns() }

// ---------- --produce: a QCAE producer (the test's other process) ----------
fn produce(name: &str, w: u32, h: u32, fps: f64, seconds: f32) {
    let bpr = aligned_bytes_per_row(w, 8);
    let mut ring = qcae::SharedRing::create(name, bpr as u64 * h as u64, 3).expect("create ring");
    println!("s4 producer: ring {name} {w}x{h} RGBA16F rows {bpr} B, 3 slots, total {} MB, slots at {} (page {})", ring.header().total_size >> 20, ring.header().slots_offset, mapping::page_size());
    ring.set_host_state(HostState::Active);
    let t0 = Instant::now(); let frame_dt = Duration::from_secs_f64(1.0 / fps);
    let mut seq = 0u64; let mut fill_ms = vec![];
    let half = |v: f32| -> u16 { let b = v.to_bits(); let s = ((b >> 16) & 0x8000) as u16; let e = ((b >> 23) & 0xff) as i32 - 127 + 15; let m = b & 0x7f_ffff; if e <= 0 { s } else { s | ((e as u16) << 10) | (m >> 13) as u16 } };
    // one row of the moving gradient per frame, memcpy'd down the slot (a 4K frame = 66 MB; the fill is the producer's own cost)
    let mut row = vec![0u16; (bpr / 2) as usize];
    while t0.elapsed().as_secs_f32() < seconds {
        let due = t0 + frame_dt.mul_f64(seq as f64);
        if let Some(d) = due.checked_duration_since(Instant::now()) { std::thread::sleep(d); }
        let Some(dst) = ring.begin_write(bpr as u64 * h as u64) else { println!("s4 producer: begin_write failed"); break };
        let tf = Instant::now();
        let phase = (seq % 240) as f32 / 240.0;
        for x in 0..w as usize { let r = ((x as f32 / w as f32 + phase) % 1.0); row[x * 4] = half(r); row[x * 4 + 1] = half(1.0 - r); row[x * 4 + 2] = half(phase); row[x * 4 + 3] = half(1.0); }
        let rb = bytemuck::cast_slice::<u16, u8>(&row);
        for y in 0..h as usize { unsafe { std::ptr::copy_nonoverlapping(rb.as_ptr(), dst.add(y * bpr as usize), bpr as usize) } }
        let mut desc = FrameDesc { width: w, height: h, bytes_per_row: bpr, pixel_format: PixelFormat::Rgba16F as u32, source_tier: SourceTier::Float32 as u32, channel_order: ChannelOrder::Rgba as u32, flags: FLAG_PREMULTIPLIED, _pad0: 0, time_value: now_ns(), time_scale: 1_000_000_000, icc_generation: 0, graphics_white: 0.0, value_scale: 1.0, comp_name: [0; MAX_COMP_NAME] };
        desc.comp_name[..11].copy_from_slice(b"s4 producer");
        ring.commit(desc); seq += 1; fill_ms.push(ms(tf.elapsed()));
    }
    ring.set_host_state(HostState::Retired);
    println!("s4 producer: {seq} frames in {:.1} s ({:.1} fps); fill+commit ms p50 {:.2} max {:.2}; ring retired", t0.elapsed().as_secs_f32(), seq as f32 / t0.elapsed().as_secs_f32(), pct(&mut fill_ms, 50.0), pct(&mut fill_ms, 100.0));
    std::thread::sleep(Duration::from_millis(300));
}

// ---------- --consume: QCView's end on wgpu ----------
struct Gpu { window: Arc<Window>, surface: wgpu::Surface<'static>, device: wgpu::Device, queue: wgpu::Queue, config: wgpu::SurfaceConfiguration, pipeline: wgpu::RenderPipeline, layout: wgpu::BindGroupLayout, sampler: wgpu::Sampler, ubuf: wgpu::Buffer }
struct Frame { desc: FrameDesc, slot: u32, t_acquired: Instant }
struct Consumer {
    name: String, copy: bool, seconds: f32, screen: Option<usize>, gpu: Option<Gpu>, ring: Option<qcae::SharedRing>,
    #[cfg(target_os = "macos")] slots: Option<mtl::SlotBuffers>,
    shown: Option<(wgpu::Texture, wgpu::BindGroup, u32)>, cpu_tex: Option<wgpu::Texture>,
    t0: Instant, first: Option<Instant>, frames: u32, import_ms: Vec<f32>, latency_ms: Vec<f32>, gaps: u64, last_seq: u64, last_report: Instant, polls: u64, opened_at: Option<Instant>,
}
impl Consumer {
    fn init(&mut self, el: &ActiveEventLoop) {
        let monitors: Vec<_> = el.available_monitors().collect();
        let mut attrs = Window::default_attributes().with_title(format!("S4 ring — {} ({})", self.name, if self.copy { "CPU copy" } else { "zero-copy" })).with_inner_size(winit::dpi::LogicalSize::new(1280.0, 720.0));
        if let Some(m) = self.screen.and_then(|i| monitors.get(i)) { let p = m.position(); attrs = attrs.with_position(winit::dpi::PhysicalPosition::new(p.x + 100, p.y + 100)); }
        let window = Arc::new(el.create_window(attrs).unwrap());
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let surface = instance.create_surface(window.clone()).expect("surface");
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions { power_preference: wgpu::PowerPreference::HighPerformance, compatible_surface: Some(&surface), force_fallback_adapter: false, apply_limit_buckets: false })).expect("adapter");
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor { label: Some("s4"), ..Default::default() })).expect("device");
        let caps = surface.get_capabilities(&adapter);
        let format = if caps.formats.contains(&wgpu::TextureFormat::Rgba16Float) { wgpu::TextureFormat::Rgba16Float } else { caps.formats[0] };
        let size = window.inner_size();
        let config = wgpu::SurfaceConfiguration { usage: wgpu::TextureUsages::RENDER_ATTACHMENT, format, color_space: wgpu::SurfaceColorSpace::ExtendedSrgbLinear, width: size.width.max(1), height: size.height.max(1), present_mode: wgpu::PresentMode::AutoVsync, alpha_mode: caps.alpha_modes[0], view_formats: vec![], desired_maximum_frame_latency: 2 };
        surface.configure(&device, &config);
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: None, source: wgpu::ShaderSource::Wgsl(include_str!("blit.wgsl").into()) });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: None, entries: &[
            wgpu::BindGroupLayoutEntry { binding: 0, visibility: wgpu::ShaderStages::FRAGMENT, ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering), count: None },
            wgpu::BindGroupLayoutEntry { binding: 1, visibility: wgpu::ShaderStages::FRAGMENT, ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: true }, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false }, count: None },
            wgpu::BindGroupLayoutEntry { binding: 2, visibility: wgpu::ShaderStages::VERTEX_FRAGMENT, ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None }, count: None }] });
        let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: None, bind_group_layouts: &[Some(&layout)], immediate_size: 0 });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor { label: None, layout: Some(&pl), vertex: wgpu::VertexState { module: &shader, entry_point: Some("vs"), buffers: &[], compilation_options: Default::default() }, fragment: Some(wgpu::FragmentState { module: &shader, entry_point: Some("fs"), targets: &[Some(wgpu::ColorTargetState { format, blend: None, write_mask: wgpu::ColorWrites::ALL })], compilation_options: Default::default() }), primitive: Default::default(), depth_stencil: None, multisample: Default::default(), multiview_mask: None, cache: None });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor { mag_filter: wgpu::FilterMode::Linear, min_filter: wgpu::FilterMode::Linear, ..Default::default() });
        let ubuf = device.create_buffer(&wgpu::BufferDescriptor { label: None, size: 16, usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
        println!("s4 consumer: {} on {}; polling {} every 2 ms", if self.copy { "CPU copy (control)" } else { "ZERO-COPY (Metal no-copy buffer → linear texture → wgpu hal)" }, adapter.get_info().name, self.name);
        self.gpu = Some(Gpu { window, surface, device, queue, config, pipeline, layout, sampler, ubuf });
    }
    fn try_open(&mut self) {
        if self.ring.is_some() { return }
        match qcae::SharedRing::open(&self.name) {
            Ok(r) => { let h = r.header(); println!("s4 consumer: ring OPEN — producer pid {} alive {} slots {} stride {} MB pixels_offset {} slots_offset {} host_state {}", h.producer_pid, r.producer_alive(), h.slot_count, h.slot_stride >> 20, h.pixels_offset, h.slots_offset, r.host_state());
                #[cfg(target_os = "macos")] if !self.copy { let g = self.gpu.as_ref().unwrap(); let offs: Vec<u64> = (0..h.slot_count).map(|i| r.pixels_offset_of(i)).collect(); self.slots = mtl::SlotBuffers::new(&g.device, r.map.base, &offs, h.pixels_capacity); println!("s4 consumer: {} slot buffers wrapped without copying ({} MB each)", self.slots.as_ref().map(|s| s.buffers.len()).unwrap_or(0), h.pixels_capacity >> 20); }
                self.ring = Some(r); self.opened_at = Some(Instant::now()); }
            Err(e) => { if self.polls % 500 == 0 { println!("s4 consumer: waiting for {}: {e}", self.name); } }
        }
    }
    fn import(&mut self, f: &Frame) -> Option<(wgpu::Texture, wgpu::BindGroup)> {
        let g = self.gpu.as_ref()?; let ring = self.ring.as_ref()?;
        let (w, h, bpr) = (f.desc.width, f.desc.height, f.desc.bytes_per_row);
        let tex = if self.copy {
            let t = self.cpu_tex.take().filter(|t| t.width() == w && t.height() == h).unwrap_or_else(|| g.device.create_texture(&wgpu::TextureDescriptor { label: Some("ring copy"), size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 }, mip_level_count: 1, sample_count: 1, dimension: wgpu::TextureDimension::D2, format: wgpu::TextureFormat::Rgba16Float, usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST, view_formats: &[] }));
            let src = unsafe { std::slice::from_raw_parts(ring.map.base.add(ring.pixels_offset_of(f.slot) as usize), (bpr * h) as usize) };
            g.queue.write_texture(wgpu::TexelCopyTextureInfo { texture: &t, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All }, src, wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(bpr), rows_per_image: Some(h) }, wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 });
            t
        } else {
            #[cfg(target_os = "macos")] { self.slots.as_ref()?.texture(&g.device, f.slot as usize, w, h, bpr)? }
            #[cfg(not(target_os = "macos"))] { return None }
        };
        let view = tex.create_view(&Default::default());
        let (vw, vh, ww, wh) = (w as f32, h as f32, g.config.width as f32, g.config.height as f32);
        let fit = if vw / vh > ww / wh { [1.0f32, (ww / wh) / (vw / vh)] } else { [(vw / vh) / (ww / wh), 1.0] };
        g.queue.write_buffer(&g.ubuf, 0, bytemuck::cast_slice(&[fit[0], fit[1], 0.0, 0.0]));
        let bind = g.device.create_bind_group(&wgpu::BindGroupDescriptor { label: None, layout: &g.layout, entries: &[wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::Sampler(&g.sampler) }, wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&view) }, wgpu::BindGroupEntry { binding: 2, resource: g.ubuf.as_entire_binding() }] });
        Some((tex, bind))
    }
    fn tick(&mut self, el: &ActiveEventLoop) {
        self.polls += 1;
        self.try_open();
        let mut new = None;
        if let Some(r) = self.ring.as_mut() {
            if let Some((desc, _px, slot)) = r.acquire_latest() {
                let seq = r.last_seen; if self.last_seq != 0 && seq > self.last_seq + 1 { self.gaps += seq - self.last_seq - 1; } self.last_seq = seq;
                new = Some(Frame { desc, slot, t_acquired: Instant::now() });
            } else if r.host_state() == HostState::Retired as u32 || !r.producer_alive() { println!("s4 consumer: producer {} — closing the ring", if r.producer_alive() { "retired" } else { "gone" }); self.ring = None; self.shown = None; #[cfg(target_os = "macos")] { self.slots = None; } }
        }
        if let Some(f) = new {
            let t = Instant::now();
            if f.desc.pixel_format != PixelFormat::Rgba16F as u32 || f.desc.channel_order != ChannelOrder::Rgba as u32 { println!("s4 consumer: REFUSED frame: format {} order {} (only RGBA16F / RGBA, as QCView)", f.desc.pixel_format, f.desc.channel_order); return }
            if let Some((tex, bind)) = self.import(&f) {
                self.import_ms.push(ms(t.elapsed()));
                if f.desc.time_scale == 1_000_000_000 { let lat = (now_ns() - f.desc.time_value) as f32 / 1e6; if lat.abs() < 10_000.0 { self.latency_ms.push(lat); } }
                let prev = self.shown.replace((tex, bind, f.slot));
                if self.frames <= 3 || self.frames % 50 == 0 { eprintln!("s4 consumer: frame {} seq {} time {}/{} fmt {} tier {} order {} flags {:#x} white {} scale {}", self.frames, self.last_seq, f.desc.time_value, f.desc.time_scale, f.desc.pixel_format, f.desc.source_tier, f.desc.channel_order, f.desc.flags, f.desc.graphics_white, f.desc.value_scale); }
                if self.first.is_none() { self.first = Some(Instant::now()); let name = String::from_utf8_lossy(&f.desc.comp_name[..f.desc.comp_name.iter().position(|&c| c == 0).unwrap_or(0)]).into_owned(); println!("s4 consumer: FIRST FRAME {}x{} rows {} B seq {} comp {:?} flags {:#x} value_scale {} — {:.0} ms after the ring opened", f.desc.width, f.desc.height, f.desc.bytes_per_row, self.last_seq, name, f.desc.flags, f.desc.value_scale, ms(self.opened_at.map(|o| f.t_acquired - o).unwrap_or_default())); }
                self.frames += 1;
                self.draw();
                drop(prev);   // the previous slot's texture dies after the new frame is submitted; the claim moved with acquire
            }
        }
        if self.last_report.elapsed() >= Duration::from_secs(2) && self.frames > 0 {
            println!("s4 consumer: {:>5.1} s  frames {}  import ms p50 {:.3} p95 {:.3} max {:.3}  commit→import latency ms p50 {:.1} p95 {:.1}  latest-wins skips {}", self.t0.elapsed().as_secs_f32(), self.frames, pct(&mut self.import_ms, 50.0), pct(&mut self.import_ms, 95.0), pct(&mut self.import_ms, 100.0), pct(&mut self.latency_ms, 50.0), pct(&mut self.latency_ms, 95.0), self.gaps);
            self.import_ms.clear(); self.latency_ms.clear(); self.last_report = Instant::now();
        }
        if self.seconds > 0.0 && self.t0.elapsed().as_secs_f32() > self.seconds { el.exit(); return }
        el.set_control_flow(winit::event_loop::ControlFlow::WaitUntil(Instant::now() + Duration::from_millis(2)));
    }
    fn draw(&mut self) {
        let Some(g) = self.gpu.as_mut() else { return };
        let frame = match g.surface.get_current_texture() { wgpu::CurrentSurfaceTexture::Success(f) | wgpu::CurrentSurfaceTexture::Suboptimal(f) => f, _ => return };
        let view = frame.texture.create_view(&Default::default());
        let mut enc = g.device.create_command_encoder(&Default::default());
        { let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor { label: None, color_attachments: &[Some(wgpu::RenderPassColorAttachment { view: &view, depth_slice: None, resolve_target: None, ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store } })], depth_stencil_attachment: None, timestamp_writes: None, occlusion_query_set: None, multiview_mask: None });
          if let Some((_, bind, _)) = self.shown.as_ref() { pass.set_pipeline(&g.pipeline); pass.set_bind_group(0, bind, &[]); pass.draw(0..3, 0..1); } }
        g.queue.submit([enc.finish()]);
        g.window.pre_present_notify();
        g.queue.present(frame);
    }
}
impl ApplicationHandler for Consumer {
    fn resumed(&mut self, el: &ActiveEventLoop) { if self.gpu.is_none() { self.init(el); } }
    fn about_to_wait(&mut self, el: &ActiveEventLoop) { self.tick(el); }
    fn window_event(&mut self, el: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => el.exit(),
            WindowEvent::Resized(s) => if let Some(g) = self.gpu.as_mut() { g.config.width = s.width.max(1); g.config.height = s.height.max(1); g.surface.configure(&g.device, &g.config); },
            WindowEvent::RedrawRequested => self.draw(),
            WindowEvent::KeyboardInput { event, .. } if event.state.is_pressed() => { if let winit::keyboard::Key::Named(winit::keyboard::NamedKey::Escape) = event.logical_key { el.exit(); } }
            _ => {}
        }
    }
}

// ---------- --output: the QCVO producer + the real sink ----------
fn output(id: &str, seconds: f32, launch: bool) {
    let (w, h) = (1920u32, 1080u32);
    let cap = (aligned_bytes_per_row(3840, 4) as u64) * 2160;
    let name = qcvo::name_for(id);
    let mut ring = qcvo::OutputRing::create(&name, cap, 3, if launch { OUTPUT_LAUNCHED_BY_PRODUCER } else { 0 }).expect("create output ring");
    ring.write_hdr_info(OutputHdrInfo { transfer: 1, primaries: 0, ..Default::default() });
    ring.set_state(OutputState::Active);
    println!("s4 output: ring {name} created ({} MB, 3 slots); state Active", ring.header().total_size >> 20);
    let mut child = None;
    if launch {
        let manifest = format!("/Library/Application Support/QCView/Outputs/{id}.json");
        match std::fs::read_to_string(&manifest) { Ok(m) => {
            let exec = m.split("\"exec\":").nth(1).and_then(|s| s.split('"').nth(1)).map(|s| s.to_string());
            if let Some(exec) = exec { println!("s4 output: launching the sink from its manifest: {exec} --id {id} (QCVO_ID={id})"); child = std::process::Command::new(&exec).args(["--id", id]).env("QCVO_ID", id).spawn().ok(); } }
            Err(e) => println!("s4 output: no manifest {manifest}: {e}") }
    }
    let t0 = Instant::now(); let mut seq = 0u64; let mut accepted = false; let mut last_report = Instant::now(); let mut last_hb = 0i64;
    let mut row = vec![0u8; aligned_bytes_per_row(w, 4) as usize];
    while t0.elapsed().as_secs_f32() < seconds {
        if let Some(req) = ring.read_request() {
            let g = ring.request_generation();
            let (rw, rh) = if req.width == 0 || req.height == 0 { (w, h) } else { (req.width.min(3840), req.height.min(2160)) };
            let ok = req.protocol_version == OUTPUT_DESC_VERSION && matches!(req.pixel_format, 0 | 1);
            let mut reply = OutputReply { accepted: ok as u32, refusal: if ok { 0 } else { 3 }, width: rw, height: rh, pixel_format: 1, transfer: 1, content: 1, layers: 0, message: [0; OUTPUT_MESSAGE_BYTES] };
            let msg = if ok { "S4 Rust producer: BGRA8 SDR, no layers" } else { "S4: only BGRA8" }; reply.message[..msg.len()].copy_from_slice(msg.as_bytes());
            ring.write_reply(reply, g); accepted = ok;
            println!("s4 output: REQUEST gen {g}: proto {} {}x{} fmt {} transfer {} fit {} content {} layers {:#x} fps cap {}/{} → {} {}x{}", req.protocol_version, req.width, req.height, req.pixel_format, req.transfer, req.fit, req.content, req.layers, req.max_fps_num, req.max_fps_den, if ok { "ACCEPTED" } else { "REFUSED" }, rw, rh);
        }
        if let Some(s) = ring.read_status() { println!("s4 output: sink status: {s:?}"); }
        let now = now_ns();
        let hb = ring.header().heartbeat_ns.load(std::sync::atomic::Ordering::Acquire);
        if hb != last_hb && last_hb == 0 { println!("s4 output: sink ATTACHED: pid {} protocol {}", ring.header().consumer_pid.load(std::sync::atomic::Ordering::Acquire), ring.header().consumer_protocol.load(std::sync::atomic::Ordering::Acquire)); }
        last_hb = hb;
        if accepted {
            let bpr = aligned_bytes_per_row(w, 4);
            if let Some(dst) = ring.begin_write(bpr as u64 * h as u64) {
                let phase = (seq % 120) as f32 / 120.0;
                for x in 0..w as usize { let v = ((x as f32 / w as f32 + phase) % 1.0 * 255.0) as u8; row[x * 4] = v; row[x * 4 + 1] = 255 - v; row[x * 4 + 2] = (phase * 255.0) as u8; row[x * 4 + 3] = 255; }
                for y in 0..h as usize { unsafe { std::ptr::copy_nonoverlapping(row.as_ptr(), dst.add(y * bpr as usize), bpr as usize) } }
                seq += 1;
                ring.commit(OutputFrameDesc { width: w, height: h, bytes_per_row: bpr, pixel_format: 1, transfer: 1, content: 1, layers: 0, flags: OUTPUT_FRAME_HAS_A, presentation_ns: now, source_frame_index: seq as i64, source_fps_num: 24, source_fps_den: 1, timecode: 0, _pad0: 0 });
            }
        }
        if last_report.elapsed() >= Duration::from_secs(2) {
            let h2 = ring.header();
            println!("s4 output: {:>5.1} s  published {seq}  sink present {}  heartbeat age {:.0} ms  sink last_seen_seq {}  request gen {}", t0.elapsed().as_secs_f32(), ring.consumer_present(now), (now - hb) as f32 / 1e6, h2.last_seen_seq.load(std::sync::atomic::Ordering::Acquire), h2.request_generation.load(std::sync::atomic::Ordering::Acquire));
            last_report = Instant::now();
        }
        std::thread::sleep(Duration::from_micros(41_667));
    }
    ring.set_state(OutputState::Retired);
    println!("s4 output: retired after {seq} frames; the sink (launched by us) should quit on its own");
    std::thread::sleep(Duration::from_millis(1500));
    if let Some(mut c) = child { match c.try_wait() { Ok(Some(st)) => println!("s4 output: sink exited: {st}"), _ => { println!("s4 output: sink still running — stopping it"); c.kill().ok(); } } }
}

fn main() {
    env_logger::init();
    let args: Vec<String> = std::env::args().collect();
    let arg = |k: &str| args.iter().position(|a| a == k).and_then(|i| args.get(i + 1).cloned());
    let has = |k: &str| args.iter().any(|a| a == k);
    let seconds: f32 = arg("--seconds").and_then(|v| v.parse().ok()).unwrap_or(10.0);
    if has("--layout") {
        let (n, bad) = layout::check();
        for b in &bad { println!("s4 layout: MISMATCH {b}"); }
        println!("s4 layout: {n} sizes/offsets/constants compared against the C++ compiler, {} mismatches — {}", bad.len(), if bad.is_empty() { "BYTE-EXACT" } else { "NOT exact" });
        std::process::exit(if bad.is_empty() { 0 } else { 1 });
    }
    if let Some(name) = arg("--produce") { produce(&name, arg("--w").and_then(|v| v.parse().ok()).unwrap_or(3840), arg("--h").and_then(|v| v.parse().ok()).unwrap_or(2160), arg("--fps").and_then(|v| v.parse().ok()).unwrap_or(24.0), seconds); return }
    if let Some(id) = arg("--output") { output(&id, seconds, !has("--no-launch")); return }
    if let Some(name) = arg("--consume") {
        let el = EventLoop::new().unwrap();
        let mut app = Consumer { name, copy: has("--copy"), seconds, screen: arg("--screen").and_then(|v| v.parse().ok()), gpu: None, ring: None, #[cfg(target_os = "macos")] slots: None, shown: None, cpu_tex: None, t0: Instant::now(), first: None, frames: 0, import_ms: vec![], latency_ms: vec![], gaps: 0, last_seq: 0, last_report: Instant::now(), polls: 0, opened_at: None };
        el.run_app(&mut app).unwrap();
        println!("s4 consumer: done — {} frames shown, {} latest-wins skips", app.frames, app.gaps);
        return
    }
    println!("usage: s4-ring --layout | --produce NAME [--w --h --fps --seconds] | --consume NAME [--copy --screen N --seconds] | --output ID [--no-launch --seconds]");
}
