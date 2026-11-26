//! QCView's Quick Look preview, the Rust half (canon rows 605-611; `docs/phase2/quicklook-spec.md`). The Swift shell owns
//! the view and its CAMetalLayer and passes the pointer on; everything drawn is ours: the picture through the family's
//! presenter, and ONE bar over its foot - the layer and the colour as min-ui pickers, a line of facts - that shows while
//! the pointer moves over the picture. The plain sandbox: this process reads the one file it was handed, and writes
//! only its own state file. All calls come on the main thread; decoding and the colour kernel's build run on workers.
use kurbo::{Affine, Rect};
use min_i18n::trc;
use min_present::{Chroma, ColorMeta, ColorPass, Frame, Matrix, Output, Pixels, PlanarLayout, Plane, Presenter, Primaries, Range, Transfer};
use min_theme::tokens as t;
use min_ui::{anim::Ramp, fonts::draw_layout, paint, picker::Picker, ActionButton, Button, Event, Family, Fonts, Response, ScrubCmd, ScrubState, Scrubber, Tooltips, UiLayer};
use qcv_color::{model::{builtins, ClipChain, Preset}, Chain, MinColorPass};
use qcv_player::{source::Source, Action, AudioPlayer, HwDevice, Playback, ScrubOptions, TimecodeFormatter, Transport, TransportOptions};
use std::{cell::RefCell, ffi::{c_char, c_void, CStr}, path::PathBuf, rc::Rc, sync::mpsc, time::{Duration, Instant}};

pub mod thumb;

macro_rules! catalogs { ($($code:literal),*) => { &[$(($code, include_str!(concat!("../../qcv/i18n/qcview_", $code, ".po")))),*] } }
/// QCView's own catalogs: the extension's strings live in them (context `QuickLook`, and `ColorPanel` for "Color Off").
const CATALOGS: &[(&str, &str)] = catalogs!("en", "de", "es", "fr", "it", "ja", "ko", "pl", "pt_BR", "ru", "uk", "zh_CN");
const CTX: &str = "QuickLook";

unsafe extern "C" { fn qlp_host_log(msg: *const c_char); }
/// A line in the system's log (the shell's `NSLog`): read with `/usr/bin/log show --predicate 'process == "QCViewPreview"'`
/// - the full path, zsh has a builtin of that name. The extension's container is closed to other programs, so this
/// is its only record outside the screen.
fn note(s: &str) { if let Ok(c) = std::ffi::CString::new(s) { unsafe { qlp_host_log(c.as_ptr()) } } }

/// The bar stays this long after the pointer last moved over the picture (row 611: "about two seconds").
const REST: Duration = Duration::from_millis(2000);

/// The one minColor pass, shared between this shell (its chain changes) and the engine's slot (it runs) - QCView's own.
struct SharedPass(Rc<RefCell<MinColorPass>>);
impl ColorPass for SharedPass {
    fn input(&self) -> min_present::Space { self.0.borrow().input() }
    fn output(&self) -> min_present::Space { self.0.borrow().output() }
    fn run(&mut self, enc: &mut wgpu::CommandEncoder, src: &wgpu::Texture, dst: &wgpu::Texture) { self.0.borrow_mut().run(enc, src, dst) }
}

/// The scrub cache's budget in the extension (canon row 615, point 6; QCView's own is 512 MB).
const SCRUB_BYTES: usize = 128 << 20;
/// The transport bar's strings (their translations exist there); the play key named is K - Space is the panel's.
fn tip(s: &str) -> String { min_i18n::trc_hint("TransportBar", s) }

