//! S1 present — step 1: a decoded video (software FFmpeg, planes uploaded by the CPU) on an HDR-capable wgpu
//! surface; or `--ramp` for the EDR ramp. `--edr` / `--pq` pick the surface colour space (wgpu 30 configures
//! the CAMetalLayer / DXGI itself). Prints open→first-frame, decode fps, upload ms and present pacing.
//! Verdict and pass bars: docs/spikes/S1-present.md. This binary dies with the spike.
mod decode;
use std::{sync::Arc, time::Instant};
use winit::{application::ApplicationHandler, event::WindowEvent, event_loop::{ActiveEventLoop, EventLoop}, window::{Window, WindowId}};

fn pct(v: &mut Vec<f32>, p: f32) -> f32 { if v.is_empty() { return f32::NAN } v.sort_by(|a, b| a.partial_cmp(b).unwrap()); v[((v.len() - 1) as f32 * p / 100.0).round() as usize] }

#[repr(C)] #[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Uniforms { fit: [f32; 2], yscale: f32, full_range: f32 }

struct Video {
    rx: crossbeam_channel::Receiver<decode::Frame>,
    textures: Vec<wgpu::Texture>, bind_group: Option<wgpu::BindGroup>, layout: wgpu::BindGroupLayout, sampler: wgpu::Sampler, ubuf: wgpu::Buffer,
    pipeline: wgpu::RenderPipeline, size: (u32, u32), bps: u32,
    t_open: Instant, clock0: Option<(Instant, f64)>, pending: Option<decode::Frame>, first_presented: bool,
    shown: u32, upload_ms: Vec<f32>, decode_ms: Vec<f32>, presents: Vec<f32>, last_present: Option<Instant>, last_report: Instant, decoded: u32,
}

