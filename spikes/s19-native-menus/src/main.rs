//! S19 - a native menu holding our own items and the system's items for a set of files, in a winit 0.31 window.
//! (docs/spikes/S19-native-menus.md). One windowed program; it prints what it finds.
//!   S19_SCRIPT=all        enumerate, menus, plumbing, popups (no hand needed; about 40 s; a window shows)
//!   S19_SCRIPT=enumerate | menus | plumbing | popups     one part
//!   S19_SCRIPT=inhandler  ONE popup shown from inside a winit callback (on macOS winit 0.31 panics: the finding)
//!   S19_SCRIPT=hand (or unset)   the window stays: a RIGHT CLICK in it pops the menu for S19_HAND=one|many (Part B)
//! Test files are made under S19_FILES (default target/s19-files); chosen items are logged to <files>/s19.log.
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering::Relaxed};
use std::time::{Duration, Instant};
use winit::application::ApplicationHandler;
use winit::event::{ButtonSource, ElementState, MouseButton, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::window::{Window, WindowAttributes, WindowId};

#[cfg(target_os = "macos")] mod mac;
#[cfg(windows)] mod win;

/// What winit's loop does, counted: redraws delivered, turns of the loop, wake-ups from another thread.
pub static REDRAW: AtomicU64 = AtomicU64::new(0);
pub static ABOUT: AtomicU64 = AtomicU64::new(0);
pub static WAKE: AtomicU64 = AtomicU64::new(0);
/// A deferred popup (one run from the run loop, outside winit's handler) is still up.
pub static BUSY: AtomicBool = AtomicBool::new(false);
pub static FAILS: AtomicU64 = AtomicU64::new(0);
pub fn fail(s: &str) { FAILS.fetch_add(1, Relaxed); println!("FAIL: {s}"); }

pub struct Files { pub dir: PathBuf, pub txt: PathBuf, pub png: PathBuf, pub pdf: PathBuf, pub mov: PathBuf, pub folder: PathBuf, pub log: PathBuf }
impl Files {
    fn make() -> Files {
        let dir = PathBuf::from(std::env::var("S19_FILES").unwrap_or_else(|_| "target/s19-files".into())); std::fs::create_dir_all(dir.join("folder")).expect("files folder");
        let dir = dir.canonicalize().expect("files folder");
        let f = Files { txt: dir.join("sample.txt"), png: dir.join("sample.png"), pdf: dir.join("sample.pdf"), mov: dir.join("sample.mov"), folder: dir.join("folder"), log: dir.join("s19.log"), dir };
        let _ = std::fs::write(&f.txt, "S19\n"); let _ = std::fs::write(&f.txt.with_file_name("second.txt"), "S19 too\n");
        // a 1x1 PNG, a PDF with nothing in it, a ".mov" that is not a film: the system goes by the name's extension
        let _ = std::fs::write(&f.png, [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a, 0, 0, 0, 0x0d, b'I', b'H', b'D', b'R', 0, 0, 0, 1, 0, 0, 0, 1, 8, 2, 0, 0, 0, 0x90, 0x77, 0x53, 0xde, 0, 0, 0, 0x0c, b'I', b'D', b'A', b'T', 8, 0xd7, 0x63, 0xf8, 0xcf, 0xc0, 0, 0, 3, 1, 1, 0, 0x18, 0xdd, 0x8d, 0xb0, 0, 0, 0, 0, b'I', b'E', b'N', b'D', 0xae, 0x42, 0x60, 0x82]);
        let _ = std::fs::write(&f.pdf, "%PDF-1.4\n%%EOF\n"); let _ = std::fs::write(&f.mov, "not a film\n"); let _ = std::fs::write(&f.log, "");
        f
    }
    pub fn one(&self) -> Vec<PathBuf> { vec![self.txt.clone()] }
    pub fn many(&self) -> Vec<PathBuf> { vec![self.txt.clone(), self.txt.with_file_name("second.txt")] }
    pub fn mixed(&self) -> Vec<PathBuf> { vec![self.txt.clone(), self.png.clone(), self.pdf.clone()] }
    pub fn all(&self) -> Vec<(&'static str, PathBuf)> { vec![(".txt", self.txt.clone()), (".png", self.png.clone()), (".pdf", self.pdf.clone()), (".mov (a dummy)", self.mov.clone()), ("a folder", self.folder.clone())] }
}
/// A path as the verdict may show it: below the files folder only.
pub fn short(files: &Files, p: &std::path::Path) -> String { p.strip_prefix(&files.dir).map(|r| format!("<files>/{}", r.display())).unwrap_or_else(|_| format!("<elsewhere>/{}", p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default())) }

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Step { Settle, Baseline(&'static str), Measure, Enumerate, Menus, Plumbing, PopInHandler(u32), PopDeferred(u32), PopContext(u32), PopPick, Done }

struct App { window: Option<Box<dyn Window>>, files: Files, steps: Vec<Step>, at: usize, due: Instant, hand: bool, base: Option<([u64; 5], Instant)>, t0: Instant }
impl App {
    fn view(&self) -> Option<*mut std::ffi::c_void> { use raw_window_handle::{HasWindowHandle, RawWindowHandle}; let w = self.window.as_ref()?;
        match w.window_handle().ok()?.as_raw() { #[cfg(target_os = "macos")] RawWindowHandle::AppKit(h) => Some(h.ns_view.as_ptr()), #[cfg(windows)] RawWindowHandle::Win32(h) => Some(h.hwnd.get() as *mut std::ffi::c_void), _ => None } }
    /// One step of the script; called from INSIDE winit's handler (`proxy_wake_up`).
    fn step(&mut self, el: &dyn ActiveEventLoop) {
        if self.window.is_none() || BUSY.load(Relaxed) || Instant::now() < self.due || self.at >= self.steps.len() { return }   // (the first wake-up can come before the window is made)
        let s = self.steps[self.at]; self.at += 1; self.due = Instant::now() + Duration::from_millis(250);
        let Some(view) = self.view() else { fail("no native view"); el.exit(); return };
        match s {
            Step::Settle => { self.due = Instant::now() + Duration::from_millis(1200); }
            Step::Baseline(what) => { println!("\n== the loop {what}, over one second"); self.base = Some((counters(), Instant::now())); self.due = Instant::now() + Duration::from_secs(1); }
            Step::Measure => { if let Some((c, t)) = self.base.take() { let d = delta(c); println!("   {} in {:.2} s", show(d), t.elapsed().as_secs_f64()); if d[0] == 0 || d[1] == 0 || d[2] == 0 { fail("the winit loop is not alive"); } else { println!("   the winit loop is alive: PASS"); } } }
            Step::Done => { let n = FAILS.load(Relaxed); println!("\n== S19 Part A: {} ({} failure(s)); {:.1} s", if n == 0 { "PASS" } else { "FAIL" }, n, self.t0.elapsed().as_secs_f64()); el.exit(); }
            #[cfg(target_os = "macos")] _ => mac::step(s, view, &self.files),
            #[cfg(windows)] _ => win::step(s, view, &self.files),
        }
    }
}
pub fn counters() -> [u64; 5] { #[cfg(target_os = "macos")] let (a, b) = mac::ticks(); #[cfg(not(target_os = "macos"))] let (a, b) = (0, 0); [REDRAW.load(Relaxed), ABOUT.load(Relaxed), WAKE.load(Relaxed), a, b] }
pub fn delta(c: [u64; 5]) -> [u64; 5] { let n = counters(); [n[0] - c[0], n[1] - c[1], n[2] - c[2], n[3] - c[3], n[4] - c[4]] }
pub fn show(d: [u64; 5]) -> String { format!("winit redraws {:3}, loop turns {:3}, wake-ups {:3} | timer (default mode) {:3}, timer (common modes) {:3}", d[0], d[1], d[2], d[3], d[4]) }

impl ApplicationHandler for App {
    fn can_create_surfaces(&mut self, el: &dyn ActiveEventLoop) {
        if self.window.is_some() { return }
        let w = el.create_window(WindowAttributes::default().with_title("S19 - native menus").with_surface_size(winit::dpi::LogicalSize::new(520.0, 360.0))).expect("window");
        w.request_redraw(); self.window = Some(w);
        if let Some(v) = self.view() { #[cfg(target_os = "macos")] mac::init(v, &self.files); #[cfg(windows)] win::init(v, &self.files); }
        if self.hand { println!("hand mode: right-click in the window (S19_HAND=one|many|mixed); close the window to end"); }
    }
    fn window_event(&mut self, el: &dyn ActiveEventLoop, _id: WindowId, e: WindowEvent) {
        match e {
            WindowEvent::CloseRequested => el.exit(),
            // (nothing is drawn: the question is whether winit DELIVERS redraws while a menu is tracked)
            WindowEvent::RedrawRequested => { REDRAW.fetch_add(1, Relaxed); }
            WindowEvent::PointerButton { state: ElementState::Pressed, position, button: ButtonSource::Mouse(MouseButton::Right), .. } if self.hand => {
                let scale = self.window.as_ref().map(|w| w.scale_factor()).unwrap_or(1.0);
                let which = std::env::var("S19_HAND").unwrap_or_default(); let paths = match which.as_str() { "many" => self.files.many(), "mixed" => self.files.mixed(), _ => self.files.one() };
                if let Some(v) = self.view() { let at = (position.x / scale, position.y / scale); let _ = (v, at, &paths);
                    #[cfg(target_os = "macos")] mac::hand(v, at, paths); #[cfg(windows)] win::hand(v, (position.x, position.y), paths); }
            }
            _ => {}
        }
    }
    fn proxy_wake_up(&mut self, el: &dyn ActiveEventLoop) { WAKE.fetch_add(1, Relaxed); if let Some(w) = &self.window { w.request_redraw(); } if !self.hand { self.step(el); } }
    fn about_to_wait(&mut self, el: &dyn ActiveEventLoop) { ABOUT.fetch_add(1, Relaxed); el.set_control_flow(ControlFlow::Wait); }
}

fn main() {
    let script = std::env::var("S19_SCRIPT").unwrap_or_else(|_| "hand".into());
    let files = Files::make();
    let pops = || { let mut v = vec![Step::Baseline("with no menu up"), Step::Measure]; v.extend((1..=20).map(Step::PopDeferred)); v.extend((1..=3).map(Step::PopContext)); v.push(Step::PopPick); v.extend([Step::Baseline("AFTER the popups"), Step::Measure]); v };
    let mut steps = vec![Step::Settle];
    match script.as_str() { "all" => { steps.extend([Step::Enumerate, Step::Menus, Step::Plumbing]); steps.extend(pops()); } "enumerate" => steps.push(Step::Enumerate), "menus" => steps.push(Step::Menus), "plumbing" => steps.push(Step::Plumbing), "popups" => steps.extend(pops()), "inhandler" => steps.push(Step::PopInHandler(1)), _ => {} }
    steps.push(Step::Done);
    let el = EventLoop::new().expect("event loop");
    // another thread wakes the loop 100 times a second: the script's clock, and the measure of "does the loop turn"
    { let p = el.create_proxy(); std::thread::spawn(move || loop { std::thread::sleep(Duration::from_millis(10)); p.wake_up(); }); }
    let app = App { window: None, files, steps, at: 0, due: Instant::now(), hand: script == "hand", base: None, t0: Instant::now() };
    el.run_app(app).expect("run");
    println!("the loop ended; leaving");
}
