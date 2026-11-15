//! S17 - do a virtualised list and a virtualised grid stay smooth with 100,000 entries on min-ui / vello / min-present?
//! (docs/spikes/S17-browser-scale.md). A spike: it dies with its verdict.
//!
//! Driven by script, no hand on the machine:
//!   S17_SCRIPT=bench:<n>                      no window: the model's memory, the sorts, type-ahead, select all, text layout per row
//!   S17_SCRIPT=list:<n>[:fresh|cached]        a window: the list, scrolled by the program
//!   S17_SCRIPT=grid:<n>[:fresh|cached][:img=<k>]   a window: the grid, k distinct placeholder pictures (default 1)
//! S17_NOGPUWAIT=1 leaves the wait for the GPU out of the frame's time (CPU only).
use kurbo::{Affine, Rect};
use min_present::{Output, Presenter};
use min_theme::tokens as t;
use min_ui::{fonts::{draw_layout, TextLayout}, paint, Family, Fonts, ScrollBar, UiLayer};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;
use winit31 as winit;
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::window::{Window, WindowAttributes, WindowId};

// ---------------------------------------------------------------- the model

/// One entry: 24 bytes, its name in a shared arena.
#[repr(C)]
#[derive(Clone, Copy)]
struct Entry { size: u64, mtime: i64, name_off: u32, name_len: u16, kind: u8, flags: u8 }

struct Model { entries: Vec<Entry>, names: String, /// the view's order: indices into `entries`
    order: Vec<u32>, /// one bit per entry
    sel: Vec<u64> }

const KINDS: [(&str, &str); 6] = [("exr", "EXR image"), ("dpx", "DPX image"), ("mov", "QuickTime movie"), ("png", "PNG image"), ("wav", "WAV audio"), ("", "Folder")];
const WORDS: [&str; 8] = ["comp", "plate", "matte", "roto", "Grade", "PREVIZ", "anim", "light"];

impl Model {
    fn generate(n: usize) -> Model {
        let mut seed = 0x9E3779B97F4A7C15u64; let mut rnd = move || { seed ^= seed << 13; seed ^= seed >> 7; seed ^= seed << 17; seed };
        let mut entries = Vec::with_capacity(n); let mut names = String::with_capacity(n * 26);
        use std::fmt::Write;
        for i in 0..n {
            let r = rnd(); let kind = (r % 100) as usize; let kind = if kind < 55 { 0 } else if kind < 70 { 1 } else if kind < 80 { 2 } else if kind < 90 { 3 } else if kind < 97 { 4 } else { 5 };
            let off = names.len(); let shot = (r >> 8) % 4000; let word = WORDS[((r >> 24) % 8) as usize]; let ver = (r >> 32) % 30;
            // numbers without padding, mixed case: what a natural, case-blind order is for
            match (r >> 40) % 4 {
                0 => write!(names, "shot_{shot}_{word}_v{ver:03}").unwrap(),
                1 => write!(names, "Shot_{shot:04}_{word}.{}", 1001 + (i % 5000)).unwrap(),
                2 => write!(names, "SHOT_{shot}_{word}_take{}", (r >> 44) % 200).unwrap(),
                _ => write!(names, "render_{word}_{}_{shot}", (r >> 44) % 120).unwrap(),
            }
            if kind < 5 { names.push('.'); names.push_str(KINDS[kind].0); }
            entries.push(Entry { size: if kind == 5 { 0 } else { (rnd() % (1 << 31)) + 1 }, mtime: 1_700_000_000 + (rnd() % 80_000_000) as i64, name_off: off as u32, name_len: (names.len() - off) as u16, kind: kind as u8, flags: 0 });
        }
        names.shrink_to_fit();
        Model { entries, names, order: (0..n as u32).collect(), sel: vec![0; n.div_ceil(64)] }
    }
    fn name(&self, e: u32) -> &str { let e = &self.entries[e as usize]; &self.names[e.name_off as usize..e.name_off as usize + e.name_len as usize] }
    fn bytes(&self) -> usize { self.entries.len() * std::mem::size_of::<Entry>() + self.names.len() + self.order.len() * 4 + self.sel.len() * 8 }
    fn selected(&self, e: u32) -> bool { self.sel[e as usize / 64] >> (e % 64) & 1 == 1 }
    fn select_all(&mut self) { let n = self.entries.len(); for w in self.sel.iter_mut() { *w = !0; } if n % 64 != 0 { if let Some(l) = self.sel.last_mut() { *l = (1u64 << (n % 64)) - 1; } } }

