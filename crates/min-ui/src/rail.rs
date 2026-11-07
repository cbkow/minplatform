//! Rail — a side rail as a node (canon §16.3: "collapsible rails = animated width 0↔W with push layout"; the 2026-11-07
//! rails ruling = QCView's geometry verbatim): a logical width clamped to `rail-width-min..max`, persisted by the
//! owner; collapse animates the open fraction over `rail-collapse-duration` (QCView: 140 ms OutCubic → the family's
//! one slide duration, 160 ms, the written deviation); a 1-px splitter on the inner edge with a ±3-px hot zone that
//! drags the width (hover one step up in grey — QCView lights it accent; blue is rationed); when collapsed a narrow
//! tab at the window's edge with a caret reopens it (QCView's `ViewportOverlay` chevrons); the content scrolls with the
//! wheel and shows Spectrum 2's 3-px overlay scrollbar at the inner edge while hovered. The rail paints its backdrop,
//! clips its content (`push_clip` / `pop_clip`) and paints the splitter, tab and scrollbar; the panel paints the rest.
use crate::{anim::{mix, Ramp}, input::{Button, Event}, paint, Fonts};
use kurbo::{Affine, Rect};
use min_theme::tokens as t;
use std::time::Instant;
use vello::Scene;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side { Left, Right }

#[derive(Default, Debug, PartialEq)]
pub struct RailResponse { pub dirty: bool, pub consumed: bool, pub width_changed: bool, pub toggled: bool }

pub struct Rail {
    pub side: Side,
    /// logical px, clamped
    pub width: f64,
    pub collapsed: bool,
    open: Ramp,
    /// physical px the content is scrolled by
    pub scroll: f64,
    pub content_height: f64,
    pub rect: Rect,
    splitter: Rect, hot: Rect, tab: Rect,
    splitter_hover: bool, drag: Option<(f64, f64)>, tab_hover: bool, inside: bool, last_scroll: Option<Instant>,
    splitter_ramp: Ramp, tab_ramp: Ramp,
    scale: f64,
}

