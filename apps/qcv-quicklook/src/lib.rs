//! QCView's Quick Look preview, the Rust half (canon rows 605-611; `docs/phase2/quicklook-spec.md`). The Swift shell owns
//! the view and its CAMetalLayer and passes the pointer on; everything drawn is ours: the picture through the family's
//! presenter, and ONE bar over its foot - the layer and the colour as min-ui pickers, a line of facts - that shows while
//! the pointer moves over the picture. The plain sandbox: this process reads the one file it was handed, and writes
//! only its own state file. All calls come on the main thread; decoding and the colour kernel's build run on workers.
use kurbo::{Affine, Rect};
use min_i18n::trc;
use min_present::{Chroma, ColorMeta, ColorPass, Frame, Matrix, Output, Pixels, PlanarLayout, Plane, Presenter, Primaries, Range, Transfer};
use min_theme::tokens as t;
use min_ui::{anim::Ramp, fonts::draw_layout, paint, picker::Picker, Button, Event, Family, Fonts, UiLayer};
use qcv_color::{model::{builtins, ClipChain, Preset}, Chain, MinColorPass};
use std::{cell::RefCell, ffi::{c_char, c_void, CStr}, path::PathBuf, rc::Rc, sync::mpsc, time::{Duration, Instant}};

macro_rules! catalogs { ($($code:literal),*) => { &[$(($code, include_str!(concat!("../../qcv/i18n/qcview_", $code, ".po")))),*] } }
/// QCView's own catalogs: the extension's strings live in them (context `QuickLook`, and `ColorPanel` for "Color Off").
const CATALOGS: &[(&str, &str)] = catalogs!("en", "de", "es", "fr", "it", "ja", "ko", "pl", "pt_BR", "ru", "uk", "zh_CN");
const CTX: &str = "QuickLook";

/// The bar stays this long after the pointer last moved over the picture (row 611: "about two seconds").
const REST: Duration = Duration::from_millis(2000);

/// The one minColor pass, shared between this shell (its chain changes) and the engine's slot (it runs) - QCView's own.
struct SharedPass(Rc<RefCell<MinColorPass>>);
impl ColorPass for SharedPass {
    fn input(&self) -> min_present::Space { self.0.borrow().input() }
    fn output(&self) -> min_present::Space { self.0.borrow().output() }
    fn run(&mut self, enc: &mut wgpu::CommandEncoder, src: &wgpu::Texture, dst: &wgpu::Texture) { self.0.borrow_mut().run(enc, src, dst) }
}

pub struct Preview {
    presenter: Presenter, layer: UiLayer, fonts: Fonts, scale: f64, size: (u32, u32),
    path: String, state: Option<PathBuf>, dims: (u32, u32), facts: String, error: Option<String>,
    layers: Picker, colors: Picker, presets: Vec<Preset>,
    /// the kernel: being built, built, and whether the engine holds it
    pass_rx: Option<mpsc::Receiver<MinColorPass>>, pass: Option<Rc<RefCell<MinColorPass>>>, bound: bool,
    /// the decode in flight (its number; a later pick outdates it)
    job: u64, rx: mpsc::Receiver<(u64, Result<Frame, String>)>, tx: mpsc::Sender<(u64, Result<Frame, String>)>,
    inside: bool, moved: Instant, pointer: (f64, f64), bar: Ramp, bar_rect: Rect, dirty: bool,
}

fn frame(path: &str, layer: &str) -> Result<Frame, String> {
    let img = min_image::load(path, if layer == "default" { "" } else { layer }, 8).map_err(|e| e.to_string())?;
    let n = if img.alpha { 4 } else { 3 }; if img.planes.len() < n { return Err("fewer planes than a picture needs".into()) }
    let (bps, code_bits, float) = match img.sample { min_image::Sample::U8 => (1, 8, false), min_image::Sample::U16 => (2, 16, false), min_image::Sample::F16 => (2, 16, true) };
    let meta = ColorMeta { matrix: Matrix::Identity, range: Range::Full, transfer: if float { Transfer::Linear } else { Transfer::Srgb }, primaries: Primaries::Bt709, bit_depth: code_bits as _, chroma: Chroma::Rgb, range_forced: false, rotation: 0, sar: (1, 1), mastering: None, raw: None };
    let (w, h, pitch, alpha) = (img.width, img.height, img.pitch, img.alpha); let mut planes = img.planes; planes.truncate(n);
    Ok(Frame { width: w, height: h, pixels: Pixels::Planar { planes: planes.into_iter().map(|p| Plane::owned(p, w, h, pitch)).collect(), layout: PlanarLayout::Rgb { bytes_per_sample: bps, code_bits, float, alpha, legal_expand: false } }, meta })
}

