//! Menu — a context menu opened at the pointer (QCView's `ThemedMenu` + `ThemedMenuItem` + `MenuSeparator`): rows of
//! `menu-item-height`, disabled rows in the disabled content colour and inert, separators as a hairline in a spacing-100
//! band. Picks on release over the row the press began on (the picker's rule); any press outside closes and is consumed
//! (minNotes' flyout rule); Escape closes. Drawn last, over every panel.
use crate::{input::{Button, Event}, paint, Family, Fonts, Placement, Popover};
use kurbo::{Affine, Rect};
use min_theme::tokens as t;
use vello::Scene;

#[derive(Clone, Debug, PartialEq)]
pub enum MenuItem { Action { label: String, enabled: bool, /// a shortcut shown at the right edge (QCView's menus carry them)
    shortcut: String }, Separator }
impl MenuItem {
    pub fn action(label: &str, enabled: bool) -> MenuItem { MenuItem::Action { label: label.to_string(), enabled, shortcut: String::new() } }
    pub fn with_shortcut(label: &str, enabled: bool, shortcut: &str) -> MenuItem { MenuItem::Action { label: label.to_string(), enabled, shortcut: shortcut.to_string() } }
}

pub struct Menu { pub items: Vec<MenuItem>, pub popover: Popover, hover: Option<usize>, pressed: Option<usize>, scale: f64 }

impl Menu {
    pub fn new() -> Menu { Menu { items: vec![], popover: Popover::new(""), hover: None, pressed: None, scale: 1.0 } }
    pub fn is_open(&self) -> bool { self.popover.open }
    pub fn close(&mut self) { self.popover.close(); self.hover = None; self.pressed = None; }
    fn row_h(&self, i: usize) -> f64 { match self.items[i] { MenuItem::Separator => t::SPACING_100 as f64, _ => t::MENU_ITEM_HEIGHT as f64 } }
    /// Open `items` with the menu's top-left at the pointer (flipped above / clamped inside `bounds` when there is no room).
    pub fn open_at(&mut self, items: Vec<MenuItem>, fonts: &mut Fonts, x: f64, y: f64, bounds: Rect, scale: f64) {
        self.items = items; self.scale = scale; self.hover = None; self.pressed = None;
        let w = self.items.iter().map(|it| match it { MenuItem::Action { label, shortcut, .. } => row_width(fonts, label, shortcut, false, false), MenuItem::Separator => 0.0 }).fold(t::MENU_MIN_WIDTH as f64, f64::max);
        let h: f64 = (0..self.items.len()).map(|i| self.row_h(i)).sum();
        self.popover.open_at_prefer(fonts, Rect::new(x, y, x, y), w, h, bounds, scale, Placement::Below);
    }
    fn item_rect(&self, i: usize) -> Rect {
        let c = self.popover.content; let s = self.scale;
        let y0 = c.y0 + (0..i).map(|k| self.row_h(k)).sum::<f64>() * s;
        Rect::new(c.x0, y0, c.x1, y0 + self.row_h(i) * s)
    }
    fn item_at(&self, x: f64, y: f64) -> Option<usize> {
        if !self.popover.open || !self.popover.content.contains(kurbo::Point::new(x, y)) { return None }
        (0..self.items.len()).find(|&i| self.item_rect(i).contains(kurbo::Point::new(x, y))).filter(|&i| matches!(self.items[i], MenuItem::Action { enabled: true, .. }))
    }
    /// Returns (the chosen item's index, redraw needed, consumed). Inert while closed.
    pub fn event(&mut self, e: &Event) -> (Option<usize>, bool, bool) {
        if !self.popover.open { return (None, false, false) }
        let was = (self.hover, self.pressed, self.popover.open);
        let mut chosen = None;
        match *e {
            Event::Move { x, y } => self.hover = self.item_at(x, y),
            Event::Down { x, y, .. } => match self.item_at(x, y) { Some(i) => self.pressed = Some(i), None => if !self.popover.hit(x, y) { self.close(); } },
            Event::Up { x, y, button: Button::Primary } | Event::Up { x, y, button: Button::Secondary } => {
                if let (Some(p), Some(i)) = (self.pressed, self.item_at(x, y)) { if p == i { chosen = Some(i); self.close(); } }
                self.pressed = None;
            }
            Event::Leave => self.hover = None,
            _ => {}
        }
        (chosen, chosen.is_some() || was != (self.hover, self.pressed, self.popover.open), true)
    }
    pub fn paint(&self, scene: &mut Scene, fonts: &mut Fonts, scale: f64) {
        if !self.popover.open { return }
        self.popover.paint(scene, fonts, scale);
        for (i, it) in self.items.iter().enumerate() {
            let r = self.item_rect(i);
            match it {
                MenuItem::Separator => paint_separator(scene, r, scale),
                MenuItem::Action { label, enabled, shortcut } => paint_row(scene, fonts, r, &Row { label, shortcut, enabled: *enabled, hover: self.hover == Some(i), down: self.pressed == Some(i), checked: None, gutter: false, sub: false, underline: None }, scale),
            }
        }
    }
}

