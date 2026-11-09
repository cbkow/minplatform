//! Rail — a side rail as a node (canon §16.3: "collapsible rails = animated width 0↔W with push layout"; the 2026-11-07
//! rails ruling = QCView's geometry verbatim): a logical width clamped to `rail-width-min..max`, persisted by the
//! owner; collapse animates the open fraction over `rail-collapse-duration` (QCView: 140 ms OutCubic → the family's
//! one slide duration, 160 ms, the written deviation); a 1-px splitter on the inner edge with a ±3-px hot zone that
//! drags the width (hover one step up in grey — QCView lights it accent; blue is rationed); when collapsed a narrow
//! tab at the window's edge with a caret reopens it (QCView's `ViewportOverlay` chevrons); the content scrolls with the
//! wheel and shows Spectrum 2's 3-px overlay scrollbar at the inner edge while hovered. The rail paints its backdrop,
//! clips its content (`push_clip` / `pop_clip`) and paints the splitter, tab and scrollbar; the panel paints the rest.
use crate::{anim::{mix, Ramp}, input::{Button, Event}, paint, Fonts, Tooltips};
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
    /// the overlay scrollbar: hovered (drawn thicker, S2's way) and a thumb drag (pointer y at grab, scroll at grab)
    sb_hover: bool, sb_drag: Option<(f64, f64)>, sb_ramp: Ramp,
    /// QCView's collapse arrow at the top inner corner of the rail (its header row); the reopen half is the edge tab
    collapse: Rect, collapse_hover: bool, collapse_ramp: Ramp,
    /// the arrow's tooltip (the shell's shortcut text goes in `shortcut`, e.g. "⌘1" / "Ctrl+1")
    pub tooltips: Tooltips, pub shortcut: String,
    /// the reopen tab is an overlay on the picture: shown while the pointer is in the window, faded away when it is not (the user 2026-11-07: "I quite like them as overlays on the viewport, but not always")
    present: Ramp,
    splitter_ramp: Ramp, tab_ramp: Ramp,
    scale: f64,
}

