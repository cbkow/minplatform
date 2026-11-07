//! The Background section of the left rail (QCView's `LeftRail.qml` Background section: four tiles — Black, Dark Gray,
//! Dark Check, Light Check — the current one outlined; `display/backgroundMode`). Here the four are labelled action
//! buttons in one compact group (the 2026-11-07 rails ruling: "four chips in a compact group, radius 4"), the current
//! one selected in the family's deliberate grey. The fill itself is stage 0 of the presenter (row 153).
use kurbo::Rect;
use min_theme::tokens as t;
use min_ui::{ActionButton, ActionGroup, Event, Fonts, Response, Section};
use std::time::Instant;
use vello::Scene;

pub const LABELS: [&str; 4] = ["Black", "Dark Gray", "Dark Check", "Light Check"];

pub struct BackgroundPanel { pub section: Section, group: ActionGroup, mode: u8, height: f64 }

impl BackgroundPanel {
    pub fn new() -> BackgroundPanel {
        let buttons = LABELS.iter().enumerate().map(|(i, l)| { let mut b = ActionButton::new("", &format!("{l} background")); b.label = Some(l.to_string()); b.selected = i == 0; b }).collect();
        BackgroundPanel { section: Section::new("Background", true), group: ActionGroup::new(buttons).compact(), mode: 0, height: 0.0 }
    }
    pub fn sync(&mut self, mode: u8) { self.mode = mode.min(3); for (i, b) in self.group.buttons.iter_mut().enumerate() { b.selected = i as u8 == self.mode; } }
    pub fn layout(&mut self, fonts: &mut Fonts, x0: f64, x1: f64, y: f64, scale: f64) -> f64 {
        let hh = Section::height(scale);
        self.section.rect = Rect::new(x0, y, x1, y + hh);
        if !self.section.expanded { self.height = hh; return hh }
        let pad = t::SPACING_100 as f64 * scale;
        let gw = self.group.layout(fonts, 0.0, 0.0, scale);
        // centred in the rail when it fits, else left-aligned from the padding (the labels are what they are)
        let x = if gw <= x1 - x0 - 2.0 * pad { x0 + ((x1 - x0) - gw) / 2.0 } else { x0 + pad };
        self.group.layout(fonts, x, y + hh + pad / 2.0, scale);
        self.height = hh + pad / 2.0 + self.group.rect.height() + t::SPACING_200 as f64 * scale;
        self.height
    }
    /// Returns (the mode chosen, redraw needed).
    pub fn event(&mut self, e: &Event, now: Instant) -> (Option<u8>, bool) {
        let (toggled, mut dirty) = self.section.event(e, now);
        if toggled { dirty = true; }
        let mut chosen = None;
        if self.section.expanded { for (i, r) in self.group.event(e, now) { dirty = true; if r == Response::Clicked && i as u8 != self.mode { chosen = Some(i as u8); } } }
        (chosen, dirty)
    }
    pub fn animating(&self, now: Instant) -> bool { self.section.animating(now) || self.group.animating(now) }
    pub fn paint(&self, scene: &mut Scene, fonts: &mut Fonts, scale: f64, now: Instant, first: bool) {
        self.section.paint(scene, fonts, scale, now, first);
        if self.section.expanded { self.group.paint(scene, fonts, scale, now); }
    }
}