/// One row of a menu — the context menu's and the menu bar's popups share it (QCView's `ThemedMenuItem`): the rounded
/// hover / down fill, the label at the edge (after a check gutter when the menu has checkable rows), the shortcut right-
/// aligned in the disabled colour, a caret for a submenu, the mnemonic underlined while the keyboard drives the menu.
pub(crate) struct Row<'a> { pub label: &'a str, pub shortcut: &'a str, pub enabled: bool, pub hover: bool, pub down: bool, pub checked: Option<bool>, pub gutter: bool, pub sub: bool, pub underline: Option<usize> }

/// The check gutter (a checkable row's tick sits in it) and the caret column, logical px.
pub(crate) fn gutter_width() -> f64 { t::ICON_SIZE_SMALL as f64 + t::SPACING_100 as f64 }

/// A row's natural width in logical px.
pub(crate) fn row_width(fonts: &mut Fonts, label: &str, shortcut: &str, gutter: bool, sub: bool) -> f64 {
    let edge = t::MENU_ITEM_EDGE_TO_TEXT as f64;
    let lw = fonts.layout(label, Family::Sans, t::BODY_SIZE_M, t::FONT_WEIGHT_REGULAR as f32, peniko::Color::WHITE).width() as f64;
    let sw = if shortcut.is_empty() { 0.0 } else { fonts.layout(shortcut, Family::Sans, t::BODY_SIZE_M, t::FONT_WEIGHT_REGULAR as f32, peniko::Color::WHITE).width() as f64 + t::SPACING_400 as f64 };
    lw + sw + 2.0 * edge + if gutter { gutter_width() } else { 0.0 } + if sub { gutter_width() } else { 0.0 }
}

pub(crate) fn paint_separator(scene: &mut Scene, r: Rect, scale: f64) {
    let edge = t::MENU_ITEM_EDGE_TO_TEXT as f64 * scale; let y = ((r.y0 + r.y1) / 2.0).floor();
    paint::rect(scene, Rect::new(r.x0 + edge / 2.0, y, r.x1 - edge / 2.0, y + scale.max(1.0).floor()), t::DIVIDER_COLOR);
}

