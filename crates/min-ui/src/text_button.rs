//! TextButton — Spectrum 2's Button (a labelled button, as against the icon ActionButton): a PILL (§25.4: pills for
//! standalone actions) at height 32, bold 14 label, three variants — primary (gray-800 fill, inverted text), secondary
//! (gray-300 fill, light text — the family's deliberate grey), negative (red fill, white text, "red last" in the
//! context-menu grammar). Hover one step up over the hover duration, pressed one more; disabled = gray-100 with gray-400.
use crate::{anim::{mix, Ramp}, input::{Button, Event}, paint, Family, Fonts};
use kurbo::{Affine, Rect};
use min_theme::tokens as t;
use std::time::Instant;
use vello::Scene;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Variant { Primary, Secondary, Negative }

pub struct TextButton { pub label: String, pub variant: Variant, pub enabled: bool, pub rect: Rect, /// size S: 24 px, bold 12 — the rail's size (the user 2026-11-07: match the chips)
    pub size_s: bool, hovered: bool, pressed: bool, hover: Ramp }

impl TextButton {
    pub fn new(label: &str, variant: Variant) -> TextButton { TextButton { label: label.to_string(), variant, enabled: true, rect: Rect::ZERO, size_s: false, hovered: false, pressed: false, hover: Ramp::new(0.0, t::ANIMATION_DURATION_HOVER) } }
    pub fn size_s(mut self) -> TextButton { self.size_s = true; self }
    fn font(&self) -> f32 { if self.size_s { t::BODY_SIZE_S } else { t::BODY_SIZE_M } }
    pub fn hovered(&self) -> bool { self.hovered }
    pub fn hit(&self, x: f64, y: f64) -> bool { self.rect.contains(kurbo::Point::new(x, y)) }
    /// The natural size at `scale`: label width plus the pill's edge insets, height M.
    pub fn measure(&self, fonts: &mut Fonts, scale: f64) -> (f64, f64) {
        let l = fonts.layout(&self.label, Family::Sans, self.font() * scale as f32, t::FONT_WEIGHT_BOLD as f32, peniko::Color::WHITE);
        let (edge, h) = if self.size_s { (t::COMPONENT_PILL_EDGE_TO_TEXT_75, t::COMPONENT_HEIGHT_75) } else { (t::BUTTON_PILL_EDGE_TO_TEXT_M, t::BUTTON_HEIGHT_M) };
        (l.width() as f64 + 2.0 * edge as f64 * scale, h as f64 * scale)
    }
    /// True on a click (press and release inside); the second value = redraw needed.
    pub fn event(&mut self, e: &Event, now: Instant) -> (bool, bool) {
        let inside = e.pos().is_some_and(|(x, y)| self.hit(x, y));
        let was = (self.hovered, self.pressed);
        let mut clicked = false;
        match *e {
            Event::Move { .. } => self.hovered = inside && self.enabled,
            Event::Down { button: Button::Primary, .. } if inside && self.enabled => { self.pressed = true; self.hovered = true; }
            Event::Down { .. } => {}
            Event::Up { button: Button::Primary, .. } => { if self.pressed && inside { clicked = true; } self.pressed = false; self.hovered = inside && self.enabled; }
            Event::Up { .. } => {}
            Event::Leave => { self.hovered = false; self.pressed = false; }
        }
        self.hover.set_target(if self.hovered { 1.0 } else { 0.0 }, now);
        (clicked, was != (self.hovered, self.pressed) || clicked)
    }
    pub fn reset(&mut self, now: Instant) { self.hovered = false; self.pressed = false; self.hover.set_target(0.0, now); }
    pub fn animating(&self, now: Instant) -> bool { self.hover.animating(now) }
    pub fn paint(&self, scene: &mut Scene, fonts: &mut Fonts, scale: f64, now: Instant) {
        let hv = self.hover.value(now);
        let (bg, fg) = if !self.enabled { (t::BUTTON_DISABLED_BACKGROUND_COLOR, t::BUTTON_DISABLED_CONTENT_COLOR) } else { match self.variant {
            Variant::Primary => (if self.pressed { t::BUTTON_PRIMARY_BACKGROUND_COLOR_DOWN } else { mix(t::BUTTON_PRIMARY_BACKGROUND_COLOR, t::BUTTON_PRIMARY_BACKGROUND_COLOR_HOVER, hv) }, t::BUTTON_PRIMARY_CONTENT_COLOR),
            Variant::Secondary => (if self.pressed { t::DOWN_BACKGROUND_COLOR } else { mix(t::BUTTON_SECONDARY_BACKGROUND_COLOR, t::BUTTON_SECONDARY_BACKGROUND_COLOR_HOVER, hv) }, t::BUTTON_SECONDARY_CONTENT_COLOR),
            Variant::Negative => (if self.pressed { t::NEGATIVE_BACKGROUND_COLOR_HOVER } else { mix(t::BUTTON_NEGATIVE_BACKGROUND_COLOR, t::BUTTON_NEGATIVE_BACKGROUND_COLOR_HOVER, hv) }, t::BUTTON_NEGATIVE_CONTENT_COLOR),
        } };
        paint::rounded(scene, self.rect, paint::radius(t::BUTTON_CORNER_RADIUS, scale, self.rect), bg);
        let l = fonts.layout(&self.label, Family::Sans, self.font() * scale as f32, t::FONT_WEIGHT_BOLD as f32, paint::color(fg));
        crate::fonts::draw_layout(scene, &l, Affine::translate((self.rect.x0 + (self.rect.width() - l.width() as f64) / 2.0, self.rect.y0 + (self.rect.height() - l.height() as f64) / 2.0)));
    }
}