    /// Natural, case-blind, by the comparator itself.
    fn sort_name_cmp(&mut self) { let mut o = std::mem::take(&mut self.order); o.sort_unstable_by(|a, b| natural_cmp(self.name(*a).as_bytes(), self.name(*b).as_bytes()).then(a.cmp(b))); self.order = o; }
    /// Natural, case-blind, by a key made once per name and compared as bytes.
    fn sort_name_key(&mut self) {
        let mut keys: Vec<u8> = Vec::with_capacity(self.names.len() + self.entries.len() * 4); let mut at: Vec<(u32, u32)> = Vec::with_capacity(self.entries.len());
        for e in 0..self.entries.len() as u32 { let a = keys.len() as u32; natural_key(self.name(e).as_bytes(), &mut keys); at.push((a, keys.len() as u32)); }
        let key = |e: u32| { let (a, b) = at[e as usize]; &keys[a as usize..b as usize] };
        self.order.sort_unstable_by(|a, b| key(*a).cmp(key(*b)).then(a.cmp(b)));
    }
    fn sort_size(&mut self) { let e = &self.entries; self.order.sort_unstable_by_key(|i| (e[*i as usize].size, *i)); }
    fn sort_date(&mut self) { let e = &self.entries; self.order.sort_unstable_by_key(|i| (e[*i as usize].mtime, *i)); }
    /// The first entry, in the view's order, whose name starts with `prefix` (case-blind). A scan: in a natural order the
    /// names with one prefix are not together ("shot_1" holds shot_1, shot_10 - and shot_2 lies between them).
    fn type_ahead(&self, prefix: &str) -> Option<usize> { let p = prefix.as_bytes(); self.order.iter().position(|e| { let n = self.name(*e).as_bytes(); n.len() >= p.len() && n[..p.len()].eq_ignore_ascii_case(p) }) }
}

/// shot_2 before shot_10, case-blind (ASCII folding here; the real one folds Unicode).
fn natural_cmp(a: &[u8], b: &[u8]) -> std::cmp::Ordering {
    use std::cmp::Ordering::*;
    let (mut i, mut j) = (0, 0);
    while i < a.len() && j < b.len() {
        let (x, y) = (a[i], b[j]);
        if x.is_ascii_digit() && y.is_ascii_digit() {
            while i < a.len() && a[i] == b'0' { i += 1; } while j < b.len() && b[j] == b'0' { j += 1; }
            let (si, sj) = (i, j);
            while i < a.len() && a[i].is_ascii_digit() { i += 1; } while j < b.len() && b[j].is_ascii_digit() { j += 1; }
            match (i - si).cmp(&(j - sj)).then_with(|| a[si..i].cmp(&b[sj..j])) { Equal => {} o => return o }
        } else {
            match x.to_ascii_lowercase().cmp(&y.to_ascii_lowercase()) { Equal => { i += 1; j += 1; } o => return o }
        }
    }
    (a.len() - i).cmp(&(b.len() - j))
}
/// The same order as bytes: letters folded, a run of digits as '0', its length, its digits without leading zeros.
fn natural_key(a: &[u8], out: &mut Vec<u8>) {
    let mut i = 0;
    while i < a.len() {
        if a[i].is_ascii_digit() {
            while i < a.len() && a[i] == b'0' { i += 1; } let s = i; while i < a.len() && a[i].is_ascii_digit() { i += 1; }
            out.push(b'0'); out.push((i - s).min(255) as u8); out.extend_from_slice(&a[s..i]);
        } else { out.push(a[i].to_ascii_lowercase()); i += 1; }
    }
}

