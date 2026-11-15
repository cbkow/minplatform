//! The window's content (canon row 405): today's layout, drawn with the family's toolkit in minRender's own blue and
//! its layer-2 surfaces. This first slice shows: the title row, this node's card, the peers, the job list.
//!
//! Built for row 389's scale bar from the start: a list draws only the rows that are in view, whatever its length, and
//! nothing here asks another node or the database for anything - the window is handed a `View` (node.rs).
use crate::node::{JobRow, Node, View};
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
    confirm: Option<(Dialog, &'static str)>, demo: bool, jobs_rect: Rect, peers_rect: Rect, rows_from: f64, born: Instant, shot: bool }

/// The job list's columns: (header, default width).
const COLUMNS: [(&str, f64); 6] = [("Pri", 36.0), ("Name", 300.0), ("State", 90.0), ("Progress", 280.0), ("Duration", 80.0), ("Created", 140.0)];
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
        let b = |l: &str, v: Variant| TextButton::new(l, v).size_s();
        let strip = vec![(b("Pause", Variant::Secondary), "pause"), (b("Resume", Variant::Secondary), "resume"), (b("Retry failed", Variant::Secondary), "retry-failed"), (b("Requeue", Variant::Secondary), "requeue"), (b("Archive", Variant::Secondary), "archive"), (b("Cancel", Variant::Secondary), "cancel"), (b("Delete", Variant::Negative), "delete")];
        Some(Ui { window, node, presenter, layer, fonts: Fonts::new(), jobs_bar: ScrollBar::default(), peers_bar: ScrollBar::default(), pointer: (0.0, 0.0), sel: BTreeSet::new(), cursor: None, anchor: None, strip, menu: Menu::new(), menu_acts: Vec::new(), confirm: None, demo: false, jobs_rect: Rect::ZERO, peers_rect: Rect::ZERO, rows_from: 0.0, born: Instant::now(), shot: false })
    }
    pub fn resize(&mut self, w: u32, h: u32) { if w == 0 || h == 0 { return } self.presenter.resize(w, h); self.presenter.engine.set_ui_scale(self.window.scale_factor() as f32); self.layer.resize(&self.presenter.engine.device, w, h); self.presenter.engine.set_overlay(Some(self.layer.view())); }
    pub fn wheel(&mut self, dy: f64) { let now = Instant::now(); let p = kurbo::Point::new(self.pointer.0, self.pointer.1); if self.peers_rect.contains(p) { self.peers_bar.wheel(dy, now); } else { self.jobs_bar.wheel(dy, now); } }
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
        // what lies over the lists takes the pointer first: the question, then the menu
        if let Some((d, what)) = self.confirm.as_mut() { let what = *what; d.pointer(e, &mut self.fonts); if let (Some(i), _) = d.event(e, now) { self.confirm = None; if i == 1 { self.act(view, what, true); } } return }
        if self.menu.is_open() { let (chosen, _, _) = self.menu.event(e); if let Some(i) = chosen { let what = self.menu_acts.get(i).copied().unwrap_or(""); self.menu.close(); if !what.is_empty() { self.act(view, what, false); } } else if matches!(e, Event::Down { .. }) && !self.menu.is_open() {} return }
        if self.jobs_bar.event(e, now).1 || self.peers_bar.event(e, now).1 { return }
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
    pub fn key(&mut self, name: &str, view: &View, mods: (bool, bool)) {
        if self.confirm.is_some() { if name == "Escape" { self.confirm = None; } return } if self.menu.is_open() { if name == "Escape" { self.menu.close(); } return }
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
        if self.sel.is_empty() && !self.demo && view.jobs.len() > 4 { if let Ok(demo) = std::env::var("MR_DEMO") { self.demo = true; self.sel = view.jobs[1..4].iter().map(|j| j.id.clone()).collect(); self.cursor = Some(view.jobs[2].id.clone());
            // (MR_DEMO=pause | cancel | ...: the command is given as a click on its button would give it)
            if demo != "select" { let what: &'static str = Box::leak(demo.into_boxed_str()); self.act(view, what, what != "cancel" && what != "delete"); self.sel = view.jobs[1..4].iter().map(|j| j.id.clone()).collect(); } } }
        let strip_labels: Vec<(f64, &'static str)> = { let fonts = &mut self.fonts; self.strip.iter().map(|(b, w)| (b.measure(fonts, s).0, *w)).collect() }; let mut strip_rects: Vec<(Rect, bool)> = Vec::new(); let sel = self.sel.clone();
        let scene: &mut Scene = self.layer.scene_mut(); scene.reset(); let fonts = &mut self.fonts;
        // one line of text, left-aligned at x, centred on cy, cut to the room it has
        let mut text = |scene: &mut Scene, txt: &str, x: f64, cy: f64, size: f32, bold: bool, mono: bool, color: peniko::Color, room: f64| -> f64 { if room < 4.0 { return 0.0 }
            let l = fonts.layout_fit(txt, if mono { Family::Mono } else { Family::Sans }, size * s as f32, if bold { t::FONT_WEIGHT_BOLD } else { t::FONT_WEIGHT_REGULAR } as f32, color, room); draw_layout(scene, &l, Affine::translate((x, cy - l.height() as f64 / 2.0))); l.width() as f64 };
        paint::rect(scene, Rect::new(0.0, 0.0, w, h), t::BACKGROUND_BASE_COLOR);
        // -- the title row: this node, whether it leads, whether it takes work
        let me = &view.me; let host = me["hostname"].as_str().unwrap_or(""); let stopped = me["node_state"] == "stopped"; let pad = 12.0 * s;
        let mut x = pad + text(scene, host, pad, TITLE * s / 2.0, t::BODY_SIZE_M, true, false, ink, 320.0 * s) + 10.0 * s;
        if view.leads { let lw = 58.0 * s; paint::rounded(scene, Rect::new(x, TITLE * s / 2.0 - 9.0 * s, x + lw, TITLE * s / 2.0 + 9.0 * s), 4.0 * s, t::ACCENT_BACKGROUND_COLOR_DEFAULT); text(scene, "LEADER", x + 8.0 * s, TITLE * s / 2.0, t::DETAIL_SIZE_S, true, false, paint::color(t::ACCENT_CONTENT_COLOR_ON_FILL), lw); x += lw + 10.0 * s; }
        scene.fill(peniko::Fill::NonZero, Affine::IDENTITY, paint::color(if stopped { t::NEUTRAL_SUBDUED_CONTENT_COLOR_DEFAULT } else { t::POSITIVE_VISUAL_COLOR }), None, &Circle::new((x + 4.0 * s, TITLE * s / 2.0), 4.0 * s));
        paint::hline(scene, 0.0, w, TITLE * s, t::DIVIDER_THICKNESS as f64 * s, t::DIVIDER_COLOR_MEDIUM);
        // -- the node panel
        let left = (w * 0.28).max(260.0 * s).min(w * 0.5); let top = TITLE * s + pad;
        let card = Rect::new(pad, top, left - pad / 2.0, top + 5.0 * 24.0 * s + 16.0 * s); paint::rounded(scene, card, t::CORNER_RADIUS_500 as f64 * s, t::BACKGROUND_LAYER_2_COLOR);
        let ram = me["ram_mb"].as_u64().unwrap_or(0); let ram = if ram == 0 { "unknown".to_string() } else if ram >= 1024 { format!("{:.1} GB", ram as f64 / 1024.0) } else { format!("{ram} MB") };
        let tags = me["tags"].as_array().map(|a| a.iter().filter_map(|x| x.as_str()).collect::<Vec<_>>().join(", ")).unwrap_or_default(); let dash = |v: &str| if v.is_empty() { "\u{2014}".to_string() } else { v.to_string() };
        for (i, (k, v, mono)) in [("Host", host.to_string(), false), ("Node ID", me["node_id"].as_str().unwrap_or("").to_string(), true), ("GPU", dash(me["gpu_name"].as_str().unwrap_or("")), false), ("CPU / RAM", format!("{} cores \u{b7} {ram}", me["cpu_cores"]), false), ("Tags", dash(&tags), false)].iter().enumerate() {
            let cy = card.y0 + 8.0 * s + (i as f64 + 0.5) * 24.0 * s; text(scene, k, card.x0 + 12.0 * s, cy, t::DETAIL_SIZE_M, false, false, sub, 80.0 * s); text(scene, v, card.x0 + 100.0 * s, cy, t::DETAIL_SIZE_M, false, *mono, ink, card.width() - 112.0 * s); }
        let py = card.y1 + pad; let online = view.peers.iter().filter(|p| p.alive).count();
        let hw = text(scene, "Peers", pad, py + 10.0 * s, t::BODY_SIZE_S, true, false, ink, 100.0 * s); text(scene, &format!("\u{b7} {online} online"), pad + hw + 6.0 * s, py + 10.0 * s, t::DETAIL_SIZE_M, false, false, sub, 160.0 * s);
        self.peers_rect = Rect::new(pad, py + 24.0 * s, left - pad / 2.0, h - pad); self.peers_bar.set(self.peers_rect, view.peers.len() as f64 * PEER_ROW * s, s);
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
        let list = Rect::new(jx, top + STRIP * s - 8.0 * s, w - pad, (top + (h - top) / 3.0).max(top + 160.0 * s).min(h - pad)); paint::rounded(scene, list, t::CORNER_RADIUS_500 as f64 * s, t::BACKGROUND_LAYER_2_COLOR);
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
            let bar = Rect::new(cols[3].0, cy - 5.0 * s, cols[3].0 + cols[3].1 - 64.0 * s, cy + 5.0 * s); paint::rounded(scene, bar, 3.0 * s, t::BACKGROUND_LAYER_1_COLOR);
            if j.total > 0 && j.done > 0 { paint::rounded(scene, Rect::new(bar.x0, bar.y0, bar.x0 + bar.width() * (j.done as f64 / j.total as f64).min(1.0), bar.y1), 3.0 * s, t::ACCENT_VISUAL_COLOR); }
            text(scene, &format!("{}/{}", j.done, j.total), bar.x1 + 8.0 * s, cy, t::DETAIL_SIZE_S, false, false, sub, 56.0 * s);
            let d = if j.first_ms <= 0 { "\u{2014}".to_string() } else if j.state == "active" { duration((now_ms - j.first_ms).max(0)) } else if j.last_ms > j.first_ms { duration(j.last_ms - j.first_ms) } else { "\u{2014}".to_string() };
            text(scene, &d, cols[4].0, cy, t::DETAIL_SIZE_M, false, true, sub, cols[4].1);
            text(scene, &if j.created_ms <= 0 { "\u{2014}".to_string() } else { crate::share::local_stamp(j.created_ms) }, cols[5].0, cy, t::DETAIL_SIZE_M, false, false, sub, cols[5].1);
        }
        scene.pop_layer(); self.jobs_bar.paint(scene, now);
        drop(text);
        // the strip's buttons, then what lies over everything: the menu, the question
        for ((b, _), (r, on)) in self.strip.iter_mut().zip(&strip_rects) { b.rect = *r; b.enabled = *on; b.paint(scene, &mut self.fonts, s, now); }
        self.menu.paint(scene, &mut self.fonts, s);
        if let Some((d, _)) = self.confirm.as_mut() { let view_rect = Rect::new(0.0, 0.0, w, h); d.layout(&mut self.fonts, view_rect, s); d.paint(scene, &mut self.fonts, s, view_rect, now); }
        self.layer.render(&self.presenter.engine.device, &self.presenter.engine.queue);
        let window = self.window.clone(); self.presenter.draw(|| window.pre_present_notify());
        // (scripted looks: MR_SHOT=<file.png> saves the window as drawn, once, MR_SHOT_AFTER seconds - 8 - after it was made)
        if !self.shot && self.born.elapsed().as_secs_f64() > std::env::var("MR_SHOT_AFTER").ok().and_then(|v| v.parse().ok()).unwrap_or(8.0) { self.shot = true; if let Some(path) = std::env::var_os("MR_SHOT") { if let Some((px, w, h)) = self.presenter.capture_window() {
            let oetf = |v: f32| { let v = v.clamp(0.0, 1.0); ((if v <= 0.003_130_8 { 12.92 * v } else { 1.055 * v.powf(1.0 / 2.4) - 0.055 }) * 255.0 + 0.5) as u8 };
            let bytes: Vec<u8> = (0..(w * h) as usize).flat_map(|i| [oetf(px[i * 4]), oetf(px[i * 4 + 1]), oetf(px[i * 4 + 2])]).collect();
            if let Ok(f) = std::fs::File::create(&path) { let mut e = png::Encoder::new(std::io::BufWriter::new(f), w, h); e.set_color(png::ColorType::Rgb); e.set_depth(png::BitDepth::Eight); let _ = e.write_header().and_then(|mut wr| wr.write_image_data(&bytes)); } } } }
    }
    pub fn animating(&self) -> bool { let now = Instant::now(); self.jobs_bar.animating(now) || self.peers_bar.animating(now) || self.strip.iter().any(|(b, _)| b.animating(now)) || self.confirm.as_ref().is_some_and(|(d, _)| d.animating(now)) }
}
