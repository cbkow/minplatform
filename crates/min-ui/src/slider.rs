//! Slider — Spectrum 2's slider in the family's terms (canon §25.4, the Lightroom rail reference): a 1-px track with
//! the filled part one step lighter, a 16-px hollow ring knob (layer-1 fill, 2-px light stroke, brighter on hover),
//! no accent anywhere. Press sets the value at the pointer, drag follows, the value is 0..1.
use crate::{anim::{mix, Ramp}, input::{Button, Event}, paint};
use kurbo::{Affine, Circle, Rect, Stroke};
use min_theme::tokens as t;
use std::time::Instant;
use vello::Scene;

pub struct Slider { pub rect: Rect, pub value: f32, pub enabled: bool, pub tooltip: String, hovered: bool, dragging: bool, hover: Ramp }

impl Slider {
    pub fn new(value: f32) -> Slider { Slider { rect: Rect::ZERO, value: value.clamp(0.0, 1.0), enabled: true, tooltip: String::new(), hovered: false, dragging: false, hover: Ramp::new(0.0, t::ANIMATION_DURATION_HOVER) } }
    pub fn hovered(&self) -> bool { self.hovered }
    pub fn dragging(&self) -> bool { self.dragging }
    pub fn hit(&self, x: f64, y: f64) -> bool { self.rect.contains(kurbo::Point::new(x, y)) }
    fn knob_r(&self, scale: f64) -> f64 { t::SLIDER_HANDLE_SIZE as f64 * scale / 2.0 }
    fn track(&self, scale: f64) -> (f64, f64) { let r = self.knob_r(scale); (self.rect.x0 + r, self.rect.x1 - r) }
    fn value_at(&self, x: f64, scale: f64) -> f32 { let (a, b) = self.track(scale); if b <= a { 0.0 } else { ((x - a) / (b - a)).clamp(0.0, 1.0) as f32 } }
    /// Returns the new value when the pointer changed it, and whether a redraw is needed.
    pub fn event(&mut self, e: &Event, now: Instant, scale: f64) -> (Option<f32>, bool) {
        let inside = e.pos().is_some_and(|(x, y)| self.hit(x, y));
        let was = (self.hovered, self.dragging, self.value);
        let mut changed = None;
        match *e {
            Event::Move { x, .. } => { self.hovered = inside && self.enabled; if self.dragging { let v = self.value_at(x, scale); if v != self.value { self.value = v; changed = Some(v); } } }
            Event::Down { x, button: Button::Primary, .. } if inside && self.enabled => { self.dragging = true; self.hovered = true; let v = self.value_at(x, scale); if v != self.value { self.value = v; changed = Some(v); } }
            Event::Down { .. } => {}
            Event::Up { button: Button::Primary, .. } => { self.dragging = false; self.hovered = inside && self.enabled; }
            Event::Up { .. } => {}
            Event::Leave => { self.hovered = false; }
        }
        self.hover.set_target(if self.hovered || self.dragging { 1.0 } else { 0.0 }, now);
        (changed, was != (self.hovered, self.dragging, self.value))
    }
    pub fn animating(&self, now: Instant) -> bool { self.hover.animating(now) }
    pub fn paint(&self, scene: &mut Scene, scale: f64, now: Instant) {
        let (a, b) = self.track(scale); let cy = (self.rect.y0 + self.rect.y1) / 2.0;
        let th = t::SLIDER_TRACK_THICKNESS as f64 * scale;
        let kx = a + (b - a) * self.value as f64;
        let (track, fill, stroke) = if self.enabled { (t::SLIDER_TRACK_COLOR, t::SLIDER_TRACK_FILL_COLOR, mix(t::SLIDER_HANDLE_BORDER_COLOR, t::SLIDER_HANDLE_BORDER_COLOR_HOVER, self.hover.value(now))) } else { (t::DISABLED_BORDER_COLOR, t::DISABLED_BORDER_COLOR, t::DISABLED_CONTENT_COLOR) };
        paint::rect(scene, Rect::new(a, cy - th / 2.0, b, cy + th / 2.0), track);
        paint::rect(scene, Rect::new(a, cy - th / 2.0, kx, cy + th / 2.0), fill);
        let r = self.knob_r(scale); let bw = t::SLIDER_HANDLE_BORDER_WIDTH as f64 * scale;
        scene.fill(peniko::Fill::NonZero, Affine::IDENTITY, paint::color(t::SLIDER_HANDLE_FILL_COLOR), None, &Circle::new((kx, cy), r - bw / 2.0));
        scene.stroke(&Stroke::new(bw), Affine::IDENTITY, paint::color(stroke), None, &Circle::new((kx, cy), r - bw / 2.0));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn press_sets_and_drag_follows() {
        let now = Instant::now();
        let mut s = Slider::new(1.0); s.rect = Rect::new(0.0, 0.0, 116.0, 32.0);   // track 8..108
        let (v, _) = s.event(&Event::Down { x: 58.0, y: 16.0, button: Button::Primary }, now, 1.0);
        assert!((v.unwrap() - 0.5).abs() < 1e-6 && s.dragging());
        let (v, _) = s.event(&Event::Move { x: 500.0, y: 16.0 }, now, 1.0);
        assert_eq!(v, Some(1.0), "a drag past the end clamps");
        s.event(&Event::Up { x: 500.0, y: 16.0, button: Button::Primary }, now, 1.0);
        assert!(!s.dragging() && !s.hovered());
    }
}