fn size_text(b: u64) -> String { if b == 0 { "--".into() } else if b < 1 << 10 { format!("{b} B") } else if b < 1 << 20 { format!("{:.1} KB", b as f64 / 1024.0) } else if b < 1 << 30 { format!("{:.1} MB", b as f64 / 1048576.0) } else { format!("{:.2} GB", b as f64 / 1073741824.0) } }
fn date_text(secs: i64) -> String {
    let days = secs.div_euclid(86400); let rem = secs.rem_euclid(86400); let z = days + 719468; let era = z.div_euclid(146097); let doe = z.rem_euclid(146097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365; let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); let mp = (5 * doy + 2) / 153; let d = doy - (153 * mp + 2) / 5 + 1; let m = if mp < 10 { mp + 3 } else { mp - 9 }; let y = yoe + era * 400 + if m <= 2 { 1 } else { 0 };
    format!("{y:04}-{m:02}-{d:02} {:02}:{:02}", rem / 3600, rem % 3600 / 60)
}
fn rss_mb() -> f64 { std::process::Command::new("ps").args(["-o", "rss=", "-p", &std::process::id().to_string()]).output().ok().and_then(|o| String::from_utf8_lossy(&o.stdout).trim().parse::<f64>().ok()).map(|kb| kb / 1024.0).unwrap_or(0.0) }
fn ms(t: Instant) -> f64 { t.elapsed().as_secs_f64() * 1000.0 }
fn best_of<R>(n: usize, mut f: impl FnMut() -> R) -> f64 { (0..n).map(|_| { let t = Instant::now(); std::hint::black_box(f()); ms(t) }).fold(f64::MAX, f64::min) }

// ---------------------------------------------------------------- the bench with no window

fn bench(n: usize) {
    let before = rss_mb(); let t0 = Instant::now(); let mut m = Model::generate(n); let made = ms(t0); let after = rss_mb();
    println!("model n={n}: {:.2} MB by its parts ({} B an entry + {:.1} B of name on average + 4 B order + 1 bit selection); process RSS +{:.1} MB; made in {made:.0} ms", m.bytes() as f64 / 1048576.0, std::mem::size_of::<Entry>(), m.names.len() as f64 / n as f64, after - before);
    let shuffled = m.order.clone();
    let name_cmp = best_of(3, || { m.order.copy_from_slice(&shuffled); m.sort_name_cmp(); }); let by_cmp = m.order.clone();
    let name_key = best_of(3, || { m.order.copy_from_slice(&shuffled); m.sort_name_key(); });
    println!("sort n={n} by name, natural: comparator {name_cmp:.1} ms; key made once + byte compare {name_key:.1} ms; same order: {}", by_cmp == m.order);
    for w in m.order.windows(2).take(200_000) { assert!(natural_cmp(m.name(w[0]).as_bytes(), m.name(w[1]).as_bytes()) != std::cmp::Ordering::Greater); }
    let resort = best_of(3, || m.sort_name_key());
    let size = best_of(3, || { m.order.copy_from_slice(&shuffled); m.sort_size(); }); let date = best_of(3, || { m.order.copy_from_slice(&shuffled); m.sort_date(); });
    println!("sort n={n}: by size {size:.1} ms; by date {date:.1} ms; by name when already in order {resort:.1} ms");
    m.order.copy_from_slice(&by_cmp);
    println!("  the order's start: {:?}", (0..6).map(|i| m.name(m.order[i])).collect::<Vec<_>>());
    for p in ["shot_1", "SHOT_39", "render_roto", "zzz"] { let mut hit = None; let d = best_of(5, || { hit = m.type_ahead(p); hit }); println!("type-ahead n={n} {p:?}: {d:.3} ms -> {:?}", hit.map(|i| (i, m.name(m.order[i])))); }
    let sel = best_of(5, || m.select_all()); let count: u32 = m.sel.iter().map(|w| w.count_ones()).sum(); println!("select all n={n}: {:.4} ms ({count} chosen)", sel);
    // text: what one name costs to lay out, and to draw into the scene
    let mut fonts = Fonts::new(); let ink = paint::color(t::NEUTRAL_CONTENT_COLOR_DEFAULT); let k = 4000.min(n);
    let _ = fonts.layout("warm", Family::Sans, 24.0, 400.0, ink);
    let t0 = Instant::now(); let mut ls: Vec<TextLayout> = Vec::with_capacity(k); for i in 0..k { ls.push(fonts.layout_fit(m.name(m.order[i * (n / k)]), Family::Sans, 24.0, 400.0, ink, 1200.0)); } let fit = ms(t0) * 1000.0 / k as f64;
    let t0 = Instant::now(); for i in 0..k { std::hint::black_box(fonts.layout_fit(m.name(m.order[i * (n / k)]), Family::Sans, 24.0, 400.0, ink, 180.0)); } let cut = ms(t0) * 1000.0 / k as f64;
    let t0 = Instant::now(); for i in 0..k { let e = &m.entries[i]; std::hint::black_box((fonts.layout(&date_text(e.mtime), Family::Sans, 24.0, 400.0, ink), fonts.layout(&size_text(e.size), Family::Sans, 24.0, 400.0, ink), fonts.layout(KINDS[e.kind as usize].1, Family::Sans, 24.0, 400.0, ink))); } let rest = ms(t0) * 1000.0 / k as f64;
    let mut scene = vello::Scene::new(); let t0 = Instant::now(); for l in &ls { draw_layout(&mut scene, l, Affine::translate((10.0, 10.0))); } let draw = ms(t0) * 1000.0 / k as f64;
    println!("text, one row: name laid out {fit:.1} us (fits) / {cut:.1} us (cut with an ellipsis); date + size + kind {rest:.1} us; a laid-out name drawn into the scene {draw:.2} us");
}

