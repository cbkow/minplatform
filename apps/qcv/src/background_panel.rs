//! The Background section of the left rail (QCView's `LeftRail.qml` Background section: four tiles — Black, Dark Gray,
//! Dark Check, Light Check — the current one outlined; `display/backgroundMode`). Here the four are labelled action
//! buttons fused into one 2 × 2 block (QCView's two-column grid; the 2026-11-07 ruling's compact group — a single row
//! did not fit a 280-px rail), the current one selected in the family's deliberate grey. The fill itself is stage 0 of
//! the presenter (row 153).
use kurbo::Rect;
use min_theme::tokens as t;
use min_ui::{action_button::Corners, paint, ActionButton, Event, Fonts, Response, Section};
use std::time::Instant;
use vello::Scene;

pub const LABELS: [&str; 4] = ["Black", "Dark Gray", "Dark Check", "Light Check"];

pub struct BackgroundPanel { pub section: Section, buttons: Vec<ActionButton>, mode: u8, height: f64 }

impl BackgroundPanel {
    pub fn new() -> BackgroundPanel {
        let buttons = LABELS.iter().enumerate().map(|(i, l)| { let mut b = ActionButton::new("", &format!("{l} background")).size_s(); b.label = Some(l.to_string()); b.selected = i == 0; b }).collect();
        BackgroundPanel { section: Section::new("Background", true), buttons, mode: 0, height: 0.0 }
    }
    pub fn sync(&mut self, mode: u8) { self.mode = mode.min(3); for (i, b) in self.buttons.iter_mut().enumerate() { b.selected = i as u8 == self.mode; } }
    pub fn layout(&mut self, fonts: &mut Fonts, x0: f64, x1: f64, y: f64, scale: f64) -> f64 {
        let hh = Section::height(scale);
        self.section.rect = Rect::new(x0, y, x1, y + hh);
        if !self.section.expanded { self.height = hh; return hh }
        let pad = t::SPACING_100 as f64 * scale;
        let (x0, x1) = (x0 + pad, x1 - pad);
        let cw = (x1 - x0) / 2.0; let ch = t::ACTION_BUTTON_HEIGHT_S as f64 * scale; let r = t::ACTION_BUTTON_CORNER_RADIUS_S as f64 * scale;
        let top = y + hh + pad / 2.0;
        for (i, b) in self.buttons.iter_mut().enumerate() {
            let (col, row) = ((i % 2) as f64, (i / 2) as f64);
            b.rect = Rect::new(x0 + col * cw, top + row * ch, x0 + (col + 1.0) * cw, top + (row + 1.0) * ch);
            b.corners = Corners { tl: if i == 0 { r } else { 0.0 }, tr: if i == 1 { r } else { 0.0 }, bl: if i == 2 { r } else { 0.0 }, br: if i == 3 { r } else { 0.0 } };
        }
        self.height = hh + pad / 2.0 + 2.0 * ch + t::SPACING_200 as f64 * scale;
        self.height
    }
    /// Returns (the mode chosen, redraw needed).
    pub fn event(&mut self, e: &Event, now: Instant) -> (Option<u8>, bool) {
        let (toggled, mut dirty) = self.section.event(e, now);
        if toggled { dirty = true; }
        let mut chosen = None;
        if self.section.expanded { for (i, b) in self.buttons.iter_mut().enumerate() { match b.event(e, now) { Response::None => {} r => { dirty = true; if r == Response::Clicked && i as u8 != self.mode { chosen = Some(i as u8); } } } } }
        (chosen, dirty)
    }
    pub fn animating(&self, now: Instant) -> bool { self.section.animating(now) || self.buttons.iter().any(|b| b.animating(now)) }
    pub fn paint(&self, scene: &mut Scene, fonts: &mut Fonts, scale: f64, now: Instant, first: bool) {
        self.section.paint(scene, fonts, scale, now, first);
        if !self.section.expanded { return }
        // the fused block's seams (the compact group's hairlines), then the four cells
        let th = t::HAIRLINE as f64 * scale; let b = &self.buttons;
        let (l, r_, t_, b_) = (b[0].rect.x0, b[1].rect.x1, b[0].rect.y0, b[2].rect.y1); let mx = b[0].rect.x1; let my = b[0].rect.y1;
        paint::rect(scene, Rect::new(mx - th / 2.0, t_ + th * 6.0, mx + th / 2.0, b_ - th * 6.0), t::DIVIDER_COLOR);
        paint::rect(scene, Rect::new(l + th * 6.0, my - th / 2.0, r_ - th * 6.0, my + th / 2.0), t::DIVIDER_COLOR);
        for b in &self.buttons { b.paint(scene, fonts, scale, now); }
    }
}
