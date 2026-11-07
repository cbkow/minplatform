//! `qcv` — Phase 2 step 1: open a file (or an image-sequence frame) → the fused presenter at the display's cadence.
//! `qcv <file> [--sw] [--out auto|sdr|edr|pq] [--white N] [--seconds N] [--stats] [--snap-out path@N] [--verify-pq]
//!      [--identity-chain] [--refresh ms] [--screen i]`. Space = play / pause · F = fullscreen · Esc = quit · drop a file to open it.
use winit31 as winit;
use min_present::{Output, Presenter, Snapshot};
use min_theme::tokens as t;
use qcv_player::{HwDevice, Transport, TransportOptions, Source, Action};
use std::{collections::HashMap, sync::Arc, time::Instant};
use winit::application::ApplicationHandler;
use winit::data_transfer::TypeHint;
use winit::event::{ElementState, WindowEvent};
use winit::event_loop::{ActiveEventLoop, AsyncRequestSerial, ControlFlow, DndAction, EventLoop};
use winit::keyboard::{Key, NamedKey};
use winit::monitor::Fullscreen;
use winit::window::{Window, WindowAttributes, WindowId};
use winit::dpi::{LogicalSize, PhysicalPosition};
#[cfg(target_os = "macos")] mod mac;

fn pct(v: &mut Vec<f32>, p: f32) -> f32 { if v.is_empty() { return f32::NAN } v.sort_by(|a, b| a.partial_cmp(b).unwrap()); v[((v.len() - 1) as f32 * p / 100.0).round() as usize] }

struct Opts { cut: Option<String>, hover: Option<u32>, seek_gap_ms: u64, seeks: Option<u32>, scrub: Option<(i32, i32, f32)>, scrub_hw: bool, headless: Option<u32>, looping: bool, file: Option<String>, sw: bool, out: Output, white: f32, seconds: f32, stats: bool, snap_out: Option<(String, u32)>, verify_pq: bool, identity_chain: bool, refresh_ms: f32, screen: Option<usize>, threads: i32, autoplay: bool }

mod ui_sketch;
mod transport_bar;
mod status_strip;
mod hover_thumbs;
mod settings;
mod settings_panel;
mod inspector_panel;

struct Gpu { window: Arc<Box<dyn Window>>, presenter: Presenter, ui: Option<min_ui::UiLayer>, fonts: min_ui::Fonts, ui_ms: Vec<f32>, ui_shown_frame: Option<(i32, i32)>, bar: transport_bar::TransportBar, strip: status_strip::StatusStrip, scrubber: min_ui::Scrubber, thumbs: hover_thumbs::HoverThumbs, pointer: (f64, f64), ui_dirty: bool,
    /// Fullscreen: the picture alone; the bands slide in (1) from the bottom edge while the pointer rests there (canon §25 row 171).
    fullscreen: bool, bands: min_ui::anim::Ramp, bands_leave: Option<Instant>,
    waveform: qcv_player::WaveformProbeEngine, wave_seen: u64, wave_req: (String, f64, f64, usize), wave_peaks: Vec<(f32, f32)>,
    /// The rails (canon §16.3 + the 2026-11-07 rails ruling): left = Settings (QCView's section), right = the Inspector; both hidden in fullscreen.
    left: min_ui::Rail, right: min_ui::Rail, settings_panel: settings_panel::SettingsPanel, inspector: inspector_panel::InspectorPanel, content_set: Option<(u32, u32, u32, u32)>, waveform_enabled: bool, cursor: winit::cursor::CursorIcon }

struct App {
    o: Opts, gpu: Option<Gpu>, player: Option<Transport>, hw: Option<HwDevice>, source_label: String, source_path: String,
    t0: Option<Instant>, t_open: Option<Instant>, first_presented: bool,
    presents: Vec<f32>, last_present: Option<Instant>, show_ms: Vec<f32>, last_report: Instant, shown_in_window: u32,
    fetches: HashMap<AsyncRequestSerial, WindowId>, mods: qcv_player::Mods, finished: bool, frames_presented: u64, edr_seen: f64, snapped: bool, verified: bool,
    settings: min_settings::Settings,
}

