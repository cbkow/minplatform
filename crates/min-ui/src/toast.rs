//! Toast — QCView's `Toast.qml` carried: the app's single outcome voice, ONE at a time (a new one replaces the current and
//! restarts its clock), top-right of the content area, non-modal (presses elsewhere keep reaching the app), a click on it
//! dismisses early. Kinds: positive (check), notice (warning), negative (x-circle). An optional action button ("Undo" on
//! the removal toast) — the toast then lingers 5.6 s instead of 3.4 s. Long text wraps at 360 px. Spectrum 2's look: the
//! popover surface + elevation, the status colour on the icon only (row 162 rations colour); a 120-ms fade in.
use crate::{anim::Ramp, input::{Button, Event}, paint, Family, Fonts};
use kurbo::{Affine, Rect, RoundedRect};
use min_theme::tokens as t;
use std::time::{Duration, Instant};
use vello::Scene;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToastKind { Positive, Notice, Negative }
struct Shown { text: String, kind: ToastKind, action: Option<(String, String)>, until: Instant }

pub struct Toast { shown: Option<Shown>, fade: Ramp, rect: Rect, action_rect: Rect, action_hover: bool }

const MAX_TEXT_W: f64 = 360.0;
impl Toast {
    pub fn new() -> Toast { Toast { shown: None, fade: Ramp::new(0.0, 120), rect: Rect::ZERO, action_rect: Rect::ZERO, action_hover: false } }
    pub fn show(&mut self, text: &str, kind: ToastKind, now: Instant) { self.shown = Some(Shown { text: text.to_string(), kind, action: None, until: now + Duration::from_millis(3400) }); self.fade = Ramp::new(0.0, 120); self.fade.set_target(1.0, now); }
    /// With one action button; `id` comes back from `event` when it is pressed.
    pub fn show_action(&mut self, text: &str, kind: ToastKind, label: &str, id: &str, now: Instant) { self.shown = Some(Shown { text: text.to_string(), kind, action: Some((label.to_string(), id.to_string())), until: now + Duration::from_millis(5600) }); self.fade = Ramp::new(0.0, 120); self.fade.set_target(1.0, now); }
    pub fn is_shown(&self) -> bool { self.shown.is_some() }
    pub fn text(&self) -> Option<&str> { self.shown.as_ref().map(|s| s.text.as_str()) }
    /// Expiry + the fade; true while the frame must be redrawn.
    pub fn tick(&mut self, now: Instant) -> bool { if self.shown.as_ref().is_some_and(|s| now >= s.until) { self.shown = None; return true } self.fade.animating(now) }
    /// Returns (the action id when its button was pressed, redraw, consumed). Only presses ON the toast are consumed.
    pub fn event(&mut self, e: &Event) -> (Option<String>, bool, bool) {
        if self.shown.is_none() { return (None, false, false) }
        let inside = e.pos().is_some_and(|(x, y)| self.rect.contains(kurbo::Point::new(x, y)));
        match *e {
            Event::Move { x, y } => { let h = self.action_rect.contains(kurbo::Point::new(x, y)); let d = h != self.action_hover; self.action_hover = h; (None, d, false) }
            Event::Down { x, y, button: Button::Primary } if inside => {
                let id = if self.action_rect.contains(kurbo::Point::new(x, y)) { self.shown.as_ref().and_then(|s| s.action.as_ref().map(|a| a.1.clone())) } else { None };
                self.shown = None; (id, true, true)
            }
            _ => (None, false, false),
        }
    }
    /// Lay out and draw in the top-right of `area` (the content rect).
    pub fn paint(&mut self, scene: &mut Scene, fonts: &mut Fonts, area: Rect, scale: f64, now: Instant) {
        let Some(s) = &self.shown else { self.rect = Rect::ZERO; return };
        let a = self.fade.value(now) as f32;
        let pad = t::SPACING_200 as f64 * scale; let gap = t::SPACING_100 as f64 * scale; let icon_sz = t::ICON_SIZE_SMALL * scale as f32;
        let mut text = fonts.layout(&s.text, Family::Sans, t::BODY_SIZE_S * scale as f32, t::FONT_WEIGHT_REGULAR as f32, paint::color(t::NEUTRAL_CONTENT_COLOR_DEFAULT));
        if text.width() as f64 > MAX_TEXT_W * scale { text.break_all_lines(Some((MAX_TEXT_W * scale) as f32)); }
        let action = s.action.as_ref().map(|(l, _)| fonts.layout(l, Family::Sans, t::BODY_SIZE_S * scale as f32, t::FONT_WEIGHT_BOLD as f32, paint::color(t::NEUTRAL_CONTENT_COLOR_DEFAULT)));
        let aw = action.as_ref().map(|l| l.width() as f64 + 2.0 * pad).unwrap_or(0.0);
        let w = pad + icon_sz as f64 + gap + text.width() as f64 + if action.is_some() { gap + aw } else { 0.0 } + pad;
        let h = (text.height() as f64).max(icon_sz as f64) + 2.0 * gap.max(t::SPACING_100 as f64 * scale) + 2.0 * scale;
        let h = h.max(t::COMPONENT_HEIGHT_100 as f64 * scale);
        let x1 = area.x1 - pad; let y0 = area.y0 + pad;
        self.rect = Rect::new(x1 - w, y0, x1, y0 + h);
        let r = t::POPOVER_CORNER_RADIUS as f64 * scale;
        let fade = |c: min_theme::Color| min_theme::Color { a: c.a * a, ..c };
        for sh in t::POPOVER_SHADOW { scene.draw_blurred_rounded_rect(Affine::translate((sh.x as f64 * scale, sh.y as f64 * scale)), self.rect, paint::color(fade(sh.color)), r, (sh.blur as f64 * scale) / 2.0); }
        paint::rounded(scene, self.rect, r, fade(t::POPOVER_BORDER_COLOR));
        let bw = t::POPOVER_BORDER_WIDTH as f64 * scale;
        scene.fill(peniko::Fill::NonZero, Affine::IDENTITY, paint::color(fade(t::POPOVER_BACKGROUND_COLOR)), None, &RoundedRect::from_rect(self.rect.inset(-bw), r - bw));
        let (icon, col) = match s.kind { ToastKind::Positive => ("check-circle", t::POSITIVE_VISUAL_COLOR), ToastKind::Notice => ("warning", t::NOTICE_VISUAL_COLOR), ToastKind::Negative => ("x-circle", t::NEGATIVE_VISUAL_COLOR) };
        if let Some(g) = fonts.icon(icon, icon_sz, paint::color(fade(col))) { crate::fonts::draw_layout(scene, &g, Affine::translate((self.rect.x0 + pad, self.rect.y0 + (h - g.height() as f64) / 2.0))); }
        let tx = self.rect.x0 + pad + icon_sz as f64 + gap;
        crate::fonts::draw_layout(scene, &text, Affine::translate((tx, self.rect.y0 + (h - text.height() as f64) / 2.0)));
        if let Some(l) = action {
            let bh = t::COMPONENT_HEIGHT_75 as f64 * scale; let bx = tx + text.width() as f64 + gap;
            self.action_rect = Rect::new(bx, self.rect.y0 + (h - bh) / 2.0, bx + aw, self.rect.y0 + (h + bh) / 2.0);
            paint::rounded(scene, self.action_rect, bh / 2.0, fade(if self.action_hover { t::GRAY_400 } else { t::GRAY_300 }));
            crate::fonts::draw_layout(scene, &l, Affine::translate((self.action_rect.x0 + pad, self.action_rect.y0 + (bh - l.height() as f64) / 2.0)));
        } else { self.action_rect = Rect::ZERO; }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn one_at_a_time_expires_on_its_clock_and_the_action_button_reports_its_id() {
        let mut fonts = Fonts::new(); let mut scene = Scene::new(); let now = Instant::now(); let area = Rect::new(0.0, 0.0, 1200.0, 800.0);
        let mut toast = Toast::new();
        toast.show("first", ToastKind::Notice, now); toast.show("second", ToastKind::Positive, now);
        assert_eq!(toast.text(), Some("second"), "a new toast replaces the current");
        assert!(!toast.tick(now + Duration::from_millis(3300)) || toast.is_shown()); toast.tick(now + Duration::from_millis(3500)); assert!(!toast.is_shown(), "gone after 3.4 s");
        toast.show_action("Removed a.mov", ToastKind::Notice, "Undo", "undo-media-delete", now);
        toast.tick(now + Duration::from_millis(4000)); assert!(toast.is_shown(), "an action toast lingers 5.6 s");
        toast.paint(&mut scene, &mut fonts, area, 1.0, now);
        assert!(toast.rect.x1 <= area.x1 && toast.rect.y0 >= area.y0, "top-right of the area");
        let (id, _, consumed) = toast.event(&Event::Down { x: 10.0, y: 700.0, button: Button::Primary });
        assert!(id.is_none() && !consumed && toast.is_shown(), "a press elsewhere goes on to the app");
        let c = toast.action_rect.center();
        let (id, _, consumed) = toast.event(&Event::Down { x: c.x, y: c.y, button: Button::Primary });
        assert_eq!(id.as_deref(), Some("undo-media-delete")); assert!(consumed && !toast.is_shown());
    }
}
