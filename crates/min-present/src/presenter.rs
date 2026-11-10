//! The surface owner: picks the format + colour space for the wanted output (SDR / EDR / PQ), configures, draws, and
//! carries S1's two Metal facts — wgpu 30 configures EDR itself, and the BT.2100 PQ colour space must be set on the
//! layer by name (wgpu-hal 30.0.1 crashes in `configure` with `Bt2100Pq`).
use crate::engine::{self, DisplayMode, Engine, OutMode};
use crate::Error;
use std::ffi::c_void;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Output { /// EDR when the surface offers it, else SDR
    Auto, Sdr, Edr, Pq }

#[derive(Clone, Debug)]
pub struct OutputInfo { pub output: Output, pub format: wgpu::TextureFormat, pub color_space: wgpu::SurfaceColorSpace, pub layer_workaround: bool, pub adapter: String, pub backend: String, pub features: wgpu::Features, pub present_modes: Vec<wgpu::PresentMode> }

pub struct Presenter {
    pub engine: Engine,
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    info: OutputInfo,
    #[allow(dead_code)] ns_view: Option<*mut c_void>,
    /// the surface offers Rgb10a2Unorm in sRGB (the SDR-monitor surface, canon row 220)
    rgb10_ok: bool,
    /// an SDR monitor (Windows): the engine draws into this fp16 buffer as ever, one pass encodes it to the 10-bit sRGB surface
    sdr10: Option<Sdr10>,
    /// frames in a row the surface refused to hand over (not counting an occluded window) — see [`Presenter::stalled`]
    fail_streak: u32,
}

/// Canon row 220's final stage: the engine's linear fp16 picture (1.0 = SDR white) → sRGB-encoded 10-bit, clamped to SDR —
/// DWM gets finished code values, so Windows' undithered float → 8-bit conversion (the banding) never runs.
struct Sdr10 { view: wgpu::TextureView, pipeline: wgpu::RenderPipeline, bind: wgpu::BindGroup }
const SDR10_WGSL: &str = r#"
@group(0) @binding(0) var src: texture_2d<f32>;
@vertex fn vs(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
  let p = array<vec2<f32>, 3>(vec2(-1.0, -1.0), vec2(3.0, -1.0), vec2(-1.0, 3.0));
  return vec4(p[i], 0.0, 1.0);
}
fn oetf(c: vec3<f32>) -> vec3<f32> { let x = clamp(c, vec3(0.0), vec3(1.0)); return select(1.055 * pow(x, vec3(1.0 / 2.4)) - 0.055, 12.92 * x, x <= vec3(0.0031308)); }
@fragment fn fs(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
  let c = textureLoad(src, vec2<i32>(pos.xy), 0);
  return vec4(oetf(c.rgb), 1.0);
}
"#;
impl Sdr10 {
    fn new(device: &wgpu::Device, w: u32, h: u32, out: wgpu::TextureFormat) -> Sdr10 {
        let tex = device.create_texture(&wgpu::TextureDescriptor { label: Some("sdr10 source"), size: wgpu::Extent3d { width: w.max(1), height: h.max(1), depth_or_array_layers: 1 }, mip_level_count: 1, sample_count: 1, dimension: wgpu::TextureDimension::D2, format: wgpu::TextureFormat::Rgba16Float, usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING, view_formats: &[] });
        let view = tex.create_view(&Default::default());
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: Some("sdr10"), entries: &[wgpu::BindGroupLayoutEntry { binding: 0, visibility: wgpu::ShaderStages::FRAGMENT, ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: false }, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false }, count: None }] });
        let bind = device.create_bind_group(&wgpu::BindGroupDescriptor { label: Some("sdr10"), layout: &layout, entries: &[wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&view) }] });
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("sdr10"), source: wgpu::ShaderSource::Wgsl(SDR10_WGSL.into()) });
        let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some("sdr10"), bind_group_layouts: &[Some(&layout)], immediate_size: 0 });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor { label: Some("sdr10 encode"), layout: Some(&pl),
            vertex: wgpu::VertexState { module: &module, entry_point: Some("vs"), compilation_options: Default::default(), buffers: &[] },
            fragment: Some(wgpu::FragmentState { module: &module, entry_point: Some("fs"), compilation_options: Default::default(), targets: &[Some(wgpu::ColorTargetState { format: out, blend: None, write_mask: wgpu::ColorWrites::ALL })] }),
            primitive: Default::default(), depth_stencil: None, multisample: Default::default(), multiview_mask: None, cache: None });
        Sdr10 { view, pipeline, bind }
    }
}

