//! Drawing a note from its layout into the UI layer's scene. Everything here is paint: positions come from
//! `mn_layout`. Colours are placeholders until the theme's tokens for the page are settled (how it looks is ruled when
//! it is seen).
use kurbo::{Affine, Line, Rect, RoundedRect, Stroke};
use min_ui::fonts::{draw_layout, Family, Fonts};
use mn_doc::{BlockId, Doc, Kind, Mark, MarkKind};
use mn_layout::{hex, NoteLayout, Part};
use peniko::{Color, Fill};
use vello::Scene;

pub const PAGE: Color = Color::from_rgb8(0x1E, 0x1E, 0x1E);
const TEXT: Color = Color::from_rgb8(0xEB, 0xEB, 0xEB);
const DIM: Color = Color::from_rgb8(0x8A, 0x8A, 0x8A);
const LINE: Color = Color::from_rgb8(0x3A, 0x3A, 0x3A);
const PANEL: Color = Color::from_rgb8(0x2A, 0x2A, 0x2A);
const HEADER: Color = Color::from_rgb8(0x30, 0x30, 0x30);
const ACCENT: Color = Color::from_rgb8(0x01, 0x89, 0xF1);
const CONFLICT: Color = Color::from_rgb8(0xE0, 0xA0, 0x30);

fn fill(scene: &mut Scene, r: Rect, c: Color) { scene.fill(Fill::NonZero, Affine::IDENTITY, c, None, &r); }

/// The number a numbered item shows: it counts the numbered items above it at its depth, through deeper items, until
/// anything else (or a shallower item) breaks the list.
fn number(doc: &Doc, id: BlockId, depth: u8) -> usize {
    let Some(b) = doc.get(id) else { return 1 };
    let sibs = doc.children(b.parent); let at = sibs.iter().position(|x| *x == id).unwrap_or(0); let mut n = 1;
    for s in sibs[..at].iter().rev() {
        match doc.get(*s).map(|x| &x.kind) {
            Some(Kind::Numbered { depth: d }) if *d == depth => n += 1,
            Some(Kind::Numbered { depth: d } | Kind::Bullet { depth: d } | Kind::Task { depth: d, .. }) if *d > depth => {}
            _ => break,
        }
    }
    n
}

