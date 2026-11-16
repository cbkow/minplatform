//! One window's content (canon row 463's layout): the sidebar (Favorites, Shares, Jobs, All Tracked, the system's
//! places), ONE toolbar for the window that acts on the active pane (back, forward, up, the path bar, list / grid, the
//! grid's size, search, the second pane), one or two panes, the status bar. Drawn with the family's toolkit as
//! minRender's and minNotes' windows are: one vello scene over the presenter, the theme's tokens, min-ui's parts.
//!
//! Nothing here asks the file system for anything: the panes and the sidebar are fed by worker threads through one
//! channel (`lister::Port`), which also wakes the loop.
use crate::crumbs;
use crate::icons::Icons;
use crate::thumbs::Thumbs;
use crate::lister::{self, Msg, Port};
use crate::menu::{self, Cmd, InWindowMenu};
use crate::model::SortBy;
use crate::pane::{Load, Mode, Pane};
use crate::places::{self, Place};
use kurbo::{Affine, Rect};
use min_present::{Output, Presenter};
use min_theme::tokens as t;
use min_ui::{action_button::Response, fonts::draw_layout, grid, paint, section, tooltip::Tooltips, ActionButton, Button, Corners, Event, Family, FieldResponse, Fonts, Key, KeyEvent, Mods, Named, ScrollBar, Section, Slider, TextField, UiLayer};
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::mpsc::Receiver;
use std::sync::Arc;
use std::time::{Duration, Instant};
use vello::Scene;
use winit31::window::Window;

/// What the window asks of the application.
#[derive(Clone, Debug, PartialEq)]
pub enum Out { /// the clipboard is the application's to reach: files to put on it (`cut`: they are moved when pasted); a paste of files asked for
    /// (`mv`: "Move Item Here"); the text field in hand copies (`cut`) or pastes
    /// the system's drag of these files begins (a press on them was moved)
    Drag(Vec<PathBuf>),
    /// a file to hand to the system: it opens in its default application
    OpenDefault(PathBuf),
    CopyFiles { paths: Vec<PathBuf>, cut: bool }, PasteFiles { mv: bool }, FieldCopy { cut: bool }, FieldPaste,
    NewWindow(PathBuf), Close, Minimize, Quit, /// a context menu to show (the application's loop does: it is the system's)
    Menu(crate::ctxmenu::Ask) }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Focus { Pane, Path, Search, Rename }
/// A name being changed in place (row 437): the entry, its field over the name.
struct Renaming { pane: usize, path: PathBuf, dir: bool, field: TextField }
/// What an open question is about.
enum Asked { /// delete these for good
    Delete(Vec<PathBuf>), /// the extension would change: the entry, the name typed, the name with the old extension kept
    Extension { path: PathBuf, typed: String, kept: String }, /// the engine's question about a name that is taken (its number)
    Conflict(u64) }
#[derive(Clone, Copy, Debug, PartialEq)]
enum Drag { Sidebar(f64, f64), Split }
struct SideRow { rect: Rect, path: Option<PathBuf>, enabled: bool }

/// Logical px.
const LB_STRIP: f64 = 34.0; const TOOLBAR: f64 = 44.0; const STATUS: f64 = 26.0; const HEAD: f64 = 26.0; const SIDE_ROW: f64 = 28.0; const SIDE_MIN: f64 = 150.0; const SIDE_MAX: f64 = 420.0; const GAP: f64 = 5.0;

impl Drop for Ui { fn drop(&mut self) { crate::ops::unsubscribe(self.id); } }
pub struct Ui {
    window: Arc<Box<dyn Window>>, presenter: Presenter, layer: UiLayer, fonts: Fonts, tips: Tooltips,
    port: Port, rx: Receiver<Msg>, icons: Icons, thumbs: Thumbs,
    /// one pane, or two side by side; the one the toolbar, the sidebar and the menus act on; the first pane's share of the width
    panes: Vec<Pane>, active: usize, two: bool, split: f64, pane_heads: [Rect; 2],
    /// options of the window (both panes follow)
    hidden: bool, folders_first: bool,
    side_w: f64, side_rect: Rect, sections: [Section; 4], favorites: Vec<Place>, fixed: Vec<Place>, volumes: Vec<Place>, root: String, side_bar: ScrollBar, side_rows: Vec<SideRow>, side_hover: Option<usize>,
    back: ActionButton, forward: ActionButton, up: ActionButton, list_btn: ActionButton, grid_btn: ActionButton, pane2_btn: ActionButton, size: Slider, search: TextField,
    /// the path bar: its box; the segments as buttons with where each goes (none: the ellipsis) and what they were made for; the field while a path is typed; a path that was no folder
    path_rect: Rect, crumbs: Vec<(ActionButton, Option<PathBuf>)>, crumbs_for: Option<(PathBuf, String, i64)>, field: TextField, path_error: Option<(String, Instant)>, check: Option<u64>,
    focus: Focus, drag: Option<Drag>, gaps: [Rect; 2], over_gap: Option<usize>, cursor: winit31::cursor::CursorIcon,
    pub menubar: Option<InWindowMenu>, pointer: (f64, f64), pub mods: Mods,
    /// the scripted looks: the steps still to do, a wait, the picture to save, since when nothing is pending
    /// the window's title as it was last set: the active pane's folder
    titled: String,
    /// this window among the application's (a file operation's report says which window asked)
    id: u64,
    /// a name being changed in place; a question that is open; what went wrong, said at the window's foot;
    /// the entry to rename as soon as its folder is read again (a new folder goes straight into its name)
    rename: Option<Renaming>, dialog: Option<(min_ui::dialog::Dialog, Asked)>, toast: min_ui::toast::Toast, rename_next: Option<PathBuf>,
    /// the engine's questions that wait for the one that is open; the running operation in the status bar: its cancel button, the place a click opens the list at, the list open
    /// drags of files: where a drop can go (written every frame), what the layer left for the loop
    /// the lightbox (rows 452, 473): the file shown large; its close button; its last press (two make a double click)
    lightbox: crate::lightbox::Lightbox, lb_close: ActionButton, lb_press: Option<(Instant, (f64, f64))>, /// the contact sheet's tiles as they were drawn, and the one under the pointer
    lb_tiles: Vec<Rect>, lb_hover: Option<usize>, preview_btn: ActionButton,
    pub dnd: crate::dnd::Handle,
    /// after a drag of ours ended: the moments at which the panes ask again whether their folders changed (the
    /// application that took the files moves them AFTER the drag is over - the owner's walk, 2026-11-16)
    recheck: Vec<Instant>,
    asks: VecDeque<Arc<crate::ops::Question>>, op_cancel: ActionButton, op_rect: Rect, op_list: bool,
    /// what the context menu last asked for is about: the pane (none: the sidebar) and the folder its own rows act on
    menu_for: Option<(Option<usize>, PathBuf)>,
    demo: VecDeque<String>, demo_until: Option<Instant>, shot: Option<PathBuf>, settled: Option<Instant>, tip_demo: Option<String>, born: Instant,
    /// set when the scripted picture is saved: the application ends
    pub done: bool, pub out: Vec<Out>,
}

