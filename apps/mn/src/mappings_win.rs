//! The Path Mappings window (canon row 350): the volumes this machine names - a shared drive's root as Windows, macOS
//! and Linux each call it - so that a file added from such a drive is recorded by its volume and its place on it, and
//! found on the other system. The table is the family's (`min_paths::map::Mapping`, row 121), kept PER MACHINE in the
//! settings folder's `mappings.json`; the first row that matches a path is the one that names it. Under each volume a
//! line says how many of the open collection's references its root on THIS system covers - a root typed wrong covers
//! none. Made of min-ui's parts, as the export windows are.
use kurbo::{Affine, Rect, RoundedRect};
use min_paths::map::{to_ref, Mapping, Ref};
use min_theme::tokens as t;
use min_ui::{action_button::{ActionButton, Response}, fonts::draw_layout, paint, Button, Event, Family, FieldResponse, Fonts, Key, KeyEvent, Named, ScrollBar, Switch, TextButton, TextField, Variant};
use std::time::Instant;
use vello::Scene;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Out { None, Redraw, Close, /// the table is to be kept and used
    Done, /// a folder is to be chosen for this row's root on this system
    Pick(usize) }

struct Row { id: String, on: Switch, /// the label, then the root on Windows, macOS, Linux
    f: [TextField; 4], remove: ActionButton, pick: ActionButton, rect: Rect }
pub struct Window {
    pub shown: bool, rows: Vec<Row>, /// the collection's references that are plain paths (what a new mapping could cover)
    refs: Vec<String>, focus: Option<(usize, usize)>, add: TextButton, cancel: TextButton, done: TextButton, bar: ScrollBar, card: Rect, list: Rect, scale: f64,
}
const CAPTIONS: [&str; 4] = ["Volume", "Windows", "macOS", "Linux"];
/// The field of a row that is this system's root.
fn here() -> usize { if cfg!(windows) { 1 } else if cfg!(target_os = "macos") { 2 } else { 3 } }
fn field(text: &str, placeholder: &str, fonts: &mut Fonts) -> TextField { let mut f = TextField::new(Family::Sans, t::BODY_SIZE_M); f.placeholder = placeholder.into(); f.set_text_end(text, fonts); f }
fn row_of(m: &Mapping, fonts: &mut Fonts) -> Row {
    let mut remove = ActionButton::new("x", "Remove this volume").size_s(); remove.icon_size = Some(t::ICON_SIZE_SMALL);
    let mut pick = ActionButton::new("folder-simple", "Choose the folder").size_s(); pick.icon_size = Some(t::ICON_SIZE_SMALL);
    Row { id: m.id.clone(), on: Switch::new(m.enabled), f: [field(&m.label, "A name, such as Projects", fonts), field(&m.win, "P:\\ or \\\\server\\share", fonts), field(&m.mac, "/Volumes/Projects", fonts), field(&m.lin, "/mnt/projects", fonts)], remove, pick, rect: Rect::ZERO }
}