/// A video (an MXF) in the preview: QCView's player and the bar's transport parts (canon row 615).
struct Video {
    t: Transport, rew: ActionButton, play: ActionButton, ff: ActionButton, mute: ActionButton, big: ActionButton,
    scrub: Scrubber, tips: Tooltips, fmt: TimecodeFormatter, start: i32, tc: String, tc_rect: Rect,
    /// the panel plays on opening, a small Finder preview does not: decided at the first picture on screen, by the view's size
    auto: bool,
    /// a press on the picture itself (not on a control): its release there toggles play
    press: bool,
}
impl Video {
    fn open(path: &str, uploader: min_present::engine::Uploader, muted: bool) -> Result<Video, String> {
        let hw = HwDevice::platform(None);
        let opts = TransportOptions { hw: hw.clone(), scrub: Some(ScrubOptions { hw, budget_bytes: SCRUB_BYTES, ..Default::default() }), loop_enabled: true, uploader: Some(uploader), ..Default::default() };
        let mut t = Transport::open(&Source::from_path(path, None), opts).map_err(|e| e.to_string())?;
        if let Ok(a) = AudioPlayer::new() { t.set_audio(Box::new(a)); }
        let now = Instant::now(); if muted && !Playback::is_muted(&t) { Playback::apply(&mut t, Action::ToggleMute, now); }
        let i = Playback::timecode_info(&t); let fmt = TimecodeFormatter::new(i.frame_rate.0, i.frame_rate.1, i.drop_frame);
        let start = i.source_timecode.as_deref().filter(|o| !o.is_empty()).map(|tc| fmt.parse(tc).max(0)).unwrap_or(0);
        let mut scrub = Scrubber::new(); scrub.compact = true; scrub.bare = true;
        Ok(Video { t, rew: ActionButton::new("rewind", &tip("Rewind (hold; J)")).size_s().hold().glass(), play: ActionButton::new("play", &tip("Play (K)")).size_s().glass(), ff: ActionButton::new("fast-forward", &tip("Fast-forward (hold; L)")).size_s().hold().glass(),
            mute: ActionButton::new("speaker-high", &tip("Mute (M)")).size_s().glass(), big: ActionButton::new("play", &tip("Play (K)")).glass(), scrub, tips: Tooltips::default(), fmt, start, tc: String::new(), tc_rect: Rect::ZERO, auto: true, press: false })
    }
    fn state(&self) -> ScrubState { let p: &dyn Playback = &self.t; ScrubState { frames: p.frame_count(), fps: p.rate().as_f64(), current: p.current_frame(), in_point: p.in_point(), out_point: p.out_point(), playing: p.is_playing(), scrubbing: p.is_scrubbing(), cached: None } }
    fn sync(&mut self, now: Instant, scale: f64) {
        let (playing, has, muted, cur) = { let p: &dyn Playback = &self.t; (p.is_playing(), p.has_audio(), p.is_muted(), p.current_frame()) };
        for b in [&mut self.rew, &mut self.play, &mut self.ff, &mut self.mute, &mut self.big] { b.corners = min_ui::action_button::Corners::all(6.0 * scale); }
        for b in [&mut self.play, &mut self.big] { b.icon = if playing { "pause" } else { "play" }; b.tooltip = tip(if playing { "Pause (K)" } else { "Play (K)" }); }
        if self.mute.enabled != has { self.mute.enabled = has; if !has { self.mute.reset(now); } }
        self.mute.icon = if muted { "speaker-x" } else { "speaker-high" }; self.mute.tooltip = tip(if muted { "Unmute (M)" } else { "Mute (M)" });
        self.tc = if self.fmt.is_valid() { self.fmt.format(self.start + cur) } else { "—".into() };
    }
}

pub struct Preview {
    video: Option<Video>,
    presenter: Presenter, layer: UiLayer, fonts: Fonts, scale: f64, size: (u32, u32),
    path: String, state: Option<PathBuf>, dims: (u32, u32), facts: String, error: Option<String>,
    layers: Picker, colors: Picker, presets: Vec<Preset>,
    /// the kernel: being built, built, and whether the engine holds it
    pass_rx: Option<mpsc::Receiver<MinColorPass>>, pass: Option<Rc<RefCell<MinColorPass>>>, bound: bool,
    /// the decode in flight (its number; a later pick outdates it)
    job: u64, rx: mpsc::Receiver<(u64, Result<Frame, String>)>, tx: mpsc::Sender<(u64, Result<Frame, String>)>,
    inside: bool, moved: Instant, pointer: (f64, f64), bar: Ramp, bar_rect: Rect, dirty: bool,
    /// the mute carried in from the last file (kept as it was by a still)
    was_muted: bool, presented: bool,
    /// a walk build's readout (`qlp_debug`): the pointer events counted (down, up, leave) and the last one
    debug: bool, seen: [u32; 3], last: &'static str,
    /// the held press the view begins itself (`qlp_held`): the button seen down since, a press of ours running, a press
    /// the panel delivered running, and until when the panel's late pair is dropped
    held_since: Option<Instant>, phantom: bool, real_down: bool, swallow: Option<Instant>,
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
fn carried(state: Option<&PathBuf>, path: &str) -> (String, String, bool) {
    let Some(v) = state.and_then(|p| std::fs::read(p).ok()).and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok()) else { return Default::default() };
    if v["folder"].as_str() != Some(&folder(path)) { return Default::default() }
    (v["layer"].as_str().unwrap_or_default().to_string(), v["color"].as_str().unwrap_or_default().to_string(), v["muted"].as_bool().unwrap_or(false))
}

