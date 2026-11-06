//! S1 present — scaffold. Opens a wgpu window, reports the surface's capabilities, and draws a linear-light
//! ramp 0→4 on an Rgba16Float swapchain so an EDR display shows where the headroom ends.
//! `--edr` asks wgpu for `SurfaceColorSpace::ExtendedSrgbLinear`, `--pq` for `Bt2100Pq` (wgpu 30 sets the
//! CAMetalLayer EDR flag + colourspace itself — no objc2 needed; objc2 only READS the layer back to verify).
//! Verdict and pass bars: docs/spikes/S1-present.md. This binary dies with the spike.
use std::sync::Arc;
use winit::{application::ApplicationHandler, event::WindowEvent, event_loop::{ActiveEventLoop, EventLoop}, window::{Window, WindowId}};

struct Gpu {
    window: Arc<Window>,
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    pipeline: wgpu::RenderPipeline,
}

#[derive(Default)]
struct App { gpu: Option<Gpu>, color_space: wgpu::SurfaceColorSpace, frames: u32 }

impl App {
    fn init(&mut self, el: &ActiveEventLoop) {
        let window = Arc::new(el.create_window(Window::default_attributes().with_title("S1 present — EDR ramp").with_inner_size(winit::dpi::LogicalSize::new(1280.0, 720.0))).unwrap());
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let surface = instance.create_surface(window.clone()).expect("surface");
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions { power_preference: wgpu::PowerPreference::HighPerformance, compatible_surface: Some(&surface), force_fallback_adapter: false, apply_limit_buckets: false })).expect("adapter");
        let info = adapter.get_info();
        println!("adapter: {} ({:?}, {:?}) driver {} {}", info.name, info.backend, info.device_type, info.driver, info.driver_info);
        let caps = surface.get_capabilities(&adapter);
        println!("surface formats: {:?}", caps.formats);
        for fc in &caps.format_capabilities { println!("  {:?} supports colour spaces {:?}", fc.format, fc.color_spaces); }
        println!("present modes:   {:?}", caps.present_modes);
        println!("alpha modes:     {:?}", caps.alpha_modes);
        let format = if caps.formats.contains(&wgpu::TextureFormat::Rgba16Float) { wgpu::TextureFormat::Rgba16Float } else { caps.formats[0] };
        println!("using format:    {:?}  colour space {:?}  scale factor {}", format, self.color_space, window.scale_factor());
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor { label: Some("s1"), ..Default::default() })).expect("device");
        let size = window.inner_size();
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT, format, color_space: self.color_space, width: size.width.max(1), height: size.height.max(1),
            present_mode: wgpu::PresentMode::AutoVsync, alpha_mode: caps.alpha_modes[0], view_formats: vec![], desired_maximum_frame_latency: 2,
        };
        surface.configure(&device, &config);
        #[cfg(target_os = "macos")]
        mac::report_layer(&window);
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("ramp"), source: wgpu::ShaderSource::Wgsl(include_str!("ramp.wgsl").into()) });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: None, bind_group_layouts: &[], immediate_size: 0 });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("ramp"), layout: Some(&layout),
            vertex: wgpu::VertexState { module: &shader, entry_point: Some("vs"), buffers: &[], compilation_options: Default::default() },
            fragment: Some(wgpu::FragmentState { module: &shader, entry_point: Some("fs"), targets: &[Some(wgpu::ColorTargetState { format, blend: None, write_mask: wgpu::ColorWrites::ALL })], compilation_options: Default::default() }),
            primitive: Default::default(), depth_stencil: None, multisample: Default::default(), multiview_mask: None, cache: None,
        });
        self.gpu = Some(Gpu { window, surface, device, queue, config, pipeline });
    }
    fn draw(&mut self) {
        let Some(g) = self.gpu.as_mut() else { return };
        let frame = match g.surface.get_current_texture() { wgpu::CurrentSurfaceTexture::Success(f) | wgpu::CurrentSurfaceTexture::Suboptimal(f) => f, other => { log::warn!("get_current_texture: {other:?}"); g.surface.configure(&g.device, &g.config); return; } };
        let view = frame.texture.create_view(&Default::default());
        let mut enc = g.device.create_command_encoder(&Default::default());
        {
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("ramp"), color_attachments: &[Some(wgpu::RenderPassColorAttachment { view: &view, depth_slice: None, resolve_target: None, ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store } })],
                depth_stencil_attachment: None, timestamp_writes: None, occlusion_query_set: None, multiview_mask: None,
            });
            pass.set_pipeline(&g.pipeline); pass.draw(0..3, 0..1);
        }
        g.queue.submit([enc.finish()]);
        g.window.pre_present_notify();
        g.queue.present(frame);
        self.frames += 1;
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, el: &ActiveEventLoop) { if self.gpu.is_none() { self.init(el); } }
    fn window_event(&mut self, el: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => el.exit(),
            WindowEvent::Resized(s) => if let Some(g) = self.gpu.as_mut() { g.config.width = s.width.max(1); g.config.height = s.height.max(1); g.surface.configure(&g.device, &g.config); },
            WindowEvent::RedrawRequested => { self.draw(); if let Some(g) = &self.gpu { g.window.request_redraw(); } }
            WindowEvent::KeyboardInput { event, .. } if event.state.is_pressed() => {
                if let winit::keyboard::Key::Named(winit::keyboard::NamedKey::Escape) = event.logical_key { el.exit(); }
            }
            _ => {}
        }
    }
}

#[cfg(target_os = "macos")]
mod mac {
    use objc2_app_kit::{NSScreen, NSView};
    use objc2_quartz_core::CAMetalLayer;
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    /// Read back what wgpu configured on the CAMetalLayer, and the screens' EDR headroom.
    pub fn report_layer(window: &winit::window::Window) {
        let RawWindowHandle::AppKit(h) = window.window_handle().unwrap().as_raw() else { return };
        // SAFETY: winit hands out the NSView of the window it owns; we are on the main thread.
        let view: &NSView = unsafe { &*(h.ns_view.as_ptr() as *const NSView) };
        let Some(layer) = view.layer() else { println!("layer: view has no layer"); return };
        // wgpu adds its CAMetalLayer as a SUBLAYER of the view's backing layer (NSViewBackingLayer).
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
    let color_space = if std::env::args().any(|a| a == "--edr") { wgpu::SurfaceColorSpace::ExtendedSrgbLinear } else if std::env::args().any(|a| a == "--pq") { wgpu::SurfaceColorSpace::Bt2100Pq } else { wgpu::SurfaceColorSpace::Auto };
    let el = EventLoop::new().unwrap();
    let mut app = App { color_space, ..Default::default() };
    el.run_app(&mut app).unwrap();
    println!("frames presented: {}", app.frames);
}
