//! S21: an EXR shown by the family's presenter inside a view somebody else owns (Quick Look's preview), with one
//! min-ui picker (the file's layers) in a bar that shows while the pointer is over the view. Spike code: everything on
//! the caller's thread, no colour chain, timings and input written to `s21.log` in the temporary folder.
use kurbo::{Affine, Rect};
use min_present::{Chroma, ColorMeta, Frame, Matrix, Output, Pixels, PlanarLayout, Plane, Presenter, Primaries, Range, Transfer};
use min_theme::tokens as t;
use min_ui::{fonts::draw_layout, paint, picker::Picker, Button, Event, Family, Fonts, UiLayer};
use std::{ffi::{c_char, c_void, CStr}, io::Write, time::Instant};

pub struct S21 { presenter: Presenter, layer: UiLayer, fonts: Fonts, picker: Picker, path: String, scale: f64, size: (u32, u32), inside: bool, opened: Instant, drawn: bool, line: String }

fn note(s: &str) { if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(std::env::var("S21_LOG").unwrap_or_else(|_| "/tmp/s21.log".into())) { let _ = writeln!(f, "{s}"); } }
fn ms(t: Instant) -> u128 { t.elapsed().as_millis() }

fn frame(path: &str, layer: &str) -> Result<Frame, String> {
    let layer = if layer == "default" { "" } else { layer };
    let img = min_image::load(path, layer, 8).map_err(|e| e.to_string())?;
    let n = if img.alpha { 4 } else { 3 }; if img.planes.len() < n { return Err("fewer planes than a picture needs".into()) }
    let (bps, code_bits, float) = match img.sample { min_image::Sample::U8 => (1, 8, false), min_image::Sample::U16 => (2, 16, false), min_image::Sample::F16 => (2, 16, true) };
    let meta = ColorMeta { matrix: Matrix::Identity, range: Range::Full, transfer: if float { Transfer::Linear } else { Transfer::Srgb }, primaries: Primaries::Bt709, bit_depth: code_bits as _, chroma: Chroma::Rgb, range_forced: false, rotation: 0, sar: (1, 1), mastering: None, raw: None };
    let (w, h, pitch, alpha) = (img.width, img.height, img.pitch, img.alpha); let mut planes = img.planes; planes.truncate(n);
    Ok(Frame { width: w, height: h, pixels: Pixels::Planar { planes: planes.into_iter().map(|p| Plane::owned(p, w, h, pitch)).collect(), layout: PlanarLayout::Rgb { bytes_per_sample: bps, code_bits, float, alpha, legal_expand: false } }, meta })
}

/// `ca_layer`: the view's CAMetalLayer. Sizes in pixels. Null when nothing could be opened.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn s21_open(ca_layer: *mut c_void, ns_view: *mut c_void, w: u32, h: u32, scale: f64, path: *const c_char) -> *mut S21 {
    let t0 = Instant::now(); let path = unsafe { CStr::from_ptr(path) }.to_string_lossy().into_owned();
    let instance = Presenter::instance();
    let surface = match unsafe { instance.create_surface_unsafe(wgpu::SurfaceTargetUnsafe::CoreAnimationLayer(ca_layer)) } { Ok(s) => s, Err(e) => { note(&format!("no surface: {e}")); return std::ptr::null_mut() } };
    let (w, h) = (w.max(1), h.max(1));
    let mut presenter = match Presenter::new(&instance, surface, (w, h), Output::Auto, Some(ns_view)) { Ok(p) => p, Err(e) => { note(&format!("no presenter: {e}")); return std::ptr::null_mut() } };
    presenter.engine.set_ui_scale(scale as f32);
    let layer = match UiLayer::new(&presenter.engine.device, w, h) { Ok(l) => l, Err(e) => { note(&format!("no ui layer: {e}")); return std::ptr::null_mut() } };
    presenter.engine.set_overlay(Some(layer.view()));
    let t_gpu = ms(t0); let t1 = Instant::now();
    let layers = min_image::exr::layers(&path).unwrap_or_else(|_| vec!["default".into()]);
    let t_layers = ms(t1); let t2 = Instant::now();
    let first = layers.first().cloned().unwrap_or_default();
    let shown = match frame(&path, &first) { Ok(f) => { let d = (f.width, f.height); presenter.engine.show(&f).map(|_| d).map_err(|e| e.to_string()) } Err(e) => Err(e) };
    let info = presenter.info(); let line = format!("{} layers; presenter {t_gpu} ms ({} {:?}, {}), layer list {t_layers} ms, decode + upload {} ms: {:?}", layers.len(), info.backend, info.format, info.adapter, ms(t2), shown);
    note(&format!("open {line}"));
    Box::into_raw(Box::new(S21 { presenter, layer, fonts: Fonts::new(), picker: Picker::new(layers, 0), path, scale, size: (w, h), inside: false, opened: t0, drawn: false, line }))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn s21_resize(p: *mut S21, w: u32, h: u32, scale: f64) { let s = unsafe { &mut *p }; let (w, h) = (w.max(1), h.max(1));
    if s.size == (w, h) && s.scale == scale { return } s.size = (w, h); s.scale = scale;
    s.presenter.resize(w, h); s.presenter.engine.set_ui_scale(scale as f32); s.layer.resize(&s.presenter.engine.device, w, h); s.presenter.engine.set_overlay(Some(s.layer.view())); note(&format!("resize {w}x{h} scale {scale}")); }

