//! S16 - real OS drag and drop of files from a winit window, both ways (docs/spikes/S16-dragdrop.md).
//!
//! Two windows, each with a list of generated files (drag rows OUT; the rows are also drop targets, so the row under the
//! pointer highlights) and a log of everything that arrives (also written to a file). The platform layers are `mac.rs`
//! (AppKit, run here) and `win.rs` (OLE, written without a Windows machine). A spike: it dies with its verdict.
//!
//!   s16                         the two windows, for the owner's hand (Part B)
//!   s16 --selftest              Part A: sets everything up, checks what can be checked without a drag, exits 0 / 1
//!   s16 --log <file>            where the log goes (default target/s16-files/s16.log)
//!   s16 --files <dir>           where the generated files live (default target/s16-files)
//!   s16 --quit-after <seconds>  leave by itself
//! Keys in a window: R makes the files again, C clears the log, 1 / 2 / 3 offer copy+move+link / copy only / move only.
use kurbo::{Affine, Rect};
use min_present::{Output, Presenter};
use min_theme::tokens as t;
use min_ui::{fonts::{draw_layout, Fonts}, paint, Family, UiLayer};
use std::cell::RefCell;
use std::collections::BTreeSet;
use std::io::Write;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};
use winit31 as winit;
use winit::application::ApplicationHandler;
use winit::event::{ButtonSource, ElementState, MouseButton, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::window::{Window, WindowAttributes, WindowId};

/// What the platform layers and the application say to each other.
pub mod dnd {
    use std::path::PathBuf;
    use std::rc::Rc;
    pub const COPY: u32 = 1; pub const LINK: u32 = 2; pub const MOVE: u32 = 4;
    pub fn ops(m: u32) -> String { if m == 0 { return "none".into() } let mut v = Vec::new(); if m & COPY != 0 { v.push("copy"); } if m & MOVE != 0 { v.push("move"); } if m & LINK != 0 { v.push("link"); } v.join("+") }
    #[derive(Clone, Copy, PartialEq, Debug)] pub enum Phase { Enter, Over, Leave, Drop }
    /// What a drag carries, as far as a file browser cares.
    #[derive(Clone, Debug, Default)]
    pub struct Payload { /// the types / formats offered, by the system's own names
        pub types: Vec<String>, /// local files and folders
        pub paths: Vec<PathBuf>, /// URLs that are not local files
        pub urls: Vec<String>, pub text: Option<String>, /// files that do not exist yet (a mail attachment, a browser's image): one entry each, what is known before delivery
        pub promised: Vec<String> }
    pub struct DragEvent { pub win: usize, pub phase: Phase, /// physical pixels from the window's content's top left
        pub x: f64, pub y: f64, /// what the source allows, narrowed by the modifier keys held
        pub allowed: u32, /// the platform's own account (raw masks, key state, where the drag comes from)
        pub detail: String, /// our own drag's serial, when this drag was started by this process
        pub mine: Option<u64>, pub payload: Rc<Payload> }
    pub enum Notice { Drag(DragEvent), /// a promised file has arrived (or has not)
        Promised { win: usize, result: Result<PathBuf, String> }, /// a drag we started is over: what the target did
        Ended { serial: u64, op: u32, detail: String } }
    /// The application's answer to a `Drag` is the operation it would perform (0: refuse); to the others, nothing.
    pub type Sink = Rc<dyn Fn(Notice) -> u32>;
    /// The picture under the pointer during a drag: straight (not premultiplied) RGBA, physical pixels.
    pub struct DragImage { pub w: u32, pub h: u32, pub rgba: Vec<u8>, pub hot: (f64, f64), pub scale: f64 }
}
use dnd::*;

#[cfg(target_os = "macos")] #[path = "mac.rs"] mod os;
#[cfg(windows)] #[path = "win.rs"] mod os;
#[cfg(not(any(target_os = "macos", windows)))]
mod os {
    use super::dnd::*; use std::path::{Path, PathBuf};
    pub fn attach(_w: &dyn winit31::window::Window, _win: usize, _dest: &Path, _sink: Sink) -> Result<String, String> { Err("no platform layer".into()) }
    pub fn start_drag(_w: &dyn winit31::window::Window, _paths: &[PathBuf], _img: DragImage, _ops: u32, _sink: Sink) -> Result<u64, String> { Err("no platform layer".into()) }
    pub fn selftest(_w: &dyn winit31::window::Window, _paths: &[PathBuf], _dest: &Path) -> Vec<(String, bool, String)> { Vec::new() }
    pub fn selftest_late() -> Vec<(String, bool, String)> { Vec::new() }
}

// the layout, in logical pixels (the same for drawing, for presses and for "the row under the pointer")
const PAD: f64 = 16.0; const TOP: f64 = 64.0; const ROW: f64 = 28.0; const LIST_W: f64 = 300.0;
fn row_at(x: f64, y: f64, s: f64, n: usize) -> Option<usize> { if x < PAD * s || x > (PAD + LIST_W) * s || y < TOP * s { return None } let i = ((y - TOP * s) / (ROW * s)) as usize; (i < n).then_some(i) }

struct Hover { x: f64, y: f64, op: u32, row: Option<usize> }
struct Shared { born: Instant, log: Vec<String>, file: Option<std::fs::File>, dir: PathBuf, files: Vec<PathBuf>, sel: [BTreeSet<usize>; 2], hover: [Option<Hover>; 2], scale: [f64; 2], offer: u32,
    /// counted for the self-test: pointer presses and releases that reached us through winit; winit's own drag events
    buttons: usize, winit_drags: usize, last_over: (usize, Option<usize>, u32, u32), dragging: Option<Vec<PathBuf>> }
impl Shared {
    fn say(&mut self, win: Option<usize>, msg: impl AsRef<str>) { let line = format!("[{:8.2}] {} {}", self.born.elapsed().as_secs_f64(), win.map(|w| format!("w{}", w + 1)).unwrap_or_else(|| "--".into()), msg.as_ref());
        if let Some(f) = self.file.as_mut() { let _ = writeln!(f, "{line}"); let _ = f.flush(); } self.log.push(line); if self.log.len() > 400 { self.log.drain(..100); } }
    /// The files to drag out: plain ones, awkward names, an empty one, a folder with a file in it. Never anything of the user's.
    fn generate(&mut self) { let out = self.dir.join("out"); let _ = std::fs::create_dir_all(&out); let _ = std::fs::create_dir_all(self.dir.join("promised")); let mut v = Vec::new();
        for i in 1..=5 { v.push((format!("s16 file {i:02}.txt"), format!("S16 generated file {i}\n"))); }
        v.push(("s16 \u{fc}n\u{ef}c\u{f6}d\u{e9} #7 & more.txt".into(), "an awkward name\n".into())); v.push(("s16 empty.dat".into(), String::new()));
        v.push((format!("s16 {}.txt", "long name ".repeat(12).trim_end()), "a long name\n".into()));
        self.files = v.into_iter().map(|(n, body)| { let p = out.join(n); if !p.exists() { let _ = std::fs::write(&p, body); } p }).collect();
        let folder = out.join("s16 folder"); let _ = std::fs::create_dir_all(&folder); let inner = folder.join("inside.txt"); if !inner.exists() { let _ = std::fs::write(inner, "inside the folder\n"); } self.files.push(folder);
        self.files = self.files.iter().map(|p| std::fs::canonicalize(p).unwrap_or(p.clone())).collect(); }
}
/// The operation a drop here would be: the only one left by the modifiers; else a move for our own drag, a copy for a stranger's.
fn choose(allowed: u32, mine: bool) -> u32 { if allowed.count_ones() == 1 { allowed } else if mine && allowed & MOVE != 0 { MOVE } else if allowed & COPY != 0 { COPY } else if allowed & MOVE != 0 { MOVE } else { allowed & LINK } }

/// Everything the platform layers report comes through here: logged, the hover kept for the next frame.
fn notice(sh: &Rc<RefCell<Shared>>, n: Notice) -> u32 {
    let mut s = sh.borrow_mut();
    match n {
        Notice::Drag(e) => { let op = if e.phase == Phase::Leave { 0 } else { choose(e.allowed, e.mine.is_some()) }; let row = row_at(e.x, e.y, s.scale[e.win], s.files.len()); let p = &e.payload;
            let target = row.map(|r| format!("row {} \"{}\"", r + 1, s.files[r].file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default())).unwrap_or_else(|| "the pane".into());
            let from = match e.mine { Some(id) => format!("OURS (drag #{id})"), None => "not ours".into() };
            match e.phase {
                Phase::Enter => { s.say(Some(e.win), format!("ENTER at {:.0},{:.0} allowed={} would={} {from} | {} file(s), {} url(s), {} promised, text={} | {}", e.x, e.y, ops(e.allowed), ops(op), p.paths.len(), p.urls.len(), p.promised.len(), p.text.is_some(), e.detail));
                    s.say(Some(e.win), format!("  types: {}", p.types.join(", "))); s.last_over = (e.win, row, op, e.allowed); }
                Phase::Over => { if s.last_over != (e.win, row, op, e.allowed) { s.last_over = (e.win, row, op, e.allowed); s.say(Some(e.win), format!("OVER at {:.0},{:.0} on {target} allowed={} would={} | {}", e.x, e.y, ops(e.allowed), ops(op), e.detail)); } }
                Phase::Leave => s.say(Some(e.win), "LEAVE"),
                Phase::Drop => { s.say(Some(e.win), format!("DROP at {:.0},{:.0} on {target}: {} {from} allowed={} | {}", e.x, e.y, ops(op).to_uppercase(), ops(e.allowed), e.detail)); s.say(Some(e.win), format!("  types: {}", p.types.join(", ")));
                    for f in &p.paths { let kind = if f.is_dir() { "folder" } else if f.is_file() { "file" } else { "MISSING" }; s.say(Some(e.win), format!("  path ({kind}): {}", f.display())); }
                    for u in &p.urls { s.say(Some(e.win), format!("  url: {u}")); } if let Some(tx) = &p.text { let short: String = tx.chars().take(120).collect(); s.say(Some(e.win), format!("  text: {short:?}")); }
                    for pr in &p.promised { s.say(Some(e.win), format!("  promised: {pr}")); }
                    if p.paths.is_empty() && p.urls.is_empty() && p.promised.is_empty() && p.text.is_none() { s.say(Some(e.win), "  (nothing a file browser can use)"); } }
            }
            s.hover[e.win] = if matches!(e.phase, Phase::Enter | Phase::Over) { Some(Hover { x: e.x, y: e.y, op, row }) } else { None };
            op }
        Notice::Promised { win, result } => { match result { Ok(p) => s.say(Some(win), format!("PROMISED FILE DELIVERED: {} ({} bytes)", p.display(), std::fs::metadata(&p).map(|m| m.len()).unwrap_or(0))), Err(e) => s.say(Some(win), format!("PROMISED FILE FAILED: {e}")) } 0 }
        Notice::Ended { serial, op, detail } => { let gone = s.dragging.take().map(|v| { let n = v.len(); (v.iter().filter(|p| !p.exists()).count(), n) });
            s.say(None, format!("DRAG #{serial} ENDED: the target reports {} | {detail}{}", ops(op).to_uppercase(), gone.map(|(g, n)| format!(" | {g} of {n} source(s) no longer exist")).unwrap_or_default())); 0 }
    }
}

struct Ui { idx: usize, window: Arc<Box<dyn Window>>, presenter: Presenter, layer: UiLayer, fonts: Fonts }
impl Ui {
    fn new(window: Arc<Box<dyn Window>>, idx: usize) -> Option<Ui> {
        let instance = Presenter::instance(); let surface = instance.create_surface(window.clone()).ok()?; let size = window.surface_size();
        #[cfg(target_os = "macos")] let view = { use raw_window_handle::{HasWindowHandle, RawWindowHandle}; match window.window_handle().ok()?.as_raw() { RawWindowHandle::AppKit(h) => Some(h.ns_view.as_ptr()), _ => None } };
        #[cfg(not(target_os = "macos"))] let view = None;
        let mut presenter = Presenter::new(&instance, surface, (size.width.max(1), size.height.max(1)), Output::Auto, view).ok()?; presenter.engine.set_ui_scale(window.scale_factor() as f32);
        let layer = UiLayer::new(&presenter.engine.device, size.width.max(1), size.height.max(1)).ok()?; presenter.engine.set_overlay(Some(layer.view()));
        Some(Ui { idx, window, presenter, layer, fonts: Fonts::new() })
    }
    fn resize(&mut self, w: u32, h: u32) { if w == 0 || h == 0 { return } self.presenter.resize(w, h); self.presenter.engine.set_ui_scale(self.window.scale_factor() as f32); self.layer.resize(&self.presenter.engine.device, w, h); self.presenter.engine.set_overlay(Some(self.layer.view())); }
    fn redraw(&mut self, sh: &Shared) {
        let s = self.window.scale_factor(); let size = self.window.surface_size(); let (w, h) = (size.width as f64, size.height as f64); if w < 1.0 || h < 1.0 { return }
        let Ui { layer, fonts, idx, .. } = self; let i = *idx; let scene = layer.scene_mut(); scene.reset();
        let (ink, sub) = (paint::color(t::NEUTRAL_CONTENT_COLOR_DEFAULT), paint::color(t::NEUTRAL_SUBDUED_CONTENT_COLOR_DEFAULT));
        paint::rect(scene, Rect::new(0.0, 0.0, w, h), t::BACKGROUND_LAYER_1_COLOR);
        let mut text = |scene: &mut vello::Scene, txt: &str, x: f64, y: f64, size: f32, mono: bool, color, room: f64| { let l = fonts.layout_fit(txt, if mono { Family::Mono } else { Family::Sans }, size * s as f32, t::FONT_WEIGHT_REGULAR as f32, color, room.max(10.0)); draw_layout(scene, &l, Affine::translate((x, y))); };
        text(scene, &format!("S16 drag and drop - window {}", i + 1), PAD * s, 12.0 * s, 15.0, false, ink, w);
        text(scene, &format!("Drag rows out (offering {}; keys 1 / 2 / 3 change it). Drop anything anywhere: a row under the pointer is the target. R: files again. C: clear.", ops(sh.offer)), PAD * s, 36.0 * s, 11.0, false, sub, w - 2.0 * PAD * s);
        let hover = sh.hover[i].as_ref();
        for (r, p) in sh.files.iter().enumerate() { let y = (TOP + r as f64 * ROW) * s; let rect = Rect::new(PAD * s, y, (PAD + LIST_W) * s, y + (ROW - 2.0) * s);
            let target = hover.is_some_and(|hv| hv.row == Some(r));
            paint::rounded(scene, rect, 4.0 * s, if target { t::ACCENT_VISUAL_COLOR } else if sh.sel[i].contains(&r) { t::ACCENT_BACKGROUND_COLOR_DEFAULT } else { t::BACKGROUND_LAYER_2_COLOR });
            let name = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default(); let exists = p.exists();
            text(scene, &format!("{}{name}{}", if p.is_dir() { "[folder] " } else { "" }, if exists { "" } else { "  (gone)" }), (PAD + 8.0) * s, y + 5.0 * s, 12.0, false, if exists { ink } else { sub }, (LIST_W - 16.0) * s); }
        // the log: the newest lines that fit
        let (lx, ly) = ((PAD * 2.0 + LIST_W) * s, TOP * s); let pane = Rect::new(lx, ly, w - PAD * s, h - PAD * s); paint::rounded(scene, pane, 6.0 * s, t::BACKGROUND_LAYER_2_COLOR);
        let line = 15.0 * s; let fit = (((pane.height() - 12.0 * s) / line) as usize).max(1); let from = sh.log.len().saturating_sub(fit);
        for (k, l) in sh.log[from..].iter().enumerate() { text(scene, l, lx + 8.0 * s, ly + 6.0 * s + k as f64 * line, 10.5, true, if l.contains("DROP") || l.contains("ENDED") || l.contains("DELIVERED") { ink } else { sub }, pane.width() - 16.0 * s); }
        // a drag over this window: an outline round the pane, and where the pointer is with what a drop would do
        if let Some(hv) = hover { let c = if hv.op == 0 { t::NEGATIVE_VISUAL_COLOR } else { t::POSITIVE_VISUAL_COLOR }; let b = 2.0 * s;
            for r in [Rect::new(0.0, 0.0, w, b), Rect::new(0.0, h - b, w, h), Rect::new(0.0, 0.0, b, h), Rect::new(w - b, 0.0, w, h), Rect::new(hv.x - 8.0 * s, hv.y - 0.5 * s, hv.x + 8.0 * s, hv.y + 0.5 * s), Rect::new(hv.x - 0.5 * s, hv.y - 8.0 * s, hv.x + 0.5 * s, hv.y + 8.0 * s)] { paint::rect(scene, r, c); }
            text(scene, &format!("{} at {:.0},{:.0}", ops(hv.op), hv.x, hv.y), hv.x + 12.0 * s, hv.y + 10.0 * s, 11.0, false, ink, 400.0 * s); }
        self.layer.render(&self.presenter.engine.device, &self.presenter.engine.queue);
        let window = self.window.clone(); self.presenter.draw(|| window.pre_present_notify());
    }
}
/// The picture a drag carries: a small card with one mark per file (the real thing would be the row as drawn, with a count).
fn drag_image(n: usize, s: f64) -> DragImage { let (w, h) = ((180.0 * s) as u32, (34.0 * s) as u32); let mut rgba = vec![0u8; (w * h * 4) as usize]; let u = s.round().max(1.0) as u32;
    for y in 0..h { for x in 0..w { let edge = x < u || y < u || x >= w - u || y >= h - u; let mark = { let k = (x.saturating_sub(10 * u)) / (14 * u); x >= 10 * u && (k as usize) < n.min(10) && (x - 10 * u) % (14 * u) < 10 * u && y >= 12 * u && y < 22 * u };
        let c: [u8; 4] = if edge { [70, 140, 255, 255] } else if mark { [240, 240, 240, 255] } else { [40, 42, 48, 215] }; rgba[((y * w + x) * 4) as usize..][..4].copy_from_slice(&c); } }
    DragImage { w, h, rgba, hot: (8.0 * s, 8.0 * s), scale: s } }

struct App { sh: Rc<RefCell<Shared>>, uis: Rc<RefCell<Vec<Ui>>>, ids: Vec<WindowId>, mods: (bool, bool), pointer: [(f64, f64); 2], /// a press on a row: (window, row, x, y, whether it was already chosen)
    press: Option<(usize, usize, f64, f64, bool)>, selftest: bool, stage: u8, at: Instant, code: Rc<std::cell::Cell<i32>>, quit_after: Option<f64> }
impl App {
    fn sink(&self) -> Sink { let (sh, uis) = (self.sh.clone(), self.uis.clone());
        // the platform calls this from its own callbacks (on Windows inside DoDragDrop's own loop, where winit's loop does
        // not turn): the frame is drawn here and now, when nothing else holds the windows
        Rc::new(move |n| { let win = match &n { Notice::Drag(e) => Some(e.win), Notice::Promised { win, .. } => Some(*win), _ => None }; let op = notice(&sh, n);
            if let Ok(mut uis) = uis.try_borrow_mut() { for ui in uis.iter_mut() { if win.is_none_or(|w| w == ui.idx) { if let Ok(s) = sh.try_borrow() { ui.redraw(&s); } ui.window.request_redraw(); } } } op }) }
    fn redraw_all(&self) { for ui in self.uis.borrow().iter() { ui.window.request_redraw(); } }
}
impl ApplicationHandler for App {
    fn can_create_surfaces(&mut self, el: &dyn ActiveEventLoop) {
        if !self.ids.is_empty() { return }
        for i in 0..2usize {
            #[allow(unused_mut)] let mut attrs = WindowAttributes::default().with_title(format!("S16 drag and drop - window {}", i + 1)).with_surface_size(winit::dpi::LogicalSize::new(980.0, 560.0)).with_position(winit::dpi::LogicalPosition::new(60.0 + 120.0 * i as f64, 80.0 + 380.0 * i as f64));
            // Windows: winit registers an IDropTarget of its own on every window unless told not to; a window has ONE drop target, so ours needs its place
            #[cfg(windows)] { use winit::platform::windows::WindowAttributesWindows; attrs = attrs.with_platform_attributes(Box::new(WindowAttributesWindows::default().with_drag_and_drop(false))); }
            let Ok(window) = el.create_window(attrs) else { self.sh.borrow_mut().say(Some(i), "the window could not be made"); continue }; let window = Arc::new(window);
            self.sh.borrow_mut().scale[i] = window.scale_factor(); self.ids.push(window.id());
            let attached = os::attach(&**window, i, &self.sh.borrow().dir.join("promised"), self.sink());
            self.sh.borrow_mut().say(Some(i), match attached { Ok(d) => format!("drop target attached: {d}"), Err(e) => format!("DROP TARGET NOT ATTACHED: {e}") });
            match Ui::new(window, i) { Some(ui) => self.uis.borrow_mut().push(ui), None => self.sh.borrow_mut().say(Some(i), "no renderer for this window") }
        }
        self.at = Instant::now();
    }
    fn window_event(&mut self, el: &dyn ActiveEventLoop, id: WindowId, e: WindowEvent) {
        let Some(i) = self.ids.iter().position(|w| *w == id) else { return };
        match e {
            WindowEvent::CloseRequested => el.exit(),
            WindowEvent::RedrawRequested => { if let Ok(mut uis) = self.uis.try_borrow_mut() { if let Some(ui) = uis.iter_mut().find(|u| u.idx == i) { ui.redraw(&self.sh.borrow()); } } }
            WindowEvent::SurfaceResized(sz) => { if let Some(ui) = self.uis.borrow_mut().iter_mut().find(|u| u.idx == i) { ui.resize(sz.width, sz.height); } self.redraw_all(); }
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => { self.sh.borrow_mut().scale[i] = scale_factor; self.redraw_all(); }
            WindowEvent::ModifiersChanged(m) => { let st = m.state(); self.mods = (st.shift_key(), if cfg!(target_os = "macos") { st.meta_key() } else { st.control_key() }); }
            WindowEvent::PointerMoved { position, .. } => { self.pointer[i] = (position.x, position.y);
                // a pressed row moved 6 px (the old application's threshold) is a drag: the chosen rows if the pressed one is among them, else that row alone
                if let Some((w, row, x0, y0, _)) = self.press { let s = self.sh.borrow().scale[i]; if w == i && (position.x - x0).hypot(position.y - y0) > 6.0 * s { self.press = None;
                    let (paths, offer) = { let sh = self.sh.borrow(); let rows: Vec<usize> = if sh.sel[i].contains(&row) { sh.sel[i].iter().copied().collect() } else { vec![row] }; (rows.iter().filter_map(|r| sh.files.get(*r).cloned()).collect::<Vec<_>>(), sh.offer) };
                    { let mut sh = self.sh.borrow_mut(); sh.dragging = Some(paths.clone()); sh.say(Some(i), format!("DRAG START: {} path(s), offering {}", paths.len(), ops(offer))); for p in &paths { sh.say(Some(i), format!("  out: {}", p.display())); } }
                    // nothing of ours is borrowed from here on: on Windows this call returns only when the drag is over, and our own windows are drawn from inside it
                    let window = self.uis.borrow().iter().find(|u| u.idx == i).map(|u| u.window.clone());
                    if let Some(window) = window { let r = os::start_drag(&**window, &paths, drag_image(paths.len(), s), offer, self.sink());
                        let mut sh = self.sh.borrow_mut(); match r { Ok(serial) => sh.say(Some(i), format!("  the system took the drag (#{serial})")), Err(e) => { sh.dragging = None; sh.say(Some(i), format!("  THE DRAG DID NOT START: {e}")) } } }
                    self.redraw_all(); } } }
            WindowEvent::PointerButton { state, position, button: ButtonSource::Mouse(MouseButton::Left), .. } => { let (x, y) = (position.x, position.y); let mut sh = self.sh.borrow_mut(); sh.buttons += 1; let s = sh.scale[i]; let n = sh.files.len();
                if state == ElementState::Pressed { match row_at(x, y, s, n) {
                        // a press on a chosen row keeps the choice (a drag carries all of it); a plain press on another row chooses it alone
                        Some(r) => { let was = sh.sel[i].contains(&r); if self.mods.1 { if !sh.sel[i].remove(&r) { sh.sel[i].insert(r); } } else if self.mods.0 { let a = sh.sel[i].iter().next().copied().unwrap_or(r); sh.sel[i] = (a.min(r)..=a.max(r)).collect(); } else if !was { sh.sel[i] = BTreeSet::from([r]); }
                            if !self.mods.1 && !self.mods.0 { self.press = Some((i, r, x, y, was)); } }
                        None => { sh.sel[i].clear(); self.press = None; } } }
                // let go without a drag on a row of a larger choice: that row alone
                else if let Some((w, r, _, _, was)) = self.press.take() { if w == i && was { sh.sel[i] = BTreeSet::from([r]); } }
                drop(sh); self.redraw_all(); }
            WindowEvent::KeyboardInput { event, .. } if event.state.is_pressed() => { if let winit::keyboard::Key::Character(c) = &event.logical_key { let mut sh = self.sh.borrow_mut(); match c.to_lowercase().as_str() {
                    "r" => { sh.generate(); sh.say(None, "the files were made again"); } "c" => sh.log.clear(),
                    "1" => sh.offer = COPY | MOVE | LINK, "2" => sh.offer = COPY, "3" => sh.offer = MOVE, _ => {} } } self.redraw_all(); }
            // winit's own drag events: with our layer in place none of these should arrive (coexistence, question d)
            WindowEvent::DragEntered { .. } | WindowEvent::DragDropped { .. } | WindowEvent::DragLeft { .. } => { let mut sh = self.sh.borrow_mut(); sh.winit_drags += 1; sh.say(Some(i), "NOTE: winit's OWN drag event arrived (its handling is still in place beside ours)"); }
            _ => {}
        }
    }
    fn about_to_wait(&mut self, el: &dyn ActiveEventLoop) {
        el.set_control_flow(ControlFlow::WaitUntil(Instant::now() + Duration::from_millis(100)));
        if self.quit_after.is_some_and(|q| self.at.elapsed().as_secs_f64() > q) { el.exit(); return }
        if !self.selftest || self.ids.is_empty() { return }
        let say = |sh: &Rc<RefCell<Shared>>, r: Vec<(String, bool, String)>| -> bool { let mut ok = true; for (name, pass, detail) in r { ok &= pass; let line = format!("SELFTEST {} {name}: {detail}", if pass { "ok  " } else { "FAIL" }); println!("{line}"); sh.borrow_mut().say(None, line); } ok };
        if self.stage == 0 && self.at.elapsed() > Duration::from_millis(700) { self.stage = 1; self.at = Instant::now();
            let window = self.uis.borrow().first().map(|u| u.window.clone()); let (files, dest, attached) = { let sh = self.sh.borrow(); (sh.files.clone(), sh.dir.join("promised"), sh.log.iter().filter(|l| l.contains("drop target attached")).count()) };
            let mut r = vec![("two windows, each a drop target, each drawn".to_string(), attached == 2 && self.uis.borrow().len() == 2, format!("{attached} attached, {} drawn", self.uis.borrow().len()))];
            if let Some(w) = window { r.extend(os::selftest(&**w, &files, &dest)); } if !say(&self.sh, r) { self.code.set(1); } }
        else if self.stage == 1 && self.at.elapsed() > Duration::from_millis(1500) { self.stage = 2; let mut r = os::selftest_late(); let (b, d) = { let sh = self.sh.borrow(); (sh.buttons, sh.winit_drags) };
            if cfg!(target_os = "macos") { r.push(("pointer events still reach us through winit (a press and a release handed to the view in-process)".into(), b >= 2, format!("{b} button events"))); }
            r.push(("winit's own drag handling stayed silent".into(), d == 0, format!("{d} winit drag events"))); if !say(&self.sh, r) { self.code.set(1); }
            println!("SELFTEST {}", if self.code.get() == 0 { "PASS" } else { "FAIL" }); el.exit(); }
    }
}
fn main() {
    let args: Vec<String> = std::env::args().collect(); let arg = |k: &str| args.iter().position(|a| a == k).and_then(|i| args.get(i + 1).cloned());
    let dir = arg("--files").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("target/s16-files")); let _ = std::fs::create_dir_all(&dir); let dir = std::fs::canonicalize(&dir).unwrap_or(dir);
    let log = arg("--log").map(PathBuf::from).unwrap_or_else(|| dir.join("s16.log"));
    let file = std::fs::OpenOptions::new().create(true).append(true).open(&log).ok();
    let mut sh = Shared { born: Instant::now(), log: Vec::new(), file, dir, files: Vec::new(), sel: Default::default(), hover: [None, None], scale: [1.0; 2], offer: COPY | MOVE | LINK, buttons: 0, winit_drags: 0, last_over: (9, None, 0, 0), dragging: None };
    sh.generate(); sh.say(None, format!("S16 started ({}); {} generated files; the log: {}", std::env::consts::OS, sh.files.len(), log.display()));
    // the self-test never outstays its welcome: if the loop stops turning, the process still ends (and says so)
    if args.iter().any(|a| a == "--selftest") { std::thread::spawn(|| { std::thread::sleep(Duration::from_secs(20)); println!("SELFTEST FAIL: the loop stopped turning (watchdog)"); std::process::exit(3); }); }
    let el = EventLoop::new().expect("the event loop");
    let code = Rc::new(std::cell::Cell::new(0)); let shared;
    let app = App { sh: { shared = Rc::new(RefCell::new(sh)); shared.clone() }, uis: Rc::new(RefCell::new(Vec::new())), ids: Vec::new(), mods: (false, false), pointer: [(0.0, 0.0); 2], press: None, selftest: args.iter().any(|a| a == "--selftest"), stage: 0, at: Instant::now(), code: code.clone(), quit_after: arg("--quit-after").and_then(|v| v.parse().ok()) };
    let _ = el.run_app(app);
    shared.borrow_mut().say(None, "S16 left"); std::process::exit(code.get());
}
