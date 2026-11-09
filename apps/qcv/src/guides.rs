//! Safety guides (canon rows 152 / 215; QCView `render/safety_overlay` + `svg_path_parser`): one SVG at a time from QCView's
//! set (assets/safety, embedded), its outlines fitted to the PICTURE — the viewBox fits into the source frame (square pixels,
//! centred), then follows the display (pixel aspect, rotation) — stroked over the picture at a width in screen pixels, the
//! user's colour × opacity. Parsed with kurbo's full SVG path syntax (QCView's parser took only M / L / H / V / Z, so the
//! TikTok guide's relative `l` was dropped); `<rect>` / `<polygon>` / `<polyline>` / `<line>` too; the file's own styles
//! are ignored (QCView's rule). Mitred joins, so corners close (QCView's butt-ended segments notched them).
use kurbo::{Affine, BezPath, Join, Point, Rect, Stroke};
use vello::Scene;

macro_rules! guides { ($($n:literal),*) => { &[$(($n, include_str!(concat!("../assets/safety/", $n, ".svg")))),*] } }
/// QCView's list, by file name (its Guide combo shows these, sorted, after "(none)").
pub const GUIDES: &[(&str, &str)] = guides!("16x9", "Meta_9x16", "Meta_Feed_4x5", "Meta_Post_3x4", "Pinterest_9x16", "Pinterest_PremiumSpotlight_1x1", "Samsung_9x16_Safety", "Snapchat_9x16_unofficial", "TikTok_9x16", "Youtube_16x9", "Youtube_16x9_Masthead", "Youtube_1x1", "Youtube_9x16");

/// A parsed guide: its viewBox (w, h) and outlines in viewBox units.
pub struct Guide { pub view: (f64, f64), pub paths: Vec<BezPath> }

fn attr<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    let key = format!(" {name}=\""); let i = tag.find(&key)? + key.len(); let j = tag[i..].find('"')?; Some(&tag[i..i + j])
}
fn num(tag: &str, name: &str) -> f64 { attr(tag, name).and_then(|v| v.trim().parse().ok()).unwrap_or(0.0) }
fn points(s: &str) -> Vec<Point> { let v: Vec<f64> = s.split(|c: char| c == ',' || c.is_whitespace()).filter(|x| !x.is_empty()).filter_map(|x| x.parse().ok()).collect(); v.chunks(2).filter(|c| c.len() == 2).map(|c| Point::new(c[0], c[1])).collect() }

pub fn parse(svg: &str) -> Guide {
    let view = attr(svg, "viewBox").map(|v| { let n: Vec<f64> = v.split_whitespace().filter_map(|x| x.parse().ok()).collect(); if n.len() == 4 { (n[2], n[3]) } else { (1920.0, 1080.0) } }).unwrap_or((1920.0, 1080.0));
    let mut paths = vec![];
    for (i, _) in svg.match_indices('<') {
        let Some(end) = svg[i..].find('>') else { continue }; let tag = &svg[i..i + end];
        let name = tag[1..].split(|c: char| c.is_whitespace() || c == '/').next().unwrap_or("");
        match name {
            "path" => if let Some(d) = attr(tag, "d") { if let Ok(p) = BezPath::from_svg(d) { paths.push(p); } },
            "rect" => { let r = Rect::new(num(tag, "x"), num(tag, "y"), num(tag, "x") + num(tag, "width"), num(tag, "y") + num(tag, "height")); paths.push(kurbo::Shape::to_path(&r, 0.1)); }
            "line" => { let mut p = BezPath::new(); p.move_to((num(tag, "x1"), num(tag, "y1"))); p.line_to((num(tag, "x2"), num(tag, "y2"))); paths.push(p); }
            "polygon" | "polyline" => if let Some(pts) = attr(tag, "points").map(points) { if pts.len() >= 2 { let mut p = BezPath::new(); p.move_to(pts[0]); for q in &pts[1..] { p.line_to(*q); } if name == "polygon" { p.close_path(); } paths.push(p); } },
            _ => {}
        }
    }
    Guide { view, paths }
}

