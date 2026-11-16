//! One window's content (canon row 463's layout): the sidebar (Favorites, Shares, Jobs, All Tracked, the system's
//! places), ONE toolbar for the window that acts on the active pane (back, forward, up, the path bar, list / grid, the
//! grid's size, search, the second pane), one or two panes, the status bar. Drawn with the family's toolkit as
//! minRender's and minNotes' windows are: one vello scene over the presenter, the theme's tokens, min-ui's parts.
//!
//! Nothing here asks the file system for anything: the panes and the sidebar are fed by worker threads through one
//! channel (`lister::Port`), which also wakes the loop.
use crate::crumbs;
use crate::icons::Icons;
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
pub enum Out { NewWindow(PathBuf), Close, Minimize, Quit }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Focus { Pane, Path, Search }
#[derive(Clone, Copy, Debug, PartialEq)]
enum Drag { Sidebar(f64, f64), Split }
struct SideRow { rect: Rect, path: Option<PathBuf>, enabled: bool }

/// Logical px.
const TOOLBAR: f64 = 44.0; const STATUS: f64 = 26.0; const HEAD: f64 = 26.0; const SIDE_ROW: f64 = 28.0; const SIDE_MIN: f64 = 150.0; const SIDE_MAX: f64 = 420.0; const GAP: f64 = 5.0;

