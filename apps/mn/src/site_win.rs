//! The window of "Export as Web Folder" (canon row 339): what is to go - this note, the whole collection, or notes
//! ticked in the collection's tree - then the pages as they are written, then how it ended. The writing itself is
//! `export::site`, off this thread; this is its face. Made of min-ui's parts, as the Collect Media window is: the
//! menu's rows (a tick for the choice), checkboxes, a switch, text buttons, the progress bar, the scroll bar.
use crate::export::site::{Progress, DONE, FAILED, KEPT, SKIPPED, WORKING};
use kurbo::{Affine, Rect, RoundedRect};
use min_theme::tokens as t;
use min_ui::{checkbox, fonts::draw_layout, menu, paint, progress, Button, Event, Family, Fonts, ScrollBar, Switch, TextButton, Variant};
use mn_doc::BlockId;
use mn_store::DocKind;
use std::{path::PathBuf, sync::{atomic::Ordering::Relaxed, Arc}, time::{Duration, Instant}};
use vello::Scene;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Phase { Options, Running, Done }
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Out { None, Redraw, Close, /// the folder is to be asked for, and the export started
    Choose, Reveal }
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Scope { This, All, Chosen }
pub struct Entry { pub id: BlockId, pub parent: Option<BlockId>, pub depth: usize, pub kind: DocKind, pub title: String, pub on: bool }

pub struct Window {
    pub shown: bool, pub phase: Phase, pub scope: Scope, pub entries: Vec<Entry>,
    /// the note or canvas that is open (None: "This note" cannot be chosen)
    pub current: Option<BlockId>,
    pub files: Switch, /// videos get a playable copy (row 340)
    pub videos: Switch, pub dest: Option<PathBuf>, pub prog: Option<Arc<Progress>>,
    /// the pages being written: (their place in the job's pages, title, a canvas)
    pub pages: Vec<(usize, String, bool)>, pub note: String, pub failed: bool,
    bar: ScrollBar, b0: TextButton, b1: TextButton, scopes: [Rect; 3], rows: Vec<(Rect, usize)>, card: Rect, list: Rect, hover: Option<usize>, hover_scope: Option<usize>, scale: f64, drawn: Instant, started: Option<Instant>,
}
fn media(n: u64) -> String { if n == 1 { "1 picture or file".into() } else { format!("{n} pictures and files") } }
fn home(p: &std::path::Path) -> String { let s = p.to_string_lossy().into_owned(); match std::env::var("HOME") { Ok(h) if !h.is_empty() && s.starts_with(&h) => format!("~{}", &s[h.len()..]), _ => s } }
fn title_of(s: &str) -> &str { if s.trim().is_empty() { "Untitled" } else { s } }

