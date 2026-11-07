//! Section — a rail section's header in Spectrum 2's flat form (canon §25.4, the Lightroom reference): a bold 14 title
//! with a caret on the LEFT ("⌄ Light" open, "› Effects" closed), a full-width hairline above, no card. The header is
//! the whole click target and hovers gray-200 at radius 4 inside its padding. QCView's `CollapsibleSection` is a
//! tinted card with tracked tiny capitals; the flat header is the written deviation.
use crate::{anim::{mix, Ramp}, input::{Button, Event}, paint, Family, Fonts};
use kurbo::{Affine, Rect};
use min_theme::tokens as t;
use std::time::Instant;
use vello::Scene;

pub struct Section { pub title: String, pub expanded: bool, pub rect: Rect, hovered: bool, pressed: bool, hover: Ramp }

impl Section {
    pub fn new(title: &str, expanded: bool) -> Section { Section { title: title.to_string(), expanded, rect: Rect::ZERO, hovered: false, pressed: false, hover: Ramp::new(0.0, t::ANIMATION_DURATION_HOVER) } }
    pub fn height(scale: f64) -> f64 { t::RAIL_SECTION_HEADER_HEIGHT as f64 * scale }
    pub fn hit(&self, x: f64, y: f64) -> bool { self.rect.contains(kurbo::Point::new(x, y)) }
    /// Returns (toggled, redraw needed).
    pub fn event(&mut self, e: &Event, now: Instant) -> (bool, bool) {
        let inside = e.pos().is_some_and(|(x, y)| self.hit(x, y));
        let was = (self.hovered, self.pressed, self.expanded);
        match *e {
            Event::Move { .. } => self.hovered = inside,
            Event::Down { button: Button::Primary, .. } if inside => { self.pressed = true; self.hovered = true; }
            Event::Down { .. } => {}
            Event::Up { button: Button::Primary, .. } => { if self.pressed && inside { self.expanded = !self.expanded; } self.pressed = false; self.hovered = inside; }
            Event::Up { .. } => {}
            Event::Leave => { self.hovered = false; self.pressed = false; }
        }
        self.hover.set_target(if self.hovered { 1.0 } else { 0.0 }, now);
        (was.2 != self.expanded, was != (self.hovered, self.pressed, self.expanded))
    }
    pub fn animating(&self, now: Instant) -> bool { self.hover.animating(now) }
    /// Paint at `self.rect` (the row across the rail's content width); `first` skips the hairline above.
    pub fn paint(&self, scene: &mut Scene, fonts: &mut Fonts, scale: f64, now: Instant, first: bool) {
        let r = self.rect;
        if !first { paint::hline(scene, r.x0, r.x1, r.y0, t::DIVIDER_THICKNESS as f64 * scale, t::DIVIDER_COLOR); }
        let hv = self.hover.value(now);
        let inset = t::SPACING_75 as f64 * scale;
        let row = Rect::new(r.x0, r.y0 + inset, r.x1, r.y1 - inset);
        let bg = if self.pressed { t::DOWN_BACKGROUND_COLOR } else { mix(t::HOVER_BACKGROUND_COLOR_IDLE, t::HOVER_BACKGROUND_COLOR, hv) };
        if bg.a > 0.0 { paint::rounded(scene, row, t::CORNER_RADIUS_SMALL_DEFAULT as f64 * scale, bg); }
        let c = paint::color(t::RAIL_SECTION_HEADER_COLOR);
        let sz = t::RAIL_SECTION_CHEVRON_SIZE * scale as f32;
        let mut x = row.x0 + t::SPACING_100 as f64 * scale;
        if let Some(g) = fonts.icon(if self.expanded { "caret-down" } else { "caret-right" }, sz, c) { crate::fonts::draw_layout(scene, &g, Affine::translate((x, row.y0 + (row.height() - g.height() as f64) / 2.0))); x += sz as f64 + t::SPACING_100 as f64 * scale; }
        let l = fonts.layout(&self.title, Family::Sans, t::RAIL_SECTION_HEADER_SIZE * scale as f32, t::FONT_WEIGHT_BOLD as f32, c);
        crate::fonts::draw_layout(scene, &l, Affine::translate((x, row.y0 + (row.height() - l.height() as f64) / 2.0)));
    }
}
