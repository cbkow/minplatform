//! Small drawing helpers over vello, in the theme's terms.
use kurbo::{Affine, Rect, RoundedRect};
use peniko::{Color, Fill};
use vello::Scene;

/// A theme colour as vello's (sRGB-encoded, straight alpha — the same convention).
pub fn color(c: min_theme::Color) -> Color { Color::new([c.r, c.g, c.b, c.a]) }

/// A filled rectangle in physical pixels.
pub fn rect(scene: &mut Scene, r: Rect, c: min_theme::Color) { scene.fill(Fill::NonZero, Affine::IDENTITY, color(c), None, &r); }

/// A filled rounded rectangle; `radius` is a token value (px, already scaled) or `corner-radius-full`'s 0.5 → half the short side.
pub fn rounded(scene: &mut Scene, r: Rect, radius: f64, c: min_theme::Color) {
    let rad = if radius < 1.0 { radius * r.width().min(r.height()) } else { radius };
    scene.fill(Fill::NonZero, Affine::IDENTITY, color(c), None, &RoundedRect::from_rect(r, rad));
}

/// A horizontal hairline `thickness` px tall along the top edge of `y`.
pub fn hline(scene: &mut Scene, x0: f64, x1: f64, y: f64, thickness: f64, c: min_theme::Color) { rect(scene, Rect::new(x0, y, x1, y + thickness), c); }
