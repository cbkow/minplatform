//! Drawing a note from its layout into the UI layer's scene. Everything here is paint: positions come from
//! `mn_layout`. Colours are placeholders until the theme's tokens for the page are settled (how it looks is ruled when
//! it is seen).
use kurbo::{Affine, Line, Rect, RoundedRect, Stroke};
use min_ui::fonts::{draw_layout, Family, Fonts};
use mn_doc::{BlockId, Doc, Kind, Mark, MarkKind};
use mn_layout::{hex, NoteLayout, Part};
use peniko::{Color, Fill};
use vello::Scene;
use crate::media::{Media, View};

// The page's colours are the theme's (min-theme, as QCView's panels): the page is the base background, raised parts
// (a code block, a media box, a table's header) are its layers, lines are dividers, the caret and the selection are the
// text fields' own.
fn c(k: min_theme::Color) -> Color { min_ui::paint::color(k) }
use min_theme::tokens as t;
pub fn page() -> Color { c(t::BACKGROUND_BASE_COLOR) }
pub fn selection() -> Color { c(t::FIELD_SELECTION_COLOR) }
pub fn caret() -> Color { c(t::FIELD_CARET_COLOR) }
pub fn text() -> Color { c(t::NEUTRAL_CONTENT_COLOR_DEFAULT) }
pub fn link() -> Color { c(t::ACCENT_CONTENT_COLOR_DEFAULT) }
pub fn subdued() -> Color { dim() }
fn dim() -> Color { c(t::NEUTRAL_SUBDUED_CONTENT_COLOR_DEFAULT) }
fn line() -> Color { c(t::DIVIDER_COLOR) }
fn panel() -> Color { c(t::BACKGROUND_LAYER_2_COLOR) }
fn header() -> Color { c(t::BACKGROUND_LAYER_1_COLOR) }
pub fn accent() -> Color { c(t::ACCENT_BACKGROUND_COLOR_DEFAULT) }
fn conflict() -> Color { c(t::NOTICE_VISUAL_COLOR) }

fn fill(scene: &mut Scene, r: Rect, c: Color) { scene.fill(Fill::NonZero, Affine::IDENTITY, c, None, &r); }

/// The video that plays, as the page's painter needs it: its block, the frame it is on, whether it plays, whether the
/// presenter shows it yet (until then its poster is painted), its speed, and whether it is muted / does not loop.
#[derive(Clone, Copy)]
pub struct Live { pub id: BlockId, pub frame: i32, pub playing: bool, pub shown: bool, pub speed: f64, pub muted: bool, pub looping: bool }
/// The parts of a video block's strip: the play button's box and the line to scrub on (the counter sits between).
pub fn video_strip(strip: Rect, s: f64) -> (Rect, Rect) {
    let play = Rect::new(strip.x0, strip.y0, strip.x0 + 34.0 * s, strip.y1); let x0 = strip.x0 + 34.0 * s + 96.0 * s;
    // the right end is kept for the speed, mute and loop readouts
    let x1 = strip.x1 - 12.0 * s - 96.0 * s;
    (play, Rect::new(x0.min(x1), strip.center().y - s, x1.max(x0.min(x1)), strip.center().y + s))
}

/// A long path keeps its two ends (the file's name is at the far one).
fn middle(t: &str, max: usize) -> String {
    let n = t.chars().count(); if n <= max || max < 8 { return t.to_string() }
    let keep = max - 1; let (a, z) = (keep / 3, keep - keep / 3);
    format!("{}\u{2026}{}", t.chars().take(a).collect::<String>(), t.chars().skip(n - z).collect::<String>())
}

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

