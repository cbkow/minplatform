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
}

impl Presenter {
    /// The instance the app creates its surface from: DX12 on Windows (row 45; wgpu's default picks Vulkan on NVIDIA, whose
    /// swapchain is 8-bit there), the platform default elsewhere.
    pub fn instance() -> wgpu::Instance {
        let mut d = wgpu::InstanceDescriptor::new_without_display_handle();
        if cfg!(windows) { d.backends = wgpu::Backends::DX12; }
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
        Ok(Presenter { engine, surface, config, info, ns_view })
    }
    pub fn info(&self) -> &OutputInfo { &self.info }
    pub fn size(&self) -> (u32, u32) { (self.config.width, self.config.height) }
    pub fn resize(&mut self, w: u32, h: u32) {
        self.config.width = w.max(1); self.config.height = h.max(1);
        self.surface.configure(&self.engine.device, &self.config);
        #[cfg(target_os = "macos")]
        if self.info.layer_workaround { if let Some(v) = self.ns_view { mac::set_layer_colorspace(v, "kCGColorSpaceITUR_2100_PQ"); } }
        self.engine.set_viewport(self.config.width, self.config.height);
    }
    /// One present. `before_present` runs between the submit and the present (winit's `pre_present_notify`).
    /// Returns false when the surface had nothing to give (occluded / lost — reconfigured for the next call).
    pub fn draw(&mut self, before_present: impl FnOnce()) -> bool {
        let frame = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(f) | wgpu::CurrentSurfaceTexture::Suboptimal(f) => f,
            wgpu::CurrentSurfaceTexture::Occluded => { log::warn!("surface occluded ({}x{} {:?})", self.config.width, self.config.height, self.config.format); return false }
            other => { log::warn!("get_current_texture: {other:?}"); self.surface.configure(&self.engine.device, &self.config); return false }
        };
        let view = frame.texture.create_view(&Default::default());
        let mut enc = self.engine.device.create_command_encoder(&Default::default());
        self.engine.encode(&mut enc, &view);
        self.engine.queue.submit([enc.finish()]);
        before_present();
        self.engine.queue.present(frame);
        true
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
    pub fn report_layer(ns_view: *mut c_void) -> String {
        let Some(metal) = metal_layer(ns_view) else { return "layer: no CAMetalLayer".into() };
        let mtm = objc2::MainThreadMarker::new().expect("main thread");
        let screens: Vec<(f64, f64)> = NSScreen::screens(mtm).iter().map(|s| (s.maximumExtendedDynamicRangeColorComponentValue(), s.maximumPotentialExtendedDynamicRangeColorComponentValue())).collect();
        format!("layer: wantsEDR={} pixelFormat={:?} colorspace={:?}; screens (EDR now, potential) {:?}", metal.wantsExtendedDynamicRangeContent(), metal.pixelFormat(), metal.colorspace(), screens)
    }
}
