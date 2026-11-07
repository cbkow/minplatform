//! Tooltip — QCView's timing (600 ms delay, 5 s timeout, hides on press or leave) in Spectrum 2's clothes (the
//! 2026-11-07 ruling: the softer neutral-subdued gray-500 fill with white body-S text, radius 4, a 10 × 5 tip,
//! max width 160), drawn as a node in the one scene — no OS popup to beat.
use crate::{paint, Family, Fonts};
use kurbo::{Affine, BezPath, Rect};
use min_theme::tokens as t;
use std::time::Instant;
use vello::Scene;

#[derive(Clone, Debug)]
struct Hover { key: u64, anchor: Rect, text: String, since: Instant }

#[derive(Default)]
pub struct Tooltips { hover: Option<Hover>, shown: Option<Instant>, last_visible: bool }

impl Tooltips {
    /// Report what the pointer rests on each frame (`None` = nothing with a tooltip). Returns true when visibility changed.
    pub fn hover(&mut self, over: Option<(u64, Rect, &str)>, now: Instant) -> bool {
        match (over, &mut self.hover) {
            (None, _) => { self.hover = None; self.shown = None; }
            (Some((key, anchor, text)), Some(h)) if h.key == key => { h.anchor = anchor; if h.text != text { h.text = text.to_string(); } }
            (Some((key, anchor, text)), _) => { self.hover = Some(Hover { key, anchor, text: text.to_string(), since: now }); self.shown = None; }
        }
        self.tick(now)
    }
    /// A press hides the tip and keeps it hidden until the pointer moves to something else.
    pub fn pressed(&mut self, now: Instant) { if let Some(h) = &mut self.hover { h.since = now + std::time::Duration::from_secs(3600); } self.shown = None; self.last_visible = false; }
    /// Advance the delay / timeout; true when visibility changed.
    pub fn tick(&mut self, now: Instant) -> bool {
        if let Some(h) = &self.hover {
            if self.shown.is_none() && now >= h.since && now.duration_since(h.since).as_millis() as u32 >= t::TOOLTIP_DELAY { self.shown = Some(now); }
        }
        let v = self.visible(now);
        let changed = v != self.last_visible; self.last_visible = v; changed
    }
    pub fn visible(&self, now: Instant) -> bool { self.shown.is_some_and(|s| now.duration_since(s).as_millis() as u32 <= t::TOOLTIP_TIMEOUT) }
    /// When the next visibility change is due (the app schedules a redraw for it), if any.
    pub fn next_change(&self, now: Instant) -> Option<Instant> {
        let h = self.hover.as_ref()?;
        match self.shown { None => Some(h.since + std::time::Duration::from_millis(t::TOOLTIP_DELAY as u64)), Some(s) => { let end = s + std::time::Duration::from_millis(t::TOOLTIP_TIMEOUT as u64); (end > now).then_some(end) } }
    }
    /// Draw above the anchor (below when there is no room), centred and clamped into `bounds` (the window).
    pub fn paint(&self, scene: &mut Scene, fonts: &mut Fonts, scale: f64, bounds: Rect, now: Instant) {
        if !self.visible(now) { return }
        let Some(h) = &self.hover else { return };
        let pad_x = t::TOOLTIP_EDGE_TO_TEXT as f64 * scale; let pad_y = t::TOOLTIP_TOP_TO_TEXT as f64 * scale;
        let max_w = t::TOOLTIP_MAX_WIDTH as f64 * scale;
        let layout = fonts.layout_width(&h.text, Family::Sans, t::TOOLTIP_FONT_SIZE * scale as f32, t::FONT_WEIGHT_REGULAR as f32, paint::color(t::TOOLTIP_CONTENT_COLOR), Some((max_w - 2.0 * pad_x) as f32));
        let (w, hh) = (layout.width() as f64 + 2.0 * pad_x, layout.height() as f64 + 2.0 * pad_y);
        let (tip_w, tip_h) = (t::TOOLTIP_TIP_WIDTH as f64 * scale, t::TOOLTIP_TIP_HEIGHT as f64 * scale);
        let off = t::TOOLTIP_OFFSET as f64 * scale;
        let cx = (h.anchor.x0 + h.anchor.x1) / 2.0;
        let above = h.anchor.y0 - off - tip_h - hh >= bounds.y0;
        let y0 = if above { h.anchor.y0 - off - tip_h - hh } else { h.anchor.y1 + off + tip_h };
        let x0 = (cx - w / 2.0).clamp(bounds.x0 + pad_x, (bounds.x1 - w - pad_x).max(bounds.x0));
        let body = Rect::new(x0, y0, x0 + w, y0 + hh);
        paint::rounded(scene, body, t::TOOLTIP_CORNER_RADIUS as f64 * scale, t::TOOLTIP_BACKGROUND_COLOR);
        let mut tip = BezPath::new();
        let tx = cx.clamp(body.x0 + tip_w, body.x1 - tip_w);
        if above { tip.move_to((tx - tip_w / 2.0, body.y1)); tip.line_to((tx + tip_w / 2.0, body.y1)); tip.line_to((tx, body.y1 + tip_h)); }
        else { tip.move_to((tx - tip_w / 2.0, body.y0)); tip.line_to((tx + tip_w / 2.0, body.y0)); tip.line_to((tx, body.y0 - tip_h)); }
        tip.close_path();
        scene.fill(peniko::Fill::NonZero, Affine::IDENTITY, paint::color(t::TOOLTIP_BACKGROUND_COLOR), None, &tip);
        crate::fonts::draw_layout(scene, &layout, Affine::translate((body.x0 + pad_x, body.y0 + pad_y)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    #[test]
    fn delay_then_timeout_then_press_hides() {
        let t0 = Instant::now();
        let mut tt = Tooltips::default();
        let a = Rect::new(0.0, 0.0, 32.0, 32.0);
        assert!(!tt.hover(Some((1, a, "Play")), t0));
        assert!(!tt.visible(t0 + Duration::from_millis(t::TOOLTIP_DELAY as u64 - 1)));
        let t1 = t0 + Duration::from_millis(t::TOOLTIP_DELAY as u64);
        assert!(tt.tick(t1) && tt.visible(t1));
        assert!(tt.visible(t1 + Duration::from_millis(t::TOOLTIP_TIMEOUT as u64 - 1)));
        let t2 = t1 + Duration::from_millis(t::TOOLTIP_TIMEOUT as u64 + 1);
        assert!(tt.tick(t2) && !tt.visible(t2));
        // a new button restarts the delay; a press hides for good until the pointer moves on
        assert!(!tt.hover(Some((2, a, "Pause")), t2));
        let t3 = t2 + Duration::from_millis(t::TOOLTIP_DELAY as u64);
        assert!(tt.tick(t3) && tt.visible(t3));
        tt.pressed(t3); assert!(!tt.visible(t3));
        assert!(!tt.hover(Some((2, a, "Pause")), t3 + Duration::from_secs(2)) && !tt.visible(t3 + Duration::from_secs(2)));
        assert!(!tt.hover(None, t3) && !tt.visible(t3));
    }
}