impl Rail {
    pub fn new(side: Side, width: f64, collapsed: bool) -> Rail {
        Rail { side, width: width.clamp(t::RAIL_WIDTH_MIN as f64, t::RAIL_WIDTH_MAX as f64), collapsed, open: Ramp::new(if collapsed { 0.0 } else { 1.0 }, t::RAIL_COLLAPSE_DURATION), scroll: 0.0, content_height: 0.0, rect: Rect::ZERO,
            splitter: Rect::ZERO, hot: Rect::ZERO, tab: Rect::ZERO, splitter_hover: false, drag: None, tab_hover: false, inside: false, last_scroll: None, sb_hover: false, sb_drag: None, sb_ramp: Ramp::new(0.0, t::ANIMATION_DURATION_HOVER), collapse: Rect::ZERO, collapse_hover: false, collapse_ramp: Ramp::new(0.0, t::ANIMATION_DURATION_HOVER), tooltips: Tooltips::default(), shortcut: String::new(), present: Ramp::new(0.0, t::ANIMATION_DURATION_SLIDE), splitter_ramp: Ramp::new(0.0, t::ANIMATION_DURATION_HOVER), tab_ramp: Ramp::new(0.0, t::ANIMATION_DURATION_HOVER), scale: 1.0 }
    }
    pub fn set_collapsed(&mut self, c: bool, now: Instant) { if self.collapsed != c { self.collapsed = c; self.open.set_target(if c { 0.0 } else { 1.0 }, now); } }
    pub fn toggle(&mut self, now: Instant) { let c = !self.collapsed; self.set_collapsed(c, now); }
    /// Snap the animation (a restored layout at launch).
    pub fn settle(&mut self) { self.open = Ramp::new(if self.collapsed { 0.0 } else { 1.0 }, t::RAIL_COLLAPSE_DURATION); }
    /// The width the rail occupies now, physical px.
    pub fn visible_width(&self, now: Instant, scale: f64) -> f64 { (self.width * scale * self.open.value(now) as f64).round() }
    pub fn is_dragging(&self) -> bool { self.drag.is_some() }
    /// The pointer rests in the splitter's hot zone (the shell shows the column-resize cursor).
    pub fn splitter_hovered(&self) -> bool { self.splitter_hover || self.drag.is_some() }
    pub fn scrollbar_hovered(&self) -> bool { self.sb_hover || self.sb_drag.is_some() }
    pub fn hovered(&self) -> bool { self.inside || self.splitter_hover || self.tab_hover }
    /// Lay the rail against the window edge `edge_x` between `y0` and `y1` (physical px).
    pub fn layout(&mut self, edge_x: f64, y0: f64, y1: f64, scale: f64, now: Instant) {
        self.scale = scale;
        let vw = self.visible_width(now, scale);
        let hz = t::SPLITTER_HOT_ZONE as f64 * scale; let th = t::HAIRLINE as f64 * scale;
        let sz = t::COMPONENT_HEIGHT_75 as f64 * scale; let tab_y = y0 + t::SPACING_75 as f64 * scale;   // the floating tab (the user 2026-11-08: "I like how the close button for this panel floats outside the panel. Can we do the same for the sidebar rails?"): one element that flips, open or closed
        let m = t::SPACING_75 as f64 * scale;   // a 4-px margin from the window's edge: a real button, clear of the chrome border (the user 2026-11-07)
        match self.side {
            Side::Left => { self.rect = Rect::new(edge_x, y0, edge_x + vw, y1); self.splitter = Rect::new(self.rect.x1 - th, y0, self.rect.x1, y1); self.hot = Rect::new(self.rect.x1 - hz, y0, self.rect.x1 + hz, y1); self.tab = Rect::new(edge_x + vw + m, tab_y, edge_x + vw + m + sz, tab_y + sz); }
            Side::Right => { self.rect = Rect::new(edge_x - vw, y0, edge_x, y1); self.splitter = Rect::new(self.rect.x0, y0, self.rect.x0 + th, y1); self.hot = Rect::new(self.rect.x0 - hz, y0, self.rect.x0 + hz, y1); self.tab = Rect::new(edge_x - vw - m - sz, tab_y, edge_x - vw - m, tab_y + sz); }
        }
        let max = (self.content_height - self.body_height()).max(0.0);
        self.scroll = self.scroll.clamp(0.0, max);
        // the collapse arrow: a 24-px square in the header strip, at the inner end
        let pad = t::RAIL_PADDING as f64 * scale; let sz = t::COMPONENT_HEIGHT_75 as f64 * scale; let hh = self.header_height();
        let cy = y0 + (hh - sz) / 2.0;
        let _ = (pad, sz, cy); self.collapse = Rect::ZERO;   // retired: the floating tab toggles both ways
    }
    /// The pointer is over one of the rail's own controls (the collapse arrow, the scrollbar) — the panel should not see the press.
    pub fn over_control(&self, x: f64, y: f64, now: Instant) -> bool { self.fully_open(now) && (self.collapse.contains(kurbo::Point::new(x, y)) || self.scrollbar_hit(x, y, now).is_some()) }
    /// The space above the scrolling body: the padding alone — the collapse arrow that once had a header strip floats outside
    /// the rail as its tab now (2026-11-08).
    pub fn header_height(&self) -> f64 { t::RAIL_PADDING as f64 * self.scale }
    /// Where the panel lays its content: below the header strip, inset by the padding, its top shifted by the scroll.
    pub fn content_rect(&self) -> Rect {
        let pad = t::RAIL_PADDING as f64 * self.scale;
        Rect::new(self.rect.x0 + pad, self.rect.y0 + self.header_height() - self.scroll, self.rect.x1 - pad, self.rect.y1 - pad)
    }
    /// The body's height (what the content can fill without scrolling).
    pub fn body_height(&self) -> f64 { (self.rect.height() - self.header_height() - t::RAIL_PADDING as f64 * self.scale).max(0.0) }
    /// The overlay scrollbar's track and thumb (physical px) when the content overflows; the thumb is drawn
    /// `scrollbar-thickness` wide, twice that while hovered or dragged, and hit in a 12-px band along the inner edge.
    pub fn scrollbar(&self, now: Instant) -> Option<(Rect, Rect)> {
        let max = (self.content_height - self.body_height()).max(0.0);
        if max <= 0.0 || self.rect.width() <= 0.0 { return None }
        let scale = self.scale;
        let th = t::SCROLLBAR_THICKNESS as f64 * scale * (1.0 + self.sb_ramp.value(now) as f64); let pad = t::SPACING_50 as f64 * scale;
        let body_top = self.rect.y0 + self.header_height();
        // the thumb sits just past the splitter's hot zone (one hairline clear of it) so the two never share a pixel column
        let inset = (t::SPLITTER_HOT_ZONE + t::HAIRLINE) as f64 * scale;
        let track_h = self.rect.y1 - body_top - 2.0 * pad;
        let bar_h = (track_h * self.body_height() / self.content_height).max(t::COMPONENT_HEIGHT_75 as f64 * scale).min(track_h);
        let y = body_top + pad + (track_h - bar_h) * (self.scroll / max);
        let x = match self.side { Side::Left => self.rect.x1 - inset - th, Side::Right => self.rect.x0 + inset };
        Some((Rect::new(x, body_top + pad, x + th, self.rect.y1 - pad), Rect::new(x, y, x + th, y + bar_h)))
    }
    fn scrollbar_hit(&self, x: f64, y: f64, now: Instant) -> Option<(Rect, Rect, bool)> {
        let (track, thumb) = self.scrollbar(now)?;
        // the hit band: 12 px around the thumb's column, stopping at the splitter's hot zone (the splitter wins its own 3 px)
        let band = t::SPACING_200 as f64 * self.scale; let hz = t::SPLITTER_HOT_ZONE as f64 * self.scale;
        let zone = match self.side { Side::Left => Rect::new(self.rect.x1 - hz - band, track.y0, self.rect.x1 - hz, track.y1), Side::Right => Rect::new(self.rect.x0 + hz, track.y0, self.rect.x0 + hz + band, track.y1) };
        if !zone.contains(kurbo::Point::new(x, y)) { return None }
        Some((track, thumb, y >= thumb.y0 && y <= thumb.y1))
    }
    fn fully_collapsed(&self, now: Instant) -> bool { self.collapsed && self.open.value(now) <= 0.001 }
    fn fully_open(&self, now: Instant) -> bool { !self.collapsed && self.open.value(now) >= 0.999 }
    pub fn hit(&self, x: f64, y: f64) -> bool { self.rect.width() > 0.0 && self.rect.contains(kurbo::Point::new(x, y)) }
    /// Route a pointer event. Inside the rail everything is consumed (the panel routes within `content_rect`); the
    /// splitter drag and the open tab are handled here.
    pub fn event(&mut self, e: &Event, now: Instant) -> RailResponse {
        let mut r = RailResponse::default();
        let was = (self.splitter_hover, self.tab_hover, self.inside, self.drag.is_some(), self.sb_hover, self.sb_drag.is_some(), self.collapse_hover);
        let pos = e.pos();
        // the scrollbar first: a thumb drag owns the pointer; a press on the track jumps there
        if let Some((y0, s0)) = self.sb_drag {
            match *e {
                Event::Move { y, .. } => { if let Some((track, thumb)) = self.scrollbar(now) { let max = (self.content_height - self.body_height()).max(0.0); let run = (track.height() - thumb.height()).max(1.0); let s = (s0 + (y - y0) * max / run).clamp(0.0, max); if s != self.scroll { self.scroll = s; self.last_scroll = Some(now); } } r.consumed = true; r.dirty = true; return r }
                Event::Up { button: Button::Primary, .. } => { self.sb_drag = None; r.consumed = true; r.dirty = true; return r }
                Event::Leave => { self.sb_drag = None; }
                _ => {}
            }
        }
        if let (Some((x, y)), true) = (pos, self.fully_open(now)) {
            let on_collapse = self.collapse.contains(kurbo::Point::new(x, y));
            match e {
                Event::Move { .. } => self.collapse_hover = on_collapse,
                Event::Down { button: Button::Primary, .. } if on_collapse => { self.toggle(now); self.collapse_hover = false; r.toggled = true; r.consumed = true; r.dirty = true; self.collapse_ramp.set_target(0.0, now); return r }
                Event::Leave => self.collapse_hover = false,
                _ => {}
            }
        } else if matches!(e, Event::Leave) { self.collapse_hover = false; }
        if let (Some((x, y)), false) = (pos, self.fully_collapsed(now)) {
            match (e, self.scrollbar_hit(x, y, now)) {
                (Event::Move { .. }, hit) => { self.sb_hover = hit.is_some(); }
                (Event::Down { button: Button::Primary, .. }, Some((track, thumb, on_thumb))) => {
                    if on_thumb { self.sb_drag = Some((y, self.scroll)); }
                    else { let max = (self.content_height - self.body_height()).max(0.0); let run = (track.height() - thumb.height()).max(1.0); let s = ((y - track.y0 - thumb.height() / 2.0) / run * max).clamp(0.0, max); self.scroll = s; self.last_scroll = Some(now); self.sb_drag = Some((y, s)); }
                    self.sb_ramp.set_target(1.0, now);
                    r.consumed = true; r.dirty = true; return r
                }
                _ => {}
            }
        } else if matches!(e, Event::Leave) { self.sb_hover = false; }
        let on_splitter = self.fully_open(now) && pos.is_some_and(|(x, y)| self.hot.contains(kurbo::Point::new(x, y)));
        let on_tab = self.present.value(now) > 0.5 && !self.open.animating(now) && pos.is_some_and(|(x, y)| self.tab.contains(kurbo::Point::new(x, y)));
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
        if !r.consumed && pos.is_some_and(|(x, y)| self.hit(x, y) || (self.present.value(now) > 0.5 && self.tab.contains(kurbo::Point::new(x, y)))) { r.consumed = true; }
        self.splitter_ramp.set_target(if self.splitter_hover || self.drag.is_some() { 1.0 } else { 0.0 }, now);
        self.tab_ramp.set_target(if self.tab_hover { 1.0 } else { 0.0 }, now);
        self.sb_ramp.set_target(if self.sb_hover || self.sb_drag.is_some() { 1.0 } else { 0.0 }, now);
        self.collapse_ramp.set_target(if self.collapse_hover { 1.0 } else { 0.0 }, now);
        // the arrow's tooltip: hide when open, show when collapsed
        if matches!(e, Event::Down { .. }) { self.tooltips.pressed(now); }
        // QCView's words (`LeftRail` / `RightRail.qml`: "Expand panel" / "Collapse panel"), translated at hover time
        let ctx = match self.side { Side::Left => "LeftRail", Side::Right => "RightRail" };
        let key = if self.shortcut.is_empty() { String::new() } else { format!(" ({})", self.shortcut) };
        let over = if self.tab_hover { Some((2u64, self.tab, format!("{}{key}", min_i18n::trc(ctx, if self.collapsed { "Expand panel" } else { "Collapse panel" })))) } else { None };
        if self.tooltips.hover(over.as_ref().map(|(k, r, s)| (*k, *r, s.as_str())), now) { r.dirty = true; }
        r.dirty = was != (self.splitter_hover, self.tab_hover, self.inside, self.drag.is_some(), self.sb_hover, self.sb_drag.is_some(), self.collapse_hover) || r.width_changed || r.toggled;
        r
    }
    /// Scroll the content by `dy` physical px (positive = down); true when it moved.
    pub fn wheel(&mut self, dy: f64, now: Instant) -> bool {
        let max = (self.content_height - self.body_height()).max(0.0);
        let s = (self.scroll + dy).clamp(0.0, max);
        self.last_scroll = Some(now);
        if s != self.scroll { self.scroll = s; true } else { false }
    }
    /// The pointer is over the picture or this rail (the shell decides; the window has focus): the tab shows; otherwise it
    /// fades away (the user 2026-11-08: so the timeline can be used without them in view).
    pub fn set_pointer_present(&mut self, on: bool, now: Instant) { if on { if self.present.target() < 0.5 || self.present.value(now) < 0.999 { self.present = Ramp::new(1.0, t::ANIMATION_DURATION_SLIDE); } } else { self.present.set_target(0.0, now); } }   // shows at once, fades away
    /// Per frame: the tooltip's delay / timeout; true when a redraw is needed now.
    pub fn tick(&mut self, now: Instant) -> bool { self.tooltips.tick(now) }
    pub fn paint_tooltip(&self, scene: &mut Scene, fonts: &mut Fonts, scale: f64, bounds: Rect, now: Instant) { self.tooltips.paint(scene, fonts, scale, bounds, now); }
    pub fn animating(&self, now: Instant) -> bool { self.open.animating(now) || self.splitter_ramp.animating(now) || self.tab_ramp.animating(now) || self.sb_ramp.animating(now) || self.collapse_ramp.animating(now) || self.present.animating(now) || self.last_scroll.is_some_and(|t0| now.duration_since(t0).as_millis() as u32 <= t::BANDS_HIDE_DELAY + t::ANIMATION_DURATION_HOVER) }
    pub fn paint_background(&self, scene: &mut Scene, _now: Instant) {
        if self.rect.width() > 0.0 { paint::rect(scene, self.rect, t::RAIL_BACKGROUND_COLOR); }
    }
    /// Clip to the rail for the panel's content.
    pub fn push_clip(&self, scene: &mut Scene) { let pad = t::RAIL_PADDING as f64 * self.scale; scene.push_clip_layer(peniko::Fill::NonZero, Affine::IDENTITY, &Rect::new(self.rect.x0, self.rect.y0 + self.header_height(), self.rect.x1, self.rect.y1 - pad)); }
    pub fn pop_clip(&self, scene: &mut Scene) { scene.pop_layer(); }
    /// The splitter, the open tab's caret and the overlay scrollbar, over the content.
    pub fn paint_foreground(&self, scene: &mut Scene, fonts: &mut Fonts, scale: f64, now: Instant) {
        if self.rect.width() > 0.0 {
            paint::rect(scene, self.splitter, mix(t::RAIL_SPLITTER_COLOR, t::RAIL_SPLITTER_COLOR_HOVER, self.splitter_ramp.value(now)));
            if let Some((_, thumb)) = self.scrollbar(now) {
                let recent = self.last_scroll.is_some_and(|t0| now.duration_since(t0).as_millis() as u32 <= t::BANDS_HIDE_DELAY);
                if self.inside || recent || self.sb_hover || self.sb_drag.is_some() {
                    let c = if self.sb_hover || self.sb_drag.is_some() { t::SCROLLBAR_COLOR_HOVER } else { t::SCROLLBAR_COLOR };
                    paint::rounded(scene, thumb, thumb.width() / 2.0, c);
                }
            }
        }
        // the floating tab, open or closed (it rides the rail's inner edge as the rail slides): a quiet caret over the picture,
        // brighter on hover, faded out while the pointer is elsewhere or parked
        {
            let p = self.present.value(now); if p <= 0.001 { return }
            let a = self.tab_ramp.value(now);
            let bg = mix(t::HOVER_BACKGROUND_COLOR_IDLE, t::HOVER_BACKGROUND_COLOR, a); let bg = bg.with_alpha(bg.a * p);
            if bg.a > 0.0 { paint::rounded(scene, self.tab, t::CORNER_RADIUS_SMALL_DEFAULT as f64 * scale, bg); }
            let c = mix(t::RAIL_OPEN_TAB_COLOR, t::NEUTRAL_CONTENT_COLOR_HOVER, a); let c = c.with_alpha(c.a * p);
            let slug = match (self.side, self.collapsed) { (Side::Left, true) | (Side::Right, false) => "caret-right", _ => "caret-left" };
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
        r.set_collapsed(true, now); r.settle(); r.layout(0.0, 0.0, 800.0, 2.0, now); r.set_pointer_present(true, now);
        assert_eq!(r.visible_width(now, 2.0), 0.0);
        let e = r.event(&Event::Down { x: 8.0, y: 20.0, button: Button::Primary }, now);
        assert!(e.toggled && !r.collapsed);
        // open, the same floating tab sits just past the rail's inner edge and hides it (2026-11-08: the header arrow retired)
        let mut l2 = Rail::new(Side::Left, 280.0, false); l2.settle(); l2.layout(0.0, 0.0, 800.0, 1.0, now); l2.set_pointer_present(true, now);
        let c = l2.tab.center();
        assert!(c.x > l2.rect.x1, "the tab floats outside the rail");
        let e = l2.event(&Event::Down { x: c.x, y: c.y, button: Button::Primary }, now);
        assert!(e.toggled && e.consumed && l2.collapsed, "the floating tab collapses the rail");
        let mut l3 = Rail::new(Side::Left, 280.0, false); l3.settle(); l3.layout(0.0, 0.0, 800.0, 1.0, now); l3.set_pointer_present(false, now);
        let c = l3.tab.center(); let _ = l3.event(&Event::Down { x: c.x, y: c.y, button: Button::Primary }, now + std::time::Duration::from_secs(1));
        assert!(!l3.collapsed, "a faded tab does not take presses");
        let mut rr = Rail::new(Side::Right, 320.0, false); rr.settle(); rr.layout(1600.0, 0.0, 800.0, 1.0, now);
        rr.event(&Event::Down { x: 1281.0, y: 10.0, button: Button::Primary }, now);
        rr.event(&Event::Move { x: 1181.0, y: 10.0 }, now);
        assert_eq!(rr.width, 420.0, "dragging a right rail's splitter left widens it");
        rr.content_height = 2000.0; rr.layout(1600.0, 0.0, 800.0, 1.0, now);
        assert!(rr.wheel(5000.0, now)); assert_eq!(rr.scroll, 2000.0 - rr.body_height(), "scroll clamps to the overflow of the body");
        // the scrollbar: a press on the track's top jumps there, then a drag down follows the pointer
        rr.wheel(-5000.0, now); rr.layout(1600.0, 0.0, 800.0, 1.0, now);
        let (track, thumb) = rr.scrollbar(now).unwrap();
        assert!(thumb.y0 == track.y0 && thumb.height() < track.height());
        let e = rr.event(&Event::Down { x: rr.rect.x0 + 4.0, y: track.y1 - 1.0, button: Button::Primary }, now);
        assert!(e.consumed && rr.scroll > 1000.0, "a press near the track's end jumps there: {}", rr.scroll);
        rr.event(&Event::Up { x: rr.rect.x0 + 4.0, y: track.y1 - 1.0, button: Button::Primary }, now);
        rr.event(&Event::Down { x: rr.rect.x0 + 4.0, y: rr.scrollbar(now).unwrap().1.center().y, button: Button::Primary }, now);
        let before = rr.scroll;
        rr.event(&Event::Move { x: rr.rect.x0 + 4.0, y: rr.scrollbar(now).unwrap().1.center().y - 100.0 }, now);
        assert!(rr.scroll < before, "dragging the thumb up scrolls up");
    }
}