impl Window {
    pub fn open(entries: Vec<Entry>, current: Option<BlockId>, files: bool, videos: bool) -> Window {
        Window { shown: true, phase: Phase::Options, scope: if current.is_some() { Scope::This } else { Scope::All }, entries, current, files: Switch::new(files), videos: Switch::new(videos), dest: None, prog: None, pages: vec![], note: String::new(), failed: false,
            bar: ScrollBar::default(), b0: TextButton::new("Cancel", Variant::Secondary), b1: TextButton::new("Export\u{2026}", Variant::Primary), scopes: [Rect::ZERO; 3], rows: vec![], card: Rect::ZERO, list: Rect::ZERO, hover: None, hover_scope: None, scale: 1.0, drawn: Instant::now(), started: None }
    }
    pub fn active(&self) -> bool { self.phase == Phase::Running }
    /// What goes, in the collection's order.
    pub fn chosen(&self) -> Vec<BlockId> {
        self.entries.iter().filter(|e| e.kind != DocKind::Folder && match self.scope { Scope::This => Some(e.id) == self.current, Scope::All => true, Scope::Chosen => e.on }).map(|e| e.id).collect()
    }
    /// The export begins: the window shows its pages.
    pub fn begin(&mut self, dest: PathBuf, prog: Arc<Progress>, pages: Vec<(usize, String, bool)>) { self.dest = Some(dest); self.prog = Some(prog); self.pages = pages; self.phase = Phase::Running; self.bar.offset = 0.0; self.started = Some(Instant::now()); self.note.clear(); self.failed = false; }
    /// What the footer says while the window is put away.
    pub fn readout(&self) -> Option<String> {
        if self.shown { return None }
        match self.phase { Phase::Running => { let (n, of) = self.counts(); let (v, vof, _) = self.video_counts(); Some(if n == of && vof > 0 { format!("Making the videos \u{2014} {v} of {vof} \u{00b7} click to show") } else { format!("Writing the web folder \u{2014} {n} of {of} pages \u{00b7} click to show") }) } Phase::Done if !self.note.is_empty() => Some(format!("{} \u{00b7} click to show", self.note)), _ => None }
    }
    fn counts(&self) -> (usize, usize) { (self.prog.as_ref().map(|p| self.pages.iter().filter(|x| p.states[x.0].load(Relaxed) == DONE).count()).unwrap_or(0), self.pages.len()) }
    /// The videos: (made or kept, to be made, failed).
    fn video_counts(&self) -> (usize, usize, usize) { let Some(p) = &self.prog else { return (0, 0, 0) }; let v = p.videos.lock().unwrap(); (v.iter().filter(|r| matches!(r.state, DONE | KEPT)).count(), v.iter().filter(|r| r.state != SKIPPED).count(), v.iter().filter(|r| r.state == FAILED).count()) }
    /// Between events: the writing ending.
    pub fn tick(&mut self) -> Out {
        if self.phase != Phase::Running { return Out::None } let Some(p) = self.prog.clone() else { return Out::None };
        if !p.over.load(Relaxed) { let now = Instant::now(); return if now.duration_since(self.drawn) >= Duration::from_millis(100) { self.drawn = now; Out::Redraw } else { Out::None } }
        let (n, of) = self.counts(); let took = self.started.take().map(|t| t.elapsed().as_secs_f64()).unwrap_or(0.0);
        self.phase = Phase::Done; self.shown = true;
        match p.error.lock().unwrap().clone() {
            Some(e) if e == "Stopped" => { self.failed = true; self.note = format!("Stopped \u{2014} {n} of {of} pages were written; the folder is not complete"); }
            Some(e) => { self.failed = true; self.note = e; }
            None => { let (v, vof, bad) = self.video_counts(); let vids = if vof == 0 { String::new() } else if bad > 0 { format!(" \u{00b7} {v} of {vof} videos ({bad} failed)") } else { format!(" \u{00b7} {v} video{}", if v == 1 { "" } else { "s" }) };
                self.note = format!("{of} page{} written{vids} \u{00b7} {} \u{00b7} {} \u{00b7} {}", if of == 1 { "" } else { "s" }, media(p.media.load(Relaxed)), crate::gather::bytes(p.bytes.load(Relaxed)), crate::gather::span(took)); }
        }
        Out::Redraw
    }
    fn toggle(&mut self, i: usize) {
        let on = !self.entries[i].on; let depth = self.entries[i].depth; self.entries[i].on = on;
        // what is under it goes with it
        for e in self.entries[i + 1..].iter_mut() { if e.depth <= depth { break } e.on = on; }
    }
    /// A folder's box: all, none or some of what is under it.
    fn state(&self, i: usize) -> Option<bool> {
        let e = &self.entries[i]; let under: Vec<&Entry> = self.entries[i + 1..].iter().take_while(|x| x.depth > e.depth).filter(|x| x.kind != DocKind::Folder).collect();
        let goes = |x: &Entry| match self.scope { Scope::This => Some(x.id) == self.current, Scope::All => true, Scope::Chosen => x.on };
        if e.kind != DocKind::Folder { return Some(goes(e)) }
        let n = under.iter().filter(|x| goes(x)).count(); if n == 0 { Some(false) } else if n == under.len() { Some(true) } else { None }
    }
    fn layout(&mut self, fonts: &mut Fonts, view: Rect, s: f64) {
        self.scale = s; let pad = t::DIALOG_PADDING as f64 * s; let bh = t::BUTTON_HEIGHT_M as f64 * s; let rh = t::MENU_ITEM_HEIGHT as f64 * s;
        let (w, h) = ((640.0 * s).min(view.width() - 48.0 * s).max(320.0 * s), (640.0 * s).min(view.height() - 48.0 * s).max(300.0 * s));
        let (x0, y0) = (view.x0 + (view.width() - w) / 2.0, view.y0 + (view.height() - h) / 2.0);
        self.card = Rect::new(x0, y0, x0 + w, y0 + h);
        let options = self.phase == Phase::Options;
        let top = y0 + pad + if options { 66.0 * s + 3.0 * rh + 12.0 * s } else { 104.0 * s };
        if options { for i in 0..3 { let y = y0 + pad + 66.0 * s + i as f64 * rh; self.scopes[i] = Rect::new(x0 + pad, y, x0 + w - pad, y + rh); } } else { self.scopes = [Rect::ZERO; 3]; }
        self.list = Rect::new(x0 + pad, top, x0 + w - pad, y0 + h - pad - bh - 16.0 * s);
        let row = 30.0 * s; let nv = self.prog.as_ref().map(|p| p.videos.lock().unwrap().len()).unwrap_or(0); let n = if options { self.entries.len() } else { self.pages.len() + nv };
        self.bar.set(self.list, n as f64 * row + 8.0 * s, s);
        // while it runs the page being written is kept in sight
        if let (Phase::Running, Some(p)) = (self.phase, &self.prog) { if !self.bar.dragging() { if let Some(i) = self.pages.iter().position(|x| p.states[x.0].load(Relaxed) == WORKING).or_else(|| p.videos.lock().unwrap().iter().position(|r| r.state == WORKING).map(|i| self.pages.len() + i)) { let (a, z) = (i as f64 * row, (i + 2) as f64 * row); if z > self.bar.offset + self.list.height() { self.bar.offset = (z - self.list.height()).min(self.bar.max()); } else if a < self.bar.offset { self.bar.offset = a; } } } }
        let off = self.bar.offset; self.rows = (0..n).map(|i| (Rect::new(self.list.x0, self.list.y0 + 4.0 * s + i as f64 * row - off, self.list.x1, self.list.y0 + 4.0 * s + (i + 1) as f64 * row - off), i)).collect();
        let (l0, l1, v1): (&str, &str, Variant) = match self.phase {
            Phase::Options => ("Cancel", "Export\u{2026}", Variant::Primary),
            Phase::Running => ("Hide", if self.prog.as_ref().is_some_and(|p| p.stop.load(Relaxed)) { "Stopping\u{2026}" } else { "Stop" }, Variant::Secondary),
            Phase::Done => (if self.failed { "" } else if cfg!(target_os = "macos") { "Show in Finder" } else { "Show the Folder" }, "Close", Variant::Primary) };
        if self.b0.label != l0 { self.b0 = TextButton::new(l0, Variant::Secondary); } if self.b1.label != l1 { self.b1 = TextButton::new(l1, v1); }
        self.b1.enabled = match self.phase { Phase::Options => !self.chosen().is_empty(), Phase::Running => !self.prog.as_ref().is_some_and(|p| p.stop.load(Relaxed)), Phase::Done => true };
        let by = y0 + h - pad - bh; let (w1, _) = self.b1.measure(fonts, s); let (w0, _) = self.b0.measure(fonts, s);
        self.b1.rect = Rect::new(x0 + w - pad - w1, by, x0 + w - pad, by + bh); self.b0.rect = Rect::new(self.b1.rect.x0 - 8.0 * s - w0, by, self.b1.rect.x0 - 8.0 * s, by + bh);
        let (sw, sh) = (t::SWITCH_TRACK_WIDTH as f64 * s, t::SWITCH_TRACK_HEIGHT as f64 * s);
        self.files.rect = if options { Rect::new(x0 + pad, by + (bh - sh) / 2.0, x0 + pad + sw, by + (bh + sh) / 2.0) } else { Rect::ZERO };
        self.videos.rect = if options { self.files.rect + kurbo::Vec2::new(sw + 132.0 * s, 0.0) } else { Rect::ZERO };
    }
    fn row_at(&self, x: f64, y: f64) -> Option<usize> { let p = kurbo::Point::new(x, y); if !self.list.contains(p) { return None } self.rows.iter().find(|(r, _)| r.contains(p)).map(|(_, i)| *i) }
    fn press(&mut self, first: bool) -> Out {
        match (self.phase, first) {
            (Phase::Options, true) => Out::Close, (Phase::Options, false) => if self.chosen().is_empty() { Out::None } else { Out::Choose },
            (Phase::Running, true) => { self.shown = false; Out::Redraw }
            (Phase::Running, false) => { if let Some(p) = &self.prog { p.stop.store(true, Relaxed); } Out::Redraw }
            (Phase::Done, true) => Out::Reveal, (Phase::Done, false) => Out::Close,
        }
    }
    pub fn pointer(&mut self, e: &Event) -> Out {
        let now = Instant::now();
        let (_, mine) = self.bar.event(e, now); if mine { return Out::Redraw }
        if !self.b0.label.is_empty() && self.b0.event(e, now).0 { return self.press(true) }
        if self.b1.event(e, now).0 { return self.press(false) }
        if self.phase == Phase::Options { self.files.event(e, now); self.videos.event(e, now); }
        match *e {
            Event::Move { x, y } => { self.hover = self.row_at(x, y); self.hover_scope = self.scopes.iter().position(|r| r.contains(kurbo::Point::new(x, y))); Out::Redraw }
            Event::Down { x, y, button: Button::Primary } if self.phase == Phase::Options => {
                if let Some(i) = self.scopes.iter().position(|r| r.contains(kurbo::Point::new(x, y))) { if i != 0 || self.current.is_some() { self.scope = [Scope::This, Scope::All, Scope::Chosen][i]; } }
                else if let Some(i) = self.row_at(x, y) {
                    // a tick in the tree is a choice of notes: the whole collection's ticks become the start of it
                    if self.scope != Scope::Chosen { let all = self.scope == Scope::All; let cur = self.current; for e in self.entries.iter_mut() { e.on = all || Some(e.id) == cur; } self.scope = Scope::Chosen; }
                    self.toggle(i);
                }
                Out::Redraw }
            Event::Leave => { self.hover = None; self.hover_scope = None; Out::Redraw }
            _ => Out::Redraw,
        }
    }
    pub fn wheel(&mut self, dy: f64) { self.bar.wheel(dy, Instant::now()); }
    /// Escape (`enter` false) or Enter.
    pub fn key(&mut self, enter: bool) -> Out { match (self.phase, enter) { (Phase::Options, true) => self.press(false), (Phase::Options, false) => Out::Close, (Phase::Running, false) => { self.shown = false; Out::Redraw } (Phase::Done, _) => Out::Close, _ => Out::None } }
    pub fn animating(&self, now: Instant) -> bool { self.shown && (self.b0.animating(now) || self.b1.animating(now) || self.files.animating(now) || self.videos.animating(now) || self.bar.animating(now)) }

