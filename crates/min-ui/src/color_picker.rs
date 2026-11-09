//! ColorPicker — QCView's `ColorPickerInline.qml` as a node: a saturation × value square over the current hue, a hue bar,
//! and a Hex field (the Safety Guides' colour, the annotation colour — canon row 215). Opaque colours only (QCView's alpha is
//! always 1). Press-drag in the square or the bar picks; the Hex field commits on Enter / blur ("#rrggbb", "rrggbb").
use crate::{input::{Button, Event, ImeEvent, KeyEvent}, paint, text_field::{FieldResponse, TextField}, Family, Fonts};
use kurbo::{Affine, Circle, Rect, Stroke};
use min_theme::tokens as t;
use peniko::{Gradient, Fill};
use vello::Scene;

#[derive(Clone, Copy, PartialEq)]
enum Part { Sv, Hue }

pub struct ColorPicker { pub rect: Rect, sv: Rect, hue: Rect, pub field: TextField, h: f32, s: f32, v: f32, drag: Option<Part>, label: Rect }

/// "#rrggbb" / "rrggbb" → (r, g, b) 0..1.
pub fn parse_hex(s: &str) -> Option<[f32; 3]> {
    let h = s.trim().trim_start_matches('#'); if h.len() != 6 || !h.chars().all(|c| c.is_ascii_hexdigit()) { return None }
    let b = |i: usize| u8::from_str_radix(&h[i..i + 2], 16).ok().map(|v| v as f32 / 255.0);
    Some([b(0)?, b(2)?, b(4)?])
}
pub fn to_hex(c: [f32; 3]) -> String { let q = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8; format!("#{:02x}{:02x}{:02x}", q(c[0]), q(c[1]), q(c[2])) }
fn hsv_to_rgb(h: f32, s: f32, v: f32) -> [f32; 3] {
    let h6 = (h.rem_euclid(1.0)) * 6.0; let i = h6.floor() as i32; let f = h6 - i as f32;
    let (p, q, tt) = (v * (1.0 - s), v * (1.0 - s * f), v * (1.0 - s * (1.0 - f)));
    match i.rem_euclid(6) { 0 => [v, tt, p], 1 => [q, v, p], 2 => [p, v, tt], 3 => [p, q, v], 4 => [tt, p, v], _ => [v, p, q] }
}
fn rgb_to_hsv(c: [f32; 3]) -> (f32, f32, f32) {
    let (mx, mn) = (c[0].max(c[1]).max(c[2]), c[0].min(c[1]).min(c[2])); let d = mx - mn;
    let h = if d <= 0.0 { 0.0 } else if mx == c[0] { ((c[1] - c[2]) / d).rem_euclid(6.0) / 6.0 } else if mx == c[1] { ((c[2] - c[0]) / d + 2.0) / 6.0 } else { ((c[0] - c[1]) / d + 4.0) / 6.0 };
    (h, if mx <= 0.0 { 0.0 } else { d / mx }, mx)
}