struct Gpu { window: Arc<Window>, surface: wgpu::Surface<'static>, device: wgpu::Device, queue: wgpu::Queue, config: wgpu::SurfaceConfiguration, ramp: wgpu::RenderPipeline, video: Option<Video> }

#[derive(Default)]
struct App { gpu: Option<Gpu>, color_space: wgpu::SurfaceColorSpace, file: Option<String>, threads: i32, frames: u32 }

impl App {
    fn init(&mut self, el: &ActiveEventLoop) {
        let window = Arc::new(el.create_window(Window::default_attributes().with_title("S1 present").with_inner_size(winit::dpi::LogicalSize::new(1280.0, 720.0))).unwrap());
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let surface = instance.create_surface(window.clone()).expect("surface");
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions { power_preference: wgpu::PowerPreference::HighPerformance, compatible_surface: Some(&surface), force_fallback_adapter: false, apply_limit_buckets: false })).expect("adapter");
        let info = adapter.get_info();
        println!("adapter: {} ({:?}, {:?})", info.name, info.backend, info.device_type);
        let caps = surface.get_capabilities(&adapter);
        for fc in &caps.format_capabilities { println!("  {:?} supports colour spaces {:?}", fc.format, fc.color_spaces); }
        let format = if caps.formats.contains(&wgpu::TextureFormat::Rgba16Float) { wgpu::TextureFormat::Rgba16Float } else { caps.formats[0] };
        println!("using format:    {:?}  colour space {:?}  scale factor {}  present modes {:?}", format, self.color_space, window.scale_factor(), caps.present_modes);
        let required_features = adapter.features() & wgpu::Features::TEXTURE_FORMAT_16BIT_NORM;   // R16Unorm planes for 10-bit
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor { label: Some("s1"), required_features, ..Default::default() })).expect("device");
        let size = window.inner_size();
        let config = wgpu::SurfaceConfiguration { usage: wgpu::TextureUsages::RENDER_ATTACHMENT, format, color_space: self.color_space, width: size.width.max(1), height: size.height.max(1), present_mode: wgpu::PresentMode::AutoVsync, alpha_mode: caps.alpha_modes[0], view_formats: vec![], desired_maximum_frame_latency: 2 };
        surface.configure(&device, &config);
        #[cfg(target_os = "macos")]
        mac::report_layer(&window);
        let ramp = {
            let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("ramp"), source: wgpu::ShaderSource::Wgsl(include_str!("ramp.wgsl").into()) });
            let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: None, bind_group_layouts: &[], immediate_size: 0 });
            pipeline(&device, &shader, &layout, format)
        };
        let video = self.file.as_ref().map(|path| {
            let t_open = Instant::now();
            let mut dec = decode::Decoder::open(path, self.threads).expect("open");
            println!("opened {} — {} {}x{} {:.3} fps {}  (open took {:.0} ms)", path, dec.codec, dec.width, dec.height, dec.fps, dec.pix_fmt, t_open.elapsed().as_secs_f32() * 1000.0);
            let size = (dec.width, dec.height);
            let (tx, rx) = crossbeam_channel::bounded::<decode::Frame>(3);
            std::thread::Builder::new().name("decode".into()).spawn(move || { while let Some(f) = dec.next() { if tx.send(f).is_err() { break; } } }).unwrap();
            let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("present"), source: wgpu::ShaderSource::Wgsl(include_str!("present.wgsl").into()) });
            let tex = |b| wgpu::BindGroupLayoutEntry { binding: b, visibility: wgpu::ShaderStages::FRAGMENT, ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: true }, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false }, count: None };
            let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: Some("video"), entries: &[
                wgpu::BindGroupLayoutEntry { binding: 0, visibility: wgpu::ShaderStages::FRAGMENT, ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering), count: None },
                tex(1), tex(2), tex(3),
                wgpu::BindGroupLayoutEntry { binding: 4, visibility: wgpu::ShaderStages::VERTEX_FRAGMENT, ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None }, count: None }] });
            let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: None, bind_group_layouts: &[Some(&layout)], immediate_size: 0 });
            let pipeline = pipeline(&device, &shader, &pl, format);
            let sampler = device.create_sampler(&wgpu::SamplerDescriptor { mag_filter: wgpu::FilterMode::Linear, min_filter: wgpu::FilterMode::Linear, ..Default::default() });
            let ubuf = device.create_buffer(&wgpu::BufferDescriptor { label: Some("u"), size: 16, usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
            Video { rx, textures: vec![], bind_group: None, layout, sampler, ubuf, pipeline, size, bps: 0, t_open, clock0: None, pending: None, first_presented: false, shown: 0, upload_ms: vec![], decode_ms: vec![], presents: vec![], last_present: None, last_report: Instant::now(), decoded: 0 }
        });
        self.gpu = Some(Gpu { window, surface, device, queue, config, ramp, video });
    }

    fn draw(&mut self) {
        let Some(g) = self.gpu.as_mut() else { return };
        let frame = match g.surface.get_current_texture() { wgpu::CurrentSurfaceTexture::Success(f) | wgpu::CurrentSurfaceTexture::Suboptimal(f) => f, other => { log::warn!("get_current_texture: {other:?}"); g.surface.configure(&g.device, &g.config); return; } };
        let view = frame.texture.create_view(&Default::default());
        let mut uploaded_new = false;
        if let Some(v) = g.video.as_mut() {
            // pull every frame that is due by the wall clock; upload the newest due one
            let now = Instant::now();
            let mut due: Option<decode::Frame> = None;
            loop {
                if v.pending.is_none() { match v.rx.try_recv() { Ok(f) => { v.decoded += 1; v.decode_ms.push(f.decode_ms); v.pending = Some(f) } Err(_) => break } }
                let f = v.pending.as_ref().unwrap();
                let (c0, pts0) = *v.clock0.get_or_insert((now, f.pts));
                if f.pts - pts0 <= (now - c0).as_secs_f64() + 0.001 { due = v.pending.take(); } else { break }
            }
            if let Some(f) = due {
                let t = Instant::now();
                if v.textures.is_empty() {
                    v.bps = f.bytes_per_sample;
                    let fmt = if f.bytes_per_sample == 2 { wgpu::TextureFormat::R16Unorm } else { wgpu::TextureFormat::R8Unorm };
                    v.textures = f.planes.iter().map(|p| g.device.create_texture(&wgpu::TextureDescriptor { label: Some("plane"), size: wgpu::Extent3d { width: p.width, height: p.height, depth_or_array_layers: 1 }, mip_level_count: 1, sample_count: 1, dimension: wgpu::TextureDimension::D2, format: fmt, usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST, view_formats: &[] })).collect();
                    let views: Vec<_> = v.textures.iter().map(|t| t.create_view(&Default::default())).collect();
                    v.bind_group = Some(g.device.create_bind_group(&wgpu::BindGroupDescriptor { label: Some("video"), layout: &v.layout, entries: &[
                        wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::Sampler(&v.sampler) },
                        wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&views[0]) },
                        wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(&views[1]) },
                        wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::TextureView(&views[2]) },
                        wgpu::BindGroupEntry { binding: 4, resource: v.ubuf.as_entire_binding() }] }));
                    let yscale = if f.bit_depth == 10 { 65535.0 / 1023.0 } else { 1.0 };
                    let (vw, vh, ww, wh) = (v.size.0 as f32, v.size.1 as f32, g.config.width as f32, g.config.height as f32);
                    let fit = if vw / vh > ww / wh { [1.0, (ww / wh) / (vw / vh)] } else { [(vw / vh) / (ww / wh), 1.0] };
                    g.queue.write_buffer(&v.ubuf, 0, bytemuck::bytes_of(&Uniforms { fit, yscale, full_range: if f.full_range { 1.0 } else { 0.0 } }));
                }
                for (tex, p) in v.textures.iter().zip(&f.planes) {
                    g.queue.write_texture(wgpu::TexelCopyTextureInfo { texture: tex, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All }, &p.data,
                        wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(p.pitch), rows_per_image: Some(p.height) }, wgpu::Extent3d { width: p.width, height: p.height, depth_or_array_layers: 1 });
                }
                v.upload_ms.push(t.elapsed().as_secs_f32() * 1000.0);
                v.shown += 1; uploaded_new = true;
            }
        }
        let mut enc = g.device.create_command_encoder(&Default::default());
        {
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor { label: Some("present"), color_attachments: &[Some(wgpu::RenderPassColorAttachment { view: &view, depth_slice: None, resolve_target: None, ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store } })], depth_stencil_attachment: None, timestamp_writes: None, occlusion_query_set: None, multiview_mask: None });
            match g.video.as_ref().and_then(|v| v.bind_group.as_ref().map(|b| (b, &v.pipeline))) {
                Some((bg, pl)) => { pass.set_pipeline(pl); pass.set_bind_group(0, bg, &[]); pass.draw(0..3, 0..1); }
                None => { if g.video.is_none() { pass.set_pipeline(&g.ramp); pass.draw(0..3, 0..1); } }
            }
        }
        g.queue.submit([enc.finish()]);
        g.window.pre_present_notify();
        g.queue.present(frame);
        self.frames += 1;
        if let Some(v) = g.video.as_mut() {
            let now = Instant::now();
            if uploaded_new && !v.first_presented { v.first_presented = true; println!("open → first frame presented: {:.0} ms", v.t_open.elapsed().as_secs_f32() * 1000.0); }
            if let Some(lp) = v.last_present { v.presents.push((now - lp).as_secs_f32() * 1000.0); }
            v.last_present = Some(now);
            if now.duration_since(v.last_report).as_secs_f32() >= 2.0 {
                let dt = now.duration_since(v.last_report).as_secs_f32();
                println!("{:>5.1} s  decoded {:>5.1} fps (decode ms p50 {:.1} max {:.1})  shown {:>5.1} fps  upload ms p50 {:.2} max {:.2}  present ms p50 {:.1} p95 {:.1} max {:.1}",
                    v.t_open.elapsed().as_secs_f32(), v.decoded as f32 / dt, pct(&mut v.decode_ms, 50.0), pct(&mut v.decode_ms, 100.0), v.shown as f32 / dt, pct(&mut v.upload_ms, 50.0), pct(&mut v.upload_ms, 100.0), pct(&mut v.presents, 50.0), pct(&mut v.presents, 95.0), pct(&mut v.presents, 100.0));
                v.decoded = 0; v.shown = 0; v.decode_ms.clear(); v.upload_ms.clear(); v.presents.clear(); v.last_report = now;
            }
        }
    }
}

