//! ActionGroup — a row of action buttons: regular (8-px gaps, every button its own rounded rect) or compact (fused:
//! gap 0, the radius only on the group's two ends — Spectrum 2's compact group; the transport cluster per the
//! 2026-11-07 ruling).
use crate::{action_button::{ActionButton, Corners, Response}, input::Event, Fonts};
use kurbo::Rect;
use min_theme::tokens as t;
use std::time::Instant;
use vello::Scene;

pub struct ActionGroup { pub buttons: Vec<ActionButton>, pub compact: bool, pub rect: Rect }

impl ActionGroup {
    pub fn new(buttons: Vec<ActionButton>) -> ActionGroup { ActionGroup { buttons, compact: false, rect: Rect::ZERO } }
    pub fn compact(mut self) -> ActionGroup { self.compact = true; self }
    /// Lay the buttons out from `(x, y)`; returns the group's width. Hidden buttons (`enabled` is a state, hiding is `visible`) are not modelled yet: QCView hides the clip-step buttons outside playlists — piece 6c.
    pub fn layout(&mut self, fonts: &mut Fonts, x: f64, y: f64, scale: f64) -> f64 {
        let gap = if self.compact { t::ACTION_GROUP_GAP_COMPACT } else { t::ACTION_GROUP_GAP } as f64 * scale;
        let r = t::ACTION_BUTTON_CORNER_RADIUS_M as f64 * scale;
        let n = self.buttons.len();
        let mut cx = x; let mut h = 0.0f64;
        for (i, b) in self.buttons.iter_mut().enumerate() {
            let (w, bh) = b.measure(fonts, scale);
            b.rect = Rect::new(cx, y, cx + w, y + bh);
            b.corners = if !self.compact { Corners::all(r) } else {
                let (first, last) = (i == 0, i + 1 == n);
                Corners { tl: if first { r } else { 0.0 }, bl: if first { r } else { 0.0 }, tr: if last { r } else { 0.0 }, br: if last { r } else { 0.0 } }
            };
            cx += w + gap; h = h.max(bh);
        }
        let w = if n == 0 { 0.0 } else { cx - gap - x };
        self.rect = Rect::new(x, y, x + w, y + h);
        w
    }
    /// Route an event to every button; the responses that were not `None`, by index.
    pub fn event(&mut self, e: &Event, now: Instant) -> Vec<(usize, Response)> {
        self.buttons.iter_mut().enumerate().filter_map(|(i, b)| match b.event(e, now) { Response::None => None, r => Some((i, r)) }).collect()
    }
    pub fn hovered(&self) -> Option<usize> { self.buttons.iter().position(|b| b.hovered()) }
    pub fn animating(&self, now: Instant) -> bool { self.buttons.iter().any(|b| b.animating(now)) }
    pub fn paint(&self, scene: &mut Scene, fonts: &mut Fonts, scale: f64, now: Instant) {
        if self.compact && self.buttons.len() > 1 {
            // the fused base: one rounded bar in the idle hover colour at alpha 0 draws nothing, but a hairline seam between cells reads as one control
            let seam = t::DIVIDER_COLOR; let th = t::HAIRLINE as f64 * scale;
            for w in self.buttons.windows(2) { let x = w[1].rect.x0; crate::paint::rect(scene, Rect::new(x - th / 2.0, w[1].rect.y0 + th * 6.0, x + th / 2.0, w[1].rect.y1 - th * 6.0), seam); }
        }
        for b in &self.buttons { b.paint(scene, fonts, scale, now); }
    }
}
