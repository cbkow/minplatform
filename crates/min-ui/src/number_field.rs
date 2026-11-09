//! NumberField — Spectrum 2's number field: a TextField in the mono face with a stacked ± stepper at its right edge;
//! typing commits on Enter / focus loss (parsed, clamped to the range, else reverted — QCView's `FlatSpinBox` rule),
//! the steppers move by `step` and commit at once, the wheel over the field steps too. A `zero_label` shows in place
//! of 0 ("Auto" for the decode-thread count, QCView's row).
use crate::{input::{Button, Event, Key, KeyEvent, Named}, paint, Family, FieldResponse, Fonts, TextField};
use kurbo::{Affine, Rect};
use min_theme::tokens as t;
use vello::Scene;

pub struct NumberField {
    pub field: TextField,
    pub value: i64, pub min: i64, pub max: i64, pub step: i64,
    pub zero_label: Option<String>,
    pub enabled: bool,
    pub rect: Rect,
    up: Rect, down: Rect, hover_up: bool, hover_down: bool,
    scale: f64,
    /// the well's background (None = the field token's) — the Output card's controls share one recessed grey
    pub bg: Option<min_theme::Color>,
}

impl NumberField {
    pub fn new(value: i64, min: i64, max: i64, step: i64) -> NumberField {
        let mut field = TextField::new(Family::Mono, t::CODE_SIZE_S); field.bare = true;
        let mut f = NumberField { field, value: value.clamp(min, max), min, max, step: step.max(1), zero_label: None, enabled: true, rect: Rect::ZERO, up: Rect::ZERO, down: Rect::ZERO, hover_up: false, hover_down: false, scale: 1.0, bg: None };
        f.show();
        f
    }
    /// Show `label` in place of 0 ("Auto").
    pub fn zero_label(mut self, label: &str) -> NumberField { self.zero_label = Some(label.to_string()); self.show(); self }
    pub fn hit(&self, x: f64, y: f64) -> bool { self.rect.contains(kurbo::Point::new(x, y)) }
    pub fn focused(&self) -> bool { self.field.focused }
    fn display(&self) -> String { match (&self.zero_label, self.value) { (Some(l), 0) => l.clone(), _ => self.value.to_string() } }
    fn show(&mut self) { let s = self.display(); self.field.set_text(&s); }
    /// Set from outside (a settings change); refreshes the text unless the user is typing.
    pub fn set(&mut self, v: i64) { let v = v.clamp(self.min, self.max); if v != self.value || !self.field.focused { self.value = v; if !self.field.focused { self.show(); } } }
    /// Place the field: the steppers take the right `number-field-stepper-width`.
    pub fn layout(&mut self, fonts: &mut Fonts, rect: Rect, scale: f64) {
        self.rect = rect; self.scale = scale;
        let sw = t::NUMBER_FIELD_STEPPER_WIDTH as f64 * scale;
        let mid = (rect.y0 + rect.y1) / 2.0;
        self.up = Rect::new(rect.x1 - sw, rect.y0, rect.x1, mid);
        self.down = Rect::new(rect.x1 - sw, mid, rect.x1, rect.y1);
        self.field.layout(fonts, Rect::new(rect.x0, rect.y0, rect.x1 - sw, rect.y1), scale);
    }
    fn commit_text(&mut self) -> Option<i64> {
        let txt = self.field.text().trim().to_string();
        let parsed = if self.zero_label.as_deref().is_some_and(|l| l.eq_ignore_ascii_case(&txt)) { Some(0) } else { txt.parse::<i64>().ok().or_else(|| txt.parse::<f64>().ok().map(|f| f.round() as i64)) };
        let before = self.value;
        if let Some(v) = parsed { self.value = v.clamp(self.min, self.max); }
        self.show();
        (self.value != before).then_some(self.value)
    }
    fn nudge(&mut self, dir: i64) -> Option<i64> { let v = (self.value + dir * self.step).clamp(self.min, self.max); if v != self.value { self.value = v; self.show(); Some(v) } else { None } }
    /// Returns the committed value when it changed, and whether a redraw is needed.
    pub fn event(&mut self, e: &Event, fonts: &mut Fonts) -> (Option<i64>, bool) {
        if !self.enabled { return (None, false) }
        let pos = e.pos();
        let was = (self.hover_up, self.hover_down, self.field.focused);
        let mut committed = None;
        match *e {
            Event::Move { x, y } => { self.hover_up = self.up.contains(kurbo::Point::new(x, y)); self.hover_down = self.down.contains(kurbo::Point::new(x, y)); }
            Event::Down { x, y, button: Button::Primary } => {
                let p = kurbo::Point::new(x, y);
                if self.up.contains(p) { committed = self.nudge(1); }
                else if self.down.contains(p) { committed = self.nudge(-1); }
                else if !self.field.hit(x, y) && self.field.focused { self.field.focused = false; committed = self.commit_text(); }
            }
            Event::Leave => { self.hover_up = false; self.hover_down = false; }
            _ => {}
        }
        let was_focused = self.field.focused;
        let fd = self.field.pointer(e, fonts);
        // a press that focuses the field selects its number: a value is replaced, not edited (Qt's SpinBox, QCView's FlatSpinBox)
        if self.field.focused && !was_focused { self.field.select_all(fonts); }
        let _ = pos;
        (committed, fd || was != (self.hover_up, self.hover_down, self.field.focused) || committed.is_some())
    }
    /// Keys while focused: Up / Down step, Enter commits, Escape reverts; the rest edit the text.
    pub fn key(&mut self, k: &KeyEvent, fonts: &mut Fonts) -> Option<i64> {
        if !self.field.focused || !self.enabled { return None }
        match k.key { Key::Named(Named::Up) => return self.nudge(1), Key::Named(Named::Down) => return self.nudge(-1), _ => {} }
        match self.field.key(k, fonts) {
            FieldResponse::Accepted => { self.field.focused = false; self.commit_text() }
            FieldResponse::Cancelled => { self.field.focused = false; self.show(); None }
            _ => None,
        }
    }
    /// Wheel over the field: one step per notch (QCView's spin boxes step on the wheel).
    pub fn wheel(&mut self, dy: f64) -> Option<i64> { if !self.enabled || dy == 0.0 { return None } self.nudge(if dy > 0.0 { 1 } else { -1 }) }
    /// Blur without an event (the panel lost focus).
    pub fn blur(&mut self) -> Option<i64> { if self.field.focused { self.field.focused = false; self.commit_text() } else { None } }
    pub fn paint(&mut self, scene: &mut Scene, fonts: &mut Fonts, scale: f64) {
        // the field's well spans the whole rect; the editor draws its text in the left part
        let r = t::FIELD_CORNER_RADIUS_S as f64 * scale; let bw = t::BORDER_WIDTH_100 as f64 * scale;
        let border = if !self.enabled { t::DISABLED_BORDER_COLOR } else if self.field.focused { t::FIELD_BORDER_COLOR_FOCUS } else if self.field.hovered() || self.hover_up || self.hover_down { t::FIELD_BORDER_COLOR_HOVER } else { t::FIELD_BORDER_COLOR };
        paint::rounded(scene, self.rect, r, border);
        paint::rounded(scene, self.rect.inset(-bw), (r - bw).max(0.0), self.bg.unwrap_or(t::FIELD_BACKGROUND_COLOR));
        if self.enabled {
            self.field.paint(scene, fonts, scale);   // bare: the text alone inside this one well (no inner corners against the stepper seam — the user's nit, 2026-11-07)
        } else {
            let l = fonts.layout(&self.display(), Family::Mono, t::CODE_SIZE_S * scale as f32, t::FONT_WEIGHT_REGULAR as f32, paint::color(t::DISABLED_CONTENT_COLOR));
            crate::fonts::draw_layout(scene, &l, Affine::translate((self.rect.x0 + t::FIELD_EDGE_TO_TEXT as f64 * scale, self.rect.y0 + (self.rect.height() - l.height() as f64) / 2.0)));
        }
        // the seam and the two chevrons
        paint::rect(scene, Rect::new(self.up.x0, self.rect.y0 + bw, self.up.x0 + t::HAIRLINE as f64 * scale, self.rect.y1 - bw), border);
        let sz = t::ICON_SIZE_SMALL * scale as f32 * 0.75;
        for (r, slug, hot) in [(self.up, "caret-up", self.hover_up), (self.down, "caret-down", self.hover_down)] {
            let c = if !self.enabled { t::DISABLED_CONTENT_COLOR } else if hot { t::NUMBER_FIELD_STEPPER_COLOR_HOVER } else { t::NUMBER_FIELD_STEPPER_COLOR };
            if let Some(l) = fonts.icon(slug, sz, paint::color(c)) { crate::fonts::draw_layout(scene, &l, Affine::translate((r.x0 + (r.width() - l.width() as f64) / 2.0, r.y0 + (r.height() - l.height() as f64) / 2.0))); }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn key(k: Key, text: Option<&str>) -> KeyEvent { KeyEvent { key: k, text: text.map(str::to_string), shift: false, cmd: false } }
    #[test]
    fn steppers_typing_and_clamps() {
        let mut fonts = Fonts::new();
        let mut f = NumberField::new(0, 0, 32, 1).zero_label("Auto");
        f.layout(&mut fonts, Rect::new(0.0, 0.0, 96.0, 24.0), 1.0);
        assert_eq!(f.field.text(), "Auto");
        let (c, _) = f.event(&Event::Down { x: 90.0, y: 4.0, button: Button::Primary }, &mut fonts);
        assert_eq!(c, Some(1)); assert_eq!(f.field.text(), "1");
        assert_eq!(f.wheel(-120.0), Some(0)); assert_eq!(f.field.text(), "Auto");
        assert_eq!(f.wheel(-120.0), None, "clamped at the minimum");
        f.event(&Event::Down { x: 20.0, y: 12.0, button: Button::Primary }, &mut fonts);
        assert!(f.focused());
        for ch in ["9", "9"] { f.key(&key(Key::Char(ch.into()), Some(ch)), &mut fonts); }   // the focusing press selected "Auto"; typing replaces it
        assert_eq!(f.field.text(), "99");
        assert_eq!(f.key(&key(Key::Named(Named::Enter), None), &mut fonts), Some(32), "typed 99 commits clamped to 32");
        assert!(!f.focused());
        f.event(&Event::Down { x: 20.0, y: 12.0, button: Button::Primary }, &mut fonts);
        f.key(&key(Key::Char("x".into()), Some("x")), &mut fonts);
        assert_eq!(f.key(&key(Key::Named(Named::Escape), None), &mut fonts), None); assert_eq!(f.field.text(), "32", "escape reverts");
    }
}