// ---------------------------------------------------------------- the window

#[derive(Clone, Copy, PartialEq)] enum View { List, Grid }
#[derive(Clone, Copy)] struct Phase { name: &'static str, frames: usize, sweep: bool, select_all: bool }
const WARM: usize = 40;
const PHASES: [Phase; 3] = [Phase { name: "sweep (the whole length)", frames: 600, sweep: true, select_all: false }, Phase { name: "glide (37 px a frame)", frames: 300, sweep: false, select_all: false }, Phase { name: "sweep, all selected", frames: 300, sweep: true, select_all: true }];
#[derive(Clone, Copy, Default)] struct Sample { build: f64, render: f64, gpu: f64, work: f64, present: f64, interval: f64, layouts: usize, presented: bool }
struct RowText { name: TextLayout, date: Option<TextLayout>, size: Option<TextLayout>, kind: Option<TextLayout>, seen: u64 }

struct Ui { window: Arc<Box<dyn Window>>, presenter: Presenter, layer: UiLayer, fonts: Fonts, bar: ScrollBar, model: Model, view: View, cached: bool, gpu_wait: bool,
    cache: HashMap<u32, RowText>, icons: Vec<TextLayout>, pictures: Vec<peniko::ImageBrush>, frame: u64, phase: usize, in_phase: usize, samples: Vec<Sample>, last: Option<Instant>, offset: f64, select_ms: f64, shown: usize, layouts: usize }

const ROW: f64 = 24.0; const HEADER: f64 = 22.0; const TILE: f64 = 128.0; const CELL_W: f64 = 148.0; const CELL_H: f64 = 172.0;

