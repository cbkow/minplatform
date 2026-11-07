//! Dialog — Spectrum 2's dialog as a node: a scrim over the window, an elevated card (radius 16, 24-px padding) with a
//! heading, a body in the subdued colour and a right-aligned row of pill buttons; the last button is the default. Modal:
//! presses outside do nothing (the owner closes it on Escape via `cancel`). QCView's "Reset all preferences?" lives here.
use crate::{input::Event, paint, text_button::{TextButton, Variant}, Family, Fonts};
use kurbo::{Affine, Rect, RoundedRect};
use min_theme::tokens as t;
use std::time::Instant;
use vello::Scene;

pub struct Dialog { pub title: String, pub body: String, pub buttons: Vec<TextButton>, pub open: bool, pub rect: Rect }

impl Dialog {
    pub fn new(title: &str, body: &str, buttons: Vec<(&str, Variant)>) -> Dialog { Dialog { title: title.to_string(), body: body.to_string(), buttons: buttons.into_iter().map(|(l, v)| TextButton::new(l, v)).collect(), open: false, rect: Rect::ZERO } }
    pub fn show(&mut self) { self.open = true; }
    pub fn cancel(&mut self) { self.open = false; }
    /// Centre in `bounds`; the body wraps to the dialog's width.
    pub fn layout(&mut self, fonts: &mut Fonts, bounds: Rect, scale: f64) {
        let w = t::DIALOG_WIDTH_S as f64 * scale; let pad = t::DIALOG_PADDING as f64 * scale; let gap = t::SPACING_200 as f64 * scale;
        let title = fonts.layout(&self.title, Family::Sans, t::DIALOG_TITLE_SIZE * scale as f32, t::FONT_WEIGHT_BOLD as f32, peniko::Color::WHITE);
        let body = fonts.layout_width(&self.body, Family::Sans, t::DIALOG_BODY_SIZE * scale as f32, t::FONT_WEIGHT_REGULAR as f32, peniko::Color::WHITE, Some((w - 2.0 * pad) as f32));
        let bh = t::BUTTON_HEIGHT_M as f64 * scale;   // the dialog's buttons stay size M
        let h = pad + title.height() as f64 + gap + body.height() as f64 + gap * 1.5 + bh + pad;
        let x0 = bounds.x0 + (bounds.width() - w) / 2.0; let y0 = bounds.y0 + (bounds.height() - h) / 2.0;
        self.rect = Rect::new(x0, y0, x0 + w, y0 + h);
        let mut x = self.rect.x1 - pad; let by = self.rect.y1 - pad - bh; let bgap = t::DIALOG_BUTTON_GAP as f64 * scale;
        for b in self.buttons.iter_mut().rev() { let (bw, _) = b.measure(fonts, scale); b.rect = Rect::new(x - bw, by, x, by + bh); x -= bw + bgap; }
    }
    /// Returns (the index of the button clicked, redraw needed). Every event is consumed while open.
    pub fn event(&mut self, e: &Event, now: Instant) -> (Option<usize>, bool) {
        if !self.open { return (None, false) }
        let mut clicked = None; let mut dirty = false;
        for (i, b) in self.buttons.iter_mut().enumerate() { let (c, d) = b.event(e, now); if c { clicked = Some(i); } dirty |= d; }
        if clicked.is_some() { self.open = false; dirty = true; }
        (clicked, dirty)
    }
    pub fn animating(&self, now: Instant) -> bool { self.open && self.buttons.iter().any(|b| b.animating(now)) }
    pub fn paint(&self, scene: &mut Scene, fonts: &mut Fonts, scale: f64, bounds: Rect, now: Instant) {
        if !self.open { return }
        paint::rect(scene, bounds, t::DIALOG_SCRIM_COLOR);
        let r = t::DIALOG_CORNER_RADIUS as f64 * scale;
        for sh in t::POPOVER_SHADOW { scene.draw_blurred_rounded_rect(Affine::translate((sh.x as f64 * scale, sh.y as f64 * scale)), self.rect.inflate(sh.spread as f64 * scale, sh.spread as f64 * scale), paint::color(sh.color), r, (sh.blur as f64 * scale / 2.0).max(0.1)); }
        scene.fill(peniko::Fill::NonZero, Affine::IDENTITY, paint::color(t::DIALOG_BACKGROUND_COLOR), None, &RoundedRect::from_rect(self.rect, r));
        let pad = t::DIALOG_PADDING as f64 * scale; let gap = t::SPACING_200 as f64 * scale;
        let title = fonts.layout(&self.title, Family::Sans, t::DIALOG_TITLE_SIZE * scale as f32, t::FONT_WEIGHT_BOLD as f32, paint::color(t::NEUTRAL_CONTENT_COLOR_DEFAULT));
        crate::fonts::draw_layout(scene, &title, Affine::translate((self.rect.x0 + pad, self.rect.y0 + pad)));
        let body = fonts.layout_width(&self.body, Family::Sans, t::DIALOG_BODY_SIZE * scale as f32, t::FONT_WEIGHT_REGULAR as f32, paint::color(t::DIALOG_BODY_COLOR), Some((self.rect.width() - 2.0 * pad) as f32));
        crate::fonts::draw_layout(scene, &body, Affine::translate((self.rect.x0 + pad, self.rect.y0 + pad + title.height() as f64 + gap)));
        for b in &self.buttons { b.paint(scene, fonts, scale, now); }
    }
}