pub(crate) fn paint_row(scene: &mut Scene, fonts: &mut Fonts, r: Rect, row: &Row, scale: f64) {
    let edge = t::MENU_ITEM_EDGE_TO_TEXT as f64 * scale; let radius = t::PICKER_CORNER_RADIUS as f64 * scale;
    if row.enabled { if row.down { paint::rounded(scene, r, radius, t::MENU_ITEM_DOWN_COLOR); } else if row.hover { paint::rounded(scene, r, radius, t::MENU_ITEM_HOVER_COLOR); } }
    let c = paint::color(if row.enabled { t::MENU_ITEM_CONTENT_COLOR } else { t::DISABLED_CONTENT_COLOR });
    let size = t::BODY_SIZE_M * scale as f32; let icon = t::ICON_SIZE_SMALL * scale as f32;
    let mut x = r.x0 + edge;
    if row.gutter {
        if row.checked == Some(true) { if let Some(g) = fonts.icon("check", icon, c) { crate::fonts::draw_layout(scene, &g, Affine::translate((x, r.y0 + (r.height() - g.height() as f64) / 2.0))); } }
        x += gutter_width() * scale;
    }
    let l = fonts.layout(row.label, Family::Sans, size, t::FONT_WEIGHT_REGULAR as f32, c);
    let ty = r.y0 + (r.height() - l.height() as f64) / 2.0;
    crate::fonts::draw_layout(scene, &l, Affine::translate((x, ty)));
    if let Some(u) = row.underline { underline(scene, fonts, row.label, u, x, ty, &l, size, row.enabled, scale); }
    let mut x1 = r.x1 - edge;
    if row.sub {
        if let Some(g) = fonts.icon("caret-right", icon, c) { crate::fonts::draw_layout(scene, &g, Affine::translate((x1 - g.width() as f64, r.y0 + (r.height() - g.height() as f64) / 2.0))); }
        x1 -= gutter_width() * scale;
    }
    if !row.shortcut.is_empty() {
        let s = fonts.layout(row.shortcut, Family::Sans, size, t::FONT_WEIGHT_REGULAR as f32, paint::color(t::DISABLED_CONTENT_COLOR));
        crate::fonts::draw_layout(scene, &s, Affine::translate((x1 - s.width() as f64, r.y0 + (r.height() - s.height() as f64) / 2.0)));
    }
}

/// Underline the `i`-th char of `text` (laid out at `x, y` as `l`): Windows' keyboard-cue mnemonic.
pub(crate) fn underline(scene: &mut Scene, fonts: &mut Fonts, text: &str, i: usize, x: f64, y: f64, l: &crate::fonts::TextLayout, size: f32, enabled: bool, scale: f64) {
    let (Some((b0, ch)), Some(line)) = (text.char_indices().nth(i), l.lines().next()) else { return };
    let mut w = |s: &str| fonts.layout(s, Family::Sans, size, t::FONT_WEIGHT_REGULAR as f32, peniko::Color::WHITE).width() as f64;
    let x0 = x + w(&text[..b0]); let cw = w(&text[b0..b0 + ch.len_utf8()]);
    let base = y + line.metrics().baseline as f64 + scale.max(1.0);
    paint::rect(scene, Rect::new(x0, base.round(), x0 + cw, base.round() + scale.max(1.0).floor()), if enabled { t::MENU_ITEM_CONTENT_COLOR } else { t::DISABLED_CONTENT_COLOR });
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn picks_enabled_rows_on_release_skips_disabled_and_separators_and_closes_outside() {
        let mut fonts = Fonts::new(); let bounds = Rect::new(0.0, 0.0, 800.0, 600.0);
        let mut m = Menu::new();
        m.open_at(vec![MenuItem::action("Create Playlist from Selection", true), MenuItem::Separator, MenuItem::action("Input", false), MenuItem::Separator, MenuItem::action("Delete", true)], &mut fonts, 100.0, 100.0, bounds, 1.0);
        assert!(m.is_open() && m.popover.rect.y0 >= 100.0, "opens below the pointer");
        let mid = |m: &Menu, i: usize| { let r = m.item_rect(i); ((r.x0 + r.x1) / 2.0, (r.y0 + r.y1) / 2.0) };
        let (x, y) = mid(&m, 2);   // the disabled row is inert
        m.event(&Event::Down { x, y, button: Button::Primary }); let (c, _, _) = m.event(&Event::Up { x, y, button: Button::Primary });
        assert!(c.is_none() && m.is_open());
        let (x, y) = mid(&m, 4);
        m.event(&Event::Down { x, y, button: Button::Primary }); let (c, _, _) = m.event(&Event::Up { x, y, button: Button::Primary });
        assert_eq!(c, Some(4)); assert!(!m.is_open());
        m.open_at(vec![MenuItem::action("Delete", true)], &mut fonts, 790.0, 590.0, bounds, 1.0);
        assert!(m.popover.rect.x1 <= 800.0 && m.popover.rect.y1 <= 600.0, "clamped / flipped inside the window");
        let (_, _, consumed) = m.event(&Event::Down { x: 10.0, y: 10.0, button: Button::Primary });
        assert!(consumed && !m.is_open(), "a press outside closes and is consumed");
    }
}
