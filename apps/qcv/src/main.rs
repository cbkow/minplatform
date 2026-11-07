//! `qcv` — Phase 2 step 1: open a file (or an image-sequence frame) → the fused presenter at the display's cadence.
//! `qcv <file> [--sw] [--out auto|sdr|edr|pq] [--white N] [--seconds N] [--stats] [--snap-out path@N] [--verify-pq]
//!      [--identity-chain] [--refresh ms] [--screen i]`. Space = play / pause · F = fullscreen · Esc = quit · drop a file to open it.
use winit31 as winit;
use min_present::{Output, Presenter, Snapshot};
use qcv_player::{HwDevice, Player, Source};
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

struct Opts { headless: Option<u32>, file: Option<String>, sw: bool, out: Output, white: f32, seconds: f32, stats: bool, snap_out: Option<(String, u32)>, verify_pq: bool, identity_chain: bool, refresh_ms: f32, screen: Option<usize>, threads: i32 }

struct Gpu { window: Arc<Box<dyn Window>>, presenter: Presenter }

struct App {
    o: Opts, gpu: Option<Gpu>, player: Option<Player>, hw: Option<HwDevice>, source_label: String, source_path: String,
    t0: Option<Instant>, t_open: Option<Instant>, first_presented: bool,
    presents: Vec<f32>, last_present: Option<Instant>, show_ms: Vec<f32>, last_report: Instant, shown_in_window: u32,
    fetches: HashMap<AsyncRequestSerial, WindowId>, finished: bool, frames_presented: u64, edr_seen: f64,
}