fn pipeline(device: &wgpu::Device, shader: &wgpu::ShaderModule, layout: &wgpu::PipelineLayout, format: wgpu::TextureFormat) -> wgpu::RenderPipeline {
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor { label: None, layout: Some(layout),
        vertex: wgpu::VertexState { module: shader, entry_point: Some("vs"), buffers: &[], compilation_options: Default::default() },
        fragment: Some(wgpu::FragmentState { module: shader, entry_point: Some("fs"), targets: &[Some(wgpu::ColorTargetState { format, blend: None, write_mask: wgpu::ColorWrites::ALL })], compilation_options: Default::default() }),
        primitive: Default::default(), depth_stencil: None, multisample: Default::default(), multiview_mask: None, cache: None })
}

impl ApplicationHandler for App {
    fn resumed(&mut self, el: &ActiveEventLoop) { if self.gpu.is_none() { self.init(el); } }
    fn window_event(&mut self, el: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => el.exit(),
            WindowEvent::Resized(s) => if let Some(g) = self.gpu.as_mut() { g.config.width = s.width.max(1); g.config.height = s.height.max(1); g.surface.configure(&g.device, &g.config); if let Some(v) = g.video.as_mut() { v.textures.clear(); v.bind_group = None; } },
            WindowEvent::RedrawRequested => { self.draw(); if let Some(g) = &self.gpu { g.window.request_redraw(); } }
            WindowEvent::KeyboardInput { event, .. } if event.state.is_pressed() => { if let winit::keyboard::Key::Named(winit::keyboard::NamedKey::Escape) = event.logical_key { el.exit(); } }
            _ => {}
        }
    }
}