impl App {
    fn open(&mut self, path: &str) {
        let Some(g) = self.gpu.as_mut() else { return };
        let src = Source::from_path(path, None);
        let t = Instant::now();
        // QCView's performance settings apply at open: threads, hardware decode (its row is Windows-only), the scrub cache budget and the inter-codec hw-scrub gate
        let st = &self.settings;
        let hw_ok = !cfg!(windows) || st.bool(settings::k::HW_DECODE);
        let hw = if self.o.sw || !hw_ok { None } else { self.hw.clone() };
        let threads = if self.o.threads != 0 { self.o.threads } else { st.int(settings::k::FFMPEG_THREADS) as i32 };
        let scrub = Some(qcv_player::ScrubOptions { budget_bytes: (st.int(settings::k::SCRUB_CACHE_MB).max(1) as usize) << 20, hw_inter_scrub: st.bool(settings::k::HW_SCRUB_INTER), ..Default::default() });
        let loop_enabled = st.bool(settings::k::LOOP_ENABLED) || self.o.looping;   // the store's loop (QCView default ON); `--loop` forces it for the harness
        match Transport::open(&src, TransportOptions { hw, threads, loop_enabled, refresh_ms: self.o.refresh_ms, scrub, ..Default::default() }) {
            Ok(mut p) => {
                // audio first, so the harness's play below starts it too (the player's hooks run inside Transport::play)
                if std::env::var_os("QCV_NO_AUDIO").is_none() { match qcv_player::AudioPlayer::new() { Ok(a) => { p.set_audio(Box::new(a)); println!("audio: {} (device latency {:.1} ms)", if p.audio.has_audio() { "open (CoreAudio default output)" } else { "no audio stream" }, p.audio.device_latency_ms()); } Err(e) => println!("audio: {e}") } }
                if p.audio.has_audio() { p.audio.set_volume(st.float(settings::k::VOLUME) as f32); p.audio.set_muted(st.bool(settings::k::MUTED)); p.audio.set_sync_offset_ms(st.int(settings::k::SYNC_OFFSET_MS) as i32); p.audio.set_scrub_muted(st.bool(settings::k::SCRUB_MUTED)); }
                p.play();   // the harness plays on open (QCView opens paused; the shell decides)
                println!("opened {} — {}  (open took {:.0} ms)", src.display(), describe(&p), t.elapsed().as_secs_f32() * 1000.0);
                self.source_label = src.display(); self.source_path = path.to_string(); g.thumbs.on_open(&p);
                self.player = Some(p); self.t_open = Some(t); self.first_presented = false;
                g.window.set_title(&format!("qcv — {}", self.source_label));
            }
            Err(e) => println!("open {path}: {e}"),
        }
    }
    fn tick(&mut self) {
        let Some(g) = self.gpu.as_mut() else { return };
        let now = Instant::now();
        let mut shown_new = false; let mut reopen = false;
        if let Some(p) = self.player.as_mut() {
            if let Some(d) = p.tick(now) {
                match g.presenter.engine.show(&d.frame) {
                    Ok(()) => { shown_new = true; self.show_ms.push(g.presenter.engine.last_show_ms); self.shown_in_window += 1; }
                    Err(e) => { println!("show: {e} — reopening in software"); reopen = true; }
                }
            }
        }
        if reopen { self.o.sw = true; self.player = None; let path = self.source_path.clone(); self.open(&path); return }
        if !shown_new { if let Some(p) = self.player.as_mut() { p.idle_refresh(); } }
        if let Some(u) = g.ui.as_mut() {
            let frame = self.player.as_ref().and_then(|p| p.current().map(|_| (p.current_frame(), p.frame_count(), p.rate().as_f64())));
            let key = frame.map(|(c, n, _)| (c, n));
            g.bar.sync(self.player.as_ref(), now);
            if g.bar.tick(now) || g.strip.animating(now) { g.ui_dirty = true; }
            let (w, h) = u.size(); let scale = g.window.scale_factor();
            let st = scrub_state(self.player.as_ref());
            let bands_h = (t::STATUS_STRIP_HEIGHT + t::TRANSPORT_BAR_HEIGHT + t::SCRUBBER_HEIGHT) as f64 * scale;
            if g.fullscreen {
                // the pointer at the bottom edge (or on the shown bands) reveals; leaving starts the hide delay
                let (px, py) = g.pointer;
                let shown_rect_top = h as f64 - bands_h * g.bands.value(now) as f64;
                let near = px >= 0.0 && (py >= h as f64 - t::BANDS_REVEAL_ZONE as f64 * scale || (g.bands.target() > 0.5 && py >= shown_rect_top));
                if near { g.bands_leave = None; g.bands.set_target(1.0, now); }
                else if g.bands.target() > 0.5 { match g.bands_leave { None => g.bands_leave = Some(now), Some(t0) => if now.duration_since(t0).as_millis() as u32 >= t::BANDS_HIDE_DELAY { g.bands.set_target(0.0, now); g.bands_leave = None; } } }
            }
            if g.bands.animating(now) { g.ui_dirty = true; }
            let slide = bands_h * (1.0 - g.bands.value(now) as f64);
            let scrub_y = h as f64 - t::SCRUBBER_HEIGHT as f64 * scale + slide;
            // the rails span the viewport row (QCView's layout: the bands run full width beneath); none in fullscreen
            let rails_bottom = scrub_y - (t::TRANSPORT_BAR_HEIGHT + t::STATUS_STRIP_HEIGHT) as f64 * scale;
            let (lw, rw) = if g.fullscreen { (0.0, 0.0) } else {
                g.left.layout(0.0, 0.0, rails_bottom, scale, now); g.right.layout(w as f64, 0.0, rails_bottom, scale, now);
                (g.left.visible_width(now, scale), g.right.visible_width(now, scale))
            };
            if g.left.animating(now) || g.right.animating(now) { g.ui_dirty = true; }
            let content = if g.fullscreen { (0, 0, w, h) } else { let x0 = lw.round() as u32; let x1 = (w as f64 - rw).round() as u32; (x0, 0, x1.saturating_sub(x0).max(1), (rails_bottom.round() as u32).max(1)) };
            if g.content_set != Some(content) { g.content_set = Some(content); g.presenter.engine.set_content_rect(Some(content)); g.ui_dirty = true; }
            // the panels: the settings mirror the store; the inspector mirrors the Transport
            g.settings_panel.sync(&self.settings, now);
            if g.settings_panel.tick(now) || g.inspector.animating(now) { g.ui_dirty = true; }
            if !g.fullscreen {
                let lc = g.left.content_rect(); let lh = g.settings_panel.layout(&mut g.fonts, lc.x0, lc.x1, lc.y0, scale); g.left.content_height = lh + 2.0 * t::RAIL_PADDING as f64 * scale;
                g.inspector.sync(self.player.as_ref());
                let rc = g.right.content_rect(); let rh = g.inspector.layout(&mut g.fonts, rc.x0, rc.x1, rc.y0, scale); g.right.content_height = rh + 2.0 * t::RAIL_PADDING as f64 * scale;
            }
            // the bar's volume / mute / loop and the keys write the store back (QCView persists them on change)
            if let Some(p) = self.player.as_ref() { if p.audio.has_audio() { self.settings.set_float(settings::k::VOLUME, p.audio.volume() as f64); self.settings.set_bool(settings::k::MUTED, p.audio.is_muted()); } self.settings.set_bool(settings::k::LOOP_ENABLED, p.loop_enabled()); }
            if !g.fullscreen { self.settings.set_int(settings::k::LEFT_RAIL_WIDTH, g.left.width.round() as i64); self.settings.set_int(settings::k::RIGHT_RAIL_WIDTH, g.right.width.round() as i64); self.settings.set_bool(settings::k::LEFT_RAIL_COLLAPSED, g.left.collapsed); self.settings.set_bool(settings::k::RIGHT_RAIL_COLLAPSED, g.right.collapsed); }
            if let Err(e) = self.settings.save_if_dirty() { log::warn!("settings: {e}"); }
            g.scrubber.layout(kurbo::Rect::new(0.0, scrub_y, w as f64, h as f64), scale);
            if g.scrubber.follow(&st, scale) { g.ui_dirty = true; }
            // the waveform: ask the probe engine for the visible window at column resolution; repaint as peaks land
            if let Some(p) = self.player.as_ref() { if p.audio.has_audio() && g.waveform_enabled {
                g.waveform.set_playback_active(p.is_playing() || p.is_scrubbing());
                let (t0, t1) = g.scrubber.visible_window(&st); let cols = g.scrubber.track_columns();
                let req = (p.path().to_string(), t0, t1, cols);
                if req != g.wave_req { g.wave_req = req.clone(); g.waveform.request_window(&req.0, t0, t1, cols); g.ui_dirty = true; }
                let wgen = g.waveform.data_generation();
                if wgen != g.wave_seen || g.ui_dirty { g.wave_seen = wgen; g.wave_peaks = g.waveform.sample_columns(&req.0, t0, t1, cols).into_iter().map(|c| if c.valid { (c.mn, c.mx) } else { (0.0, 0.0) }).collect(); g.ui_dirty = true; }
            } }
            if g.thumbs.tick(self.player.as_mut(), g.scrubber.hover_frame, now) { g.ui_dirty = true; }
            let _ = frame;
            if u.is_dirty() || g.ui_dirty || key != g.ui_shown_frame {
                let bands_y = scrub_y - t::TRANSPORT_BAR_HEIGHT as f64 * scale;
                g.bar.layout(&mut g.fonts, w as f64, bands_y, scale);
                g.strip.sync(self.player.as_ref());
                g.strip.layout(w as f64, bands_y - t::STATUS_STRIP_HEIGHT as f64 * scale, scale);
                let content = g.presenter.engine.content_rect().map(|(x, y, cw, ch)| kurbo::Rect::new(x as f64, y as f64, (x + cw) as f64, (y + ch) as f64)).unwrap_or(kurbo::Rect::new(0.0, 0.0, w as f64, h as f64));
                let scene = u.scene_mut();
                ui_sketch::bands(scene, w, h, scale, slide);
                if !g.fullscreen {
                    for (rail, is_left) in [(&g.left, true), (&g.right, false)] {
                        rail.paint_background(scene, now);
                        if rail.rect.width() > 0.0 {
                            rail.push_clip(scene);
                            if is_left { g.settings_panel.paint(scene, &mut g.fonts, scale, now, true); } else { g.inspector.paint(scene, &mut g.fonts, scale, now); }
                            rail.pop_clip(scene);
                        }
                        rail.paint_foreground(scene, &mut g.fonts, scale, now);
                    }
                }
                g.strip.paint(scene, &mut g.fonts, scale, now);
                g.bar.paint(scene, &mut g.fonts, scale, now);
                g.scrubber.paint(scene, &mut g.fonts, scale, &st, if g.wave_peaks.is_empty() { None } else { Some(&g.wave_peaks) });
                g.thumbs.paint(scene, content, scale, Some(g.pointer.0));
                g.strip.paint_popover(scene, &mut g.fonts, scale);
                g.bar.paint_tooltip(scene, &mut g.fonts, scale, kurbo::Rect::new(0.0, 0.0, w as f64, h as f64), now);
                if !g.fullscreen { g.settings_panel.paint_overlays(scene, &mut g.fonts, scale, kurbo::Rect::new(0.0, 0.0, w as f64, h as f64), now); }
                g.ui_shown_frame = key; g.ui_dirty = false;
            }
            if let Some(ms) = u.render(&g.presenter.engine.device, &g.presenter.engine.queue) { g.ui_ms.push(ms); }
        }
        let presented = g.presenter.draw(|| g.window.pre_present_notify());
        if presented {
            self.frames_presented += 1;
            if let Some(lp) = self.last_present { self.presents.push((now - lp).as_secs_f32() * 1000.0); }
            self.last_present = Some(now);
            if shown_new && !self.first_presented { self.first_presented = true; if let Some(t) = self.t_open { println!("open → first frame presented: {:.0} ms", t.elapsed().as_secs_f32() * 1000.0); } }
        } else { std::thread::sleep(std::time::Duration::from_millis(16)); }
        if shown_new { self.after_show(); }
        if self.o.stats && now.duration_since(self.last_report).as_secs_f32() >= 2.0 {
            let dt = now.duration_since(self.last_report).as_secs_f32();
            let edr = Presenter::edr_headroom(); let edr_max = edr.iter().cloned().fold(0.0, f64::max); if edr_max > self.edr_seen { self.edr_seen = edr_max; }
            let (lat_p95, dropped, late) = self.player.as_mut().map(|p| (pct(&mut p.stats.lateness_ms.clone(), 95.0), p.stats.dropped, p.stats.late)).unwrap_or((f32::NAN, 0, 0));
            let g = self.gpu.as_mut().unwrap();
            let ui = if g.ui_ms.is_empty() { String::new() } else { format!("  ui renders {} ms p50 {:.2} max {:.2}", g.ui_ms.len(), pct(&mut g.ui_ms, 50.0), pct(&mut g.ui_ms, 100.0)) };
            if let Some(p) = self.player.as_mut() { if p.audio.has_audio() { let mut d = p.audio.take_drift(); if let Some(a) = p.audio.stats() { if !d.is_empty() { println!("        audio drift ms p50 {:+.1} p95 {:+.1} max {:+.1} (n={})  re-seeks {} holds {} skips {} underruns {} fades {} callbacks {} contended {}", pct(&mut d, 50.0), pct(&mut d.iter().map(|v| v.abs()).collect::<Vec<_>>(), 95.0), d.iter().fold(0.0f32, |m, v| m.max(v.abs())), d.len(), a.reseeks, a.holds, a.skips, a.underruns, a.fades, a.callbacks, a.contended); } } } }
            println!("{:>5.1} s  shown {:>5.1} fps  show ms p50 {:.2} max {:.2}  present ms p50 {:.1} p95 {:.1} max {:.1}  lateness p95 {:.1}  dropped {dropped} late {late}  EDR now {:?}{ui}",
                self.t0.map(|t| t.elapsed().as_secs_f32()).unwrap_or(0.0), self.shown_in_window as f32 / dt, pct(&mut self.show_ms, 50.0), pct(&mut self.show_ms, 100.0), pct(&mut self.presents, 50.0), pct(&mut self.presents, 95.0), pct(&mut self.presents, 100.0), lat_p95, edr);
            g.ui_ms.clear();
            self.shown_in_window = 0; self.show_ms.clear(); self.presents.clear(); self.last_report = now;
        }
    }
    /// One pointer event through the UI's router: the transport bar's commands go the keys' way (`Transport::apply`).
    fn pointer(&mut self, e: min_ui::Event) {
        let now = Instant::now();
        let Some(g) = self.gpu.as_mut() else { return };
        let (w, h) = g.ui.as_ref().map(|u| u.size()).unwrap_or((1, 1)); let scale = g.window.scale_factor();
        if let Some((x, y)) = e.pos() { g.pointer = (x, y); } else { g.pointer = (-1.0, -1.0); }
        let bounds = kurbo::Rect::new(0.0, 0.0, w as f64, h as f64);
        // the rails first (a modal dialog or an open menu in the settings panel takes everything)
        if !g.fullscreen {
            let had_focus = g.settings_panel.has_focus();
            let modal = g.settings_panel.is_modal() || g.settings_panel.menu_open();
            let lr = g.left.event(&e, now); let rr = g.right.event(&e, now);
            if lr.dirty || rr.dirty { g.ui_dirty = true; }
            let over_left = e.pos().is_some_and(|(x, y)| g.left.hit(x, y)); let over_right = e.pos().is_some_and(|(x, y)| g.right.hit(x, y));
            let want = if g.left.is_dragging() || g.right.is_dragging() || (lr.consumed && !over_left) || (rr.consumed && !over_right) { winit::cursor::CursorIcon::ColResize } else if st_hit(self.player.as_ref(), &g.scrubber, e.pos()) { winit::cursor::CursorIcon::Pointer } else { winit::cursor::CursorIcon::Default };
            if want != g.cursor { g.cursor = want; g.window.set_cursor(winit::cursor::Cursor::Icon(want)); }
            let mut cmds = vec![];
            if modal || over_left || matches!(e, min_ui::Event::Leave | min_ui::Event::Up { .. }) || g.settings_panel.has_focus() {
                // a press outside the settings' fields commits a typed number
                if matches!(e, min_ui::Event::Down { .. }) && !over_left && had_focus { cmds.extend(g.settings_panel.blur(&mut self.settings)); }
                let (d, _c, more) = g.settings_panel.event(&e, now, &mut g.fonts, &mut self.settings, bounds, scale); if d { g.ui_dirty = true; } cmds.extend(more);
            }
            if g.settings_panel.has_focus() != had_focus { set_ime(&**g.window, g.settings_panel.has_focus()); }
            if over_right || matches!(e, min_ui::Event::Leave | min_ui::Event::Up { .. }) { if g.inspector.event(&e, now) { g.ui_dirty = true; } }
            let _ = g; for c in cmds { self.apply_setting(c); }
            let g = self.gpu.as_mut().unwrap();
            if modal || lr.consumed || rr.consumed || over_left || over_right { if !matches!(e, min_ui::Event::Leave) { return } }
            let _ = g;
        }
        let g = self.gpu.as_mut().unwrap();
        let was_open = g.strip.is_open();
        let (sd, consumed) = g.strip.event(&e, now, &mut g.fonts, kurbo::Rect::new(0.0, 0.0, w as f64, h as f64), scale);
        if sd { g.ui_dirty = true; }
        if g.strip.is_open() != was_open { set_ime(&**g.window, g.strip.is_open()); }
        if consumed { return }
        let (cmds, dirty) = g.bar.event(&e, now);
        if dirty { g.ui_dirty = true; }
        let st = scrub_state(self.player.as_ref());
        let (scmds, sd) = g.scrubber.event(&e, now, &st, scale);
        if sd { g.ui_dirty = true; }
        if let Some(p) = self.player.as_mut() { for c in scmds { match c { min_ui::ScrubCmd::Begin => p.begin_scrub(), min_ui::ScrubCmd::To(f) => p.scrub_to_frame(f), min_ui::ScrubCmd::End => p.end_scrub() } } }
        if g.fullscreen { let want = if st.frames > 0 && e.pos().is_some_and(|(x, y)| g.scrubber.hit(x, y)) { winit::cursor::CursorIcon::Pointer } else { winit::cursor::CursorIcon::Default }; if want != g.cursor { g.cursor = want; g.window.set_cursor(winit::cursor::Cursor::Icon(want)); } }
        for c in cmds {
            match c {
                transport_bar::Cmd::Act(a) => {
                    let handled = self.player.as_mut().map(|p| p.apply(a, now)).unwrap_or(false);
                    if !handled && a == Action::ToggleFullscreen { self.toggle_fullscreen(); return }
                    else if !handled { log::info!("{a:?}: nothing to apply it to"); }
                }
                transport_bar::Cmd::Open => println!("open: the file dialog arrives with the shell step (drop a file on the window meanwhile)"),
                transport_bar::Cmd::Pin => {}
                transport_bar::Cmd::Volume(v) => { if let Some(p) = self.player.as_mut() { p.audio.set_volume(v); } }
            }
        }
    }
    /// A settings change took: push it to whatever it drives (QCView's `WindowManager` setters, one by one).
    fn apply_setting(&mut self, c: settings_panel::Cmd) {
        use settings::k;
        let keys: Vec<&'static str> = match c { settings_panel::Cmd::Changed(k) => vec![k], settings_panel::Cmd::ResetAll => vec![k::TIMELINE_HOVER_THUMBS, k::TIMELINE_WAVEFORMS, k::VOLUME, k::MUTED, k::SYNC_OFFSET_MS, k::SCRUB_MUTED, k::LOOP_ENABLED, k::LEFT_RAIL_WIDTH, k::RIGHT_RAIL_WIDTH, k::LEFT_RAIL_COLLAPSED, k::RIGHT_RAIL_COLLAPSED] };
        let now = Instant::now();
        for key in keys {
            let Some(g) = self.gpu.as_mut() else { return };
            match key {
                k::TIMELINE_HOVER_THUMBS => { g.thumbs.enabled = self.settings.bool(key); g.ui_dirty = true; }
                k::TIMELINE_WAVEFORMS => { g.waveform_enabled = self.settings.bool(key); if !g.waveform_enabled { g.wave_peaks.clear(); g.wave_req = (String::new(), 0.0, 0.0, 0); } g.ui_dirty = true; }
                k::VOLUME => if let Some(p) = self.player.as_mut() { p.audio.set_volume(self.settings.float(key) as f32); },
                k::MUTED => if let Some(p) = self.player.as_mut() { p.audio.set_muted(self.settings.bool(key)); },
                k::SYNC_OFFSET_MS => if let Some(p) = self.player.as_mut() { p.audio.set_sync_offset_ms(self.settings.int(key) as i32); },
                k::SCRUB_MUTED => if let Some(p) = self.player.as_mut() { p.audio.set_scrub_muted(self.settings.bool(key)); },
                k::LOOP_ENABLED => if let Some(p) = self.player.as_mut() { p.set_loop_enabled(self.settings.bool(key)); },
                k::LEFT_RAIL_WIDTH => { g.left.width = self.settings.int(key) as f64; g.ui_dirty = true; }
                k::RIGHT_RAIL_WIDTH => { g.right.width = self.settings.int(key) as f64; g.ui_dirty = true; }
                k::LEFT_RAIL_COLLAPSED => { g.left.set_collapsed(self.settings.bool(key), now); g.ui_dirty = true; }
                k::RIGHT_RAIL_COLLAPSED => { g.right.set_collapsed(self.settings.bool(key), now); g.ui_dirty = true; }
                _ => {}   // applies at the next open (performance/*) or has no consumer yet
            }
        }
    }
    fn after_show(&mut self) {
        let frame_no = self.player.as_ref().and_then(|p| p.current()).map(|h| h.frame_no).unwrap_or(-1);
        let g = self.gpu.as_ref().unwrap();
        if let Some((path, n)) = &self.o.snap_out { if frame_no == *n as i32 && !self.snapped { self.snapped = true; match g.presenter.engine.snapshot(Snapshot::Encoded) { Ok((px, w, h)) => { write_png_rgb8(path, &px, w, h); println!("snap-out written: {path} ({w}x{h}, display-encoded 8-bit, frame {n})"); } Err(e) => println!("snap-out: {e}") } } }
        if self.o.verify_pq && frame_no == 24 && !self.verified { self.verified = true; self.verify_pq(); }
    }
    /// S1's check for the synthetic PQ ramp fixture (rows < H/2: grey ramp code = x/(W-1); then bands at 0.5081 / 0.7518 / 0.9026).
    fn verify_pq(&self) {
        let g = self.gpu.as_ref().unwrap();
        let Ok((px, w, h)) = g.presenter.engine.snapshot(Snapshot::Linear) else { return };
        let pq = |c: f64| { let (m1, m2, c1, c2, c3) = (0.1593017578125, 78.84375, 0.8359375, 18.8515625, 18.6875); let e = c.powf(1.0 / m2); ((e - c1).max(0.0) / (c2 - c3 * e)).powf(1.0 / m1) * 10000.0 };
        let white = self.o.white as f64; let row = (h / 4) as usize;
        let (mut worst, mut worst_x, mut over1) = (0f64, 0usize, 0u32);
        for x in 0..w as usize {
            let code = x as f64 / (w - 1) as f64; let exp = pq(code) / white; let got = px[(row * w as usize + x) * 4] as f64;
            if got > 1.0 { over1 += 1 }
            if exp > 0.001 { let rel = ((got - exp) / exp).abs(); if rel > worst { worst = rel; worst_x = x; } }
        }
        let bands: Vec<String> = [0.5081f64, 0.7518, 0.9026].iter().enumerate().map(|(i, code)| { let y = (h / 2 + (h / 6) * i as u32 + h / 12) as usize; let got = px[(y * w as usize + w as usize / 2) * 4] as f64; format!("band {code:.4} → expect {:.4} ({:.0} nit) got {:.4} ({:.0} nit)", pq(*code) / white, pq(*code), got, got * white) }).collect();
        println!("verify-pq (row {row}, {} nits = 1.0): worst relative error {:.3} % at x={worst_x} (code {:.3}); {} of {} samples above 1.0 (EDR range)\n  {}", self.o.white, worst * 100.0, worst_x as f64 / (w - 1) as f64, over1, w, bands.join("\n  "));
    }
    /// Enter / leave fullscreen (QCView's F): the content rect becomes the whole window, the bands hide until hovered.
    fn toggle_fullscreen(&mut self) {
        let Some(g) = self.gpu.as_mut() else { return };
        let fs = g.window.fullscreen();
        let entering = fs.is_none();
        g.window.set_fullscreen(if entering { Some(Fullscreen::Borderless(None)) } else { None });
        g.fullscreen = entering;
        let now = Instant::now();
        g.bands.set_target(if entering { 0.0 } else { 1.0 }, now); g.bands_leave = None;
        let s = g.window.surface_size(); let sc = g.window.scale_factor();
        g.content_set = None;
        g.presenter.engine.set_content_rect(Some(if entering { (0, 0, s.width, s.height) } else { content_rect(s.width, s.height, sc) }));
        g.ui_dirty = true;
    }
    fn finish(&mut self) {
        if let Some(g) = self.gpu.as_mut() {
            g.thumbs.shutdown();
            if !g.fullscreen {
                let maximized = g.window.is_maximized();
                self.settings.set_bool(settings::k::WINDOW_MAXIMIZED, maximized);
                if !maximized { if let Ok(p) = g.window.outer_position() { self.settings.set_int(settings::k::WINDOW_X, p.x as i64); self.settings.set_int(settings::k::WINDOW_Y, p.y as i64); } let s = g.window.surface_size(); self.settings.set_int(settings::k::WINDOW_WIDTH, s.width as i64); self.settings.set_int(settings::k::WINDOW_HEIGHT, s.height as i64); }
            }
        }
        if let Err(e) = self.settings.save_if_dirty() { log::warn!("settings: {e}"); }
        if self.finished { return } self.finished = true;
        let wall = self.t0.map(|t| t.elapsed().as_secs_f64()).unwrap_or(0.0);
        if let Some(p) = self.player.as_mut() {
            let s = &mut p.stats;
            println!("play: shown {} frames, dropped {}, late (> 1 refresh) {}, black refreshes {}, loops {}; lateness ms vs the clock p50 {:.1} p95 {:.1} max {:.1} min {:.1}; cadence (refreshes per frame → count) {:?}; decode ms p50 {:.1} max {:.1}{}",
                s.shown, s.dropped, s.late, s.black, s.loops, pct(&mut s.lateness_ms, 50.0), pct(&mut s.lateness_ms, 95.0), pct(&mut s.lateness_ms, 100.0), pct(&mut s.lateness_ms, 0.0), s.cadence, pct(&mut s.decode_ms, 50.0), pct(&mut s.decode_ms, 100.0),
                if s.seek_land_ms.is_empty() { String::new() } else { format!("; stream seeks landed ms p50 {:.1} p95 {:.1} (n={})", pct(&mut s.seek_land_ms, 50.0), pct(&mut s.seek_land_ms, 95.0), s.seek_land_ms.len()) });
        }
        if let Some(p) = self.player.as_ref() { if let Some(idx) = p.decoder().frame_index() { println!("frame index: tier 2 {:?} ({} frames, {} keyframes); decode errors {}{}", idx.scan_state(), idx.scan_frames_built(), idx.keyframe_count(), p.decoder().decode_error_count(), if p.decoder().decode_error_count() > 0 { format!(" (last: {})", p.decoder().last_decode_error()) } else { String::new() }); } }
        let cpu = qcv_player::process_cpu_seconds();
        if wall > 0.0 { println!("process CPU time {cpu:.2} s over {wall:.1} s = {:.0} % of one core (decode threads included); max RSS {:.0} MB; frames presented {}; EDR headroom seen {:.1}", 100.0 * cpu / wall, qcv_player::max_rss_mb(), self.frames_presented, self.edr_seen); }
        if let Some(g) = self.gpu.as_ref() { let r = g.presenter.layer_report(); if !r.is_empty() { println!("{r}"); } }
    }
}