/// Paints the note with its top-left at (`ox`, `oy`) physical px; blocks wholly outside `view` are skipped.
pub fn note(scene: &mut Scene, doc: &Doc, layout: &NoteLayout, fonts: &mut Fonts, ox: f64, oy: f64, view: Rect) {
    let s = layout.scale as f64; let st = layout.style.clone();
    for b in doc.walk() {
        let Some(r) = layout.rect(doc, b.id) else { continue };
        let r = r + kurbo::Vec2::new(ox, oy);
        if r.y1 < view.y0 || r.y0 > view.y1 { continue }
        if b.conflict.is_some() { fill(scene, Rect::new(r.x0 - 10.0 * s, r.y0, r.x0 - 7.0 * s, r.y1), CONFLICT); }
        match (&b.kind, layout.get(b.id).map(|x| &x.part)) {
            (Kind::Divider, _) => { let y = ((r.y0 + r.y1) / 2.0).round(); fill(scene, Rect::new(r.x0, y, r.x1, y + s.max(1.0)), LINE); }
            (Kind::Media { descriptor }, _) => {
                scene.fill(Fill::NonZero, Affine::IDENTITY, PANEL, None, &RoundedRect::from_rect(r, 6.0 * s));
                let v: serde_json::Value = serde_json::from_str(descriptor).unwrap_or_default();
                let label = format!("{} — {}", v.get("kind").and_then(|k| k.as_str()).unwrap_or("image"), v.get("name").and_then(|k| k.as_str()).or(v.get("src").and_then(|k| k.as_str())).unwrap_or(""));
                let l = fonts.layout_fit(&label, Family::Sans, (12.0 * s) as f32, 400.0, DIM, r.width() - 24.0 * s);
                draw_layout(scene, &l, Affine::translate((r.x0 + 12.0 * s, r.y0 + 10.0 * s)));
            }
            (Kind::Table { header_rows, .. }, _) => {
                for (i, row) in doc.children(Some(b.id)).iter().enumerate() {
                    let Some(rr) = layout.rect(doc, *row) else { continue }; let rr = rr + kurbo::Vec2::new(ox, oy);
                    let row_bg = match doc.get(*row).map(|x| &x.kind) { Some(Kind::Row { bg, .. }) => hex(bg), _ => None };
                    if let Some(c) = row_bg.or((i < *header_rows as usize).then_some(HEADER)) { fill(scene, rr, c); }
                    for cell in doc.children(Some(*row)) {
                        let Some(cr) = layout.rect(doc, *cell) else { continue }; let cr = cr + kurbo::Vec2::new(ox, oy);
                        if let Some(Kind::Cell { bg, .. }) = doc.get(*cell).map(|x| &x.kind) { if let Some(c) = hex(bg) { fill(scene, cr, c); } }
                        scene.stroke(&Stroke::new(s.max(1.0)), Affine::IDENTITY, LINE, None, &cr);
                    }
                }
            }
            (kind, Some(Part::Text { layout: text, dx, dy })) => {
                let (tx, ty) = (r.x0 + *dx as f64, r.y0 + *dy as f64);
                match kind {
                    Kind::Code { .. } => scene.fill(Fill::NonZero, Affine::IDENTITY, PANEL, None, &RoundedRect::from_rect(r, 6.0 * s)),
                    Kind::Quote => fill(scene, Rect::new(r.x0 + 4.0 * s, r.y0, r.x0 + 7.0 * s, r.y1), LINE),
                    // the marker is drawn, not typed: the bundled face has no hollow bullet
                    Kind::Bullet { depth } => { let c = kurbo::Circle::new((tx - 11.0 * s, ty + st.body as f64 * layout.scale as f64 * st.line_height as f64 * 0.5), 2.2 * s);
                        if depth % 2 == 0 { scene.fill(Fill::NonZero, Affine::IDENTITY, DIM, None, &c) } else { scene.stroke(&Stroke::new(1.2 * s), Affine::IDENTITY, DIM, None, &c) } }
                    Kind::Numbered { depth } => { let g = fonts.layout(&format!("{}.", number(doc, b.id, *depth)), Family::Sans, (st.body * layout.scale) as f32, 400.0, DIM); draw_layout(scene, &g, Affine::translate((tx - 6.0 * s - g.width() as f64, ty))); }
                    Kind::Task { state, .. } => {
                        let bx = Rect::new(tx - 20.0 * s, ty + 3.0 * s, tx - 6.0 * s, ty + 17.0 * s); let rr = RoundedRect::from_rect(bx, 3.0 * s);
                        match state { 0 => scene.stroke(&Stroke::new(1.5 * s), Affine::IDENTITY, DIM, None, &rr),
                            1 => { scene.stroke(&Stroke::new(1.5 * s), Affine::IDENTITY, ACCENT, None, &rr); fill(scene, Rect::new(bx.x0 + 3.0 * s, bx.center().y - s, bx.x1 - 3.0 * s, bx.center().y + s), ACCENT); }
                            _ => { scene.fill(Fill::NonZero, Affine::IDENTITY, ACCENT, None, &rr); let k = kurbo::BezPath::from_vec(vec![kurbo::PathEl::MoveTo((bx.x0 + 3.0 * s, bx.center().y).into()), kurbo::PathEl::LineTo((bx.x0 + 6.0 * s, bx.y1 - 3.5 * s).into()), kurbo::PathEl::LineTo((bx.x1 - 3.0 * s, bx.y0 + 3.5 * s).into())]); scene.stroke(&Stroke::new(1.8 * s), Affine::IDENTITY, Color::WHITE, None, &k); } }
                    }
                    _ => {}
                }
                // behind the text: highlights and chips; over it: underlines and strike-throughs
                for sp in b.spans.as_slice() {
                    let rects = layout.range_rects(doc, b.id, sp.start, sp.end);
                    for q in rects { let q = q + kurbo::Vec2::new(ox, oy);
                        match &sp.mark {
                            Mark::Highlight(c) => fill(scene, q, hex(c).unwrap_or(Color::from_rgb8(0x66, 0x55, 0x00))),
                            Mark::Choice(payload) => { let v: serde_json::Value = serde_json::from_str(payload).unwrap_or_default(); let sel = v.get("v").and_then(|x| x.as_str()).unwrap_or("");
                                let c = v.get("o").and_then(|o| o.as_array()).and_then(|o| o.iter().find(|x| x.get("id").and_then(|i| i.as_str()) == Some(sel))).and_then(|o| o.get("c")).and_then(|c| c.as_str()).and_then(hex).unwrap_or(HEADER);
                                scene.fill(Fill::NonZero, Affine::IDENTITY, c, None, &RoundedRect::from_rect(q.inflate(5.0 * s, 0.0), 9.0 * s)); }
                            Mark::Code => scene.fill(Fill::NonZero, Affine::IDENTITY, PANEL, None, &RoundedRect::from_rect(q.inflate(2.0 * s, 0.0), 3.0 * s)),
                            _ => {}
                        }
                    }
                }
                draw_layout(scene, text, Affine::translate((tx, ty)));
                for sp in b.spans.as_slice() {
                    let k = sp.mark.kind(); if !matches!(k, MarkKind::Underline | MarkKind::Link | MarkKind::Strike) { continue }
                    for q in layout.range_rects(doc, b.id, sp.start, sp.end) { let q = q + kurbo::Vec2::new(ox, oy);
                        let y = if k == MarkKind::Strike { q.y0 + q.height() * 0.56 } else { q.y1 - 3.0 * s };
                        scene.stroke(&Stroke::new(s.max(1.0)), Affine::IDENTITY, if k == MarkKind::Link { st.link } else { TEXT }, None, &Line::new((q.x0, y), (q.x1, y))); }
                }
            }
            _ => {}
        }
    }
}