pub struct Ui {
    window: Arc<Box<dyn Window>>, presenter: Presenter, layer: UiLayer, fonts: Fonts, tips: Tooltips,
    port: Port, rx: Receiver<Msg>, icons: Icons,
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
        Some(Ui { window, presenter, layer, fonts: Fonts::new(), tips: Tooltips::default(), icons: Icons::new(port.clone()), port, rx,
            panes: vec![pane], active: 0, two: false, split: 0.5, pane_heads: [Rect::ZERO; 2], hidden: false, folders_first: true,
            side_w: t::RAIL_WIDTH_MIN as f64, side_rect: Rect::ZERO, sections: [Section::new("Favorites", true), Section::new("Shares", true), Section::new("Jobs", true), Section::new("Places", true)], favorites, fixed: places::fixed(), volumes: vec![], root: String::new(), side_bar: ScrollBar::default(), side_rows: vec![], side_hover: None,
            back: icon_button("arrow-left", key(Cmd::Back, "Back")), forward: icon_button("arrow-right", key(Cmd::Forward, "Forward")), up: icon_button("arrow-up", key(Cmd::Up, "Enclosing folder")),
            list_btn: icon_button("list", key(Cmd::AsList, "List")), grid_btn: icon_button("squares-four", key(Cmd::AsGrid, "Grid")), pane2_btn: icon_button("columns", "Second pane  (Tab switches panes)".to_string()), size: Slider::new(((96.0 - grid::TILE_MIN) / (grid::TILE_MAX - grid::TILE_MIN)) as f32), search,
            path_rect: Rect::ZERO, crumbs: vec![], crumbs_for: None, field: TextField::new(Family::Sans, t::BODY_SIZE_S), path_error: None, check: None,
            focus: Focus::Pane, drag: None, gaps: [Rect::ZERO; 2], over_gap: None, cursor: winit31::cursor::CursorIcon::Default,
            menubar: InWindowMenu::wanted().then(InWindowMenu::new), pointer: (0.0, 0.0), mods: Mods::default(),
            demo, demo_until: None, shot: std::env::var_os("UFB_SHOT").filter(|_| first).map(PathBuf::from), settled: None, tip_demo: None, born: Instant::now(), done: false, out: vec![] })
    }
    pub fn resize(&mut self, w: u32, h: u32) { if w == 0 || h == 0 { return } self.presenter.resize(w, h); self.presenter.engine.set_ui_scale(self.window.scale_factor() as f32); self.layer.resize(&self.presenter.engine.device, w, h); self.presenter.engine.set_overlay(Some(self.layer.view())); if let Some(mb) = self.menubar.as_mut() { mb.bar.cancel(); } }
    fn scale(&self) -> f64 { self.window.scale_factor() }
    fn pane(&mut self) -> &mut Pane { let i = self.active.min(self.panes.len() - 1); &mut self.panes[i] }
    fn shown(&self) -> usize { if self.two { self.panes.len().min(2) } else { 1 } }
    /// What the menus show of this window.
    pub fn menu_state(&self) -> menu::State { let p = &self.panes[self.active.min(self.panes.len() - 1)]; menu::State { grid: p.mode == Mode::Grid, hidden: self.hidden, folders_first: self.folders_first, second: self.two, sort: p.model.sort, can_back: p.history.can_back(), can_forward: p.history.can_forward(), can_up: p.history.parent().is_some() } }

    // ---------------------------------------------------------------- what the workers send
    /// Take in what the worker threads sent. True when anything came.
    pub fn poll(&mut self) -> bool {
        let mut any = self.icons.poll();
        while let Ok(m) = self.rx.try_recv() {
            any = true;
            match m {
                Msg::Places { volumes, root } => { self.volumes = volumes; self.root = root; self.crumbs_for = None; }
                Msg::Checked { req, path, dir } => if self.check == Some(req) { self.check = None; if dir { self.leave_path(); let port = self.port.clone(); self.pane().go(&port, path); } else { self.path_error = Some(("No such folder".into(), Instant::now())); self.field.error = true; } },
                Msg::Icon => { self.icons.poll(); }
                other => { let mut m = Some(other); let port = self.port.clone(); for p in self.panes.iter_mut() { if p.on_msg(&port, &mut m) { break } } }
            }
        }
        any
    }
    /// The window became active: the volumes are looked for again, and each pane asks whether its folder changed.
    pub fn activated(&mut self) { places::refresh(&self.port); let port = self.port.clone(); let n = self.shown(); for p in self.panes.iter_mut().take(n) { p.activated(&port); } }
    pub fn deactivated(&mut self) { if let Some(mb) = self.menubar.as_mut() { mb.bar.cancel(); } }

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
        match c {
            Cmd::NewWindow => { let p = self.pane().path().to_path_buf(); self.out.push(Out::NewWindow(p)); }
            Cmd::CloseWindow => self.out.push(Out::Close), Cmd::Minimize => self.out.push(Out::Minimize), Cmd::Quit => self.out.push(Out::Quit),
            Cmd::Open => self.pane().open_chosen(&port),
            Cmd::SelectAll => match self.focus { Focus::Path => self.field.select_all(&mut self.fonts), Focus::Search => self.search.select_all(&mut self.fonts), Focus::Pane => self.pane().select_all() },
            Cmd::AsList => self.pane().set_mode(Mode::List), Cmd::AsGrid => self.pane().set_mode(Mode::Grid),
            Cmd::Hidden => { self.hidden = !self.hidden; let on = self.hidden; for p in self.panes.iter_mut() { p.set_hidden(on); } }
            Cmd::FoldersFirst => { self.folders_first = !self.folders_first; let on = self.folders_first; for p in self.panes.iter_mut() { p.set_folders_first(on); } }
            Cmd::SecondPane => self.set_two(!self.two),
            Cmd::Sort(i) => { let by = SortBy::of_column(i as usize); let p = self.pane(); let asc = if p.model.sort == by { !p.model.ascending } else { true }; p.set_sort(by, asc); }
            Cmd::Refresh => { self.pane().refresh(&port); places::refresh(&port); }
            Cmd::Back => self.pane().back(&port), Cmd::Forward => self.pane().forward(&port), Cmd::Up => self.pane().up(&port),
            Cmd::Home => self.go(places::home()), Cmd::Desktop => self.go(places::home().join("Desktop")), Cmd::Downloads => self.go(places::home().join("Downloads")),
            Cmd::GoToFolder => self.edit_path(), Cmd::Find => self.focus_search(),
        }
    }

    // ---------------------------------------------------------------- input
    pub fn wheel(&mut self, dy: f64) {
        let now = Instant::now(); let p = kurbo::Point::new(self.pointer.0, self.pointer.1);
        if self.menubar.as_ref().is_some_and(|m| m.bar.is_open()) { return }
        if self.side_rect.contains(p) { self.side_bar.wheel(dy, now); return }
        let n = self.shown(); for pane in self.panes.iter_mut().take(n) { if pane.rect.contains(p) { pane.wheel(dy, now); return } }
    }
    fn set_cursor(&mut self, c: winit31::cursor::CursorIcon) { if c != self.cursor { self.cursor = c; self.window.set_cursor(winit31::cursor::Cursor::Icon(c)); } }
    pub fn pointer(&mut self, e: &Event) {
        if let Some(p) = e.pos() { self.pointer = p; } let now = Instant::now(); let s = self.scale(); let port = self.port.clone();
        if matches!(e, Event::Down { .. }) { self.tips.pressed(now); self.tip_demo = None; }
        // the drawn menu bar and its open menus take the pointer first
        if let Some(mb) = self.menubar.as_mut() { let (chosen, _, consumed) = mb.bar.event(e, &mut self.fonts); if let Some(c) = chosen.and_then(|id| mb.cmd(id)) { self.command(c); return } if consumed { return } }
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
        // the panes: a press in one makes it the active one
        let n = self.shown();
        if let Event::Down { x, y, .. } = *e { let p = kurbo::Point::new(x, y); for i in 0..n { if self.panes[i].rect.contains(p) || self.pane_heads[i].contains(p) { self.active = i; self.leave_path(); self.leave_search(); self.focus = Focus::Pane; } } }
        for pane in self.panes.iter_mut().take(n) { pane.pointer(e, self.mods, now, &port); }
        let resizing = self.over_gap.is_some() || self.panes.iter().take(n).any(|p| p.resizing());
        let text = at.is_some_and(|p| (self.focus == Focus::Path && self.path_rect.contains(p)) || self.search.rect.contains(p));
        self.set_cursor(if resizing { winit31::cursor::CursorIcon::ColResize } else if text { winit31::cursor::CursorIcon::Text } else { winit31::cursor::CursorIcon::Default });
    }
    /// Text from the clipboard, at the command key with V in a text field. (The window asks for it; this type holds no clipboard.)
    pub fn paste(&mut self, text: &str) { match self.focus { Focus::Path => { self.field.paste(text, &mut self.fonts); } Focus::Search => { if self.search.paste(text, &mut self.fonts) == FieldResponse::Changed { let t = self.search.text().to_string(); self.pane().set_search(&t); } } Focus::Pane => {} } }
    /// A key for the drawn menu bar: (the command of the row chosen, redraw, the key was the bar's).
    pub fn with_menu_key(&mut self, k: &min_ui::BarKey) -> (Option<Cmd>, bool, bool) { match self.menubar.as_mut() { Some(mb) => { let (id, redraw, consumed) = mb.bar.key(k, &mut self.fonts); (id.and_then(|i| mb.cmd(i)), redraw, consumed) } None => (None, false, false) } }
    /// A text field holds the keyboard (the clipboard's keys are its).
    pub fn typing(&self) -> bool { self.focus != Focus::Pane }
    /// The chosen text of the field in hand, for a copy; `cut` takes it out.
    pub fn copy(&mut self, cut: bool) -> Option<String> { match self.focus { Focus::Path => self.field.copy(cut, &mut self.fonts), Focus::Search => { let r = self.search.copy(cut, &mut self.fonts); if cut { let t = self.search.text().to_string(); self.pane().set_search(&t); } r } Focus::Pane => None } }
    /// A key. `name`: the key as the key map names it ("N", "[", "Up", "F5"); `alt`: the Alt / Option key is down;
    /// `menu_keys`: the system's menu bar answers the command-key shortcuts itself (macOS), so they are not taken here.
    pub fn key(&mut self, k: &KeyEvent, name: &str, alt: bool, menu_keys: bool) {
        let now = Instant::now(); let port = self.port.clone(); let mac = cfg!(target_os = "macos"); self.tip_demo = None;
        match self.focus {
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
            Key::Char(c) if c == " " && !k.cmd => { if self.pane().sel().typed(now).is_empty() { return } }
            // Return is Rename on macOS (row 450) - not this slice's; it does not open
            Key::Named(Named::Enter) if mac => return,
            Key::Named(Named::Escape) => { if let Some(mb) = self.menubar.as_mut() { mb.bar.cancel(); } return }
            _ => {}
        }
        if alt && !mac { return }
        self.pane().key(k, now, &port);
    }

    // ---------------------------------------------------------------- the scripted looks
    /// `UFB_DEMO` steps, done one after another once the panes have what they wait for (no system input is involved):
    /// `grid` `list` `size:<px>` `pane2` `tab` `expand:<row>` `collapse:<row>` `select:<a>-<b>` `selectall` `sort:<name|date|size|kind>[:desc]`
    /// `search:<text>` `go:<path>` `back` `forward` `up` `editpath` `typepath:<text>` `hidden` `foldersfirst` `scroll:<row>` `side:<n>` (fold a
    /// sidebar section) `sidebar:<px>` `menu:<letter>` (the drawn menu bar) `tip:<back|up|list|grid|pane2>` `newwindow` `wait:<ms>`.
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
            || self.crumbs.iter().any(|(b, _)| b.animating(now)) || self.sections.iter().any(|sec| sec.animating(now)) || self.size.animating(now) || self.focus != Focus::Pane || self.path_error.is_some() || !self.demo.is_empty() || (self.shot.is_some() && !self.done)
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
        let now = Instant::now(); let (ink, sub) = (paint::color(t::NEUTRAL_CONTENT_COLOR_DEFAULT), paint::color(t::NEUTRAL_SUBDUED_CONTENT_COLOR_DEFAULT));
        if self.active >= self.shown() { self.active = 0; }
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
            self.panes[i].paint(scene, fonts, &mut self.icons, s, now);
        }
        if n == 2 { let hot = self.over_gap == Some(1) || self.drag == Some(Drag::Split); paint::rect(scene, Rect::new(split_x, body.y0, split_x + if hot { 2.0 * s } else { s }, body.y1), if hot { t::RAIL_SPLITTER_COLOR_HOVER } else { t::DIVIDER_COLOR_MEDIUM }); }

        // -- the toolbar
        paint::hline(scene, tool.x0, tool.x1, tool.y1 - s, s, t::DIVIDER_COLOR);
        for b in [&self.back, &self.forward, &self.up, &self.list_btn, &self.grid_btn, &self.pane2_btn] { b.paint(scene, fonts, s, now); }
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
            let fl = fonts.layout(&free, Family::Sans, size, t::FONT_WEIGHT_REGULAR as f32, sub); let fx = status.x1 - 12.0 * s - fl.width() as f64;
            if fx > status.x0 + side_w { draw_layout(scene, &fl, Affine::translate((fx, status.y0 + (status.height() - fl.height() as f64) / 2.0))); }
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

        self.layer.render(&self.presenter.engine.device, &self.presenter.engine.queue);
        let window = self.window.clone(); self.presenter.draw(|| window.pre_present_notify());

        // (the scripted picture: UFB_SHOT=<file.png> saves the window as drawn - no OS screenshot - once the steps are done,
        // the panes have their listings and the icons have come; then the application ends)
        if self.shot.is_some() && !self.done {
            let idle = self.demo.is_empty() && self.demo_until.is_none_or(|t| now >= t) && !self.busy() && self.icons.pending() == 0;
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
