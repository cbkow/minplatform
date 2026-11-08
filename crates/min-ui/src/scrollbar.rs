//! An overlay scrollbar for a scrolled box (the rails' look, canon §25.4: a pill 4 px in from the edge, shown while the
//! pointer is over the box or for a moment after a scroll, brighter and draggable on hover). The box owns `offset`.
use crate::{paint, Button, Event};
use kurbo::Rect;
use min_theme::tokens as t;
use std::time::Instant;
use vello::Scene;

#[derive(Default)]
pub struct ScrollBar { pub offset: f64, content: f64, view: Rect, scale: f64, hover: bool, inside: bool, drag: Option<(f64, f64)>, last: Option<Instant> }

impl ScrollBar {
    /// The visible box and the content's full height (px).
    pub fn set(&mut self, view: Rect, content: f64, scale: f64) { self.view = view; self.content = content; self.scale = scale; self.offset = self.offset.clamp(0.0, self.max()); }
    pub fn max(&self) -> f64 { (self.content - self.view.height()).max(0.0) }
    pub fn dragging(&self) -> bool { self.drag.is_some() }
    pub fn thumb(&self) -> Option<Rect> {
        if self.max() <= 0.5 || self.view.height() <= 0.0 { return None }
        let w = t::SCROLLBAR_THICKNESS as f64 * self.scale; let inset = t::SPACING_50 as f64 * self.scale;
        let track = self.view.height() - 2.0 * inset;
        let h = (track * self.view.height() / self.content).max(24.0 * self.scale).min(track);
        let y = self.view.y0 + inset + (track - h) * self.offset / self.max();
        Some(Rect::new(self.view.x1 - inset - w, y, self.view.x1 - inset, y + h))
    }
    fn hit_thumb(&self, x: f64, y: f64) -> bool { self.thumb().is_some_and(|r| r.inflate(4.0 * self.scale, 0.0).contains(kurbo::Point::new(x, y))) }
    /// Returns (offset changed, the event was the bar's).
    pub fn event(&mut self, e: &Event, now: Instant) -> (bool, bool) {
        let before = (self.offset, self.hover, self.inside);
        if let Some((x, y)) = e.pos() { self.inside = self.view.contains(kurbo::Point::new(x, y)); self.hover = self.hit_thumb(x, y); } else { self.inside = false; self.hover = false; }
        let mut mine = false;
        match *e {
            Event::Down { x, y, button: Button::Primary } if self.hit_thumb(x, y) => { self.drag = Some((y, self.offset)); mine = true; }
            Event::Move { y, .. } if self.drag.is_some() => {
                let (y0, o0) = self.drag.unwrap();
                if let Some(th) = self.thumb() { let travel = (self.view.height() - th.height() - 2.0 * t::SPACING_50 as f64 * self.scale).max(1.0); self.offset = (o0 + (y - y0) * self.max() / travel).clamp(0.0, self.max()); }
                self.last = Some(now); mine = true;
            }
            Event::Up { .. } if self.drag.is_some() => { self.drag = None; mine = true; }
            _ => {}
        }
        ((self.offset, self.hover, self.inside) != before, mine)
    }
    pub fn wheel(&mut self, dy_px: f64, now: Instant) -> bool {
        let o = (self.offset + dy_px).clamp(0.0, self.max());
        if (o - self.offset).abs() < 0.5 { return false }
        self.offset = o; self.last = Some(now); true
    }
    pub fn animating(&self, now: Instant) -> bool { self.last.is_some_and(|t0| now.duration_since(t0).as_millis() as u32 <= t::BANDS_HIDE_DELAY + 50) }
    pub fn paint(&self, scene: &mut Scene, now: Instant) {
        let Some(th) = self.thumb() else { return };
        let recent = self.last.is_some_and(|t0| now.duration_since(t0).as_millis() as u32 <= t::BANDS_HIDE_DELAY);
        if !(self.inside || recent || self.drag.is_some()) { return }
        let c = if self.hover || self.drag.is_some() { t::SCROLLBAR_COLOR_HOVER } else { t::SCROLLBAR_COLOR };
        paint::rounded(scene, th, th.width() / 2.0, c);
    }
}
