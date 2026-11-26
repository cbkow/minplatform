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

pub struct Popover { pub title: String, pub rect: Rect, pub content: Rect, pub open: bool, pub placement: Placement,
    /// On glass (the Quick Look preview, canon row 616): the panel floats over a picture beside a translucent bar - the
    /// first layer's grey, nearly opaque so a list stays readable over a bright picture, and a faint white edge.
    pub glass: bool }

impl Popover {
    pub fn new(title: &str) -> Popover { Popover { title: title.to_string(), rect: Rect::ZERO, content: Rect::ZERO, open: false, placement: Placement::Above, glass: false } }
    pub fn hit(&self, x: f64, y: f64) -> bool { self.open && self.rect.contains(kurbo::Point::new(x, y)) }
    fn title_height(&self, fonts: &mut Fonts, scale: f64) -> f64 {
        if self.title.is_empty() { 0.0 } else { fonts.detail(&self.title, t::DETAIL_SIZE_S * scale as f32, peniko::Color::WHITE).height() as f64 + t::SPACING_75 as f64 * scale }
    }
    /// Open with a content area of `content_w × content_h` logical px, anchored to `anchor` (window px), inside `bounds`.
    pub fn open_at(&mut self, fonts: &mut Fonts, anchor: Rect, content_w: f64, content_h: f64, bounds: Rect, scale: f64) { self.open_at_prefer(fonts, anchor, content_w, content_h, bounds, scale, Placement::Above) }
    /// As `open_at`, preferring `prefer` (a picker's menu opens below its field) and falling back when there is no room.
    pub fn open_at_prefer(&mut self, fonts: &mut Fonts, anchor: Rect, content_w: f64, content_h: f64, bounds: Rect, scale: f64, prefer: Placement) {
        let pad = t::POPOVER_PADDING as f64 * scale; let off = t::POPOVER_OFFSET as f64 * scale;
        let th = self.title_height(fonts, scale);
        let (w, h) = (content_w * scale + 2.0 * pad, content_h * scale + th + 2.0 * pad);
        let room_above = anchor.y0 - off - h >= bounds.y0; let room_below = anchor.y1 + off + h <= bounds.y1;
        self.placement = match prefer { Placement::Above => if room_above || !room_below { Placement::Above } else { Placement::Below }, Placement::Below => if room_below || !room_above { Placement::Below } else { Placement::Above } };
        // Room on neither side (a long list in a small window - the Quick Look preview, 2026-11-26): the side with more of
        // it, and no taller than what is there. The owner of the content scrolls it (`Picker` does).
        let (above, below) = (anchor.y0 - off - bounds.y0 - pad, bounds.y1 - anchor.y1 - off - pad);
        let h = if room_above || room_below { h } else { self.placement = if above >= below { Placement::Above } else { Placement::Below }; h.min(above.max(below).max(th + 2.0 * pad)) };
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
        let (edge, fill) = if self.glass { (min_theme::Color::rgba(1.0, 1.0, 1.0, 0.12), t::BACKGROUND_LAYER_1_COLOR.with_alpha(0.94)) } else { (t::POPOVER_BORDER_COLOR, t::POPOVER_BACKGROUND_COLOR) };
        let bw = t::POPOVER_BORDER_WIDTH as f64 * scale;
        // the edge as a ring, the fill inside it: a translucent fill over a full plate of the edge's colour would take its tint
        scene.stroke(&kurbo::Stroke::new(bw), Affine::IDENTITY, paint::color(edge), None, &RoundedRect::from_rect(self.rect.inset(-bw / 2.0), r - bw / 2.0));
        scene.fill(peniko::Fill::NonZero, Affine::IDENTITY, paint::color(fill), None, &RoundedRect::from_rect(self.rect.inset(-bw), r - bw));
        if !self.title.is_empty() {
            let l = fonts.detail(&self.title, t::DETAIL_SIZE_S * scale as f32, paint::color(t::NEUTRAL_SUBDUED_CONTENT_COLOR_DEFAULT));
            let pad = t::POPOVER_PADDING as f64 * scale;
            crate::fonts::draw_layout(scene, &l, Affine::translate((self.rect.x0 + pad, self.rect.y0 + pad)));
        }
    }
}