impl Window {
    pub fn open(mappings: &[Mapping], refs: Vec<String>, fonts: &mut Fonts) -> Window {
        Window { shown: true, rows: mappings.iter().map(|m| row_of(m, fonts)).collect(), refs, focus: None, add: TextButton::new("Add Volume", Variant::Secondary), cancel: TextButton::new("Cancel", Variant::Secondary), done: TextButton::new("Done", Variant::Primary),
            bar: ScrollBar::default(), card: Rect::ZERO, list: Rect::ZERO, scale: 1.0 }
    }
    /// The table as it stands in the window (a row with no root at all is left out).
    pub fn mappings(&self) -> Vec<Mapping> {
        self.rows.iter().map(|r| { let v = |i: usize| r.f[i].text().trim().to_string(); Mapping { id: r.id.clone(), label: v(0), win: v(1), mac: v(2), lin: v(3), enabled: r.on.on } }).filter(|m| !(m.win.is_empty() && m.mac.is_empty() && m.lin.is_empty())).collect()
    }
    /// A folder was chosen for a row's root on this system.
    pub fn picked(&mut self, row: usize, path: &std::path::Path, fonts: &mut Fonts) { if let Some(r) = self.rows.get_mut(row) { r.f[here()].set_text_end(&path.to_string_lossy(), fonts); if r.f[0].text().trim().is_empty() { if let Some(n) = path.file_name() { r.f[0].set_text_end(&n.to_string_lossy(), fonts); } } } }
    /// The field that has the keys.
    pub fn field(&mut self) -> Option<&mut TextField> { let (r, i) = self.focus?; self.rows.get_mut(r).map(|r| &mut r.f[i]) }
    pub fn typing(&self) -> bool { self.shown && self.focus.is_some() }
    fn focus_on(&mut self, to: Option<(usize, usize)>) { for (ri, r) in self.rows.iter_mut().enumerate() { for (i, f) in r.f.iter_mut().enumerate() { f.focused = to == Some((ri, i)); } } self.focus = to; }
    /// How many of the collection's plain references this row's root on this system covers.
    fn covers(&self, row: usize) -> usize {
        let r = &self.rows[row]; let root = r.f[here()].text().trim().to_string(); if root.is_empty() { return 0 }
        let m = Mapping { id: "x".into(), label: String::new(), win: if here() == 1 { root.clone() } else { String::new() }, mac: if here() == 2 { root.clone() } else { String::new() }, lin: if here() == 3 { root } else { String::new() }, enabled: true };
        self.refs.iter().filter(|p| matches!(to_ref(p, min_paths::Os::current(), std::slice::from_ref(&m)), Ref::Vol { .. })).count()
    }
    fn layout(&mut self, fonts: &mut Fonts, view: Rect, s: f64) {
        self.scale = s; let pad = t::DIALOG_PADDING as f64 * s; let bh = t::BUTTON_HEIGHT_M as f64 * s;
        let (w, h) = ((680.0 * s).min(view.width() - 48.0 * s).max(360.0 * s), (640.0 * s).min(view.height() - 48.0 * s).max(300.0 * s));
        let (x0, y0) = (view.x0 + (view.width() - w) / 2.0, view.y0 + (view.height() - h) / 2.0);
        self.card = Rect::new(x0, y0, x0 + w, y0 + h);
        self.list = Rect::new(x0 + pad, y0 + pad + 74.0 * s, x0 + w - pad, y0 + h - pad - bh - 16.0 * s);
        let (fh, gap, card_h) = (30.0 * s, 6.0 * s, 4.0 * 36.0 * s + 34.0 * s);
        self.bar.set(self.list, self.rows.len() as f64 * (card_h + 10.0 * s) + 8.0 * s, s);
        let off = self.bar.offset; let b = t::ACTION_BUTTON_HEIGHT_S as f64 * s; let (sw, sh) = (t::SWITCH_TRACK_WIDTH as f64 * s, t::SWITCH_TRACK_HEIGHT as f64 * s);
        for (i, r) in self.rows.iter_mut().enumerate() {
            let top = self.list.y0 + 4.0 * s + i as f64 * (card_h + 10.0 * s) - off; r.rect = Rect::new(self.list.x0, top, self.list.x1 - 10.0 * s, top + card_h);
            let (lx, rx) = (r.rect.x0 + 12.0 * s + 72.0 * s, r.rect.x1 - 12.0 * s);
            for k in 0..4 { let y = r.rect.y0 + 10.0 * s + k as f64 * (fh + gap);
                let right = if k == 0 { rx - b - 8.0 * s - sw - 10.0 * s } else if k == here() { rx - b - 6.0 * s } else { rx };
                r.f[k].layout(fonts, Rect::new(lx, y, right, y + fh), s);
                if k == 0 { r.on.rect = Rect::new(right + 10.0 * s, y + (fh - sh) / 2.0, right + 10.0 * s + sw, y + (fh + sh) / 2.0); r.remove.rect = Rect::new(rx - b, y + (fh - b) / 2.0, rx, y + (fh + b) / 2.0); }
                if k == here() { r.pick.rect = Rect::new(rx - b, y + (fh - b) / 2.0, rx, y + (fh + b) / 2.0); } }
        }
        let by = y0 + h - pad - bh; let (wd, _) = self.done.measure(fonts, s); let (wc, _) = self.cancel.measure(fonts, s); let (wa, _) = self.add.measure(fonts, s);
        self.done.rect = Rect::new(x0 + w - pad - wd, by, x0 + w - pad, by + bh); self.cancel.rect = Rect::new(self.done.rect.x0 - 8.0 * s - wc, by, self.done.rect.x0 - 8.0 * s, by + bh);
        self.add.rect = Rect::new(x0 + pad, by, x0 + pad + wa, by + bh);
    }
    pub fn add_row(&mut self, fonts: &mut Fonts) { let m = Mapping { id: mn_doc::BlockId::new().to_string(), enabled: true, ..Default::default() }; self.rows.push(row_of(&m, fonts)); let n = self.rows.len() - 1; self.focus_on(Some((n, 0))); self.bar.offset = f64::MAX; }
    pub fn pointer(&mut self, e: &Event, fonts: &mut Fonts) -> Out {
        let now = Instant::now();
        let (_, mine) = self.bar.event(e, now); if mine { return Out::Redraw }
        if self.cancel.event(e, now).0 { return Out::Close }
        if self.done.event(e, now).0 { return Out::Done }
        if self.add.event(e, now).0 { self.add_row(fonts); return Out::Redraw }
        let inside = |x: f64, y: f64, list: Rect| list.contains(kurbo::Point::new(x, y));
        let in_list = e.pos().is_none_or(|(x, y)| inside(x, y, self.list));
        let (mut gone, mut pick) = (None, None);
        if in_list { for (i, r) in self.rows.iter_mut().enumerate() { r.on.event(e, now); if r.remove.event(e, now) == Response::Clicked { gone = Some(i); } if r.pick.event(e, now) == Response::Clicked { pick = Some(i); } } }
        if let Some(i) = gone { self.rows.remove(i); self.focus_on(None); return Out::Redraw }
        if let Some(i) = pick { return Out::Pick(i) }
        match *e {
            Event::Down { x, y, button: Button::Primary } => {
                let hit = if inside(x, y, self.list) { self.rows.iter().enumerate().find_map(|(ri, r)| r.f.iter().position(|f| f.hit(x, y)).map(|i| (ri, i))) } else { None };
                self.focus_on(hit); if let Some((r, i)) = hit { self.rows[r].f[i].pointer(e, fonts); }
                Out::Redraw }
            _ => { if let Some((r, i)) = self.focus { if let Some(row) = self.rows.get_mut(r) { row.f[i].pointer(e, fonts); } } Out::Redraw }
        }
    }
    pub fn wheel(&mut self, dy: f64) { self.bar.wheel(dy, Instant::now()); }
    /// A key: Tab goes from field to field, Enter keeps the table, Escape gives the changes up; anything else is the field's.
    pub fn key(&mut self, k: &KeyEvent, fonts: &mut Fonts) -> Out {
        match k.key {
            Key::Named(Named::Escape) => Out::Close, Key::Named(Named::Enter) => Out::Done,
            Key::Named(Named::Tab) => { let n = self.rows.len() * 4; if n == 0 { return Out::None }
                let at = self.focus.map(|(r, i)| r * 4 + i); let to = match (at, k.shift) { (None, false) => 0, (None, true) => n - 1, (Some(a), false) => (a + 1) % n, (Some(a), true) => (a + n - 1) % n };
                self.focus_on(Some((to / 4, to % 4))); if let Some(f) = self.field() { f.select_all(fonts); } Out::Redraw }
            _ => { if let Some(f) = self.field() { if f.key(k, fonts) != FieldResponse::None { return Out::Redraw } } Out::None }
        }
    }
    pub fn animating(&self, now: Instant) -> bool { self.shown && (self.add.animating(now) || self.cancel.animating(now) || self.done.animating(now) || self.bar.animating(now) || self.rows.iter().any(|r| r.on.animating(now) || r.remove.animating(now) || r.pick.animating(now))) }