/// kind: 0 move, 1 primary down, 2 primary up, 3 the pointer left. Pixels, the top left the origin.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn s21_pointer(p: *mut S21, x: f64, y: f64, kind: i32) { let s = unsafe { &mut *p };
    let e = match kind { 0 => Event::Move { x, y }, 1 => Event::Down { x, y, button: Button::Primary }, 2 => Event::Up { x, y, button: Button::Primary }, _ => Event::Leave };
    s.inside = kind != 3; if kind == 1 || kind == 2 { note(&format!("pointer {} at {x:.0},{y:.0}", if kind == 1 { "down" } else { "up" })); }
    let bounds = Rect::new(0.0, 0.0, s.size.0 as f64, s.size.1 as f64);
    let (chosen, _, _) = s.picker.event(&e, Instant::now(), &mut s.fonts, bounds, s.scale);
    if let Some(i) = chosen { let t0 = Instant::now(); let name = s.picker.options[i].clone();
        let r = frame(&s.path, &name).and_then(|f| s.presenter.engine.show(&f).map_err(|e| e.to_string()));
        s.line = format!("layer \"{name}\" in {} ms: {r:?}", ms(t0)); note(&format!("chose {}", s.line)); }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn s21_key(p: *mut S21, code: u32) { let s = unsafe { &mut *p }; s.line = format!("key code {code} reached the view"); note(&s.line); }

#[unsafe(no_mangle)]
pub unsafe extern "C" fn s21_draw(p: *mut S21) { let s = unsafe { &mut *p }; let now = Instant::now(); let k = s.scale; let (w, h) = (s.size.0 as f64, s.size.1 as f64);
    let mut scene = vello::Scene::new();
    if s.inside || s.picker.is_open() {
        // the bar: over the bottom of the picture, QuickTime's place for it
        let bar = Rect::new(12.0 * k, h - 56.0 * k, w - 12.0 * k, h - 12.0 * k);
        paint::rounded(&mut scene, bar, 10.0 * k, t::BACKGROUND_LAYER_1_COLOR.with_alpha(0.86));
        let (pw, ph) = s.picker.measure(&mut s.fonts, k); let pw = pw.max(180.0 * k);
        s.picker.rect = Rect::new(bar.x0 + 10.0 * k, bar.center().y - ph / 2.0, bar.x0 + 10.0 * k + pw, bar.center().y + ph / 2.0);
        s.picker.paint(&mut scene, &mut s.fonts, k, now);
        let l = s.fonts.layout_fit(&s.line, Family::Sans, t::BODY_SIZE_M * k as f32, t::FONT_WEIGHT_REGULAR as f32, paint::color(t::PICKER_CONTENT_COLOR), bar.x1 - s.picker.rect.x1 - 30.0 * k);
        draw_layout(&mut scene, &l, Affine::translate((s.picker.rect.x1 + 14.0 * k, bar.center().y - l.height() as f64 / 2.0)));
        s.picker.paint_menu(&mut scene, &mut s.fonts, k);
    }
    let dst = s.layer.scene_mut(); dst.reset(); dst.append(&scene, None);
    s.layer.render(&s.presenter.engine.device, &s.presenter.engine.queue);
    let ok = s.presenter.draw(|| {});
    if ok && !s.drawn { s.drawn = true; let m = ms(s.opened); s.line = format!("first picture {m} ms; {}", s.line); note(&format!("first picture {m} ms after the open began")); }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn s21_close(p: *mut S21) { if !p.is_null() { note("close"); drop(unsafe { Box::from_raw(p) }); } }