fn describe(p: &Transport) -> String {
    let i = p.info();
    format!("{} {}x{} {} fps ({} frames) {} {:?} {:?} {:?} {:?} {}-bit  decode {}{}", i.codec_short, i.width, i.height, i.rate(), i.frame_count, i.pix_fmt, i.meta.matrix, i.meta.transfer, i.meta.primaries, i.meta.range, i.meta.bit_depth,
        if i.hw_accel.is_empty() { "software + upload".to_string() } else { format!("{} (zero-copy when the layout is bridged)", i.hw_accel) }, i.source_timecode.as_ref().map(|t| format!("  TC {t}")).unwrap_or_default())
}

/// IME on while a text node has focus (winit 0.31's request API; the deprecated `set_ime_allowed` is gone).
fn set_ime(window: &dyn Window, on: bool) {
    use winit::window::{ImeCapabilities, ImeEnableRequest, ImeRequest, ImeRequestData};
    let req = if on { match ImeEnableRequest::new(ImeCapabilities::new(), ImeRequestData::default()) { Some(r) => ImeRequest::Enable(r), None => return } } else { ImeRequest::Disable };
    if let Err(e) = window.request_ime_update(req) { log::debug!("ime {on}: {e}"); }
}

/// winit's key to the UI's (S3's table; the strip and the settings fields share it).
fn to_key_event(event: &winit::event::KeyEvent, mods: qcv_player::Mods) -> min_ui::KeyEvent {
    let key = match &event.logical_key {
        Key::Named(NamedKey::Escape) => min_ui::Key::Named(min_ui::Named::Escape), Key::Named(NamedKey::Enter) => min_ui::Key::Named(min_ui::Named::Enter), Key::Named(NamedKey::Tab) => min_ui::Key::Named(min_ui::Named::Tab),
        Key::Named(NamedKey::Backspace) => min_ui::Key::Named(min_ui::Named::Backspace), Key::Named(NamedKey::Delete) => min_ui::Key::Named(min_ui::Named::Delete),
        Key::Named(NamedKey::ArrowLeft) => min_ui::Key::Named(min_ui::Named::Left), Key::Named(NamedKey::ArrowRight) => min_ui::Key::Named(min_ui::Named::Right),
        Key::Named(NamedKey::ArrowUp) => min_ui::Key::Named(min_ui::Named::Up), Key::Named(NamedKey::ArrowDown) => min_ui::Key::Named(min_ui::Named::Down),
        Key::Named(NamedKey::Home) => min_ui::Key::Named(min_ui::Named::Home), Key::Named(NamedKey::End) => min_ui::Key::Named(min_ui::Named::End),
        Key::Character(c) => min_ui::Key::Char(c.to_string()), _ => min_ui::Key::Char(String::new()),
    };
    min_ui::KeyEvent { key, text: event.text.as_ref().map(|t| t.to_string()), shift: mods.shift, cmd: mods.ctrl }
}
fn st_hit(p: Option<&Transport>, sc: &min_ui::Scrubber, pos: Option<(f64, f64)>) -> bool { p.is_some_and(|p| p.frame_count() > 0) && pos.is_some_and(|(x, y)| sc.hit(x, y)) }

/// The scrubber's view of the Transport.
fn scrub_state(p: Option<&Transport>) -> min_ui::ScrubState {
    let Some(p) = p else { return Default::default() };
    min_ui::ScrubState { frames: p.frame_count(), fps: p.rate().as_f64(), current: p.current_frame(), in_point: p.in_point(), out_point: p.out_point(), playing: p.is_playing(), scrubbing: p.is_scrubbing(), cached: None }
}

/// The video's rect: everything above the three bottom bands (canon §25 row 169).
fn content_rect(w: u32, h: u32, scale: f64) -> (u32, u32, u32, u32) {
    let bands = ((t::STATUS_STRIP_HEIGHT + t::TRANSPORT_BAR_HEIGHT + t::SCRUBBER_HEIGHT) as f64 * scale).round() as u32;
    (0, 0, w, h.saturating_sub(bands).max(1))
}

fn write_png_rgb8(path: &str, px: &[f32], w: u32, h: u32) {
    let bytes: Vec<u8> = px.chunks(4).flat_map(|c| [(c[0].clamp(0.0, 1.0) * 255.0 + 0.5) as u8, (c[1].clamp(0.0, 1.0) * 255.0 + 0.5) as u8, (c[2].clamp(0.0, 1.0) * 255.0 + 0.5) as u8]).collect();
    let Ok(file) = std::fs::File::create(path) else { println!("cannot write {path}"); return };
    let mut enc = png::Encoder::new(std::io::BufWriter::new(file), w, h); enc.set_color(png::ColorType::Rgb); enc.set_depth(png::BitDepth::Eight);
    enc.write_header().unwrap().write_image_data(&bytes).unwrap();
}

