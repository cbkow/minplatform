//! S1 present — a decoded video on an HDR-capable wgpu surface: `--hw` = VideoToolbox + ZERO-COPY (CVPixelBuffer →
//! Metal texture → wgpu via hal), default = software FFmpeg + CPU plane upload (the control); `--ramp` = the EDR ramp. `--edr` / `--pq` pick the surface colour space (wgpu 30 configures
//! the CAMetalLayer / DXGI itself). Prints open→first-frame, decode fps, upload ms and present pacing.
//! Verdict and pass bars: docs/spikes/S1-present.md. This binary dies with the spike.
mod decode;
#[cfg(target_os = "macos")] mod vt;
#[cfg(feature = "ocio")] mod ocio;   // S1 step 2: --features ocio
use std::{collections::VecDeque, sync::Arc, time::Instant};
use winit::{application::ApplicationHandler, event::WindowEvent, event_loop::{ActiveEventLoop, EventLoop}, window::{Window, WindowId}};

pub fn pct(v: &mut Vec<f32>, p: f32) -> f32 { if v.is_empty() { return f32::NAN } v.sort_by(|a, b| a.partial_cmp(b).unwrap()); v[((v.len() - 1) as f32 * p / 100.0).round() as usize] }

#[repr(C)] #[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Uniforms { fit: [f32; 2], yscale: f32, full_range: f32, mode: f32, _pad: [f32; 3] }   // mode 1 = encoded R'G'B' out (OCIO input)

struct Video {
    rx: crossbeam_channel::Receiver<decode::Frame>,
    textures: Vec<wgpu::Texture>, bind_group: Option<wgpu::BindGroup>, layout: wgpu::BindGroupLayout, sampler: wgpu::Sampler, ubuf: wgpu::Buffer,
    pipeline: wgpu::RenderPipeline, size: (u32, u32), bps: u32,
    ubuf_enc: wgpu::Buffer, pipeline_inter: wgpu::RenderPipeline, bi_pipeline_inter: wgpu::RenderPipeline, encoded: bool,
    #[cfg(feature = "ocio")] ocio: Option<ocio::Stage>, ocio_verify: bool,
    bi_layout: wgpu::BindGroupLayout, bi_pipeline: wgpu::RenderPipeline, held: VecDeque<Held>, imported: u32,
    #[cfg(target_os = "macos")] vt: Option<vt::VtImport>,
    t_open: Instant, clock0: Option<(Instant, f64)>, pending: Option<decode::Frame>, first_presented: bool,
    shown: u32, shown_total: u32, upload_ms: Vec<f32>, decode_ms: Vec<f32>, presents: Vec<f32>, last_present: Option<Instant>, last_report: Instant, decoded: u32,
}