#[cfg(target_os = "macos")]
mod mac {
    use objc2_app_kit::{NSScreen, NSView};
    use objc2_quartz_core::CAMetalLayer;
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    /// Read back what wgpu configured on the CAMetalLayer (a SUBLAYER of the NSView's backing layer), and the screens' EDR headroom.
    pub fn report_layer(window: &winit::window::Window) {
        let RawWindowHandle::AppKit(h) = window.window_handle().unwrap().as_raw() else { return };
        // SAFETY: winit hands out the NSView of the window it owns; we are on the main thread.
        let view: &NSView = unsafe { &*(h.ns_view.as_ptr() as *const NSView) };
        let Some(layer) = view.layer() else { println!("layer: view has no layer"); return };
        let mut candidates = vec![layer.clone()];
        if let Some(subs) = unsafe { layer.sublayers() } { candidates.extend(subs.iter().map(|l| l.clone())); }
        let Some(metal) = candidates.into_iter().find_map(|l| l.downcast::<CAMetalLayer>().ok()) else { println!("layer: no CAMetalLayer on the view or its sublayers"); return };
        let mtm = objc2::MainThreadMarker::new().unwrap();
        let screens: Vec<(f64, f64)> = NSScreen::screens(mtm).iter().map(|s| (s.maximumExtendedDynamicRangeColorComponentValue(), s.maximumPotentialExtendedDynamicRangeColorComponentValue())).collect();
        println!("layer: wantsEDR={} pixelFormat={:?} colorspace={:?}; screens (current EDR max, potential) = {:?}", metal.wantsExtendedDynamicRangeContent(), metal.pixelFormat(), metal.colorspace(), screens);
    }
}

fn main() {
    env_logger::init();
    let args: Vec<String> = std::env::args().collect();
    let color_space = if args.iter().any(|a| a == "--edr") { wgpu::SurfaceColorSpace::ExtendedSrgbLinear } else if args.iter().any(|a| a == "--pq") { wgpu::SurfaceColorSpace::Bt2100Pq } else { wgpu::SurfaceColorSpace::Auto };
    let file = if args.iter().any(|a| a == "--ramp") { None } else { Some(args.iter().position(|a| a == "--file").and_then(|i| args.get(i + 1).cloned()).unwrap_or_else(|| concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/prores_4k.mov").to_string())) };
    let threads = args.iter().position(|a| a == "--threads").and_then(|i| args.get(i + 1)).and_then(|t| t.parse().ok()).unwrap_or(0);
    let el = EventLoop::new().unwrap();
    let mut app = App { color_space, file, threads, ..Default::default() };
    el.run_app(&mut app).unwrap();
    println!("frames presented: {}", app.frames);
}