impl Ui {
    fn new(window: Arc<Box<dyn Window>>, model: Model, view: View, cached: bool, pictures: usize) -> Option<Ui> {
        let instance = Presenter::instance(); let surface = instance.create_surface(window.clone()).ok()?; let size = window.surface_size();
        #[cfg(target_os = "macos")] let nsview = { use raw_window_handle::{HasWindowHandle, RawWindowHandle}; match window.window_handle().ok()?.as_raw() { RawWindowHandle::AppKit(h) => Some(h.ns_view.as_ptr()), _ => None } };
        #[cfg(not(target_os = "macos"))] let nsview = None;
        let mut presenter = Presenter::new(&instance, surface, (size.width.max(1), size.height.max(1)), Output::Auto, nsview).ok()?; presenter.engine.set_ui_scale(window.scale_factor() as f32);
        let layer = UiLayer::new(&presenter.engine.device, size.width.max(1), size.height.max(1)).ok()?; presenter.engine.set_overlay(Some(layer.view()));
        let mut fonts = Fonts::new(); let s = window.scale_factor() as f32; let sub = paint::color(t::NEUTRAL_SUBDUED_CONTENT_COLOR_DEFAULT);
        let icons = ["file-image", "file-image", "file-video", "file-image", "file-audio", "folder"].iter().map(|slug| fonts.icon(slug, 16.0 * s, sub).or_else(|| fonts.icon("file", 16.0 * s, sub)).expect("an icon")).collect();
        // the placeholder pictures: 256 x 256 (a 128 px tile at a scale of 2), each its own pixels, as thumbnails are
        let pictures = (0..pictures.max(1)).map(|k| { let mut px = vec![0u8; 256 * 256 * 4]; for y in 0..256usize { for x in 0..256usize { let i = (y * 256 + x) * 4; let c = ((x / 32 + y / 32) % 2) as u8; px[i] = (40 + (k * 37 % 160)) as u8 + c * 30; px[i + 1] = (60 + (k * 91 % 140)) as u8 + (x / 4) as u8 / 2; px[i + 2] = (80 + (k * 53 % 120)) as u8 + (y / 4) as u8 / 2; px[i + 3] = 255; } } paint::image_rgba8(256, 256, px) }).collect();
        println!("window {}x{} px at scale {}; {} / {}; present modes {:?}", size.width, size.height, window.scale_factor(), presenter.info().adapter, presenter.info().backend, presenter.info().present_modes);
        Some(Ui { window, presenter, layer, fonts, bar: ScrollBar::default(), model, view, cached, gpu_wait: std::env::var_os("S17_NOGPUWAIT").is_none(), cache: HashMap::new(), icons, pictures, frame: 0, phase: 0, in_phase: 0, samples: Vec::new(), last: None, offset: 0.0, select_ms: 0.0, shown: 0, layouts: 0 })
    }
    fn resize(&mut self, w: u32, h: u32) { if w == 0 || h == 0 { return } self.presenter.resize(w, h); self.presenter.engine.set_ui_scale(self.window.scale_factor() as f32); self.layer.resize(&self.presenter.engine.device, w, h); self.presenter.engine.set_overlay(Some(self.layer.view())); }