struct Held { _frame: decode::VtFrame, #[cfg(target_os = "macos")] _imp: vt::Imported, bind_group: wgpu::BindGroup }

struct Gpu { window: Arc<Window>, surface: wgpu::Surface<'static>, device: wgpu::Device, queue: wgpu::Queue, config: wgpu::SurfaceConfiguration, ramp: wgpu::RenderPipeline, video: Option<Video> }

#[derive(Default)]
struct App { gpu: Option<Gpu>, color_space: wgpu::SurfaceColorSpace, file: Option<String>, threads: i32, hw: bool, frames: u32, snap: Option<String>, snap_pipes: Option<(wgpu::RenderPipeline, wgpu::RenderPipeline)>,
    #[cfg(feature = "ocio")] ocio_opts: Option<ocio::Opts>, snap_blit: Option<wgpu::RenderPipeline>, seconds: f32, t0: Option<Instant>, screen: Option<usize> }

impl App {
    fn init(&mut self, el: &ActiveEventLoop) {
        let monitors: Vec<_> = el.available_monitors().collect();
        for (i, m) in monitors.iter().enumerate() { println!("monitor {i}: {:?} {:?} @ {:?} mHz, origin {:?}", m.name(), m.size(), m.refresh_rate_millihertz(), m.position()); }
        let mut attrs = Window::default_attributes().with_title("S1 present").with_inner_size(winit::dpi::LogicalSize::new(1280.0, 720.0));
        if let Some(m) = self.screen.and_then(|i| monitors.get(i)) { let p = m.position(); attrs = attrs.with_position(winit::dpi::PhysicalPosition::new(p.x + 100, p.y + 100)); }
        let window = Arc::new(el.create_window(attrs).unwrap());
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let surface = instance.create_surface(window.clone()).expect("surface");
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions { power_preference: wgpu::PowerPreference::HighPerformance, compatible_surface: Some(&surface), force_fallback_adapter: false, apply_limit_buckets: false })).expect("adapter");
        let info = adapter.get_info();
        println!("adapter: {} ({:?}, {:?})", info.name, info.backend, info.device_type);
        let caps = surface.get_capabilities(&adapter);
        for fc in &caps.format_capabilities { println!("  {:?} supports colour spaces {:?}", fc.format, fc.color_spaces); }
        let format = if caps.formats.contains(&wgpu::TextureFormat::Rgba16Float) { wgpu::TextureFormat::Rgba16Float } else { caps.formats[0] };
        println!("using format:    {:?}  colour space {:?}  scale factor {}  present modes {:?}", format, self.color_space, window.scale_factor(), caps.present_modes);
        let wanted = wgpu::Features::TEXTURE_FORMAT_16BIT_NORM | wgpu::Features::FLOAT32_FILTERABLE | wgpu::Features::TIMESTAMP_QUERY | wgpu::Features::PASSTHROUGH_SHADERS;   // R16Unorm planes; OCIO f32 LUTs; GPU timing; OCIO MSL
        let required_features = adapter.features() & wanted;
        println!("features: wanted {:?}, missing {:?}", wanted, wanted - required_features);
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor { label: Some("s1"), required_features, ..Default::default() })).expect("device");
        let size = window.inner_size();
        let config = wgpu::SurfaceConfiguration { usage: wgpu::TextureUsages::RENDER_ATTACHMENT, format, color_space: self.color_space, width: size.width.max(1), height: size.height.max(1), present_mode: wgpu::PresentMode::AutoVsync, alpha_mode: caps.alpha_modes[0], view_formats: vec![], desired_maximum_frame_latency: 2 };
        surface.configure(&device, &config);
        #[cfg(target_os = "macos")]
        mac::report_layer(&window);
        let ramp = {
            let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("ramp"), source: wgpu::ShaderSource::Wgsl(include_str!("ramp.wgsl").into()) });
            let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: None, bind_group_layouts: &[], immediate_size: 0 });
            make_pipeline(&device, &shader, &layout, format)
        };
        let video = self.file.as_ref().map(|path| {
            let t_open = Instant::now();
            let mut dec = decode::Decoder::open(path, self.threads, self.hw).expect("open");
            println!("opened {} — {} {}x{} {:.3} fps {} {}  (open took {:.0} ms)", path, dec.codec, dec.width, dec.height, dec.fps, dec.pix_fmt, if self.hw { "VIDEOTOOLBOX zero-copy" } else { "software + CPU upload" }, t_open.elapsed().as_secs_f32() * 1000.0);
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
            let pipeline = make_pipeline(&device, &shader, &pl, format);
            let bi_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("present_bi"), source: wgpu::ShaderSource::Wgsl(include_str!("present_bi.wgsl").into()) });
            let bi_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: Some("video_bi"), entries: &[
                wgpu::BindGroupLayoutEntry { binding: 0, visibility: wgpu::ShaderStages::FRAGMENT, ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering), count: None },
                tex(1), tex(2),
                wgpu::BindGroupLayoutEntry { binding: 3, visibility: wgpu::ShaderStages::VERTEX_FRAGMENT, ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None }, count: None }] });
            let bi_pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: None, bind_group_layouts: &[Some(&bi_layout)], immediate_size: 0 });
            let bi_pipeline = make_pipeline(&device, &bi_shader, &bi_pl, format);
            let pipeline_inter = make_pipeline(&device, &shader, &pl, wgpu::TextureFormat::Rgba16Float);   // the OCIO input pass
            let bi_pipeline_inter = make_pipeline(&device, &bi_shader, &bi_pl, wgpu::TextureFormat::Rgba16Float);
            #[cfg(target_os = "macos")]
            let vt = if self.hw { let v = vt::VtImport::new(&device); if v.is_none() { println!("zero-copy import unavailable (no Metal hal device)"); } v } else { None };
            let sampler = device.create_sampler(&wgpu::SamplerDescriptor { mag_filter: wgpu::FilterMode::Linear, min_filter: wgpu::FilterMode::Linear, ..Default::default() });
            let ubuf = device.create_buffer(&wgpu::BufferDescriptor { label: Some("u"), size: 32, usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
            let ubuf_enc = device.create_buffer(&wgpu::BufferDescriptor { label: Some("u-enc"), size: 32, usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
            #[cfg(feature = "ocio")]
            let ocio = self.ocio_opts.clone().map(|o| ocio::Stage::new(&device, &queue, o, size, format, (config.width, config.height), required_features.contains(wgpu::Features::FLOAT32_FILTERABLE)).expect("OCIO stage"));
            #[cfg(feature = "ocio")] let encoded = ocio.is_some();
            #[cfg(not(feature = "ocio"))] let encoded = false;
            #[cfg(feature = "ocio")] let ocio_verify = self.ocio_opts.as_ref().map(|o| o.verify).unwrap_or(false);
            #[cfg(not(feature = "ocio"))] let ocio_verify = false;
            Video { rx, textures: vec![], bind_group: None, layout, sampler, ubuf, pipeline, size, bps: 0, bi_layout, bi_pipeline, held: VecDeque::new(), imported: 0,
                ubuf_enc, pipeline_inter, bi_pipeline_inter, encoded, #[cfg(feature = "ocio")] ocio, ocio_verify,
                #[cfg(target_os = "macos")] vt, t_open, clock0: None, pending: None, first_presented: false, shown: 0, shown_total: 0, upload_ms: vec![], decode_ms: vec![], presents: vec![], last_present: None, last_report: Instant::now(), decoded: 0 }
        });
        self.gpu = Some(Gpu { window, surface, device, queue, config, ramp, video });
    }

    fn draw(&mut self) {
        let Some(g) = self.gpu.as_mut() else { return };
        let frame = match g.surface.get_current_texture() { wgpu::CurrentSurfaceTexture::Success(f) | wgpu::CurrentSurfaceTexture::Suboptimal(f) => f, wgpu::CurrentSurfaceTexture::Occluded => { std::thread::sleep(std::time::Duration::from_millis(16)); return; }   // display asleep / covered: no frame to draw, do not spin
            other => { log::warn!("get_current_texture: {other:?}"); g.surface.configure(&g.device, &g.config); return; } };
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
                if f.vt.is_some() {
                    #[cfg(target_os = "macos")]
                    {
                        let vtf = f.vt.unwrap();
                        let imp = v.vt.as_mut().expect("vt import").import(&g.device, vtf.pixel_buffer());
                        if v.imported == 0 {
                            let yscale = if imp.ten_bit { 65535.0 / 65472.0 } else { 1.0 };   // 10-bit left-justified in 16
                            let (vw, vh, ww, wh) = (v.size.0 as f32, v.size.1 as f32, g.config.width as f32, g.config.height as f32);
                            let fit = if vw / vh > ww / wh { [1.0, (ww / wh) / (vw / vh)] } else { [(vw / vh) / (ww / wh), 1.0] };
                            let fr = if imp.full_range || f.full_range { 1.0 } else { 0.0 };
                            g.queue.write_buffer(&v.ubuf, 0, bytemuck::bytes_of(&Uniforms { fit, yscale, full_range: fr, mode: 0.0, _pad: [0.0; 3] }));
                            g.queue.write_buffer(&v.ubuf_enc, 0, bytemuck::bytes_of(&Uniforms { fit: [1.0, 1.0], yscale, full_range: fr, mode: 1.0, _pad: [0.0; 3] }));
                            println!("first VideoToolbox frame: fourcc {:?}, {} planes, {}-bit, full range {}", v.vt.as_ref().unwrap().last_fourcc.to_be_bytes().map(|b| b as char).iter().collect::<String>(), imp.textures.len(), if imp.ten_bit { 10 } else { 8 }, imp.full_range);
                        }
                        let views: Vec<_> = imp.textures.iter().map(|t| t.create_view(&Default::default())).collect();
                        let bind_group = g.device.create_bind_group(&wgpu::BindGroupDescriptor { label: Some("video_bi"), layout: &v.bi_layout, entries: &[
                            wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::Sampler(&v.sampler) },
                            wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&views[0]) },
                            wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(&views[1]) },
                            wgpu::BindGroupEntry { binding: 3, resource: if v.encoded { v.ubuf_enc.as_entire_binding() } else { v.ubuf.as_entire_binding() } }] });
                        v.held.push_back(Held { _frame: vtf, _imp: imp, bind_group });
                        while v.held.len() > 3 { v.held.pop_front(); }   // the GPU is ≤ 2 frames behind (frame latency 2)
                        v.imported += 1; if v.imported % 60 == 0 { v.vt.as_ref().unwrap().flush(); }
                    }
                    v.upload_ms.push(t.elapsed().as_secs_f32() * 1000.0);
                    v.shown += 1; v.shown_total += 1; uploaded_new = true;
                } else {
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
                        wgpu::BindGroupEntry { binding: 4, resource: if v.encoded { v.ubuf_enc.as_entire_binding() } else { v.ubuf.as_entire_binding() } }] }));
                    let yscale = if f.bit_depth == 10 { 65535.0 / 1023.0 } else { 1.0 };
                    let (vw, vh, ww, wh) = (v.size.0 as f32, v.size.1 as f32, g.config.width as f32, g.config.height as f32);
                    let fit = if vw / vh > ww / wh { [1.0, (ww / wh) / (vw / vh)] } else { [(vw / vh) / (ww / wh), 1.0] };
                    let fr = if f.full_range { 1.0 } else { 0.0 };
                    g.queue.write_buffer(&v.ubuf, 0, bytemuck::bytes_of(&Uniforms { fit, yscale, full_range: fr, mode: 0.0, _pad: [0.0; 3] }));
                    g.queue.write_buffer(&v.ubuf_enc, 0, bytemuck::bytes_of(&Uniforms { fit: [1.0, 1.0], yscale, full_range: fr, mode: 1.0, _pad: [0.0; 3] }));
                }
                for (tex, p) in v.textures.iter().zip(&f.planes) {
                    g.queue.write_texture(wgpu::TexelCopyTextureInfo { texture: tex, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All }, &p.data,
                        wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(p.pitch), rows_per_image: Some(p.height) }, wgpu::Extent3d { width: p.width, height: p.height, depth_or_array_layers: 1 });
                }
                v.upload_ms.push(t.elapsed().as_secs_f32() * 1000.0);
                v.shown += 1; v.shown_total += 1; uploaded_new = true;
                }
            }
        }
        let mut enc = g.device.create_command_encoder(&Default::default());
        #[cfg(feature = "ocio")]
        let ocio_on = g.video.as_ref().map(|v| v.ocio.is_some()).unwrap_or(false);
        #[cfg(not(feature = "ocio"))]
        let ocio_on = false;
        if ocio_on {
            #[cfg(feature = "ocio")]
            {
                let v = g.video.as_mut().unwrap();
                let cur = v.held.back().map(|h| (&h.bind_group, &v.bi_pipeline_inter)).or_else(|| v.bind_group.as_ref().map(|b| (b, &v.pipeline_inter)));
                let st = v.ocio.as_mut().unwrap();
                if uploaded_new { if let Some((bg, pl)) = cur {
                    // A: planes → encoded R'G'B' at the video size (the OCIO input), B: the colour pass
                    { let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor { label: Some("ocio-in"), color_attachments: &[Some(attach(&st.inter_view))], depth_stencil_attachment: None, timestamp_writes: None, occlusion_query_set: None, multiview_mask: None });
                      pass.set_pipeline(pl); pass.set_bind_group(0, bg, &[]); pass.draw(0..3, 0..1); }
                    st.run(&mut enc);
                } }
                // C: post-OCIO → the surface (fit + EOTF)
                let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor { label: Some("present"), color_attachments: &[Some(attach(&view))], depth_stencil_attachment: None, timestamp_writes: None, occlusion_query_set: None, multiview_mask: None });
                pass.set_pipeline(&st.blit_pipeline); pass.set_bind_group(0, &st.blit_bind, &[]); pass.draw(0..3, 0..1);
            }
        } else {
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor { label: Some("present"), color_attachments: &[Some(attach(&view))], depth_stencil_attachment: None, timestamp_writes: None, occlusion_query_set: None, multiview_mask: None });
            match g.video.as_ref().and_then(|v| v.held.back().map(|h| (&h.bind_group, &v.bi_pipeline)).or_else(|| v.bind_group.as_ref().map(|b| (b, &v.pipeline)))) {
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
        let snap_due = uploaded_new && self.gpu.as_ref().and_then(|g| g.video.as_ref()).map(|v| v.shown_total == 24).unwrap_or(false);
        #[cfg(feature = "ocio")]
        if snap_due { if let Some(g) = self.gpu.as_ref() { if let Some(v) = g.video.as_ref() { if v.ocio_verify { if let Some(st) = v.ocio.as_ref() { println!("{}", st.verify(&g.device, &g.queue)); } } } } }
        if let (Some(path), true) = (self.snap.clone(), snap_due) { self.snapshot(&path); }
    }
}

