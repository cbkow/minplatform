//! Picker — Spectrum 2's picker in the rail's QUIET form (canon §25.4: "text plus a chevron, no border"): the chosen
//! option in body 14 with a caret at its right, a hover fill at radius 4; its menu is a [`Popover`] of
//! `menu-item-height` rows opening BELOW the field, the chosen row ticked, hover gray-200, press gray-300. QCView's
//! `FlatComboBox` is a bordered 26-px combo; the quiet form is the written deviation (the reference image's pickers).
use crate::{anim::{mix, Ramp}, input::{Button, Event}, paint, Family, Fonts, Placement, Popover};
use kurbo::{Affine, Rect};
use min_theme::tokens as t;
use std::time::Instant;
use vello::Scene;

pub struct Picker {
    pub options: Vec<String>, pub selected: usize, pub enabled: bool, pub rect: Rect,
    pub menu: Popover, hovered: bool, hover: Ramp, hover_item: Option<usize>, pressed_item: Option<usize>, scale: f64,
}

impl Picker {
    pub fn new(options: Vec<String>, selected: usize) -> Picker { Picker { selected: selected.min(options.len().saturating_sub(1)), options, enabled: true, rect: Rect::ZERO, menu: Popover::new(""), hovered: false, hover: Ramp::new(0.0, t::ANIMATION_DURATION_HOVER), hover_item: None, pressed_item: None, scale: 1.0 } }
    pub fn hovered(&self) -> bool { self.hovered }
    pub fn is_open(&self) -> bool { self.menu.open }
    pub fn hit(&self, x: f64, y: f64) -> bool { self.rect.contains(kurbo::Point::new(x, y)) }
    pub fn value(&self) -> &str { self.options.get(self.selected).map(String::as_str).unwrap_or("") }
    /// Pick by option text (a settings value); unknown leaves the selection.
    pub fn select_text(&mut self, text: &str) { if let Some(i) = self.options.iter().position(|o| o == text) { self.selected = i; } }
    /// The natural size at `scale`: the widest option plus the caret and the edge insets, height S (24).
    pub fn measure(&self, fonts: &mut Fonts, scale: f64) -> (f64, f64) {
        let w = self.options.iter().map(|o| fonts.layout(o, Family::Sans, t::BODY_SIZE_M * scale as f32, t::FONT_WEIGHT_REGULAR as f32, peniko::Color::WHITE).width() as f64).fold(0.0, f64::max);
        (w + t::PICKER_CHEVRON_SIZE as f64 * scale + t::SPACING_75 as f64 * scale + 2.0 * t::PICKER_EDGE_TO_TEXT as f64 * scale, t::FIELD_HEIGHT_S as f64 * scale)
    }
    fn item_h(&self) -> f64 { t::MENU_ITEM_HEIGHT as f64 * self.scale }
    fn item_at(&self, x: f64, y: f64) -> Option<usize> {
        if !self.menu.open || !self.menu.content.contains(kurbo::Point::new(x, y)) { return None }
        let i = ((y - self.menu.content.y0) / self.item_h()).floor() as usize;
        (i < self.options.len()).then_some(i)
    }
    pub fn close(&mut self) { self.menu.close(); self.hover_item = None; self.pressed_item = None; }
    /// Returns (the newly chosen index, redraw needed, consumed). While the menu is open every press is consumed: a
    /// press outside closes it and goes no further (minNotes' flyout rule).
    pub fn event(&mut self, e: &Event, now: Instant, fonts: &mut Fonts, bounds: Rect, scale: f64) -> (Option<usize>, bool, bool) {
        self.scale = scale;
        let was = (self.hovered, self.hover_item, self.pressed_item, self.menu.open);
        let mut chosen = None; let mut consumed = false;
        if self.menu.open {
            consumed = true;
            match *e {
                Event::Move { x, y } => self.hover_item = self.item_at(x, y),
                Event::Down { x, y, button: Button::Primary } => { match self.item_at(x, y) { Some(i) => self.pressed_item = Some(i), None => { if !self.hit(x, y) { self.close(); } else { self.close(); } } } }
                Event::Up { x, y, button: Button::Primary } => { if let (Some(p), Some(i)) = (self.pressed_item, self.item_at(x, y)) { if p == i { if i != self.selected { self.selected = i; chosen = Some(i); } self.close(); } } self.pressed_item = None; }
                Event::Leave => { self.hover_item = None; }
                _ => {}
            }
            self.hovered = false;
        } else {
            let inside = e.pos().is_some_and(|(x, y)| self.hit(x, y));
            match *e {
                Event::Move { .. } => self.hovered = inside && self.enabled,
                Event::Down { button: Button::Primary, .. } if inside && self.enabled => {
                    let n = self.options.len() as f64;
                    let (mw, _) = self.measure(fonts, scale);
                    let w = (mw / scale).max(t::MENU_MIN_WIDTH as f64).max(self.rect.width() / scale) + t::SPACING_300 as f64;   // room for the tick
                    self.menu.open_at_prefer(fonts, self.rect, w, n * t::MENU_ITEM_HEIGHT as f64, bounds, scale, Placement::Below);
                    consumed = true;
                }
                Event::Leave => self.hovered = false,
                _ => {}
            }
        }
        self.hover.set_target(if self.hovered { 1.0 } else { 0.0 }, now);
        (chosen, was != (self.hovered, self.hover_item, self.pressed_item, self.menu.open) || chosen.is_some(), consumed)
    }
    pub fn animating(&self, now: Instant) -> bool { self.hover.animating(now) }
    pub fn paint(&self, scene: &mut Scene, fonts: &mut Fonts, scale: f64, now: Instant) {
        let hv = self.hover.value(now);
        // a resting fill a step lighter than the rail's tiles (the walk 2026-11-08: dropdowns read as controls), the hover grey over it
        let bg = if !self.enabled { t::PICKER_BACKGROUND_COLOR.with_alpha(0.5) } else if self.menu.open { t::DOWN_BACKGROUND_COLOR } else { mix(t::PICKER_BACKGROUND_COLOR, t::HOVER_BACKGROUND_COLOR, hv) };
        paint::rounded(scene, self.rect, t::PICKER_CORNER_RADIUS as f64 * scale, bg);
        let c = if !self.enabled { t::DISABLED_CONTENT_COLOR } else if self.hovered || self.menu.open { t::PICKER_CONTENT_COLOR_HOVER } else { t::PICKER_CONTENT_COLOR };
        let edge = t::PICKER_EDGE_TO_TEXT as f64 * scale;
        let sz = t::PICKER_CHEVRON_SIZE * scale as f32;
        let l = fonts.layout_fit(self.value(), Family::Sans, t::BODY_SIZE_M * scale as f32, t::FONT_WEIGHT_REGULAR as f32, paint::color(c), self.rect.width() - 2.0 * edge - sz as f64 - t::SPACING_100 as f64 * scale);
        crate::fonts::draw_layout(scene, &l, Affine::translate((self.rect.x0 + edge, self.rect.y0 + (self.rect.height() - l.height() as f64) / 2.0)));
        if let Some(g) = fonts.icon("caret-down", sz, paint::color(c)) { crate::fonts::draw_layout(scene, &g, Affine::translate((self.rect.x1 - edge - g.width() as f64, self.rect.y0 + (self.rect.height() - g.height() as f64) / 2.0))); }
    }
    /// The open menu, drawn last so it sits over the panel.
    pub fn paint_menu(&self, scene: &mut Scene, fonts: &mut Fonts, scale: f64) {
        if !self.menu.open { return }
        self.menu.paint(scene, fonts, scale);
        let ih = self.item_h(); let c = self.menu.content; let edge = t::MENU_ITEM_EDGE_TO_TEXT as f64 * scale;
        for (i, o) in self.options.iter().enumerate() {
            let r = Rect::new(c.x0, c.y0 + i as f64 * ih, c.x1, c.y0 + (i + 1) as f64 * ih);
            if self.pressed_item == Some(i) { paint::rounded(scene, r, t::PICKER_CORNER_RADIUS as f64 * scale, t::MENU_ITEM_DOWN_COLOR); }
            else if self.hover_item == Some(i) { paint::rounded(scene, r, t::PICKER_CORNER_RADIUS as f64 * scale, t::MENU_ITEM_HOVER_COLOR); }
            let l = fonts.layout(o, Family::Sans, t::BODY_SIZE_M * scale as f32, t::FONT_WEIGHT_REGULAR as f32, paint::color(t::MENU_ITEM_CONTENT_COLOR));
            crate::fonts::draw_layout(scene, &l, Affine::translate((r.x0 + edge, r.y0 + (ih - l.height() as f64) / 2.0)));
            if i == self.selected { if let Some(g) = fonts.icon("check", t::ICON_SIZE_SMALL * scale as f32, paint::color(t::MENU_ITEM_CONTENT_COLOR)) { crate::fonts::draw_layout(scene, &g, Affine::translate((r.x1 - edge - g.width() as f64, r.y0 + (ih - g.height() as f64) / 2.0))); } }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn opens_below_picks_on_release_and_a_press_outside_closes() {
        let now = Instant::now(); let mut fonts = Fonts::new();
        let bounds = Rect::new(0.0, 0.0, 800.0, 600.0);
        let mut p = Picker::new(vec!["PNG".into(), "JPEG".into(), "TIFF".into()], 0);
        p.rect = Rect::new(100.0, 100.0, 180.0, 124.0);
        let (_, _, c) = p.event(&Event::Down { x: 120.0, y: 110.0, button: Button::Primary }, now, &mut fonts, bounds, 1.0);
        assert!(c && p.is_open() && p.menu.placement == Placement::Below && p.menu.rect.y0 > 124.0);
        let y = p.menu.content.y0 + 1.5 * t::MENU_ITEM_HEIGHT as f64; let x = p.menu.content.x0 + 10.0;
        p.event(&Event::Move { x, y }, now, &mut fonts, bounds, 1.0);
        p.event(&Event::Down { x, y, button: Button::Primary }, now, &mut fonts, bounds, 1.0);
        let (chosen, _, _) = p.event(&Event::Up { x, y, button: Button::Primary }, now, &mut fonts, bounds, 1.0);
        assert_eq!(chosen, Some(1)); assert_eq!(p.value(), "JPEG"); assert!(!p.is_open());
        p.event(&Event::Down { x: 120.0, y: 110.0, button: Button::Primary }, now, &mut fonts, bounds, 1.0);
        let (chosen, _, c) = p.event(&Event::Down { x: 700.0, y: 500.0, button: Button::Primary }, now, &mut fonts, bounds, 1.0);
        assert!(chosen.is_none() && c && !p.is_open(), "a press outside closes and is consumed");
    }
}
