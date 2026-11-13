//! The window of Collect Media and of the package (canon row 332; the user 2026-11-13: "a detailed ui with itemized
//! list of assets and progress bars for each to keep the user informed of every step"). A card over the page with two
//! faces: a REVIEW of what would be copied - one row per file under its kind, what cannot be copied said before
//! anything starts, a tick per row and per kind, the chosen size against the room at the destination - and the
//! PROGRESS - each row's state and bar, a bar for the whole with the time left, Retry on a row that failed. It can be
//! put away while the copy runs (the footer then says how far it is and brings it back). All of it is min-ui's parts:
//! the dialog's card, `TextButton`, `Switch`, `ScrollBar`, and two made for it, `checkbox` and `progress`.
use crate::gather::{self, Asset, Kind, Run, What, COPYING, DONE, FAILED, OFF, QUEUED, THERE, VERIFYING};
use kurbo::{Affine, Rect, RoundedRect};
use min_theme::tokens as t;
use min_ui::{checkbox, fonts::draw_layout, paint, progress, Button, Event, Family, Fonts, ScrollBar, Switch, TextButton, Variant};
use std::{collections::HashMap, path::PathBuf, sync::{atomic::Ordering::Relaxed, mpsc, Arc}, time::{Duration, Instant}};
use vello::Scene;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Phase { Scanning, Review, Running, Asking, Done }
/// What the application is to do after an event or a tick.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Out { None, Redraw, /// the window is done with
    Close, /// a file is to be located (its place in `assets`)
    Relink(usize), /// the copying is over (or stopped and kept): the references are to be pointed at the copies
    Commit, /// stopped with nothing kept: nothing is to change
    Discard, /// show the destination in the system's file browser
    Reveal }
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Row { Group(Kind), Asset(usize) }

pub struct Window {
    pub what: What, pub shown: bool, pub phase: Phase,
    /// the folder the files go to (a collection's `.minnotes`)
    pub dest: PathBuf,
    /// the package: its folder, the collection as it was when the package was asked for, the document's name in it
    pub folder: Option<PathBuf>, pub snap: Option<PathBuf>, pub doc_name: String,
    pub assets: Vec<Asset>, pub run: Option<Arc<Run>>,
    /// how it ended, in words
    pub note: String,
    scan: Option<mpsc::Receiver<Vec<Asset>>>, wake: Arc<dyn Fn() + Send + Sync>,
    check: Switch, bar: ScrollBar, b0: TextButton, b1: TextButton, row_btn: HashMap<usize, TextButton>,
    rows: Vec<(Rect, Row)>, card: Rect, list: Rect, hover: Option<Row>, scale: f64, free: Option<u64>,
    /// the list keeps the file being copied in sight (until the list is scrolled by hand)
    follow: bool, started: Option<Instant>, took: Duration, drawn: Instant,
    rates: HashMap<usize, (Instant, u64, f64)>, overall: Option<(Instant, u64, f64)>,
}

fn home(p: &std::path::Path) -> String {
    let s = p.to_string_lossy().into_owned();
    let s = match std::env::var("HOME") { Ok(h) if !h.is_empty() && s.starts_with(&h) => format!("~{}", &s[h.len()..]), _ => s };
    let c: Vec<char> = s.chars().collect(); if c.len() <= 76 { s } else { format!("{}\u{2026}{}", c[..30].iter().collect::<String>(), c[c.len() - 44..].iter().collect::<String>()) }
}
fn files(n: usize) -> String { if n == 1 { "1 file".into() } else { format!("{n} files") } }