/// The transform from viewBox units to window pixels for a picture shown in `rect` (x, y, w, h — post-rotation), its
/// source `src` (w, h, unrotated, square pixels) and `quarters` of clockwise rotation (QCView `meshForImage`).
pub fn transform(view: (f64, f64), src: (f64, f64), rect: [f64; 4], quarters: u8) -> Affine {
    let (vw, vh) = view; let (sw, sh) = (src.0.max(1.0), src.1.max(1.0));
    // viewBox → source pixels: fit, centred
    let k = (sw / vw).min(sh / vh); let (ox, oy) = ((sw - vw * k) / 2.0, (sh - vh * k) / 2.0);
    let to_src = Affine::translate((ox, oy)) * Affine::scale(k);
    // source pixels → unit square → rotated unit square → the displayed rect
    let to_unit = Affine::scale_non_uniform(1.0 / sw, 1.0 / sh);
    let rot = match quarters & 3 { 1 => Affine::new([0.0, 1.0, -1.0, 0.0, 1.0, 0.0]), 2 => Affine::new([-1.0, 0.0, 0.0, -1.0, 1.0, 1.0]), 3 => Affine::new([0.0, -1.0, 1.0, 0.0, 0.0, 1.0]), _ => Affine::IDENTITY };
    let to_rect = Affine::translate((rect[0], rect[1])) * Affine::scale_non_uniform(rect[2], rect[3]);
    to_rect * rot * to_unit * to_src
}

/// Stroke `g` into `scene` through `tf` at `width` window pixels in `color` (opacity already in its alpha).
pub fn paint(scene: &mut Scene, g: &Guide, tf: Affine, width: f64, color: peniko::Color) {
    let st = Stroke::new(width.max(0.5)).with_join(Join::Miter);
    for p in &g.paths { let mut q = p.clone(); q.apply_affine(tf); scene.stroke(&st, Affine::IDENTITY, color, None, &q); }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn every_guide_parses_and_16x9_is_title_and_action_safe() {
        for (n, svg) in GUIDES { let g = parse(svg); assert!(!g.paths.is_empty(), "{n} has outlines"); }
        let g = parse(GUIDES[0].1); assert_eq!(g.view, (1920.0, 1080.0)); assert_eq!(g.paths.len(), 2);
        let b = kurbo::Shape::bounding_box(&g.paths[0]); assert!((b.width() - 1728.0).abs() < 0.01 && (b.height() - 972.0).abs() < 0.01, "title safe 90 %: {b:?}");
        // the TikTok guide's relative `l` (QCView dropped it): its outline closes back on its start
        let tk = parse(GUIDES.iter().find(|g| g.0 == "TikTok_9x16").unwrap().1); assert_eq!(tk.view, (1080.0, 1920.0));
        assert!(kurbo::Shape::bounding_box(&tk.paths[0]).width() > 800.0);
    }
    #[test]
    fn a_16x9_guide_fits_the_picture_and_follows_rotation() {
        // a 1920×1080 source shown at 960×540 at (100, 50): the viewBox corner (0,0) lands on the picture's corner
        let tf = transform((1920.0, 1080.0), (1920.0, 1080.0), [100.0, 50.0, 960.0, 540.0], 0);
        let p = tf * Point::new(0.0, 0.0); assert!((p.x - 100.0).abs() < 1e-9 && (p.y - 50.0).abs() < 1e-9);
        let q = tf * Point::new(1920.0, 1080.0); assert!((q.x - 1060.0).abs() < 1e-9 && (q.y - 590.0).abs() < 1e-9);
        // rotated 90° CW, shown 540×960: the source's top-left goes to the displayed top-RIGHT
        let tf = transform((1920.0, 1080.0), (1920.0, 1080.0), [0.0, 0.0, 540.0, 960.0], 1);
        let p = tf * Point::new(0.0, 0.0); assert!((p.x - 540.0).abs() < 1e-9 && p.y.abs() < 1e-9, "{p:?}");
    }
}
