//! Small drawing helpers over vello, in the theme's terms.
use kurbo::{Affine, Rect, RoundedRect};
use peniko::{Color, Fill};
use vello::Scene;

/// A theme colour as vello's (sRGB-encoded, straight alpha — the same convention).
pub fn color(c: min_theme::Color) -> Color { Color::new([c.r, c.g, c.b, c.a]) }

/// A filled rectangle in physical pixels.
pub fn rect(scene: &mut Scene, r: Rect, c: min_theme::Color) { scene.fill(Fill::NonZero, Affine::IDENTITY, color(c), None, &r); }

/// A corner-radius token in physical px for `r`: px tokens scale by the window factor; `corner-radius-full` (the ratio
/// 0.5) is half of `r`'s short side. Resolve BEFORE scaling — 0.5 × a scale of 2 is 1.0, which read as one pixel
/// (the pill came out square on macOS, round on Windows; 2026-11-07).
pub fn radius(token: f64, scale: f64, r: Rect) -> f64 { if token < 1.0 { token * r.width().min(r.height()) } else { token * scale } }

/// A filled rounded rectangle; `radius` is a token value (px, already scaled) or `corner-radius-full`'s 0.5 → half the short side.
pub fn rounded(scene: &mut Scene, r: Rect, radius: f64, c: min_theme::Color) {
    let rad = if radius < 1.0 { radius * r.width().min(r.height()) } else { radius };
    scene.fill(Fill::NonZero, Affine::IDENTITY, color(c), None, &RoundedRect::from_rect(r, rad));
}

/// A horizontal hairline `thickness` px tall along the top edge of `y`.
pub fn hline(scene: &mut Scene, x0: f64, x1: f64, y: f64, thickness: f64, c: min_theme::Color) { rect(scene, Rect::new(x0, y, x1, y + thickness), c); }

/// An RGBA8 (straight alpha) bitmap as a vello image brush — the hover thumbnail's pixels, kept across frames.
pub fn image_rgba8(width: u32, height: u32, rgba: Vec<u8>) -> peniko::ImageBrush {
    peniko::ImageBrush::new(peniko::ImageData { data: peniko::Blob::new(std::sync::Arc::new(rgba)), format: peniko::ImageFormat::Rgba8, alpha_type: peniko::ImageAlphaType::Alpha, width, height })
}
/// Draw `img` scaled into `dst`.
pub fn image(scene: &mut Scene, img: &peniko::ImageBrush, dst: Rect) {
    let (w, h) = (img.image.width.max(1) as f64, img.image.height.max(1) as f64);
    scene.draw_image(img, Affine::translate((dst.x0, dst.y0)) * Affine::scale_non_uniform(dst.width() / w, dst.height() / h));
}