    /// One frame; true when the script is over.
    fn redraw(&mut self) -> bool {
        let s = self.window.scale_factor(); let size = self.window.surface_size(); let (w, h) = (size.width as f64, size.height as f64); if w < 1.0 || h < 1.0 { return false }
        let t0 = Instant::now(); let interval = self.last.map(|l| t0.duration_since(l).as_secs_f64() * 1000.0).unwrap_or(0.0); self.last = Some(t0);
        let n = self.model.entries.len(); let area = Rect::new(0.0, if self.view == View::List { HEADER * s + 1.0 } else { 0.0 }, w, h);
        let per_row = if self.view == View::Grid { ((w - 16.0 * s) / (CELL_W * s)).floor().max(1.0) as usize } else { 1 };
        let content = if self.view == View::List { n as f64 * ROW * s } else { n.div_ceil(per_row) as f64 * CELL_H * s };
        let max = (content - area.height()).max(0.0);
        // the script moves the list itself
        let warm = self.frame < WARM as u64; let ph = PHASES[self.phase];
        if !warm { if self.in_phase == 0 && ph.select_all { let t = Instant::now(); self.model.select_all(); self.select_ms = ms(t); }
            self.offset = if ph.sweep { max * self.in_phase as f64 / (ph.frames - 1) as f64 } else { (max * 0.37 + self.in_phase as f64 * 37.0).min(max) }.round(); }
        self.bar.set(area, content, s); self.bar.offset = self.offset;
        self.layouts = 0; self.shown = 0;
        let scene = self.layer.scene_mut(); scene.reset();
        paint::rect(scene, Rect::new(0.0, 0.0, w, h), t::BACKGROUND_LAYER_1_COLOR);
        match self.view { View::List => Self::list(scene, &mut self.fonts, &self.model, &mut self.cache, &self.icons, self.cached, self.frame, self.offset, area, s, &mut self.layouts, &mut self.shown), View::Grid => Self::grid(scene, &mut self.fonts, &self.model, &mut self.cache, &self.pictures, self.cached, self.frame, self.offset, area, per_row, s, &mut self.layouts, &mut self.shown) }
        self.bar.paint(scene, t0);
        if self.cache.len() > 600 { let f = self.frame; self.cache.retain(|_, r| r.seen + 1 >= f); }
        let t1 = Instant::now();
        self.layer.render(&self.presenter.engine.device, &self.presenter.engine.queue);
        let t2 = Instant::now();
        if self.gpu_wait { let _ = self.presenter.engine.device.poll(wgpu::PollType::wait_indefinitely()); }
        let t3 = Instant::now();
        let window = self.window.clone(); let presented = self.presenter.draw(|| window.pre_present_notify());
        let t4 = Instant::now(); let d = |a: Instant, b: Instant| b.duration_since(a).as_secs_f64() * 1000.0;
        self.frame += 1; if warm { return false }
        // (S17_SHOT=<file.png>: the window as drawn, once, 100 frames into the first sweep - no OS screenshot is involved)
        if self.phase == 0 && self.in_phase == 100 { if let Some(path) = std::env::var_os("S17_SHOT") { if let Some((px, w, h)) = self.presenter.capture_window() { let enc = |v: f32| { let v = v.clamp(0.0, 1.0); ((if v <= 0.0031308 { v * 12.92 } else { 1.055 * v.powf(1.0 / 2.4) - 0.055 }) * 255.0).round() as u8 }; let bytes: Vec<u8> = px.chunks(4).flat_map(|p| [enc(p[0]), enc(p[1]), enc(p[2])]).collect();
            if let Ok(f) = std::fs::File::create(&path) { let mut e = png::Encoder::new(std::io::BufWriter::new(f), w, h); e.set_color(png::ColorType::Rgb); e.set_depth(png::BitDepth::Eight); let _ = e.write_header().and_then(|mut wr| wr.write_image_data(&bytes)); } } } }
        self.samples.push(Sample { build: d(t0, t1), render: d(t1, t2), gpu: d(t2, t3), work: d(t0, t3), present: d(t3, t4), interval, layouts: self.layouts, presented });
        self.in_phase += 1;
        if self.in_phase == ph.frames { self.report(ph); self.samples.clear(); self.in_phase = 0; self.phase += 1; }
        self.phase == PHASES.len()
    }
    fn report(&self, ph: Phase) {
        let col = |f: fn(&Sample) -> f64| { let mut v: Vec<f64> = self.samples.iter().map(f).collect(); v.sort_by(|a, b| a.total_cmp(b)); (v[v.len() / 2], v[(v.len() * 95 / 100).min(v.len() - 1)], v[v.len() - 1]) };
        let (work, build, render, gpu, present, iv) = (col(|s| s.work), col(|s| s.build), col(|s| s.render), col(|s| s.gpu), col(|s| s.present), col(|s| s.interval));
        let unseen = self.samples.iter().filter(|s| !s.presented).count(); let late = self.samples.iter().skip(1).filter(|s| s.interval > iv.0 * 1.5).count(); let lay = self.samples.iter().map(|s| s.layouts).sum::<usize>() as f64 / self.samples.len() as f64;
        println!("{} n={} {} [{}] {} frames, {} entries in view, {:.0} layouts a frame{}", if self.view == View::List { "list" } else { "grid" }, self.model.entries.len(), if self.cached { "cached" } else { "fresh" }, ph.name, self.samples.len(), self.shown, lay, if ph.select_all { format!(", select all {:.4} ms", self.select_ms) } else { String::new() });
        println!("    FRAME (build + vello + GPU done)  p50 {:.2}  p95 {:.2}  worst {:.2} ms", work.0, work.1, work.2);
        println!("    build p50 {:.2} p95 {:.2} worst {:.2} | vello cpu p50 {:.2} p95 {:.2} worst {:.2} | gpu wait p50 {:.2} p95 {:.2} worst {:.2} | present p50 {:.2} | interval p50 {:.2} p95 {:.2} worst {:.2}, late {}", build.0, build.1, build.2, render.0, render.1, render.2, gpu.0, gpu.1, gpu.2, present.0, iv.0, iv.1, iv.2, late);
        if unseen > 0 { println!("    !! {unseen} frames were NOT presented (the window was covered or its screen asleep): run again"); }
    }