/// The bundled DXC (canon row 226): beside the executable (the package), else the repo's `external/dxc/win64` (development:
/// `target/<profile>/QCView.exe` → two folders up).
fn bundled_dxc() -> Option<std::path::PathBuf> {
    let exe = std::env::current_exe().ok()?; let dir = exe.parent()?;
    [dir.join("dxcompiler.dll"), dir.join("../../external/dxc/win64/dxcompiler.dll")].into_iter().find(|p| p.is_file())
}

impl Presenter {
    /// The instance the app creates its surface from: DX12 on Windows (row 45; wgpu's default picks Vulkan on NVIDIA, whose
    /// swapchain is 8-bit there), the platform default elsewhere.
    pub fn instance() -> wgpu::Instance {
        let mut d = wgpu::InstanceDescriptor::new_without_display_handle();
        if cfg!(windows) { d.backends = wgpu::Backends::DX12; }
        // canon row 226: the bundled DXC, by explicit path — never whatever happens to be on PATH (rhombus had one only from
        // the Vulkan SDK); without it wgpu falls back to FXC and the minColor kernel took 71–78 s instead of 4.8 s
        if cfg!(windows) && let Some(dxc) = bundled_dxc() {
            log::info!("DX12 shader compiler: {}", dxc.display());
            d.backend_options.dx12.shader_compiler = wgpu::Dx12Compiler::DynamicDxc { dxc_path: dxc.to_string_lossy().into_owned() };
        } else if cfg!(windows) {
            log::warn!("no bundled dxcompiler.dll beside QCView.exe or in external/dxc/win64 — wgpu falls back to DXC on PATH or FXC (slow first colour)");
        }
        // wgpu's own switches (WGPU_DX12_COMPILER = fxc | dxc …) still win — the measurements
        d.backend_options = d.backend_options.with_env();
        wgpu::Instance::new(d)
    }
    /// `ns_view`: the window's NSView on macOS (for the PQ layer workaround and the layer report); None elsewhere.
    pub fn new(instance: &wgpu::Instance, surface: wgpu::Surface<'static>, size: (u32, u32), want: Output, ns_view: Option<*mut c_void>) -> Result<Presenter, Error> {
        let (adapter, device, queue) = engine::request(instance, Some(&surface))?;
        let ai = adapter.get_info();
        let caps = surface.get_capabilities(&adapter);
        let offers = |f: wgpu::TextureFormat, cs: wgpu::SurfaceColorSpaces| caps.format_capabilities.iter().any(|fc| fc.format == f && fc.color_spaces.contains(cs));
        let srgb8 = caps.formats.iter().copied().find(|f| matches!(f, wgpu::TextureFormat::Bgra8UnormSrgb | wgpu::TextureFormat::Rgba8UnormSrgb));
        let plain8 = caps.formats.iter().copied().find(|f| matches!(f, wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Rgba8Unorm));
        let edr_ok = offers(wgpu::TextureFormat::Rgba16Float, wgpu::SurfaceColorSpaces::EXTENDED_SRGB_LINEAR);
        let want = match want { Output::Auto => if edr_ok { Output::Edr } else { Output::Sdr }, w => w };
        let (format, color_space, layer_workaround, out) = match want {
            Output::Edr if edr_ok => (wgpu::TextureFormat::Rgba16Float, wgpu::SurfaceColorSpace::ExtendedSrgbLinear, false, OutMode::Linear),
            Output::Pq if cfg!(target_os = "macos") && edr_ok => (wgpu::TextureFormat::Rgba16Float, wgpu::SurfaceColorSpace::ExtendedSrgbLinear, true, OutMode::Pq),
            Output::Pq if offers(wgpu::TextureFormat::Rgb10a2Unorm, wgpu::SurfaceColorSpaces::BT2100_PQ) => (wgpu::TextureFormat::Rgb10a2Unorm, wgpu::SurfaceColorSpace::Bt2100Pq, false, OutMode::Pq),
            Output::Pq if offers(wgpu::TextureFormat::Rgba16Float, wgpu::SurfaceColorSpaces::BT2100_PQ) => (wgpu::TextureFormat::Rgba16Float, wgpu::SurfaceColorSpace::Bt2100Pq, false, OutMode::Pq),
            _ => match (srgb8, plain8) {
                (Some(f), _) => (f, wgpu::SurfaceColorSpace::Auto, false, OutMode::SdrLinear),
                (None, Some(f)) => (f, wgpu::SurfaceColorSpace::Auto, false, OutMode::SdrEncode),
                (None, None) => return Err(Error::NoFormat),
            },
        };
        let output = match out { OutMode::Linear => Output::Edr, OutMode::Pq => Output::Pq, _ => Output::Sdr };
        let config = wgpu::SurfaceConfiguration { usage: wgpu::TextureUsages::RENDER_ATTACHMENT, format, color_space, width: size.0.max(1), height: size.1.max(1), present_mode: wgpu::PresentMode::AutoVsync, alpha_mode: caps.alpha_modes[0], view_formats: vec![], desired_maximum_frame_latency: 2 };
        surface.configure(&device, &config);
        #[cfg(target_os = "macos")]
        if layer_workaround { if let Some(v) = ns_view { mac::set_layer_colorspace(v, "kCGColorSpaceITUR_2100_PQ"); } }
        let info = OutputInfo { output, format, color_space, layer_workaround, adapter: ai.name.clone(), backend: format!("{:?}", ai.backend), features: device.features(), present_modes: caps.present_modes.clone() };
        let mut engine = Engine::new(device, queue, format);
        engine.set_viewport(config.width, config.height);
        engine.set_display(DisplayMode { out, white: 100.0 });
        let rgb10_ok = offers(wgpu::TextureFormat::Rgb10a2Unorm, wgpu::SurfaceColorSpaces::SRGB) && format == wgpu::TextureFormat::Rgba16Float;
        Ok(Presenter { engine, surface, config, info, ns_view, rgb10_ok, sdr10: None, fail_streak: 0 })
    }
    pub fn info(&self) -> &OutputInfo { &self.info }
    /// Canon row 220 (Windows): the window's monitor shows no HDR → a 10-bit sRGB surface fed by one encode pass; HDR → the
    /// fp16 scRGB surface as built. The engine keeps drawing linear fp16 either way. False when nothing changed (or the
    /// surface can't: then the fp16 surface stays).
    pub fn set_sdr_surface(&mut self, on: bool) -> bool {
        if on == self.sdr10.is_some() || (on && !self.rgb10_ok) { return false }
        if on {
            self.config.format = wgpu::TextureFormat::Rgb10a2Unorm; self.config.color_space = wgpu::SurfaceColorSpace::Srgb;
            self.sdr10 = Some(Sdr10::new(&self.engine.device, self.config.width, self.config.height, self.config.format));
        } else {
            self.config.format = self.info.format; self.config.color_space = self.info.color_space; self.sdr10 = None;
        }
        self.surface.configure(&self.engine.device, &self.config);
        log::info!("surface: {} ({:?}, {:?})", if on { "10-bit sRGB for an SDR monitor" } else { "fp16 scRGB for an HDR monitor" }, self.config.format, self.config.color_space);
        true
    }
    pub fn sdr_surface(&self) -> bool { self.sdr10.is_some() }
    pub fn size(&self) -> (u32, u32) { (self.config.width, self.config.height) }
    pub fn resize(&mut self, w: u32, h: u32) {
        self.config.width = w.max(1); self.config.height = h.max(1);
        self.surface.configure(&self.engine.device, &self.config);
        #[cfg(target_os = "macos")]
        if self.info.layer_workaround { if let Some(v) = self.ns_view { mac::set_layer_colorspace(v, "kCGColorSpaceITUR_2100_PQ"); } }
        if self.sdr10.is_some() { self.sdr10 = Some(Sdr10::new(&self.engine.device, self.config.width, self.config.height, self.config.format)); }
        self.engine.set_viewport(self.config.width, self.config.height);
    }
    /// One present. `before_present` runs between the submit and the present (winit's `pre_present_notify`).
    /// Returns false when the surface had nothing to give (occluded / lost — reconfigured for the next call).
    pub fn draw(&mut self, before_present: impl FnOnce()) -> bool {
        let frame = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(f) | wgpu::CurrentSurfaceTexture::Suboptimal(f) => f,
            wgpu::CurrentSurfaceTexture::Occluded => { log::warn!("surface occluded ({}x{} {:?})", self.config.width, self.config.height, self.config.format); return false }
            other => { self.fail_streak += 1; if self.fail_streak <= 3 || self.fail_streak % 600 == 0 { log::warn!("get_current_texture: {other:?} (×{})", self.fail_streak); } self.surface.configure(&self.engine.device, &self.config); return false }
        };
        self.fail_streak = 0;
        let view = frame.texture.create_view(&Default::default());
        let mut enc = self.engine.device.create_command_encoder(&Default::default());
        if let Some(s) = &self.sdr10 {
            self.engine.encode(&mut enc, &s.view);
            let mut rp = enc.begin_render_pass(&wgpu::RenderPassDescriptor { label: Some("sdr10 encode"), color_attachments: &[Some(wgpu::RenderPassColorAttachment { view: &view, depth_slice: None, resolve_target: None, ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store } })], depth_stencil_attachment: None, timestamp_writes: None, occlusion_query_set: None, multiview_mask: None });
            rp.set_pipeline(&s.pipeline); rp.set_bind_group(0, &s.bind, &[]); rp.draw(0..3, 0..1);
        } else {
            self.engine.encode(&mut enc, &view);
        }
        self.engine.queue.submit([enc.finish()]);
        before_present();
        self.engine.queue.present(frame);
        true
    }
    /// The window as it is drawn — the picture and the interface — rendered once more into a texture and read back (linear,
    /// RGBA f32). For the scripted docs screenshots (`apps/qcv` `shots.rs`): no OS screenshot is involved. Float surfaces only
    /// (the Mac's; Windows' HDR one) — the 10-bit SDR path encodes for the swapchain and is not what a picture file wants.
    pub fn capture_window(&mut self) -> Option<(Vec<f32>, u32, u32)> {
        // the engine draws linear fp16 on a float surface and into the 10-bit SDR stage's source alike (Windows, row 220)
        if self.sdr10.is_none() && self.config.format != wgpu::TextureFormat::Rgba16Float { return None }
        let (w, h) = (self.config.width.max(1), self.config.height.max(1));
        let tex = self.engine.device.create_texture(&wgpu::TextureDescriptor { label: Some("window capture"), size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 }, mip_level_count: 1, sample_count: 1, dimension: wgpu::TextureDimension::D2, format: wgpu::TextureFormat::Rgba16Float, usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC, view_formats: &[] });
        let view = tex.create_view(&Default::default());
        let mut enc = self.engine.device.create_command_encoder(&Default::default());
        self.engine.encode(&mut enc, &view);
        self.engine.queue.submit([enc.finish()]);
        Some((crate::readback::read_rgba16f(&self.engine.device, &self.engine.queue, &tex, w, h), w, h))
    }
    /// The surface has refused a frame this many times in a row: the graphics device is gone or cannot present here (a machine
    /// with no usable GPU loses its device at the first present and every later frame fails; a resize costs one or two).
    pub fn stalled(&self) -> bool { self.fail_streak >= 120 }
    /// Can the surface still hand over a frame? Asked once after the first present, before the window is shown: the frame
    /// is taken and dropped unpresented. False = the first present killed the device (no usable GPU).
    pub fn surface_alive(&mut self) -> bool {
        match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(_) | wgpu::CurrentSurfaceTexture::Suboptimal(_) | wgpu::CurrentSurfaceTexture::Occluded | wgpu::CurrentSurfaceTexture::Timeout => true,
            other => { log::error!("the surface is dead after the first present: {other:?}"); false }
        }
    }
    /// macOS: the potential EDR headroom of the screen holding the window (≤ 1 = no HDR there) — read for the Color drawer's
    /// "highlights clip" pill only (canon row 222); None elsewhere.
    pub fn window_screen_headroom(&self) -> Option<f64> {
        #[cfg(target_os = "macos")] { if let Some(v) = self.ns_view { return mac::window_screen_headroom(v) } }
        None
    }
    /// What the Metal layer ended up with (format, colour space, EDR flag) and every screen's EDR headroom (current, potential).
    pub fn layer_report(&self) -> String {
        #[cfg(target_os = "macos")] { if let Some(v) = self.ns_view { return mac::report_layer(v) } }
        String::new()
    }
    /// Every screen's current EDR headroom (macOS; empty elsewhere) — `maximumExtendedDynamicRangeColorComponentValue`.
    #[cfg(target_os = "macos")]
    pub fn edr_headroom() -> Vec<f64> { mac::edr_now() }
    #[cfg(not(target_os = "macos"))]
    pub fn edr_headroom() -> Vec<f64> { vec![] }
}

