//! ActionButton — Spectrum 2's action button in the family's terms (canon §25 rows 162 / 165, the 2026-11-07 ruling on
//! the transport row): size M 32 × 32 for an icon alone (6 + 20 + 6), radius 8, no border, quiet idle; hover gray-200
//! ramped over 100 ms from its alpha-0 twin, pressed one step past (gray-300, the family's rule — Spectrum has no
//! pressed step); selected = a deliberate mid grey (gray-300, light glyph), not Spectrum's inverted fill ("more
//! deliberate greys than inverts"); selected + emphasized = the accent with white; disabled = the gray-400 glyph with no
//! fill; keyboard focus = a 2-px accent ring with a 2-px gap. A `hold` button reports press begin / end (rewind,
//! fast-forward); the others click on release inside.
use crate::{anim::{mix, Ramp}, input::{Button, Event}, paint, Family, Fonts};
use kurbo::{Affine, Rect, RoundedRect, RoundedRectRadii, Stroke};
use min_theme::tokens as t;
use std::time::Instant;
use vello::Scene;

/// Per-corner radii in physical px (a compact group squares the shared edges).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Corners { pub tl: f64, pub tr: f64, pub br: f64, pub bl: f64 }
impl Corners { pub fn all(r: f64) -> Corners { Corners { tl: r, tr: r, br: r, bl: r } } }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Response { None, Clicked, PressBegan, PressEnded }

pub struct ActionButton {
    pub icon: &'static str,
    pub label: Option<String>,
    pub tooltip: String,
    pub enabled: bool,
    pub selected: bool,
    pub emphasized: bool,
    /// Press-and-hold: `PressBegan` on the press, `PressEnded` on release anywhere or when the pointer leaves.
    pub hold: bool,
    pub focused: bool,
    /// Size S (24 px, a bold 12 label — the reference's "Auto / B&W / HDR" chips) for buttons that live inside a rail.
    pub size_s: bool,
    pub rect: Rect,
    pub corners: Corners,
    /// a smaller glyph in the same button (logical px; None = the toolbar size) — the Output header's speaker
    pub icon_size: Option<f32>,
    hovered: bool,
    pressed: bool,
    hover: Ramp,
}