impl App {
    fn open(&mut self, path: &str) {
        let Some(g) = self.gpu.as_mut() else { return };
        let src = Source::from_path(path, None);
        let t = Instant::now();
        let hw = if self.o.sw { None } else { self.hw.clone() };
        match Player::open(&src, hw, self.o.threads) {
            Ok(mut p) => {
                p.refresh_ms = self.o.refresh_ms;
                let i = p.info();
                println!("opened {} — {} {}x{} {} fps ({} frames) {} {:?} {:?} {:?} {}-bit  decode {}  (open took {:.0} ms)", src.display(), i.codec, i.width, i.height, i.rate, i.frames, i.pix_fmt, i.meta.matrix, i.meta.transfer, i.meta.range, i.meta.bit_depth, if i.hw_active { "hw zero-copy" } else { "software + upload" }, t.elapsed().as_secs_f32() * 1000.0);
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
        let shown = self.player.as_ref().map(|p| p.stats.shown).unwrap_or(0);
        let g = self.gpu.as_ref().unwrap();
        if let Some((path, n)) = &self.o.snap_out { if shown == *n { match g.presenter.engine.snapshot(Snapshot::Encoded) { Ok((px, w, h)) => { write_png_rgb8(path, &px, w, h); println!("snap-out written: {path} ({w}x{h}, display-encoded 8-bit, frame {n})"); } Err(e) => println!("snap-out: {e}") } } }
        if self.o.verify_pq && shown == 24 { self.verify_pq(); }
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
            println!("play: shown {} frames, dropped {}, late (> 1 refresh) {}, black refreshes {}, loops {}; lateness ms vs the ideal clock p50 {:.1} p95 {:.1} max {:.1} min {:.1}; cadence (refreshes per frame → count) {:?}; decode ms p50 {:.1} max {:.1}",
                s.shown, s.dropped, s.late, s.black, s.loops, pct(&mut s.lateness_ms, 50.0), pct(&mut s.lateness_ms, 95.0), pct(&mut s.lateness_ms, 100.0), pct(&mut s.lateness_ms, 0.0), s.cadence, pct(&mut s.decode_ms, 50.0), pct(&mut s.decode_ms, 100.0));
        }
        let cpu = qcv_player::process_cpu_seconds();
        if wall > 0.0 { println!("process CPU time {cpu:.2} s over {wall:.1} s = {:.0} % of one core (decode threads included); max RSS {:.0} MB; frames presented {}; EDR headroom seen {:.1}", 100.0 * cpu / wall, qcv_player::max_rss_mb(), self.frames_presented, self.edr_seen); }
        if let Some(g) = self.gpu.as_ref() { let r = g.presenter.layer_report(); if !r.is_empty() { println!("{r}"); } }
    }
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
            WindowEvent::KeyboardInput { event, .. } if event.state == ElementState::Pressed => match &event.logical_key {
                Key::Named(NamedKey::Escape) => { self.finish(); el.exit(); }
                Key::Character(c) if c == " " => { if let Some(p) = self.player.as_mut() { p.toggle(Instant::now()); println!("{}", if p.paused() { "paused" } else { "playing" }); } }
                Key::Character(c) if c == "f" => { if let Some(g) = &self.gpu { let fs = g.window.fullscreen(); g.window.set_fullscreen(if fs.is_some() { None } else { Some(Fullscreen::Borderless(None)) }); } }
                _ => {}
            },
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
    let file = { let mut f = None; let mut skip = false; for (i, a) in args.iter().enumerate() { if skip { skip = false; continue } if a.starts_with("--") { skip = ["--out", "--white", "--seconds", "--snap-out", "--refresh", "--screen", "--threads", "--headless"].contains(&a.as_str()); continue } let _ = i; f = Some(a.clone()); } f };
    let out = match arg("--out").as_deref() { Some("sdr") => Output::Sdr, Some("edr") => Output::Edr, Some("pq") => Output::Pq, _ => Output::Auto };
    let snap_out = arg("--snap-out").map(|s| { let (p, n) = s.rsplit_once('@').unwrap_or((&s, "24")); (p.to_string(), n.parse().unwrap_or(24)) });
    let o = Opts { headless: arg("--headless").and_then(|v| v.parse().ok()), file, sw: has("--sw"), out, white: arg("--white").and_then(|v| v.parse().ok()).unwrap_or(100.0), seconds: arg("--seconds").and_then(|v| v.parse().ok()).unwrap_or(0.0), stats: has("--stats"), snap_out, verify_pq: has("--verify-pq"), identity_chain: has("--identity-chain"), refresh_ms: arg("--refresh").and_then(|v| v.parse().ok()).unwrap_or(1000.0 / 60.0), screen: arg("--screen").and_then(|v| v.parse().ok()), threads: arg("--threads").and_then(|v| v.parse().ok()).unwrap_or(0) };
    if o.headless.is_some() { run_headless(o); return }
    let el = EventLoop::new().unwrap();
    let app = App { o, gpu: None, player: None, hw: None, source_label: String::new(), source_path: String::new(), t0: None, t_open: None, first_presented: false, presents: vec![], last_present: None, show_ms: vec![], last_report: Instant::now(), shown_in_window: 0, fetches: HashMap::new(), finished: false, frames_presented: 0, edr_seen: 0.0 };
    el.run_app(app).unwrap();
}

/// `--headless N`: no window — decode N frames through the player, show each on a headless engine (the same zero-copy
/// import and stage graph), write `--snap-out` / run `--verify-pq`. The evidence that needs no display.
fn run_headless(o: Opts) {
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
    let mut p = match Player::open(&src, hw, o.threads) { Ok(p) => p, Err(e) => { println!("open {file}: {e}"); return } };
    { let i = p.info(); println!("opened {} — {} {}x{} {} fps ({} frames) {} {:?} {:?} {:?} {:?} {}-bit  decode {}  (open took {:.0} ms)", src.display(), i.codec, i.width, i.height, i.rate, i.frames, i.pix_fmt, i.meta.matrix, i.meta.transfer, i.meta.primaries, i.meta.range, i.meta.bit_depth, if i.hw_active { "hw zero-copy" } else { "software + upload" }, t.elapsed().as_secs_f32() * 1000.0); }
    p.looping = false;
    let mut first: Option<f32> = None; let mut show_ms = vec![];
    let deadline = Instant::now() + std::time::Duration::from_secs(60);
    while p.stats.shown < n && Instant::now() < deadline {
        let now = Instant::now();
        let Some(d) = p.tick(now) else { std::thread::sleep(std::time::Duration::from_micros(500)); continue };
        match engine.show(&d.frame) { Ok(()) => { show_ms.push(engine.last_show_ms); if first.is_none() { first = Some(t.elapsed().as_secs_f32() * 1000.0); } } Err(e) => { println!("show: {e}"); return } }
        let shown = p.stats.shown;
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
    let s = &mut p.stats;
    println!("headless: {} frames shown (open → first frame on the engine {:.0} ms), show ms p50 {:.2} max {:.2}, decode ms p50 {:.1} max {:.1}, dropped {}; CPU {:.0} % of one core; max RSS {:.0} MB",
        s.shown, first.unwrap_or(f32::NAN), pct(&mut show_ms, 50.0), pct(&mut show_ms, 100.0), pct(&mut s.decode_ms, 50.0), pct(&mut s.decode_ms, 100.0), s.dropped, 100.0 * qcv_player::process_cpu_seconds() / t.elapsed().as_secs_f64(), qcv_player::max_rss_mb());
}