impl Preview {
    fn keep(&self) {
        let Some(p) = &self.state else { return };
        let color = if self.colors.selected == 0 { "" } else { self.colors.value() };
        let muted = self.video.as_ref().map(|v| Playback::is_muted(&v.t)).unwrap_or(self.was_muted);
        let _ = std::fs::write(p, serde_json::json!({ "folder": folder(&self.path), "layer": self.layers.value(), "color": color, "muted": muted }).to_string());
    }
    fn decode(&mut self) {
        if self.video.is_some() { return }
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
        if let Some(v) = self.video.as_mut() {
            let now = Instant::now();
            match v.t.tick(now) { Some(d) => { if let Err(e) = self.presenter.engine.show(&d.frame) { self.error = Some(e.to_string()); } self.dirty = true; } None => v.t.idle_refresh() }
            if v.t.is_playing() || Playback::is_scrubbing(&v.t) || v.t.is_fast_seeking() || v.tips.tick(now) || self.debug { self.dirty = true; }
        }
        if let Some(Ok(p)) = self.pass_rx.as_ref().map(|rx| rx.try_recv()) { self.pass = Some(Rc::new(RefCell::new(p))); self.pass_rx = None; self.color(); }
        self.pass_rx.is_some()
    }
    fn bar_wanted(&self, now: Instant) -> bool {
        self.layers.is_open() || self.colors.is_open() || self.video.as_ref().is_some_and(|v| v.scrub.is_pressed() || v.rew.is_pressed() || v.ff.is_pressed()) || (self.inside && (now.duration_since(self.moved) < REST || self.bar_rect.contains(kurbo::Point::new(self.pointer.0, self.pointer.1))))
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
            let mut x = bar.x0 + 10.0 * k; let cy = bar.center().y; let mut right = bar.x1 - 12.0 * k;
            let chip = |p: &mut Picker, fonts: &mut Fonts, x0: f64, max: f64| -> f64 { let (pw, ph) = p.measure(fonts, k); let pw = pw.clamp(96.0 * k, max); p.rect = Rect::new(x0, cy - ph / 2.0, x0 + pw, cy + ph / 2.0); pw };
            if let Some(v) = self.video.as_mut() {
                // rewind, play, fast-forward | the playbar | timecode, mute, colour, the facts (row 615)
                v.sync(now, k); let st = v.state();
                let (bw, bh) = v.play.measure(&mut self.fonts, k); let tight = t::SPACING_50 as f64 * k; let gap = 10.0 * k;
                for bt in [&mut v.rew, &mut v.play, &mut v.ff] { bt.rect = Rect::new(x, cy - bh / 2.0, x + bw, cy + bh / 2.0); bt.paint(&mut b, &mut self.fonts, k, now); x += bw + tight; }
                x += gap - tight;
                let facts = self.fonts.layout(&self.facts, Family::Sans, t::BODY_SIZE_M * k as f32, t::FONT_WEIGHT_REGULAR as f32, paint::color(t::PICKER_CONTENT_COLOR));
                if bar.width() > 900.0 * k { right -= facts.width() as f64; draw_layout(&mut b, &facts, Affine::translate((right, cy - facts.height() as f64 / 2.0))); right -= gap; }
                let cw = chip(&mut self.colors, &mut self.fonts, 0.0, 180.0 * k); right -= cw; let ph = self.colors.rect.height(); self.colors.rect = Rect::new(right, cy - ph / 2.0, right + cw, cy + ph / 2.0); self.colors.paint(&mut b, &mut self.fonts, k, now); right -= gap;
                right -= bw; v.mute.rect = Rect::new(right, cy - bh / 2.0, right + bw, cy + bh / 2.0); v.mute.paint(&mut b, &mut self.fonts, k, now); right -= gap;
                let tc = self.fonts.layout(&v.tc, Family::Mono, t::TIMECODE_WELL_FONT_SIZE * k as f32, t::FONT_WEIGHT_REGULAR as f32, paint::color(t::NEUTRAL_CONTENT_COLOR_DEFAULT));
                right -= tc.width() as f64; v.tc_rect = Rect::new(right, bar.y0, right + tc.width() as f64, bar.y1); draw_layout(&mut b, &tc, Affine::translate((right, cy - tc.height() as f64 / 2.0))); right -= gap;
                let sh = (t::SCRUBBER_HEIGHT as f64 * k).min(bar.height());
                v.scrub.layout(Rect::new(x, cy - sh / 2.0, right.max(x + 1.0), cy + sh / 2.0), k);
                if right - x > 40.0 * k { v.scrub.paint(&mut b, &mut self.fonts, k, &st, &[]); }
                self.layers.rect = Rect::ZERO;
                v.tips.paint(&mut b, &mut self.fonts, k, Rect::new(0.0, 0.0, w, h), now);
            } else {
                if self.layers.options.len() > 1 { let pw = chip(&mut self.layers, &mut self.fonts, x, 260.0 * k); self.layers.paint(&mut b, &mut self.fonts, k, now); x += pw + 8.0 * k; } else { self.layers.rect = Rect::ZERO; }
                let pw = chip(&mut self.colors, &mut self.fonts, x, 260.0 * k); self.colors.paint(&mut b, &mut self.fonts, k, now); x += pw + 8.0 * k;
                let l = self.fonts.layout_fit(&self.facts, Family::Sans, t::BODY_SIZE_M * k as f32, t::FONT_WEIGHT_REGULAR as f32, paint::color(t::PICKER_CONTENT_COLOR), (right - x).max(0.0));
                if right - x > 120.0 * k { draw_layout(&mut b, &l, Affine::translate((right - l.width() as f64, cy - l.height() as f64 / 2.0))); }
            }
            self.layers.paint_menu(&mut b, &mut self.fonts, k); self.colors.paint_menu(&mut b, &mut self.fonts, k);
            scene.push_layer(peniko::Fill::NonZero, peniko::Mix::Normal, a, Affine::IDENTITY, &Rect::new(0.0, 0.0, w, h)); scene.append(&b, None); scene.pop_layer();
        } else {
            self.bar_rect = Rect::ZERO; self.layers.rect = Rect::ZERO; self.colors.rect = Rect::ZERO;
            if let Some(v) = self.video.as_mut() {
                for bt in [&mut v.rew, &mut v.play, &mut v.ff, &mut v.mute] { bt.rect = Rect::ZERO; } v.scrub.layout(Rect::ZERO, k); v.big.rect = Rect::ZERO;
                // a small Finder preview: the picture, and on hover ONE button in its middle (row 615, point 1)
                if a > 0.01 && !room {
                    v.sync(now, k); let (bw, bh) = v.big.measure(&mut self.fonts, k); let c = kurbo::Point::new(w / 2.0, h / 2.0);
                    v.big.rect = Rect::new(c.x - bw / 2.0, c.y - bh / 2.0, c.x + bw / 2.0, c.y + bh / 2.0);
                    let mut b = vello::Scene::new(); paint::rounded(&mut b, v.big.rect.inflate(6.0 * k, 6.0 * k), 10.0 * k, t::BACKGROUND_LAYER_1_COLOR.with_alpha(0.9)); v.big.paint(&mut b, &mut self.fonts, k, now);
                    scene.push_layer(peniko::Fill::NonZero, peniko::Mix::Normal, a, Affine::IDENTITY, &Rect::new(0.0, 0.0, w, h)); scene.append(&b, None); scene.pop_layer();
                }
            }
        }
        if self.debug { if let Some(v) = self.video.as_ref() {
            let line = format!("fast seek: {}  audio shuttle: {}  playing: {}  frame {} / {}  has sound: {}  rew held: {}  ff held: {}  pointer down {} up {} leave {} last {}", v.t.is_fast_seeking(), v.t.audio.shuttle_active(), v.t.is_playing(), v.t.current_frame(), v.t.frame_count(), v.t.audio.has_audio(), v.rew.is_pressed(), v.ff.is_pressed(), self.seen[0], self.seen[1], self.seen[2], self.last);
            let l = self.fonts.layout_fit(&line, Family::Mono, 12.0 * k as f32, t::FONT_WEIGHT_REGULAR as f32, peniko::Color::WHITE, w - 24.0 * k);
            paint::rect(&mut scene, Rect::new(0.0, 0.0, w, l.height() as f64 + 12.0 * k), t::BACKGROUND_LAYER_1_COLOR.with_alpha(0.8)); draw_layout(&mut scene, &l, Affine::translate((12.0 * k, 6.0 * k)));
        } }
        let dst = self.layer.scene_mut(); dst.reset(); dst.append(&scene, None);
        self.layer.render(&self.presenter.engine.device, &self.presenter.engine.queue);
        // Not presented (the panel has not put the view on screen yet - the surface says "occluded" - or a drawable was not
        // to be had): the picture is still owed, and the next tick tries again. Clearing the flag here left the first
        // file black until the pointer moved (the owner's walk, 2026-11-26).
        let shown = self.presenter.draw(|| {}); self.dirty = !shown;
        if shown && !self.presented { self.presented = true;
            // the first picture on screen: the space-bar panel plays, a small Finder preview stays a silent still (row 615, points 1-2)
            if let Some(v) = self.video.as_mut() { if v.auto { v.auto = false; if room { v.t.play(); self.dirty = true; } note(&format!("first picture on screen: view {w:.0}x{h:.0}, plays by itself: {room}")); } } }
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
    let is_exr = std::path::Path::new(&path).extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case("exr"));
    let (was_layer, was_color, was_muted) = carried(state.as_ref(), &path);
    let mut error = None;
    let video = if is_exr { None } else { match Video::open(&path, presenter.engine.uploader(), was_muted) { Ok(v) => Some(v), Err(e) => { error = Some(e); None } } };
    let names = if is_exr { min_image::exr::layers(&path).unwrap_or_else(|_| vec!["default".into()]) } else { vec!["default".into()] };
    let presets: Vec<Preset> = builtins().into_iter().filter(|p| matches!(p.stored.chain, ClipChain::MinColor(_))).collect();
    let mut color_names = vec![trc("ColorPanel", "Color Off").to_string()]; color_names.extend(presets.iter().map(|p| p.name.clone()));
    let layer_at = names.iter().position(|n| *n == was_layer).unwrap_or(0);
    let color_at = presets.iter().position(|p| p.name == was_color).map(|i| i + 1).unwrap_or(0);
    let dot = |parts: Vec<String>| parts.into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join("  ·  ");
    let (dims, facts) = if let Some(v) = &video {
        let i = v.t.info(); let fps = i.frame_rate.0 as f64 / i.frame_rate.1.max(1) as f64;
        ((i.width.max(1), i.height.max(1)), dot(vec![format!("{} × {}", i.width, i.height), i.codec_short.clone(), format!("{} fps", (fps * 1000.0).round() / 1000.0)]))
    } else {
        let dims = min_image::dimensions(&path, if names[layer_at] == "default" { "" } else { &names[layer_at] }).unwrap_or((1920, 1080));
        let kind = match min_image::exr::sample_kind(&path) { Some(0) => trc(CTX, "32-bit integer"), Some(1) => trc(CTX, "16-bit float"), Some(2) => trc(CTX, "32-bit float"), _ => "" };
        (dims, dot(vec![format!("{} × {}", dims.0, dims.1), kind.to_string(), min_image::exr::compression(&path).unwrap_or_default().to_string()]))
    };
    let (tx, rx) = mpsc::channel(); let now = Instant::now();
    let mut p = Preview { held_since: None, phantom: false, real_down: false, swallow: None, debug: false, seen: [0; 3], last: "-", video, was_muted, presented: false, presenter, layer, fonts: Fonts::new(), scale, size: (w, h), path, state, dims, facts, error,
        layers: Picker::new(names, layer_at), colors: Picker::new(color_names, color_at), presets, pass_rx: None, pass: None, bound: false,
        job: 0, rx, tx, inside: false, moved: now - REST, pointer: (-1.0, -1.0), bar: Ramp::new(0.0, 160), bar_rect: Rect::ZERO, dirty: true };
    p.decode(); p.color();
    note(&format!("open: {} (video {}, sound {}), view {w}x{h} scale {scale}", p.facts, p.video.is_some(), p.video.as_ref().is_some_and(|v| v.t.audio.has_audio())));
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
    // a press the view began by itself (`qlp_held`) is ended there too: the panel's own late press and release are dropped
    if matches!(kind, 1 | 2) && (s.phantom || s.swallow.is_some_and(|t| now < t)) { return; }
    match kind { 1 => s.real_down = true, 2 | 3 => s.real_down = false, _ => {} }
    pointer_event(s, x, y, kind, now);
}

