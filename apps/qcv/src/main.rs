//! `qcv` — Phase 2 step 1: open a file (or an image-sequence frame) → the fused presenter at the display's cadence.
//! `qcv <file> [--sw] [--out auto|sdr|edr|pq] [--white N] [--seconds N] [--stats] [--snap-out path@N] [--verify-pq]
//!      [--identity-chain] [--refresh ms] [--screen i]`. Space = play / pause · F = fullscreen · Esc = quit · drop a file to open it.
use winit31 as winit;
use min_present::{Output, Presenter, Snapshot};
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

struct Opts { cut: Option<String>, hover: Option<u32>, seek_gap_ms: u64, seeks: Option<u32>, scrub: Option<(i32, i32, f32)>, scrub_hw: bool, headless: Option<u32>, looping: bool, file: Option<String>, sw: bool, out: Output, white: f32, seconds: f32, stats: bool, snap_out: Option<(String, u32)>, verify_pq: bool, identity_chain: bool, refresh_ms: f32, screen: Option<usize>, threads: i32 }

struct Gpu { window: Arc<Box<dyn Window>>, presenter: Presenter }

struct App {
    o: Opts, gpu: Option<Gpu>, player: Option<Transport>, hw: Option<HwDevice>, source_label: String, source_path: String,
    t0: Option<Instant>, t_open: Option<Instant>, first_presented: bool,
    presents: Vec<f32>, last_present: Option<Instant>, show_ms: Vec<f32>, last_report: Instant, shown_in_window: u32,
    fetches: HashMap<AsyncRequestSerial, WindowId>, mods: qcv_player::Mods, finished: bool, frames_presented: u64, edr_seen: f64, snapped: bool, verified: bool,
}