impl ApplicationHandler for App {
    fn can_create_surfaces(&mut self, el: &dyn ActiveEventLoop) {
        if self.gpu.is_some() { return }
        for (i, m) in el.available_monitors().enumerate() { println!("monitor {i}: {:?} at {:?} scale {}", m.name(), m.position(), m.scale_factor()); }
        let mut attrs = WindowAttributes::default().with_title("qcv").with_surface_size(LogicalSize::new(1280.0, 720.0));
        // QCView's `window/*` restore: the saved physical size and position when there is one (first run: 1280 × 720, the OS places it)
        { let st = &self.settings; use settings::k;
          if st.is_set(k::WINDOW_WIDTH) && st.int(k::WINDOW_WIDTH) >= 320 && st.int(k::WINDOW_HEIGHT) >= 200 { attrs = attrs.with_surface_size(winit::dpi::PhysicalSize::new(st.int(k::WINDOW_WIDTH) as u32, st.int(k::WINDOW_HEIGHT) as u32)); }
          if st.is_set(k::WINDOW_X) && st.is_set(k::WINDOW_Y) && self.o.screen.is_none() { attrs = attrs.with_position(PhysicalPosition::new(st.int(k::WINDOW_X) as i32, st.int(k::WINDOW_Y) as i32)); }
          if st.bool(k::WINDOW_MAXIMIZED) { attrs = attrs.with_maximized(true); } }
        if let Some(i) = self.o.screen { if let Some(m) = el.available_monitors().nth(i) { if let Some(p) = m.position() { attrs = attrs.with_position(PhysicalPosition::new(p.x + 100, p.y + 100)); } } }
        let window: Arc<Box<dyn Window>> = Arc::new(el.create_window(attrs).expect("window"));
        let instance = Presenter::instance();
        let surface = instance.create_surface(window.clone()).expect("surface");
        let size = window.surface_size();
        #[cfg(target_os = "macos")]
        let ns_view = { use raw_window_handle::{HasWindowHandle, RawWindowHandle}; match window.window_handle().unwrap().as_raw() { RawWindowHandle::AppKit(h) => Some(h.ns_view.as_ptr()), _ => None } };
        #[cfg(not(target_os = "macos"))]
        let ns_view = None;
        let mut presenter = Presenter::new(&instance, surface, (size.width, size.height), self.o.out, ns_view).expect("presenter");
        let mut d = presenter.engine.display(); d.white = self.o.white; presenter.engine.set_display(d);
        presenter.engine.set_content_rect(Some(content_rect(size.width, size.height, window.scale_factor())));
        if self.o.identity_chain { presenter.engine.set_color_pass(Some(Box::new(min_present::IdentityColorPass))); }
        let i = presenter.info();
        println!("presenter: {} ({}) → {:?} {:?} colour space {:?}{}; features {:?}; scale {}", i.adapter, i.backend, i.output, i.format, i.color_space, if i.layer_workaround { " (PQ set on the layer by name)" } else { "" }, i.features, window.scale_factor());
        if !i.present_modes.is_empty() { println!("present modes {:?}", i.present_modes); }
        let r = presenter.layer_report(); if !r.is_empty() { println!("{r}"); }
        #[cfg(windows)] let dev = presenter.engine.dx12_device_raw();
        #[cfg(not(windows))] let dev = None;
        self.hw = if self.o.sw { None } else { HwDevice::platform(dev) };
        if let Some(h) = &self.hw { println!("hardware decode: {}", h.name()); } else if !self.o.sw { println!("hardware decode unavailable — software"); }
        #[cfg(target_os = "macos")] { mac::menu_bar(); mac::activate(); }
        window.focus_window();
        println!("window: surface {:?} scale {} visible {:?} monitor {:?}", window.surface_size(), window.scale_factor(), window.is_visible(), window.current_monitor().and_then(|m| m.name().map(|n| n.to_string())));
        window.request_redraw();
        // the rails from the store (QCView `ui.layout/*`; "Always open in minimal mode" collapses both at launch)
        let (left, right, waveform_enabled, thumbs_enabled) = { let st = &self.settings; use settings::k; let minimal = st.bool(k::ALWAYS_OPEN_MINIMAL);
            let mut l = min_ui::Rail::new(min_ui::Side::Left, st.int(k::LEFT_RAIL_WIDTH) as f64, minimal || st.bool(k::LEFT_RAIL_COLLAPSED)); l.settle();
            let mut r = min_ui::Rail::new(min_ui::Side::Right, st.int(k::RIGHT_RAIL_WIDTH) as f64, minimal || st.bool(k::RIGHT_RAIL_COLLAPSED)); r.settle();
            (l, r, st.bool(k::TIMELINE_WAVEFORMS), st.bool(k::TIMELINE_HOVER_THUMBS)) };
        // the UI layer (stage 4): at the surface's physical size, on the presenter's device; `QCV_NO_UI=1` leaves it out for A/B
        let mut ui = if std::env::var_os("QCV_NO_UI").is_some() { None } else { min_ui::UiLayer::new(&presenter.engine.device, size.width, size.height).map_err(|e| println!("ui layer: {e}")).ok() };
        if let Some(u) = ui.as_mut() { presenter.engine.set_overlay(Some(u.view())); }
        self.gpu = Some(Gpu { window, presenter, ui, fonts: min_ui::Fonts::new(), ui_ms: vec![], ui_shown_frame: None, bar: transport_bar::TransportBar::new(), strip: status_strip::StatusStrip::new(), scrubber: min_ui::Scrubber::new(), thumbs: hover_thumbs::HoverThumbs::new(), pointer: (-1.0, -1.0), ui_dirty: true, fullscreen: false, bands: min_ui::anim::Ramp::new(1.0, t::BANDS_SLIDE_DURATION), bands_leave: None, waveform: qcv_player::WaveformProbeEngine::new(), wave_seen: 0, wave_req: (String::new(), 0.0, 0.0, 0), wave_peaks: vec![],
            left, right, settings_panel: settings_panel::SettingsPanel::new(), inspector: inspector_panel::InspectorPanel::new(), content_set: None, waveform_enabled, cursor: winit::cursor::CursorIcon::Default });
        if let Some(g) = self.gpu.as_mut() { g.thumbs.enabled = thumbs_enabled; }
        self.t0 = Some(Instant::now()); self.last_report = Instant::now();
        if let Some(f) = self.o.file.clone() { self.open(&f); }
    }
    fn about_to_wait(&mut self, el: &dyn ActiveEventLoop) {
        if self.o.seconds > 0.0 { if let Some(t0) = self.t0 { let dl = t0 + std::time::Duration::from_secs_f32(self.o.seconds); if Instant::now() >= dl { self.finish(); el.exit(); return } el.set_control_flow(ControlFlow::WaitUntil(dl)); } }
    }
    fn window_event(&mut self, el: &dyn ActiveEventLoop, id: WindowId, event: WindowEvent) {
        // `QCV_TRACE_INPUT=1`: every pointer / focus event as the window layer delivers it (the Windows leave-and-return click report, 2026-11-07)
        if *TRACE_INPUT { match &event { WindowEvent::PointerMoved { .. } | WindowEvent::PointerButton { .. } | WindowEvent::PointerLeft { .. } | WindowEvent::PointerEntered { .. } | WindowEvent::Focused(_) | WindowEvent::MouseWheel { .. } => println!("input: {event:?}"), _ => {} } }
        match event {
            WindowEvent::CloseRequested => { self.finish(); el.exit(); }
            WindowEvent::Focused(f) => { log::info!("focused {f}"); if !f { if let Some(g) = self.gpu.as_mut() { if g.strip.is_open() { g.strip.close(); g.ui_dirty = true; } } } }
            WindowEvent::Ime(ime) => { if let Some(g) = self.gpu.as_mut() { let e = match ime { winit::event::Ime::Preedit(t, c) => Some(min_ui::ImeEvent::Preedit(t, c)), winit::event::Ime::Commit(t) => Some(min_ui::ImeEvent::Commit(t)), _ => None }; if let Some(e) = e { if g.settings_panel.has_focus() { g.settings_panel.ime(&e, &mut g.fonts); } else { g.strip.ime(&e, &mut g.fonts); } g.ui_dirty = true; } } }
            WindowEvent::SurfaceResized(s) => if let Some(g) = self.gpu.as_mut() { g.presenter.resize(s.width, s.height); let sc = g.window.scale_factor(); g.content_set = None; g.presenter.engine.set_content_rect(Some(if g.fullscreen { (0, 0, s.width, s.height) } else { content_rect(s.width, s.height, sc) })); if let Some(u) = g.ui.as_mut() { u.resize(&g.presenter.engine.device, s.width, s.height); g.presenter.engine.set_overlay(Some(u.view())); } g.window.request_redraw(); },
            WindowEvent::RedrawRequested => { self.tick(); if let Some(g) = &self.gpu { g.window.request_redraw(); } }
            WindowEvent::KeyboardInput { event, .. } => {
                let pressed = event.state == ElementState::Pressed;
                if pressed { if let Some(g) = self.gpu.as_mut() { if !g.fullscreen && (g.settings_panel.has_focus() || g.settings_panel.is_modal() || g.settings_panel.menu_open()) {
                    let k = to_key_event(&event, self.mods);
                    let had_focus = g.settings_panel.has_focus();
                    let (handled, cmds) = g.settings_panel.key(&k, &mut g.fonts, &mut self.settings);
                    g.ui_dirty = true;
                    if g.settings_panel.has_focus() != had_focus { set_ime(&**g.window, g.settings_panel.has_focus()); }
                    for c in cmds { self.apply_setting(c); }
                    if handled { return }
                } } }
                // QCView's View menu: Ctrl+1 / Ctrl+2 toggle the rails
                if pressed && self.mods.ctrl && !event.repeat { if let Key::Character(c) = &event.logical_key { if c == "1" || c == "2" { if let Some(g) = self.gpu.as_mut() { if !g.fullscreen { let now = Instant::now(); if c == "1" { g.left.toggle(now); } else { g.right.toggle(now); } g.ui_dirty = true; } } return } } }
                if let Some(g) = self.gpu.as_mut() { if g.strip.is_open() {
                    if pressed {
                        let k = to_key_event(&event, self.mods);
                        let commit = g.strip.key(&k, &mut g.fonts);
                        g.ui_dirty = true;
                        if !g.strip.is_open() { set_ime(&**g.window, false); }
                        match commit { Some(status_strip::Commit::Frame(f)) => { if let Some(p) = self.player.as_mut() { p.seek_to_frame(f); } } Some(status_strip::Commit::Time(s)) => { if let Some(p) = self.player.as_mut() { p.seek_to_time(s); } } None => {} }
                    }
                    return
                } }
                if pressed && matches!(event.logical_key, Key::Named(NamedKey::Escape)) { if self.gpu.as_ref().is_some_and(|g| g.fullscreen) { self.toggle_fullscreen(); return } self.finish(); el.exit(); return }
                let key = match &event.logical_key {
                    Key::Character(c) if c == " " => Some(qcv_player::Key::Space),
                    Key::Character(c) => c.chars().next().map(qcv_player::Key::Char),
                    Key::Named(NamedKey::ArrowLeft) => Some(qcv_player::Key::Left), Key::Named(NamedKey::ArrowRight) => Some(qcv_player::Key::Right),
                    Key::Named(NamedKey::ArrowUp) => Some(qcv_player::Key::Up), Key::Named(NamedKey::ArrowDown) => Some(qcv_player::Key::Down), Key::Named(NamedKey::Home) => Some(qcv_player::Key::Home), Key::Named(NamedKey::End) => Some(qcv_player::Key::End),
                    _ => None,
                };
                let m = self.mods;
                if let Some(a) = key.and_then(|k| qcv_player::map_key(k, m, pressed, event.repeat)) {
                    let handled = self.player.as_mut().map(|p| p.apply(a, Instant::now())).unwrap_or(false);
                    if handled { if let Some(p) = &self.player { println!("{a:?} → {} at frame {}{}", if p.is_playing() { "playing" } else { "paused" }, p.current_frame(), if p.is_fast_seeking() { " (shuttle)" } else { "" }); } }
                    else if a == Action::ToggleFullscreen { self.toggle_fullscreen(); }
                }
            }
            WindowEvent::PointerMoved { position, .. } => self.pointer(min_ui::Event::Move { x: position.x, y: position.y }),
            WindowEvent::PointerLeft { .. } => self.pointer(min_ui::Event::Leave),
            WindowEvent::MouseWheel { delta, .. } => {
                let alt = self.mods.alt;
                // Qt angle units (120 per notch): lines → notches; pixel deltas (trackpads) at 2 units per px; Alt+wheel folds x into y (QCView's Windows rule)
                let dy = match delta { winit::event::MouseScrollDelta::LineDelta(x, y) => (if alt && y == 0.0 { x } else { y }) as f64 * 120.0, winit::event::MouseScrollDelta::PixelDelta(p) => (if alt && p.y == 0.0 { p.x } else { p.y }) * 2.0, _ => 0.0 };
                if let Some(g) = self.gpu.as_mut() { let (x, y) = g.pointer; let now = Instant::now(); let scale = g.window.scale_factor();
                    if !g.fullscreen && g.left.hit(x, y) {
                        if let Some(cmds) = g.settings_panel.wheel(x, y, dy, &mut self.settings) { g.ui_dirty = true; for c in cmds { self.apply_setting(c); } return }
                        if g.left.wheel(-dy / 120.0 * 48.0 * scale, now) { g.ui_dirty = true; } return
                    }
                    if !g.fullscreen && g.right.hit(x, y) { if g.right.wheel(-dy / 120.0 * 48.0 * scale, now) { g.ui_dirty = true; } return }
                    if g.scrubber.hit(x, y) { let st = scrub_state(self.player.as_ref()); let scale = g.window.scale_factor(); if g.scrubber.wheel(x, dy, alt, &st, scale) { g.ui_dirty = true; } } }
            }
            WindowEvent::PointerButton { state, position, button, .. } => {
                let b = match button.mouse_button() { Some(winit::event::MouseButton::Left) => min_ui::Button::Primary, Some(winit::event::MouseButton::Right) => min_ui::Button::Secondary, Some(winit::event::MouseButton::Middle) => min_ui::Button::Middle, _ => return };
                let (x, y) = (position.x, position.y);
                self.pointer(if state == ElementState::Pressed { min_ui::Event::Down { x, y, button: b } } else { min_ui::Event::Up { x, y, button: b } });
            }
            WindowEvent::ModifiersChanged(m) => { let st = m.state(); self.mods = qcv_player::Mods { shift: st.shift_key(), ctrl: st.control_key() || st.meta_key(), alt: st.alt_key(), keypad: false }; if let Some(g) = self.gpu.as_mut() { g.bar.set_shift(self.mods.shift); } }
            WindowEvent::DragEntered { id: tid, .. } => {
                let ok = el.data_transfer(tid).map(|dt| dt.has_type(&TypeHint::UriList)).unwrap_or(false);
                el.set_valid_dnd_actions(tid, if ok { &[DndAction::Copy] } else { &[] }).ok();
            }
            WindowEvent::DragDropped { id: tid, .. } => { if let Ok(s) = el.fetch_data_transfer(tid, &TypeHint::UriList) { self.fetches.insert(s, id); } }
            WindowEvent::DataTransferReceived { serial, value, .. } => {
                if self.fetches.remove(&serial).is_some() { if let Ok(uris) = value.try_as_uris() { if let Some(p) = uris.iter().find_map(|u| min_paths::url::file_url_to_path(u)) { self.open(&p); } } }
            }
            _ => {}
        }
    }
}

static TRACE_INPUT: std::sync::LazyLock<bool> = std::sync::LazyLock::new(|| std::env::var_os("QCV_TRACE_INPUT").is_some());