/// The primary button's state, read by the shell at every tick. Quick Look's panel keeps a press that does not move
/// until its release and then hands over both at once (seen in the log 2026-11-26: down and up in one millisecond), so
/// a held rewind or fast-forward never began. The view therefore begins the press itself once the button has been down
/// for `HOLD` over one of the two with no press delivered, and ends it when the button comes up.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn qlp_held(p: *mut Preview, held: bool) { let s = unsafe { &mut *p }; let now = Instant::now();
    const HOLD: Duration = Duration::from_millis(150);
    let (x, y) = s.pointer;
    if !held {
        s.held_since = None;
        if s.phantom { s.phantom = false; s.swallow = Some(now + Duration::from_millis(400)); pointer_event(s, x, y, 2, now); }
        return;
    }
    if s.phantom || s.real_down || !s.inside || s.layers.is_open() || s.colors.is_open() { return; }
    let on = s.video.as_ref().is_some_and(|v| { let at = kurbo::Point::new(x, y); v.rew.rect.contains(at) || v.ff.rect.contains(at) });
    if !on { s.held_since = None; return; }
    if now.duration_since(*s.held_since.get_or_insert(now)) >= HOLD { s.phantom = true; note("a held press the panel kept: begun here"); pointer_event(s, x, y, 1, now); }
}

