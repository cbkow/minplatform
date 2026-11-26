//! S22: one video file played by the family's `Transport` inside a view Quick Look hosts, with sound. It starts
//! playing at once; a click on the picture toggles play; the bar is always shown and is the spike's READOUT (a
//! Developer ID container's log cannot be read from outside): how the open went (hardware decode, the audio unit),
//! frames shown / dropped / late, the peak memory, and the last key that reached the view.
use kurbo::{Affine, Rect};
use min_present::{Output, Presenter};
use min_theme::tokens as t;
use min_ui::{fonts::draw_layout, paint, Family, Fonts, UiLayer};
use qcv_player::{source::Source, AudioPlayer, HwDevice, Transport, TransportOptions};
use std::{ffi::{c_char, c_void, CStr}, time::Instant};

pub struct S22 { presenter: Presenter, layer: UiLayer, fonts: Fonts, scale: f64, size: (u32, u32), player: Option<Transport>, opened: String, key: String, started: Instant, first: Option<u128>, show_err: u32 }

fn peak_mb() -> u64 { let mut u: libc::rusage = unsafe { std::mem::zeroed() }; unsafe { libc::getrusage(libc::RUSAGE_SELF, &mut u); } u.ru_maxrss as u64 / (1024 * 1024) }

/// `hw`: 1 = ask for VideoToolbox, 0 = software. `scrub_mb`: the scrub cache's budget, 0 = no scrub decoder.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn s22_open(ca_layer: *mut c_void, ns_view: *mut c_void, w: u32, h: u32, scale: f64, path: *const c_char, hw: i32) -> *mut S22 {
    let t0 = Instant::now(); let path = unsafe { CStr::from_ptr(path) }.to_string_lossy().into_owned();
    let instance = Presenter::instance();
    let Ok(surface) = (unsafe { instance.create_surface_unsafe(wgpu::SurfaceTargetUnsafe::CoreAnimationLayer(ca_layer)) }) else { return std::ptr::null_mut() };
    let (w, h) = (w.max(1), h.max(1));
    let Ok(mut presenter) = Presenter::new(&instance, surface, (w, h), Output::Auto, Some(ns_view)) else { return std::ptr::null_mut() };
    presenter.engine.set_ui_scale(scale as f32);
    let Ok(layer) = UiLayer::new(&presenter.engine.device, w, h) else { return std::ptr::null_mut() };
    presenter.engine.set_overlay(Some(layer.view()));
    let t_gpu = t0.elapsed().as_millis(); let t1 = Instant::now();
    let dev = if hw != 0 { HwDevice::platform(None) } else { None }; let asked = if hw == 0 { "software asked" } else if dev.is_some() { "VideoToolbox context made" } else { "NO VideoToolbox context" };
    let opts = TransportOptions { hw: dev, scrub: None, uploader: Some(presenter.engine.uploader()), ..Default::default() };
    let (player, opened) = match Transport::open(&Source::from_path(&path, None), opts) {
        Ok(mut p) => {
            let t_open = t1.elapsed().as_millis(); let t2 = Instant::now();
            let audio = match AudioPlayer::new() { Ok(a) => { p.set_audio(Box::new(a)); format!("audio unit ok in {} ms, file has sound: {}", t2.elapsed().as_millis(), p.audio.has_audio()) } Err(e) => format!("NO audio unit: {e}") };
            let i = p.info(); let line = format!("{}x{} {} {:.3} fps, {} frames; decode: {} ({asked}); {audio}; presenter {t_gpu} ms, open {t_open} ms", i.width, i.height, i.codec_short, i.frame_rate.0 as f64 / i.frame_rate.1.max(1) as f64, i.frame_count, if i.hw_accel.is_empty() { "software" } else { &i.hw_accel });
            p.play(); (Some(p), line)
        }
        Err(e) => (None, format!("the player could not open the file: {e}")),
    };
    Box::into_raw(Box::new(S22 { presenter, layer, fonts: Fonts::new(), scale, size: (w, h), player, opened, key: "no key yet".into(), started: Instant::now(), first: None, show_err: 0 }))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn s22_resize(p: *mut S22, w: u32, h: u32, scale: f64) { let s = unsafe { &mut *p }; let (w, h) = (w.max(1), h.max(1));
    if s.size == (w, h) && s.scale == scale { return } s.size = (w, h); s.scale = scale;
    s.presenter.resize(w, h); s.presenter.engine.set_ui_scale(scale as f32); s.layer.resize(&s.presenter.engine.device, w, h); s.presenter.engine.set_overlay(Some(s.layer.view())); }