impl Window {
    /// Opened on "looking at the files": the plan is made off this thread (a file on a share that does not answer must
    /// not hold the window) and arrives with a wake-up.
    pub fn open(what: What, descriptors: Vec<String>, places: crate::media::Places, dest: PathBuf, wake: Arc<dyn Fn() + Send + Sync>) -> Window {
        let (tx, rx) = mpsc::channel(); let (d, w) = (dest.clone(), wake.clone());
        let _ = std::thread::Builder::new().name("mn-gather-plan".into()).spawn(move || { let _ = tx.send(gather::plan(what, &descriptors, &places, &d)); w(); });
        Window { what, shown: true, phase: Phase::Scanning, dest, folder: None, snap: None, doc_name: String::new(), assets: vec![], run: None, note: String::new(), scan: Some(rx), wake,
            check: Switch::new(false), bar: ScrollBar::default(), b0: TextButton::new("Cancel", Variant::Secondary), b1: TextButton::new("Collect", Variant::Primary), row_btn: HashMap::new(),
            rows: vec![], card: Rect::ZERO, list: Rect::ZERO, hover: None, scale: 1.0, free: None, follow: true, started: None, took: Duration::ZERO, drawn: Instant::now(), rates: HashMap::new(), overall: None }
    }
    /// A copy is under way or waits for an answer: the window cannot be replaced or dropped.
    pub fn active(&self) -> bool { matches!(self.phase, Phase::Running | Phase::Asking) }
    fn state(&self, i: usize) -> u8 { match &self.run { Some(r) => r.state(i), None => if self.assets[i].on { if self.assets[i].there { THERE } else { QUEUED } } else { OFF } } }
    /// (files chosen, bytes that would be copied)
    fn chosen(&self) -> (usize, u64) { self.assets.iter().filter(|a| a.on).fold((0, 0), |(n, b), a| (n + 1, b + if a.there { 0 } else { a.size })) }
    fn fits(&self) -> bool { self.free.is_none_or(|f| self.chosen().1 <= f) }
    fn can_go(&self) -> bool { self.fits() && (self.what == What::Package || self.assets.iter().any(|a| a.on)) }
    /// (files done, files to copy, bytes done, bytes to copy) of the run
    fn totals(&self) -> (usize, usize, u64, u64) {
        let Some(run) = &self.run else { return (0, 0, 0, 0) }; let (mut n, mut of, mut b, mut all) = (0, 0, 0u64, 0u64);
        for (i, a) in self.assets.iter().enumerate() { match run.state(i) { OFF | THERE => {} DONE => { n += 1; of += 1; b += a.size; all += a.size; } COPYING | VERIFYING => { of += 1; b += run.slots[i].done.load(Relaxed).min(a.size); all += a.size; } _ => { of += 1; all += a.size; } } }
        (n, of, b, all)
    }
    /// The files that are at the destination now, each way the collection named them -> their place with it.
    pub fn map(&self) -> HashMap<String, String> { self.run.as_ref().map(|r| gather::map(&self.assets, r)).unwrap_or_default() }
    /// A file was located: its row is looked at again.
    pub fn relinked(&mut self, i: usize, path: PathBuf) {
        let mut taken: std::collections::HashSet<String> = self.assets.iter().enumerate().filter(|(k, a)| *k != i && !a.dest.is_empty()).map(|(_, a)| a.dest.to_lowercase()).collect();
        if let Some(a) = self.assets.get_mut(i) { a.path = Some(path); a.problem = None; gather::settle(a, &self.dest, &mut taken); }
        self.row_btn.remove(&i);
    }
    /// The application did what `Out::Commit` asked: `err` when it could not.
    pub fn committed(&mut self, err: Option<String>) {
        let Some(run) = &self.run else { return }; let (done, there, failed, left) = (run.count(DONE), run.count(THERE), run.count(FAILED), run.count(QUEUED));
        let size: u64 = self.assets.iter().enumerate().filter(|(i, _)| run.state(*i) == DONE).map(|(_, a)| a.size).sum();
        let mut s = match self.what { What::Collect => format!("Collected {} ({})", files(done), gather::bytes(size)), What::Package => format!("Package written with {} ({})", files(done + there), gather::bytes(size)) };
        if self.took.as_secs() >= 1 { s.push_str(&format!(" in {}", gather::span(self.took.as_secs_f64()).trim_start_matches("about "))); }
        if there > 0 && self.what == What::Collect { s.push_str(&format!(" \u{00b7} {there} already there")); }
        let missing = self.assets.iter().filter(|a| a.problem.is_some()).count(); if missing > 0 { s.push_str(&format!(" \u{00b7} {missing} missing, left as {}", if missing == 1 { "it is" } else { "they are" })); }
        if failed > 0 { s.push_str(&format!(" \u{00b7} {failed} failed")); } if left > 0 { s.push_str(&format!(" \u{00b7} {left} not copied")); }
        if run.full.load(Relaxed) { s.push_str(" \u{00b7} the destination is full"); }
        if let Some(e) = err { s = format!("The files were copied, but the references could not be changed: {e}"); }
        self.note = s;
    }
    /// What the footer says while the window is put away (None: nothing to say).
    pub fn readout(&self) -> Option<String> {
        if self.shown { return None }
        let verb = if self.what == What::Collect { "Collecting media" } else { "Writing the package" };
        match self.phase {
            Phase::Running => { let (_, _, b, all) = self.totals(); let pct = if all > 0 { (b as f64 / all as f64 * 100.0) as u32 } else { 100 };
                Some(match self.left() { Some(l) => format!("{verb} \u{2014} {pct}% \u{00b7} {l} left \u{00b7} click to show"), None => format!("{verb} \u{2014} {pct}% \u{00b7} click to show") }) }
            Phase::Asking => Some("The copy was stopped \u{2014} click to choose what to keep".into()),
            Phase::Done if !self.note.is_empty() => Some(format!("{} \u{00b7} click for the list", self.note)),
            _ => None,
        }
    }
    fn left(&self) -> Option<String> { let (_, _, b, all) = self.totals(); let (_, _, rate) = self.overall?; (rate > 1.0).then(|| gather::span((all.saturating_sub(b)) as f64 / rate)) }
    fn begin(&mut self) {
        let run = Run::new(&self.assets, &self.dest, self.check.on, { let w = self.wake.clone(); move || w() });
        run.start(); self.run = Some(run); self.phase = Phase::Running; self.started = Some(Instant::now()); self.overall = None; self.rates.clear(); self.follow = true; self.row_btn.clear(); self.note.clear();
    }
    /// Starts the copy with what is chosen (the scripted check; the window's own button does the same).
    pub fn go(&mut self) -> bool { if self.phase == Phase::Review && self.can_go() { self.begin(); true } else { false } }
    /// Between events: the plan arriving, the run ending.
    pub fn tick(&mut self) -> Out {
        if let Some(rx) = &self.scan { return match rx.try_recv() { Ok(a) => { self.assets = a; self.scan = None; self.phase = Phase::Review; self.free = gather::free_space(&self.dest); Out::Redraw } Err(_) => Out::None } }
        if self.phase != Phase::Running { return Out::None }
        let Some(run) = self.run.clone() else { return Out::None };
        // the rates: the whole run's, and each file's that is being copied
        let now = Instant::now(); let (_, _, b, _) = self.totals();
        match self.overall { None => self.overall = Some((now, b, 0.0)), Some((t0, b0, r)) => { let dt = now.duration_since(t0).as_secs_f64(); if dt >= 1.0 { let new = b.saturating_sub(b0) as f64 / dt; self.overall = Some((now, b, if r > 0.0 { r * 0.6 + new * 0.4 } else { new })); } } }
        for i in 0..self.assets.len() { if run.state(i) == COPYING { let d = run.slots[i].done.load(Relaxed); match self.rates.get(&i).copied() { None => { self.rates.insert(i, (now, d, 0.0)); } Some((t0, d0, r)) => { let dt = now.duration_since(t0).as_secs_f64(); if dt >= 0.7 { let new = d.saturating_sub(d0) as f64 / dt; self.rates.insert(i, (now, d, if r > 0.0 { r * 0.5 + new * 0.5 } else { new })); } } } } }
        // (a frame ten times a second at most while it runs: the threads wake the loop far more often)
        if run.busy() { return if now.duration_since(self.drawn) >= Duration::from_millis(100) { self.drawn = now; Out::Redraw } else { Out::None } }
        self.took += self.started.take().map(|t| t.elapsed()).unwrap_or_default();
        if run.stop.load(Relaxed) {
            if run.made_any() { self.phase = Phase::Asking; self.shown = true; return Out::Redraw }
            self.phase = Phase::Done; self.note = "Stopped \u{2014} nothing was copied and nothing was changed".into(); return Out::Discard
        }
        self.phase = Phase::Done; Out::Commit
    }