fn folder(path: &str) -> String { std::path::Path::new(path).parent().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default() }

/// The picks carried from the last file (row 609): (layer, colour preset) when that file was in this folder.
fn carried(state: Option<&PathBuf>, path: &str) -> (String, String) {
    let Some(v) = state.and_then(|p| std::fs::read(p).ok()).and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok()) else { return Default::default() };
    if v["folder"].as_str() != Some(&folder(path)) { return Default::default() }
    (v["layer"].as_str().unwrap_or_default().to_string(), v["color"].as_str().unwrap_or_default().to_string())
}

impl Preview {
    fn keep(&self) {
        let Some(p) = &self.state else { return };
        let color = if self.colors.selected == 0 { "" } else { self.colors.value() };
        let _ = std::fs::write(p, serde_json::json!({ "folder": folder(&self.path), "layer": self.layers.value(), "color": color }).to_string());
    }
    fn decode(&mut self) {
        self.job += 1; let (job, tx, path, layer) = (self.job, self.tx.clone(), self.path.clone(), self.layers.value().to_string());
        let _ = std::thread::Builder::new().name("ql-decode".into()).spawn(move || { let _ = tx.send((job, frame(&path, &layer))); });
    }
    /// The colour as picked: off = the engine's own conversion from the file's tags (QCView with no chain); else the
    /// preset's chain in the one kernel, built on a worker the first time it is wanted.
    fn color(&mut self) {
        let chain: Option<Chain> = self.colors.selected.checked_sub(1).and_then(|i| self.presets.get(i)).and_then(|p| match &p.stored.chain { ClipChain::MinColor(c) => Some(*c), _ => None });
        match (chain, &self.pass) {
            (None, _) => if self.bound { self.presenter.engine.set_color_pass(None); self.bound = false; },
            (Some(c), Some(pass)) => { pass.borrow_mut().set_chain(c); if !self.bound { self.presenter.engine.set_color_pass(Some(Box::new(SharedPass(pass.clone())))); self.bound = true; } }
            (Some(c), None) => if self.pass_rx.is_none() {
                let (tx, rx) = mpsc::channel(); let (d, q) = (self.presenter.engine.device.clone(), self.presenter.engine.queue.clone());
                let _ = std::thread::Builder::new().name("ql-mincolor".into()).spawn(move || { let _ = tx.send(MinColorPass::new(&d, &q, c)); });
                self.pass_rx = Some(rx);
            },
        }
        self.dirty = true;
    }
    /// What the workers finished. True while one is still out.
    fn poll(&mut self) -> bool {
        while let Ok((job, r)) = self.rx.try_recv() {
            if job != self.job { continue }
            match r { Ok(f) => match self.presenter.engine.show(&f) { Ok(()) => self.error = None, Err(e) => self.error = Some(e.to_string()) }, Err(e) => { self.presenter.engine.clear_picture(); self.error = Some(e) } }
            self.dirty = true;
        }
        if let Some(Ok(p)) = self.pass_rx.as_ref().map(|rx| rx.try_recv()) { self.pass = Some(Rc::new(RefCell::new(p))); self.pass_rx = None; self.color(); }
        self.pass_rx.is_some()
    }
    fn bar_wanted(&self, now: Instant) -> bool {
        self.layers.is_open() || self.colors.is_open() || (self.inside && (now.duration_since(self.moved) < REST || self.bar_rect.contains(kurbo::Point::new(self.pointer.0, self.pointer.1))))
    }
    fn draw(&mut self, now: Instant) {
        let k = self.scale; let (w, h) = (self.size.0 as f64, self.size.1 as f64);
        let mut scene = vello::Scene::new();
        if let Some(e) = &self.error {
            // a file we cannot show says why, in our own view (row 611)
            let text = min_i18n::fill(trc(CTX, "This file cannot be shown: %1"), &[e]);
            let l = self.fonts.layout_width(&text, Family::Sans, t::BODY_SIZE_M * k as f32, t::FONT_WEIGHT_REGULAR as f32, paint::color(t::PICKER_CONTENT_COLOR), Some((w - 80.0 * k).max(100.0) as f32));
            draw_layout(&mut scene, &l, Affine::translate(((w - l.width() as f64) / 2.0, (h - l.height() as f64) / 2.0)));
        }
        self.bar.set_target(if self.bar_wanted(now) { 1.0 } else { 0.0 }, now);
        let a = self.bar.value(now);
        // the bar: over the foot of the picture, QuickTime's place for it; narrow views (Finder's column) get none
        let room = w >= 360.0 * k && h >= 200.0 * k;
        if a > 0.01 && room {
            let m = 12.0 * k; let bar = Rect::new(m, h - m - 44.0 * k, w - m, h - m); self.bar_rect = bar;
            let mut b = vello::Scene::new();
            paint::rounded(&mut b, bar, 10.0 * k, t::BACKGROUND_LAYER_1_COLOR.with_alpha(0.9));
            let mut x = bar.x0 + 10.0 * k; let cy = bar.center().y;
            let chip = |p: &mut Picker, fonts: &mut Fonts, b: &mut vello::Scene, x: &mut f64, max: f64| {
                let (pw, ph) = p.measure(fonts, k); let pw = pw.clamp(96.0 * k, max);
                p.rect = Rect::new(*x, cy - ph / 2.0, *x + pw, cy + ph / 2.0); p.paint(b, fonts, k, now); *x += pw + 8.0 * k;
            };
            if self.layers.options.len() > 1 { chip(&mut self.layers, &mut self.fonts, &mut b, &mut x, 260.0 * k); } else { self.layers.rect = Rect::ZERO; }
            chip(&mut self.colors, &mut self.fonts, &mut b, &mut x, 260.0 * k);
            let l = self.fonts.layout_fit(&self.facts, Family::Sans, t::BODY_SIZE_M * k as f32, t::FONT_WEIGHT_REGULAR as f32, paint::color(t::PICKER_CONTENT_COLOR), (bar.x1 - x - 14.0 * k).max(0.0));
            if bar.x1 - x > 120.0 * k { draw_layout(&mut b, &l, Affine::translate((bar.x1 - 14.0 * k - l.width() as f64, cy - l.height() as f64 / 2.0))); }
            self.layers.paint_menu(&mut b, &mut self.fonts, k); self.colors.paint_menu(&mut b, &mut self.fonts, k);
            scene.push_layer(peniko::Fill::NonZero, peniko::Mix::Normal, a, Affine::IDENTITY, &Rect::new(0.0, 0.0, w, h)); scene.append(&b, None); scene.pop_layer();
        } else { self.bar_rect = Rect::ZERO; self.layers.rect = Rect::ZERO; self.colors.rect = Rect::ZERO; }
        let dst = self.layer.scene_mut(); dst.reset(); dst.append(&scene, None);
        self.layer.render(&self.presenter.engine.device, &self.presenter.engine.queue);
        self.presenter.draw(|| {});
        self.dirty = false;
    }
}

