//! The window's content (canon row 405): today's layout, drawn with the family's toolkit in minRender's own blue and
//! its layer-2 surfaces. This first slice shows: the title row, this node's card, the peers, the job list.
//!
//! Built for row 389's scale bar from the start: a list draws only the rows that are in view, whatever its length, and
//! nothing here asks another node or the database for anything - the window is handed a `View` (node.rs).
use crate::node::{ChunkRow, Detail, JobRow, Node, View};
use kurbo::{Affine, Circle, Rect};
use min_present::{Output, Presenter};
use min_theme::tokens as t;
use min_ui::{dialog::Dialog, fonts::{draw_layout, Fonts}, menu::{Menu, MenuItem}, paint, Button, Event, Family, ScrollBar, TextButton, UiLayer, Variant};
use std::collections::BTreeSet;
use std::sync::Arc;
use std::time::Instant;
use vello::Scene;
use winit31::window::Window;

pub struct Ui { window: Arc<Box<dyn Window>>, node: Arc<Node>, presenter: Presenter, layer: UiLayer, fonts: Fonts, jobs_bar: ScrollBar, peers_bar: ScrollBar, pointer: (f64, f64), /// the chosen jobs; the one the cursor is on; where a range began
    sel: BTreeSet<String>, pub cursor: Option<String>, anchor: Option<String>, /// the strip's buttons, with what each asks of the chosen jobs
    strip: Vec<(TextButton, &'static str)>, menu: Menu, menu_acts: Vec<&'static str>, /// the question before Cancel and Delete (0.5.x's own; nothing else asks)
    confirm: Option<(Dialog, &'static str)>, demo: bool, /// the job's detail: the frame grid's and the chunk table's scroll, the chosen chunks, where each lies
    grid_bar: ScrollBar, chunks_bar: ScrollBar, csel: BTreeSet<i64>, ccursor: Option<i64>, canchor: Option<i64>, grid_rect: Rect, chunks_rect: Rect, /// the keys go to the chunk table (the last click was there), not the job list
    on_chunks: bool, /// the panels' sizes, set by dragging the gaps between them and kept for the next start: the node panel's share of the
    /// width, the job list's share of the height beside it, the frame grid's height
    layout: Layout, /// the three gaps that can be dragged (the node panel's edge, under the job list, under the frame grid), and the one in hand or under the pointer
    gaps: [Rect; 4], drag: Option<usize>, over: Option<usize>, /// the preview: the frames it is held to and since when (a pin lasts a minute), the picture as drawn, its "Latest" button, where the pane lies
    pin: Option<((i64, i64), Instant)>, picture: Option<(u64, peniko::ImageBrush)>, latest: TextButton, pane: Rect, shown_job: Option<String>, /// the frame grid as laid out: (cells a row, a cell's size, the first frame)
    grid_geom: (i64, f64, i64), /// this node's Stop / Resume; the peer the menu is for
    node_btn: TextButton, peer_menu: Option<String>, /// the title row's New Job, and its card while it is open
    new_job: TextButton, form: Option<crate::form::Form>, clipboard: Option<arboard::Clipboard>, jobs_rect: Rect, peers_rect: Rect, rows_from: f64, born: Instant, shot: bool }

/// How the window is laid out, as the owner left it (0.5.x kept the same in its own settings): in `window.json` in the
/// data folder - never in `config.json`, which a 0.5.x build on the same machine rewrites without what it does not know.
#[derive(Clone, Copy)]
pub struct Layout { pub left: f64, pub list: f64, pub grid: f64, pub preview: f64, pub size: Option<(f64, f64)> }
impl Layout {
    pub fn load(data: &std::path::Path) -> Layout { let v: serde_json::Value = std::fs::read_to_string(data.join("window.json")).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or(serde_json::Value::Null); let f = |k: &str, d: f64| v[k].as_f64().unwrap_or(d);
        Layout { left: f("left", 0.28).clamp(0.1, 0.6), list: f("list", 0.34).clamp(0.1, 0.8), grid: f("grid", 220.0).clamp(60.0, 2000.0), preview: f("preview", 240.0).clamp(80.0, 2000.0), size: v["width"].as_f64().zip(v["height"].as_f64()).filter(|(w, h)| *w >= 640.0 && *h >= 400.0) } }
    pub fn save(&self, data: &std::path::Path) { let mut v = serde_json::json!({ "left": self.left, "list": self.list, "grid": self.grid, "preview": self.preview }); if let Some((w, h)) = self.size { v["width"] = w.into(); v["height"] = h.into(); } let _ = std::fs::write(data.join("window.json"), serde_json::to_string_pretty(&v).unwrap_or_default()); }
}
/// The job list's columns: (header, default width).
const COLUMNS: [(&str, f64); 6] = [("Pri", 36.0), ("Name", 300.0), ("State", 90.0), ("Progress", 280.0), ("Duration", 80.0), ("Created", 140.0)];
/// The chunk table's columns.
const CHUNK_COLUMNS: [(&str, f64); 6] = [("State", 80.0), ("Frames", 120.0), ("Node", 160.0), ("Progress", 120.0), ("Duration", 70.0), ("Retries", 60.0)];
const CHUNK_ROW: f64 = 24.0; const CELL: f64 = 10.0;
/// The frame grid's own colours (minRender's): the pit behind it, a frame nobody has, one finished inside a chunk that is not.
const GRID_PIT: min_theme::Color = min_theme::Color::rgba(0.051, 0.051, 0.051, 1.0); const GRID_FREE: min_theme::Color = min_theme::Color::rgba(0.188, 0.188, 0.188, 1.0); const GRID_RENDERED: min_theme::Color = min_theme::Color::rgba(0.165, 0.510, 0.157, 1.0);
/// A chunk's colour, the same in the table and in the grid (row 389): given out - the accent; done - green; failed - red; waiting or stopped - grey.
fn chunk_color(state: &str) -> min_theme::Color { match state { "assigned" => t::ACCENT_VISUAL_COLOR, "completed" => t::POSITIVE_VISUAL_COLOR, "failed" => t::NEGATIVE_VISUAL_COLOR, _ => t::NEUTRAL_SUBDUED_CONTENT_COLOR_DEFAULT } }
const ROW: f64 = 32.0; const HEADER: f64 = 22.0; const PEER_ROW: f64 = 52.0; const TITLE: f64 = 40.0; const STRIP: f64 = 36.0;
fn duration(ms: i64) -> String { let s = ms / 1000; if s < 60 { format!("{s}s") } else if s < 3600 { format!("{}m {}s", s / 60, s % 60) } else { format!("{}h {}m", s / 3600, s % 3600 / 60) } }
/// The word and colour a job's state is shown with (a completed job with stopped chunks reads "partial").
fn state_of(j: &JobRow) -> (&str, min_theme::Color) { match j.state.as_str() { "completed" if j.stopped > 0 => ("partial", t::NOTICE_VISUAL_COLOR), "active" => ("active", t::POSITIVE_VISUAL_COLOR), "paused" => ("paused", t::NOTICE_VISUAL_COLOR),
    "cancelled" => ("cancelled", t::NEGATIVE_VISUAL_COLOR), "failed" => ("failed", t::NEGATIVE_VISUAL_COLOR), "completed" => ("completed", t::INFORMATIVE_VISUAL_COLOR), other => (other, t::NEUTRAL_SUBDUED_CONTENT_COLOR_DEFAULT) } }

impl Ui {
    pub fn new(window: Arc<Box<dyn Window>>, node: Arc<Node>) -> Option<Ui> {
        let instance = Presenter::instance(); let surface = instance.create_surface(window.clone()).ok()?; let size = window.surface_size();
        #[cfg(target_os = "macos")] let view = { use raw_window_handle::{HasWindowHandle, RawWindowHandle}; match window.window_handle().ok()?.as_raw() { RawWindowHandle::AppKit(h) => Some(h.ns_view.as_ptr()), _ => None } };
        #[cfg(not(target_os = "macos"))] let view = None;
        let mut presenter = Presenter::new(&instance, surface, (size.width.max(1), size.height.max(1)), Output::Auto, view).ok()?; presenter.engine.set_ui_scale(window.scale_factor() as f32);
        let layer = UiLayer::new(&presenter.engine.device, size.width.max(1), size.height.max(1)).ok()?; presenter.engine.set_overlay(Some(layer.view()));
        let node_data = node.data.clone(); let b = |l: &str, v: Variant| TextButton::new(l, v).size_s();
        let strip = vec![(b("Pause", Variant::Secondary), "pause"), (b("Resume", Variant::Secondary), "resume"), (b("Retry failed", Variant::Secondary), "retry-failed"), (b("Requeue", Variant::Secondary), "requeue"), (b("Archive", Variant::Secondary), "archive"), (b("Cancel", Variant::Secondary), "cancel"), (b("Delete", Variant::Negative), "delete")];
        Some(Ui { window, node, presenter, layer, fonts: Fonts::new(), jobs_bar: ScrollBar::default(), peers_bar: ScrollBar::default(), pointer: (0.0, 0.0), sel: BTreeSet::new(), cursor: None, anchor: None, strip, menu: Menu::new(), menu_acts: Vec::new(), confirm: None, demo: false, grid_bar: ScrollBar::default(), chunks_bar: ScrollBar::default(), csel: BTreeSet::new(), ccursor: None, canchor: None, grid_rect: Rect::ZERO, chunks_rect: Rect::ZERO, on_chunks: false, layout: Layout::load(&node_data), gaps: [Rect::ZERO; 4], drag: None, over: None, pin: None, picture: None, latest: TextButton::new("Latest", Variant::Secondary).size_s(), pane: Rect::ZERO, shown_job: None, grid_geom: (1, 10.0, 0), node_btn: TextButton::new("Stop", Variant::Secondary).size_s(), peer_menu: None, new_job: TextButton::new("New Job", Variant::Primary).size_s(), form: None, clipboard: arboard::Clipboard::new().ok(), jobs_rect: Rect::ZERO, peers_rect: Rect::ZERO, rows_from: 0.0, born: Instant::now(), shot: false })
    }
    pub fn resize(&mut self, w: u32, h: u32) { if w == 0 || h == 0 { return } let s = self.window.scale_factor(); self.layout.size = Some((w as f64 / s, h as f64 / s)); self.layout.save(&self.node.data); self.presenter.resize(w, h); self.presenter.engine.set_ui_scale(self.window.scale_factor() as f32); self.layer.resize(&self.presenter.engine.device, w, h); self.presenter.engine.set_overlay(Some(self.layer.view())); }
    pub fn wheel(&mut self, dy: f64) { let now = Instant::now(); let p = kurbo::Point::new(self.pointer.0, self.pointer.1);
        if self.peers_rect.contains(p) { self.peers_bar.wheel(dy, now); } else if self.grid_rect.contains(p) { self.grid_bar.wheel(dy, now); } else if self.chunks_rect.contains(p) { self.chunks_bar.wheel(dy, now); } else { self.jobs_bar.wheel(dy, now); } }
    fn chunk_at(&self, y: f64, d: &Detail) -> Option<usize> { let s = self.window.scale_factor(); let i = ((y - self.chunks_rect.y0 + self.chunks_bar.offset) / (CHUNK_ROW * s)) as usize; (y >= self.chunks_rect.y0 && i < d.chunks.len()).then_some(i) }
    fn choose_chunk(&mut self, i: usize, d: &Detail, shift: bool, cmd: bool) {
        let id = d.chunks[i].id;
        if shift { let a = self.canchor.and_then(|a| d.chunks.iter().position(|c| c.id == a)).unwrap_or(i); self.csel = d.chunks[a.min(i)..=a.max(i)].iter().map(|c| c.id).collect(); }
        else if cmd { if !self.csel.remove(&id) { self.csel.insert(id); } self.canchor = Some(id); } else { self.csel = BTreeSet::from([id]); self.canchor = Some(id); }
        self.ccursor = Some(id); self.pin = Some(((d.chunks[i].a, d.chunks[i].b), Instant::now()));   // the preview follows the chunk in hand
    }
    /// The chosen jobs that a command applies to (row 389: a command is offered when it applies to at least one).
    fn applies(&self, view: &View, what: &str) -> Vec<String> { view.jobs.iter().filter(|j| self.sel.contains(&j.id) && match what { "pause" => j.state == "active", "resume" => j.state == "paused", _ => true }).map(|j| j.id.clone()).collect() }
    /// A command for the chosen jobs: Cancel and Delete ask first, as they do in 0.5.x; the others act. The choice is
    /// let go afterwards and the cursor stays.
    fn act(&mut self, view: &View, what: &'static str, asked: bool) {
        let ids = self.applies(view, what); if ids.is_empty() { return } let n = ids.len(); let s = if n == 1 { "" } else { "s" };
        if !asked && (what == "cancel" || what == "delete") {
            let mut d = if what == "cancel" { Dialog::new(&format!("Cancel {n} job{s}?"), "This stops dispatch and immediately kills in-flight renders on all nodes. The jobs can be requeued later.", vec![("Cancel", Variant::Secondary), ("Cancel jobs", Variant::Negative)]) }
                else { Dialog::new(&format!("Delete {n} job{s}?"), "Removes job records and SQLite history. Output files on disk are NOT deleted. This cannot be undone.", vec![("Cancel", Variant::Secondary), ("Delete", Variant::Negative)]) };
            d.show(); self.confirm = Some((d, what)); return }
        self.node.job_command(ids, what); self.sel.clear();
    }
    fn row_at(&self, y: f64, view: &View) -> Option<usize> { let s = self.window.scale_factor(); if y < self.rows_from { return None } let i = ((y - self.rows_from + self.jobs_bar.offset) / (ROW * s)) as usize; (i < view.jobs.len()).then_some(i) }
    fn choose(&mut self, i: usize, view: &View, shift: bool, cmd: bool) {
        let id = view.jobs[i].id.clone();
        if shift { let a = self.anchor.as_ref().and_then(|a| view.jobs.iter().position(|j| j.id == *a)).unwrap_or(i); self.sel = view.jobs[a.min(i)..=a.max(i)].iter().map(|j| j.id.clone()).collect(); }
        else if cmd { if !self.sel.remove(&id) { self.sel.insert(id.clone()); } self.anchor = Some(id.clone()); }
        else { self.sel = BTreeSet::from([id.clone()]); self.anchor = Some(id.clone()); }
        self.cursor = Some(id);
    }
    /// A pointer event. `mods`: (shift, the command key).
    pub fn pointer(&mut self, e: &Event, view: &View, mods: (bool, bool)) {
        if let Some(p) = e.pos() { self.pointer = p; } let now = Instant::now(); let s = self.window.scale_factor();
        // what lies over the lists takes the pointer first: the New Job card, the question, the menu
        if let Some(f) = self.form.as_mut() { if let crate::form::Out::Close = f.pointer(e, &mut self.fonts, &self.node) { self.form = None; } return }
        if self.new_job.event(e, now).0 { self.form = Some(crate::form::Form::open(&self.node, &mut self.fonts)); return }
        if let Some((d, what)) = self.confirm.as_mut() { let what = *what; d.pointer(e, &mut self.fonts); if let (Some(i), _) = d.event(e, now) { self.confirm = None; if i == 1 { self.act(view, what, true); } } return }
        if self.menu.is_open() { let (chosen, _, _) = self.menu.event(e); if let Some(i) = chosen { let what = self.menu_acts.get(i).copied().unwrap_or(""); self.menu.close();
            // a chunk command goes to the chunks it applies to: back to the queue - those given out, failed or stopped; stop - those waiting or given out
            if let Some(c) = what.strip_prefix("chunk:") { if let Some(d) = self.node.detail() { let ids: Vec<i64> = d.chunks.iter().filter(|x| self.csel.contains(&x.id) && if c == "stop" { matches!(x.state.as_str(), "pending" | "assigned") } else { matches!(x.state.as_str(), "assigned" | "failed" | "stopped") }).map(|x| x.id).collect(); if !ids.is_empty() { self.node.chunk_command(ids, if c == "stop" { "stop" } else { "reassign" }); } } }
            else if let Some(c) = what.strip_prefix("peer:") { if let Some(id) = self.peer_menu.take() { self.node.peer_command(vec![id], match c { "stop" => "stop", "start" => "start", "restart" => "restart", "unsuspend" => "unsuspend", _ => "signal" }); } }
            else if !what.is_empty() { self.act(view, what, false); } } return }
        // the gaps between the panels: dragged, they resize what lies on either side; the sizes are kept when the drag ends
        { let size = self.window.surface_size(); let (w, h) = (size.width as f64, size.height as f64); let top = (TITLE + 12.0) * s;
            match *e {
                Event::Down { x, y, button: Button::Primary } => if let Some(i) = self.gaps.iter().position(|g| g.inflate(2.0 * s, 2.0 * s).contains(kurbo::Point::new(x, y))) { self.drag = Some(i); return },
                Event::Move { x, y } => { if let Some(i) = self.drag { match i { 0 => self.layout.left = (x / w).clamp(260.0 * s / w, 0.6), 1 => self.layout.list = ((y - top) / (h - top)).clamp(0.1, 0.8), 2 => self.layout.grid = ((y - self.grid_rect.y0 + 4.0 * s) / s).clamp(60.0, 2000.0), _ => self.layout.preview = ((h - 12.0 * s - 32.0 * s - y) / s).clamp(80.0, (h / s - 320.0).max(80.0)) } return }
                    let over = self.gaps.iter().position(|g| g.inflate(2.0 * s, 2.0 * s).contains(kurbo::Point::new(x, y)));
                    if over != self.over { self.over = over; self.window.set_cursor(winit31::cursor::Cursor::Icon(match over { Some(0) => winit31::cursor::CursorIcon::ColResize, Some(_) => winit31::cursor::CursorIcon::RowResize, None => winit31::cursor::CursorIcon::Default })); } }
                Event::Up { .. } => if self.drag.take().is_some() { self.layout.save(&self.node.data); return },
                _ => {}
            } }
        if self.jobs_bar.event(e, now).1 || self.peers_bar.event(e, now).1 || self.grid_bar.event(e, now).1 || self.chunks_bar.event(e, now).1 { return }
        if self.node_btn.event(e, now).0 { self.node.toggle_stopped(); return } if self.pin.is_some() && self.latest.event(e, now).0 { self.pin = None; return }
        // a frame's cell in the grid: the preview is held to that frame
        if let Event::Down { x, y, button: Button::Primary } = *e { if self.grid_rect.contains(kurbo::Point::new(x, y)) { let (per_row, slot, first) = self.grid_geom; let (col, row) = (((x - self.grid_rect.x0) / slot) as i64, ((y - self.grid_rect.y0 + self.grid_bar.offset) / slot) as i64);
            if col < per_row { let f = first + row * per_row + col; if self.node.detail().is_some_and(|d| f <= d.last) { self.pin = Some(((f, f), now)); } } return } }
        // a peer: the right button opens what can be asked of it
        if let Event::Down { x, y, button: Button::Secondary } = *e { if self.peers_rect.contains(kurbo::Point::new(x, y)) { let i = ((y - self.peers_rect.y0 + self.peers_bar.offset) / (PEER_ROW * s)) as usize; if let Some(p) = view.peers.get(i) {
            let (items, acts) = if p.alive { (vec![MenuItem::action(&p.id, false), MenuItem::Separator, MenuItem::action("Stop", !p.stopped), MenuItem::action("Start", p.stopped), MenuItem::action("Restart App", true), MenuItem::action("Unsuspend", true)], vec!["", "", "peer:stop", "peer:start", "peer:restart", "peer:unsuspend"]) }
                else { (vec![MenuItem::action(&p.id, false), MenuItem::Separator, MenuItem::action("Write restart signal (filesystem)", true)], vec!["", "", "peer:signal"]) };
            self.peer_menu = Some(p.id.clone()); self.menu_acts = acts; let size = self.window.surface_size(); self.menu.open_at(items, &mut self.fonts, x, y, Rect::new(0.0, 0.0, size.width as f64, size.height as f64), s); } return } }
        if let Event::Down { x, y, button } = *e { if self.chunks_rect.contains(kurbo::Point::new(x, y)) { self.on_chunks = true; if let Some(d) = self.node.detail() { match self.chunk_at(y, &d) {
            Some(i) => { if button == Button::Secondary { if !self.csel.contains(&d.chunks[i].id) { self.choose_chunk(i, &d, false, false); }
                    let chosen: Vec<&ChunkRow> = d.chunks.iter().filter(|c| self.csel.contains(&c.id)).collect(); let n = chosen.len(); let all_stopped = chosen.iter().all(|c| c.state == "stopped"); let back = chosen.iter().filter(|c| matches!(c.state.as_str(), "assigned" | "failed" | "stopped")).count(); let stoppable = chosen.iter().filter(|c| matches!(c.state.as_str(), "pending" | "assigned")).count();
                    let first = if all_stopped { "Requeue (resume stopped chunk)".to_string() } else { "Return to queue".to_string() }; let first = if n > 1 { format!("{first} ({back} chunks)") } else { first };
                    let items = vec![MenuItem::action(&first, back > 0), MenuItem::action(&if n > 1 { format!("Stop {stoppable} chunk(s)") } else { "Stop chunk".to_string() }, stoppable > 0)];
                    self.menu_acts = vec!["chunk:reassign", "chunk:stop"]; let size = self.window.surface_size(); self.menu.open_at(items, &mut self.fonts, x, y, Rect::new(0.0, 0.0, size.width as f64, size.height as f64), s); }
                else { self.choose_chunk(i, &d, mods.0, mods.1); } }
            None => self.csel.clear() } } return } }
        if matches!(e, Event::Down { .. }) && self.jobs_rect.contains(kurbo::Point::new(self.pointer.0, self.pointer.1)) { self.on_chunks = false; }
        let mut asked = None; for (b, what) in self.strip.iter_mut() { if b.event(e, now).0 { asked = Some(*what); } } if let Some(w) = asked { self.act(view, w, false); return }
        match *e {
            Event::Down { x, y, button: Button::Primary } if self.jobs_rect.contains(kurbo::Point::new(x, y)) => match self.row_at(y, view) { Some(i) => self.choose(i, view, mods.0, mods.1), None => self.sel.clear() },
            // the right button keeps a choice that holds the row, else chooses the row; then the menu, at the pointer
            Event::Down { x, y, button: Button::Secondary } if self.jobs_rect.contains(kurbo::Point::new(x, y)) => { if let Some(i) = self.row_at(y, view) {
                if !self.sel.contains(&view.jobs[i].id) { self.choose(i, view, false, false); }
                let n = self.sel.len(); let many = |l: &str| if n > 1 { format!("{l} {n} jobs") } else { l.to_string() }; let st = view.jobs[i].state.as_str();
                let items = vec![MenuItem::action(&many("Pause"), n > 1 || st == "active"), MenuItem::action(&many("Resume"), n > 1 || st == "paused"), MenuItem::Separator, MenuItem::action(&many("Retry failed chunks"), true), MenuItem::action(&many("Requeue"), true), MenuItem::Separator, MenuItem::action(&many("Cancel"), true), MenuItem::action(&many("Archive"), true), MenuItem::action(&many("Delete"), true)];
                self.menu_acts = vec!["pause", "resume", "", "retry-failed", "requeue", "", "cancel", "archive", "delete"];
                let size = self.window.surface_size(); self.menu.open_at(items, &mut self.fonts, x, y, Rect::new(0.0, 0.0, size.width as f64, size.height as f64), s); } }
            _ => {}
        }
    }
    /// A key for the job list: the arrows, Home, End and the page keys move the cursor (with Shift the choice grows from
    /// where it began); the command key with A chooses all; Escape lets the choice go; Delete asks to delete it.
    /// A key as it was pressed. The New Job card takes every key while it is open (the command key with V pastes); else
    /// the lists take theirs.
    pub fn key_event(&mut self, k: &min_ui::KeyEvent, view: &View, mods: (bool, bool)) {
        if let Some(f) = self.form.as_mut() { let pasted = if k.cmd && matches!(&k.key, min_ui::Key::Char(c) if c.eq_ignore_ascii_case("v")) { Some(self.clipboard.as_mut().and_then(|cb| cb.get_text().ok()).unwrap_or_default()) } else { None }; f.key(k, pasted, &mut self.fonts, &self.node); return }
        use min_ui::{Key, Named}; let name = match &k.key { Key::Named(Named::Up) => "Up", Key::Named(Named::Down) => "Down", Key::Named(Named::Home) => "Home", Key::Named(Named::End) => "End", Key::Named(Named::Escape) => "Escape", Key::Named(Named::Delete) => "Delete", Key::Named(Named::Backspace) => "Backspace",
            Key::Char(c) if c == "PageUp" => "PageUp", Key::Char(c) if c == "PageDown" => "PageDown", Key::Char(c) if c.eq_ignore_ascii_case("a") => "a", _ => "" };
        if !name.is_empty() { self.key(name, view, mods); }
    }
    fn key(&mut self, name: &str, view: &View, mods: (bool, bool)) {
        if self.confirm.is_some() { if name == "Escape" { self.confirm = None; } return } if self.menu.is_open() { if name == "Escape" { self.menu.close(); } return }
        if self.on_chunks { let Some(d) = self.node.detail() else { return }; let n = d.chunks.len(); if n == 0 { return } let s = self.window.scale_factor(); let page = ((self.chunks_rect.height() / (CHUNK_ROW * s)) as usize).saturating_sub(1).max(1);
            let at = self.ccursor.and_then(|c| d.chunks.iter().position(|x| x.id == c));
            let to = match name { "Up" => Some(at.map(|a| a.saturating_sub(1)).unwrap_or(0)), "Down" => Some(at.map(|a| (a + 1).min(n - 1)).unwrap_or(0)), "Home" => Some(0), "End" => Some(n - 1), "PageUp" => Some(at.unwrap_or(0).saturating_sub(page)), "PageDown" => Some((at.unwrap_or(0) + page).min(n - 1)), _ => None };
            if let Some(i) = to { self.choose_chunk(i, &d, mods.0, false); let (top, h) = (i as f64 * CHUNK_ROW * s, self.chunks_rect.height()); if top < self.chunks_bar.offset { self.chunks_bar.offset = top; } else if top + CHUNK_ROW * s > self.chunks_bar.offset + h { self.chunks_bar.offset = top + CHUNK_ROW * s - h; } }
            else if name == "a" && mods.1 { self.csel = d.chunks.iter().map(|c| c.id).collect(); } else if name == "Escape" { self.csel.clear(); }
            return }
        let n = view.jobs.len(); if n == 0 { return } let s = self.window.scale_factor(); let page = ((self.jobs_rect.height() / (ROW * s)) as usize).saturating_sub(1).max(1);
        let at = self.cursor.as_ref().and_then(|c| view.jobs.iter().position(|j| j.id == *c));
        let to = match name { "Up" => Some(at.map(|a| a.saturating_sub(1)).unwrap_or(0)), "Down" => Some(at.map(|a| (a + 1).min(n - 1)).unwrap_or(0)), "Home" => Some(0), "End" => Some(n - 1), "PageUp" => Some(at.unwrap_or(0).saturating_sub(page)), "PageDown" => Some((at.unwrap_or(0) + page).min(n - 1)), _ => None };
        if let Some(i) = to { self.choose(i, view, mods.0, false); let (top, h) = (i as f64 * ROW * s, self.jobs_rect.height()); if top < self.jobs_bar.offset { self.jobs_bar.offset = top; } else if top + ROW * s > self.jobs_bar.offset + h { self.jobs_bar.offset = top + ROW * s - h; } return }
        match name { "a" if mods.1 => self.sel = view.jobs.iter().map(|j| j.id.clone()).collect(), "Escape" => self.sel.clear(), "Delete" => self.act(view, "delete", false), "Backspace" if mods.1 => self.act(view, "delete", false), _ => {} }
    }
    pub fn redraw(&mut self, view: &View) {
        let s = self.window.scale_factor(); let size = self.window.surface_size(); let (w, h) = (size.width as f64, size.height as f64); if w < 1.0 || h < 1.0 { return }
        let now = Instant::now(); let now_ms = crate::node::now_ms() as i64;
        let (ink, sub) = (paint::color(t::NEUTRAL_CONTENT_COLOR_DEFAULT), paint::color(t::NEUTRAL_SUBDUED_CONTENT_COLOR_DEFAULT));
        // (scripted looks: MR_DEMO=select chooses the second to fourth job, so that a picture shows the strip alive)
        // (MR_DEMO=newjob opens the New Job card)
        if !self.demo { if let Some(d) = std::env::var("MR_DEMO").ok().filter(|d| d.starts_with("newjob")) { self.demo = true; let mut f = crate::form::Form::open(&self.node, &mut self.fonts); if let Some(word) = d.strip_prefix("newjob:") { f.demo(word, &mut self.fonts, &self.node); } self.form = Some(f); } }
        // (MR_DEMO=open:<job id> puts the cursor on that job, so that a picture shows its detail)
        if !self.demo { if let Some(id) = std::env::var("MR_DEMO").ok().and_then(|d| d.strip_prefix("open:").map(str::to_string)) { if view.jobs.iter().any(|j| j.id == id) { self.demo = true; self.sel = BTreeSet::from([id.clone()]); self.cursor = Some(id); } } }
        if self.sel.is_empty() && !self.demo && view.jobs.len() > 4 { if let Some(demo) = std::env::var("MR_DEMO").ok().filter(|d| !d.starts_with("open:") && !d.starts_with("newjob")) { self.demo = true; self.sel = view.jobs[1..4].iter().map(|j| j.id.clone()).collect(); self.cursor = Some(view.jobs[2].id.clone());
            // (MR_DEMO=pause | cancel | ...: the command is given as a click on its button would give it)
            if demo != "select" { let what: &'static str = Box::leak(demo.into_boxed_str()); self.act(view, what, what != "cancel" && what != "delete"); self.sel = view.jobs[1..4].iter().map(|j| j.id.clone()).collect(); } } }
        let strip_labels: Vec<(f64, &'static str)> = { let fonts = &mut self.fonts; self.strip.iter().map(|(b, w)| (b.measure(fonts, s).0, *w)).collect() }; let mut strip_rects: Vec<(Rect, bool)> = Vec::new(); let sel = self.sel.clone();
        let scene: &mut Scene = self.layer.scene_mut(); scene.reset(); let fonts = &mut self.fonts;
        // one line of text, left-aligned at x, centred on cy, cut to the room it has
        let mut text = |scene: &mut Scene, txt: &str, x: f64, cy: f64, size: f32, bold: bool, mono: bool, color: peniko::Color, room: f64| -> f64 { if room < 4.0 { return 0.0 }
            let l = fonts.layout_fit(txt, if mono { Family::Mono } else { Family::Sans }, size * s as f32, if bold { t::FONT_WEIGHT_BOLD } else { t::FONT_WEIGHT_REGULAR } as f32, color, room); draw_layout(scene, &l, Affine::translate((x, cy - l.height() as f64 / 2.0))); l.width() as f64 };
        // The surfaces, as the family's other applications stack them in their panels: the window is layer 1, a card
        // on it layer 2, and what is sunk into a card - a bar's track, the frame grid's pit - the base colour.
        paint::rect(scene, Rect::new(0.0, 0.0, w, h), t::BACKGROUND_LAYER_1_COLOR);
        // -- the title row: this node, whether it leads, whether it takes work
        let me = &view.me; let host = me["hostname"].as_str().unwrap_or(""); let stopped = me["node_state"] == "stopped"; let pad = 12.0 * s;
        let mut x = pad + text(scene, host, pad, TITLE * s / 2.0, t::BODY_SIZE_M, true, false, ink, 320.0 * s) + 10.0 * s;
        if view.leads { let lw = 58.0 * s; paint::rounded(scene, Rect::new(x, TITLE * s / 2.0 - 9.0 * s, x + lw, TITLE * s / 2.0 + 9.0 * s), 4.0 * s, t::ACCENT_BACKGROUND_COLOR_DEFAULT); text(scene, "LEADER", x + 8.0 * s, TITLE * s / 2.0, t::DETAIL_SIZE_S, true, false, paint::color(t::ACCENT_CONTENT_COLOR_ON_FILL), lw); x += lw + 10.0 * s; }
        scene.fill(peniko::Fill::NonZero, Affine::IDENTITY, paint::color(if stopped { t::NEUTRAL_SUBDUED_CONTENT_COLOR_DEFAULT } else { t::POSITIVE_VISUAL_COLOR }), None, &Circle::new((x + 4.0 * s, TITLE * s / 2.0), 4.0 * s));
        paint::hline(scene, 0.0, w, TITLE * s, t::DIVIDER_THICKNESS as f64 * s, t::DIVIDER_COLOR_MEDIUM);
        // -- the node panel
        let left = (w * self.layout.left).max(260.0 * s).min(w * 0.6); let top = TITLE * s + pad;
        let card = Rect::new(pad, top, left - pad / 2.0, top + 5.0 * 24.0 * s + 16.0 * s); paint::rounded(scene, card, t::CORNER_RADIUS_500 as f64 * s, t::BACKGROUND_LAYER_2_COLOR);
        let ram = me["ram_mb"].as_u64().unwrap_or(0); let ram = if ram == 0 { "unknown".to_string() } else if ram >= 1024 { format!("{:.1} GB", ram as f64 / 1024.0) } else { format!("{ram} MB") };
        let tags = me["tags"].as_array().map(|a| a.iter().filter_map(|x| x.as_str()).collect::<Vec<_>>().join(", ")).unwrap_or_default(); let dash = |v: &str| if v.is_empty() { "\u{2014}".to_string() } else { v.to_string() };
        for (i, (k, v, mono)) in [("Host", host.to_string(), false), ("Node ID", me["node_id"].as_str().unwrap_or("").to_string(), true), ("GPU", dash(me["gpu_name"].as_str().unwrap_or("")), false), ("CPU / RAM", format!("{} cores \u{b7} {ram}", me["cpu_cores"]), false), ("Tags", dash(&tags), false)].iter().enumerate() {
            let cy = card.y0 + 8.0 * s + (i as f64 + 0.5) * 24.0 * s; text(scene, k, card.x0 + 12.0 * s, cy, t::DETAIL_SIZE_M, false, false, sub, 80.0 * s); text(scene, v, card.x0 + 100.0 * s, cy, t::DETAIL_SIZE_M, false, *mono, ink, card.width() - 112.0 * s); }
        // this node's Stop / Resume, under its card (Stop ends a render under way at once: row 404)
        let nb = Rect::new(card.x0, card.y1 + 8.0 * s, card.x1, card.y1 + 8.0 * s + 24.0 * s); let node_btn_rect = nb;
        // the preview's place at the foot of the panel, when a job is in hand: its header, the pane, its foot
        let pv_h = if self.cursor.is_some() { (self.layout.preview * s).min(h - top - 320.0 * s).max(80.0 * s) } else { 0.0 };
        let pane = if pv_h > 0.0 { Rect::new(pad, h - pad - 32.0 * s - pv_h, left - pad / 2.0, h - pad - 32.0 * s) } else { Rect::ZERO }; self.pane = pane;
        self.gaps[3] = if pv_h > 0.0 { Rect::new(pad, pane.y0 - 32.0 * s - pad / 2.0, left - pad / 2.0, pane.y0 - 32.0 * s) } else { Rect::ZERO };
        let peers_bottom = if pv_h > 0.0 { pane.y0 - 32.0 * s - pad / 2.0 } else { h - pad };
        let py = nb.y1 + pad; let online = view.peers.iter().filter(|p| p.alive).count();
        let hw = text(scene, "Peers", pad, py + 10.0 * s, t::BODY_SIZE_S, true, false, ink, 100.0 * s); text(scene, &format!("\u{b7} {online} online"), pad + hw + 6.0 * s, py + 10.0 * s, t::DETAIL_SIZE_M, false, false, sub, 160.0 * s);
        self.peers_rect = Rect::new(pad, py + 24.0 * s, left - pad / 2.0, peers_bottom.max(py + 24.0 * s)); self.peers_bar.set(self.peers_rect, view.peers.len() as f64 * PEER_ROW * s, s);
        scene.push_clip_layer(peniko::Fill::NonZero, Affine::IDENTITY, &self.peers_rect);
        let first = (self.peers_bar.offset / (PEER_ROW * s)) as usize;
        for (i, p) in view.peers.iter().enumerate().skip(first).take((self.peers_rect.height() / (PEER_ROW * s)) as usize + 2) {
            let y = self.peers_rect.y0 + i as f64 * PEER_ROW * s - self.peers_bar.offset; let r = Rect::new(self.peers_rect.x0, y, self.peers_rect.x1 - 8.0 * s, y + PEER_ROW * s - 4.0 * s);
            paint::rounded(scene, r, t::CORNER_RADIUS_500 as f64 * s, t::BACKGROUND_LAYER_2_COLOR);
            let dot = if !p.alive { t::NEUTRAL_SUBDUED_CONTENT_COLOR_DEFAULT } else if p.health_ok { t::POSITIVE_VISUAL_COLOR } else { t::NEGATIVE_VISUAL_COLOR };
            scene.fill(peniko::Fill::NonZero, Affine::IDENTITY, paint::color(dot), None, &Circle::new((r.x0 + 14.0 * s, r.center().y), 5.0 * s));
            let name = if p.host.is_empty() { &p.id } else { &p.host }; let nw = text(scene, name, r.x0 + 28.0 * s, r.y0 + 16.0 * s, t::BODY_SIZE_S, false, false, if p.alive { ink } else { sub }, r.width() - 100.0 * s);
            if p.leader { text(scene, "LEADER", r.x0 + 28.0 * s + nw + 8.0 * s, r.y0 + 16.0 * s, t::DETAIL_SIZE_S, true, false, paint::color(t::ACCENT_VISUAL_COLOR), 60.0 * s); }
            let line = if !p.alive { "offline".to_string() } else if !p.job.is_empty() { format!("rendering \u{b7} {}", p.job) } else if p.stopped { "stopped".to_string() } else { "idle".to_string() };
            text(scene, &line, r.x0 + 28.0 * s, r.y0 + 34.0 * s, t::DETAIL_SIZE_M, false, false, sub, r.width() - 36.0 * s);
        }
        scene.pop_layer(); self.peers_bar.paint(scene, now);
        // -- the preview (the spec's 3): the job's latest frame, or the frames the grid or the chunk table hold it to
        if self.shown_job != self.cursor { self.shown_job = self.cursor.clone(); self.pin = None; self.picture = None; }
        if self.pin.is_some_and(|(_, t)| t.elapsed().as_secs() >= 60) { self.pin = None; }
        self.node.show_preview(self.cursor.clone().map(|j| (j, self.pin.map(|(r, _)| r)))); let pv = self.node.preview();
        let mut latest_rect = Rect::ZERO;
        if pv_h > 0.0 {
            let hy = pane.y0 - 16.0 * s; let hw = text(scene, "Preview", pad, hy, t::BODY_SIZE_S, true, false, ink, 100.0 * s); text(scene, &format!("\u{b7} {}", self.cursor.as_deref().unwrap_or("")), pad + hw + 6.0 * s, hy, t::DETAIL_SIZE_M, false, true, sub, pane.width() - hw - 12.0 * s);
            paint::rounded(scene, pane, t::CORNER_RADIUS_500 as f64 * s, GRID_PIT);
            match &pv { Some(p) if p.note.is_none() && p.size.0 > 0 => {
                    if self.picture.as_ref().is_none_or(|(st, _)| *st != p.stamp) { self.picture = Some((p.stamp, paint::image_rgba8(p.size.0, p.size.1, p.rgba.as_ref().clone()))); }
                    // fitted, its shape kept, four pixels clear of the pane's edge
                    let inner = pane.inset(-4.0 * s); let k = (inner.width() / p.size.0 as f64).min(inner.height() / p.size.1 as f64); let (dw, dh) = (p.size.0 as f64 * k, p.size.1 as f64 * k);
                    if let Some((_, brush)) = &self.picture { paint::image(scene, brush, Rect::new(inner.center().x - dw / 2.0, inner.center().y - dh / 2.0, inner.center().x + dw / 2.0, inner.center().y + dh / 2.0)); } }
                Some(p) => { let note = p.note.unwrap_or(""); text(scene, note, pane.center().x - note.len() as f64 * 3.4 * s, pane.center().y, t::DETAIL_SIZE_M, false, false, sub, pane.width() - 16.0 * s); }
                None => {} }
            // the foot: the pin's tag, the file's name, "Latest" while pinned
            let fy = pane.y1 + 16.0 * s; let mut fx = pad;
            if let Some(((a, b), _)) = self.pin { fx += text(scene, &if a == b { format!("\u{25c9} f{a}") } else { format!("\u{25c9} {a}-{b}") }, fx, fy, t::DETAIL_SIZE_M, true, false, paint::color(t::NOTICE_VISUAL_COLOR), 120.0 * s) + 8.0 * s; }
            let lw = if self.pin.is_some() { 64.0 * s } else { 0.0 };
            text(scene, pv.as_ref().map(|p| p.file.as_str()).filter(|f| !f.is_empty()).unwrap_or("Preview"), fx, fy, t::DETAIL_SIZE_M, false, true, sub, (pane.x1 - fx - lw - 8.0 * s).max(0.0));
            if self.pin.is_some() { latest_rect = Rect::new(pane.x1 - lw, fy - 12.0 * s, pane.x1, fy + 12.0 * s); }
        }
        // -- the job list
        let jx = left + pad / 2.0; let n = view.jobs.len();
        let tw = text(scene, "Jobs", jx, top + STRIP * s / 2.0 - 6.0 * s, t::BODY_SIZE_S, true, false, ink, 80.0 * s); let cw = text(scene, &format!("\u{b7} {n}"), jx + tw + 6.0 * s, top + STRIP * s / 2.0 - 6.0 * s, t::DETAIL_SIZE_M, false, false, sub, 120.0 * s);
        let mut sx = jx + tw + cw + 18.0 * s; let cy0 = top + STRIP * s / 2.0 - 6.0 * s;
        if !self.sel.is_empty() { sx += text(scene, &format!("\u{b7} {} selected", self.sel.len()), sx - 6.0 * s, cy0, t::DETAIL_SIZE_M, false, false, ink, 140.0 * s) + 6.0 * s; }
        if !view.leader_answers { sx += text(scene, "leader unreachable", sx, cy0, t::DETAIL_SIZE_M, false, false, paint::color(t::NOTICE_VISUAL_COLOR), 200.0 * s) + 14.0 * s; }
        strip_rects.clear(); let bh = 24.0 * s;
        let delete_w = strip_labels.last().map(|x| x.0).unwrap_or(0.0);
        for (i, (bw, what)) in strip_labels.iter().enumerate() { let bw = *bw; let on = view.jobs.iter().any(|j| sel.contains(&j.id) && match *what { "pause" => j.state == "active", "resume" => j.state == "paused", _ => true });
            let r = if i == 6 { Rect::new(w - pad - bw, cy0 - bh / 2.0, w - pad, cy0 + bh / 2.0) } else if i == 5 { let x = w - pad - delete_w - 6.0 * s - bw; Rect::new(x, cy0 - bh / 2.0, x + bw, cy0 + bh / 2.0) } else { let r = Rect::new(sx, cy0 - bh / 2.0, sx + bw, cy0 + bh / 2.0); sx += bw + 6.0 * s; r }; strip_rects.push((r, on)); }
        let list = Rect::new(jx, top + STRIP * s - 8.0 * s, w - pad, (top + (h - top) * self.layout.list).max(top + 120.0 * s).min(h - pad - 200.0 * s).max(top + 80.0 * s));
        self.gaps[0] = Rect::new(left - pad / 2.0, top, left + pad / 2.0, h - pad); self.gaps[1] = Rect::new(jx, list.y1, w - pad, list.y1 + pad); self.gaps[2] = Rect::ZERO; paint::rounded(scene, list, t::CORNER_RADIUS_500 as f64 * s, t::BACKGROUND_LAYER_2_COLOR);
        let cols: Vec<(f64, f64)> = { let mut x = list.x0 + 8.0 * s; COLUMNS.iter().map(|(_, cw)| { let r = (x, cw * s); x += cw * s + 8.0 * s; r }).collect() };
        for ((label, _), (cx, cwid)) in COLUMNS.iter().zip(&cols) { text(scene, label, *cx, list.y0 + HEADER * s / 2.0, t::DETAIL_SIZE_M, false, false, sub, *cwid); }
        paint::hline(scene, list.x0, list.x1, list.y0 + HEADER * s, t::DIVIDER_THICKNESS as f64 * s, t::DIVIDER_COLOR_MEDIUM);
        let rows = Rect::new(list.x0, list.y0 + HEADER * s + 1.0, list.x1, list.y1); self.jobs_rect = rows; self.rows_from = rows.y0; self.jobs_bar.set(rows, n as f64 * ROW * s, s);
        scene.push_clip_layer(peniko::Fill::NonZero, Affine::IDENTITY, &rows);
        // only the rows in view are laid out and drawn: ten thousand jobs cost what thirty do
        let first = (self.jobs_bar.offset / (ROW * s)) as usize;
        for (i, j) in view.jobs.iter().enumerate().skip(first).take((rows.height() / (ROW * s)) as usize + 2) {
            let y = rows.y0 + i as f64 * ROW * s - self.jobs_bar.offset; let cy = y + ROW * s / 2.0;
            if sel.contains(&j.id) { paint::rect(scene, Rect::new(rows.x0, y, rows.x1, y + ROW * s), t::SELECTION_BACKGROUND_COLOR); }
            if !sel.is_empty() && self.cursor.as_deref() == Some(j.id.as_str()) { paint::rect(scene, Rect::new(rows.x0, y, rows.x0 + 3.0 * s, y + ROW * s), t::ACCENT_VISUAL_COLOR); }
            text(scene, &j.priority.to_string(), cols[0].0, cy, t::DETAIL_SIZE_M, false, true, sub, cols[0].1);
            text(scene, &j.id, cols[1].0, cy, t::BODY_SIZE_S, false, true, ink, cols[1].1);
            let (word, color) = state_of(j); text(scene, word, cols[2].0, cy, t::DETAIL_SIZE_M, true, false, paint::color(color), cols[2].1);
            let bar = Rect::new(cols[3].0, cy - 5.0 * s, cols[3].0 + cols[3].1 - 64.0 * s, cy + 5.0 * s); paint::rounded(scene, bar, 3.0 * s, t::BACKGROUND_BASE_COLOR);
            if j.total > 0 && j.done > 0 { paint::rounded(scene, Rect::new(bar.x0, bar.y0, bar.x0 + bar.width() * (j.done as f64 / j.total as f64).min(1.0), bar.y1), 3.0 * s, t::ACCENT_VISUAL_COLOR); }
            text(scene, &format!("{}/{}", j.done, j.total), bar.x1 + 8.0 * s, cy, t::DETAIL_SIZE_S, false, false, sub, 56.0 * s);
            let d = if j.first_ms <= 0 { "\u{2014}".to_string() } else if j.state == "active" { duration((now_ms - j.first_ms).max(0)) } else if j.last_ms > j.first_ms { duration(j.last_ms - j.first_ms) } else { "\u{2014}".to_string() };
            text(scene, &d, cols[4].0, cy, t::DETAIL_SIZE_M, false, true, sub, cols[4].1);
            text(scene, &if j.created_ms <= 0 { "\u{2014}".to_string() } else { crate::share::local_stamp(j.created_ms) }, cols[5].0, cy, t::DETAIL_SIZE_M, false, false, sub, cols[5].1);
        }
        scene.pop_layer(); self.jobs_bar.paint(scene, now);
        // -- the job's detail: what the cursor's job is made of
        self.node.show_detail(self.cursor.clone()); let detail = self.node.detail(); if self.cursor.is_none() { self.csel.clear(); }
        let area = Rect::new(jx, list.y1 + pad, w - pad, h - pad); let tab_y = area.y0 + 10.0 * s;
        let tw = text(scene, "Job Detail", area.x0, tab_y, t::BODY_SIZE_S, true, false, ink, 120.0 * s); paint::rect(scene, Rect::new(area.x0, tab_y + 11.0 * s, area.x0 + tw, tab_y + 13.0 * s), t::ACCENT_VISUAL_COLOR); text(scene, "Logs", area.x0 + tw + 18.0 * s, tab_y, t::BODY_SIZE_S, false, false, sub, 80.0 * s);
        let body = Rect::new(area.x0, area.y0 + 28.0 * s, area.x1, area.y1);
        self.grid_rect = Rect::ZERO; self.chunks_rect = Rect::ZERO;
        match (&self.cursor, &detail) {
            (None, _) => { text(scene, "No job selected", body.x0 + 12.0 * s, body.y0 + 24.0 * s, t::BODY_SIZE_S, false, false, sub, 240.0 * s); }
            (Some(_), None) => {}
            (Some(_), Some(d)) if body.height() > 160.0 * s => {
                // the header: the job, its state, how far it is, what is under way
                let head = Rect::new(body.x0, body.y0, body.x1, body.y0 + 86.0 * s); paint::rounded(scene, head, t::CORNER_RADIUS_500 as f64 * s, t::BACKGROUND_LAYER_2_COLOR);
                let (total, done, running, failed) = d.chunks.iter().fold((0, 0, 0, 0), |(t0, dn, rn, fl), c| (t0 + (c.state != "stopped") as i64, dn + (c.state == "completed") as i64, rn + (c.state == "assigned") as i64, fl + (c.state == "failed") as i64));
                let idw = text(scene, &d.id, head.x0 + 12.0 * s, head.y0 + 20.0 * s, t::BODY_SIZE_M, true, true, ink, head.width() - 160.0 * s);
                let fake = JobRow { id: String::new(), state: d.state.clone(), priority: 0, total, done, stopped: 0, first_ms: 0, last_ms: 0, created_ms: 0, output_dir: String::new(), output_stem: String::new() }; let (word, color) = state_of(&fake); let word = word.to_uppercase();
                let pill = Rect::new(head.x0 + 12.0 * s + idw + 10.0 * s, head.y0 + 11.0 * s, head.x0 + 12.0 * s + idw + 10.0 * s + (word.len() as f64 * 7.5 + 14.0) * s, head.y0 + 29.0 * s); paint::rounded(scene, pill, 3.0 * s, color); text(scene, &word, pill.x0 + 7.0 * s, pill.center().y, t::DETAIL_SIZE_S, true, false, paint::color(t::BACKGROUND_BASE_COLOR), pill.width());
                let bar = Rect::new(head.x0 + 12.0 * s, head.y0 + 40.0 * s, head.x1 - 110.0 * s, head.y0 + 50.0 * s); paint::rounded(scene, bar, 3.0 * s, t::BACKGROUND_BASE_COLOR); if total > 0 && done > 0 { paint::rounded(scene, Rect::new(bar.x0, bar.y0, bar.x0 + bar.width() * (done as f64 / total as f64).min(1.0), bar.y1), 3.0 * s, t::ACCENT_VISUAL_COLOR); }
                text(scene, &format!("{done} / {total}"), bar.x1 + 10.0 * s, bar.center().y, t::DETAIL_SIZE_M, false, true, ink, 96.0 * s);
                let mut lx = head.x0 + 12.0 * s; let ly = head.y0 + 68.0 * s;
                if running > 0 { lx += text(scene, &format!("\u{25cf} {running} rendering"), lx, ly, t::DETAIL_SIZE_M, false, false, paint::color(t::INFORMATIVE_VISUAL_COLOR), 160.0 * s) + 14.0 * s; }
                if failed > 0 { lx += text(scene, &format!("\u{25cf} {failed} failed"), lx, ly, t::DETAIL_SIZE_M, false, false, paint::color(t::NEGATIVE_VISUAL_COLOR), 140.0 * s) + 14.0 * s; }
                text(scene, &format!("priority {}", d.priority), lx, ly, t::DETAIL_SIZE_M, false, false, sub, 120.0 * s);
                if d.created_ms > 0 { text(scene, &crate::share::local_stamp(d.created_ms), head.x1 - 130.0 * s, ly, t::DETAIL_SIZE_M, false, false, sub, 120.0 * s); }
                // the frame grid: one cell a frame, in reading order; only the rows in view are drawn, whatever the job's length
                let rest = Rect::new(body.x0, head.y1 + pad / 2.0, body.x1, body.y1); let grid_h = (self.layout.grid * s).min(rest.height() - 120.0 * s).max(60.0 * s);
                let grid = Rect::new(rest.x0, rest.y0, rest.x1, rest.y0 + grid_h); paint::rounded(scene, grid, t::CORNER_RADIUS_500 as f64 * s, GRID_PIT); let inner = grid.inset(-4.0 * s); self.grid_rect = inner;
                let slot = CELL * s; let per_row = ((inner.width() - 10.0 * s) / slot).floor().max(1.0) as i64; self.grid_geom = (per_row, slot, d.first); let frames = (d.last - d.first + 1).max(0); let grid_rows = (frames + per_row - 1) / per_row;
                self.grid_bar.set(inner, grid_rows as f64 * slot, s);
                scene.push_clip_layer(peniko::Fill::NonZero, Affine::IDENTITY, &inner);
                let first_row = (self.grid_bar.offset / slot) as i64; let mut ci = 0usize;
                for row in first_row..(first_row + (inner.height() / slot) as i64 + 2).min(grid_rows) { for col in 0..per_row { let f = d.first + row * per_row + col; if f > d.last { break }
                    // the chunk this frame is in: the chunks are in the order of their frames, and so are the cells
                    while ci < d.chunks.len() && d.chunks[ci].b < f { ci += 1; } if ci >= d.chunks.len() || d.chunks[ci].a > f { if ci > 0 && d.chunks[ci.min(d.chunks.len() - 1)].a > f { ci = d.chunks.partition_point(|c| c.b < f); } }
                    let c = d.chunks.get(ci).filter(|c| c.a <= f && f <= c.b);
                    let color = match c { None => Some(GRID_FREE), Some(c) => match c.state.as_str() { "stopped" => None, "completed" => Some(t::POSITIVE_VISUAL_COLOR), st => if c.done.binary_search(&f).is_ok() { Some(GRID_RENDERED) } else { match st { "assigned" => Some(t::ACCENT_VISUAL_COLOR), "failed" => Some(t::NEGATIVE_VISUAL_COLOR), _ => Some(GRID_FREE) } } } };
                    let (x, y) = (inner.x0 + col as f64 * slot, inner.y0 + row as f64 * slot - self.grid_bar.offset);
                    if let Some(color) = color { paint::rect(scene, Rect::new(x + s, y + s, x + slot - s, y + slot - s), color); }
                    if self.pin.is_some_and(|((a, b), _)| a == b && a == f) { scene.stroke(&kurbo::Stroke::new(s), Affine::IDENTITY, paint::color(t::NEUTRAL_CONTENT_COLOR_DEFAULT), None, &Rect::new(x, y, x + slot, y + slot)); } } }
                scene.pop_layer(); self.grid_bar.paint(scene, now);
                // the chunk table
                self.gaps[2] = Rect::new(rest.x0, grid.y1, rest.x1, grid.y1 + pad / 2.0);
                let table = Rect::new(rest.x0, grid.y1 + pad / 2.0, rest.x1, rest.y1); paint::rounded(scene, table, t::CORNER_RADIUS_500 as f64 * s, t::BACKGROUND_LAYER_2_COLOR);
                let ccols: Vec<(f64, f64)> = { let mut x = table.x0 + 8.0 * s; CHUNK_COLUMNS.iter().map(|(_, cw)| { let r = (x, cw * s); x += cw * s + 8.0 * s; r }).collect() };
                for ((label, _), (cx, cwid)) in CHUNK_COLUMNS.iter().zip(&ccols) { text(scene, label, *cx, table.y0 + HEADER * s / 2.0, t::DETAIL_SIZE_M, false, false, sub, *cwid); }
                paint::hline(scene, table.x0, table.x1, table.y0 + HEADER * s, t::DIVIDER_THICKNESS as f64 * s, t::DIVIDER_COLOR_MEDIUM);
                let crows = Rect::new(table.x0, table.y0 + HEADER * s + 1.0, table.x1, table.y1); self.chunks_rect = crows; self.chunks_bar.set(crows, d.chunks.len() as f64 * CHUNK_ROW * s, s);
                let host = |id: &str| -> String { if id.is_empty() { return "\u{2014}".into() } if view.me["node_id"].as_str() == Some(id) { return view.me["hostname"].as_str().unwrap_or(id).to_string() } view.peers.iter().find(|p| p.id == id).map(|p| if p.host.is_empty() { p.id.clone() } else { p.host.clone() }).unwrap_or_else(|| id.to_string()) };
                scene.push_clip_layer(peniko::Fill::NonZero, Affine::IDENTITY, &crows);
                let first = (self.chunks_bar.offset / (CHUNK_ROW * s)) as usize;
                for (i, c) in d.chunks.iter().enumerate().skip(first).take((crows.height() / (CHUNK_ROW * s)) as usize + 2) {
                    let y = crows.y0 + i as f64 * CHUNK_ROW * s - self.chunks_bar.offset; let cy = y + CHUNK_ROW * s / 2.0;
                    if self.csel.contains(&c.id) { paint::rect(scene, Rect::new(crows.x0, y, crows.x1, y + CHUNK_ROW * s), t::SELECTION_BACKGROUND_COLOR); }
                    text(scene, &c.state, ccols[0].0, cy, t::DETAIL_SIZE_M, true, false, paint::color(chunk_color(&c.state)), ccols[0].1);
                    text(scene, &if c.a == c.b { c.a.to_string() } else { format!("{}-{}", c.a, c.b) }, ccols[1].0, cy, t::DETAIL_SIZE_M, false, true, ink, ccols[1].1);
                    text(scene, &host(&c.node), ccols[2].0, cy, t::DETAIL_SIZE_M, false, true, if c.node.is_empty() { sub } else { ink }, ccols[2].1);
                    let n_frames = (c.b - c.a + 1).max(1); let frac = if c.state == "completed" { 1.0 } else { c.done_count as f64 / n_frames as f64 };
                    let pb = Rect::new(ccols[3].0, cy - 4.0 * s, ccols[3].0 + ccols[3].1 - 6.0 * s, cy + 4.0 * s); paint::rounded(scene, pb, 3.0 * s, t::BACKGROUND_BASE_COLOR); if frac > 0.0 { paint::rounded(scene, Rect::new(pb.x0, pb.y0, pb.x0 + pb.width() * frac.min(1.0), pb.y1), 3.0 * s, t::ACCENT_VISUAL_COLOR); }
                    text(scene, &if c.assigned_ms > 0 && c.completed_ms > c.assigned_ms { duration(c.completed_ms - c.assigned_ms) } else { "\u{2014}".to_string() }, ccols[4].0, cy, t::DETAIL_SIZE_M, false, true, sub, ccols[4].1);
                    text(scene, &c.retries.to_string(), ccols[5].0, cy, t::DETAIL_SIZE_M, false, true, if c.retries > 0 { paint::color(t::NOTICE_VISUAL_COLOR) } else { sub }, ccols[5].1);
                }
                scene.pop_layer(); self.chunks_bar.paint(scene, now);
            }
            _ => {}
        }
        drop(text);
        // the gap under the pointer or in hand shows as a line of the accent
        if let Some(i) = self.drag.or(self.over) { let g = self.gaps[i]; if i == 0 { paint::rect(scene, Rect::new(g.center().x - s, g.y0, g.center().x + s, g.y1), t::ACCENT_VISUAL_COLOR); } else { paint::rect(scene, Rect::new(g.x0, g.center().y - s, g.x1, g.center().y + s), t::ACCENT_VISUAL_COLOR); } }
        // the strip's buttons, then what lies over everything: the menu, the question
        for ((b, _), (r, on)) in self.strip.iter_mut().zip(&strip_rects) { b.rect = *r; b.enabled = *on; b.paint(scene, &mut self.fonts, s, now); }
        self.node_btn.label = if stopped { "Resume".into() } else { "Stop".into() }; self.node_btn.variant = if stopped { Variant::Primary } else { Variant::Secondary }; self.node_btn.rect = node_btn_rect; self.node_btn.paint(scene, &mut self.fonts, s, now);
        if self.pin.is_some() && latest_rect.width() > 0.0 { self.latest.rect = latest_rect; self.latest.paint(scene, &mut self.fonts, s, now); } else { self.latest.rect = Rect::ZERO; }
        // New Job, always in reach at the title row's right
        { let (bw, _) = self.new_job.measure(&mut self.fonts, s); self.new_job.rect = Rect::new(w - pad - bw, TITLE * s / 2.0 - 12.0 * s, w - pad, TITLE * s / 2.0 + 12.0 * s); self.new_job.paint(scene, &mut self.fonts, s, now); }
        self.menu.paint(scene, &mut self.fonts, s);
        if let Some((d, _)) = self.confirm.as_mut() { let view_rect = Rect::new(0.0, 0.0, w, h); d.layout(&mut self.fonts, view_rect, s); d.paint(scene, &mut self.fonts, s, view_rect, now); }
        // the New Job card, over everything; a submission that went through closes it and the new job is the one in hand
        if let Some(f) = self.form.as_mut() { if let Some(id) = f.answered() { self.form = None; self.sel = BTreeSet::from([id.clone()]); self.cursor = Some(id); } }
        if let Some(f) = self.form.as_mut() { f.paint(scene, &mut self.fonts, Rect::new(0.0, 0.0, w, h), s, now); }
        self.layer.render(&self.presenter.engine.device, &self.presenter.engine.queue);
        let window = self.window.clone(); self.presenter.draw(|| window.pre_present_notify());
        // (scripted looks: MR_SHOT=<file.png> saves the window as drawn, once, MR_SHOT_AFTER seconds - 8 - after it was made)
        if !self.shot && self.born.elapsed().as_secs_f64() > std::env::var("MR_SHOT_AFTER").ok().and_then(|v| v.parse().ok()).unwrap_or(8.0) { self.shot = true; if let Some(path) = std::env::var_os("MR_SHOT") { if let Some((px, w, h)) = self.presenter.capture_window() {
            let oetf = |v: f32| { let v = v.clamp(0.0, 1.0); ((if v <= 0.003_130_8 { 12.92 * v } else { 1.055 * v.powf(1.0 / 2.4) - 0.055 }) * 255.0 + 0.5) as u8 };
            let bytes: Vec<u8> = (0..(w * h) as usize).flat_map(|i| [oetf(px[i * 4]), oetf(px[i * 4 + 1]), oetf(px[i * 4 + 2])]).collect();
            if let Ok(f) = std::fs::File::create(&path) { let mut e = png::Encoder::new(std::io::BufWriter::new(f), w, h); e.set_color(png::ColorType::Rgb); e.set_depth(png::BitDepth::Eight); let _ = e.write_header().and_then(|mut wr| wr.write_image_data(&bytes)); } } } }
    }
    pub fn animating(&self) -> bool { let now = Instant::now(); self.jobs_bar.animating(now) || self.peers_bar.animating(now) || self.grid_bar.animating(now) || self.chunks_bar.animating(now) || self.strip.iter().any(|(b, _)| b.animating(now)) || self.confirm.as_ref().is_some_and(|(d, _)| d.animating(now)) || self.form.as_ref().is_some_and(|f| f.animating(now)) }
}