fn pointer_event(s: &mut Preview, x: f64, y: f64, kind: i32, now: Instant) {
    let e = match kind { 0 => Event::Move { x, y }, 1 => Event::Down { x, y, button: Button::Primary }, 2 => Event::Up { x, y, button: Button::Primary }, _ => Event::Leave };
    s.inside = kind != 3; if kind != 3 { s.pointer = (x, y); s.moved = now; }
    match kind { 1 => { s.seen[0] += 1; s.last = "down"; } 2 => { s.seen[1] += 1; s.last = "up"; } 3 => { s.seen[2] += 1; s.last = "leave"; } _ => {} }
    if kind != 0 { note(&format!("pointer {} at {x:.0},{y:.0} (view {}x{})", s.last, s.size.0, s.size.1)); }
    let bounds = Rect::new(0.0, 0.0, s.size.0 as f64, s.size.1 as f64);
    let menu_open = s.layers.is_open() || s.colors.is_open(); let mut keep = false;
    if let (Some(v), false) = (s.video.as_mut(), menu_open) {
        let st = v.state(); let mut on_control = false;
        for (bt, dir) in [(&mut v.rew, -1), (&mut v.ff, 1)] { match bt.event(&e, now) {
            Response::PressBegan => { Playback::apply(&mut v.t, Action::ShuttleBegin(dir), now); note(&format!("shuttle {dir} began: fast seek {}, audio shuttle {}, frame {}", v.t.is_fast_seeking(), v.t.audio.shuttle_active(), v.t.current_frame())); }
            Response::PressEnded => { let f0 = v.t.current_frame(); Playback::apply(&mut v.t, Action::ShuttleEnd, now); note(&format!("shuttle {dir} ended by event kind {kind}: frame {f0} -> {}", v.t.current_frame())); }
            _ => {} } on_control |= bt.is_pressed(); }
        for bt in [&mut v.play, &mut v.big] { if bt.event(&e, now) == Response::Clicked { v.t.toggle_playback(); } }
        if v.mute.event(&e, now) == Response::Clicked { Playback::apply(&mut v.t, Action::ToggleMute, now); keep = true; }
        let (cmds, _) = v.scrub.event(&e, now, &st, s.scale);
        for c in cmds { match c { ScrubCmd::Begin => Playback::begin_scrub(&mut v.t), ScrubCmd::To(f) => Playback::scrub_to_frame(&mut v.t, f), ScrubCmd::End => Playback::end_scrub(&mut v.t), _ => {} } }
        let over = |r: Rect| r.contains(kurbo::Point::new(x, y));
        on_control |= kind != 3 && (over(v.rew.rect) || over(v.play.rect) || over(v.ff.rect) || over(v.mute.rect) || over(v.big.rect) || over(v.scrub.rect) || over(s.colors.rect) || over(s.bar_rect));
        // a press and release on the picture itself: play / pause, as the spike the owner walked did
        match kind { 1 => v.press = !on_control, 2 => { if v.press && !on_control { v.t.toggle_playback(); } v.press = false; } 3 => v.press = false, _ => {} }
        if kind == 1 { v.tips.pressed(now); }
        let tip_at: Option<(u64, Rect, String)> = [(1u64, &v.rew), (2, &v.play), (3, &v.ff), (4, &v.mute), (5, &v.big)].into_iter().find(|(_, b)| b.hovered() && b.rect.width() > 0.0).map(|(i, b)| (i, b.rect, b.tooltip.clone()));
        v.tips.hover(tip_at.as_ref().map(|(i, r, t)| (*i, *r, t.as_str())), now);
    }
    if keep { s.keep(); }
    // an open menu takes every press (min-ui's rule); the other picker does not see it
    let first_colors = s.colors.is_open();
    let (layer, color) = if first_colors { let c = s.colors.event(&e, now, &mut s.fonts, bounds, s.scale); (if c.2 { (None, false, false) } else { s.layers.event(&e, now, &mut s.fonts, bounds, s.scale) }, c) }
        else { let l = s.layers.event(&e, now, &mut s.fonts, bounds, s.scale); (l, if l.2 { (None, false, false) } else { s.colors.event(&e, now, &mut s.fonts, bounds, s.scale) }) };
    if layer.0.is_some() { s.decode(); s.keep(); }
    if color.0.is_some() { s.color(); s.keep(); }
    s.dirty = true;
}