fn icon_button(icon: &'static str, tip: String) -> ActionButton { ActionButton::new(icon, &tip) }

impl Ui {
    pub fn new(window: Arc<Box<dyn Window>>, port: Port, rx: Receiver<Msg>, path: PathBuf, favorites: Vec<Place>) -> Option<Ui> {
        let instance = Presenter::instance(); let surface = instance.create_surface(window.clone()).ok()?; let size = window.surface_size();
        #[cfg(target_os = "macos")] let view = { use raw_window_handle::{HasWindowHandle, RawWindowHandle}; match window.window_handle().ok()?.as_raw() { RawWindowHandle::AppKit(h) => Some(h.ns_view.as_ptr()), _ => None } };
        #[cfg(not(target_os = "macos"))] let view = None;
        let mut presenter = Presenter::new(&instance, surface, (size.width.max(1), size.height.max(1)), Output::Auto, view).ok()?; presenter.engine.set_ui_scale(window.scale_factor() as f32);
        let layer = UiLayer::new(&presenter.engine.device, size.width.max(1), size.height.max(1)).ok()?; presenter.engine.set_overlay(Some(layer.view()));
        let mut search = TextField::new(Family::Sans, t::BODY_SIZE_S); search.placeholder = "Search".into();
        let pane = Pane::new(path, &port); places::refresh(&port);
        // (the scripted looks are the FIRST window's: a window they open does not run them again)
        static FIRST: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true); let first = FIRST.swap(false, std::sync::atomic::Ordering::Relaxed);
        let demo = std::env::var("UFB_DEMO").ok().filter(|_| first).map(|d| d.split(';').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect()).unwrap_or_default();
        let key = |c: Cmd, text: &str| menu::tip(text, c);
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1); let id = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        crate::ops::subscribe(id, port.clone());
        let lightbox = { let p = port.clone(); crate::lightbox::Lightbox::new(Arc::new(move || p.send(Msg::Icon))) };
        Some(Ui { window, presenter, layer, fonts: Fonts::new(), tips: Tooltips::default(), icons: Icons::new(port.clone()), thumbs: { let p = port.clone(); Thumbs::new(std::sync::Arc::new(move || p.send(Msg::Icon))) }, port, rx,
            panes: vec![pane], active: 0, two: false, split: 0.5, pane_heads: [Rect::ZERO; 2], hidden: false, folders_first: true,
            side_w: t::RAIL_WIDTH_MIN as f64, side_rect: Rect::ZERO, sections: [Section::new("Favorites", true), Section::new("Shares", true), Section::new("Jobs", true), Section::new("Places", true)], favorites, fixed: places::fixed(), volumes: vec![], root: String::new(), side_bar: ScrollBar::default(), side_rows: vec![], side_hover: None,
            back: icon_button("arrow-left", key(Cmd::Back, "Back")), forward: icon_button("arrow-right", key(Cmd::Forward, "Forward")), up: icon_button("arrow-up", key(Cmd::Up, "Enclosing folder")),
            list_btn: icon_button("list", key(Cmd::AsList, "List")), grid_btn: icon_button("squares-four", key(Cmd::AsGrid, "Grid")), pane2_btn: icon_button("columns", "Second pane  (Tab switches panes)".to_string()), size: Slider::new(((96.0 - grid::TILE_MIN) / (grid::TILE_MAX - grid::TILE_MIN)) as f32), search,
            path_rect: Rect::ZERO, crumbs: vec![], crumbs_for: None, field: TextField::new(Family::Sans, t::BODY_SIZE_S), path_error: None, check: None,
            focus: Focus::Pane, drag: None, gaps: [Rect::ZERO; 2], over_gap: None, cursor: winit31::cursor::CursorIcon::Default,
            menubar: InWindowMenu::wanted().then(InWindowMenu::new), pointer: (0.0, 0.0), mods: Mods::default(),
            titled: String::new(), id, rename: None, dialog: None, toast: min_ui::toast::Toast::new(), rename_next: None, lightbox, lb_close: ActionButton::new("x", "Close  (Space)"), lb_press: None, lb_tiles: vec![], lb_hover: None, preview_btn: icon_button("eye", "Preview  (Space)".to_string()),
            dnd: Default::default(), recheck: vec![], asks: VecDeque::new(), op_cancel: ActionButton::new("x", "Stop").size_s(), op_rect: Rect::ZERO, op_list: false, menu_for: None, demo, demo_until: None, shot: std::env::var_os("UFB_SHOT").filter(|_| first).map(PathBuf::from), settled: None, tip_demo: None, born: Instant::now(), done: false, out: vec![] })
    }
    pub fn resize(&mut self, w: u32, h: u32) { if w == 0 || h == 0 { return } self.presenter.resize(w, h); self.presenter.engine.set_ui_scale(self.window.scale_factor() as f32); self.layer.resize(&self.presenter.engine.device, w, h); self.presenter.engine.set_overlay(Some(self.layer.view())); if let Some(mb) = self.menubar.as_mut() { mb.bar.cancel(); } }
    fn scale(&self) -> f64 { self.window.scale_factor() }
    fn pane(&mut self) -> &mut Pane { let i = self.active.min(self.panes.len() - 1); &mut self.panes[i] }
    fn shown(&self) -> usize { if self.two { self.panes.len().min(2) } else { 1 } }
    /// What the menus show of this window.
    pub fn menu_state(&self) -> menu::State { let p = &self.panes[self.active.min(self.panes.len() - 1)]; menu::State { grid: p.mode == Mode::Grid, hidden: self.hidden, folders_first: self.folders_first, second: self.two, sort: p.model.sort, can_back: p.history.can_back(), can_forward: p.history.can_forward(), can_up: p.history.parent().is_some(),
        chosen: p.chosen_count().min(2) as u8, undo: crate::ops::undo_label().is_some(), writable: crate::ops::allowed_in(p.path()),
        typing: self.focus != Focus::Pane, other: self.two && self.panes.len() > 1 && crate::ops::allowed_in(self.panes[1 - self.active.min(1)].path()) } }

    // ---------------------------------------------------------------- what the workers send
    /// Take in what the worker threads sent. True when anything came.
    pub fn poll(&mut self) -> bool {
        let mut any = self.icons.poll(); any |= self.thumbs.poll(); any |= self.lightbox.poll();
        // what a drag left for the loop: drops to carry out (the engine's work, as a paste is); a drag of ours that ended
        let (drops, ended) = { let mut d = self.dnd.borrow_mut(); (std::mem::take(&mut d.drops), std::mem::take(&mut d.ended)) };
        for d in drops { any = true; crate::ops::submit(self.id, crate::ops::Op::Transfer { paths: d.paths, dest: d.dest, mv: d.mv }); }
        // a drag of ours ended: the panes ask whether their folders changed - now, and a few times more, because the
        // application that took the files moves them after the drag is over (no watcher, row 438: a bounded look after OUR OWN drag)
        let now = Instant::now();
        if ended { any = true; self.recheck = [0u64, 250, 700, 1500, 3000, 6000].iter().map(|ms| now + Duration::from_millis(*ms)).collect(); }
        if self.recheck.first().is_some_and(|t| *t <= now) { self.recheck.retain(|t| *t > now); any = true; let port = self.port.clone(); let n = self.shown(); for p in self.panes.iter_mut().take(n) { p.activated(&port); } }
        while let Ok(m) = self.rx.try_recv() {
            any = true;
            match m {
                Msg::Places { volumes, root } => { self.volumes = volumes; self.root = root; self.crumbs_for = None; }
                Msg::Checked { req, path, dir } => if self.check == Some(req) { self.check = None; if dir { self.leave_path(); let port = self.port.clone(); self.pane().go(&port, path); } else { self.path_error = Some(("No such folder".into(), Instant::now())); self.field.error = true; } },
                Msg::Icon => { self.icons.poll(); self.thumbs.poll(); self.lightbox.poll(); }
                Msg::Op(rep) => self.on_report(&rep),
                Msg::Ask(q) => if q.origin == self.id { self.asks.push_back(q); },
                other => { let mut m = Some(other); let port = self.port.clone(); for p in self.panes.iter_mut() { if p.on_msg(&port, &mut m) { if std::mem::take(&mut p.listed) { self.thumbs.listed(p.path()); } break } } }
            }
        }
        any
    }
    /// The window became active: the volumes are looked for again, and each pane asks whether its folder changed.
    pub fn activated(&mut self) { places::refresh(&self.port); let port = self.port.clone(); let n = self.shown(); for p in self.panes.iter_mut().take(n) { p.activated(&port); } }
    pub fn deactivated(&mut self) { if let Some(mb) = self.menubar.as_mut() { mb.bar.cancel(); } }

    // ---------------------------------------------------------------- the lightbox (`lightbox.rs`)
    /// The file at this row of a pane, large (a folder has none).
    fn open_lightbox(&mut self, pane: usize, row: usize) {
        let Some((path, name, size, kind, dir)) = self.panes.get(pane).and_then(|p| p.entry(row)) else { return }; if dir { return }
        self.end_rename(); self.leave_path(); self.leave_search(); self.lb_press = None;
        let ext = crate::lightbox::ext_of(&path); self.lightbox.open(path, name, size, kind, &ext, pane);
    }
    fn close_lightbox(&mut self) {
        if !self.lightbox.is_open() { return } self.lightbox.close();
        let e = &mut self.presenter.engine; e.clear_picture(); e.set_place(None); e.set_background(0);
    }
    /// The next file of the folder (or the one before): the pane's cursor goes with it.
    fn step_lightbox(&mut self, step: i32) {
        let Some((pane, path)) = self.lightbox.view.as_ref().map(|v| (v.pane, v.path.clone())) else { return };
        let Some(p) = self.panes.get_mut(pane) else { return }; let Some(next) = p.row_at_path(&path).and_then(|r| p.next_file(r, step)) else { return };
        p.choose_row(next); self.open_lightbox(pane, next);
    }
    /// Where the picture may lie: the window above the lightbox's strip.
    fn lightbox_area(&self) -> Rect { let size = self.window.surface_size(); Rect::new(0.0, 0.0, size.width as f64, (size.height as f64 - LB_STRIP * self.scale()).max(1.0)) }
    fn lightbox_pointer(&mut self, e: &Event) {
        let now = Instant::now(); let area = self.lightbox_area(); let s = self.scale();
        if self.lb_close.event(e, now) == Response::Clicked { self.close_lightbox(); return }
        // the contact sheet of an EXR's layers: a press on one shows it large
        if self.lightbox.view.as_ref().is_some_and(|v| v.state == crate::lightbox::State::Layers) {
            self.lb_hover = e.pos().and_then(|(x, y)| self.lb_tiles.iter().position(|r| r.contains(kurbo::Point::new(x, y))));
            if let (Event::Down { button: Button::Primary, .. }, Some(i)) = (*e, self.lb_hover) { self.lb_hover = None; self.lightbox.pick_layer(i); }
            return
        }
        let Some(v) = self.lightbox.view.as_mut() else { return };
        match *e {
            Event::Down { x, y, button: Button::Primary } if area.contains(kurbo::Point::new(x, y)) && !self.lb_close.hit(x, y) => {
                // two presses close together: actual size / fitted; one: the picture is taken hold of
                if self.lb_press.take().is_some_and(|(t, at)| now.duration_since(t) < Duration::from_millis(400) && (at.0 - x).hypot(at.1 - y) < 6.0 * s) { v.drag_end(); v.toggle((x, y), area); } else { self.lb_press = Some((now, (x, y))); v.drag_begin((x, y)); }
            }
            Event::Move { x, y } => { v.drag_to((x, y), area); }
            Event::Up { button: Button::Primary, .. } | Event::Leave => v.drag_end(),
            _ => {}
        }
    }
    fn lightbox_key(&mut self, k: &KeyEvent, name: &str) {
        let area = self.lightbox_area(); let mid = (area.center().x, area.center().y);
        // Escape (and Backspace) step back from one layer to the sheet of layers first (1.4.2's keys)
        if matches!(name, "Escape" | "Backspace") && self.lightbox.back_to_layers() { let e = &mut self.presenter.engine; e.clear_picture(); return }
        match name { "Space" | "Escape" => return self.close_lightbox(), "Left" | "Up" => return self.step_lightbox(-1), "Right" | "Down" => return self.step_lightbox(1), _ => {} }
        let Some(v) = self.lightbox.view.as_mut() else { return };
        match &k.key { Key::Char(c) => match c.as_str() { "0" | "f" | "F" => v.fit(), "1" => v.actual(mid, area), "=" | "+" => v.zoom_by(1.25, mid, area), "-" | "_" => v.zoom_by(0.8, mid, area), _ => {} }, _ => {} }
    }
    /// The wheel (or two fingers) zooms about the pointer, as in QCView's viewer.
    fn lightbox_wheel(&mut self, dy: f64) { let (area, at) = (self.lightbox_area(), self.pointer); if let Some(v) = self.lightbox.view.as_mut() { v.zoom_by((-dy * 0.004).exp().clamp(0.5, 2.0), at, area); } }
    /// A pinch on a trackpad: `delta` is the change of size (0.1: a tenth larger).
    pub fn pinch(&mut self, delta: f64) { let (area, at) = (self.lightbox_area(), self.pointer); if let Some(v) = self.lightbox.view.as_mut() { v.zoom_by((1.0 + delta).clamp(0.5, 2.0), at, area); } }
    /// The lightbox drawn: the picture is the presenter's (under this layer); the layer holds only the strip, the close
    /// button and, while there is no picture, what to say instead.
    fn redraw_lightbox(&mut self) {
        let s = self.scale(); let size = self.window.surface_size(); let (w, h) = (size.width as f64, size.height as f64); let now = Instant::now(); let area = self.lightbox_area();
        // a picture that has come goes to the presenter: on a dark ground, or on a checker when it has transparency
        if let Some((frame, alpha)) = self.lightbox.take_frame() {
            match self.presenter.engine.show(&frame) { Ok(()) => self.presenter.engine.set_background(if alpha { 2 } else { 1 }), Err(e) => if let Some(v) = self.lightbox.view.as_mut() { v.state = crate::lightbox::State::Failed(format!("This file could not be shown: {e}.")); } }
        }
        let Some(v) = self.lightbox.view.as_ref() else { return };
        let ready = v.info().is_some();
        if ready { self.presenter.engine.set_place(v.place(area).map(|r| [r.x0 as f32, r.y0 as f32, r.width() as f32, r.height() as f32])); } else { self.presenter.engine.clear_picture(); }
        let (ink, sub) = (paint::color(t::NEUTRAL_CONTENT_COLOR_DEFAULT), paint::color(t::NEUTRAL_SUBDUED_CONTENT_COLOR_DEFAULT));
        let scene: &mut Scene = self.layer.scene_mut(); scene.reset(); let fonts = &mut self.fonts;
        // around the picture the window is plain: the presenter's checker is for what shows THROUGH the picture only
        if let Some(r) = v.place(area).map(|r| r.intersect(area)).filter(|_| ready) {
            for q in [Rect::new(0.0, 0.0, w, r.y0), Rect::new(0.0, r.y1, w, area.y1), Rect::new(0.0, r.y0, r.x0, r.y1), Rect::new(r.x1, r.y0, w, r.y1)] { if q.width() > 0.0 && q.height() > 0.0 { paint::rect(scene, q, t::BACKGROUND_LAYER_1_COLOR); } }
        }
        self.lb_tiles.clear();
        if v.state == crate::lightbox::State::Layers {
            // the contact sheet: the layers in a grid that fills the window, each with its name
            paint::rect(scene, Rect::new(0.0, 0.0, w, h), t::BACKGROUND_LAYER_1_COLOR);
            let n = v.layers.len().max(1); let pad = 28.0 * s; let label = 26.0 * s; let room = Rect::new(pad, pad + 30.0 * s, w - pad, area.y1 - pad);
            let cols = (1..=n).find(|c| { let rows = n.div_ceil(*c); let cw = room.width() / *c as f64; (cw * 9.0 / 16.0 + label) * rows as f64 <= room.height() }).unwrap_or(n); let rows = n.div_ceil(cols);
            let (cw, chh) = (room.width() / cols as f64, room.height() / rows as f64); let gap = 10.0 * s;
            for (i, (lname, img)) in v.layers.iter().enumerate() {
                let cell = Rect::new(room.x0 + (i % cols) as f64 * cw + gap, room.y0 + (i / cols) as f64 * chh + gap, room.x0 + (i % cols + 1) as f64 * cw - gap, room.y0 + (i / cols + 1) as f64 * chh - gap);
                if self.lb_hover == Some(i) { paint::rounded(scene, cell.inflate(5.0 * s, 5.0 * s), t::CORNER_RADIUS_MEDIUM_DEFAULT as f64 * s, t::HOVER_BACKGROUND_COLOR); }
                let pic = Rect::new(cell.x0, cell.y0, cell.x1, cell.y1 - label);
                match img { Some(img) => { let (iw, ih) = (img.image.width.max(1) as f64, img.image.height.max(1) as f64); let k = (pic.width() / iw).min(pic.height() / ih); let (pw, ph) = (iw * k, ih * k); paint::image(scene, img, Rect::new(pic.center().x - pw / 2.0, pic.center().y - ph / 2.0, pic.center().x + pw / 2.0, pic.center().y + ph / 2.0)); }
                    None => paint::rounded(scene, pic.inflate(-pic.width() * 0.3, -pic.height() * 0.3), 4.0 * s, t::GRAY_300) }
                let l = fonts.layout_fit(lname, Family::Sans, t::BODY_SIZE_S * s as f32, t::FONT_WEIGHT_REGULAR as f32, ink, cell.width()); draw_layout(scene, &l, Affine::translate((cell.center().x - l.width() as f64 / 2.0, cell.y1 - label + (label - l.height() as f64) / 2.0)));
                self.lb_tiles.push(cell);
            }
        } else if !ready {
            paint::rect(scene, Rect::new(0.0, 0.0, w, h), t::BACKGROUND_LAYER_1_COLOR);
            let text = match &v.state { crate::lightbox::State::Loading => if now.duration_since(v.since) > Duration::from_millis(250) { "Loading\u{2026}".to_string() } else { String::new() }, crate::lightbox::State::Failed(why) => why.clone(), _ => "No preview of this kind yet. Open it with the Open command.".to_string() };
            if !text.is_empty() { let l = fonts.layout_fit(&text, Family::Sans, t::BODY_SIZE_M * s as f32, t::FONT_WEIGHT_REGULAR as f32, sub, w - 80.0 * s); draw_layout(scene, &l, Affine::translate(((w - l.width() as f64) / 2.0, area.center().y - l.height() as f64 / 2.0))); }
        }
        // the strip: the name, then what the picture is
        let strip = Rect::new(0.0, area.y1, w, h); paint::rect(scene, strip, t::BACKGROUND_LAYER_2_COLOR); paint::hline(scene, 0.0, w, strip.y0, s, t::DIVIDER_COLOR);
        let size_px = t::BODY_SIZE_S * s as f32;
        let facts = fonts.layout_fit(&v.facts(area), Family::Sans, size_px, t::FONT_WEIGHT_REGULAR as f32, sub, (w * 0.62).max(80.0 * s)); let fx = w - 14.0 * s - facts.width() as f64;
        draw_layout(scene, &facts, Affine::translate((fx, strip.y0 + (strip.height() - facts.height() as f64) / 2.0)));
        let name = fonts.layout_fit(&v.name, Family::Sans, size_px, t::FONT_WEIGHT_BOLD as f32, ink, (fx - 28.0 * s).max(60.0 * s));
        draw_layout(scene, &name, Affine::translate((14.0 * s, strip.y0 + (strip.height() - name.height() as f64) / 2.0)));
        // the close button, clear of the window's own buttons: top right
        let bh = t::ACTION_BUTTON_HEIGHT_M as f64 * s; self.lb_close.rect = Rect::new(w - bh - 12.0 * s, 12.0 * s, w - 12.0 * s, 12.0 * s + bh); self.lb_close.corners = Corners::all(t::ACTION_BUTTON_CORNER_RADIUS_M as f64 * s);
        paint::rounded(scene, self.lb_close.rect, t::ACTION_BUTTON_CORNER_RADIUS_M as f64 * s, t::BACKGROUND_LAYER_2_COLOR); self.lb_close.paint(scene, fonts, s, now);
        self.present(now);
    }

    // ---------------------------------------------------------------- file operations (`ops.rs`)
    /// An operation ended (in this window or another): the panes that show a folder it changed read it again; the
    /// window that asked chooses what was made, says what went wrong, and asks what is left to ask.
    fn on_report(&mut self, rep: &crate::ops::Report) {
        let mine = rep.origin == self.id; let port = self.port.clone(); let now = Instant::now();
        for (i, p) in self.panes.iter_mut().enumerate() {
            if !rep.changed.iter().any(|c| c.starts_with(p.path())) { continue }
            let here: Vec<String> = if mine && i == self.active { rep.made.iter().filter(|m| m.parent() == Some(p.path())).filter_map(|m| m.file_name()).map(|n| n.to_string_lossy().into_owned()).collect() } else { vec![] };
            p.refresh_choosing(&port, here);
        }
        if self.rename.as_ref().is_some_and(|r| rep.changed.iter().any(|c| r.path.starts_with(c))) { self.end_rename(); }
        if !mine { return }
        if let Some(why) = &rep.error { self.toast.show(why, min_ui::toast::ToastKind::Negative, now); }
        // a new folder goes straight into its name (row 437)
        if rep.kind == Some(crate::ops::Kind::NewFolder) { self.rename_next = rep.made.first().cloned(); }
        if !rep.no_trash.is_empty() { self.ask_delete(rep.no_trash.clone(), true); }
    }
    /// The engine asks about a name that is taken (rows 436 / 468): replace, skip or keep both - for this one or for all
    /// that follow - or stop. Return gives "Keep Both" (nothing is lost by it), Escape stops.
    fn show_question(&mut self) {
        use min_ui::{dialog::Dialog, text_button::Variant};
        if self.dialog.is_some() { return } let Some(q) = self.asks.pop_front() else { return }; let c = &q.conflict;
        let folder = c.dest.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| c.dest.to_string_lossy().into_owned());
        let mut d = Dialog::new(&format!("An item named \u{201c}{}\u{201d} already exists in \u{201c}{folder}\u{201d}.", c.name), &format!("Replace it with the one you are {}, skip this one, or keep both?", if c.mv { "moving" } else { "copying" }),
            vec![("Stop", Variant::Secondary), ("Replace", Variant::Secondary), ("Skip", Variant::Secondary), ("Keep Both", Variant::Primary)]).with_check("Apply to all"); d.show();
        self.dialog = Some((d, Asked::Conflict(q.id)));
    }
    /// The chosen entries are copied (`mv`: moved) into the other pane's folder (row 450: in the menus, with no key).
    fn to_other_pane(&mut self, mv: bool) {
        if !self.two || self.panes.len() < 2 { return } let dest = self.panes[1 - self.active.min(1)].path().to_path_buf(); let paths = self.chosen_paths();
        if !paths.is_empty() { crate::ops::submit(self.id, crate::ops::Op::Transfer { paths, dest, mv }); }
    }
    /// Files from the clipboard are put into the active pane's folder (`mv`: moved there).
    pub fn paste_files(&mut self, paths: Vec<PathBuf>, mv: bool) {
        if paths.is_empty() || self.focus != Focus::Pane || crate::ops::status().is_some() || self.lightbox.loading() || self.lightbox.view.as_ref().is_some_and(|v| v.dragging()) || self.lb_close.animating(Instant::now()) || !self.recheck.is_empty() || !self.asks.is_empty() || self.dialog.is_some() { return } let dest = self.pane().path().to_path_buf();
        crate::ops::submit(self.id, crate::ops::Op::Transfer { paths, dest, mv });
    }
    fn chosen_paths(&mut self) -> Vec<PathBuf> { self.pane().chosen_entries(usize::MAX).into_iter().map(|e| e.0).collect() }
    /// "Delete for good?" - always asked (row 468, point 3). `no_trash`: asked because the volume has no Trash.
    fn ask_delete(&mut self, paths: Vec<PathBuf>, no_trash: bool) {
        use min_ui::{dialog::Dialog, text_button::Variant};
        if paths.is_empty() { return }
        let what = if paths.len() == 1 { format!("\u{201c}{}\u{201d}", paths[0].file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()) } else { format!("{} items", crate::pane::group(paths.len())) };
        let body = if no_trash { "This volume has no Trash. What is deleted here is gone at once and cannot be brought back." } else { "This cannot be undone." };
        // (the last button is the one Return gives: it is Cancel)
        let mut d = Dialog::new(&format!("Delete {what} immediately?"), body, vec![("Delete", Variant::Negative), ("Cancel", Variant::Primary)]); d.show();
        self.dialog = Some((d, Asked::Delete(paths)));
    }
    /// A button of the open question was chosen (`button`: its place; the last one is the careful answer).
    fn answer(&mut self, button: usize) {
        let Some((d, asked)) = self.dialog.take() else { return };
        match asked {
            Asked::Conflict(id) => { use crate::ops::Choice; crate::ops::answer(id, [Choice::Stop, Choice::Replace, Choice::Skip, Choice::KeepBoth][button.min(3)], d.checked()); }
            Asked::Delete(paths) => if button == 0 { crate::ops::submit(self.id, crate::ops::Op::Delete { paths }); },
            Asked::Extension { path, typed, kept } => { let name = if button == 0 { typed } else { kept }; if path.file_name().is_some_and(|n| n.to_string_lossy() != name.as_str()) { crate::ops::submit(self.id, crate::ops::Op::Rename { path, name }); } }
        }
    }
    /// Rename in place (row 437): the one chosen entry's name becomes a field, the name chosen without its extension.
    fn start_rename(&mut self) {
        let i = self.active.min(self.panes.len() - 1); let entries = self.panes[i].chosen_entries(2); if entries.len() != 1 { return }
        let (path, dir, _) = entries.into_iter().next().unwrap(); let Some(name) = path.file_name().map(|n| n.to_string_lossy().into_owned()) else { return };
        if !crate::ops::allowed_in(path.parent().unwrap_or(&path)) { self.toast.show(crate::ops::GUARDED, min_ui::toast::ToastKind::Notice, Instant::now()); return }
        let mut field = TextField::new(Family::Sans, t::BODY_SIZE_S); field.set_text(&name); field.focused = true;
        let stem = if dir { name.len() } else { match name.rfind('.') { Some(k) if k > 0 => k, _ => name.len() } };
        field.select_range(0, stem, &mut self.fonts);
        self.leave_path(); self.leave_search(); self.focus = Focus::Rename; self.rename = Some(Renaming { pane: i, path, dir, field });
    }
    fn end_rename(&mut self) { self.rename = None; if self.focus == Focus::Rename { self.focus = Focus::Pane; } }
    /// The field's name is taken. A name that cannot be given, or that another entry has, is refused and the field
    /// stays open; a changed extension is asked about first, as Finder asks (Windows does not).
    fn commit_rename(&mut self) {
        let Some(r) = self.rename.as_mut() else { return }; let now = Instant::now();
        let typed = r.field.text().to_string(); let old = r.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        if typed == old || typed.is_empty() { self.end_rename(); return }
        let fault = crate::ops::name_fault(&typed).or_else(|| self.panes.get(r.pane).is_some_and(|p| p.name_taken(&r.path, &typed)).then(|| crate::ops::taken(&typed)));
        if let Some(why) = fault { r.field.error = true; self.toast.show(&why, min_ui::toast::ToastKind::Negative, now); return }
        let (path, dir) = (r.path.clone(), r.dir); self.end_rename();
        let ext = |n: &str| match n.rfind('.') { Some(k) if k > 0 && k + 1 < n.len() => n[k..].to_string(), _ => String::new() };
        let (was, is) = (ext(&old), ext(&typed));
        if cfg!(target_os = "macos") && !dir && was.to_lowercase() != is.to_lowercase() && !was.is_empty() {
            use min_ui::{dialog::Dialog, text_button::Variant};
            let stem = if is.is_empty() { typed.as_str() } else { &typed[..typed.len() - is.len()] }; let kept = format!("{stem}{was}");
            let (title, change) = if is.is_empty() { (format!("Are you sure you want to remove the extension \u{201c}{was}\u{201d}?"), "Remove".to_string()) } else { (format!("Are you sure you want to change the extension from \u{201c}{was}\u{201d} to \u{201c}{is}\u{201d}?"), format!("Use {is}")) };
            let mut d = Dialog::new(&title, "If you make this change, your file may open in a different application.", vec![(change.as_str(), Variant::Secondary), (format!("Keep {was}").as_str(), Variant::Primary)]); d.show();
            self.dialog = Some((d, Asked::Extension { path, typed, kept })); return
        }
        crate::ops::submit(self.id, crate::ops::Op::Rename { path, name: typed });
    }

    // ---------------------------------------------------------------- commands
    fn go(&mut self, path: PathBuf) { let port = self.port.clone(); self.pane().go(&port, path); }
    fn set_two(&mut self, on: bool) {
        self.two = on;
        if on && self.panes.len() < 2 { let path = self.panes[0].path().to_path_buf(); let mut p = Pane::new(path, &self.port); p.set_hidden(self.hidden); p.set_folders_first(self.folders_first); self.panes.push(p); }
        if !on { self.active = 0; }
    }
    fn edit_path(&mut self) {
        let text = self.panes[self.active.min(self.panes.len() - 1)].path().to_string_lossy().into_owned();
        self.field.set_text(&text); self.field.focused = true; self.field.error = false; self.field.select_all(&mut self.fonts); self.search.focused = false; self.focus = Focus::Path; self.path_error = None;
    }
    fn leave_path(&mut self) { if self.focus == Focus::Path { self.focus = Focus::Pane; } self.field.focused = false; self.field.error = false; self.path_error = None; self.check = None; }
    fn submit_path(&mut self) {
        let current = self.panes[self.active.min(self.panes.len() - 1)].path().to_path_buf();
        match crumbs::typed_path(self.field.text(), &places::home(), &current) {
            // whether it is a folder is asked off this thread; the answer navigates or shows the error
            Some(p) => { let req = lister::next_req(); self.check = Some(req); self.path_error = None; self.field.error = false; lister::check(&self.port, req, p); }
            None => self.leave_path(),
        }
    }
    fn focus_search(&mut self) { self.leave_path(); self.focus = Focus::Search; self.search.focused = true; self.search.select_all(&mut self.fonts); }
    fn leave_search(&mut self) { if self.focus == Focus::Search { self.focus = Focus::Pane; } self.search.focused = false; }
    pub fn command(&mut self, c: Cmd) {
        let port = self.port.clone();
        // while the lightbox is open the window's own commands and Preview act; the browser's wait
        if self.lightbox.is_open() && !matches!(c, Cmd::Preview | Cmd::CloseWindow | Cmd::Minimize | Cmd::Quit | Cmd::NewWindow) { return }
        // the commands that change files act on the pane's choice: not while a name, a path or a search is being typed,
        // and not under an open question (the system's menu keys would otherwise reach them from a text field)
        if self.dialog.is_some() && matches!(c, Cmd::Copy | Cmd::Cut | Cmd::Paste | Cmd::PasteMove | Cmd::CopyToOther | Cmd::MoveToOther) { return }
        if matches!(c, Cmd::NewFolder | Cmd::Rename | Cmd::Duplicate | Cmd::Trash | Cmd::DeleteNow | Cmd::Undo | Cmd::PasteMove | Cmd::CopyToOther | Cmd::MoveToOther) && (self.focus != Focus::Pane || self.dialog.is_some()) { return }
        match c {
            // copy and paste are the text field's while one is in hand; else they are about files (the clipboard is the application's to reach)
            Cmd::Copy | Cmd::Cut => if self.focus != Focus::Pane { self.out.push(Out::FieldCopy { cut: c == Cmd::Cut }); } else { let paths = self.chosen_paths(); if !paths.is_empty() { self.out.push(Out::CopyFiles { paths, cut: c == Cmd::Cut }); } },
            Cmd::Paste => self.out.push(if self.focus != Focus::Pane { Out::FieldPaste } else { Out::PasteFiles { mv: false } }),
            Cmd::PasteMove => self.out.push(Out::PasteFiles { mv: true }),
            Cmd::CopyToOther => self.to_other_pane(false), Cmd::MoveToOther => self.to_other_pane(true),
            Cmd::NewFolder => { let dir = self.pane().path().to_path_buf(); crate::ops::submit(self.id, crate::ops::Op::NewFolder { dir }); }
            Cmd::Rename => self.start_rename(),
            Cmd::Duplicate => { let paths = self.chosen_paths(); if !paths.is_empty() { crate::ops::submit(self.id, crate::ops::Op::Duplicate { paths }); } }
            Cmd::Trash => { let paths = self.chosen_paths(); if !paths.is_empty() { crate::ops::submit(self.id, crate::ops::Op::Trash { paths }); } }
            Cmd::DeleteNow => { let paths = self.chosen_paths(); self.ask_delete(paths, false); }
            Cmd::Undo => crate::ops::submit(self.id, crate::ops::Op::Undo),
            Cmd::NewWindow => { let p = self.pane().path().to_path_buf(); self.out.push(Out::NewWindow(p)); }
            Cmd::CloseWindow => self.out.push(Out::Close), Cmd::Minimize => self.out.push(Out::Minimize), Cmd::Quit => self.out.push(Out::Quit),
            Cmd::Open => { self.pane().open_chosen(&port); self.after_pane_events(); }
            Cmd::Preview => if self.lightbox.is_open() { self.close_lightbox(); } else if self.focus == Focus::Pane && self.dialog.is_none() { let i = self.active.min(self.panes.len() - 1); if let Some(row) = self.panes[i].cursor_row() { self.open_lightbox(i, row); } },
            Cmd::SelectAll => match self.focus { Focus::Path => self.field.select_all(&mut self.fonts), Focus::Search => self.search.select_all(&mut self.fonts), Focus::Rename => if let Some(r) = self.rename.as_mut() { r.field.select_all(&mut self.fonts); }, Focus::Pane => self.pane().select_all() },
            Cmd::AsList => self.pane().set_mode(Mode::List), Cmd::AsGrid => self.pane().set_mode(Mode::Grid),
            Cmd::Hidden => { self.hidden = !self.hidden; let on = self.hidden; for p in self.panes.iter_mut() { p.set_hidden(on); } }
            Cmd::FoldersFirst => { self.folders_first = !self.folders_first; let on = self.folders_first; for p in self.panes.iter_mut() { p.set_folders_first(on); } }
            Cmd::SecondPane => self.set_two(!self.two),
            Cmd::Sort(i) => { let by = SortBy::of_column(i as usize); let p = self.pane(); let asc = if p.model.sort == by { !p.model.ascending } else { true }; p.set_sort(by, asc); }
            Cmd::Refresh => { let dir = self.pane().path().to_path_buf(); self.thumbs.retry(&dir); self.pane().refresh(&port); places::refresh(&port); }
            Cmd::Back => self.pane().back(&port), Cmd::Forward => self.pane().forward(&port), Cmd::Up => self.pane().up(&port),
            Cmd::Home => self.go(places::home()), Cmd::Desktop => self.go(places::home().join("Desktop")), Cmd::Downloads => self.go(places::home().join("Downloads")),
            Cmd::GoToFolder => self.edit_path(), Cmd::Find => self.focus_search(),
        }
    }

    // ---------------------------------------------------------------- input
    pub fn wheel(&mut self, dy: f64) {
        if self.lightbox.is_open() { return self.lightbox_wheel(dy) }
        let now = Instant::now(); let p = kurbo::Point::new(self.pointer.0, self.pointer.1);
        if self.menubar.as_ref().is_some_and(|m| m.bar.is_open()) { return }
        if self.side_rect.contains(p) { self.side_bar.wheel(dy, now); return }
        let n = self.shown(); for pane in self.panes.iter_mut().take(n) { if pane.rect.contains(p) { pane.wheel(dy, now); return } }
    }
    /// The folder under the pointer that a new window can be opened on: a sidebar row, a segment of the path, and -
    /// `panes` - a folder in a pane.
    fn folder_at(&self, x: f64, y: f64, panes: bool) -> Option<PathBuf> {
        let p = kurbo::Point::new(x, y);
        if self.side_rect.contains(p) { return if self.sections.iter().any(|sec| sec.hit(x, y)) { None } else { self.side_rows.iter().find(|r| r.rect.contains(p) && r.enabled).and_then(|r| r.path.clone()) } }
        if self.focus != Focus::Path { if let Some(to) = self.crumbs.iter().find(|(b, path)| path.is_some() && b.hit(x, y)).and_then(|(_, path)| path.clone()) { return Some(to) } }
        if panes { for pane in self.panes.iter().take(self.shown()) { if pane.rect.contains(p) { return pane.folder_at(x, y) } } }
        None
    }
    /// A context menu for what is chosen in pane `i` (`row`: the click was on an entry), or for its folder.
    fn ask_menu(&mut self, i: usize, row: Option<usize>, at: (f64, f64)) {
        use crate::ctxmenu::{self, Ask};
        let pane = &self.panes[i]; let here = pane.path().to_path_buf();
        let entries = if row.is_some() { pane.chosen_entries(2000) } else { vec![] };
        let ask = if entries.is_empty() { self.menu_for = Some((Some(i), here.clone())); Ask { at, paths: vec![here], rows: ctxmenu::rows_for_folder() } }
            else { let kinds: Vec<(bool, bool)> = entries.iter().map(|e| (e.1, e.2)).collect(); let folder = if entries.len() == 1 && (entries[0].1 || entries[0].2) { entries[0].0.clone() } else { here };
                self.menu_for = Some((Some(i), folder)); Ask { at, paths: entries.into_iter().map(|e| e.0).collect(), rows: ctxmenu::rows_for(&kinds) } };
        self.out.push(Out::Menu(ask));
    }
    /// A row of OUR OWN in the context menu was chosen.
    pub fn menu_chosen(&mut self, id: u32) {
        use crate::ctxmenu::{CONTENTS, COPY, DUPLICATE, NEW_FOLDER, NEW_WINDOW, OPEN, PASTE, PREVIEW, REFRESH, RENAME, TRASH};
        let Some((pane, folder)) = self.menu_for.clone() else { return };
        match id {
            OPEN | CONTENTS => { if let Some(i) = pane.filter(|i| *i < self.shown()) { self.active = i; } self.go(folder); }
            NEW_WINDOW => self.out.push(Out::NewWindow(folder)),
            REFRESH | RENAME | TRASH | NEW_FOLDER | DUPLICATE | COPY | PASTE | PREVIEW => { if let Some(i) = pane.filter(|i| *i < self.shown()) { self.active = i; }
                self.command(match id { RENAME => Cmd::Rename, TRASH => Cmd::Trash, NEW_FOLDER => Cmd::NewFolder, DUPLICATE => Cmd::Duplicate, COPY => Cmd::Copy, PASTE => Cmd::Paste, PREVIEW => Cmd::Preview, _ => Cmd::Refresh }); }
            _ => {}
        }
    }
    /// What the panes left for the window after an event: a double click on a file is the lightbox's when the lightbox
    /// shows that kind, else the default application's; "Open" on a file is always the default application's (row 473, point 2).
    fn after_pane_events(&mut self) {
        for i in 0..self.panes.len() {
            if let Some(row) = self.panes[i].preview.take() { match self.panes[i].entry(row) { Some((path, ..)) if !crate::lightbox::shows(&crate::lightbox::ext_of(&path)) => self.out.push(Out::OpenDefault(path)), Some(_) => self.open_lightbox(i, row), None => {} } }
            if let Some(path) = self.panes[i].open_default.take() { self.out.push(Out::OpenDefault(path)); }
        }
    }
    fn set_cursor(&mut self, c: winit31::cursor::CursorIcon) { if c != self.cursor { self.cursor = c; self.window.set_cursor(winit31::cursor::Cursor::Icon(c)); } }
    pub fn pointer(&mut self, e: &Event) {
        if let Some(p) = e.pos() { self.pointer = p; } let now = Instant::now(); let s = self.scale(); let port = self.port.clone();
        if self.lightbox.is_open() { return self.lightbox_pointer(e) }
        if matches!(e, Event::Down { .. }) { self.tips.pressed(now); self.tip_demo = None; }
        // the drawn menu bar and its open menus take the pointer first
        if let Some(mb) = self.menubar.as_mut() { let (chosen, _, consumed) = mb.bar.event(e, &mut self.fonts); if let Some(c) = chosen.and_then(|id| mb.cmd(id)) { self.command(c); return } if consumed { return } }
        // an open question takes everything
        if let Some((d, _)) = self.dialog.as_mut() { let (clicked, _) = d.event(e, now); if let Some(b) = clicked { self.answer(b); } return }
        { let (_, _, consumed) = self.toast.event(e); if consumed { return } }
        // a name being changed: the field takes what is on it; a press anywhere else takes the name as typed (as Finder does)
        if let Some(r) = self.rename.as_mut() {
            let on = e.pos().is_some_and(|(x, y)| r.field.hit(x, y));
            if on || r.field.dragging() { r.field.pointer(e, &mut self.fonts); return }
            if matches!(e, Event::Down { .. }) { self.commit_rename(); if self.rename.is_some() || self.dialog.is_some() { return } }
        }
        // the running operation in the status bar: its cancel; a press on it opens (or closes) the list of what waits
        if self.op_rect.width() > 0.0 {
            if self.op_cancel.event(e, now) == Response::Clicked { crate::ops::cancel(); return }
            if let Event::Down { x, y, button: Button::Primary } = *e { if self.op_rect.contains(kurbo::Point::new(x, y)) && !self.op_cancel.hit(x, y) { self.op_list = !self.op_list; return } else if self.op_list { self.op_list = false; } }
        }
        // a new window (row 448): the middle button on a folder, a sidebar row or a segment of the path; the command key
        // (Ctrl on Windows) with a click on a sidebar row or a segment - in a pane that click is the selection's
        if let Event::Down { x, y, button } = *e { let middle = button == Button::Middle;
            if middle || (button == Button::Primary && self.mods.cmd) { if let Some(p) = self.folder_at(x, y, middle) { self.out.push(Out::NewWindow(p)); return } } }
        if matches!(e, Event::Down { button: Button::Middle, .. } | Event::Up { button: Button::Middle, .. }) { return }
        // a gap in hand: the sidebar's edge or the divider between the panes
        if let Some(d) = self.drag {
            match *e {
                Event::Move { x, .. } => match d {
                    Drag::Sidebar(x0, w0) => self.side_w = (w0 + (x - x0) / s).clamp(SIDE_MIN, SIDE_MAX),
                    Drag::Split => { let (a, b) = (self.side_rect.x1, self.window.surface_size().width as f64); if b - a > 1.0 { self.split = ((x - a) / (b - a)).clamp(0.15, 0.85); } }
                },
                Event::Up { button: Button::Primary, .. } | Event::Leave => self.drag = None,
                _ => {}
            }
            return
        }
        let at = e.pos().map(|(x, y)| kurbo::Point::new(x, y));
        let over = at.and_then(|p| self.gaps.iter().position(|g| g.width() > 0.0 && g.contains(p)).filter(|i| *i == 0 || self.two));
        if let (Event::Down { x, button: Button::Primary, .. }, Some(g)) = (*e, over) { self.drag = Some(if g == 0 { Drag::Sidebar(x, self.side_w) } else { Drag::Split }); return }
        self.over_gap = over;
        // the toolbar
        if self.back.event(e, now) == Response::Clicked { self.command(Cmd::Back); }
        if self.forward.event(e, now) == Response::Clicked { self.command(Cmd::Forward); }
        if self.up.event(e, now) == Response::Clicked { self.command(Cmd::Up); }
        if self.preview_btn.event(e, now) == Response::Clicked { self.command(Cmd::Preview); }
        if self.list_btn.event(e, now) == Response::Clicked { self.command(Cmd::AsList); }
        if self.grid_btn.event(e, now) == Response::Clicked { self.command(Cmd::AsGrid); }
        if self.pane2_btn.event(e, now) == Response::Clicked { self.command(Cmd::SecondPane); }
        if self.size.rect.width() > 0.0 { if let (Some(v), _) = self.size.event(e, now, s) { self.pane().grid.tile = grid::TILE_MIN + v as f64 * (grid::TILE_MAX - grid::TILE_MIN); } }
        // the path bar: a segment goes there; its empty part, pressed, takes a typed path
        if self.focus == Focus::Path {
            self.field.pointer(e, &mut self.fonts);
            if let Event::Down { x, y, .. } = *e { if !self.field.hit(x, y) { self.leave_path(); } }
        } else {
            let mut to = None; let mut on_crumb = false;
            for (b, path) in self.crumbs.iter_mut() { if b.event(e, now) == Response::Clicked { to = path.clone(); } if let Event::Down { x, y, .. } = *e { on_crumb |= b.hit(x, y); } }
            if let Some(p) = to { self.go(p); }
            else if let Event::Down { x, y, button: Button::Primary } = *e { if !on_crumb && self.path_rect.contains(kurbo::Point::new(x, y)) { self.edit_path(); return } }
        }
        // the search field
        self.search.pointer(e, &mut self.fonts);
        if let Event::Down { x, y, .. } = *e { if self.search.hit(x, y) { self.leave_path(); self.focus = Focus::Search; self.search.focused = true; } else if self.focus == Focus::Search { self.leave_search(); } }
        // the sidebar
        self.side_bar.event(e, now);
        for sec in self.sections.iter_mut() { sec.event(e, now); }
        self.side_hover = at.filter(|p| self.side_rect.contains(*p)).and_then(|p| self.side_rows.iter().position(|r| r.rect.contains(p)));
        if let Event::Down { x, y, button: Button::Primary } = *e { let p = kurbo::Point::new(x, y);
            if self.side_rect.contains(p) && !self.sections.iter().any(|sec| sec.hit(x, y)) { if let Some(to) = self.side_rows.iter().find(|r| r.rect.contains(p) && r.enabled).and_then(|r| r.path.clone()) { self.go(to); } } }
        // ... and its menu: a place opens here or in a new window (row 448)
        if let Event::Down { x, y, button: Button::Secondary } = *e { if let Some(to) = self.folder_at(x, y, false).filter(|_| self.side_rect.contains(kurbo::Point::new(x, y))) {
            self.menu_for = Some((None, to.clone())); self.out.push(Out::Menu(crate::ctxmenu::Ask { at: (x, y), paths: vec![to], rows: crate::ctxmenu::rows_for_place() })); } }
        // the panes: a press in one makes it the active one
        let n = self.shown();
        if let Event::Down { x, y, .. } = *e { let p = kurbo::Point::new(x, y); for i in 0..n { if self.panes[i].rect.contains(p) || self.pane_heads[i].contains(p) { self.active = i; self.leave_path(); self.leave_search(); self.focus = Focus::Pane; } } }
        for pane in self.panes.iter_mut().take(n) { pane.pointer(e, self.mods, now, &port); }
        for i in 0..n { if let Some((row, at)) = self.panes[i].menu_at.take() { self.ask_menu(i, row, at); } }
        self.after_pane_events();
        // a press on what is chosen was moved: the system's drag of it begins (the layer needs the pointer's event to be the current one: this is still inside it)
        for i in 0..n { if std::mem::take(&mut self.panes[i].drag_out) { let paths: Vec<PathBuf> = self.panes[i].chosen_entries(usize::MAX).into_iter().map(|e| e.0).collect(); if !paths.is_empty() { self.out.push(Out::Drag(paths)); } } }
        let resizing = self.over_gap.is_some() || self.panes.iter().take(n).any(|p| p.resizing());
        let text = at.is_some_and(|p| (self.focus == Focus::Path && self.path_rect.contains(p)) || self.search.rect.contains(p));
        self.set_cursor(if resizing { winit31::cursor::CursorIcon::ColResize } else if text { winit31::cursor::CursorIcon::Text } else { winit31::cursor::CursorIcon::Default });
    }
    /// Text from the clipboard, at the command key with V in a text field. (The window asks for it; this type holds no clipboard.)
    pub fn paste(&mut self, text: &str) { match self.focus { Focus::Rename => if let Some(r) = self.rename.as_mut() { r.field.error = false; r.field.paste(text, &mut self.fonts); }, Focus::Path => { self.field.paste(text, &mut self.fonts); } Focus::Search => { if self.search.paste(text, &mut self.fonts) == FieldResponse::Changed { let t = self.search.text().to_string(); self.pane().set_search(&t); } } Focus::Pane => {} } }
    /// A key for the drawn menu bar: (the command of the row chosen, redraw, the key was the bar's).
    pub fn with_menu_key(&mut self, k: &min_ui::BarKey) -> (Option<Cmd>, bool, bool) { match self.menubar.as_mut() { Some(mb) => { let (id, redraw, consumed) = mb.bar.key(k, &mut self.fonts); (id.and_then(|i| mb.cmd(i)), redraw, consumed) } None => (None, false, false) } }
    /// A text field holds the keyboard (the clipboard's keys are its).
    pub fn typing(&self) -> bool { self.focus != Focus::Pane }
    /// The chosen text of the field in hand, for a copy; `cut` takes it out.
    pub fn copy(&mut self, cut: bool) -> Option<String> { match self.focus { Focus::Rename => self.rename.as_mut().and_then(|r| r.field.copy(cut, &mut self.fonts)), Focus::Path => self.field.copy(cut, &mut self.fonts), Focus::Search => { let r = self.search.copy(cut, &mut self.fonts); if cut { let t = self.search.text().to_string(); self.pane().set_search(&t); } r } Focus::Pane => None } }
    /// A key. `name`: the key as the key map names it ("N", "[", "Up", "F5"); `alt`: the Alt / Option key is down;
    /// `menu_keys`: the system's menu bar answers the command-key shortcuts itself (macOS), so they are not taken here.
    pub fn key(&mut self, k: &KeyEvent, name: &str, alt: bool, menu_keys: bool) {
        let now = Instant::now(); let port = self.port.clone(); let mac = cfg!(target_os = "macos"); self.tip_demo = None;
        if self.lightbox.is_open() { return self.lightbox_key(k, name) }
        // an open question: Return and Escape both give its last button - the careful answer; a button is pressed by the mouse
        if let Some((d, asked)) = self.dialog.as_ref() { let last = d.buttons.len().saturating_sub(1);
            match k.key { Key::Named(Named::Enter) => self.answer(last), Key::Named(Named::Escape) => self.answer(if matches!(asked, Asked::Conflict(_)) { 0 } else { last }), _ => {} } return }
        match self.focus {
            Focus::Rename => {
                let Some(r) = self.rename.as_mut() else { self.focus = Focus::Pane; return };
                match r.field.key(k, &mut self.fonts) { FieldResponse::Accepted => self.commit_rename(), FieldResponse::Cancelled => self.end_rename(), FieldResponse::Changed => r.field.error = false, FieldResponse::None => {} }
                return
            }
            Focus::Path => {
                match self.field.key(k, &mut self.fonts) { FieldResponse::Accepted => self.submit_path(), FieldResponse::Cancelled => self.leave_path(), FieldResponse::Changed => { self.path_error = None; self.field.error = false; } FieldResponse::None => {} }
                return
            }
            Focus::Search => {
                if matches!(k.key, Key::Named(Named::Down | Named::Tab)) { self.leave_search(); return }
                match self.search.key(k, &mut self.fonts) {
                    FieldResponse::Cancelled => { self.search.clear(); self.pane().set_search(""); self.leave_search(); }
                    FieldResponse::Accepted => self.leave_search(),
                    FieldResponse::Changed => { let text = self.search.text().to_string(); self.pane().set_search(&text); }
                    FieldResponse::None => {}
                }
                // the command-key shortcuts still act from the search field (a second pane, the view), but not its own editing keys
                if !(k.cmd && !matches!(&k.key, Key::Char(c) if matches!(c.to_ascii_lowercase().as_str(), "a" | "c" | "v" | "x" | "z"))) || menu_keys { return }
            }
            Focus::Pane => {}
        }
        if let Some(c) = menu::shortcut(mac, name, k.cmd, k.shift, alt).filter(|_| !(menu_keys && k.cmd && name != "Down")) { self.command(c); return }
        match &k.key {
            // Tab switches panes when the second is shown (row 450)
            Key::Named(Named::Tab) if !k.cmd && !alt => { if self.two && self.panes.len() > 1 { self.active = 1 - self.active.min(1); } return }
            // Space is the lightbox's (a later slice): nothing yet, and it is not a letter to find a name by
            // Space is the lightbox's (row 452) - unless a name is being typed to find it, where it is a letter
            Key::Char(c) if c == " " && !k.cmd => { if self.pane().sel().typed(now).is_empty() { self.command(Cmd::Preview); return } }
            Key::Named(Named::Enter) if mac => return,
            Key::Named(Named::Escape) => { if let Some(mb) = self.menubar.as_mut() { mb.bar.cancel(); } return }
            _ => {}
        }
        if alt && !mac { return }
        self.pane().key(k, now, &port); self.after_pane_events();
    }

    // ---------------------------------------------------------------- the scripted looks
    /// `UFB_DEMO` steps, done one after another once the panes have what they wait for (no system input is involved):
    /// `grid` `list` `size:<px>` `pane2` `tab` `expand:<row>` `collapse:<row>` `select:<a>-<b>` `selectall` `sort:<name|date|size|kind>[:desc]`
    /// `search:<text>` `go:<path>` `back` `forward` `up` `editpath` `typepath:<text>` `hidden` `foldersfirst` `scroll:<row>` `side:<n>` (fold a
    /// sidebar section) `sidebar:<px>` `menu:<letter>` (the drawn menu bar) `tip:<back|up|list|grid|pane2>` `newwindow` `middle:<row>` `wait:<ms>`.
    fn demo_step(&mut self, step: &str) {
        let port = self.port.clone(); let (word, arg) = step.split_once(':').unwrap_or((step, "")); let num = |a: &str| a.parse::<usize>().ok();
        match word {
            "grid" => self.command(Cmd::AsGrid), "list" => self.command(Cmd::AsList), "pane2" => self.set_two(true), "hidden" => self.command(Cmd::Hidden), "foldersfirst" => self.command(Cmd::FoldersFirst),
            "tab" => if self.two { self.active = 1 - self.active.min(1); },
            "size" => if let Ok(px) = arg.parse::<f64>() { let px = px.clamp(grid::TILE_MIN, grid::TILE_MAX); self.pane().grid.tile = px; self.size.value = ((px - grid::TILE_MIN) / (grid::TILE_MAX - grid::TILE_MIN)) as f32; },
            "expand" => if let Some(n) = num(arg) { self.pane().expand(&port, n); },
            "collapse" => if let Some(n) = num(arg) { self.pane().collapse(n); },
            "select" => { let (a, b) = arg.split_once('-').unwrap_or((arg, arg)); if let (Some(a), Some(b)) = (num(a), num(b)) { self.pane().select_rows(a, b); } }
            "selectall" => self.pane().select_all(),
            "sort" => { let (by, dir) = arg.split_once(':').unwrap_or((arg, "asc")); if let Some(by) = SortBy::parse(by) { self.pane().set_sort(by, dir != "desc"); } }
            "search" => { self.search.set_text(arg); self.pane().set_search(arg); }
            "go" => self.go(PathBuf::from(arg)), "back" => self.command(Cmd::Back), "forward" => self.command(Cmd::Forward), "up" => self.command(Cmd::Up),
            "editpath" => self.edit_path(),
            "typepath" => { self.edit_path(); self.field.set_text(arg); self.submit_path(); }
            "scroll" => if let Some(n) = num(arg) { self.pane().scroll_to(n); },
            "side" => if let Some(sec) = num(arg).and_then(|n| self.sections.get_mut(n)) { sec.expanded = !sec.expanded; },
            "sidebar" => if let Ok(px) = arg.parse::<f64>() { self.side_w = px.clamp(SIDE_MIN, SIDE_MAX); },
            "menu" => if let (Some(mb), Some(ch)) = (self.menubar.as_mut(), arg.chars().next()) { mb.bar.key(&min_ui::BarKey::Alt(ch.to_ascii_lowercase()), &mut self.fonts); },
            "newwindow" => self.command(Cmd::NewWindow),
            // the file operations, as the menus give them: `newfolder` `rename` `type:<text>` (into the name's field) `accept` `cancel` `duplicate` `trash` `deletenow` `undo` `answer:<button>`
            "newfolder" => self.command(Cmd::NewFolder), "rename" => self.command(Cmd::Rename), "duplicate" => self.command(Cmd::Duplicate), "trash" => self.command(Cmd::Trash), "deletenow" => self.command(Cmd::DeleteNow), "undo" => self.command(Cmd::Undo),
            "type" => if let Some(r) = self.rename.as_mut() { r.field.set_text(arg); },
            "accept" => self.commit_rename(), "cancel" => self.end_rename(),
            "answer" => if let Some(b) = num(arg) { self.answer(b); },
            // the lightbox: `preview` (the row under the cursor; again closes it) `lbnext` `lbprev` `lbzoom:<factor>` `lbactual` `lbfit` `lbpan:<dx>,<dy>`
            "lblayer" => if let Some(i) = num(arg) { self.lightbox.pick_layer(i); }, "lbback" => { if self.lightbox.back_to_layers() { self.presenter.engine.clear_picture(); } }
            "preview" => self.command(Cmd::Preview), "lbnext" => self.step_lightbox(1), "lbprev" => self.step_lightbox(-1),
            "lbzoom" | "lbactual" | "lbfit" | "lbpan" => { let area = self.lightbox_area(); let mid = (area.center().x, area.center().y); if let Some(v) = self.lightbox.view.as_mut() { match word {
                "lbzoom" => v.zoom_by(arg.parse().unwrap_or(1.0), mid, area), "lbactual" => v.actual(mid, area), "lbfit" => v.fit(),
                _ => if let Some((dx, dy)) = arg.split_once(',').and_then(|(a, b)| Some((a.parse::<f64>().ok()?, b.parse::<f64>().ok()?))) { v.drag_begin(mid); v.drag_to((mid.0 + dx, mid.1 + dy), area); v.drag_end(); } } } }
            // a drag of ours has ended, as the layer would tell it
            "dragended" => { crate::dnd::on_notice(&self.dnd, min_dnd::Notice::Ended { serial: 0, op: min_dnd::MOVE, detail: String::new() }); }
            // a drag of a file, as the layer would tell it (no drag is performed): `over:<row>:<path>[:copy|move]` shows where it would land,
            // `drop:<row|bg|side<n>>:<path>[:copy|move]` lets it go there - on a row's folder, the pane itself, a sidebar row
            "over" | "drop" => { let (target, rest) = arg.split_once(':').unwrap_or((arg, ""));   // (a Windows path has a colon of its own: only a last ":copy" / ":move" is the keys')
                let (path, keys) = match rest.rsplit_once(':') { Some((p, k)) if k == "copy" || k == "move" => (p, k), _ => (rest, "") };
                let at = if target == "bg" { let r = self.pane().rect; Some((r.x1 - 30.0, r.y1 - 30.0)) } else if let Some(k) = target.strip_prefix("side").and_then(num) { self.side_rows.get(k).map(|r| ((r.rect.x0 + r.rect.x1) / 2.0, (r.rect.y0 + r.rect.y1) / 2.0)) } else { num(target).and_then(|k| self.pane().row_point(k)) };
                if let Some((x, y)) = at { use min_dnd::{DragEvent, Notice, Payload, Phase, COPY, LINK, MOVE};
                    let allowed = match keys { "copy" => COPY, "move" => MOVE, _ => COPY | MOVE | LINK }; let payload = std::rc::Rc::new(Payload { paths: vec![PathBuf::from(path)], ..Default::default() });
                    let tell = |phase| crate::dnd::on_notice(&self.dnd, Notice::Drag(DragEvent { win: 0, phase, x, y, allowed, detail: String::new(), mine: None, payload: payload.clone() }));
                    let op = tell(Phase::Enter); eprintln!("UFB_DEMO: a drag over {target} would be {}", match op { COPY => "a copy", MOVE => "a move", _ => "refused" });
                    if word == "drop" { tell(Phase::Drop); } } }
            // `copyother` `moveother` (the chosen entries into the other pane's folder) `all` (tick "Apply to all") `stopop` (call the running operation off) `oplist`
            "copyother" => self.command(Cmd::CopyToOther), "moveother" => self.command(Cmd::MoveToOther), "stopop" => crate::ops::cancel(), "oplist" => self.op_list = true,
            "all" => if let Some((d, _)) = self.dialog.as_mut() { if let Some(c) = d.check.as_mut() { c.1 = true; } },
            // the secondary button on a row, on the pane's empty part (`context:bg`), on a sidebar row (`context:side<n>`)
            "context" => { let at = if arg == "bg" { let r = self.pane().rect; Some((r.x1 - 30.0, r.y1 - 30.0)) } else if let Some(n) = arg.strip_prefix("side").and_then(num) { self.side_rows.get(n).map(|r| ((r.rect.x0 + r.rect.x1) / 2.0, (r.rect.y0 + r.rect.y1) / 2.0)) } else { num(arg).and_then(|n| self.pane().row_point(n)) };
                if let Some((x, y)) = at { self.pointer(&Event::Down { x, y, button: Button::Secondary }); self.pointer(&Event::Up { x, y, button: Button::Secondary }); } }
            // a middle click on the folder at a row, as the mouse would do it
            "middle" => if let Some(n) = num(arg) { if let Some((x, y)) = self.pane().row_point(n) { self.pointer(&Event::Down { x, y, button: Button::Middle }); } },
            "tip" => self.tip_demo = Some(arg.to_string()),
            "wait" => self.demo_until = Some(Instant::now() + Duration::from_millis(arg.parse().unwrap_or(0))),
            other => eprintln!("UFB_DEMO: unknown step {other:?}"),
        }
    }
    fn busy(&self) -> bool { self.panes.iter().take(self.shown()).any(|p| p.loading()) || self.check.is_some() }
    fn run_demo(&mut self) {
        while !self.demo.is_empty() {
            if self.busy() { return }
            if let Some(t) = self.demo_until { if Instant::now() < t { return } self.demo_until = None; }
            let step = self.demo.pop_front().unwrap(); self.demo_step(&step);
        }
    }

    // ---------------------------------------------------------------- drawing
    pub fn animating(&self) -> bool {
        let now = Instant::now(); let n = self.shown();
        self.tips.next_change(now).is_some() || self.side_bar.animating(now) || self.panes.iter().take(n).any(|p| p.animating(now) || (p.load == Load::Loading)) || [&self.back, &self.forward, &self.up, &self.list_btn, &self.grid_btn, &self.pane2_btn].iter().any(|b| b.animating(now))
            || self.crumbs.iter().any(|(b, _)| b.animating(now)) || self.sections.iter().any(|sec| sec.animating(now)) || self.size.animating(now) || self.focus != Focus::Pane || self.dialog.is_some() || self.toast.is_shown() || self.rename_next.is_some() || self.path_error.is_some() || !self.demo.is_empty() || (self.shot.is_some() && !self.done)
    }
    /// The path bar's segments as buttons, made anew when the path, the startup disk's name or the room changes.
    fn make_crumbs(&mut self, room: f64, s: f64) {
        let path = self.panes[self.active.min(self.panes.len() - 1)].path().to_path_buf(); let key = (path.clone(), self.root.clone(), (room * 4.0) as i64);
        if self.crumbs_for.as_ref() == Some(&key) { return } self.crumbs_for = Some(key);
        let segs = crumbs::segments(&path, if self.root.is_empty() { "/" } else { &self.root });
        let mut all: Vec<(ActionButton, Option<PathBuf>)> = segs.into_iter().map(|seg| { let mut b = ActionButton::new("", "").size_s(); b.label = Some(seg.label); (b, Some(seg.path)) }).collect();
        let widths: Vec<f64> = all.iter().map(|(b, _)| b.measure(&mut self.fonts, s).0).collect();
        let (sep, dots) = (14.0 * s, t::ACTION_BUTTON_HEIGHT_S as f64 * s);
        let keep = crumbs::elide(&widths, sep, dots, room);
        let mut out = vec![]; let mut taken: Vec<Option<(ActionButton, Option<PathBuf>)>> = all.drain(..).map(Some).collect();
        for k in keep { match k { Some(i) => if let Some(c) = taken[i].take() { out.push(c); }, None => out.push((ActionButton::new("dots-three", "").size_s().enabled(false), None)) } }
        self.crumbs = out;
    }
    pub fn redraw(&mut self) {
        let s = self.scale(); let size = self.window.surface_size(); let (w, h) = (size.width as f64, size.height as f64); if w < 1.0 || h < 1.0 { return }
        self.poll(); self.run_demo();
        if self.lightbox.is_open() { return self.redraw_lightbox() }
        let now = Instant::now(); let (ink, sub) = (paint::color(t::NEUTRAL_CONTENT_COLOR_DEFAULT), paint::color(t::NEUTRAL_SUBDUED_CONTENT_COLOR_DEFAULT));
        if self.active >= self.shown() { self.active = 0; }
        // the window is called by the folder it shows (the active pane's)
        { let title = self.panes[self.active].title(); if title != self.titled { self.window.set_title(&title); if !self.demo.is_empty() || self.shot.is_some() { eprintln!("UFB_DEMO: titled {title}"); } self.titled = title; } }
        // the search field shows the active pane's filter (a navigation clears it)
        if self.search.text() != self.panes[self.active].search { let text = self.panes[self.active].search.clone(); self.search.set_text(&text); }
        // -- where everything lies
        let st = self.menu_state();
        let menu_h = match self.menubar.as_mut() { Some(mb) => {
            mb.sync(st, &mut self.fonts); mb.bar.layout(&mut self.fonts, 0.0, 0.0, w, Rect::new(0.0, 0.0, w, h), s); mb.bar.rect.height() } None => 0.0 };
        let status = Rect::new(0.0, h - STATUS * s, w, h);
        let side_w = (self.side_w * s).min(w * 0.45).round(); self.side_rect = Rect::new(0.0, menu_h, side_w, status.y0);
        let area = Rect::new(side_w, menu_h, w, status.y0); let tool = Rect::new(area.x0, area.y0, area.x1, (area.y0 + TOOLBAR * s).min(area.y1)); let body = Rect::new(area.x0, tool.y1, area.x1, area.y1);
        self.gaps[0] = Rect::new(side_w - GAP * s / 2.0, menu_h, side_w + GAP * s / 2.0, status.y0);
        let n = self.shown(); let split_x = if n == 2 { (body.x0 + body.width() * self.split).round() } else { body.x1 };
        self.gaps[1] = if n == 2 { Rect::new(split_x - GAP * s / 2.0, body.y0, split_x + GAP * s / 2.0, body.y1) } else { Rect::ZERO };
        let head_h = if n == 2 { HEAD * s } else { 0.0 };
        for i in 0..n { let (x0, x1) = if n == 1 { (body.x0, body.x1) } else if i == 0 { (body.x0, split_x) } else { (split_x + s, body.x1) };
            self.pane_heads[i] = Rect::new(x0, body.y0, x1, body.y0 + head_h); let active = i == self.active; self.panes[i].layout(Rect::new(x0, body.y0 + head_h, x1, body.y1), s, active); }
        self.show_question();
        // a new folder goes straight into its name, as soon as its row is there
        if let Some(path) = self.rename_next.clone() { let i = self.active; if !self.panes[i].loading() { self.rename_next = None; if self.focus == Focus::Pane && self.dialog.is_none() && self.panes[i].name_rect(&path).is_some() { self.start_rename(); } } }
        // the field of a name being changed lies over the name (its entry gone: the rename is over)
        if let Some(r) = self.rename.as_mut() {
            match self.panes.get(r.pane).filter(|_| r.pane < n).and_then(|p| p.name_rect(&r.path).map(|q| (q, p.rect, p.mode))) {
                Some((q, pane, mode)) => { let fh = (t::FIELD_HEIGHT_M as f64 * s).min(30.0 * s); let cy = (q.y0 + q.y1) / 2.0;
                    let (x0, x1) = if mode == Mode::List { (q.x0 - 6.0 * s, (q.x0 + 420.0 * s).min(q.x1).max(q.x0 + 80.0 * s)) } else { (q.x0, q.x1) };
                    r.field.layout(&mut self.fonts, Rect::new(x0.max(pane.x0), cy - fh / 2.0, x1.min(pane.x1), cy + fh / 2.0), s); }
                None => { self.rename = None; if self.focus == Focus::Rename { self.focus = Focus::Pane; } }
            }
        }
        // the toolbar's controls, from both ends; the path bar takes what is left
        let bh = t::ACTION_BUTTON_HEIGHT_M as f64 * s; let by = tool.y0 + (tool.height() - bh) / 2.0; let pad = 8.0 * s; let radius = t::ACTION_BUTTON_CORNER_RADIUS_M as f64 * s;
        let grid_mode = self.panes[self.active].mode == Mode::Grid; let wide = tool.width() / s;
        let mut x = tool.x0 + pad;
        for b in [&mut self.back, &mut self.forward, &mut self.up] { b.rect = Rect::new(x, by, x + bh, by + bh); b.corners = Corners::all(radius); x += bh + 2.0 * s; }
        let mut r = tool.x1 - pad;
        self.pane2_btn.rect = Rect::new(r - bh, by, r, by + bh); self.pane2_btn.corners = Corners::all(radius); self.pane2_btn.selected = self.two; r -= bh + pad;
        let search_w = (if wide > 1000.0 { 200.0 } else if wide > 760.0 { 150.0 } else if wide > 560.0 { 110.0 } else { 0.0 }) * s; let fh = t::FIELD_HEIGHT_M as f64 * s;
        let search_rect = if search_w > 0.0 { let q = Rect::new(r - search_w, tool.y0 + (tool.height() - fh) / 2.0, r, tool.y0 + (tool.height() + fh) / 2.0); r -= search_w + pad; q } else { Rect::ZERO };
        self.search.layout(&mut self.fonts, search_rect, s);
        self.size.rect = if grid_mode && wide > 640.0 { let q = Rect::new(r - 100.0 * s, by, r, by + bh); r -= 100.0 * s + pad; q } else { Rect::ZERO };
        self.size.value = ((self.panes[self.active].grid.tile - grid::TILE_MIN) / (grid::TILE_MAX - grid::TILE_MIN)) as f32;
        // (list and grid are one control of two halves)
        self.grid_btn.rect = Rect::new(r - bh, by, r, by + bh); self.grid_btn.corners = Corners { tl: 0.0, bl: 0.0, tr: radius, br: radius }; self.grid_btn.selected = grid_mode; r -= bh + s;
        self.list_btn.rect = Rect::new(r - bh, by, r, by + bh); self.list_btn.corners = Corners { tl: radius, bl: radius, tr: 0.0, br: 0.0 }; self.list_btn.selected = !grid_mode; r -= bh + pad;
        // (the lightbox by the mouse, row 452)
        self.preview_btn.rect = if wide > 520.0 { let q = Rect::new(r - bh, by, r, by + bh); r -= bh + pad; q } else { Rect::ZERO }; self.preview_btn.corners = Corners::all(radius); self.preview_btn.enabled = self.panes[self.active].cursor_row().is_some();
        self.path_rect = Rect::new((x + pad - 2.0 * s).min(r), tool.y0 + (tool.height() - fh) / 2.0, r.max(x + pad), tool.y0 + (tool.height() + fh) / 2.0);
        { let p = &self.panes[self.active]; self.back.enabled = p.history.can_back(); self.forward.enabled = p.history.can_forward(); self.up.enabled = p.history.parent().is_some(); }
        let crumb_room = self.path_rect.width() - 12.0 * s; self.make_crumbs(crumb_room, s);
        if self.focus == Focus::Path { self.field.layout(&mut self.fonts, self.path_rect, s); }
        if self.path_error.as_ref().is_some_and(|(_, since)| since.elapsed() > Duration::from_millis(2600)) { self.path_error = None; self.field.error = false; }

        let scene: &mut Scene = self.layer.scene_mut(); scene.reset(); let fonts = &mut self.fonts;
        // The surfaces, as the family stacks them: the window is layer 1; the sidebar's sections are layer-2 tiles on it;
        // the path bar and the search are fields; the list lies on the window itself.
        paint::rect(scene, Rect::new(0.0, 0.0, w, h), t::BACKGROUND_LAYER_1_COLOR);

        // -- the panes
        self.thumbs.begin_frame();
        for i in 0..n {
            if n == 2 {
                let hr = self.pane_heads[i]; let active = i == self.active;
                paint::rect(scene, hr, if active { t::BACKGROUND_LAYER_2_COLOR } else { t::BACKGROUND_LAYER_1_COLOR });
                // the active pane's accent: the line the toolbar and the menus act on
                paint::rect(scene, Rect::new(hr.x0, hr.y0, hr.x1, hr.y0 + 2.0 * s), if active { t::FOCUS_INDICATOR_COLOR } else { t::DIVIDER_COLOR });
                let mut tx = hr.x0 + 10.0 * s;
                if let Some(g) = fonts.icon("folder-simple", 14.0 * s as f32, if active { ink } else { sub }) { draw_layout(scene, &g, Affine::translate((tx, hr.y0 + s + (hr.height() - g.height() as f64) / 2.0))); tx += g.width() as f64 + 6.0 * s; }
                let l = fonts.layout_fit(&self.panes[i].title(), Family::Sans, t::BODY_SIZE_S * s as f32, if active { t::FONT_WEIGHT_BOLD } else { t::FONT_WEIGHT_REGULAR } as f32, if active { ink } else { sub }, hr.x1 - tx - 10.0 * s);
                draw_layout(scene, &l, Affine::translate((tx, hr.y0 + s + (hr.height() - l.height() as f64) / 2.0)));
            }
            self.panes[i].paint(scene, fonts, &mut self.icons, &mut self.thumbs, s, now);
        }
        self.thumbs.end_frame();
        if let Some(r) = self.rename.as_mut() { let clip = self.panes[r.pane.min(n - 1)].rect; scene.push_clip_layer(peniko::Fill::NonZero, Affine::IDENTITY, &clip); r.field.paint(scene, fonts, s); scene.pop_layer(); }
        if n == 2 { let hot = self.over_gap == Some(1) || self.drag == Some(Drag::Split); paint::rect(scene, Rect::new(split_x, body.y0, split_x + if hot { 2.0 * s } else { s }, body.y1), if hot { t::RAIL_SPLITTER_COLOR_HOVER } else { t::DIVIDER_COLOR_MEDIUM }); }

        // -- the toolbar
        paint::hline(scene, tool.x0, tool.x1, tool.y1 - s, s, t::DIVIDER_COLOR);
        for b in [&self.back, &self.forward, &self.up, &self.list_btn, &self.grid_btn, &self.pane2_btn] { b.paint(scene, fonts, s, now); }
        if self.preview_btn.rect.width() > 0.0 { self.preview_btn.paint(scene, fonts, s, now); }
        if self.size.rect.width() > 0.0 { self.size.paint(scene, s, now); }
        if search_w > 0.0 { self.search.paint(scene, fonts, s);
            if self.search.text().is_empty() { if let Some(g) = fonts.icon("magnifying-glass", 13.0 * s as f32, sub) { draw_layout(scene, &g, Affine::translate((search_rect.x1 - g.width() as f64 - 8.0 * s, search_rect.y0 + (search_rect.height() - g.height() as f64) / 2.0))); } } }
        if self.focus == Focus::Path {
            self.field.paint(scene, fonts, s);
            if let Some((why, _)) = &self.path_error { let l = fonts.layout(why, Family::Sans, t::BODY_SIZE_S * s as f32, t::FONT_WEIGHT_BOLD as f32, paint::color(t::NEGATIVE_CONTENT_COLOR_DEFAULT));
                let lr = Rect::new(self.path_rect.x1 - l.width() as f64 - 20.0 * s, self.path_rect.y0 + 3.0 * s, self.path_rect.x1 - 3.0 * s, self.path_rect.y1 - 3.0 * s);
                paint::rounded(scene, lr, 4.0 * s, t::FIELD_BACKGROUND_COLOR); draw_layout(scene, &l, Affine::translate((lr.x0 + 8.0 * s, lr.y0 + (lr.height() - l.height() as f64) / 2.0))); }
        } else {
            let pr = self.path_rect; paint::rounded(scene, pr, t::FIELD_CORNER_RADIUS_M as f64 * s, t::FIELD_BACKGROUND_COLOR);
            let ch = t::ACTION_BUTTON_HEIGHT_S as f64 * s; let cy = pr.y0 + (pr.height() - ch) / 2.0; let mut cx = pr.x0 + 4.0 * s; let last = self.crumbs.len().saturating_sub(1); let end = pr.x1 - 8.0 * s;
            scene.push_clip_layer(peniko::Fill::NonZero, Affine::IDENTITY, &pr);
            for (i, (b, path)) in self.crumbs.iter_mut().enumerate() {
                let bw = if path.is_some() { b.measure(fonts, s).0 } else { ch }; let bw = bw.min((end - cx).max(ch));
                b.rect = Rect::new(cx, cy, cx + bw, cy + ch); b.corners = Corners::all(t::ACTION_BUTTON_CORNER_RADIUS_S as f64 * s); b.paint(scene, fonts, s, now); cx += bw;
                if i < last { if let Some(g) = fonts.icon("caret-right", 10.0 * s as f32, sub) { draw_layout(scene, &g, Affine::translate((cx + (14.0 * s - g.width() as f64) / 2.0, pr.y0 + (pr.height() - g.height() as f64) / 2.0))); } cx += 14.0 * s; }
            }
            scene.pop_layer();
        }

        // -- the sidebar
        let sr = self.side_rect; self.side_rows.clear();
        scene.push_clip_layer(peniko::Fill::NonZero, Affine::IDENTITY, &sr);
        let (pad_s, row_h, head_h2) = (8.0 * s, SIDE_ROW * s, Section::height(s)); let (x0, x1) = (sr.x0 + pad_s, sr.x1 - pad_s);
        let current = self.panes[self.active].path().to_path_buf();
        // each section: (its rows, the quiet line it shows when it has none)
        let lists: [(Vec<&Place>, &str); 4] = [(self.favorites.iter().collect(), "No favorites yet"), (vec![], "No saved shares"), (vec![], "No jobs"), (self.fixed.iter().chain(self.volumes.iter()).collect(), "")];
        // the row of the place the active pane is in: the longest of the places that is exactly it
        let mut y = sr.y0 + pad_s - self.side_bar.offset; let top = y;
        let side_row = |scene: &mut Scene, fonts: &mut Fonts, rows: &mut Vec<SideRow>, y: f64, icon: &str, label: &str, path: Option<&Path>, enabled: bool, hover: Option<usize>| {
            let r = Rect::new(x0 + 4.0 * s, y, x1 - 4.0 * s, y + row_h); let here = path.is_some_and(|p| p == current); let idx = rows.len();
            if here { paint::rounded(scene, r, t::CORNER_RADIUS_SMALL_DEFAULT as f64 * s, t::RAIL_ROW_ACTIVE_FILL); } else if enabled && hover == Some(idx) { paint::rounded(scene, r, t::CORNER_RADIUS_SMALL_DEFAULT as f64 * s, t::HOVER_BACKGROUND_COLOR); }
            let color = paint::color(if !enabled { t::DISABLED_CONTENT_COLOR } else if here { t::RAIL_ROW_ACTIVE_LABEL } else { t::NEUTRAL_CONTENT_COLOR_DEFAULT });
            let mut tx = r.x0 + 8.0 * s;
            if let Some(g) = fonts.icon(icon, 15.0 * s as f32, if here || !enabled { color } else { paint::color(t::NEUTRAL_SUBDUED_CONTENT_COLOR_DEFAULT) }) { draw_layout(scene, &g, Affine::translate((tx, y + (row_h - g.height() as f64) / 2.0))); } tx += 22.0 * s;
            let l = fonts.layout_fit(label, Family::Sans, t::BODY_SIZE_S * s as f32, t::FONT_WEIGHT_REGULAR as f32, color, r.x1 - tx - 6.0 * s);
            draw_layout(scene, &l, Affine::translate((tx, y + (row_h - l.height() as f64) / 2.0)));
            rows.push(SideRow { rect: r, path: path.map(Path::to_path_buf), enabled });
        };
        for (i, (rows, empty)) in lists.iter().enumerate() {
            let open = self.sections[i].expanded; let body_h = if !open { 0.0 } else if rows.is_empty() { row_h } else { rows.len() as f64 * row_h } + if open { 6.0 * s } else { 0.0 };
            section::paint_tile(scene, Rect::new(x0, y, x1, y + head_h2 + body_h), s);
            self.sections[i].rect = Rect::new(x0, y, x1, y + head_h2); self.sections[i].paint(scene, fonts, s, now, i == 0); y += head_h2;
            if open {
                if rows.is_empty() { let l = fonts.layout_fit(empty, Family::Sans, t::BODY_SIZE_S * s as f32, t::FONT_WEIGHT_REGULAR as f32, sub, x1 - x0 - 24.0 * s); draw_layout(scene, &l, Affine::translate((x0 + 12.0 * s, y + (row_h - l.height() as f64) / 2.0))); y += row_h; }
                for p in rows { side_row(scene, fonts, &mut self.side_rows, y, p.icon, &p.label, Some(&p.path), true, self.side_hover); y += row_h; }
                y += 6.0 * s;
            }
            y += 8.0 * s;
            // All Tracked: a destination of its own between the jobs and the places (row 463) - not this slice's
            if i == 2 { side_row(scene, fonts, &mut self.side_rows, y, "list-checks", "All Tracked", None, false, self.side_hover); y += row_h + 8.0 * s; }
        }
        scene.pop_layer();
        self.side_bar.set(sr, y - top + pad_s, s); self.side_bar.paint(scene, now);
        { let hot = self.over_gap == Some(0) || matches!(self.drag, Some(Drag::Sidebar(..))); paint::rect(scene, Rect::new(sr.x1 - s, sr.y0, sr.x1 + if hot { s } else { 0.0 }, sr.y1), if hot { t::RAIL_SPLITTER_COLOR_HOVER } else { t::DIVIDER_COLOR }); }

        // -- the status bar: the active pane's items and choice; the room left on its volume
        paint::rect(scene, status, t::BACKGROUND_LAYER_1_COLOR); paint::hline(scene, status.x0, status.x1, status.y0, s, t::DIVIDER_COLOR);
        { let p = &self.panes[self.active]; let size = t::BODY_SIZE_S * s as f32;
            let free = p.free.filter(|_| p.load != Load::Loading).map(|f| crate::format::free_text(f, crate::format::DECIMAL)).unwrap_or_default();
            let fl = fonts.layout(&free, Family::Sans, size, t::FONT_WEIGHT_REGULAR as f32, sub); let mut fx = status.x1 - 12.0 * s - fl.width() as f64;
            // a running operation takes the right of the bar (row 468, point 7): what it does, how far it is, its cancel, how many wait
            self.op_rect = Rect::ZERO;
            if let Some(st) = crate::ops::status() {
                let waiting = crate::ops::waiting().len(); let frac = if st.bytes.1 > 0 { st.bytes.0 as f64 / st.bytes.1 as f64 } else if st.items.1 > 0 { st.items.0 as f64 / st.items.1 as f64 } else { 0.0 };
                let text = format!("{}{}", st.label, if waiting > 0 { format!("  \u{b7}  {waiting} waiting") } else { String::new() });
                let bh = t::ACTION_BUTTON_HEIGHT_S as f64 * s; let x1 = status.x1 - 8.0 * s; let bar_w = 110.0 * s;
                self.op_cancel.rect = Rect::new(x1 - bh, status.y0 + (status.height() - bh) / 2.0, x1, status.y0 + (status.height() + bh) / 2.0); self.op_cancel.corners = Corners::all(t::ACTION_BUTTON_CORNER_RADIUS_S as f64 * s);
                let bar = Rect::new(x1 - bh - 8.0 * s - bar_w, status.y0 + (status.height() - min_ui::progress::height(s)) / 2.0, x1 - bh - 8.0 * s, status.y0 + (status.height() + min_ui::progress::height(s)) / 2.0);
                let l = fonts.layout_fit(&text, Family::Sans, size, t::FONT_WEIGHT_REGULAR as f32, ink, (bar.x0 - 10.0 * s - (area.x0 + 160.0 * s)).max(60.0 * s)); let lx = bar.x0 - 10.0 * s - l.width() as f64;
                draw_layout(scene, &l, Affine::translate((lx, status.y0 + (status.height() - l.height() as f64) / 2.0)));
                min_ui::progress::bar(scene, bar, frac, min_ui::progress::Tone::Neutral); self.op_cancel.paint(scene, fonts, s, now);
                self.op_rect = Rect::new(lx - 6.0 * s, status.y0, status.x1, status.y1); fx = lx - 24.0 * s;
                // the list: the one that runs (with the item in hand), then those that wait
                if self.op_list { let mut rows = vec![if st.current.is_empty() { st.label.clone() } else { format!("{}  \u{2014}  {}", st.label, st.current) }]; rows.extend(crate::ops::waiting().into_iter().map(|w| format!("Waiting:  {w}")));
                    let rh = 22.0 * s; let pw = 420.0 * s; let card = Rect::new(status.x1 - 8.0 * s - pw, status.y0 - 8.0 * s - rows.len() as f64 * rh - 12.0 * s, status.x1 - 8.0 * s, status.y0 - 8.0 * s);
                    paint::rounded(scene, card.inflate(s, s), t::CORNER_RADIUS_MEDIUM_DEFAULT as f64 * s + s, t::DIVIDER_COLOR_MEDIUM); paint::rounded(scene, card, t::CORNER_RADIUS_MEDIUM_DEFAULT as f64 * s, t::BACKGROUND_LAYER_2_COLOR);
                    for (i, row) in rows.iter().enumerate() { let rl = fonts.layout_fit(row, Family::Sans, size, t::FONT_WEIGHT_REGULAR as f32, if i == 0 { ink } else { sub }, pw - 24.0 * s); draw_layout(scene, &rl, Affine::translate((card.x0 + 12.0 * s, card.y0 + 6.0 * s + i as f64 * rh + (rh - rl.height() as f64) / 2.0))); } }
            } else { self.op_list = false; if fx > status.x0 + side_w { draw_layout(scene, &fl, Affine::translate((fx, status.y0 + (status.height() - fl.height() as f64) / 2.0))); } }
            let l = fonts.layout_fit(&p.status(), Family::Sans, size, t::FONT_WEIGHT_REGULAR as f32, sub, (fx - 24.0 * s - area.x0 - 12.0 * s).max(40.0 * s));
            draw_layout(scene, &l, Affine::translate((area.x0 + 12.0 * s, status.y0 + (status.height() - l.height() as f64) / 2.0))); }

        // -- the drawn menu bar's strip, the tooltips, and the open menus over everything
        if let Some(mb) = self.menubar.as_ref() { mb.bar.paint(scene, fonts, s); }
        let pt = kurbo::Point::new(self.pointer.0, self.pointer.1); let menu_open = self.menubar.as_ref().is_some_and(|m| m.bar.is_active());
        let mut over: Option<(u64, Rect, String)> = None;
        if !menu_open && self.drag.is_none() {
            for (k, b) in [&self.back, &self.forward, &self.up, &self.list_btn, &self.grid_btn, &self.pane2_btn].iter().enumerate() { if b.hovered() || self.tip_demo.as_deref() == Some(["back", "forward", "up", "list", "grid", "pane2"][k]) { over = Some((k as u64 + 1, b.rect, b.tooltip.clone())); } }
            if self.size.rect.width() > 0.0 && (self.size.hovered() || self.size.dragging()) { over = Some((10, self.size.rect, format!("Icon size  {} px", self.panes[self.active].grid.tile.round()))); }
            if self.search.rect.contains(pt) && self.focus != Focus::Search { over = Some((11, self.search.rect, menu::tip("Search this folder", Cmd::Find))); }
            if self.focus != Focus::Path && self.path_rect.contains(pt) && !self.crumbs.iter().any(|(b, _)| b.hovered()) { over = Some((12, self.path_rect, menu::tip("Type a path", Cmd::GoToFolder))); }
            // a sidebar row whose name is cut, and a place's full path
            if let Some(r) = self.side_hover.and_then(|i| self.side_rows.get(i)) { if let Some(p) = &r.path { over = Some((1000 + self.side_hover.unwrap_or(0) as u64, r.rect, p.to_string_lossy().into_owned())); } else if !r.enabled { over = Some((999, r.rect, "The tracker comes in a later step".into())); } }
        }
        let shown_now = if self.tip_demo.is_some() { now - Duration::from_millis(t::TOOLTIP_DELAY as u64 + 50).min(now.duration_since(self.born)) } else { now };
        self.tips.hover(over.as_ref().map(|(k, r, text)| (*k, *r, text.as_str())), shown_now); self.tips.tick(now); self.tips.paint(scene, fonts, s, Rect::new(0.0, 0.0, w, h), now);
        if let Some(mb) = self.menubar.as_ref() { mb.bar.paint_menus(scene, fonts, s); }
        // drags of files: where a drop can go, for the layer to answer from (folders in view first, then the panes, the
        // sidebar's rows, the path's segments); and the place that would take the drag in hand, outlined
        { let mut zones: Vec<crate::dnd::Zone> = vec![];
            for p in self.panes.iter().take(n) { zones.extend(p.folder_zones().into_iter().map(|(rect, folder)| crate::dnd::Zone { rect, folder })); }
            for r in &self.side_rows { if let (Some(path), true) = (&r.path, r.enabled) { let rect = r.rect.intersect(self.side_rect); if rect.width() > 0.0 && rect.height() > 0.0 { zones.push(crate::dnd::Zone { rect, folder: path.clone() }); } } }
            if self.focus != Focus::Path { for (b, path) in &self.crumbs { if let Some(path) = path { zones.push(crate::dnd::Zone { rect: b.rect, folder: path.clone() }); } } }
            for p in self.panes.iter().take(n) { zones.push(crate::dnd::Zone { rect: p.rect, folder: p.path().to_path_buf() }); }
            let mut d = self.dnd.borrow_mut(); d.zones = zones;
            if let Some(r) = d.hover { let (k, c) = (2.0 * s, t::FOCUS_INDICATOR_COLOR); for q in [Rect::new(r.x0, r.y0, r.x1, r.y0 + k), Rect::new(r.x0, r.y1 - k, r.x1, r.y1), Rect::new(r.x0, r.y0, r.x0 + k, r.y1), Rect::new(r.x1 - k, r.y0, r.x1, r.y1)] { paint::rect(scene, q, c); } } }
        // what went wrong, at the window's foot; an open question over everything
        self.toast.tick(now); self.toast.paint(scene, fonts, Rect::new(0.0, 0.0, w, status.y0), s, now);
        if let Some((d, _)) = self.dialog.as_mut() { let bounds = Rect::new(0.0, 0.0, w, h); d.layout(fonts, bounds, s); d.paint(scene, fonts, s, bounds, now); }

        self.present(now);
    }
    /// The frame is rendered and shown; and the scripted picture is saved when everything has settled.
    fn present(&mut self, now: Instant) {
        self.layer.render(&self.presenter.engine.device, &self.presenter.engine.queue);
        let window = self.window.clone(); self.presenter.draw(|| window.pre_present_notify());

        // (the scripted picture: UFB_SHOT=<file.png> saves the window as drawn - no OS screenshot - once the steps are done,
        // the panes have their listings and the icons have come; then the application ends)
        if self.shot.is_some() && !self.done {
            let idle = self.demo.is_empty() && self.demo_until.is_none_or(|t| now >= t) && !self.busy() && self.icons.pending() == 0 && self.thumbs.pending() == 0 && !self.lightbox.loading();
            if !idle { self.settled = None; } else if self.settled.is_none() { self.settled = Some(now); }
            if self.settled.is_some_and(|t0| now.duration_since(t0) > Duration::from_millis(500)) || self.born.elapsed() > Duration::from_secs(60) {
                if let (Some(path), Some((px, w, h))) = (self.shot.take(), self.presenter.capture_window()) {
                    let oetf = |v: f32| { let v = v.clamp(0.0, 1.0); ((if v <= 0.003_130_8 { 12.92 * v } else { 1.055 * v.powf(1.0 / 2.4) - 0.055 }) * 255.0 + 0.5) as u8 };
                    let bytes: Vec<u8> = (0..(w * h) as usize).flat_map(|i| [oetf(px[i * 4]), oetf(px[i * 4 + 1]), oetf(px[i * 4 + 2])]).collect();
                    if let Some(dir) = path.parent() { let _ = std::fs::create_dir_all(dir); }
                    match std::fs::File::create(&path) { Ok(f) => { let mut e = png::Encoder::new(std::io::BufWriter::new(f), w, h); e.set_color(png::ColorType::Rgb); e.set_depth(png::BitDepth::Eight); let _ = e.write_header().and_then(|mut wr| wr.write_image_data(&bytes)); } Err(e) => eprintln!("UFB_SHOT: {e}") }
                }
                self.done = true;
            }
        }
    }
}