    #[allow(clippy::too_many_arguments)]
    fn list(scene: &mut vello::Scene, fonts: &mut Fonts, m: &Model, cache: &mut HashMap<u32, RowText>, icons: &[TextLayout], cached: bool, frame: u64, offset: f64, area: Rect, s: f64, layouts: &mut usize, shown: &mut usize) {
        let (ink, sub) = (paint::color(t::NEUTRAL_CONTENT_COLOR_DEFAULT), paint::color(t::NEUTRAL_SUBDUED_CONTENT_COLOR_DEFAULT)); let px = t::BODY_SIZE_S * s as f32;
        // the columns: icon, name (what is left), date, size, kind
        let (kind_w, size_w, date_w) = (150.0 * s, 100.0 * s, 170.0 * s); let name_x = 40.0 * s; let date_x = area.x1 - 16.0 * s - kind_w - size_w - date_w; let (size_x, kind_x) = (date_x + date_w, date_x + date_w + size_w); let name_w = date_x - name_x - 12.0 * s;
        for (label, x) in [("Name", name_x), ("Date Modified", date_x), ("Size", size_x), ("Kind", kind_x)] { let l = fonts.layout(label, Family::Sans, t::DETAIL_SIZE_M * s as f32, 400.0, sub); draw_layout(scene, &l, Affine::translate((x, (HEADER * s - l.height() as f64) / 2.0))); }
        paint::hline(scene, area.x0, area.x1, HEADER * s, s, t::DIVIDER_COLOR_MEDIUM);
        scene.push_clip_layer(peniko::Fill::NonZero, Affine::IDENTITY, &area);
        // only the rows in view are laid out and drawn
        let first = (offset / (ROW * s)) as usize; let count = (area.height() / (ROW * s)) as usize + 2;
        for i in first..(first + count).min(m.order.len()) {
            let e = m.order[i]; let en = &m.entries[e as usize]; let y = area.y0 + i as f64 * ROW * s - offset; *shown += 1;
            if m.selected(e) { paint::rect(scene, Rect::new(area.x0, y, area.x1, y + ROW * s), t::SELECTION_BACKGROUND_COLOR); }
            let mut make = |fonts: &mut Fonts| { *layouts += 4; RowText { name: fonts.layout_fit(m.name(e), Family::Sans, px, 400.0, ink, name_w), date: Some(fonts.layout(&date_text(en.mtime), Family::Sans, px, 400.0, sub)), size: Some(fonts.layout(&size_text(en.size), Family::Sans, px, 400.0, sub)), kind: Some(fonts.layout(KINDS[en.kind as usize].1, Family::Sans, px, 400.0, sub)), seen: frame } };
            let fresh; let row: &RowText = if cached { let r = cache.entry(e).or_insert_with(|| make(fonts)); r.seen = frame; r } else { fresh = make(fonts); &fresh };
            let ty = y + (ROW * s - row.name.height() as f64) / 2.0;
            let icon = &icons[en.kind as usize]; draw_layout(scene, icon, Affine::translate((14.0 * s, y + (ROW * s - icon.height() as f64) / 2.0)));
            draw_layout(scene, &row.name, Affine::translate((name_x, ty)));
            for (l, x) in [(&row.date, date_x), (&row.size, size_x), (&row.kind, kind_x)] { if let Some(l) = l { draw_layout(scene, l, Affine::translate((x, ty))); } }
        }
        scene.pop_layer();
    }
    #[allow(clippy::too_many_arguments)]
    fn grid(scene: &mut vello::Scene, fonts: &mut Fonts, m: &Model, cache: &mut HashMap<u32, RowText>, pictures: &[peniko::ImageBrush], cached: bool, frame: u64, offset: f64, area: Rect, per_row: usize, s: f64, layouts: &mut usize, shown: &mut usize) {
        let ink = paint::color(t::NEUTRAL_CONTENT_COLOR_DEFAULT); let px = t::BODY_SIZE_S * s as f32; let (cw, ch) = (CELL_W * s, CELL_H * s); let left = (area.width() - per_row as f64 * cw) / 2.0;
        scene.push_clip_layer(peniko::Fill::NonZero, Affine::IDENTITY, &area);
        let first_row = (offset / ch) as usize; let rows = (area.height() / ch) as usize + 2;
        for r in first_row..first_row + rows { for c in 0..per_row {
            let i = r * per_row + c; if i >= m.order.len() { break } let e = m.order[i]; *shown += 1;
            let (x, y) = (left + c as f64 * cw, area.y0 + r as f64 * ch - offset);
            if m.selected(e) { paint::rounded(scene, Rect::new(x + 2.0 * s, y + 2.0 * s, x + cw - 2.0 * s, y + ch - 2.0 * s), 6.0 * s, t::SELECTION_BACKGROUND_COLOR); }
            let tile = Rect::new(x + (cw - TILE * s) / 2.0, y + 10.0 * s, x + (cw + TILE * s) / 2.0, y + 10.0 * s + TILE * s);
            paint::image(scene, &pictures[e as usize % pictures.len()], tile);
            let mut make = |fonts: &mut Fonts| { *layouts += 1; RowText { name: fonts.layout_fit(m.name(e), Family::Sans, px, 400.0, ink, cw - 12.0 * s), date: None, size: None, kind: None, seen: frame } };
            let fresh; let cell: &RowText = if cached { let r = cache.entry(e).or_insert_with(|| make(fonts)); r.seen = frame; r } else { fresh = make(fonts); &fresh };
            draw_layout(scene, &cell.name, Affine::translate((x + (cw - cell.name.width() as f64) / 2.0, tile.y1 + 6.0 * s)));
        } }
        scene.pop_layer();
    }
}