/// A key that reached the view (it does after a click on the picture; Space, the arrows and Escape stay the panel's).
/// `ch`: the key's character without modifiers; `special`: 1 = Home, 2 = End, 0 = none. QCView's own map (row 615,
/// point 4): K / W / S play, J / A and L / D shuttle while held, Q / E a frame, M mute, Home / End.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn qlp_key(p: *mut Preview, ch: u32, special: u32, pressed: bool, repeat: bool) { let s = unsafe { &mut *p }; let now = Instant::now();
    let Some(v) = s.video.as_mut() else { return };
    let key = match special { 1 => qcv_player::Key::Home, 2 => qcv_player::Key::End, _ => match char::from_u32(ch) { Some(c) if c.is_ascii_alphabetic() => qcv_player::Key::Char(c), _ => return } };
    let mapped = qcv_player::map_key(key, qcv_player::Mods::default(), pressed, repeat);
    note(&format!("key {key:?} pressed {pressed} repeat {repeat} -> {mapped:?}"));
    let Some(a) = mapped else { return };
    if !matches!(a, Action::ShuttleBegin(_) | Action::ShuttleEnd | Action::Step(_) | Action::TogglePlayback | Action::SeekToStart | Action::SeekToEnd | Action::ToggleMute) { return }
    Playback::apply(&mut v.t, a, now); if a == Action::ToggleMute { s.keep(); } s.dirty = true;
}

