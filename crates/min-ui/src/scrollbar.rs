//! An overlay scrollbar for a scrolled box (the rails' look, canon §25.4: a pill 4 px in from the edge, shown while the
//! pointer is over the box or for a moment after a scroll, twice as wide, brighter and draggable on hover — the rail's
//! `scrollbar()` exactly). The box owns `offset`.
use crate::{anim::Ramp, paint, Button, Event};
use kurbo::Rect;
use min_theme::tokens as t;
use std::time::Instant;
use vello::Scene;

pub struct ScrollBar { pub offset: f64, content: f64, view: Rect, scale: f64, hover: bool, inside: bool, drag: Option<(f64, f64)>, last: Option<Instant>, widen: Ramp, /// along the bottom edge, scrolling sideways (the Notes cards row)
    pub horizontal: bool, /// shown whether or not the pointer is over the box (a page's own content, where nothing appears with the pointer)
    pub always: bool }
impl Default for ScrollBar { fn default() -> Self { ScrollBar { offset: 0.0, content: 0.0, view: Rect::ZERO, scale: 1.0, hover: false, inside: false, drag: None, last: None, horizontal: false, always: false, widen: Ramp::new(0.0, t::ANIMATION_DURATION_HOVER) } } }

impl ScrollBar {
    /// The visible box and the content's full height (px).
    pub fn set(&mut self, view: Rect, content: f64, scale: f64) { self.view = view; self.content = content; self.scale = scale; self.offset = self.offset.clamp(0.0, self.max()); }
    /// A bar along the bottom edge (offset = the sideways scroll).
    pub fn horizontal() -> ScrollBar { ScrollBar { horizontal: true, ..Default::default() } }
    fn len(&self) -> f64 { if self.horizontal { self.view.width() } else { self.view.height() } }
    pub fn max(&self) -> f64 { (self.content - self.len()).max(0.0) }
    pub fn dragging(&self) -> bool { self.drag.is_some() }
    /// The pointer is over the thumb.
    pub fn hovered(&self) -> bool { self.hover }
    pub fn thumb(&self) -> Option<Rect> {
        if self.max() <= 0.5 || self.len() <= 0.0 { return None }
        let w = t::SCROLLBAR_THICKNESS as f64 * self.scale; let inset = t::SPACING_50 as f64 * self.scale;
        let track = self.len() - 2.0 * inset;
        let h = (track * self.len() / self.content).max(t::COMPONENT_HEIGHT_75 as f64 * self.scale).min(track);
        let p = inset + (track - h) * self.offset / self.max();
        Some(if self.horizontal { Rect::new(self.view.x0 + p, self.view.y1 - inset - w, self.view.x0 + p + h, self.view.y1 - inset) } else { Rect::new(self.view.x1 - inset - w, self.view.y0 + p, self.view.x1 - inset, self.view.y0 + p + h) })
    }
    fn hit_thumb(&self, x: f64, y: f64) -> bool { let g = 4.0 * self.scale; self.thumb().is_some_and(|r| (if self.horizontal { r.inflate(0.0, g) } else { r.inflate(g, 0.0) }).contains(kurbo::Point::new(x, y))) }
    /// Returns (offset changed, the event was the bar's).
    pub fn event(&mut self, e: &Event, now: Instant) -> (bool, bool) {
        let before = (self.offset, self.hover, self.inside);
        if let Some((x, y)) = e.pos() { self.inside = self.view.contains(kurbo::Point::new(x, y)); self.hover = self.hit_thumb(x, y); } else { self.inside = false; self.hover = false; }
        let mut mine = false;
        match *e {
            Event::Down { x, y, button: Button::Primary } if self.hit_thumb(x, y) => { self.drag = Some((if self.horizontal { x } else { y }, self.offset)); mine = true; }
            Event::Move { x, y } if self.drag.is_some() => {
                let (y0, o0) = self.drag.unwrap(); let y = if self.horizontal { x } else { y };
                if let Some(th) = self.thumb() { let travel = (self.len() - if self.horizontal { th.width() } else { th.height() } - 2.0 * t::SPACING_50 as f64 * self.scale).max(1.0); self.offset = (o0 + (y - y0) * self.max() / travel).clamp(0.0, self.max()); }
                self.last = Some(now); mine = true;
            }
            Event::Up { .. } if self.drag.is_some() => { self.drag = None; mine = true; }
            _ => {}
        }
        self.widen.set_target(if self.hover || self.drag.is_some() { 1.0 } else { 0.0 }, now);
        ((self.offset, self.hover, self.inside) != before, mine)
    }
    pub fn wheel(&mut self, dy_px: f64, now: Instant) -> bool {
        let o = (self.offset + dy_px).clamp(0.0, self.max());
        if (o - self.offset).abs() < 0.5 { return false }
        self.offset = o; self.last = Some(now); true
    }
    pub fn animating(&self, now: Instant) -> bool { self.widen.animating(now) || self.last.is_some_and(|t0| now.duration_since(t0).as_millis() as u32 <= t::BANDS_HIDE_DELAY + 50) }
    pub fn paint(&self, scene: &mut Scene, now: Instant) {
        let Some(th) = self.thumb() else { return };
        let recent = self.last.is_some_and(|t0| now.duration_since(t0).as_millis() as u32 <= t::BANDS_HIDE_DELAY);
        if !(self.always || self.inside || recent || self.drag.is_some()) { return }
        let c = if self.hover || self.drag.is_some() { t::SCROLLBAR_COLOR_HOVER } else { t::SCROLLBAR_COLOR };
        // the rail's widening: the pill grows inward to twice its width under the pointer
        let k = 1.0 + self.widen.value(now) as f64;
        let (th, w) = if self.horizontal { let w = th.height() * k; (Rect::new(th.x0, th.y1 - w, th.x1, th.y1), w) } else { let w = th.width() * k; (Rect::new(th.x1 - w, th.y0, th.x1, th.y1), w) };
        paint::rounded(scene, th, w / 2.0, c);
    }
}