    fn layout(&mut self, fonts: &mut Fonts, view: Rect, s: f64) {
        self.scale = s; let pad = t::DIALOG_PADDING as f64 * s; let bh = t::BUTTON_HEIGHT_M as f64 * s;
        let (w, h) = ((780.0 * s).min(view.width() - 48.0 * s).max(320.0 * s), (640.0 * s).min(view.height() - 48.0 * s).max(280.0 * s));
        let (x0, y0) = (view.x0 + (view.width() - w) / 2.0, view.y0 + (view.height() - h) / 2.0);
        self.card = Rect::new(x0, y0, x0 + w, y0 + h);
        self.list = Rect::new(x0 + pad, y0 + pad + 104.0 * s, x0 + w - pad, y0 + h - pad - bh - 16.0 * s);
        // the rows, in the list's own coordinates first
        let (gh, ah) = (32.0 * s, 52.0 * s); let mut at: Vec<(f64, f64, Row)> = vec![]; let mut y = 0.0;
        for k in Kind::ALL { let of: Vec<usize> = (0..self.assets.len()).filter(|i| self.assets[*i].kind == k).collect(); if of.is_empty() { continue }
            at.push((y, gh, Row::Group(k))); y += gh; for i in of { at.push((y, ah, Row::Asset(i))); y += ah; } y += 6.0 * s; }
        self.bar.set(self.list, y, s);
        if self.follow && self.phase == Phase::Running { if let Some(run) = &self.run { if let Some((ry, rh, _)) = at.iter().find(|(_, _, r)| matches!(r, Row::Asset(i) if run.state(*i) == COPYING && self.assets[*i].size >= gather::LARGE)).or_else(|| at.iter().find(|(_, _, r)| matches!(r, Row::Asset(i) if run.state(*i) == COPYING))) {
            let (top, bottom) = (self.bar.offset, self.bar.offset + self.list.height()); if *ry < top + gh { self.bar.offset = (*ry - gh).max(0.0); } else if ry + rh > bottom { self.bar.offset = (ry + rh - self.list.height()).min(self.bar.max()); } } } }
        let off = self.bar.offset; self.rows = at.into_iter().map(|(ry, rh, r)| (Rect::new(self.list.x0, self.list.y0 + ry - off, self.list.x1, self.list.y0 + ry - off + rh), r)).collect();
        // the buttons of this face
        let (l0, l1, v1): (&str, &str, Variant) = match self.phase {
            Phase::Scanning => ("", "Close", Variant::Secondary),
            Phase::Review => ("Cancel", if self.what == What::Collect { "Collect" } else { "Export" }, Variant::Primary),
            Phase::Running => ("Hide", if self.run.as_ref().is_some_and(|r| r.stop.load(Relaxed)) { "Stopping\u{2026}" } else { "Stop" }, Variant::Secondary),
            Phase::Asking => ("Remove Them", "Keep Them", Variant::Primary),
            Phase::Done => (if self.run.as_ref().is_some_and(|r| r.count(DONE) + r.count(THERE) > 0) { "Show in Finder" } else { "" }, "Close", Variant::Primary),
        };
        if self.b0.label != l0 { self.b0 = TextButton::new(l0, Variant::Secondary); } if self.b1.label != l1 { self.b1 = TextButton::new(l1, v1); }
        self.b1.enabled = match self.phase { Phase::Review => self.can_go(), Phase::Running => !self.run.as_ref().is_some_and(|r| r.stop.load(Relaxed)), _ => true };
        let by = self.card.y1 - pad - bh; let (w1, _) = self.b1.measure(fonts, s); self.b1.rect = Rect::new(self.card.x1 - pad - w1, by, self.card.x1 - pad, by + bh);
        if l0.is_empty() { self.b0.rect = Rect::ZERO; } else { let (w0, _) = self.b0.measure(fonts, s); let x1 = self.b1.rect.x0 - t::DIALOG_BUTTON_GAP as f64 * s; self.b0.rect = Rect::new(x1 - w0, by, x1, by + bh); }
        let (sw, sh) = Switch::size(s); self.check.rect = if self.phase == Phase::Review { Rect::new(self.card.x0 + pad, by + (bh - sh) / 2.0, self.card.x0 + pad + sw, by + (bh + sh) / 2.0) } else { Rect::ZERO };
        // a row's own button: Locate… on a file that cannot be copied (before the start), Retry on one that failed
        let want: Vec<(usize, &str)> = self.rows.iter().filter_map(|(_, r)| match r { Row::Asset(i) => match self.phase { Phase::Review if self.assets[*i].problem.is_some() => Some((*i, "Locate\u{2026}")), Phase::Running | Phase::Done if self.state(*i) == FAILED => Some((*i, "Retry")), _ => None }, _ => None }).collect();
        self.row_btn.retain(|i, b| want.iter().any(|(k, l)| k == i && b.label == *l));
        for (i, l) in want { let b = self.row_btn.entry(i).or_insert_with(|| TextButton::new(l, Variant::Secondary).size_s()); let (bw, bhs) = b.measure(fonts, s);
            let r = self.rows.iter().find(|(_, x)| *x == Row::Asset(i)).map(|(r, _)| *r).unwrap_or(Rect::ZERO); let cy = r.y0 + 35.0 * s;
            b.rect = if r.y0 >= self.list.y0 - 1.0 && r.y1 <= self.list.y1 + 1.0 { Rect::new(r.x1 - 16.0 * s - bw, cy - bhs / 2.0, r.x1 - 16.0 * s, cy + bhs / 2.0) } else { Rect::ZERO }; }
    }
    fn row_at(&self, x: f64, y: f64) -> Option<Row> { if !self.list.contains(kurbo::Point::new(x, y)) { return None } self.rows.iter().find(|(r, _)| r.contains(kurbo::Point::new(x, y))).map(|(_, r)| *r) }
    fn toggle(&mut self, r: Row) {
        match r {
            Row::Asset(i) => if let Some(a) = self.assets.get_mut(i) { if a.problem.is_none() { a.on = !a.on; } },
            Row::Group(k) => { let all = self.assets.iter().filter(|a| a.kind == k && a.problem.is_none()).all(|a| a.on); for a in self.assets.iter_mut().filter(|a| a.kind == k && a.problem.is_none()) { a.on = !all; } }
        }
    }
    fn press(&mut self, first: bool) -> Out {
        match (self.phase, first) {
            (Phase::Scanning, false) | (Phase::Review, true) => Out::Close,
            (Phase::Review, false) => { self.begin(); Out::Redraw }
            (Phase::Running, true) => { self.shown = false; Out::Redraw }
            (Phase::Running, false) => { if let Some(r) = &self.run { r.stop.store(true, Relaxed); } Out::Redraw }
            (Phase::Asking, true) => { if let Some(r) = &self.run { r.remove_made(); } self.phase = Phase::Done; self.note = "Stopped \u{2014} the copies were removed and nothing was changed".into(); Out::Discard }
            (Phase::Asking, false) => { self.phase = Phase::Done; Out::Commit }
            (Phase::Done, true) => Out::Reveal,
            (Phase::Done, false) => Out::Close,
            _ => Out::None,
        }
    }
    /// A pointer event (while the window is shown every one is its own).
    pub fn pointer(&mut self, e: &Event) -> Out {
        let now = Instant::now();
        let (_, mine) = self.bar.event(e, now); if mine { self.follow = false; return Out::Redraw }
        if !self.b0.label.is_empty() && self.b0.event(e, now).0 { return self.press(true) }
        if self.b1.event(e, now).0 { return self.press(false) }
        let mut hit = None; for (i, b) in self.row_btn.iter_mut() { if b.event(e, now).0 { hit = Some(*i); } }
        if let Some(i) = hit {
            if self.phase == Phase::Review { return Out::Relink(i) }
            if let Some(run) = self.run.clone() { run.retry(i); if self.phase == Phase::Done { self.phase = Phase::Running; self.started = Some(Instant::now()); self.overall = None; } self.row_btn.remove(&i); } return Out::Redraw
        }
        if self.phase == Phase::Review { self.check.event(e, now); }
        match *e {
            Event::Move { x, y } => { self.hover = self.row_at(x, y); Out::Redraw }
            Event::Down { x, y, button: Button::Primary } if self.phase == Phase::Review => { if self.row_btn.values().any(|b| b.hit(x, y)) { return Out::Redraw } if let Some(r) = self.row_at(x, y) { self.toggle(r); } Out::Redraw }
            Event::Leave => { self.hover = None; Out::Redraw }
            _ => Out::Redraw,
        }
    }
    pub fn wheel(&mut self, dy: f64) { if self.bar.wheel(dy, Instant::now()) { self.follow = false; } }
    /// Escape (`enter` false) or Enter.
    pub fn key(&mut self, enter: bool) -> Out {
        match (self.phase, enter) { (Phase::Review, true) if self.can_go() => self.press(false), (Phase::Done, true) => Out::Close, (Phase::Running, false) => self.press(true), (Phase::Scanning | Phase::Review | Phase::Done, false) => Out::Close, _ => Out::None }
    }
    pub fn animating(&self, now: Instant) -> bool { self.shown && (self.b0.animating(now) || self.b1.animating(now) || self.check.animating(now) || self.bar.animating(now) || self.row_btn.values().any(|b| b.animating(now))) }