fn main() {
    env_logger::init();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let arg = |k: &str| args.iter().position(|a| a == k).and_then(|i| args.get(i + 1).cloned());
    let has = |k: &str| args.iter().any(|a| a == k);
    let file = { let mut f = None; let mut skip = false; for (i, a) in args.iter().enumerate() { if skip { skip = false; continue } if a.starts_with("--") { skip = ["--out", "--white", "--seconds", "--snap-out", "--refresh", "--screen", "--threads", "--headless", "--seeks", "--scrub", "--seek-gap", "--hover", "--cut", "--ui-bench", "--ui-snap", "--ui-size", "--audio-bench"].contains(&a.as_str()); continue } let _ = i; f = Some(a.clone()); } f };
    let out = match arg("--out").as_deref() { Some("sdr") => Output::Sdr, Some("edr") => Output::Edr, Some("pq") => Output::Pq, _ => Output::Auto };
    let snap_out = arg("--snap-out").map(|s| { let (p, n) = s.rsplit_once('@').unwrap_or((&s, "24")); (p.to_string(), n.parse().unwrap_or(24)) });
    let scrub = arg("--scrub").and_then(|v| { let (r, ms) = v.rsplit_once(':')?; let (a, b) = r.split_once('-')?; Some((a.parse().ok()?, b.parse().ok()?, ms.parse().ok()?)) });
    let o = Opts { cut: arg("--cut"), hover: arg("--hover").and_then(|v| v.parse().ok()), seek_gap_ms: arg("--seek-gap").and_then(|v| v.parse().ok()).unwrap_or(0), seeks: arg("--seeks").and_then(|v| v.parse().ok()), scrub, scrub_hw: has("--scrub-hw"), headless: arg("--headless").and_then(|v| v.parse().ok()), looping: has("--loop"), file, sw: has("--sw"), out, white: arg("--white").and_then(|v| v.parse().ok()).unwrap_or(100.0), seconds: arg("--seconds").and_then(|v| v.parse().ok()).unwrap_or(0.0), stats: has("--stats"), snap_out, verify_pq: has("--verify-pq"), identity_chain: has("--identity-chain"), refresh_ms: arg("--refresh").and_then(|v| v.parse().ok()).unwrap_or(1000.0 / 60.0), screen: arg("--screen").and_then(|v| v.parse().ok()), threads: arg("--threads").and_then(|v| v.parse().ok()).unwrap_or(0), autoplay: has("--play") };
    if let Some(secs) = arg("--audio-bench").and_then(|v| v.parse().ok()) { run_audio_bench(o, secs); return }
    if let Some(n) = arg("--ui-bench").and_then(|v| v.parse().ok()) { run_ui_bench(o, n, arg("--ui-snap"), arg("--ui-size").and_then(|v| v.split_once('x').and_then(|(a, b)| Some((a.parse().ok()?, b.parse().ok()?))))); return }
    if o.headless.is_some() { run_headless(o); return }
    let el = EventLoop::new().unwrap();
    let settings = settings::open();
    let app = App { o, gpu: None, player: None, hw: None, settings, source_label: String::new(), source_path: String::new(), t0: None, t_open: None, first_presented: false, presents: vec![], last_present: None, show_ms: vec![], last_report: Instant::now(), shown_in_window: 0, fetches: HashMap::new(), mods: qcv_player::Mods::default(), finished: false, frames_presented: 0, edr_seen: 0.0, snapped: false, verified: false };
    el.run_app(app).unwrap();
}

/// `--headless N`: no window — decode N frames through the player, show each on a headless engine (the same zero-copy
/// import and stage graph), write `--snap-out` / run `--verify-pq`. The evidence that needs no display.
/// `--audio-bench SECONDS file`: no window, no speaker — the Transport plays through a wall-paced silent device; the
/// servo's drift is sampled every refresh. Then a seek to one third and the time it takes the drift to re-enter ±20 ms,
/// then a hold/release cycle through pause → play. The step-2b bar: drift settles inside the quiet band and stays,
/// no re-seeks after the first second, no underruns at 1×.
fn run_audio_bench(o: Opts, seconds: f32) {
    let Some(file) = o.file.clone() else { println!("--audio-bench needs a file"); return };
    let src = Source::from_path(&file, None);
    let mut p = match Transport::open(&src, TransportOptions { hw: None, threads: o.threads, loop_enabled: true, scrub: None, ..Default::default() }) { Ok(p) => p, Err(e) => { println!("open {file}: {e}"); return } };
    let a = match qcv_player::AudioPlayer::headless() { Ok(a) => a, Err(e) => { println!("audio: {e}"); return } };
    p.set_audio(Box::new(a));
    println!("opened {} — {}; audio: {}", src.display(), describe(&p), if p.audio.has_audio() { "open" } else { "NONE — the bench needs a file with an audio stream" });
    if !p.audio.has_audio() { return }
    let refresh = std::time::Duration::from_secs_f32(o.refresh_ms / 1000.0);
    let run = |p: &mut Transport, secs: f32, label: &str| {
        let t0 = Instant::now(); let mut drift: Vec<(f32, f32)> = vec![];
        while t0.elapsed().as_secs_f32() < secs {
            let now = Instant::now(); let _ = p.tick(now); if p.current().is_none() { p.idle_refresh(); }
            for d in p.audio.take_drift() { drift.push((t0.elapsed().as_secs_f32(), d)); }
            std::thread::sleep(refresh);
        }
        let settle = drift.iter().find(|(_, d)| d.abs() <= 20.0).map(|(t, _)| *t);
        let after: Vec<f32> = drift.iter().filter(|(t, _)| settle.is_some_and(|s| *t >= s + 0.5)).map(|(_, d)| *d).collect();
        let mut abs: Vec<f32> = after.iter().map(|v| v.abs()).collect();
        let mut signed: Vec<f32> = after.clone();
        let a = p.audio.stats().unwrap_or_default();
        println!("{label}: {} drift samples; first inside ±20 ms at {}; after settling: drift p50 {:+.1} |drift| p50 {:.1} p95 {:.1} max {:.1} ms (n={}); re-seeks {} holds {} skips {} underruns {} starved {} fades {} callbacks {} audible {} contended {}",
            drift.len(), settle.map(|s| format!("{s:.2} s")).unwrap_or("never".into()), pct(&mut signed, 50.0), pct(&mut abs, 50.0), pct(&mut abs, 95.0), pct(&mut abs, 100.0), after.len(), a.reseeks, a.holds, a.skips, a.underruns, a.starved, a.fades, a.callbacks, a.audible, a.contended);
    };
    p.play();
    run(&mut p, seconds, "play from 0");
    let third = p.frame_count() / 3; p.seek_to_frame(third);
    run(&mut p, (seconds / 2.0).max(3.0), &format!("seek to frame {third} while playing"));
    p.pause(); std::thread::sleep(std::time::Duration::from_millis(300)); p.tick(Instant::now()); p.play();
    run(&mut p, (seconds / 2.0).max(3.0), "pause 300 ms → play (hold + release)");
    // the shuttle: hold fast-forward for 1.5 s (2× → 4× → 8×), then rewind for 1 s; the grains feed the callback instead of the decoder
    let t0 = Instant::now(); p.start_fast_seek(1, t0); let mut calls0 = p.audio.stats().map(|a| a.callbacks).unwrap_or(0);
    while t0.elapsed().as_secs_f32() < 1.5 { let now = Instant::now(); let _ = p.tick(now); std::thread::sleep(refresh); }
    let active_ff = p.audio.shuttle_active(); p.stop_fast_seek();
    let t1 = Instant::now(); p.start_fast_seek(-1, t1);
    while t1.elapsed().as_secs_f32() < 1.0 { let now = Instant::now(); let _ = p.tick(now); std::thread::sleep(refresh); }
    let active_rw = p.audio.shuttle_active(); p.stop_fast_seek();
    let calls1 = p.audio.stats().map(|a| a.callbacks).unwrap_or(0); calls0 = calls1 - calls0;
    println!("shuttle: ff 1.5 s (active {active_ff}) + rw 1 s (active {active_rw}): {calls0} device callbacks fed by grains; after release the player is {} at frame {}", if p.is_playing() { "playing" } else { "paused" }, p.current_frame());
    if !p.is_playing() { p.play(); }
    run(&mut p, 3.0, "play on after the shuttle");
    // review speeds: 2× then 0.5× through the WSOLA stage (constant pitch), then back to 1×
    p.set_review_speed(2.0); run(&mut p, 4.0, "review speed 2× (tempo)");
    p.set_review_speed(0.5); run(&mut p, 3.0, "review speed 0.5× (tempo)");
    p.set_review_speed(1.0); run(&mut p, 3.0, "back to 1×");
    println!("source: {} channels ({})", p.audio.source_channels(), p.audio.layout_name());
    println!("process CPU {:.0} % of one core over the run; max RSS {:.0} MB", 0.0, qcv_player::max_rss_mb());
}