    pub fn paint(&mut self, scene: &mut Scene, fonts: &mut Fonts, view: Rect, s: f64, now: Instant) {
        if !self.shown { return }
        self.layout(fonts, view, s);
        let c = paint::color; let pad = t::DIALOG_PADDING as f64 * s;
        let (ink, sub, bad) = (c(t::NEUTRAL_CONTENT_COLOR_DEFAULT), c(t::NEUTRAL_SUBDUED_CONTENT_COLOR_DEFAULT), c(t::NEGATIVE_CONTENT_COLOR_DEFAULT));
        paint::rect(scene, view, t::DIALOG_SCRIM_COLOR);
        let rad = t::DIALOG_CORNER_RADIUS as f64 * s;
        for sh in t::POPOVER_SHADOW { scene.draw_blurred_rounded_rect(Affine::translate((sh.x as f64 * s, sh.y as f64 * s)), self.card.inflate(sh.spread as f64 * s, sh.spread as f64 * s), c(sh.color), rad, (sh.blur as f64 * s / 2.0).max(0.1)); }
        scene.fill(peniko::Fill::NonZero, Affine::IDENTITY, c(t::BACKGROUND_LAYER_1_COLOR), None, &RoundedRect::from_rect(self.card, rad));
        let (x0, x1, y0) = (self.card.x0 + pad, self.card.x1 - pad, self.card.y0 + pad); let wide = x1 - x0;
        let line = |scene: &mut Scene, fonts: &mut Fonts, text: &str, x: f64, cy: f64, size: f32, bold: bool, color: peniko::Color, room: f64, right: bool| -> f64 {
            let l = fonts.layout_fit(text, Family::Sans, size * s as f32, if bold { t::FONT_WEIGHT_BOLD } else { t::FONT_WEIGHT_REGULAR } as f32, color, room.max(8.0)); let w = l.width() as f64;
            draw_layout(scene, &l, Affine::translate((if right { x - w } else { x }, cy - l.height() as f64 / 2.0))); w };
        line(scene, fonts, "Export as Web Folder", x0, y0 + 10.0 * s, t::DIALOG_TITLE_SIZE, true, ink, wide, false);
        let about = match (self.phase, &self.dest) {
            (Phase::Options, _) => "Pages a browser opens from a shared drive or a web server: one per note and canvas, with the list of notes and a search beside them, and the pictures as files in the folder.".to_string(),
            (_, Some(d)) => format!("To {}", home(d)), _ => String::new() };
        let l = fonts.layout_width(&about, Family::Sans, t::BODY_SIZE_S * s as f32, t::FONT_WEIGHT_REGULAR as f32, c(t::DIALOG_BODY_COLOR), Some(wide as f32)); draw_layout(scene, &l, Affine::translate((x0, y0 + 26.0 * s)));
        let options = self.phase == Phase::Options;
        if options {
            let n = self.chosen().len(); let labels = ["This note".to_string(), "The whole collection".to_string(), if self.scope == Scope::Chosen { format!("Chosen notes \u{2014} {n} ticked below") } else { "Chosen notes \u{2014} tick them below".to_string() }];
            for i in 0..3 { let on = self.scope == [Scope::This, Scope::All, Scope::Chosen][i]; let able = i != 0 || self.current.is_some();
                menu::row(scene, fonts, self.scopes[i], &labels[i], able, able && self.hover_scope == Some(i), Some(on), true, s); }
        } else {
            let (n, of) = self.counts(); let sy = y0 + 76.0 * s; let p = self.prog.clone();
            let (v, vof, _) = self.video_counts(); let vids = if vof > 0 { format!(" \u{00b7} {v} of {vof} videos") } else { String::new() };
            let text = match self.phase { Phase::Running => format!("{n} of {of} pages{vids} \u{00b7} {} \u{00b7} {}", media(p.as_ref().map(|p| p.media.load(Relaxed)).unwrap_or(0)), crate::gather::bytes(p.as_ref().map(|p| p.bytes.load(Relaxed)).unwrap_or(0))), _ => self.note.clone() };
            line(scene, fonts, &text, x0, sy, t::BODY_SIZE_M, false, if self.failed { bad } else { ink }, wide, false);
            let part: f64 = p.as_ref().map(|p| p.videos.lock().unwrap().iter().filter(|r| r.state != SKIPPED).map(|r| if matches!(r.state, DONE | KEPT | FAILED) { 1.0 } else { r.done as f64 }).sum()).unwrap_or(0.0);
            progress::bar(scene, Rect::new(x0, sy + 14.0 * s, x1, sy + 14.0 * s + progress::height(s)), if of + vof > 0 { (n as f64 + part) / (of + vof) as f64 } else { 1.0 }, if self.phase == Phase::Done && !self.failed { progress::Tone::Positive } else if self.failed { progress::Tone::Negative } else { progress::Tone::Neutral });
        }
        paint::hline(scene, x0, x1, self.list.y0 - 1.0 * s, t::DIVIDER_THICKNESS as f64 * s, t::DIVIDER_COLOR_MEDIUM); paint::hline(scene, x0, x1, self.list.y1, t::DIVIDER_THICKNESS as f64 * s, t::DIVIDER_COLOR_MEDIUM);
        scene.push_clip_layer(peniko::Fill::NonZero, Affine::IDENTITY, &self.list);
        let cb = checkbox::size(s); let icon_of = |k: DocKind| match k { DocKind::Folder => "folder", DocKind::Canvas => "bounding-box", DocKind::Note => "note" };
        for (r, i) in self.rows.clone() {
            if r.y1 < self.list.y0 || r.y0 > self.list.y1 { continue }
            let cy = r.center().y;
            if options {
                let e = &self.entries[i]; let st = self.state(i); let mut x = r.x0 + 8.0 * s + e.depth as f64 * 18.0 * s;
                if self.hover == Some(i) { paint::rounded(scene, r.inset((0.0, -1.0 * s)), t::CORNER_RADIUS_500 as f64 * s, t::HOVER_BACKGROUND_COLOR); }
                checkbox::paint_box(scene, fonts, Rect::new(x, cy - cb / 2.0, x + cb, cy + cb / 2.0), st, true, self.hover == Some(i), s); x += cb + 10.0 * s;
                let dim = st == Some(false); let col = if dim { sub } else { ink };
                if e.kind != DocKind::Folder { if let Some(g) = fonts.icon(icon_of(e.kind), t::ICON_SIZE_SMALL * s as f32, col) { draw_layout(scene, &g, Affine::translate((x, cy - g.height() as f64 / 2.0))); } x += t::ICON_SIZE_SMALL as f64 * s + 8.0 * s; }
                line(scene, fonts, title_of(&e.title), x, cy, t::BODY_SIZE_M, e.kind == DocKind::Folder, col, r.x1 - 16.0 * s - x, false);
            } else if i >= self.pages.len() {
                // a video: its name, what is being done with it, how far it is
                let Some(v) = self.prog.as_ref().and_then(|p| p.videos.lock().unwrap().get(i - self.pages.len()).cloned()) else { continue }; let mut x = r.x0 + 12.0 * s;
                let col = if matches!(v.state, 0 | SKIPPED) { sub } else { ink };
                if let Some(g) = fonts.icon("film-strip", t::ICON_SIZE_SMALL * s as f32, col) { draw_layout(scene, &g, Affine::translate((x, cy - g.height() as f64 / 2.0))); } x += t::ICON_SIZE_SMALL as f64 * s + 8.0 * s;
                let (text, tone) = match v.state { WORKING => (format!("{:.0}%", v.done * 100.0), ink), DONE => ("Made".to_string(), sub), KEPT => ("Kept from before".to_string(), sub), FAILED => (v.why.clone(), bad), SKIPPED => (v.why.clone(), sub), _ => (if self.phase == Phase::Running { "Waiting" } else { "Not made" }.to_string(), sub) };
                let wide_text = matches!(v.state, FAILED | SKIPPED); let col_w = if wide_text { (r.width() * 0.55).max(110.0 * s) } else { 190.0 * s };
                line(scene, fonts, title_of(&v.name), x, cy, t::BODY_SIZE_M, false, col, r.x1 - col_w - 28.0 * s - x, false);
                if v.state == WORKING { line(scene, fonts, &text, r.x1 - 16.0 * s - 120.0 * s - 8.0 * s, cy, t::BODY_SIZE_S, false, tone, 60.0 * s, true); progress::bar(scene, Rect::new(r.x1 - 16.0 * s - 120.0 * s, cy - progress::height(s) / 2.0, r.x1 - 16.0 * s, cy + progress::height(s) / 2.0), v.done as f64, progress::Tone::Neutral); }
                else { line(scene, fonts, &text, r.x1 - 16.0 * s, cy, t::BODY_SIZE_S, false, tone, col_w, true); }
            } else {
                let (at, title, canvas) = &self.pages[i]; let st = self.prog.as_ref().map(|p| p.states[*at].load(Relaxed)).unwrap_or(0); let mut x = r.x0 + 12.0 * s;
                let col = if st == 0 { sub } else { ink };
                if let Some(g) = fonts.icon(if *canvas { "bounding-box" } else { "note" }, t::ICON_SIZE_SMALL * s as f32, col) { draw_layout(scene, &g, Affine::translate((x, cy - g.height() as f64 / 2.0))); } x += t::ICON_SIZE_SMALL as f64 * s + 8.0 * s;
                line(scene, fonts, title_of(title), x, cy, t::BODY_SIZE_M, false, col, r.x1 - 130.0 * s - x, false);
                line(scene, fonts, match st { WORKING => "Writing\u{2026}", DONE => "Written", _ => if self.phase == Phase::Running { "Waiting" } else { "Not written" } }, r.x1 - 16.0 * s, cy, t::BODY_SIZE_S, false, if st == WORKING { ink } else { sub }, 110.0 * s, true);
            }
        }
        scene.pop_layer();
        self.bar.paint(scene, now);
        if options { self.files.paint(scene, s, now); line(scene, fonts, "PDFs and files", self.files.rect.x1 + 10.0 * s, self.files.rect.center().y, t::BODY_SIZE_S, false, sub, 116.0 * s, false);
            self.videos.paint(scene, s, now); line(scene, fonts, "Playable videos", self.videos.rect.x1 + 10.0 * s, self.videos.rect.center().y, t::BODY_SIZE_S, false, sub, (self.b0.rect.x0 - self.videos.rect.x1 - 24.0 * s).max(20.0), false); }
        if !self.b0.label.is_empty() { self.b0.paint(scene, fonts, s, now); } self.b1.paint(scene, fonts, s, now);
    }
}