fn text(p: *const c_char) -> String { if p.is_null() { String::new() } else { unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned() } }

/// Open `path` in the view whose backing layer is `ca_layer` (a CAMetalLayer). Sizes in pixels. `state_dir`: a folder
/// this process may write (its container's Application Support) or null; `languages`: the system's preferred ones,
/// comma-separated, first wanted first. Returns null when no surface could be made (the shell then reports an error).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn qlp_open(ca_layer: *mut c_void, ns_view: *mut c_void, w: u32, h: u32, scale: f64, path: *const c_char, state_dir: *const c_char, languages: *const c_char) -> *mut Preview {
    static ONCE: std::sync::Once = std::sync::Once::new();
    let langs = text(languages);
    ONCE.call_once(|| { min_i18n::register(CATALOGS); for l in langs.split(',') { let l = l.trim().replace('-', "_"); if l.is_empty() { continue } if l == "en" || l.starts_with("en_") { break } if let Some(code) = min_i18n::resolve(&l) { min_i18n::set_language(code); break } } });
    let path = text(path); let state = Some(text(state_dir)).filter(|s| !s.is_empty()).map(|d| { let _ = std::fs::create_dir_all(&d); PathBuf::from(d).join("preview-state.json") });
    let instance = Presenter::instance();
    let Ok(surface) = (unsafe { instance.create_surface_unsafe(wgpu::SurfaceTargetUnsafe::CoreAnimationLayer(ca_layer)) }) else { return std::ptr::null_mut() };
    let (w, h) = (w.max(1), h.max(1));
    let Ok(mut presenter) = Presenter::new(&instance, surface, (w, h), Output::Auto, Some(ns_view)) else { return std::ptr::null_mut() };
    presenter.engine.set_ui_scale(scale as f32);
    let Ok(layer) = UiLayer::new(&presenter.engine.device, w, h) else { return std::ptr::null_mut() };
    presenter.engine.set_overlay(Some(layer.view()));
    let names = min_image::exr::layers(&path).unwrap_or_else(|_| vec!["default".into()]);
    let (was_layer, was_color) = carried(state.as_ref(), &path);
    let presets: Vec<Preset> = builtins().into_iter().filter(|p| matches!(p.stored.chain, ClipChain::MinColor(_))).collect();
    let mut color_names = vec![trc("ColorPanel", "Color Off").to_string()]; color_names.extend(presets.iter().map(|p| p.name.clone()));
    let layer_at = names.iter().position(|n| *n == was_layer).unwrap_or(0);
    let color_at = presets.iter().position(|p| p.name == was_color).map(|i| i + 1).unwrap_or(0);
    let dims = min_image::dimensions(&path, if names[layer_at] == "default" { "" } else { &names[layer_at] }).unwrap_or((1920, 1080));
    let kind = match min_image::exr::sample_kind(&path) { Some(0) => trc(CTX, "32-bit integer"), Some(1) => trc(CTX, "16-bit float"), Some(2) => trc(CTX, "32-bit float"), _ => "" };
    let facts = [format!("{} × {}", dims.0, dims.1), kind.to_string(), min_image::exr::compression(&path).unwrap_or_default().to_string()].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join("  ·  ");
    let (tx, rx) = mpsc::channel(); let now = Instant::now();
    let mut p = Preview { presenter, layer, fonts: Fonts::new(), scale, size: (w, h), path, state, dims, facts, error: None,
        layers: Picker::new(names, layer_at), colors: Picker::new(color_names, color_at), presets, pass_rx: None, pass: None, bound: false,
        job: 0, rx, tx, inside: false, moved: now - REST, pointer: (-1.0, -1.0), bar: Ramp::new(0.0, 160), bar_rect: Rect::ZERO, dirty: true };
    p.decode(); p.color();
    Box::into_raw(Box::new(p))
}