/// A walk build's readout along the top (the bundle's Info.plist key `QCVQuickLookReadout`): what the player and the
/// pointer are doing. A Developer ID container's log cannot be read from outside, so the screen is the log.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn qlp_debug(p: *mut Preview, on: bool) { let s = unsafe { &mut *p }; s.debug = on; s.dirty = true; }

/// The wheel, pixels (positive = further down a list): an open menu scrolls.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn qlp_scroll(p: *mut Preview, dy: f64) { let s = unsafe { &mut *p }; if s.layers.wheel(dy) || s.colors.wheel(dy) { s.dirty = true; } }

/// Called at the display's pace by the shell: takes what the workers finished and draws when something changed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn qlp_tick(p: *mut Preview) { let s = unsafe { &mut *p }; let now = Instant::now();
    s.poll();
    let want = s.bar_wanted(now);
    if s.dirty || s.bar.animating(now) || (want as u8 as f32 - s.bar.target()).abs() > 0.5 || s.layers.animating(now) || s.colors.animating(now) || s.video.as_ref().is_some_and(|v| [&v.rew, &v.play, &v.ff, &v.mute, &v.big].iter().any(|b| b.animating(now))) { s.draw(now); }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn qlp_close(p: *mut Preview) { if p.is_null() { return } let mut s = unsafe { Box::from_raw(p) };
    // the player's decoders end on their own threads; they are waited for before the device goes
    if let Some(v) = s.video.take() { v.t.retire(); qcv_player::wait_retired(Duration::from_secs(2)); } }