impl Rail {
    pub fn new(side: Side, width: f64, collapsed: bool) -> Rail {
        Rail { side, width: width.clamp(t::RAIL_WIDTH_MIN as f64, t::RAIL_WIDTH_MAX as f64), collapsed, open: Ramp::new(if collapsed { 0.0 } else { 1.0 }, t::RAIL_COLLAPSE_DURATION), scroll: 0.0, content_height: 0.0, rect: Rect::ZERO,
            splitter: Rect::ZERO, hot: Rect::ZERO, tab: Rect::ZERO, splitter_hover: false, drag: None, tab_hover: false, inside: false, last_scroll: None, splitter_ramp: Ramp::new(0.0, t::ANIMATION_DURATION_HOVER), tab_ramp: Ramp::new(0.0, t::ANIMATION_DURATION_HOVER), scale: 1.0 }
    }
    pub fn set_collapsed(&mut self, c: bool, now: Instant) { if self.collapsed != c { self.collapsed = c; self.open.set_target(if c { 0.0 } else { 1.0 }, now); } }
    pub fn toggle(&mut self, now: Instant) { let c = !self.collapsed; self.set_collapsed(c, now); }
    /// Snap the animation (a restored layout at launch).
    pub fn settle(&mut self) { self.open = Ramp::new(if self.collapsed { 0.0 } else { 1.0 }, t::RAIL_COLLAPSE_DURATION); }
    /// The width the rail occupies now, physical px.
    pub fn visible_width(&self, now: Instant, scale: f64) -> f64 { (self.width * scale * self.open.value(now) as f64).round() }
    pub fn is_dragging(&self) -> bool { self.drag.is_some() }
    pub fn hovered(&self) -> bool { self.inside || self.splitter_hover || self.tab_hover }
    /// Lay the rail against the window edge `edge_x` between `y0` and `y1` (physical px).
    pub fn layout(&mut self, edge_x: f64, y0: f64, y1: f64, scale: f64, now: Instant) {
        self.scale = scale;
        let vw = self.visible_width(now, scale);
        let hz = t::SPLITTER_HOT_ZONE as f64 * scale; let th = t::HAIRLINE as f64 * scale;
        let tw = t::RAIL_OPEN_TAB_WIDTH as f64 * scale; let tab_h = t::COMPONENT_HEIGHT_200 as f64 * scale;
        match self.side {
            Side::Left => { self.rect = Rect::new(edge_x, y0, edge_x + vw, y1); self.splitter = Rect::new(self.rect.x1 - th, y0, self.rect.x1, y1); self.hot = Rect::new(self.rect.x1 - hz, y0, self.rect.x1 + hz, y1); self.tab = Rect::new(edge_x, y0, edge_x + tw, y0 + tab_h); }
            Side::Right => { self.rect = Rect::new(edge_x - vw, y0, edge_x, y1); self.splitter = Rect::new(self.rect.x0, y0, self.rect.x0 + th, y1); self.hot = Rect::new(self.rect.x0 - hz, y0, self.rect.x0 + hz, y1); self.tab = Rect::new(edge_x - tw, y0, edge_x, y0 + tab_h); }
        }
        let max = (self.content_height - self.rect.height()).max(0.0);
        self.scroll = self.scroll.clamp(0.0, max);
    }
    /// Where the panel lays its content: the rail inset by the padding, its top shifted by the scroll.
    pub fn content_rect(&self) -> Rect {
        let pad = t::RAIL_PADDING as f64 * self.scale;
        Rect::new(self.rect.x0 + pad, self.rect.y0 + pad - self.scroll, self.rect.x1 - pad, self.rect.y1 - pad)
    }
    fn fully_collapsed(&self, now: Instant) -> bool { self.collapsed && self.open.value(now) <= 0.001 }
    fn fully_open(&self, now: Instant) -> bool { !self.collapsed && self.open.value(now) >= 0.999 }
    pub fn hit(&self, x: f64, y: f64) -> bool { self.rect.width() > 0.0 && self.rect.contains(kurbo::Point::new(x, y)) }
    /// Route a pointer event. Inside the rail everything is consumed (the panel routes within `content_rect`); the
    /// splitter drag and the open tab are handled here.
    pub fn event(&mut self, e: &Event, now: Instant) -> RailResponse {
        let mut r = RailResponse::default();
        let was = (self.splitter_hover, self.tab_hover, self.inside, self.drag.is_some());
        let pos = e.pos();
        let on_splitter = self.fully_open(now) && pos.is_some_and(|(x, y)| self.hot.contains(kurbo::Point::new(x, y)));
        let on_tab = self.fully_collapsed(now) && pos.is_some_and(|(x, y)| self.tab.contains(kurbo::Point::new(x, y)));
        match *e {
            Event::Move { x, .. } => {
                if let Some((x0, w0)) = self.drag {
                    let dx = (x - x0) / self.scale; let dx = if self.side == Side::Left { dx } else { -dx };
                    let w = (w0 + dx).clamp(t::RAIL_WIDTH_MIN as f64, t::RAIL_WIDTH_MAX as f64);
                    if w != self.width { self.width = w; r.width_changed = true; }
                    r.consumed = true;
                } else { self.splitter_hover = on_splitter; self.tab_hover = on_tab; self.inside = self.hit(x, e.pos().unwrap().1) && !on_splitter; }
            }
            Event::Down { x, button: Button::Primary, .. } => {
                if on_splitter { self.drag = Some((x, self.width)); r.consumed = true; }
                else if on_tab { self.toggle(now); r.toggled = true; r.consumed = true; }
            }
            Event::Down { .. } => {}
            Event::Up { button: Button::Primary, .. } => { if self.drag.take().is_some() { r.consumed = true; } }
            Event::Up { .. } => {}
            Event::Leave => { self.splitter_hover = false; self.tab_hover = false; self.inside = false; }
        }
        if !r.consumed && pos.is_some_and(|(x, y)| self.hit(x, y) || (self.fully_collapsed(now) && self.tab.contains(kurbo::Point::new(x, y)))) { r.consumed = true; }
        self.splitter_ramp.set_target(if self.splitter_hover || self.drag.is_some() { 1.0 } else { 0.0 }, now);
        self.tab_ramp.set_target(if self.tab_hover { 1.0 } else { 0.0 }, now);
        r.dirty = was != (self.splitter_hover, self.tab_hover, self.inside, self.drag.is_some()) || r.width_changed || r.toggled;
        r
    }
    /// Scroll the content by `dy` physical px (positive = down); true when it moved.
    pub fn wheel(&mut self, dy: f64, now: Instant) -> bool {
        let max = (self.content_height - self.rect.height()).max(0.0);
        let s = (self.scroll + dy).clamp(0.0, max);
        self.last_scroll = Some(now);
        if s != self.scroll { self.scroll = s; true } else { false }
    }
    pub fn animating(&self, now: Instant) -> bool { self.open.animating(now) || self.splitter_ramp.animating(now) || self.tab_ramp.animating(now) || self.last_scroll.is_some_and(|t0| now.duration_since(t0).as_millis() as u32 <= t::BANDS_HIDE_DELAY + t::ANIMATION_DURATION_HOVER) }
    pub fn paint_background(&self, scene: &mut Scene, now: Instant) {
        if self.rect.width() > 0.0 { paint::rect(scene, self.rect, t::RAIL_BACKGROUND_COLOR); }
        if self.fully_collapsed(now) {
            // the open tab: a quiet caret at the window's edge, brighter on hover
            let a = self.tab_ramp.value(now);
            let bg = mix(t::HOVER_BACKGROUND_COLOR_IDLE, t::HOVER_BACKGROUND_COLOR, a);
            if bg.a > 0.0 { paint::rect(scene, self.tab, bg); }
        }
    }
    /// Clip to the rail for the panel's content.
    pub fn push_clip(&self, scene: &mut Scene) { let pad = t::RAIL_PADDING as f64 * self.scale; scene.push_clip_layer(peniko::Fill::NonZero, Affine::IDENTITY, &Rect::new(self.rect.x0, self.rect.y0 + pad, self.rect.x1, self.rect.y1 - pad)); }
    pub fn pop_clip(&self, scene: &mut Scene) { scene.pop_layer(); }
    /// The splitter, the open tab's caret and the overlay scrollbar, over the content.
    pub fn paint_foreground(&self, scene: &mut Scene, fonts: &mut Fonts, scale: f64, now: Instant) {
        if self.rect.width() > 0.0 {
            paint::rect(scene, self.splitter, mix(t::RAIL_SPLITTER_COLOR, t::RAIL_SPLITTER_COLOR_HOVER, self.splitter_ramp.value(now)));
            let max = (self.content_height - self.rect.height()).max(0.0);
            if max > 0.0 {
                let recent = self.last_scroll.is_some_and(|t0| now.duration_since(t0).as_millis() as u32 <= t::BANDS_HIDE_DELAY);
                let a = if self.inside || recent { 1.0 } else { 0.0 };
                if a > 0.0 {
                    let th = t::SCROLLBAR_THICKNESS as f64 * scale; let pad = t::SPACING_50 as f64 * scale;
                    let track_h = self.rect.height() - 2.0 * pad;
                    let bar_h = (track_h * self.rect.height() / self.content_height).max(t::COMPONENT_HEIGHT_75 as f64 * scale);
                    let y = self.rect.y0 + pad + (track_h - bar_h) * (self.scroll / max);
                    let x = match self.side { Side::Left => self.rect.x1 - th - pad - t::HAIRLINE as f64 * scale, Side::Right => self.rect.x0 + pad + t::HAIRLINE as f64 * scale };
                    paint::rounded(scene, Rect::new(x, y, x + th, y + bar_h), th / 2.0, t::SCROLLBAR_COLOR);
                }
            }
        }
        if self.fully_collapsed(now) {
            let c = mix(t::RAIL_OPEN_TAB_COLOR, t::NEUTRAL_CONTENT_COLOR_HOVER, self.tab_ramp.value(now));
            let slug = match self.side { Side::Left => "caret-right", Side::Right => "caret-left" };
            if let Some(g) = fonts.icon(slug, t::ICON_SIZE_SMALL * scale as f32, paint::color(c)) { crate::fonts::draw_layout(scene, &g, Affine::translate((self.tab.x0 + (self.tab.width() - g.width() as f64) / 2.0, self.tab.y0 + (self.tab.height() - g.height() as f64) / 2.0))); }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn splitter_drag_clamps_and_the_tab_reopens() {
        let now = Instant::now();
        let mut r = Rail::new(Side::Left, 280.0, false); r.settle();
        r.layout(0.0, 0.0, 800.0, 2.0, now);
        assert_eq!(r.rect, Rect::new(0.0, 0.0, 560.0, 800.0));
        let e = r.event(&Event::Down { x: 561.0, y: 400.0, button: Button::Primary }, now);
        assert!(e.consumed && r.is_dragging());
        let e = r.event(&Event::Move { x: 1561.0, y: 400.0 }, now);
        assert!(e.width_changed && r.width == 600.0, "clamped at the max");
        r.event(&Event::Up { x: 1561.0, y: 400.0, button: Button::Primary }, now);
        assert!(!r.is_dragging());
        r.set_collapsed(true, now); r.settle(); r.layout(0.0, 0.0, 800.0, 2.0, now);
        assert_eq!(r.visible_width(now, 2.0), 0.0);
        let e = r.event(&Event::Down { x: 8.0, y: 20.0, button: Button::Primary }, now);
        assert!(e.toggled && !r.collapsed);
        let mut rr = Rail::new(Side::Right, 320.0, false); rr.settle(); rr.layout(1600.0, 0.0, 800.0, 1.0, now);
        rr.event(&Event::Down { x: 1281.0, y: 10.0, button: Button::Primary }, now);
        rr.event(&Event::Move { x: 1181.0, y: 10.0 }, now);
        assert_eq!(rr.width, 420.0, "dragging a right rail's splitter left widens it");
        rr.content_height = 2000.0; rr.layout(1600.0, 0.0, 800.0, 1.0, now);
        assert!(rr.wheel(5000.0, now)); assert_eq!(rr.scroll, 1200.0, "scroll clamps to the overflow");
    }
}