/// The picture's size, for the panel's own size.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn qlp_size(p: *mut Preview, w: *mut u32, h: *mut u32) { let s = unsafe { &*p }; unsafe { *w = s.dims.0; *h = s.dims.1; } }

#[unsafe(no_mangle)]
pub unsafe extern "C" fn qlp_resize(p: *mut Preview, w: u32, h: u32, scale: f64) { let s = unsafe { &mut *p }; let (w, h) = (w.max(1), h.max(1));
    if s.size == (w, h) && s.scale == scale { return } s.size = (w, h); s.scale = scale;
    s.presenter.resize(w, h); s.presenter.engine.set_ui_scale(scale as f32); s.layer.resize(&s.presenter.engine.device, w, h); s.presenter.engine.set_overlay(Some(s.layer.view()));
    s.layers.close(); s.colors.close(); s.dirty = true; }

/// kind: 0 move, 1 primary down, 2 primary up, 3 the pointer left. Pixels, the top left the origin.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn qlp_pointer(p: *mut Preview, x: f64, y: f64, kind: i32) { let s = unsafe { &mut *p }; let now = Instant::now();
    let e = match kind { 0 => Event::Move { x, y }, 1 => Event::Down { x, y, button: Button::Primary }, 2 => Event::Up { x, y, button: Button::Primary }, _ => Event::Leave };
    s.inside = kind != 3; if kind != 3 { s.pointer = (x, y); s.moved = now; }
    let bounds = Rect::new(0.0, 0.0, s.size.0 as f64, s.size.1 as f64);
    // an open menu takes every press (min-ui's rule); the other picker does not see it
    let first_colors = s.colors.is_open();
    let (layer, color) = if first_colors { let c = s.colors.event(&e, now, &mut s.fonts, bounds, s.scale); (if c.2 { (None, false, false) } else { s.layers.event(&e, now, &mut s.fonts, bounds, s.scale) }, c) }
        else { let l = s.layers.event(&e, now, &mut s.fonts, bounds, s.scale); (l, if l.2 { (None, false, false) } else { s.colors.event(&e, now, &mut s.fonts, bounds, s.scale) }) };
    if layer.0.is_some() { s.decode(); s.keep(); }
    if color.0.is_some() { s.color(); s.keep(); }
    s.dirty = true;
}

/// Called at the display's pace by the shell: takes what the workers finished and draws when something changed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn qlp_tick(p: *mut Preview) { let s = unsafe { &mut *p }; let now = Instant::now();
    s.poll();
    let want = s.bar_wanted(now);
    if s.dirty || s.bar.animating(now) || (want as u8 as f32 - s.bar.target()).abs() > 0.5 || s.layers.animating(now) || s.colors.animating(now) { s.draw(now); }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn qlp_close(p: *mut Preview) { if !p.is_null() { drop(unsafe { Box::from_raw(p) }); } }