/// `--ui-bench N [--ui-snap PATH] [file]`: no window — the UI layer at 2560×1440 (scale 2) over a headless engine; the
/// token sketch is rebuilt + rendered N times at a moving frame (scene-build ms and vello render ms apart), then the
/// composite (over the file's first frame when one is given) is written as an sRGB PNG. The piece-6 harness: the bar is
/// ≤ 1 ms of CPU when the layer changes, nothing when it doesn't.
fn run_ui_bench(o: Opts, n: u32, snap: Option<String>, size: Option<(u32, u32)>) {
    use min_present::engine::{DisplayMode, Engine, OutMode};
    let (w, h) = size.unwrap_or((2560u32, 1440u32));
    let scale = 2.0f64;
    let mut engine = Engine::headless(wgpu::TextureFormat::Rgba16Float).expect("headless engine");
    engine.set_display(DisplayMode { out: OutMode::SdrEncode, white: o.white });
    engine.set_viewport(w, h);
    let mut frame_info: Option<(i32, i32, f64)> = None;
    let mut player: Option<Transport> = None;
    if let Some(file) = o.file.clone() {
        let src = Source::from_path(&file, None);
        match Transport::open(&src, TransportOptions { hw: None, threads: o.threads, loop_enabled: true, scrub: None, ..Default::default() }) {
            Ok(mut p) => {
                p.play();
                let deadline = Instant::now() + std::time::Duration::from_secs(10);
                while p.current().is_none() && Instant::now() < deadline { if let Some(d) = p.tick(Instant::now()) { engine.show(&d.frame).expect("show"); break } p.idle_refresh(); std::thread::sleep(std::time::Duration::from_micros(500)); }
                p.pause();
                frame_info = Some((p.current_frame(), p.frame_count(), p.rate().as_f64()));
                println!("opened {} — {}", src.display(), describe(&p));
                player = Some(p);
            }
            Err(e) => println!("open {file}: {e} — the sketch over black"),
        }
    }
    let t = Instant::now();
    let mut fonts = min_ui::Fonts::new();
    println!("fonts registered in {:.1} ms", t.elapsed().as_secs_f32() * 1000.0);
    let mut ui = min_ui::UiLayer::new(&engine.device, w, h).expect("ui layer");
    engine.set_overlay(Some(ui.view()));
    // the transport bar in every state the snap can show: hover (step forward, with its tooltip), pressed (set in),
    // selected (loop), disabled (the unbacked buttons)
    engine.set_content_rect(Some(content_rect(w, h, scale)));
    let mut strip = status_strip::StatusStrip::new();
    let mut bar = transport_bar::TransportBar::new();
    let bands_y = h as f64 - (t::STATUS_STRIP_HEIGHT + t::TRANSPORT_BAR_HEIGHT + t::SCRUBBER_HEIGHT) as f64 * scale + t::STATUS_STRIP_HEIGHT as f64 * scale;
    let t0 = Instant::now();
    // the rails: the left open on Settings (a switch hovered, a number field focused), the right open on the Inspector so the snap shows both
    let mut store = min_settings::Settings::in_memory(&settings::SCHEMA);
    let mut left = min_ui::Rail::new(min_ui::Side::Left, 280.0, false); left.settle();
    let mut right = min_ui::Rail::new(min_ui::Side::Right, 320.0, false); right.settle();
    let rails_bottom = bands_y - t::STATUS_STRIP_HEIGHT as f64 * scale;
    left.layout(0.0, 0.0, rails_bottom, scale, t0); right.layout(w as f64, 0.0, rails_bottom, scale, t0);
    engine.set_content_rect(Some((left.visible_width(t0, scale) as u32, 0, (w as f64 - left.visible_width(t0, scale) - right.visible_width(t0, scale)) as u32, rails_bottom as u32)));
    let mut panel = settings_panel::SettingsPanel::new(); panel.sync(&store, t0);
    let lc = left.content_rect(); let ph = panel.layout(&mut fonts, lc.x0, lc.x1, lc.y0, scale); left.content_height = ph + 2.0 * t::RAIL_PADDING as f64 * scale;
    let mut inspector = inspector_panel::InspectorPanel::new(); inspector.sync(player.as_ref());
    let rc = right.content_rect(); let ih = inspector.layout(&mut fonts, rc.x0, rc.x1, rc.y0, scale); right.content_height = ih + 2.0 * t::RAIL_PADDING as f64 * scale;
    { let bounds = kurbo::Rect::new(0.0, 0.0, w as f64, h as f64);
      // hover the "Timeline hover thumbnails" switch's label (its help tooltip) and press the scrub-cache field
      let _ = panel.event(&min_ui::Event::Move { x: lc.x0 + 20.0 * scale, y: lc.y0 + 330.0 * scale }, t0, &mut fonts, &mut store, bounds, scale);
      let _ = panel.event(&min_ui::Event::Down { x: lc.x1 - 60.0 * scale, y: lc.y0 + 215.0 * scale, button: min_ui::Button::Primary }, t0, &mut fonts, &mut store, bounds, scale); }
    println!("settings panel: {} px tall at {scale}× ({} rows); inspector {} px", ph.round(), settings::SCHEMA.keys.len(), ih.round());
    bar.sync(player.as_ref(), t0);
    bar.layout(&mut fonts, w as f64, bands_y, scale);
    let (hover, press) = (bar.rect_of("caret-right").unwrap().center(), bar.rect_of("sign-out").unwrap().center());
    bar.event(&min_ui::Event::Down { x: press.x, y: press.y, button: min_ui::Button::Primary }, t0);
    bar.event(&min_ui::Event::Move { x: hover.x, y: hover.y }, t0);
    let shown_at = t0 + std::time::Duration::from_millis(t::TOOLTIP_DELAY as u64 + 100);   // the hover ramp done, the tooltip up
    bar.sync(player.as_ref(), shown_at); bar.tick(shown_at);
    strip.sync(player.as_ref());
    strip.layout(w as f64, bands_y - t::STATUS_STRIP_HEIGHT as f64 * scale, scale);
    strip.stage(status_strip::Mode::Timecode, "1000", &mut fonts, kurbo::Rect::new(0.0, 0.0, w as f64, h as f64), scale);
    let mut scrubber = min_ui::Scrubber::new();
    let mut thumbs = hover_thumbs::HoverThumbs::new();
    let st = scrub_state(player.as_ref());
    scrubber.layout(kurbo::Rect::new(0.0, h as f64 - t::SCRUBBER_HEIGHT as f64 * scale, w as f64, h as f64), scale);
    scrubber.wheel(w as f64 * 0.4, 600.0, false, &st, scale);   // zoomed in about 40 % — frame ticks + the extent marker
    scrubber.event(&min_ui::Event::Move { x: w as f64 * 0.62, y: h as f64 - 10.0 }, t0, &st, scale);
    if let Some(p) = player.as_mut() { thumbs.on_open(p); let deadline = Instant::now() + std::time::Duration::from_millis(1500); while Instant::now() < deadline { thumbs.tick(Some(p), scrubber.hover_frame, Instant::now()); if thumbs_shown(&thumbs) { break } std::thread::sleep(std::time::Duration::from_millis(20)); } }
    let content = engine.content_rect().map(|(x, y, cw, ch)| kurbo::Rect::new(x as f64, y as f64, (x + cw) as f64, (y + ch) as f64)).unwrap();
    let mut wave_peaks: Vec<(f32, f32)> = vec![];
    if let Some(p) = player.as_ref() {
        let eng = qcv_player::WaveformProbeEngine::new();
        let (t0w, t1w) = scrubber.visible_window(&st); let cols = scrubber.track_columns();
        eng.request_window(p.path(), t0w, t1w, cols);
        let deadline = Instant::now() + std::time::Duration::from_millis(3000); let mut last = 0;
        while Instant::now() < deadline { let g = eng.data_generation(); if g != last { last = g; let c = eng.sample_columns(p.path(), t0w, t1w, cols); if c.iter().filter(|c| c.valid).count() * 10 >= cols * 9 { wave_peaks = c.into_iter().map(|c| (c.mn, c.mx)).collect(); break } } std::thread::sleep(std::time::Duration::from_millis(30)); }
        println!("waveform: {} of {} columns probed in time", wave_peaks.len(), cols);
    }
    let (mut build_ms, mut render_ms) = (vec![], vec![]);
    for i in 0..n.max(1) {
        let fi = frame_info.map(|(_, c, f)| ((i as i32 * 7) % c.max(1), c, f)).or(Some((i as i32, 240, 24.0)));
        let t = Instant::now();
        let scene = ui.scene_mut();
        let _ = fi;
        ui_sketch::bands(scene, w, h, scale, 0.0);
        for (rail, is_left) in [(&left, true), (&right, false)] { rail.paint_background(scene, shown_at); rail.push_clip(scene); if is_left { panel.paint(scene, &mut fonts, scale, shown_at, true); } else { inspector.paint(scene, &mut fonts, scale, shown_at); } rail.pop_clip(scene); rail.paint_foreground(scene, &mut fonts, scale, shown_at); }
        strip.paint(scene, &mut fonts, scale, shown_at);
        bar.paint(scene, &mut fonts, scale, shown_at);
        scrubber.paint(scene, &mut fonts, scale, &st, if wave_peaks.is_empty() { None } else { Some(&wave_peaks) });
        thumbs.paint(scene, content, scale, Some(w as f64 * 0.62));
        strip.paint_popover(scene, &mut fonts, scale);
        bar.paint_tooltip(scene, &mut fonts, scale, kurbo::Rect::new(0.0, 0.0, w as f64, h as f64), shown_at);
        panel.tick(shown_at); panel.paint_overlays(scene, &mut fonts, scale, kurbo::Rect::new(0.0, 0.0, w as f64, h as f64), shown_at);
        build_ms.push(t.elapsed().as_secs_f32() * 1000.0);
        if let Some(ms) = ui.render(&engine.device, &engine.queue) { render_ms.push(ms); }
        let _ = engine.device.poll(wgpu::PollType::Wait { submission_index: None, timeout: None });
    }
    assert!(ui.render(&engine.device, &engine.queue).is_none(), "a clean layer renders nothing");
    println!("ui layer {w}x{h} @{scale}: {} builds — scene build ms p50 {:.2} p95 {:.2} max {:.2}; vello render (CPU, encode+submit) ms p50 {:.2} p95 {:.2} max {:.2}; clean layer: no render",
        n, pct(&mut build_ms, 50.0), pct(&mut build_ms, 95.0), pct(&mut build_ms, 100.0), pct(&mut render_ms, 50.0), pct(&mut render_ms, 95.0), pct(&mut render_ms, 100.0));
    let t = Instant::now();
    let px = engine.render_offscreen(w, h).expect("composite");
    println!("composite {w}x{h} render + readback {:.1} ms", t.elapsed().as_secs_f32() * 1000.0);
    if let Some(path) = snap { write_png_rgb8(&path, &px, w, h); println!("ui snap written: {path}"); }
    println!("hover thumbnail staged: {}", if thumbs_shown(&thumbs) { "shown" } else { "none" });
    thumbs.shutdown();
    println!("max RSS {:.0} MB", qcv_player::max_rss_mb());
}
fn thumbs_shown(t: &hover_thumbs::HoverThumbs) -> bool { t.is_shown() }

fn run_headless(o: Opts) {
    if o.cut.is_some() { return run_cut_bench(o) }
    if o.hover.is_some() { return run_hover_bench(o) }
    if o.seeks.is_some() || o.scrub.is_some() { return run_scrub_bench(o) }
    use min_present::engine::{DisplayMode, Engine, OutMode};
    let n = o.headless.unwrap_or(1);
    let Some(file) = o.file.clone() else { println!("--headless needs a file"); return };
    let mut engine = Engine::headless(wgpu::TextureFormat::Rgba16Float).expect("headless engine");
    engine.set_display(DisplayMode { out: OutMode::Linear, white: o.white });
    if o.identity_chain { engine.set_color_pass(Some(Box::new(min_present::IdentityColorPass))); }
    #[cfg(windows)] let dev = engine.dx12_device_raw();
    #[cfg(not(windows))] let dev = None;
    let hw = if o.sw { None } else { HwDevice::platform(dev) };
    println!("headless engine: zero-copy {} · decode {}", engine.zero_copy_available(), hw.as_ref().map(|h| h.name()).unwrap_or("software"));
    let src = Source::from_path(&file, None);
    let t = Instant::now();
    let mut p = match Transport::open(&src, TransportOptions { hw, threads: o.threads, loop_enabled: false, scrub: None, ..Default::default() }) { Ok(p) => p, Err(e) => { println!("open {file}: {e}"); return } };
    println!("opened {} — {}  (open took {:.0} ms)", src.display(), describe(&p), t.elapsed().as_secs_f32() * 1000.0);
    p.play();
    let mut first: Option<f32> = None; let mut show_ms = vec![];
    let deadline = Instant::now() + std::time::Duration::from_secs(60);
    while p.current().map(|h| h.frame_no + 1).unwrap_or(0) < n as i32 && !p.at_end() && Instant::now() < deadline {
        let now = Instant::now();
        let Some(d) = p.tick(now) else { p.idle_refresh(); std::thread::sleep(std::time::Duration::from_micros(500)); continue };
        match engine.show(&d.frame) { Ok(()) => { show_ms.push(engine.last_show_ms); if first.is_none() { first = Some(t.elapsed().as_secs_f32() * 1000.0); } } Err(e) => { println!("show: {e}"); return } }
        let shown = d.frame_no as u32;
        if let Some((path, k)) = &o.snap_out { if shown == *k { match engine.snapshot(Snapshot::Encoded) { Ok((px, w, h)) => { write_png_rgb8(path, &px, w, h); println!("snap-out written: {path} ({w}x{h}, display-encoded 8-bit, frame {k})"); } Err(e) => println!("snap-out: {e}") } } }
        if o.verify_pq && shown == 24 {
            let Ok((px, w, h)) = engine.snapshot(Snapshot::Linear) else { continue };
            let pq = |c: f64| { let (m1, m2, c1, c2, c3) = (0.1593017578125, 78.84375, 0.8359375, 18.8515625, 18.6875); let e = c.powf(1.0 / m2); ((e - c1).max(0.0) / (c2 - c3 * e)).powf(1.0 / m1) * 10000.0 };
            let white = o.white as f64; let row = (h / 4) as usize;
            let (mut worst, mut worst_x, mut over1) = (0f64, 0usize, 0u32);
            for x in 0..w as usize { let code = x as f64 / (w - 1) as f64; let exp = pq(code) / white; let got = px[(row * w as usize + x) * 4] as f64; if got > 1.0 { over1 += 1 } if exp > 0.001 { let rel = ((got - exp) / exp).abs(); if rel > worst { worst = rel; worst_x = x; } } }
            let bands: Vec<String> = [0.5081f64, 0.7518, 0.9026].iter().enumerate().map(|(i, code)| { let y = (h / 2 + (h / 6) * i as u32 + h / 12) as usize; let got = px[(y * w as usize + w as usize / 2) * 4] as f64; format!("band {code:.4} → expect {:.4} ({:.0} nit) got {:.4} ({:.0} nit)", pq(*code) / white, pq(*code), got, got * white) }).collect();
            println!("verify-pq (row {row}, {} nits = 1.0): worst relative error {:.3} % at x={worst_x} (code {:.3}); {} of {} samples above 1.0 (EDR range)\n  {}", o.white, worst * 100.0, worst_x as f64 / (w - 1) as f64, over1, w, bands.join("\n  "));
        }
    }
    // the FrameIndex report: tier 2 vs tier 1 on every frame, the keyframe table, the read-ahead pages touched
    if let Some(idx) = p.decoder().frame_index() {
        let t1 = Instant::now(); while idx.scan_state() == qcv_player::frame_index::ScanState::Building && t1.elapsed().as_secs() < 30 { std::thread::sleep(std::time::Duration::from_millis(20)); }
        let n = idx.total_frames();
        let (mut mism, mut worst) = (0, 0i64);
        if idx.has_exact_mapping() {
            let f1 = qcv_player::FrameIndex::new(idx.time_base().0, idx.time_base().1, idx.frame_rate().0, idx.frame_rate().1, n, idx.is_intra_only());
            for k in 0..n { let d = (idx.pts_for_frame(k) - f1.pts_for_frame(k)).abs(); if d != 0 { mism += 1; worst = worst.max(d); } }
        }
        println!("frame index: tier 2 {:?} ({} frames built), keyframes {}, tier-1 formula disagrees on {mism} of {n} frames (worst {worst} ticks); keyframe before frame {} = pts {}; read-ahead pages touched {}",
            idx.scan_state(), idx.scan_frames_built(), idx.keyframe_count(), n / 2, idx.keyframe_pts_before(n / 2), qcv_player::read_ahead::ReadAhead::instance().touched_pages.load(std::sync::atomic::Ordering::Relaxed));
    }
    let s = &mut p.stats;
    println!("headless: {} frames shown (open → first frame on the engine {:.0} ms), show ms p50 {:.2} max {:.2}, decode ms p50 {:.1} max {:.1}, dropped {}; CPU {:.0} % of one core; max RSS {:.0} MB",
        s.shown, first.unwrap_or(f32::NAN), pct(&mut show_ms, 50.0), pct(&mut show_ms, 100.0), pct(&mut s.decode_ms, 50.0), pct(&mut s.decode_ms, 100.0), s.dropped, 100.0 * qcv_player::process_cpu_seconds() / t.elapsed().as_secs_f64(), qcv_player::max_rss_mb());
}