impl App {
    /// Render the current frame into an sRGB-encoded Rgba8 texture (linear light → sRGB happens in the ROP) and write a PNG.
    fn snapshot(&mut self, path: &str) {
        let Some(g) = self.gpu.as_ref() else { return };
        let (w, h) = (g.config.width, g.config.height);
        let fmt = wgpu::TextureFormat::Rgba8UnormSrgb;
        let (vid_pipe, bi_pipe) = self.snap_pipes.get_or_insert_with(|| {
            let v = g.video.as_ref().unwrap();
            let sh = g.device.create_shader_module(wgpu::ShaderModuleDescriptor { label: None, source: wgpu::ShaderSource::Wgsl(include_str!("present.wgsl").into()) });
            let pl = g.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: None, bind_group_layouts: &[Some(&v.layout)], immediate_size: 0 });
            let sh2 = g.device.create_shader_module(wgpu::ShaderModuleDescriptor { label: None, source: wgpu::ShaderSource::Wgsl(include_str!("present_bi.wgsl").into()) });
            let pl2 = g.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: None, bind_group_layouts: &[Some(&v.bi_layout)], immediate_size: 0 });
            (make_pipeline(&g.device, &sh, &pl, fmt), make_pipeline(&g.device, &sh2, &pl2, fmt))
        });
        let tex = g.device.create_texture(&wgpu::TextureDescriptor { label: Some("snap"), size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 }, mip_level_count: 1, sample_count: 1, dimension: wgpu::TextureDimension::D2, format: fmt, usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC, view_formats: &[] });
        let view = tex.create_view(&Default::default());
        let pitch = (w * 4).div_ceil(256) * 256;
        let buf = g.device.create_buffer(&wgpu::BufferDescriptor { label: Some("snap"), size: (pitch * h) as u64, usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ, mapped_at_creation: false });
        let mut enc = g.device.create_command_encoder(&Default::default());
        {
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor { label: Some("snap"), color_attachments: &[Some(wgpu::RenderPassColorAttachment { view: &view, depth_slice: None, resolve_target: None, ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store } })], depth_stencil_attachment: None, timestamp_writes: None, occlusion_query_set: None, multiview_mask: None });
            let v = g.video.as_ref().unwrap();
            #[cfg(feature = "ocio")]
            let ocio_st = v.ocio.as_ref();
            #[cfg(not(feature = "ocio"))]
            let ocio_st: Option<&()> = None;
            if let Some(st) = ocio_st {
                #[cfg(feature = "ocio")]
                { let bp = self.snap_blit.get_or_insert_with(|| st.snapshot_pipeline(&g.device, fmt)); pass.set_pipeline(bp); pass.set_bind_group(0, &st.blit_bind, &[]); pass.draw(0..3, 0..1); }
                #[cfg(not(feature = "ocio"))] let _ = st;
            }
            else if let Some(hd) = v.held.back() { pass.set_pipeline(bi_pipe); pass.set_bind_group(0, &hd.bind_group, &[]); pass.draw(0..3, 0..1); }
            else if let Some(bg) = v.bind_group.as_ref() { pass.set_pipeline(vid_pipe); pass.set_bind_group(0, bg, &[]); pass.draw(0..3, 0..1); }
        }
        enc.copy_texture_to_buffer(wgpu::TexelCopyTextureInfo { texture: &tex, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All }, wgpu::TexelCopyBufferInfo { buffer: &buf, layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(pitch), rows_per_image: Some(h) } }, wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 });
        g.queue.submit([enc.finish()]);
        let slice = buf.slice(..);
        let (tx, rx) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |r| { tx.send(r).ok(); });
        g.device.poll(wgpu::PollType::wait_indefinitely()).ok();
        rx.recv().unwrap().expect("map");
        let data = slice.get_mapped_range().expect("mapped range");
        let mut rows = Vec::with_capacity((w * h * 4) as usize);
        for y in 0..h as usize { rows.extend_from_slice(&data[y * pitch as usize..y * pitch as usize + (w * 4) as usize]); }
        drop(data); buf.unmap();
        let file = std::fs::File::create(path).expect("png");
        let mut enc = png::Encoder::new(std::io::BufWriter::new(file), w, h); enc.set_color(png::ColorType::Rgba); enc.set_depth(png::BitDepth::Eight);
        enc.write_header().unwrap().write_image_data(&rows).unwrap();
        println!("snapshot written: {path} ({w}x{h})");
    }
}