impl ColorPicker {
    pub fn new(hex: &str) -> ColorPicker {
        let mut p = ColorPicker { rect: Rect::ZERO, sv: Rect::ZERO, hue: Rect::ZERO, field: TextField::new(Family::Mono, t::CODE_SIZE_S), h: 0.0, s: 1.0, v: 1.0, drag: None, label: Rect::ZERO };
        p.set_hex(hex); p
    }
    pub fn hex(&self) -> String { to_hex(hsv_to_rgb(self.h, self.s, self.v)) }
    /// Show `hex` (an outside change); the field follows unless it is being edited.
    pub fn set_hex(&mut self, hex: &str) {
        if let Some(c) = parse_hex(hex) { if to_hex(c) != self.hex() { let (h, s, v) = rgb_to_hsv(c); if s > 0.0 && v > 0.0 { self.h = h; } self.s = s; self.v = v; } }
        if !self.field.focused { self.field.set_text(&self.hex()); }
    }
    pub fn focused(&self) -> bool { self.field.focused }
    pub fn dragging(&self) -> bool { self.drag.is_some() }
    /// Lays out at `y` across [x0, x1]; returns the height used.
    pub fn layout(&mut self, fonts: &mut Fonts, x0: f64, x1: f64, y: f64, scale: f64) -> f64 {
        let gap = t::SPACING_100 as f64 * scale;
        let sq = ((x1 - x0) * 0.6).min(140.0 * scale);
        self.sv = Rect::new(x0, y, x1, y + sq);
        let hb = 12.0 * scale; self.hue = Rect::new(x0, self.sv.y1 + gap, x1, self.sv.y1 + gap + hb);
        let fh = t::FIELD_HEIGHT_S as f64 * scale; let fy = self.hue.y1 + gap;
        let lw = fonts.layout("Hex", Family::Sans, t::RAIL_LABEL_SIZE * scale as f32, t::FONT_WEIGHT_REGULAR as f32, paint::color(t::RAIL_LABEL_COLOR)).width() as f64 + gap;
        self.label = Rect::new(x0, fy, x0 + lw, fy + fh);
        self.field.layout(fonts, Rect::new(x0 + lw, fy, x1, fy + fh), scale);
        self.rect = Rect::new(x0, y, x1, fy + fh);
        self.rect.height()
    }
    fn pick(&mut self, x: f64, y: f64, part: Part) {
        match part {
            Part::Sv => { self.s = ((x - self.sv.x0) / self.sv.width().max(1.0)).clamp(0.0, 1.0) as f32; self.v = (1.0 - (y - self.sv.y0) / self.sv.height().max(1.0)).clamp(0.0, 1.0) as f32; }
            Part::Hue => { self.h = ((x - self.hue.x0) / self.hue.width().max(1.0)).clamp(0.0, 0.9999) as f32; }
        }
        if !self.field.focused { self.field.set_text(&self.hex()); }
    }
    /// Returns (a new colour as "#rrggbb", redraw needed).
    pub fn event(&mut self, e: &Event, fonts: &mut Fonts) -> (Option<String>, bool) {
        let before = self.hex(); let mut dirty = self.field.pointer(e, fonts);
        match *e {
            Event::Down { x, y, button: Button::Primary } => {
                let p = kurbo::Point::new(x, y);
                let part = if self.sv.contains(p) { Some(Part::Sv) } else if self.hue.inflate(0.0, 4.0).contains(p) { Some(Part::Hue) } else { None };
                if let Some(part) = part { self.drag = Some(part); self.pick(x, y, part); dirty = true; }
                if !self.field.hit(x, y) && self.field.focused { self.field.focused = false; dirty = true; if let Some(c) = self.commit_field() { return (Some(c), true) } }
            }
            Event::Move { x, y } => { if let Some(part) = self.drag { self.pick(x, y, part); dirty = true; } }
            Event::Up { .. } => { if self.drag.take().is_some() { dirty = true; } }
            _ => {}
        }
        let after = self.hex();
        (if after != before { Some(after) } else { None }, dirty)
    }
    fn commit_field(&mut self) -> Option<String> {
        let before = self.hex();
        if let Some(c) = parse_hex(self.field.text()) { let (h, s, v) = rgb_to_hsv(c); if s > 0.0 && v > 0.0 { self.h = h; } self.s = s; self.v = v; }
        self.field.set_text(&self.hex());
        let after = self.hex(); (after != before).then_some(after)
    }
    /// A key for the Hex field (while it has focus): Enter commits, Escape restores.
    pub fn key(&mut self, k: &KeyEvent, fonts: &mut Fonts) -> Option<String> {
        match self.field.key(k, fonts) {
            FieldResponse::Accepted => { self.field.focused = false; self.commit_field() }
            FieldResponse::Cancelled => { self.field.focused = false; self.field.set_text(&self.hex()); None }
            _ => None,
        }
    }
    /// The field loses focus (a press elsewhere): what it holds commits.
    pub fn blur(&mut self) -> Option<String> { if !self.field.focused { return None } self.field.focused = false; self.commit_field() }
    pub fn ime(&mut self, e: &ImeEvent, fonts: &mut Fonts) { if self.field.focused { self.field.ime(e, fonts); } }
    pub fn paint(&mut self, scene: &mut Scene, fonts: &mut Fonts, scale: f64) {
        let r = 4.0 * scale;
        let hue = hsv_to_rgb(self.h, 1.0, 1.0);
        let pc = |c: [f32; 3], a: f32| peniko::Color::new([c[0], c[1], c[2], a]);
        // the square: the hue, white → clear across, clear → black down
        let sq = kurbo::RoundedRect::from_rect(self.sv, r);
        scene.fill(Fill::NonZero, Affine::IDENTITY, pc(hue, 1.0), None, &sq);
        scene.fill(Fill::NonZero, Affine::IDENTITY, &Gradient::new_linear((self.sv.x0, self.sv.y0), (self.sv.x1, self.sv.y0)).with_stops([pc([1.0; 3], 1.0), pc([1.0; 3], 0.0)]), None, &sq);
        scene.fill(Fill::NonZero, Affine::IDENTITY, &Gradient::new_linear((self.sv.x0, self.sv.y0), (self.sv.x0, self.sv.y1)).with_stops([pc([0.0; 3], 0.0), pc([0.0; 3], 1.0)]), None, &sq);
        // the hue bar
        let stops: Vec<peniko::Color> = (0..=6).map(|i| pc(hsv_to_rgb(i as f32 / 6.0, 1.0, 1.0), 1.0)).collect();
        scene.fill(Fill::NonZero, Affine::IDENTITY, &Gradient::new_linear((self.hue.x0, self.hue.y0), (self.hue.x1, self.hue.y0)).with_stops(stops.as_slice()), None, &kurbo::RoundedRect::from_rect(self.hue, self.hue.height() / 2.0));
        // the markers: a ring at (s, v), a bar at the hue
        let (mx, my) = (self.sv.x0 + self.s as f64 * self.sv.width(), self.sv.y0 + (1.0 - self.v as f64) * self.sv.height());
        scene.stroke(&Stroke::new(2.0 * scale), Affine::IDENTITY, peniko::Color::WHITE, None, &Circle::new((mx, my), 5.0 * scale));
        scene.stroke(&Stroke::new(1.0 * scale), Affine::IDENTITY, peniko::Color::BLACK, None, &Circle::new((mx, my), 6.5 * scale));
        let hx = self.hue.x0 + self.h as f64 * self.hue.width();
        scene.stroke(&Stroke::new(2.0 * scale), Affine::IDENTITY, peniko::Color::WHITE, None, &kurbo::RoundedRect::new(hx - 3.0 * scale, self.hue.y0 - 2.0 * scale, hx + 3.0 * scale, self.hue.y1 + 2.0 * scale, 2.0 * scale));
        // the Hex row: its label, the field, a swatch of the colour at its end
        let l = fonts.layout("Hex", Family::Sans, t::RAIL_LABEL_SIZE * scale as f32, t::FONT_WEIGHT_REGULAR as f32, paint::color(t::RAIL_LABEL_COLOR));
        crate::fonts::draw_layout(scene, &l, Affine::translate((self.label.x0, self.label.y0 + (self.label.height() - l.height() as f64) / 2.0)));
        self.field.paint(scene, fonts, scale);
        let sw = self.field.rect.height() - 8.0 * scale;
        let swr = Rect::new(self.field.rect.x1 - sw - 4.0 * scale, self.field.rect.y0 + 4.0 * scale, self.field.rect.x1 - 4.0 * scale, self.field.rect.y1 - 4.0 * scale);
        scene.fill(Fill::NonZero, Affine::IDENTITY, pc(hsv_to_rgb(self.h, self.s, self.v), 1.0), None, &kurbo::RoundedRect::from_rect(swr, 2.0 * scale));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn hex_round_trips_through_hsv() {
        for h in ["#ffff00", "#ff0000", "#0189f1", "#808080", "#000000", "#ffffff"] { let p = ColorPicker::new(h); assert_eq!(p.hex(), h); }
        assert_eq!(parse_hex("FFFF00"), Some([1.0, 1.0, 0.0])); assert_eq!(parse_hex("#ff00"), None);
    }
}