#[cfg(target_os = "macos")]
mod mac {
    use objc2_app_kit::{NSScreen, NSView};
    use objc2_quartz_core::CAMetalLayer;
    use std::ffi::c_void;
    fn metal_layer(ns_view: *mut c_void) -> Option<objc2::rc::Retained<CAMetalLayer>> {
        // SAFETY: the caller hands us the NSView of the window it owns, on the main thread.
        let view: &NSView = unsafe { &*(ns_view as *const NSView) };
        let layer = view.layer()?;
        let mut candidates = vec![layer.clone()];
        if let Some(subs) = unsafe { layer.sublayers() } { candidates.extend(subs.iter().map(|l| l.clone())); }
        candidates.into_iter().find_map(|l| l.downcast::<CAMetalLayer>().ok())
    }
    /// Set the CAMetalLayer's colour space by its CoreGraphics NAME (the kCGColorSpace* constants' values are their own names).
    pub fn set_layer_colorspace(ns_view: *mut c_void, name: &str) {
        let Some(layer) = metal_layer(ns_view) else { log::warn!("no CAMetalLayer to set {name} on"); return };
        let cs = objc2_core_graphics::CGColorSpace::with_name(Some(&objc2_core_foundation::CFString::from_str(name)));
        if cs.is_none() { log::warn!("CGColorSpace {name} unavailable"); }
        layer.setColorspace(cs.as_deref());
        layer.setWantsExtendedDynamicRangeContent(true);
    }
    pub fn edr_now() -> Vec<f64> {
        let mtm = objc2::MainThreadMarker::new().expect("main thread");
        NSScreen::screens(mtm).iter().map(|s| s.maximumExtendedDynamicRangeColorComponentValue()).collect()
    }
    pub fn window_screen_headroom(ns_view: *mut c_void) -> Option<f64> {
        // SAFETY: as metal_layer — the window's own NSView, on the main thread
        let view: &NSView = unsafe { &*(ns_view as *const NSView) };
        Some(view.window()?.screen()?.maximumPotentialExtendedDynamicRangeColorComponentValue())
    }
    pub fn report_layer(ns_view: *mut c_void) -> String {
        let Some(metal) = metal_layer(ns_view) else { return "layer: no CAMetalLayer".into() };
        let mtm = objc2::MainThreadMarker::new().expect("main thread");
        let screens: Vec<(f64, f64)> = NSScreen::screens(mtm).iter().map(|s| (s.maximumExtendedDynamicRangeColorComponentValue(), s.maximumPotentialExtendedDynamicRangeColorComponentValue())).collect();
        format!("layer: wantsEDR={} pixelFormat={:?} colorspace={:?}; screens (EDR now, potential) {:?}", metal.wantsExtendedDynamicRangeContent(), metal.pixelFormat(), metal.colorspace(), screens)
    }
}