struct App { script: (View, usize, bool, usize), window: Option<Arc<Box<dyn Window>>>, ui: Option<Ui>, done: bool }
impl ApplicationHandler for App {
    fn can_create_surfaces(&mut self, el: &dyn ActiveEventLoop) {
        if self.window.is_some() { return }
        let attrs = WindowAttributes::default().with_title("S17").with_surface_size(winit::dpi::LogicalSize::new(1440.0, 900.0));
        self.window = el.create_window(attrs).ok().map(Arc::new);
        let (view, n, cached, pictures) = self.script; let t0 = Instant::now(); let mut model = Model::generate(n); model.sort_name_key(); println!("{n} entries made and sorted by name in {:.0} ms", ms(t0));
        self.ui = self.window.clone().and_then(|w| Ui::new(w, model, view, cached, pictures));
        if self.ui.is_none() { println!("no renderer"); el.exit(); return }
        if let Some(w) = &self.window { w.request_redraw(); }
    }
    fn window_event(&mut self, el: &dyn ActiveEventLoop, _id: WindowId, e: WindowEvent) {
        match e {
            WindowEvent::CloseRequested => el.exit(),
            WindowEvent::SurfaceResized(sz) => { if let Some(ui) = self.ui.as_mut() { ui.resize(sz.width, sz.height); } }
            WindowEvent::RedrawRequested => if !self.done { if let Some(ui) = self.ui.as_mut() { if ui.redraw() { self.done = true; self.ui = None; self.window = None; el.exit(); return } } if let Some(w) = &self.window { w.request_redraw(); } },
            _ => {}
        }
    }
}

fn main() {
    let script = std::env::var("S17_SCRIPT").unwrap_or_else(|_| "bench:100000".into()); let parts: Vec<&str> = script.split(':').collect();
    let n: usize = parts.get(1).and_then(|v| v.parse().ok()).unwrap_or(100_000);
    let cached = !parts.contains(&"fresh"); let pictures = parts.iter().find_map(|p| p.strip_prefix("img=")).and_then(|v| v.parse().ok()).unwrap_or(1);
    match parts[0] {
        "bench" => bench(n),
        v @ ("list" | "grid") => { let el = EventLoop::new().expect("the event loop"); let _ = el.run_app(App { script: (if v == "list" { View::List } else { View::Grid }, n, cached, pictures), window: None, ui: None, done: false }); }
        other => { eprintln!("S17_SCRIPT: unknown {other:?}"); std::process::exit(2); }
    }
}
