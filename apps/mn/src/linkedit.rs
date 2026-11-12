//! The link's editor (canon row 320): a popover at the caret with the address and the words, Remove / Open / Done.
//! Enter keeps it, Escape gives it up, Tab goes between the two fields. All of it is min-ui's (`Popover`, `TextField`,
//! `TextButton`).
use kurbo::Rect;
use min_theme::tokens as t;
use min_ui::{Event, Family, FieldResponse, Fonts, Key, KeyEvent, Named, Placement, Popover, TextButton, TextField, Variant};
use std::time::Instant;
use vello::Scene;

pub enum Out { None, Redraw, Close, Done { url: String, text: String }, Remove, Open(String) }

pub struct LinkEdit {
    /// where it stands (the caret or the selection, the note's coordinates)
    pub anchor: Rect,
    pop: Popover, url: TextField, text: TextField, remove: TextButton, open: TextButton, done: TextButton, scale: f64,
}
const W: f64 = 320.0; const FIELD: f64 = 32.0;

impl LinkEdit {
    /// `url`, `text`: what the link is now (empty: a new one); `linked`: there is a link to remove.
    pub fn new(anchor: Rect, url: &str, text: &str, linked: bool, fonts: &mut Fonts) -> LinkEdit {
        let field = |hint: &str, v: &str, fonts: &mut Fonts| { let mut f = TextField::new(Family::Sans, t::BODY_SIZE_M); f.placeholder = hint.into(); f.set_text_end(v, fonts); f };
        let mut u = field("Address \u{2014} example.com", url, fonts); u.focused = true; u.select_all(fonts);
        let mut remove = TextButton::new("Remove", Variant::Secondary).size_s(); remove.enabled = linked;
        LinkEdit { anchor, pop: Popover::new(""), url: u, text: field("Text", text, fonts), remove, open: TextButton::new("Open", Variant::Secondary).size_s(), done: TextButton::new("Done", Variant::Primary).size_s(), scale: 1.0 }
    }
    pub fn hit(&self, x: f64, y: f64) -> bool { self.pop.hit(x, y) }
    /// The field the keys go to (a paste, a copy).
    pub fn field(&mut self) -> &mut TextField { if self.url.focused { &mut self.url } else { &mut self.text } }
    pub fn animating(&self, now: Instant) -> bool { self.remove.animating(now) || self.open.animating(now) || self.done.animating(now) }
    /// Placed under `anchor` (window px) within `bounds`.
    pub fn layout(&mut self, fonts: &mut Fonts, anchor: Rect, bounds: Rect, s: f64) {
        self.scale = s; let gap = t::SPACING_100 as f64; let bh = t::COMPONENT_HEIGHT_75 as f64;
        self.pop.open_at_prefer(fonts, anchor, W, FIELD * 2.0 + gap * 2.0 + bh, bounds, s, Placement::Below);
        let k = self.pop.content; let f = FIELD * s;
        self.url.layout(fonts, Rect::new(k.x0, k.y0, k.x1, k.y0 + f), s); self.text.layout(fonts, Rect::new(k.x0, k.y0 + f + gap * s, k.x1, k.y0 + 2.0 * f + gap * s), s);
        let y = k.y1 - bh * s; self.open.enabled = mn_edit::links::openable(&mn_edit::links::normalise_url(self.url.text()));
        let (dw, _) = self.done.measure(fonts, s); let (ow, _) = self.open.measure(fonts, s); let (rw, _) = self.remove.measure(fonts, s);
        self.done.rect = Rect::new(k.x1 - dw, y, k.x1, k.y1); self.open.rect = Rect::new(k.x1 - dw - 8.0 * s - ow, y, k.x1 - dw - 8.0 * s, k.y1); self.remove.rect = Rect::new(k.x0, y, k.x0 + rw, k.y1);
    }
    fn done(&self) -> Out { let url = self.url.text().trim().to_string(); if url.is_empty() { Out::Remove } else { Out::Done { url, text: self.text.text().trim().to_string() } } }
    /// A pointer event. A press outside closes it (and is not taken: the page gets it).
    pub fn pointer(&mut self, e: &Event, fonts: &mut Fonts) -> (Out, bool) {
        let now = Instant::now();
        if self.remove.event(e, now).0 { return (Out::Remove, true) }
        if self.open.event(e, now).0 { return (Out::Open(mn_edit::links::normalise_url(self.url.text())), true) }
        if self.done.event(e, now).0 { return (self.done(), true) }
        match *e {
            Event::Down { x, y, .. } => { if !self.hit(x, y) { return (Out::Close, false) }
                for own in [true, false] { let f = if own { &mut self.url } else { &mut self.text }; if f.hit(x, y) { f.focused = true; f.pointer(e, fonts); let o = if own { &mut self.text } else { &mut self.url }; o.focused = false; } }
                (Out::Redraw, true) }
            Event::Move { x, y } => { if self.url.focused { self.url.pointer(e, fonts); } if self.text.focused { self.text.pointer(e, fonts); } (Out::Redraw, self.hit(x, y)) }
            Event::Up { .. } => { if self.url.focused { self.url.pointer(e, fonts); } if self.text.focused { self.text.pointer(e, fonts); } (Out::Redraw, true) }
            _ => (Out::None, false),
        }
    }
    pub fn ime(&mut self, e: &min_ui::ImeEvent, fonts: &mut Fonts) { if self.url.focused { self.url.ime(e, fonts); } else { self.text.ime(e, fonts); } }
    pub fn key(&mut self, k: &KeyEvent, fonts: &mut Fonts) -> Out {
        match k.key {
            Key::Named(Named::Escape) => Out::Close,
            Key::Named(Named::Enter) => self.done(),
            Key::Named(Named::Tab) => { let to_text = self.url.focused; self.url.focused = !to_text; self.text.focused = to_text; if to_text { self.text.select_all(fonts); } else { self.url.select_all(fonts); } Out::Redraw }
            _ => { let f = if self.url.focused { &mut self.url } else { &mut self.text }; match f.key(k, fonts) { FieldResponse::Accepted => self.done(), FieldResponse::Cancelled => Out::Close, _ => Out::Redraw } }
        }
    }
    pub fn paint(&mut self, scene: &mut Scene, fonts: &mut Fonts) {
        let s = self.scale; let now = Instant::now(); self.pop.paint(scene, fonts, s);
        self.url.paint(scene, fonts, s); self.text.paint(scene, fonts, s);
        self.remove.paint(scene, fonts, s, now); self.open.paint(scene, fonts, s, now); self.done.paint(scene, fonts, s, now);
    }
}
