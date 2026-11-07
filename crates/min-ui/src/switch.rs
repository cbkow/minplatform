//! Switch — Spectrum 2's switch, the DEFAULT (neutral) variant: a 26 × 14 pill track with a 10-px handle that slides
//! over the hover duration; on = gray-800 with a dark handle, off = gray-400 with a light handle; disabled = the gray-100
//! track with a gray-400 handle. No accent (the family rations blue to four uses — canon §25 row 162). QCView's
//! `FlatSwitch` is 32 × 18 with the accent track; the S2 size and the grey are the written deviation.
use crate::{anim::{mix, Ramp}, input::{Button, Event}, paint};
use kurbo::{Affine, Circle, Rect};
use min_theme::tokens as t;
use std::time::Instant;
use vello::Scene;

pub struct Switch { pub rect: Rect, pub on: bool, pub enabled: bool, hovered: bool, pressed: bool, hover: Ramp, knob: Ramp }

impl Switch {
    pub fn new(on: bool) -> Switch { Switch { rect: Rect::ZERO, on, enabled: true, hovered: false, pressed: false, hover: Ramp::new(0.0, t::ANIMATION_DURATION_HOVER), knob: Ramp::new(if on { 1.0 } else { 0.0 }, t::ANIMATION_DURATION_HOVER) } }
    /// The natural size at `scale`.
    pub fn size(scale: f64) -> (f64, f64) { (t::SWITCH_TRACK_WIDTH as f64 * scale, t::SWITCH_TRACK_HEIGHT as f64 * scale) }
    pub fn hovered(&self) -> bool { self.hovered }
    pub fn hit(&self, x: f64, y: f64) -> bool { self.rect.inflate(t::SPACING_75 as f64, t::SPACING_75 as f64).contains(kurbo::Point::new(x, y)) }
    /// Set without animating (a settings reload).
    pub fn set(&mut self, on: bool, now: Instant) { if self.on != on { self.on = on; self.knob.set_target(if on { 1.0 } else { 0.0 }, now); } }
    /// Returns the new state when the switch toggled, and whether a redraw is needed.
    pub fn event(&mut self, e: &Event, now: Instant) -> (Option<bool>, bool) {
        let inside = e.pos().is_some_and(|(x, y)| self.hit(x, y));
        let was = (self.hovered, self.pressed, self.on);
        match *e {
            Event::Move { .. } => self.hovered = inside && self.enabled,
            Event::Down { button: Button::Primary, .. } if inside && self.enabled => { self.pressed = true; self.hovered = true; }
            Event::Down { .. } => {}
            Event::Up { button: Button::Primary, .. } => { if self.pressed && inside { self.on = !self.on; self.knob.set_target(if self.on { 1.0 } else { 0.0 }, now); } self.pressed = false; self.hovered = inside && self.enabled; }
            Event::Up { .. } => {}
            Event::Leave => { self.hovered = false; self.pressed = false; }
        }
        self.hover.set_target(if self.hovered { 1.0 } else { 0.0 }, now);
        let toggled = if was.2 != self.on { Some(self.on) } else { None };
        (toggled, was != (self.hovered, self.pressed, self.on))
    }
    pub fn animating(&self, now: Instant) -> bool { self.hover.animating(now) || self.knob.animating(now) }
    pub fn paint(&self, scene: &mut Scene, scale: f64, now: Instant) {
        let r = self.rect; let h = r.height(); let hv = self.hover.value(now); let k = self.knob.value(now) as f64;
        let (track, handle) = if !self.enabled { (t::SWITCH_TRACK_COLOR_DISABLED, t::SWITCH_HANDLE_COLOR_DISABLED) } else {
            (mix(mix(t::SWITCH_TRACK_COLOR_OFF, t::SWITCH_TRACK_COLOR_OFF_HOVER, hv), mix(t::SWITCH_TRACK_COLOR_ON, t::SWITCH_TRACK_COLOR_ON_HOVER, hv), k as f32), mix(t::SWITCH_HANDLE_COLOR_OFF, t::SWITCH_HANDLE_COLOR_ON, k as f32))
        };
        paint::rounded(scene, r, h / 2.0, track);
        let hr = t::SWITCH_HANDLE_SIZE as f64 * scale / 2.0; let inset = (h - 2.0 * hr) / 2.0;
        let cx = r.x0 + inset + hr + k * (r.width() - 2.0 * (inset + hr));
        scene.fill(peniko::Fill::NonZero, Affine::IDENTITY, paint::color(handle), None, &Circle::new((cx, (r.y0 + r.y1) / 2.0), hr));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn click_toggles_release_outside_does_not() {
        let now = Instant::now();
        let mut s = Switch::new(false); s.rect = Rect::new(10.0, 10.0, 36.0, 24.0);
        s.event(&Event::Down { x: 20.0, y: 17.0, button: Button::Primary }, now);
        let (t, _) = s.event(&Event::Up { x: 20.0, y: 17.0, button: Button::Primary }, now);
        assert_eq!(t, Some(true));
        s.event(&Event::Down { x: 20.0, y: 17.0, button: Button::Primary }, now);
        let (t, _) = s.event(&Event::Up { x: 200.0, y: 17.0, button: Button::Primary }, now);
        assert_eq!(t, None, "a release away from the switch is a cancel"); assert!(s.on);
        s.enabled = false;
        s.event(&Event::Down { x: 20.0, y: 17.0, button: Button::Primary }, now);
        let (t, _) = s.event(&Event::Up { x: 20.0, y: 17.0, button: Button::Primary }, now);
        assert_eq!(t, None, "disabled ignores the press");
    }
}