impl ActionButton {
    pub fn new(icon: &'static str, tooltip: &str) -> ActionButton {
        ActionButton { icon, label: None, tooltip: tooltip.to_string(), enabled: true, selected: false, emphasized: false, hold: false, focused: false, size_s: false, rect: Rect::ZERO, corners: Corners::all(0.0), icon_size: None, hovered: false, pressed: false, hover: Ramp::new(0.0, t::ACTION_BUTTON_ANIMATION_DURATION) }
    }
    pub fn hold(mut self) -> ActionButton { self.hold = true; self }
    /// The primary button is down on this node (a hold in progress, or a click not yet released).
    pub fn is_pressed(&self) -> bool { self.pressed }
    pub fn enabled(mut self, on: bool) -> ActionButton { self.enabled = on; self }
    pub fn size_s(mut self) -> ActionButton { self.size_s = true; self }
    fn label_font(&self) -> (f32, f32) { if self.size_s { (t::BODY_SIZE_S, t::FONT_WEIGHT_BOLD as f32) } else { (t::BODY_SIZE_M, t::FONT_WEIGHT_REGULAR as f32) } }
    pub fn hovered(&self) -> bool { self.hovered }
    /// The label is cut with … at this size (paint's arithmetic): an owner without tooltips shows one then.
    pub fn label_cut(&self, fonts: &mut Fonts, scale: f64) -> bool {
        let Some(label) = &self.label else { return false };
        let (fs, fw) = self.label_font();
        let edge = t::ACTION_BUTTON_EDGE_TO_TEXT_M as f64 * scale;
        let icon = if self.icon.is_empty() { 0.0 } else { t::ICON_SIZE_TOOLBAR as f64 * scale + t::ACTION_BUTTON_TEXT_TO_VISUAL_M as f64 * scale };
        fonts.layout(label, Family::Sans, fs * scale as f32, fw, peniko::Color::WHITE).width() as f64 > self.rect.width() - 2.0 * edge - icon
    }
    pub fn pressed(&self) -> bool { self.pressed }
    pub fn hit(&self, x: f64, y: f64) -> bool { self.rect.contains(kurbo::Point::new(x, y)) }
    /// The natural size at `scale`: a square for an icon alone; a label adds its width plus the edge insets.
    pub fn measure(&self, fonts: &mut Fonts, scale: f64) -> (f64, f64) {
        let h = (if self.size_s { t::ACTION_BUTTON_HEIGHT_S } else { t::ACTION_BUTTON_HEIGHT_M }) as f64 * scale;
        match &self.label {
            None => (h, h),
            Some(l) => {
                let (fs, fw) = self.label_font();
                let lay = fonts.layout(l, Family::Sans, fs * scale as f32, fw, peniko::Color::WHITE);
                let icon_w = if self.icon.is_empty() { 0.0 } else { t::ICON_SIZE_TOOLBAR as f64 * scale + t::ACTION_BUTTON_TEXT_TO_VISUAL_M as f64 * scale };
                (2.0 * t::ACTION_BUTTON_EDGE_TO_TEXT_M as f64 * scale + icon_w + lay.width() as f64, h)
            }
        }
    }
    pub fn event(&mut self, e: &Event, now: Instant) -> Response {
        let inside = e.pos().is_some_and(|(x, y)| self.hit(x, y));
        let mut r = Response::None;
        match *e {
            Event::Move { .. } => {
                self.hovered = inside && self.enabled;
                if self.hold && self.pressed && !inside { self.pressed = false; r = Response::PressEnded; }   // QCView: leaving a held button stops the shuttle
            }
            Event::Down { button: Button::Primary, .. } if inside && self.enabled => { self.pressed = true; self.hovered = true; if self.hold { r = Response::PressBegan; } }
            Event::Down { .. } => {}
            Event::Up { button: Button::Primary, .. } => {
                if self.pressed { self.pressed = false; r = if self.hold { Response::PressEnded } else if inside { Response::Clicked } else { Response::None }; }
                self.hovered = inside && self.enabled;
            }
            Event::Up { .. } => {}
            Event::Leave => { self.hovered = false; if self.pressed { self.pressed = false; if self.hold { r = Response::PressEnded; } } }
        }
        self.hover.set_target(if self.hovered { 1.0 } else { 0.0 }, now);
        r
    }
    /// Drop hover / press without an event (the node was disabled or hidden under the pointer).
    pub fn reset(&mut self, now: Instant) { self.hovered = false; self.pressed = false; self.hover.set_target(0.0, now); }
    pub fn animating(&self, now: Instant) -> bool { self.hover.animating(now) }