/// `--seeks N` / `--scrub a-b:ms` (S2's battery, now against the PORTED scrub decoder): the streaming decoder opens the
/// file and pauses; the scrub decoder opens its own session and publishes into the stream's slot; the headless engine
/// shows what lands. Seeks: N random targets, one at a time (request → served → shown). Scrub: a pointer sweeping a → b,
/// one request per refresh interval for 2 s, the slot drained each refresh (request → ready, hits, frames behind the
/// pointer). `--scrub-hw` lifts the intra-only hwaccel gate (QCView's `performance/hwScrubInterCodecs`; the default on Windows).
fn run_scrub_bench(o: Opts) {
    use min_present::engine::{DisplayMode, Engine, OutMode};
    use qcv_player::ScrubOptions;
    let Some(file) = o.file.clone() else { println!("--seeks / --scrub need a file"); return };
    let mut engine = Engine::headless(wgpu::TextureFormat::Rgba16Float).expect("headless engine");
    engine.set_display(DisplayMode { out: OutMode::Linear, white: o.white });
    #[cfg(windows)] let dev = engine.dx12_device_raw();
    #[cfg(not(windows))] let dev = None;
    let hw = if o.sw { None } else { HwDevice::platform(dev) };
    let src = Source::from_path(&file, None);
    let t_open = Instant::now();
    let sopts = ScrubOptions { hw: hw.clone(), hw_inter_scrub: o.scrub_hw || cfg!(windows), stats: true, threads: std::env::var("QCV_SCRUB_THREADS").ok().and_then(|v| v.parse().ok()).unwrap_or(0), budget_bytes: std::env::var("QCV_SCRUB_CACHE_MB").ok().and_then(|v| v.parse::<usize>().ok()).unwrap_or(512) << 20, ..Default::default() };
    let mut p = match Transport::open(&src, TransportOptions { hw: hw.clone(), threads: o.threads, loop_enabled: false, scrub: Some(sopts), ..Default::default() }) { Ok(p) => p, Err(e) => { println!("open {file}: {e}"); return } };
    let index = p.frame_index().clone();
    let total = index.total_frames();
    let (hw_active, intra_direct, served) = match p.scrub() { Some(sd) => (sd.hw_active, sd.intra_direct, sd.served.clone().unwrap()), None => { println!("no scrub decoder"); return } };
    println!("opened {} — {}; scrub decoder: {} (intra-direct {}), transport opened in {:.0} ms", src.display(), describe(&p), if hw_active { hw.as_ref().map(|h| h.name()).unwrap_or("hw") } else { "software" }, intra_direct, t_open.elapsed().as_secs_f32() * 1000.0);
    // the open's own first frame lands through the transport's seek gate (paused at open)
    { let t = Instant::now(); while p.seek_in_flight() && t.elapsed().as_secs() < 10 { if let Some(h) = p.tick(Instant::now()) { engine.show(&h.frame).ok(); } else { std::thread::sleep(std::time::Duration::from_micros(200)); } } }
    if let Some(ms) = std::env::var("QCV_SCRUB_SETTLE_MS").ok().and_then(|v| v.parse::<u64>().ok()) { std::thread::sleep(std::time::Duration::from_millis(ms)); }   // measurement: let the index scan / warmer finish first
    // the first frame through the scrub decoder (the H.264 open carried from step 1)
    let t0 = Instant::now(); p.scrub().unwrap().request_frame(0);
    match served.recv_timeout(std::time::Duration::from_secs(10)) { Ok(s) => { let h = p.decoder().fetch_latest(); if let Some(h) = &h { engine.show(&h.frame).ok(); } println!("first frame via the scrub decoder: ready {:.1} ms, shown {:.1} ms (decoded {}, seeked {})", (s.done - s.issued).as_secs_f32() * 1000.0, t0.elapsed().as_secs_f32() * 1000.0, s.decoded, s.seeked); } Err(_) => { println!("first frame: no answer in 10 s"); return } }
    let mut rng = 0x9E3779B97F4A7C15u64; let mut next = || { rng ^= rng << 13; rng ^= rng >> 7; rng ^= rng << 17; rng };
    if let Some(n) = o.seeks {
        let (mut ready, mut shown, mut fwd, mut back, mut hits) = (vec![], vec![], vec![], vec![], 0u32);
        let mut last = 0i32;
        for _ in 0..n {
            let target = (next() % total.max(1) as u64) as i32;
            if target == last { continue }
            if o.seek_gap_ms > 0 { std::thread::sleep(std::time::Duration::from_millis(o.seek_gap_ms)); }   // a user's seeks are spaced; the decoder's in-flight frame threads drain between them
            let t = Instant::now();
            p.seek_to_frame(target);                            // the stream's own seek (QCView's seekToFrame)
            p.scrub().unwrap().request_frame(target);    // the scrub decoder answers first
            let Ok(s) = served.recv_timeout(std::time::Duration::from_secs(10)) else { println!("seek {target}: no answer in 10 s"); break };
            let r = (s.done - s.issued).as_secs_f32() * 1000.0;
            let mut sh = f32::NAN;
            while p.seek_in_flight() && t.elapsed().as_secs() < 10 { if let Some(h) = p.tick(Instant::now()) { engine.show(&h.frame).ok(); if h.frame_no < 0 && sh.is_nan() { sh = t.elapsed().as_secs_f32() * 1000.0; } } else { std::thread::sleep(std::time::Duration::from_micros(200)); } }
            if let Some(h) = p.current() { if h.frame_no >= 0 && h.frame_no != target { println!("  MISMATCH: asked {target} got {}", h.frame_no); } }
            if sh.is_nan() { sh = t.elapsed().as_secs_f32() * 1000.0; }
            if std::env::var_os("QCV_SCRUB_ROWS").is_some() { println!("  seek → {target}: ready {r:.1} ms, decoded {} (keyframe before = frame {}), seeked {}, hit {}", s.decoded, index.frame_for_pts(index.keyframe_pts_before(target)), s.seeked, s.hit); }
            ready.push(r); shown.push(sh); if s.hit { hits += 1 } if target > last { fwd.push(r) } else { back.push(r) }
            last = target;
        }
        let land = &mut p.stats.seek_land_ms;
        println!("seeks ({} random, {} frames): scrub request → ready ms p50 {:.1} p95 {:.1} max {:.1}; → shown p50 {:.1} p95 {:.1}; forward p50 {:.1} p95 {:.1} (n={}), backward p50 {:.1} p95 {:.1} (n={}); cache hits {hits}; the stream's own seek landed ms p50 {:.1} p95 {:.1} max {:.1}",
            ready.len(), total, pct(&mut ready, 50.0), pct(&mut ready, 95.0), pct(&mut ready, 100.0), pct(&mut shown, 50.0), pct(&mut shown, 95.0), pct(&mut fwd, 50.0), pct(&mut fwd, 95.0), fwd.len(), pct(&mut back, 50.0), pct(&mut back, 95.0), back.len(), pct(land, 50.0), pct(land, 95.0), pct(land, 100.0));
    }
    if let Some((a, b, ms)) = o.scrub {
        let (a, b) = (a.clamp(0, total - 1), b.clamp(0, total - 1));
        if (b - a).abs() < 10 { println!("scrub: the clip has {total} frames — the sweep {a}→{b} is too short to measure"); }
        // position on the sweep's start first (a user's drag begins where the playhead is); the sweep measures the drag, not the jump to it
        p.begin_scrub(); p.scrub_to_frame(a); while let Ok(s) = served.recv_timeout(std::time::Duration::from_secs(10)) { if s.target == a { break } }
        if let Some(h) = p.tick(Instant::now()) { engine.show(&h.frame).ok(); }
        let period = std::time::Duration::from_secs_f32(ms / 1000.0);
        let sweep = std::time::Duration::from_secs(2); let t = Instant::now();
        let (mut ready, mut behind, mut reqs, mut hits, mut shown_n, mut retargets) = (vec![], vec![], 0u32, 0u32, 0u32, 0u32);
        let mut tick = t;
        while t.elapsed() < sweep {
            let frac = (t.elapsed().as_secs_f32() / sweep.as_secs_f32()).min(1.0);
            let pointer = a + ((b - a) as f32 * frac).round() as i32;
            p.scrub_to_frame(pointer); reqs += 1;
            tick += period; let now = Instant::now(); if tick > now { std::thread::sleep(tick - now); }
            while let Ok(s) = served.try_recv() { ready.push((s.done - s.issued).as_secs_f32() * 1000.0); if s.hit { hits += 1 } if s.retargeted { retargets += 1 } }
            if let Some(h) = p.tick(Instant::now()) { engine.show(&h.frame).ok(); shown_n += 1; }
            behind.push((pointer - p.scrub().unwrap().last_shown()).abs() as f32);
        }
        std::thread::sleep(std::time::Duration::from_millis(200));
        while let Ok(s) = served.try_recv() { ready.push((s.done - s.issued).as_secs_f32() * 1000.0); if s.hit { hits += 1 } }
        println!("scrub {a}→{b} @ {ms:.1} ms: {reqs} requests → {} served ({hits} cache hits, {retargets} retargets), {shown_n} shown; request → ready ms p50 {:.1} p95 {:.1} max {:.1}; frames behind the pointer p50 {:.0} p95 {:.0} max {:.0}",
            ready.len(), pct(&mut ready, 50.0), pct(&mut ready, 95.0), pct(&mut ready, 100.0), pct(&mut behind, 50.0), pct(&mut behind, 95.0), pct(&mut behind, 100.0));
    }
    println!("CPU {:.0} % of one core over the run; max RSS {:.0} MB", 100.0 * qcv_player::process_cpu_seconds() / t_open.elapsed().as_secs_f64(), qcv_player::max_rss_mb());
    p.end_scrub();
}

