//! Find in the note (canon row 321): a bar at the page's top right - what to find, "3 of 12", match case, previous,
//! next, the replace row's switch, close; and a second row: what to put, Replace, Replace All. All of it is min-ui's.
use kurbo::{Affine, Rect};
use min_theme::tokens as t;
use min_ui::{action_button::{ActionButton, Response}, fonts::draw_layout, Event, Family, FieldResponse, Fonts, Key, KeyEvent, Named, Placement, Popover, TextButton, TextField, Variant};
use std::time::Instant;
use vello::Scene;

pub enum Out { None, Redraw, Close, Next, Prev, Changed, Replace, ReplaceAll }

pub struct FindBar {
    pub query: TextField, pub with: TextField, pub case: bool, pub replacing: bool,
    /// (the current hit, counted from 1; how many) - the window sets it
    pub count: (usize, usize),
    pop: Popover, b_case: ActionButton, b_prev: ActionButton, b_next: ActionButton, b_more: ActionButton, b_close: ActionButton, b_one: TextButton, b_all: TextButton, scale: f64, count_at: Rect,
}
const W: f64 = 460.0; const FIELD: f64 = 32.0;
fn small(icon: &'static str, tip: &str) -> ActionButton { let mut b = ActionButton::new(icon, tip).size_s(); b.icon_size = Some(t::ICON_SIZE_SMALL); b }

impl FindBar {
    pub fn new(seed: &str, fonts: &mut Fonts) -> FindBar {
        let mut q = TextField::new(Family::Sans, t::BODY_SIZE_M); q.placeholder = "Find in this note".into(); q.set_text_end(seed, fonts); q.focused = true; q.select_all(fonts);
        let mut w = TextField::new(Family::Sans, t::BODY_SIZE_M); w.placeholder = "Replace with".into();
        FindBar { query: q, with: w, case: false, replacing: false, count: (0, 0), pop: Popover::new(""), b_case: small("text-aa", "Match case"), b_prev: small("caret-up", "Previous"), b_next: small("caret-down", "Next"),
            b_more: small("swap", "Replace"), b_close: small("x", "Close"), b_one: TextButton::new("Replace", Variant::Secondary).size_s(), b_all: TextButton::new("Replace All", Variant::Secondary).size_s(), scale: 1.0, count_at: Rect::ZERO }
    }
    pub fn hit(&self, x: f64, y: f64) -> bool { self.pop.hit(x, y) }
    pub fn focused(&self) -> bool { self.query.focused || self.with.focused }
    /// The field the keys go to.
    pub fn field(&mut self) -> Option<&mut TextField> { if self.query.focused { Some(&mut self.query) } else if self.with.focused { Some(&mut self.with) } else { None } }
    pub fn focus(&mut self, fonts: &mut Fonts) { self.query.focused = true; self.with.focused = false; self.query.select_all(fonts); }
    pub fn blur(&mut self) { self.query.focused = false; self.with.focused = false; }
    pub fn animating(&self, now: Instant) -> bool { [&self.b_case, &self.b_prev, &self.b_next, &self.b_more, &self.b_close].iter().any(|b| b.animating(now)) || self.b_one.animating(now) || self.b_all.animating(now) }
    /// Placed at the top right of `page` (window px).
    pub fn layout(&mut self, fonts: &mut Fonts, page: Rect, s: f64) {
        self.scale = s; let gap = t::SPACING_100 as f64; let rows = if self.replacing { 2.0 } else { 1.0 };
        let w = W.min((page.width() / s - 48.0).max(260.0));
        let x1 = page.x1 - 16.0 * s; self.pop.open_at_prefer(fonts, Rect::new(x1 - (w + 2.0 * t::POPOVER_PADDING as f64) * s, page.y0, x1, page.y0), w, FIELD * rows + gap * (rows - 1.0), page, s, Placement::Below);
        let k = self.pop.content; let (f, b) = (FIELD * s, t::ACTION_BUTTON_HEIGHT_S as f64 * s); let by = k.y0 + (f - b) / 2.0;
        let mut x = k.x1; for btn in [&mut self.b_close, &mut self.b_more, &mut self.b_next, &mut self.b_prev, &mut self.b_case] { btn.rect = Rect::new(x - b, by, x, by + b); x -= b + 2.0 * s; }
        self.b_case.selected = self.case; self.b_more.selected = self.replacing; self.b_prev.enabled = self.count.1 > 0; self.b_next.enabled = self.count.1 > 0;
        self.count_at = Rect::new(x - 78.0 * s, k.y0, x - 4.0 * s, k.y0 + f); self.query.layout(fonts, Rect::new(k.x0, k.y0, self.count_at.x0 - 4.0 * s, k.y0 + f), s);
        if self.replacing { let y = k.y0 + f + gap * s; let (aw, ah) = self.b_all.measure(fonts, s); let (ow, _) = self.b_one.measure(fonts, s); let yy = y + (f - ah) / 2.0;
            self.b_all.rect = Rect::new(k.x1 - aw, yy, k.x1, yy + ah); self.b_one.rect = Rect::new(k.x1 - aw - 8.0 * s - ow, yy, k.x1 - aw - 8.0 * s, yy + ah);
            self.b_one.enabled = self.count.1 > 0; self.b_all.enabled = self.count.1 > 0; self.with.layout(fonts, Rect::new(k.x0, y, self.b_one.rect.x0 - 8.0 * s, y + f), s); }
        else { self.b_one.rect = Rect::ZERO; self.b_all.rect = Rect::ZERO; self.with.rect = Rect::ZERO; }
    }
    /// A pointer event; true: it was the bar's. A press elsewhere takes the keys away from the bar (it stays open).
    pub fn pointer(&mut self, e: &Event, fonts: &mut Fonts) -> (Out, bool) {
        let now = Instant::now();
        if self.b_close.event(e, now) == Response::Clicked { return (Out::Close, true) }
        if self.b_next.event(e, now) == Response::Clicked { return (Out::Next, true) }
        if self.b_prev.event(e, now) == Response::Clicked { return (Out::Prev, true) }
        if self.b_case.event(e, now) == Response::Clicked { self.case = !self.case; return (Out::Changed, true) }
        if self.b_more.event(e, now) == Response::Clicked { self.replacing = !self.replacing; if !self.replacing && self.with.focused { self.with.focused = false; self.query.focused = true; } return (Out::Redraw, true) }
        if self.replacing { if self.b_one.event(e, now).0 { return (Out::Replace, true) } if self.b_all.event(e, now).0 { return (Out::ReplaceAll, true) } }
        match *e {
            Event::Down { x, y, .. } => { if !self.hit(x, y) { let was = self.focused(); self.blur(); return (if was { Out::Redraw } else { Out::None }, false) }
                let in_q = self.query.hit(x, y); let in_w = self.replacing && self.with.hit(x, y);
                if in_q { self.query.focused = true; self.with.focused = false; self.query.pointer(e, fonts); } else if in_w { self.with.focused = true; self.query.focused = false; self.with.pointer(e, fonts); }
                (Out::Redraw, true) }
            Event::Move { x, y } => { if let Some(f) = self.field() { f.pointer(e, fonts); } (Out::Redraw, self.hit(x, y)) }
            Event::Up { x, y, .. } => { if let Some(f) = self.field() { f.pointer(e, fonts); } (Out::Redraw, self.hit(x, y)) }
            _ => (Out::None, false),
        }
    }
    pub fn ime(&mut self, e: &min_ui::ImeEvent, fonts: &mut Fonts) -> Out { let q = self.query.focused; let before = self.query.text().to_string(); if let Some(f) = self.field() { f.ime(e, fonts); } if q && self.query.text() != before { Out::Changed } else { Out::Redraw } }
    /// A key while one of its fields has the keys.
    pub fn key(&mut self, k: &KeyEvent, fonts: &mut Fonts) -> Out {
        match k.key {
            Key::Named(Named::Escape) => Out::Close,
            Key::Named(Named::Enter) => if self.with.focused { Out::Replace } else if k.shift { Out::Prev } else { Out::Next },
            Key::Named(Named::Tab) => { if self.replacing { let to_with = self.query.focused; self.query.focused = !to_with; self.with.focused = to_with; if to_with { self.with.select_all(fonts); } else { self.query.select_all(fonts); } } Out::Redraw }
            _ => { let q = self.query.focused; let Some(f) = self.field() else { return Out::None }; match f.key(k, fonts) { FieldResponse::Changed if q => Out::Changed, FieldResponse::Cancelled => Out::Close, _ => Out::Redraw } }
        }
    }
    pub fn paint(&mut self, scene: &mut Scene, fonts: &mut Fonts) {
        let s = self.scale; let now = Instant::now(); self.pop.paint(scene, fonts, s);
        self.query.paint(scene, fonts, s);
        let words = if self.query.text().is_empty() { String::new() } else if self.count.1 == 0 { "No results".to_string() } else { format!("{} of {}", self.count.0, self.count.1) };
        let l = fonts.layout_fit(&words, Family::Sans, t::RAIL_LABEL_SIZE * s as f32, t::FONT_WEIGHT_REGULAR as f32, min_ui::paint::color(t::NEUTRAL_SUBDUED_CONTENT_COLOR_DEFAULT), self.count_at.width());
        draw_layout(scene, &l, Affine::translate((self.count_at.x1 - l.width() as f64, self.count_at.center().y - l.height() as f64 / 2.0)));
        for b in [&self.b_case, &self.b_prev, &self.b_next, &self.b_more, &self.b_close] { b.paint(scene, fonts, s, now); }
        if self.replacing { self.with.paint(scene, fonts, s); self.b_one.paint(scene, fonts, s, now); self.b_all.paint(scene, fonts, s, now); }
    }
}