    pub fn paint(&self, scene: &mut Scene, fonts: &mut Fonts, scale: f64, now: Instant) {
        let shape = RoundedRect::from_rect(self.rect, RoundedRectRadii::new(self.corners.tl, self.corners.tr, self.corners.br, self.corners.bl));
        let hover = self.hover.value(now);
        let (bg, fg) = if !self.enabled {
            (None, t::ACTION_BUTTON_CONTENT_COLOR_DISABLED)
        } else if self.selected && self.emphasized {
            (Some(if self.pressed { t::ACCENT_BACKGROUND_COLOR_DOWN } else { mix(t::ACTION_BUTTON_BACKGROUND_COLOR_SELECTED_EMPHASIZED, t::ACTION_BUTTON_BACKGROUND_COLOR_SELECTED_EMPHASIZED_HOVER, hover) }), t::ACTION_BUTTON_CONTENT_COLOR_SELECTED_EMPHASIZED)
        } else if self.selected {
            (Some(if self.pressed { t::ACTION_BUTTON_BACKGROUND_COLOR_SELECTED_DOWN } else { mix(t::ACTION_BUTTON_BACKGROUND_COLOR_SELECTED, t::ACTION_BUTTON_BACKGROUND_COLOR_SELECTED_HOVER, hover) }), t::ACTION_BUTTON_CONTENT_COLOR_SELECTED)
        } else {
            (Some(if self.pressed { t::ACTION_BUTTON_BACKGROUND_COLOR_DOWN } else { mix(t::ACTION_BUTTON_BACKGROUND_COLOR_IDLE, t::ACTION_BUTTON_BACKGROUND_COLOR_HOVER, hover) }), if self.hovered { t::ACTION_BUTTON_CONTENT_COLOR_HOVER } else { t::ACTION_BUTTON_CONTENT_COLOR })
        };
        if let Some(c) = bg { if c.a > 0.0 { scene.fill(peniko::Fill::NonZero, Affine::IDENTITY, paint::color(c), None, &shape); } }
        if self.focused {
            let (gap, th) = (t::ACTION_BUTTON_FOCUS_INDICATOR_GAP as f64 * scale, t::ACTION_BUTTON_FOCUS_INDICATOR_THICKNESS as f64 * scale);
            let o = gap + th / 2.0;
            let ring = RoundedRect::from_rect(self.rect.inflate(o, o), RoundedRectRadii::new(self.corners.tl + o, self.corners.tr + o, self.corners.br + o, self.corners.bl + o));
            scene.stroke(&Stroke::new(th), Affine::IDENTITY, paint::color(t::ACTION_BUTTON_FOCUS_INDICATOR_COLOR), None, &ring);
        }
        let icon_px = self.icon_size.unwrap_or(t::ICON_SIZE_TOOLBAR) * scale as f32;
        let mut x = self.rect.x0 + if self.label.is_some() { t::ACTION_BUTTON_EDGE_TO_TEXT_M as f64 * scale } else { (self.rect.width() - icon_px as f64) / 2.0 };
        if !self.icon.is_empty() {
            if let Some(l) = fonts.icon(self.icon, icon_px, paint::color(fg)) {
                let ix = if self.label.is_some() { x } else { self.rect.x0 + (self.rect.width() - l.width() as f64) / 2.0 };
                crate::fonts::draw_layout(scene, &l, Affine::translate((ix, self.rect.y0 + (self.rect.height() - l.height() as f64) / 2.0)));
                x += icon_px as f64 + t::ACTION_BUTTON_TEXT_TO_VISUAL_M as f64 * scale;
            }
        }
        if let Some(label) = &self.label {
            let (fs, fw) = self.label_font();
            let l = fonts.layout_fit(label, Family::Sans, fs * scale as f32, fw, paint::color(fg), self.rect.x1 - x - t::ACTION_BUTTON_EDGE_TO_TEXT_M as f64 * scale);   // the tooltip carries the whole label
            crate::fonts::draw_layout(scene, &l, Affine::translate((x, self.rect.y0 + (self.rect.height() - l.height() as f64) / 2.0)));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn click_is_release_inside_and_hold_reports_both_ends() {
        let now = Instant::now();
        let mut b = ActionButton::new("play", "Play"); b.rect = Rect::new(0.0, 0.0, 32.0, 32.0);
        assert_eq!(b.event(&Event::Down { x: 10.0, y: 10.0, button: Button::Primary }, now), Response::None);
        assert!(b.pressed());
        assert_eq!(b.event(&Event::Up { x: 100.0, y: 10.0, button: Button::Primary }, now), Response::None, "released outside: no click");
        b.event(&Event::Down { x: 10.0, y: 10.0, button: Button::Primary }, now);
        assert_eq!(b.event(&Event::Up { x: 12.0, y: 12.0, button: Button::Primary }, now), Response::Clicked);
        let mut h = ActionButton::new("rewind", "Rewind (hold)").hold(); h.rect = Rect::new(0.0, 0.0, 32.0, 32.0);
        assert_eq!(h.event(&Event::Down { x: 1.0, y: 1.0, button: Button::Primary }, now), Response::PressBegan);
        assert_eq!(h.event(&Event::Move { x: 90.0, y: 1.0 }, now), Response::PressEnded, "leaving a held button ends the hold");
        assert_eq!(h.event(&Event::Up { x: 90.0, y: 1.0, button: Button::Primary }, now), Response::None);
        let mut d = ActionButton::new("play", "Play").enabled(false); d.rect = Rect::new(0.0, 0.0, 32.0, 32.0);
        assert_eq!(d.event(&Event::Down { x: 1.0, y: 1.0, button: Button::Primary }, now), Response::None);
        assert!(!d.hovered());
    }
    #[test]
    fn hover_ramps_in_over_the_token_duration() {
        let t0 = Instant::now();
        let mut b = ActionButton::new("play", "Play"); b.rect = Rect::new(0.0, 0.0, 32.0, 32.0);
        b.event(&Event::Move { x: 5.0, y: 5.0 }, t0);
        assert!(b.animating(t0));
        let mid = t0 + std::time::Duration::from_millis(50);
        let end = t0 + std::time::Duration::from_millis(t::ACTION_BUTTON_ANIMATION_DURATION as u64 + 1);
        assert!(!b.animating(end));
        let _ = mid;
    }
}