impl App {
    fn open(&mut self, path: &str) {
        let Some(g) = self.gpu.as_mut() else { return };
        let src = Source::from_path(path, None);
        let t = Instant::now();
        let hw = if self.o.sw { None } else { self.hw.clone() };
        match Transport::open(&src, TransportOptions { hw, threads: self.o.threads, loop_enabled: self.o.looping, refresh_ms: self.o.refresh_ms, ..Default::default() }) {
            Ok(mut p) => {
                p.play();   // the harness plays on open (QCView opens paused; the shell decides)
                println!("opened {} — {}  (open took {:.0} ms)", src.display(), describe(&p), t.elapsed().as_secs_f32() * 1000.0);
                self.source_label = src.display(); self.source_path = path.to_string(); self.player = Some(p); self.t_open = Some(t); self.first_presented = false;
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
            println!("{:>5.1} s  shown {:>5.1} fps  show ms p50 {:.2} max {:.2}  present ms p50 {:.1} p95 {:.1} max {:.1}  lateness p95 {:.1}  dropped {dropped} late {late}  EDR now {:?}",
                self.t0.map(|t| t.elapsed().as_secs_f32()).unwrap_or(0.0), self.shown_in_window as f32 / dt, pct(&mut self.show_ms, 50.0), pct(&mut self.show_ms, 100.0), pct(&mut self.presents, 50.0), pct(&mut self.presents, 95.0), pct(&mut self.presents, 100.0), lat_p95, edr);
            self.shown_in_window = 0; self.show_ms.clear(); self.presents.clear(); self.last_report = now;
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
    fn finish(&mut self) {
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
        self.gpu = Some(Gpu { window, presenter });
        self.t0 = Some(Instant::now()); self.last_report = Instant::now();
        if let Some(f) = self.o.file.clone() { self.open(&f); }
    }
    fn about_to_wait(&mut self, el: &dyn ActiveEventLoop) {
        if self.o.seconds > 0.0 { if let Some(t0) = self.t0 { let dl = t0 + std::time::Duration::from_secs_f32(self.o.seconds); if Instant::now() >= dl { self.finish(); el.exit(); return } el.set_control_flow(ControlFlow::WaitUntil(dl)); } }
    }
    fn window_event(&mut self, el: &dyn ActiveEventLoop, id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => { self.finish(); el.exit(); }
            WindowEvent::Focused(f) => { log::info!("focused {f}"); }
            WindowEvent::SurfaceResized(s) => if let Some(g) = self.gpu.as_mut() { g.presenter.resize(s.width, s.height); g.window.request_redraw(); },
            WindowEvent::RedrawRequested => { self.tick(); if let Some(g) = &self.gpu { g.window.request_redraw(); } }
            WindowEvent::KeyboardInput { event, .. } => {
                let pressed = event.state == ElementState::Pressed;
                if pressed && matches!(event.logical_key, Key::Named(NamedKey::Escape)) { self.finish(); el.exit(); return }
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
                    else if a == Action::ToggleFullscreen { if let Some(g) = &self.gpu { let fs = g.window.fullscreen(); g.window.set_fullscreen(if fs.is_some() { None } else { Some(Fullscreen::Borderless(None)) }); } }
                }
            }
            WindowEvent::ModifiersChanged(m) => { let st = m.state(); self.mods = qcv_player::Mods { shift: st.shift_key(), ctrl: st.control_key() || st.meta_key(), alt: st.alt_key(), keypad: false }; }
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

fn main() {
    env_logger::init();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let arg = |k: &str| args.iter().position(|a| a == k).and_then(|i| args.get(i + 1).cloned());
    let has = |k: &str| args.iter().any(|a| a == k);
    let file = { let mut f = None; let mut skip = false; for (i, a) in args.iter().enumerate() { if skip { skip = false; continue } if a.starts_with("--") { skip = ["--out", "--white", "--seconds", "--snap-out", "--refresh", "--screen", "--threads", "--headless", "--seeks", "--scrub", "--seek-gap", "--hover", "--cut"].contains(&a.as_str()); continue } let _ = i; f = Some(a.clone()); } f };
    let out = match arg("--out").as_deref() { Some("sdr") => Output::Sdr, Some("edr") => Output::Edr, Some("pq") => Output::Pq, _ => Output::Auto };
    let snap_out = arg("--snap-out").map(|s| { let (p, n) = s.rsplit_once('@').unwrap_or((&s, "24")); (p.to_string(), n.parse().unwrap_or(24)) });
    let scrub = arg("--scrub").and_then(|v| { let (r, ms) = v.rsplit_once(':')?; let (a, b) = r.split_once('-')?; Some((a.parse().ok()?, b.parse().ok()?, ms.parse().ok()?)) });
    let o = Opts { cut: arg("--cut"), hover: arg("--hover").and_then(|v| v.parse().ok()), seek_gap_ms: arg("--seek-gap").and_then(|v| v.parse().ok()).unwrap_or(0), seeks: arg("--seeks").and_then(|v| v.parse().ok()), scrub, scrub_hw: has("--scrub-hw"), headless: arg("--headless").and_then(|v| v.parse().ok()), looping: has("--loop"), file, sw: has("--sw"), out, white: arg("--white").and_then(|v| v.parse().ok()).unwrap_or(100.0), seconds: arg("--seconds").and_then(|v| v.parse().ok()).unwrap_or(0.0), stats: has("--stats"), snap_out, verify_pq: has("--verify-pq"), identity_chain: has("--identity-chain"), refresh_ms: arg("--refresh").and_then(|v| v.parse().ok()).unwrap_or(1000.0 / 60.0), screen: arg("--screen").and_then(|v| v.parse().ok()), threads: arg("--threads").and_then(|v| v.parse().ok()).unwrap_or(0) };
    if o.headless.is_some() { run_headless(o); return }
    let el = EventLoop::new().unwrap();
    let app = App { o, gpu: None, player: None, hw: None, source_label: String::new(), source_path: String::new(), t0: None, t_open: None, first_presented: false, presents: vec![], last_present: None, show_ms: vec![], last_report: Instant::now(), shown_in_window: 0, fetches: HashMap::new(), mods: qcv_player::Mods::default(), finished: false, frames_presented: 0, edr_seen: 0.0, snapped: false, verified: false };
    el.run_app(app).unwrap();
}

/// `--headless N`: no window — decode N frames through the player, show each on a headless engine (the same zero-copy
/// import and stage graph), write `--snap-out` / run `--verify-pq`. The evidence that needs no display.
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
fn run_hover_bench(o: Opts) {
    use min_present::engine::Engine;
    use qcv_player::{TimelineThumbnailCache, ThumbConfig, VideoThumbLoader, thumbnail_cache::{HOVER_REQUEST_THROTTLE_MS, UPLOAD_DRAIN_MS, PREFETCH_STRIDE}};
    let n = o.hover.unwrap_or(20);
    let Some(file) = o.file.clone() else { println!("--hover needs a file"); return };
    let engine = Engine::headless(wgpu::TextureFormat::Rgba16Float).expect("headless engine");
    let src = Source::from_path(&file, None);
    let p = match Transport::open(&src, TransportOptions { hw: None, threads: o.threads, loop_enabled: false, scrub: None, ..Default::default() }) { Ok(p) => p, Err(e) => { println!("open {file}: {e}"); return } };
    let total = p.frame_count();
    println!("opened {} — {}", src.display(), describe(&p));
    let textures: std::rc::Rc<std::cell::RefCell<std::collections::HashMap<u64, wgpu::Texture>>> = Default::default();
    let released = std::rc::Rc::new(std::cell::Cell::new(0u32));
    let (tx, rl) = (textures.clone(), released.clone());
    let mut cache = TimelineThumbnailCache::new(ThumbConfig::default(), VideoThumbLoader::factory(), Box::new(move |h| { tx.borrow_mut().remove(&h); rl.set(rl.get() + 1); }));
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
        frames.len(), pct(&mut land_ms, 50.0), pct(&mut land_ms, 95.0), pct(&mut land_ms, 100.0), sizes.0, sizes.1, pct(&mut hit_ms, 50.0), pct(&mut hit_ms, 100.0), s.total_cached, ThumbConfig::default().cache_size, s.cache_hits, s.cache_misses, fill);
    // the whole file at stride 6 (QCView's precacheRange), the priority frame in the middle
    let t = Instant::now(); cache.precache_range(&file, 0, total - 1, total / 2, PREFETCH_STRIDE);
    let wanted = ((total + PREFETCH_STRIDE - 1) / PREFETCH_STRIDE) as usize;
    let mut landed = 0usize;
    while cache.stats().pending_requests > 0 && t.elapsed().as_secs() < 120 { landed += cache.process_pending_uploads(&mut adopt); std::thread::sleep(std::time::Duration::from_millis(UPLOAD_DRAIN_MS)); }
    landed += cache.process_pending_uploads(&mut adopt);
    let s = cache.stats();
    println!("prefetch: {} of {} frames (stride {PREFETCH_STRIDE}) landed in {:.1} s = {:.1} ms per thumbnail across 3 workers; cache holds {} (budget {}), {} textures released; peak RSS {:.0} MB", landed, wanted, t.elapsed().as_secs_f32(), t.elapsed().as_secs_f32() * 1000.0 / landed.max(1) as f32, s.total_cached, ThumbConfig::default().cache_size, released.get(), qcv_player::max_rss_mb());
    // playback: one worker
    cache.notify_playback_state(true);
    let t = Instant::now(); cache.precache_range(&file, 0, (total - 1).min(120), 0, 1);
    while cache.stats().pending_requests > 0 && t.elapsed().as_secs() < 60 { cache.process_pending_uploads(&mut adopt); std::thread::sleep(std::time::Duration::from_millis(UPLOAD_DRAIN_MS)); }
    println!("playing (one worker): 121 frames at stride 1 in {:.1} s", t.elapsed().as_secs_f32());
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
