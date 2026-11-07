//! Popover — a floating node in the one scene (canon §16.4: popups are nodes; there is no native surface to beat):
//! Spectrum 2's elevated layer (gray-75 fill, 1-px gray-200 border, radius 8, the three-layer `drop-shadow-elevated`,
//! 8-px padding) with an optional detail-style title. Placed above its anchor, below when there is no room, clamped
//! into the window. Closing is the owner's call (Escape, a press outside, focus loss).
use crate::{paint, Fonts};
use kurbo::{Affine, Rect, RoundedRect};
use min_theme::tokens as t;
use vello::Scene;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Placement { Above, Below }

pub struct Popover { pub title: String, pub rect: Rect, pub content: Rect, pub open: bool, pub placement: Placement }

impl Popover {
    pub fn new(title: &str) -> Popover { Popover { title: title.to_string(), rect: Rect::ZERO, content: Rect::ZERO, open: false, placement: Placement::Above } }
    pub fn hit(&self, x: f64, y: f64) -> bool { self.open && self.rect.contains(kurbo::Point::new(x, y)) }
    fn title_height(&self, fonts: &mut Fonts, scale: f64) -> f64 {
        if self.title.is_empty() { 0.0 } else { fonts.detail(&self.title, t::DETAIL_SIZE_S * scale as f32, peniko::Color::WHITE).height() as f64 + t::SPACING_75 as f64 * scale }
    }
    /// Open with a content area of `content_w × content_h` logical px, anchored to `anchor` (window px), inside `bounds`.
    pub fn open_at(&mut self, fonts: &mut Fonts, anchor: Rect, content_w: f64, content_h: f64, bounds: Rect, scale: f64) {
        let pad = t::POPOVER_PADDING as f64 * scale; let off = t::POPOVER_OFFSET as f64 * scale;
        let th = self.title_height(fonts, scale);
        let (w, h) = (content_w * scale + 2.0 * pad, content_h * scale + th + 2.0 * pad);
        self.placement = if anchor.y0 - off - h >= bounds.y0 { Placement::Above } else { Placement::Below };
        let y0 = match self.placement { Placement::Above => anchor.y0 - off - h, Placement::Below => anchor.y1 + off };
        let x0 = anchor.x0.clamp(bounds.x0 + pad, (bounds.x1 - w - pad).max(bounds.x0));
        self.rect = Rect::new(x0, y0, x0 + w, y0 + h);
        self.content = Rect::new(x0 + pad, y0 + pad + th, x0 + w - pad, y0 + h - pad);
        self.open = true;
    }
    pub fn close(&mut self) { self.open = false; }
    pub fn paint(&self, scene: &mut Scene, fonts: &mut Fonts, scale: f64) {
        if !self.open { return }
        let r = t::POPOVER_CORNER_RADIUS as f64 * scale;
        for sh in t::POPOVER_SHADOW {
            // CSS blur radius ≈ 2 σ
            scene.draw_blurred_rounded_rect(Affine::translate((sh.x as f64 * scale, sh.y as f64 * scale)), self.rect.inflate(sh.spread as f64 * scale, sh.spread as f64 * scale), paint::color(sh.color), r, (sh.blur as f64 * scale / 2.0).max(0.1));
        }
        paint::rounded(scene, self.rect, r, t::POPOVER_BORDER_COLOR);
        let bw = t::POPOVER_BORDER_WIDTH as f64 * scale;
        scene.fill(peniko::Fill::NonZero, Affine::IDENTITY, paint::color(t::POPOVER_BACKGROUND_COLOR), None, &RoundedRect::from_rect(self.rect.inset(-bw), r - bw));
        if !self.title.is_empty() {
            let l = fonts.detail(&self.title, t::DETAIL_SIZE_S * scale as f32, paint::color(t::NEUTRAL_SUBDUED_CONTENT_COLOR_DEFAULT));
            let pad = t::POPOVER_PADDING as f64 * scale;
            crate::fonts::draw_layout(scene, &l, Affine::translate((self.rect.x0 + pad, self.rect.y0 + pad)));
        }
    }
}