fn attach(view: &wgpu::TextureView) -> wgpu::RenderPassColorAttachment<'_> { wgpu::RenderPassColorAttachment { view, depth_slice: None, resolve_target: None, ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store } } }

pub fn make_pipeline(device: &wgpu::Device, shader: &wgpu::ShaderModule, layout: &wgpu::PipelineLayout, format: wgpu::TextureFormat) -> wgpu::RenderPipeline {
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor { label: None, layout: Some(layout),
        vertex: wgpu::VertexState { module: shader, entry_point: Some("vs"), buffers: &[], compilation_options: Default::default() },
        fragment: Some(wgpu::FragmentState { module: shader, entry_point: Some("fs"), targets: &[Some(wgpu::ColorTargetState { format, blend: None, write_mask: wgpu::ColorWrites::ALL })], compilation_options: Default::default() }),
        primitive: Default::default(), depth_stencil: None, multisample: Default::default(), multiview_mask: None, cache: None })
}

impl App {
    fn finish(&mut self) {
        #[cfg(feature = "ocio")]
        if let Some(g) = self.gpu.as_ref() { if let Some(st) = g.video.as_ref().and_then(|v| v.ocio.as_ref()) { println!("{}", st.report(&g.device, &g.queue)); } }
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, el: &ActiveEventLoop) { if self.gpu.is_none() { self.init(el); } }
    fn window_event(&mut self, el: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => { self.finish(); el.exit() }
            WindowEvent::Resized(s) => if let Some(g) = self.gpu.as_mut() { g.config.width = s.width.max(1); g.config.height = s.height.max(1); g.surface.configure(&g.device, &g.config); if let Some(v) = g.video.as_mut() { v.textures.clear(); v.bind_group = None;
                #[cfg(feature = "ocio")] if let Some(st) = v.ocio.as_mut() { st.set_fit(&g.queue, (g.config.width, g.config.height)); } } },
            WindowEvent::RedrawRequested => { self.draw(); if self.seconds > 0.0 && self.t0.get_or_insert_with(Instant::now).elapsed().as_secs_f32() >= self.seconds { self.finish(); el.exit(); return; } if let Some(g) = &self.gpu { g.window.request_redraw(); } }
            WindowEvent::KeyboardInput { event, .. } if event.state.is_pressed() => { if let winit::keyboard::Key::Named(winit::keyboard::NamedKey::Escape) = event.logical_key { self.finish(); el.exit(); } }
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
    let hw = args.iter().any(|a| a == "--hw");
    let snap = args.iter().position(|a| a == "--snap").and_then(|i| args.get(i + 1).cloned());
    let threads = args.iter().position(|a| a == "--threads").and_then(|i| args.get(i + 1)).and_then(|t| t.parse().ok()).unwrap_or(0);
    let arg = |k: &str| args.iter().position(|a| a == k).and_then(|i| args.get(i + 1).cloned());
    #[cfg(feature = "ocio")]
    let ocio_opts = arg("--ocio").map(|config| {
        let display = arg("--display").unwrap_or_else(|| "sRGB - Display".into());
        let eotf = match arg("--eotf").as_deref() { Some("srgb") => 0, Some("2.4") => 1, Some("pq") => 2, Some("none") => 3, _ => if display.contains("PQ") || display.contains("2084") { 2 } else if display.contains("1886") { 1 } else { 0 } };
        ocio::Opts { config, cs: arg("--cs").unwrap_or_else(|| "Camera Rec.709".into()), display, view: arg("--view").unwrap_or_else(|| "ACES 1.0 - SDR Video".into()),
            route: if arg("--route").as_deref() == Some("lut") { ocio::Route::Lut } else { ocio::Route::Native }, eotf, verify: args.iter().any(|a| a == "--ocio-verify"), edge: arg("--lut-edge").and_then(|e| e.parse().ok()).unwrap_or(65), gpu_ts: args.iter().any(|a| a == "--gpu-ts") }
    });
    #[cfg(not(feature = "ocio"))]
    if arg("--ocio").is_some() { eprintln!("built without --features ocio"); std::process::exit(2); }
    let el = EventLoop::new().unwrap();
    let seconds = arg("--seconds").and_then(|v| v.parse().ok()).unwrap_or(0.0);   // 0 = until Esc / close
    let screen = arg("--screen").and_then(|v| v.parse().ok());
    let mut app = App { color_space, file, threads, hw, snap, seconds, screen, #[cfg(feature = "ocio")] ocio_opts, ..Default::default() };
    el.run_app(&mut app).unwrap();
    println!("frames presented: {}", app.frames);
}