/// `--hover N`: the hover-thumbnail cache (QCView's `TimelineThumbnailCache` + `VideoImageLoader`) — N random hover frames
/// at the panel's 16 ms request throttle, the drain every 33 ms (textures made on the headless engine); then the same
/// frames again (hits), then a prefetch of the whole file at stride 6. The bar: a hover answers within one refresh once
/// cached, and a miss lands in the time one thumbnail decode takes.
fn cache_size_for(_c: &qcv_player::TimelineThumbnailCache, qcview: bool) -> usize { if qcview { qcv_player::thumbnail_cache::QCVIEW_CACHE_SIZE } else { qcv_player::ThumbConfig::default().cache_size } }
fn run_hover_bench(o: Opts) {
    use min_present::engine::Engine;
    use qcv_player::{TimelineThumbnailCache, ThumbConfig, VideoThumbLoader, thumbnail_cache::{HOVER_REQUEST_THROTTLE_MS, UPLOAD_DRAIN_MS}};
    let n = o.hover.unwrap_or(20);
    let Some(file) = o.file.clone() else { println!("--hover needs a file"); return };
    let engine = Engine::headless(wgpu::TextureFormat::Rgba16Float).expect("headless engine");
    let src = Source::from_path(&file, None);
    let mut p = match Transport::open(&src, TransportOptions { hw: None, threads: o.threads, loop_enabled: false, ..Default::default() }) { Ok(p) => p, Err(e) => { println!("open {file}: {e}"); return } };
    let total = p.frame_count();
    let qcview_budget = std::env::var_os("QCV_HOVER_QCVIEW").is_some();   // QCView's 100-entry budget, no strip, no peek: the comparison run
    println!("opened {} — {}", src.display(), describe(&p));
    let textures: std::rc::Rc<std::cell::RefCell<std::collections::HashMap<u64, wgpu::Texture>>> = Default::default();
    let released = std::rc::Rc::new(std::cell::Cell::new(0u32));
    let (tx, rl) = (textures.clone(), released.clone());
    let mut cache = TimelineThumbnailCache::new(ThumbConfig { cache_size: if qcview_budget { qcv_player::thumbnail_cache::QCVIEW_CACHE_SIZE } else { ThumbConfig::default().cache_size }, ..Default::default() }, VideoThumbLoader::factory(), Box::new(move |h| { tx.borrow_mut().remove(&h); rl.set(rl.get() + 1); }));
    let mut next_handle = 0u64;
    let mut adopt = |key: &qcv_player::ThumbKey, px: &qcv_player::ThumbPixels| -> u64 {
        let _ = key;
        let tex = engine.device.create_texture(&wgpu::TextureDescriptor { label: Some("thumb"), size: wgpu::Extent3d { width: px.width, height: px.height, depth_or_array_layers: 1 }, mip_level_count: 1, sample_count: 1, dimension: wgpu::TextureDimension::D2, format: wgpu::TextureFormat::Rgba8Unorm, usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST, view_formats: &[] });
        engine.queue.write_texture(wgpu::TexelCopyTextureInfo { texture: &tex, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All }, &px.rgba, wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(px.width * 4), rows_per_image: Some(px.height) }, wgpu::Extent3d { width: px.width, height: px.height, depth_or_array_layers: 1 });
        engine.queue.submit(std::iter::empty()); let _ = engine.device.poll(wgpu::PollType::Poll);
        next_handle += 1; textures.borrow_mut().insert(next_handle, tex); next_handle
    };
    let mut rng = 0x9E3779B97F4A7C15u64; let mut next = || { rng ^= rng << 13; rng ^= rng >> 7; rng ^= rng << 17; rng };
    let frames: Vec<i32> = (0..n).map(|_| (next() % total.max(1) as u64) as i32).collect();
    let (mut land_ms, mut sizes) = (vec![], (0u32, 0u32));
    let t_all = Instant::now();
    for &f in &frames {
        let t = Instant::now();
        let e = cache.get_thumbnail(&file, f, true);
        if e.texture != 0 && cache.stats().total_cached > 0 && land_ms.is_empty() { /* a fallback answered */ }
        let mut landed = e.texture != 0 && false;
        let mut next_drain = t;
        while !landed && t.elapsed().as_secs() < 10 {
            if Instant::now() >= next_drain { cache.process_pending_uploads(&mut adopt); next_drain += std::time::Duration::from_millis(UPLOAD_DRAIN_MS); }
            let e = cache.get_thumbnail(&file, f, false);
            if e.texture != 0 { landed = true; sizes = (e.width, e.height); land_ms.push(t.elapsed().as_secs_f32() * 1000.0); }
            else { std::thread::sleep(std::time::Duration::from_millis(1)); }
        }
        std::thread::sleep(std::time::Duration::from_millis(HOVER_REQUEST_THROTTLE_MS));
    }
    let fill = t_all.elapsed().as_secs_f32();
    let mut hit_ms = vec![];
    for &f in &frames { let t = Instant::now(); let e = cache.get_thumbnail(&file, f, true); hit_ms.push(t.elapsed().as_secs_f32() * 1000.0); if e.texture == 0 { println!("  miss on a re-hover of frame {f}"); } }
    let s = cache.stats();
    println!("hover: {} random frames: miss → thumbnail landed ms p50 {:.1} p95 {:.1} max {:.1} (thumb {}x{}); re-hover hit ms p50 {:.3} max {:.3}; cached {} of {}; hits {} misses {}; the walk took {:.1} s",
        frames.len(), pct(&mut land_ms, 50.0), pct(&mut land_ms, 95.0), pct(&mut land_ms, 100.0), sizes.0, sizes.1, pct(&mut hit_ms, 50.0), pct(&mut hit_ms, 100.0), s.total_cached, cache_size_for(&cache, qcview_budget), s.cache_hits, s.cache_misses, fill);
    // the strip on open (frames / 400, floor 6), then a MOUSE SWEEP: the hover crosses the whole timeline in 2 s at the
    // 16 ms throttle; every position asks for its thumbnail through the Transport (GOP-cache peek → cache → nearest cached)
    cache.clear();
    let t = Instant::now();
    if !qcview_budget { cache.precache_strip(&file, total, 0); }
    let wanted = if qcview_budget { 0 } else { ((total as i64 + (total as i64 / 400).max(6) - 1) / (total as i64 / 400).max(6)) as usize };
    let mut landed = 0usize;
    while !qcview_budget && cache.stats().pending_requests > 0 && t.elapsed().as_secs() < 180 { landed += cache.process_pending_uploads(&mut adopt); std::thread::sleep(std::time::Duration::from_millis(UPLOAD_DRAIN_MS)); }
    landed += cache.process_pending_uploads(&mut adopt);
    println!("strip: {landed} of {wanted} thumbnails in {:.1} s ({:.0} ms each across 3 workers); cache holds {} of {}", t.elapsed().as_secs_f32(), t.elapsed().as_secs_f32() * 1000.0 / landed.max(1) as f32, cache.stats().total_cached, cache_size_for(&cache, qcview_budget));
    let _ = &p;
    let (mut answered, mut exact, mut nothing, mut off_by, mut last_f) = (0u32, 0u32, 0u32, vec![], 0i32);
    let t = Instant::now(); let sweep = std::time::Duration::from_secs(2); let mut next_drain = t;
    while t.elapsed() < sweep {
        let frac = t.elapsed().as_secs_f32() / sweep.as_secs_f32(); let f = ((total - 1) as f32 * frac) as i32; last_f = f;
        if Instant::now() >= next_drain { cache.process_pending_uploads(&mut adopt); next_drain += std::time::Duration::from_millis(UPLOAD_DRAIN_MS); }
        let (e, peeked) = p.hover_thumbnail(&mut cache, f, 320);
        if peeked { cache.process_pending_uploads(&mut adopt); }
        let (e, _) = if peeked { p.hover_thumbnail(&mut cache, f, 320) } else { (e, false) };
        if e.texture != 0 { answered += 1; if cache.contains(&file, f) { exact += 1; } else { off_by.push(cache.nearest_cached_distance(&file, f).unwrap_or(0) as f32); } } else { nothing += 1; }
        std::thread::sleep(std::time::Duration::from_millis(HOVER_REQUEST_THROTTLE_MS));
    }
    // the mouse rests on the last position: how long until the exact frame lands?
    let t2 = Instant::now(); let mut rest_ms = f32::NAN;
    while t2.elapsed().as_secs() < 10 { cache.process_pending_uploads(&mut adopt); if cache.contains(&file, last_f) { rest_ms = t2.elapsed().as_secs_f32() * 1000.0; break } std::thread::sleep(std::time::Duration::from_millis(1)); }
    let s = cache.stats();
    println!("sweep: {} hovers over 2 s — answered with a picture {answered} (exact {exact}, nearest cached {} at p50 {:.0} / p95 {:.0} / max {:.0} frames away), nothing {nothing}; the exact frame landed {rest_ms:.0} ms after the mouse rested; backlog left {} requests; cache holds {} textures",
        answered + nothing, answered - exact, pct(&mut off_by, 50.0), pct(&mut off_by, 95.0), pct(&mut off_by, 100.0), s.pending_requests, s.total_cached);
    // a drag across frames 100–190 (the scrub decoder's GOP cache fills along it on inter codecs), then hovers over the same
    // span: the GOP-cache peek answers exactly and at once where the cache holds the frame
    if total > 200 {
        p.begin_scrub(); let t = Instant::now(); let served = p.scrub().and_then(|s| s.served.clone());
        for f in (100..190).step_by(2) { p.scrub_to_frame(f); if let Some(r) = &served { let _ = r.recv_timeout(std::time::Duration::from_millis(500)); } let _ = p.tick(Instant::now()); }
        p.end_scrub(); while p.seek_in_flight() && t.elapsed().as_secs() < 5 { let _ = p.tick(Instant::now()); std::thread::sleep(std::time::Duration::from_micros(200)); }
        let (mut peek_ms, mut exact_peek, mut via_loader) = (vec![], 0u32, 0u32);
        for f in (101..189).step_by(3) {
            let t = Instant::now(); let (e, peeked) = p.hover_thumbnail(&mut cache, f, 320);
            if peeked { cache.process_pending_uploads(&mut adopt); let (e2, _) = p.hover_thumbnail(&mut cache, f, 320); if e2.texture != 0 && cache.contains(&file, f) { exact_peek += 1; peek_ms.push(t.elapsed().as_secs_f32() * 1000.0); } }
            else { via_loader += 1; let _ = e; }
        }
        println!("after a drag over 100–190: {} hovers on that span — exact through the GOP-cache peek {exact_peek} (p50 {:.1} ms, max {:.1}), to the loader {via_loader} ({})", exact_peek + via_loader, pct(&mut peek_ms, 50.0), pct(&mut peek_ms, 100.0), if p.scrub().map(|s| s.intra_direct).unwrap_or(true) { "an intra codec: the scrub decoder caches nothing, by design" } else { "frames the GOP cache did not hold" });
    }
    // playing: one worker
    cache.notify_playback_state(true);
    let t = Instant::now(); cache.precache_range(&file, 0, (total - 1).min(120), 0, 1);
    while cache.stats().pending_requests > 0 && t.elapsed().as_secs() < 60 { cache.process_pending_uploads(&mut adopt); std::thread::sleep(std::time::Duration::from_millis(UPLOAD_DRAIN_MS)); }
    println!("playing (one worker): 121 frames at stride 1 in {:.1} s; peak RSS {:.0} MB", t.elapsed().as_secs_f32(), qcv_player::max_rss_mb());
    cache.shutdown();
}

/// `--cut a.mov@0-48,b.mp4@100-148[,…]`: a playlist through the Transport's two slots, played headless at a 60 Hz refresh
/// cadence from the start to the end; the cut's evidence is the cadence / black / drop accounting around every
/// boundary (S2's bar: no black refresh, no duplicate frame at the cut).
fn run_cut_bench(o: Opts) {
    use min_present::engine::Engine;
    let spec = o.cut.clone().unwrap();
    let mut specs = vec![];
    for part in spec.split(',') {
        let (path, trim) = match part.rsplit_once('@') { Some((p, r)) if r.contains('-') => { let (a, b) = r.split_once('-').unwrap(); (p.to_string(), Some((a.parse::<i64>().unwrap_or(0), b.parse::<i64>().unwrap_or(0)))) } _ => (part.to_string(), None) };
        specs.push((path, trim));
    }
    let mut engine = Engine::headless(wgpu::TextureFormat::Rgba16Float).expect("headless engine");
    #[cfg(windows)] let dev = engine.dx12_device_raw();
    #[cfg(not(windows))] let dev = None;
    let hw = if o.sw { None } else { HwDevice::platform(dev) };
    let tl = match qcv_player::transport::playlist_from_specs(&specs) { Ok(t) => t, Err(e) => { println!("cut: {e}"); return } };
    let boundaries = tl.boundaries(); let total = tl.duration;
    let t0 = Instant::now();
    let mut p = match Transport::open_playlist(tl, TransportOptions { hw, threads: o.threads, loop_enabled: o.looping, scrub: None, ..Default::default() }) { Ok(p) => p, Err(e) => { println!("open playlist: {e}"); return } };
    println!("playlist: {} clips, {} timeline frames at {} fps, boundaries at {:?}; opened in {:.0} ms", specs.len(), total, p.rate(), boundaries, t0.elapsed().as_secs_f32() * 1000.0);
    // let the first frame land, then play at a 60 Hz refresh cadence
    { let t = Instant::now(); while p.seek_in_flight() && t.elapsed().as_secs() < 10 { if let Some(h) = p.tick(Instant::now()) { engine.show(&h.frame).ok(); } else { std::thread::sleep(std::time::Duration::from_micros(200)); } } }
    p.play();
    let period = std::time::Duration::from_secs_f32(o.refresh_ms / 1000.0);
    let mut next = Instant::now(); let mut shown_at: Vec<(i64, u32)> = vec![];   // (timeline frame shown, refresh index)
    let mut refresh = 0u32; let deadline = Instant::now() + std::time::Duration::from_secs(120);
    let loops_wanted = if o.looping { 1 } else { 0 };
    let (mut last_loops, mut wrap_at, mut wraps) = (0u32, None::<(u32, i64)>, vec![]);
    while !p.at_end() && Instant::now() < deadline && p.stats.loops <= loops_wanted {
        let now = Instant::now();
        match p.tick(now) { Some(h) => { engine.show(&h.frame).ok(); shown_at.push((p.current_frame() as i64, refresh)); if let Some((r0, f0)) = wrap_at.take() { wraps.push(format!("wrap {}: frame {f0} at refresh {r0} → frame {} at refresh {refresh} ({} refreshes; the landing {:.1} ms)", p.stats.loops, p.current_frame(), refresh - r0, p.stats.seek_land_ms.last().copied().unwrap_or(f32::NAN))); } } None => p.idle_refresh() }
        if p.stats.loops != last_loops { last_loops = p.stats.loops; wrap_at = Some((refresh, shown_at.last().map(|x| x.0).unwrap_or(-1))); }
        if o.looping && p.stats.loops > loops_wanted { break }
        refresh += 1; next += period; let n2 = Instant::now(); if next > n2 { std::thread::sleep(next - n2); }
    }
    let s = &mut p.stats;
    println!("cut: shown {} frames over {refresh} refreshes, dropped {}, late {}, black refreshes {}, loops {}; lateness ms p50 {:.1} p95 {:.1} max {:.1}; cadence {:?}; swaps prewarmed {} / opened {}; CPU {:.0} %; RSS {:.0} MB",
        s.shown, s.dropped, s.late, s.black, s.loops, pct(&mut s.lateness_ms, 50.0), pct(&mut s.lateness_ms, 95.0), pct(&mut s.lateness_ms, 100.0), s.cadence, s.swaps_prewarmed, s.swaps_opened, 100.0 * qcv_player::process_cpu_seconds() / t0.elapsed().as_secs_f64(), qcv_player::max_rss_mb());
    // around every boundary: the refresh gaps between the frames shown just before and after the cut
    for w in &wraps { println!("  {w}"); }
    // random seeks across the playlist through the 50 ms debounce (applyPlaylistSeek: clip resolve + swap + the stream's landing)
    if !o.looping && total > 1 {
        p.pause();
        let mut rng = 0x9E3779B97F4A7C15u64; let mut next = || { rng ^= rng << 13; rng ^= rng >> 7; rng ^= rng << 17; rng };
        let (mut land, mut cross) = (vec![], 0u32);
        for _ in 0..12 {
            let target = (next() % total as u64) as i32; let from_clip = p.active_clip_index();
            let t = Instant::now(); p.seek_to_frame(target);
            while (p.seek_in_flight() || t.elapsed().as_millis() < 60) && t.elapsed().as_secs() < 10 { if let Some(h) = p.tick(Instant::now()) { engine.show(&h.frame).ok(); } else { std::thread::sleep(std::time::Duration::from_micros(200)); } }
            let landed = t.elapsed().as_secs_f32() * 1000.0; land.push(landed); if p.active_clip_index() != from_clip { cross += 1; }
            if p.current_frame() != target { println!("  seek → {target}: landed on timeline frame {} (clip {})", p.current_frame(), p.active_clip_index()); }
        }
        let s = &p.stats;
        println!("playlist seeks: 12 random, {cross} cross-clip; request → landed ms (incl. the 50 ms debounce) p50 {:.1} p95 {:.1} max {:.1}; swaps prewarmed {} / opened {}", pct(&mut land, 50.0), pct(&mut land, 95.0), pct(&mut land, 100.0), s.swaps_prewarmed, s.swaps_opened);
    }
    for b in &boundaries {
        let after = shown_at.iter().find(|(f, _)| *f >= *b); let before = after.and_then(|(_, ra)| shown_at.iter().rev().find(|(f, r)| *f < *b && r < ra));
        match (before, after) { (Some((fb, rb)), Some((fa, ra))) => println!("  boundary {b}: frame {fb} at refresh {rb} → frame {fa} at refresh {ra} ({} refreshes apart; a 2/3 cadence = gapless, no duplicate)", ra - rb), _ => println!("  boundary {b}: not crossed") }
    }
}