/// A click on the picture: play / pause.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn s22_click(p: *mut S22) { let s = unsafe { &mut *p }; if let Some(pl) = s.player.as_mut() { pl.toggle_playback(); } }

/// A key that reached the view: J (38) back a second, K (40) play / pause, L (37) forward a second, comma / period a frame.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn s22_key(p: *mut S22, code: u32) { let s = unsafe { &mut *p }; s.key = format!("key code {code} reached the view");
    let Some(pl) = s.player.as_mut() else { return }; let fps = { let i = pl.info(); (i.frame_rate.0 as f64 / i.frame_rate.1.max(1) as f64).round() as i32 };
    match code { 40 => pl.toggle_playback(), 38 => pl.step_frames(-fps), 37 => pl.step_frames(fps), 43 => pl.step_frames(-1), 47 => pl.step_frames(1), _ => {} } }

#[unsafe(no_mangle)]
pub unsafe extern "C" fn s22_tick(p: *mut S22) { let s = unsafe { &mut *p }; let now = Instant::now(); let k = s.scale; let (w, h) = (s.size.0 as f64, s.size.1 as f64);
    let mut state = String::new();
    if let Some(pl) = s.player.as_mut() {
        match pl.tick(now) { Some(d) => { if s.presenter.engine.show(&d.frame).is_err() { s.show_err += 1; } else if s.first.is_none() { s.first = Some(s.started.elapsed().as_millis()); } } None => pl.idle_refresh() }
        let st = &pl.stats;
        state = format!("{} frame {} / {}   shown {}  dropped {}  late {}  loops {}  show errors {}   first picture {} ms   {:.0} s   peak memory {} MB", if pl.is_playing() { "PLAYING" } else { "paused" }, pl.current_frame(), pl.frame_count(), st.shown, st.dropped, st.late, st.loops, s.show_err, s.first.map(|m| m.to_string()).unwrap_or_else(|| "-".into()), s.started.elapsed().as_secs_f32(), peak_mb());
    }
    let mut scene = vello::Scene::new();
    let m = 12.0 * k; let bar = Rect::new(m, h - m - 86.0 * k, w - m, h - m);
    paint::rounded(&mut scene, bar, 10.0 * k, t::BACKGROUND_LAYER_1_COLOR.with_alpha(0.86));
    for (i, line) in [&s.opened, &state, &s.key].into_iter().enumerate() {
        let l = s.fonts.layout_fit(line, Family::Sans, t::BODY_SIZE_M * k as f32, t::FONT_WEIGHT_REGULAR as f32, paint::color(t::PICKER_CONTENT_COLOR), bar.width() - 24.0 * k);
        draw_layout(&mut scene, &l, Affine::translate((bar.x0 + 12.0 * k, bar.y0 + (8.0 + 24.0 * i as f64) * k)));
    }
    let dst = s.layer.scene_mut(); dst.reset(); dst.append(&scene, None);
    s.layer.render(&s.presenter.engine.device, &s.presenter.engine.queue);
    s.presenter.draw(|| {});
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn s22_close(p: *mut S22) { if p.is_null() { return } let mut s = unsafe { Box::from_raw(p) }; if let Some(pl) = s.player.take() { pl.retire(); } qcv_player::wait_retired(std::time::Duration::from_secs(2)); }