    pub fn paint(&mut self, scene: &mut Scene, fonts: &mut Fonts, view: Rect, s: f64, now: Instant) {
        if !self.shown { return }
        self.layout(fonts, view, s);
        if self.bar.offset == f64::MAX { self.bar.offset = self.bar.max(); self.layout(fonts, view, s); }
        let c = paint::color; let pad = t::DIALOG_PADDING as f64 * s;
        let (ink, sub) = (c(t::NEUTRAL_CONTENT_COLOR_DEFAULT), c(t::NEUTRAL_SUBDUED_CONTENT_COLOR_DEFAULT));
        paint::rect(scene, view, t::DIALOG_SCRIM_COLOR);
        let rad = t::DIALOG_CORNER_RADIUS as f64 * s;
        for sh in t::POPOVER_SHADOW { scene.draw_blurred_rounded_rect(Affine::translate((sh.x as f64 * s, sh.y as f64 * s)), self.card.inflate(sh.spread as f64 * s, sh.spread as f64 * s), c(sh.color), rad, (sh.blur as f64 * s / 2.0).max(0.1)); }
        scene.fill(peniko::Fill::NonZero, Affine::IDENTITY, c(t::BACKGROUND_LAYER_1_COLOR), None, &RoundedRect::from_rect(self.card, rad));
        let (x0, x1, y0) = (self.card.x0 + pad, self.card.x1 - pad, self.card.y0 + pad); let wide = x1 - x0;
        let line = |scene: &mut Scene, fonts: &mut Fonts, text: &str, x: f64, cy: f64, size: f32, bold: bool, color: peniko::Color, room: f64| {
            let l = fonts.layout_fit(text, Family::Sans, size * s as f32, if bold { t::FONT_WEIGHT_BOLD } else { t::FONT_WEIGHT_REGULAR } as f32, color, room.max(8.0)); draw_layout(scene, &l, Affine::translate((x, cy - l.height() as f64 / 2.0))); };
        line(scene, fonts, "Path Mappings", x0, y0 + 10.0 * s, t::DIALOG_TITLE_SIZE, true, ink, wide);
        let about = "Name a shared volume's root on each system. A file added from such a volume is then recorded by the volume and its place on it, and is found on the other system. These are this machine's own; the first volume that matches a path names it.";
        let l = fonts.layout_width(about, Family::Sans, t::BODY_SIZE_S * s as f32, t::FONT_WEIGHT_REGULAR as f32, c(t::DIALOG_BODY_COLOR), Some(wide as f32)); draw_layout(scene, &l, Affine::translate((x0, y0 + 26.0 * s)));
        paint::hline(scene, x0, x1, self.list.y0 - 1.0 * s, t::DIVIDER_THICKNESS as f64 * s, t::DIVIDER_COLOR_MEDIUM); paint::hline(scene, x0, x1, self.list.y1, t::DIVIDER_THICKNESS as f64 * s, t::DIVIDER_COLOR_MEDIUM);
        scene.push_clip_layer(peniko::Fill::NonZero, Affine::IDENTITY, &self.list);
        if self.rows.is_empty() { line(scene, fonts, "No volumes yet. Add one for each shared drive your files are on.", self.list.x0 + 4.0 * s, self.list.y0 + 24.0 * s, t::BODY_SIZE_M, false, sub, self.list.width()); }
        let covers: Vec<usize> = (0..self.rows.len()).map(|i| self.covers(i)).collect(); let any_refs = !self.refs.is_empty();
        for (i, r) in self.rows.iter_mut().enumerate() {
            if r.rect.y1 < self.list.y0 || r.rect.y0 > self.list.y1 { continue }
            paint::rounded(scene, r.rect, t::CORNER_RADIUS_500 as f64 * s, t::BACKGROUND_LAYER_2_COLOR);
            for k in 0..4 { let fr = r.f[k].rect; line(scene, fonts, CAPTIONS[k], r.rect.x0 + 12.0 * s, fr.center().y, t::BODY_SIZE_S, k == 0, if k == here() || k == 0 { ink } else { sub }, 64.0 * s); r.f[k].paint(scene, fonts, s); }
            r.on.paint(scene, s, now); r.remove.paint(scene, fonts, s, now); r.pick.paint(scene, fonts, s, now);
            let root = r.f[here()].text().trim().is_empty();
            let hint = if root { format!("No root for {} yet: nothing is mapped on this machine.", CAPTIONS[here()]) } else if !any_refs { "This collection has no reference by a plain path to map.".to_string() } else { match covers[i] { 0 => "Covers none of this collection's references. Is the root right?".to_string(), 1 => "Covers 1 reference of this collection.".to_string(), n => format!("Covers {n} references of this collection.") } };
            line(scene, fonts, &hint, r.rect.x0 + 12.0 * s + 72.0 * s, r.rect.y1 - 16.0 * s, t::BODY_SIZE_S, false, sub, r.rect.width() - 96.0 * s);
        }
        scene.pop_layer();
        self.bar.paint(scene, now);
        self.add.paint(scene, fonts, s, now); self.cancel.paint(scene, fonts, s, now); self.done.paint(scene, fonts, s, now);
    }
}