/// Paints the note with its top-left at (`ox`, `oy`) physical px; blocks wholly outside `view` are skipped. True: a
/// picture is still fading in - another frame is wanted.
pub fn note(scene: &mut Scene, doc: &Doc, layout: &NoteLayout, fonts: &mut Fonts, media: &mut Media, turned: &mut Vec<BlockId>, live: Option<Live>, ox: f64, oy: f64, view: Rect) -> bool {
    let s = layout.scale as f64; let st = layout.style.clone(); let mut animating = false;
    for b in doc.walk() {
        let Some(r) = layout.rect(doc, b.id) else { continue };
        let r = r + kurbo::Vec2::new(ox, oy);
        if r.y1 < view.y0 || r.y0 > view.y1 { continue }
        if b.conflict.is_some() { fill(scene, Rect::new(r.x0 - 10.0 * s, r.y0, r.x0 - 7.0 * s, r.y1), conflict()); }
        match (&b.kind, layout.get(b.id).map(|x| &x.part)) {
            (Kind::Divider, _) => { let y = ((r.y0 + r.y1) / 2.0).round(); fill(scene, Rect::new(r.x0, y, r.x1, y + s.max(1.0)), line()); }
            (Kind::Media { descriptor }, _) => {
                // a PDF: its page, and under it a strip with the page's number among the pages, previous and next
                let rec: serde_json::Value = serde_json::from_str(descriptor).unwrap_or_default();
                let pdf = rec.get("kind").and_then(|k| k.as_str()) == Some("pdf");
                let video = rec.get("kind").and_then(|k| k.as_str()) == Some("video");
                let whole = r; let r = if pdf { Rect::new(r.x0, r.y0, r.x1, r.y1 - mn_layout::PDF_STRIP as f64 * s) } else if video { Rect::new(r.x0, r.y0, r.x1, r.y1 - mn_layout::VIDEO_STRIP as f64 * s) } else { r };
                // a video: under its picture a strip - play or pause, the frame it stands on as a timecode, the line to scrub
                // on. The video that is PLAYING is not painted here: the presenter shows it under this layer, through the
                // place left open (row 287); its strip follows the player
                let me = live.filter(|x| x.id == b.id);
                if video {
                    let (frames, fps) = (rec.get("frames").and_then(|f| f.as_i64()).unwrap_or(1).max(1), rec.get("fps").and_then(|f| f.as_f64()).unwrap_or(24.0));
                    let (at, playing) = match me { Some(m) => (m.frame as i64, m.playing), None => (rec.get("frame").and_then(|f| f.as_i64()).unwrap_or(0), false) };
                    let strip = Rect::new(whole.x0, r.y1, whole.x1, whole.y1); let (play, track) = video_strip(strip, s);
                    if let Some(i) = fonts.icon(if playing { "pause" } else { "play" }, (15.0 * s) as f32, text()) { draw_layout(scene, &i, Affine::translate((play.center().x - i.width() as f64 / 2.0, play.center().y - i.height() as f64 / 2.0))); }
                    let t = fonts.layout(&crate::video::timecode(at, fps), Family::Sans, (12.0 * s) as f32, 400.0, dim()); draw_layout(scene, &t, Affine::translate((play.x1 + 2.0 * s, strip.center().y - t.height() as f64 / 2.0)));
                    if track.width() > 8.0 * s { let x = track.x0 + track.width() * (at.clamp(0, frames - 1) as f64 / (frames - 1).max(1) as f64);
                        fill(scene, track, line()); fill(scene, Rect::new(track.x0, track.y0, x, track.y1), accent());
                        scene.fill(Fill::NonZero, Affine::IDENTITY, text(), None, &RoundedRect::from_rect(Rect::new(x - 1.5 * s, strip.center().y - 7.0 * s, x + 1.5 * s, strip.center().y + 7.0 * s), 1.5 * s)); }
                    // what is not as usual, said small at the strip's right end: a speed other than 1, muted, not looping
                    if let Some(m) = me { let mut x = strip.x1 - 10.0 * s;
                        for (icon, on) in [("repeat", !m.looping), ("speaker-slash", m.muted)] { if on { if let Some(i) = fonts.icon(icon, (13.0 * s) as f32, dim()) { x -= i.width() as f64; draw_layout(scene, &i, Affine::translate((x, strip.center().y - i.height() as f64 / 2.0))); x -= 6.0 * s;
                            if icon == "repeat" { scene.stroke(&Stroke::new(1.2 * s), Affine::IDENTITY, dim(), None, &Line::new((x + 5.0 * s, strip.center().y + 6.0 * s), (x + 6.0 * s + i.width() as f64 + 1.0 * s, strip.center().y - 6.0 * s))); } } } }
                        if (m.speed - 1.0).abs() > 0.01 { let t = fonts.layout(&format!("{}\u{d7}", if (m.speed * 10.0).fract().abs() < 0.01 { format!("{:.1}", m.speed) } else { format!("{:.2}", m.speed) }), Family::Sans, (11.0 * s) as f32, 600.0, dim()); x -= t.width() as f64; draw_layout(scene, &t, Affine::translate((x, strip.center().y - t.height() as f64 / 2.0))); } }
                }
                if me.is_some_and(|x| x.shown) { continue }
                if pdf {
                    let (page, pages) = (rec.get("page").and_then(|p| p.as_u64()).unwrap_or(0) + 1, rec.get("pages").and_then(|p| p.as_u64()).unwrap_or(1).max(1));
                    let t = fonts.layout(&format!("{} / {pages}", page.min(pages)), Family::Sans, (12.0 * s) as f32, 400.0, dim()); let (cx, cy) = (whole.center().x, (r.y1 + whole.y1) / 2.0);
                    draw_layout(scene, &t, Affine::translate((cx - t.width() as f64 / 2.0, cy - t.height() as f64 / 2.0)));
                    for (name, dx, on) in [("caret-left", -1.0, page > 1), ("caret-right", 1.0, page < pages)] { if let Some(i) = fonts.icon(name, (16.0 * s) as f32, if on { text() } else { line() }) { draw_layout(scene, &i, Affine::translate((cx + dx * (t.width() as f64 / 2.0 + 22.0 * s) - i.width() as f64 / 2.0, cy - i.height() as f64 / 2.0))); } }
                    scene.fill(Fill::NonZero, Affine::IDENTITY, Color::WHITE, None, &r);
                }
                let box_ = RoundedRect::from_rect(r, if pdf { 0.0 } else { 6.0 * s });
                // a state in words: the file's name, and under it where it was looked for or why it was not read
                let words = |scene: &mut Scene, fonts: &mut Fonts, head: &str, name: &str, more: &str| {
                    scene.fill(Fill::NonZero, Affine::IDENTITY, panel(), None, &box_);
                    let a = fonts.layout_fit(&if name.is_empty() { head.to_string() } else { format!("{head} \u{2014} {name}") }, Family::Sans, (13.0 * s) as f32, 400.0, text(), r.width() - 24.0 * s);
                    draw_layout(scene, &a, Affine::translate((r.x0 + 12.0 * s, r.y0 + 10.0 * s)));
                    if !more.is_empty() && r.height() > 52.0 * s { let more = &middle(more, ((r.width() - 24.0 * s) / (6.6 * s)) as usize); let l = fonts.layout_fit(more, Family::Sans, (12.0 * s) as f32, 400.0, dim(), r.width() - 24.0 * s); draw_layout(scene, &l, Affine::translate((r.x0 + 12.0 * s, r.y0 + 14.0 * s + a.height() as f64))); }
                };
                match media.view(descriptor, r.width(), r.height()) {
                    View::Ready { brush, w, h, fade, pill } => {
                        // the whole picture inside its box, centred (the box has its shape, but for a picture the
                        // file stores lying on its side), on whole pixels
                        // lying on its side in a box of the stored shape (a record the Qt app wrote): the caller lays the box out upright
                        { let (ba, pa) = (r.width() / r.height(), w as f64 / h as f64); if (ba * pa - 1.0).abs() < 0.04 && (ba / pa - 1.0).abs() > 0.04 { turned.push(b.id); } }
                        let k = (r.width() / w as f64).min(r.height() / h as f64); let (dw, dh) = ((w as f64 * k).round(), (h as f64 * k).round());
                        let (x, y) = ((r.x0 + (r.width() - dw) / 2.0).round(), (r.y0 + (r.height() - dh) / 2.0).round());
                        if fade < 1.0 { if !pdf { scene.fill(Fill::NonZero, Affine::IDENTITY, panel().multiply_alpha(1.0 - fade), None, &box_); } animating = true; }
                        min_ui::paint::image(scene, &brush.clone().with_alpha(fade), Rect::new(x, y, x + dw, y + dh));
                        // a video at rest: its length, small, in the picture's lower right corner
                        if let Some(ms) = rec.get("kind").and_then(|k| k.as_str()).filter(|k| *k == "video").and_then(|_| rec.get("durMs")).and_then(|d| d.as_f64()) {
                            let t = fonts.layout(&crate::video::length(ms), Family::Sans, (11.0 * s) as f32, 600.0, Color::WHITE); let icon = fonts.icon("play", (11.0 * s) as f32, Color::WHITE); let iw = icon.as_ref().map(|i| i.width() as f64 + 5.0 * s).unwrap_or(0.0);
                            let q = Rect::new(x + dw - 8.0 * s - t.width() as f64 - iw - 14.0 * s, y + dh - 8.0 * s - t.height() as f64 - 6.0 * s, x + dw - 8.0 * s, y + dh - 8.0 * s);
                            if q.x0 > x && q.y0 > y { scene.fill(Fill::NonZero, Affine::IDENTITY, Color::BLACK.multiply_alpha(0.6 * fade), None, &RoundedRect::from_rect(q, 4.0 * s));
                                if let Some(i) = &icon { draw_layout(scene, i, Affine::translate((q.x0 + 7.0 * s, q.center().y - i.height() as f64 / 2.0))); }
                                draw_layout(scene, &t, Affine::translate((q.x0 + 7.0 * s + iw, q.y0 + 3.0 * s))); } }
                        // a picture made of another file (a Photoshop file, an SVG ...) says which, small, in its corner
                        if let Some(p) = pill { let t = fonts.layout(&p, Family::Sans, (10.5 * s) as f32, 600.0, text());
                            let q = Rect::new(x + 8.0 * s, y + dh - 8.0 * s - t.height() as f64 - 4.0 * s, x + 8.0 * s + t.width() as f64 + 12.0 * s, y + dh - 8.0 * s);
                            if q.y0 > y && q.x1 < x + dw { scene.fill(Fill::NonZero, Affine::IDENTITY, panel().multiply_alpha(0.85 * fade), None, &RoundedRect::from_rect(q, 4.0 * s)); draw_layout(scene, &t, Affine::translate((q.x0 + 6.0 * s, q.y0 + 2.0 * s))); } }
                    }
                    View::Loading => if !pdf { scene.fill(Fill::NonZero, Affine::IDENTITY, panel(), None, &box_) },
                    View::Missing { name, looked } => words(scene, fonts, "Not found", &name, &looked),
                    View::Failed { name, why } => words(scene, fonts, "Cannot be shown", &name, &why),
                    View::Other => {
                        scene.fill(Fill::NonZero, Affine::IDENTITY, panel(), None, &box_);
                        let v = &rec;
                        let label = format!("{} \u{2014} {}", v.get("kind").and_then(|k| k.as_str()).unwrap_or("image"), v.get("name").and_then(|k| k.as_str()).or(v.get("src").and_then(|k| k.as_str())).unwrap_or(""));
                        let l = fonts.layout_fit(&label, Family::Sans, (12.0 * s) as f32, 400.0, dim(), r.width() - 24.0 * s);
                        draw_layout(scene, &l, Affine::translate((r.x0 + 12.0 * s, r.y0 + 10.0 * s)));
                    }
                }
                if pdf { scene.stroke(&Stroke::new(s.max(1.0)), Affine::IDENTITY, line(), None, &r); }
            }
            (Kind::Table { header_rows, .. }, _) => {
                for (i, row) in doc.children(Some(b.id)).iter().enumerate() {
                    let Some(rr) = layout.rect(doc, *row) else { continue }; let rr = rr + kurbo::Vec2::new(ox, oy);
                    let row_bg = match doc.get(*row).map(|x| &x.kind) { Some(Kind::Row { bg, .. }) => hex(bg), _ => None };
                    if let Some(c) = row_bg.or((i < *header_rows as usize).then_some(header())) { fill(scene, rr, c); }
                    for cell in doc.children(Some(*row)) {
                        let Some(cr) = layout.rect(doc, *cell) else { continue }; let cr = cr + kurbo::Vec2::new(ox, oy);
                        if let Some(Kind::Cell { bg, .. }) = doc.get(*cell).map(|x| &x.kind) { if let Some(c) = hex(bg) { fill(scene, cr, c); } }
                        scene.stroke(&Stroke::new(s.max(1.0)), Affine::IDENTITY, line(), None, &cr);
                    }
                }
            }
            (kind, Some(Part::Text { layout: text, dx, dy })) => {
                let (tx, ty) = (r.x0 + *dx as f64, r.y0 + *dy as f64);
                match kind {
                    Kind::Code { .. } => scene.fill(Fill::NonZero, Affine::IDENTITY, panel(), None, &RoundedRect::from_rect(r, 6.0 * s)),
                    Kind::Quote => fill(scene, Rect::new(r.x0 + 4.0 * s, r.y0, r.x0 + 7.0 * s, r.y1), line()),
                    // the marker is drawn, not typed: the bundled face has no hollow bullet
                    Kind::Bullet { depth } => { let c = kurbo::Circle::new((tx - 11.0 * s, ty + st.body as f64 * layout.scale as f64 * st.line_height as f64 * 0.5), 2.2 * s);
                        if depth % 2 == 0 { scene.fill(Fill::NonZero, Affine::IDENTITY, dim(), None, &c) } else { scene.stroke(&Stroke::new(1.2 * s), Affine::IDENTITY, dim(), None, &c) } }
                    Kind::Numbered { depth } => { let g = fonts.layout(&format!("{}.", number(doc, b.id, *depth)), Family::Sans, (st.body * layout.scale) as f32, 400.0, dim()); draw_layout(scene, &g, Affine::translate((tx - 6.0 * s - g.width() as f64, ty))); }
                    Kind::Task { state, .. } => {
                        let bx = Rect::new(tx - 20.0 * s, ty + 3.0 * s, tx - 6.0 * s, ty + 17.0 * s); let rr = RoundedRect::from_rect(bx, 3.0 * s);
                        match state { 0 => scene.stroke(&Stroke::new(1.5 * s), Affine::IDENTITY, dim(), None, &rr),
                            1 => { scene.stroke(&Stroke::new(1.5 * s), Affine::IDENTITY, accent(), None, &rr); fill(scene, Rect::new(bx.x0 + 3.0 * s, bx.center().y - s, bx.x1 - 3.0 * s, bx.center().y + s), accent()); }
                            _ => { scene.fill(Fill::NonZero, Affine::IDENTITY, accent(), None, &rr); let k = kurbo::BezPath::from_vec(vec![kurbo::PathEl::MoveTo((bx.x0 + 3.0 * s, bx.center().y).into()), kurbo::PathEl::LineTo((bx.x0 + 6.0 * s, bx.y1 - 3.5 * s).into()), kurbo::PathEl::LineTo((bx.x1 - 3.0 * s, bx.y0 + 3.5 * s).into())]); scene.stroke(&Stroke::new(1.8 * s), Affine::IDENTITY, Color::WHITE, None, &k); } }
                    }
                    _ => {}
                }
                // behind the text: highlights and chips; over it: underlines and strike-throughs
                for sp in b.spans.as_slice() {
                    let rects = layout.range_rects(doc, b.id, sp.start, sp.end);
                    for q in rects { let q = q + kurbo::Vec2::new(ox, oy);
                        match &sp.mark {
                            Mark::Highlight(c) => { if let Some(c) = mn_layout::tint(c) { fill(scene, q, c) } }
                            Mark::Choice(payload) => { let v: serde_json::Value = serde_json::from_str(payload).unwrap_or_default(); let sel = v.get("v").and_then(|x| x.as_str()).unwrap_or("");
                                let c = v.get("o").and_then(|o| o.as_array()).and_then(|o| o.iter().find(|x| x.get("id").and_then(|i| i.as_str()) == Some(sel))).and_then(|o| o.get("c")).and_then(|c| c.as_str()).and_then(hex).unwrap_or(header());
                                scene.fill(Fill::NonZero, Affine::IDENTITY, c, None, &RoundedRect::from_rect(q.inflate(5.0 * s, 0.0), 9.0 * s)); }
                            Mark::Code => scene.fill(Fill::NonZero, Affine::IDENTITY, panel(), None, &RoundedRect::from_rect(q.inflate(2.0 * s, 0.0), 3.0 * s)),
                            _ => {}
                        }
                    }
                }
                draw_layout(scene, text, Affine::translate((tx, ty)));
                for sp in b.spans.as_slice() {
                    let k = sp.mark.kind(); if !matches!(k, MarkKind::Underline | MarkKind::Link | MarkKind::Strike) { continue }
                    for q in layout.range_rects(doc, b.id, sp.start, sp.end) { let q = q + kurbo::Vec2::new(ox, oy);
                        let y = if k == MarkKind::Strike { q.y0 + q.height() * 0.56 } else { q.y1 - 3.0 * s };
                        scene.stroke(&Stroke::new(s.max(1.0)), Affine::IDENTITY, if k == MarkKind::Link { st.link } else { st.text }, None, &Line::new((q.x0, y), (q.x1, y))); }
                }
            }
            _ => {}
        }
    }
    animating
}