    pub fn paint(&mut self, scene: &mut Scene, fonts: &mut Fonts, view: Rect, s: f64, now: Instant) {
        if !self.shown { return }
        self.layout(fonts, view, s);
        let c = paint::color; let pad = t::DIALOG_PADDING as f64 * s;
        let (ink, sub, bad) = (c(t::NEUTRAL_CONTENT_COLOR_DEFAULT), c(t::NEUTRAL_SUBDUED_CONTENT_COLOR_DEFAULT), c(t::NEGATIVE_CONTENT_COLOR_DEFAULT));
        paint::rect(scene, view, t::DIALOG_SCRIM_COLOR);
        let rad = t::DIALOG_CORNER_RADIUS as f64 * s;
        for sh in t::POPOVER_SHADOW { scene.draw_blurred_rounded_rect(Affine::translate((sh.x as f64 * s, sh.y as f64 * s)), self.card.inflate(sh.spread as f64 * s, sh.spread as f64 * s), c(sh.color), rad, (sh.blur as f64 * s / 2.0).max(0.1)); }
        // the card is layer 1; each file is a tile of layer 2 on it (the user 2026-11-13)
        scene.fill(peniko::Fill::NonZero, Affine::IDENTITY, c(t::BACKGROUND_LAYER_1_COLOR), None, &RoundedRect::from_rect(self.card, rad));
        let (x0, x1, y0) = (self.card.x0 + pad, self.card.x1 - pad, self.card.y0 + pad); let wide = x1 - x0;
        // a line of text, its left end at `x` and its middle at `cy`; its width
        let line = |scene: &mut Scene, fonts: &mut Fonts, text: &str, x: f64, cy: f64, size: f32, bold: bool, color: peniko::Color, room: f64, right: bool| -> f64 {
            let l = fonts.layout_fit(text, Family::Sans, size * s as f32, if bold { t::FONT_WEIGHT_BOLD } else { t::FONT_WEIGHT_REGULAR } as f32, color, room.max(8.0)); let w = l.width() as f64;
            draw_layout(scene, &l, Affine::translate((if right { x - w } else { x }, cy - l.height() as f64 / 2.0))); w };
        line(scene, fonts, if self.what == What::Collect { "Collect Media" } else { "Export Package" }, x0, y0 + 10.0 * s, t::DIALOG_TITLE_SIZE, true, ink, wide, false);
        let about = match self.what {
            What::Collect => "Copies the files this collection refers to elsewhere into its own folder, so it stays whole when the originals are moved. The notes then point at the copies.".to_string(),
            What::Package => format!("A copy of the collection with every file it shows, in one folder that opens on any machine: {}", self.folder.as_deref().map(home).unwrap_or_default()) };
        let l = fonts.layout_width(&about, Family::Sans, t::BODY_SIZE_S * s as f32, t::FONT_WEIGHT_REGULAR as f32, c(t::DIALOG_BODY_COLOR), Some(wide as f32)); draw_layout(scene, &l, Affine::translate((x0, y0 + 26.0 * s)));
        // where things stand, and the bar of the whole
        let sy = y0 + 76.0 * s; let (n_on, need) = self.chosen();
        match self.phase {
            Phase::Scanning => { line(scene, fonts, "Looking at the files\u{2026}", x0, sy, t::BODY_SIZE_M, false, ink, wide, false); }
            Phase::Review => {
                if self.assets.is_empty() { line(scene, fonts, if self.what == What::Collect { "All media already lives with this collection." } else { "This collection shows no files: the package is the collection alone." }, x0, sy, t::BODY_SIZE_M, false, ink, wide, false); }
                else { let room = match self.free { Some(f) => format!(" \u{00b7} {} free at the destination", gather::bytes(f)), None => String::new() };
                    let w = line(scene, fonts, &format!("{} of {} chosen \u{00b7} {} to copy{room}", n_on, files(self.assets.len()), gather::bytes(need)), x0, sy, t::BODY_SIZE_M, false, ink, wide, false);
                    if !self.fits() { line(scene, fonts, "\u{2014} not enough room", x0 + w + 8.0 * s, sy, t::BODY_SIZE_M, false, bad, wide - w - 8.0 * s, false); } }
            }
            Phase::Running | Phase::Asking | Phase::Done => {
                let (n, of, b, all) = self.totals(); let frac = if all > 0 { b as f64 / all as f64 } else { 1.0 };
                let text = match self.phase {
                    Phase::Running => { let mut s = format!("{n} of {} \u{00b7} {} of {}", files(of), gather::bytes(b), gather::bytes(all)); if let Some(l) = self.left() { s.push_str(&format!(" \u{00b7} {l} left")); } s }
                    Phase::Asking => format!("Stopped. {} copied \u{2014} keep {} or remove {}?", files(n), if n == 1 { "it" } else { "them" }, if n == 1 { "it" } else { "them" }),
                    _ => self.note.clone() };
                line(scene, fonts, &text, x0, sy, t::BODY_SIZE_M, false, ink, wide, false);
                let failed = self.run.as_ref().is_some_and(|r| r.count(FAILED) > 0);
                progress::bar(scene, Rect::new(x0, sy + 14.0 * s, x1, sy + 14.0 * s + progress::height(s)), frac, if self.phase == Phase::Done && !failed && n == of { progress::Tone::Positive } else { progress::Tone::Neutral });
            }
        }
        // the list
        paint::hline(scene, x0, x1, self.list.y0 - 1.0 * s, t::DIVIDER_THICKNESS as f64 * s, t::DIVIDER_COLOR_MEDIUM); paint::hline(scene, x0, x1, self.list.y1, t::DIVIDER_THICKNESS as f64 * s, t::DIVIDER_COLOR_MEDIUM);
        scene.push_clip_layer(peniko::Fill::NonZero, Affine::IDENTITY, &self.list);
        let review = self.phase == Phase::Review; let cb = checkbox::size(s);
        for (r, row) in self.rows.clone() {
            if r.y1 < self.list.y0 || r.y0 > self.list.y1 { continue }
            let mut x = r.x0 + 8.0 * s; let right = r.x1 - 16.0 * s;
            match row {
                Row::Group(k) => {
                    let of: Vec<&Asset> = self.assets.iter().filter(|a| a.kind == k).collect(); let cy = r.y0 + 18.0 * s;
                    if review { let able: Vec<&&Asset> = of.iter().filter(|a| a.problem.is_none()).collect(); let on = able.iter().filter(|a| a.on).count();
                        checkbox::paint_box(scene, fonts, Rect::new(x, cy - cb / 2.0, x + cb, cy + cb / 2.0), if on == 0 { Some(false) } else if on == able.len() { Some(true) } else { None }, !able.is_empty(), self.hover == Some(row), s); x += cb + 10.0 * s; }
                    let w = line(scene, fonts, k.label(), x, cy, t::BODY_SIZE_S, true, ink, 200.0 * s, false);
                    line(scene, fonts, &format!("{} \u{00b7} {}", files(of.len()), gather::bytes(of.iter().map(|a| a.size).sum())), x + w + 10.0 * s, cy, t::BODY_SIZE_S, false, sub, 300.0 * s, false);
                }
                Row::Asset(i) => {
                    let a = self.assets[i].clone(); let st = self.state(i); let (c1, c2) = (r.y0 + 17.0 * s, r.y0 + 35.0 * s);
                    paint::rounded(scene, r.inset((0.0, -2.0 * s)), t::CORNER_RADIUS_500 as f64 * s, if review && self.hover == Some(row) && a.problem.is_none() { t::HOVER_BACKGROUND_COLOR } else { t::BACKGROUND_LAYER_2_COLOR });
                    x += 4.0 * s;
                    if review { checkbox::paint_box(scene, fonts, Rect::new(x, c1 - cb / 2.0, x + cb, c1 + cb / 2.0), Some(a.on), a.problem.is_none(), self.hover == Some(row), s); x += cb + 10.0 * s; }
                    let dim = !review && st == OFF;
                    if let Some(g) = fonts.icon(a.kind.icon(), t::ICON_SIZE_SMALL * s as f32, if dim { sub } else { ink }) { draw_layout(scene, &g, Affine::translate((x, c1 - g.height() as f64 / 2.0))); } x += t::ICON_SIZE_SMALL as f64 * s + 8.0 * s;
                    let col = 190.0 * s; let room = right - col - 12.0 * s - x;
                    line(scene, fonts, &a.name, x, c1, t::BODY_SIZE_M, false, if dim { sub } else { ink }, room, false);
                    if a.path.is_some() && a.problem.is_none() { line(scene, fonts, &gather::bytes(a.size), right, c1, t::BODY_SIZE_S, false, sub, col, true); }
                    let has_btn = self.row_btn.get(&i).is_some_and(|b| b.rect.width() > 0.0);
                    let room2 = if has_btn { right - x - 90.0 * s } else if review { right - x } else { room };
                    let place = || if a.inside { "In the collection".to_string() } else { a.path.as_deref().map(home).unwrap_or_default() };
                    let (text, color) = if let Some(p) = &a.problem { (p.clone(), bad) } else if review {
                        (if a.there { format!("{} \u{00b7} at the destination already, not copied again", place()) } else if a.uses > 1 { format!("{} \u{00b7} used {} times", place(), a.uses) } else if a.notes.is_some() { format!("{} \u{00b7} with its notes", place()) } else { place() }, sub)
                    } else { match st {
                        QUEUED => (if self.phase == Phase::Running { "Waiting" } else { "Not copied" }.to_string(), sub),
                        COPYING => { let d = self.run.as_ref().map(|r| r.slots[i].done.load(Relaxed)).unwrap_or(0); let rate = self.rates.get(&i).map(|x| x.2).unwrap_or(0.0);
                            (if rate > 1.0 { format!("{} of {} \u{00b7} {}/s", gather::bytes(d), gather::bytes(a.size), gather::bytes(rate as u64)) } else { format!("{} of {}", gather::bytes(d), gather::bytes(a.size)) }, ink) }
                        VERIFYING => ("Checking the copy\u{2026}".to_string(), ink),
                        DONE => ("Copied".to_string(), sub), THERE => ("Already there".to_string(), sub),
                        FAILED => (self.run.as_ref().map(|r| r.slots[i].why.lock().unwrap().clone()).unwrap_or_default(), bad),
                        _ => ("Left where it is".to_string(), sub) } };
                    line(scene, fonts, &text, x, c2, t::BODY_SIZE_S, false, color, room2, false);
                    if !review && !has_btn && !matches!(st, OFF | FAILED) {
                        let frac = match st { DONE | THERE | VERIFYING => 1.0, COPYING => self.run.as_ref().map(|r| r.slots[i].done.load(Relaxed) as f64 / a.size.max(1) as f64).unwrap_or(0.0), _ => 0.0 };
                        progress::bar(scene, Rect::new(right - col, c2 - progress::height(s) / 2.0, right, c2 + progress::height(s) / 2.0), frac, if matches!(st, DONE | THERE) { progress::Tone::Positive } else { progress::Tone::Neutral });
                    }
                }
            }
        }
        for b in self.row_btn.values() { if b.rect.width() > 0.0 { b.paint(scene, fonts, s, now); } }
        scene.pop_layer();
        self.bar.paint(scene, now);
        // the foot: the option before the start, the buttons
        if review { self.check.paint(scene, s, now); let cy = self.check.rect.center().y; line(scene, fonts, "Read every copy back and compare it (slower)", self.check.rect.x1 + 10.0 * s, cy, t::BODY_SIZE_S, false, sub, (self.b0.rect.x0 - self.check.rect.x1 - 24.0 * s).max(20.0), false); }
        if !self.b0.label.is_empty() { self.b0.paint(scene, fonts, s, now); } self.b1.paint(scene, fonts, s, now);
    }
}
