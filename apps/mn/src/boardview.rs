//! A table shown as a board, PAINTED (row 365; the board as data and places is `mn_layout::board`). The page is the
//! base colour; a lane is a panel one step up (layer 1), a card a tile one more (layer 2): fills part the surfaces, no
//! borders. The accent colour is kept for where a dragged card would land.
use kurbo::{Affine, Rect, RoundedRect};
use min_theme::tokens as t;
use min_ui::{fonts::draw_layout, paint::color, Family, Fonts};
use mn_layout::board::{self, Board, Detail, Placed};

fn hex(s: &str) -> Option<peniko::Color> { let h = s.trim().trim_start_matches('#'); if h.len() != 6 { return None } let v = u32::from_str_radix(h, 16).ok()?; Some(peniko::Color::from_rgb8((v >> 16) as u8, (v >> 8) as u8, v as u8)) }
fn fill(scene: &mut vello::Scene, r: Rect, radius: f64, c: peniko::Color) { scene.fill(peniko::Fill::NonZero, Affine::IDENTITY, c, None, &RoundedRect::from_rect(r, radius)); }

/// What is under a window point of a board.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Hit { /// the bar's "Show as Table"
    ToTable, Bar, /// a lane's head (its index)
    Head(usize), /// a card (its lane, its place in it)
    Card(usize, usize), /// a lane's "+" at its foot
    Add(usize), /// a lane's empty room
    Lane(usize), Nothing }

/// The "Show as Table" button's place in the bar, from the board's top left (logical points -> the caller scales).
fn to_table(width: f64) -> Rect { Rect::new((width - 118.0).max(0.0), 5.0, width, 29.0) }

pub fn hit(b: &Board, p: &Placed, origin: (f64, f64), s: f64, x: f64, y: f64, visible_w: f64) -> Hit {
    let (lx, ly) = ((x - origin.0) / s, (y - origin.1) / s); let pt = kurbo::Point::new(lx, ly);
    if ly < board::BAR { return if to_table(visible_w / s - (0.0)).contains(kurbo::Point::new((x - origin.0) / s, ly)) { Hit::ToTable } else { Hit::Bar } }
    for (i, (lane, cards)) in p.lanes.iter().enumerate() { if !lane.contains(pt) { continue }
        if ly < lane.y0 + board::HEAD { return Hit::Head(i) }
        if let Some(k) = cards.iter().position(|c| c.contains(pt)) { return Hit::Card(i, k) }
        let foot = cards.last().map(|c| c.y1 + board::CARD_GAP).unwrap_or(lane.y0 + board::HEAD); if ly >= foot && ly < foot + board::FOOT - 6.0 { return Hit::Add(i) }
        return Hit::Lane(i) }
    let _ = b; Hit::Nothing
}

/// Paints the board with its top left at `origin` (the lanes moved left by the sideways scroll; the bar stays), clipped
/// to `clip`. `cover`: a card's picture, if it is ready. `hover`: what the pointer is over. `drop`: where a dragged
/// card would land (the lane, the place in it). `fixed_x`: the bar's left edge (the block's own, unscrolled).
#[allow(clippy::too_many_arguments)]
pub fn paint(scene: &mut vello::Scene, fonts: &mut Fonts, b: &Board, p: &Placed, origin: (f64, f64), fixed_x: f64, visible_w: f64, clip: Rect, s: f64, group: &str, hover: Hit, drop: Option<(usize, usize)>, lifted: Option<mn_doc::BlockId>, cover: &mut dyn FnMut(&str, f64, f64) -> Option<peniko::ImageBrush>) {
    scene.push_clip_layer(peniko::Fill::NonZero, Affine::IDENTITY, &clip);
    let at = |r: Rect| Rect::new(origin.0 + r.x0 * s, origin.1 + r.y0 * s, origin.0 + r.x1 * s, origin.1 + r.y1 * s);
    let (text, sub) = (color(t::NEUTRAL_CONTENT_COLOR_DEFAULT), color(t::NEUTRAL_SUBDUED_CONTENT_COLOR_DEFAULT));
    // the bar: what the board is grouped by; the way back to the table
    let label = fonts.layout_fit(&format!("Board \u{2014} by {group}"), Family::Sans, (12.0 * s) as f32, 500.0, sub, (visible_w - 140.0 * s).max(40.0)); draw_layout(scene, &label, Affine::translate((fixed_x + 2.0 * s, origin.1 + (board::BAR * s - 6.0 * s - label.height() as f64) / 2.0)));
    { let r = to_table(visible_w / s); let r = Rect::new(fixed_x + r.x0 * s, origin.1 + r.y0 * s, fixed_x + r.x1 * s, origin.1 + r.y1 * s);
        fill(scene, r, t::CORNER_RADIUS_MEDIUM_DEFAULT as f64 * s, color(if hover == Hit::ToTable { t::BACKGROUND_LAYER_2_COLOR } else { t::BACKGROUND_LAYER_1_COLOR }));
        let l = fonts.layout("Show as Table", Family::Sans, (12.0 * s) as f32, 500.0, text); draw_layout(scene, &l, Affine::translate((r.center().x - l.width() as f64 / 2.0, r.center().y - l.height() as f64 / 2.0))); }
    for (i, ((lane, cards), data)) in p.lanes.iter().zip(&b.lanes).enumerate() {
        let lr = at(*lane); if lr.x1 < clip.x0 || lr.x0 > clip.x1 { continue }
        fill(scene, lr, t::CORNER_RADIUS_LARGE_DEFAULT as f64 * s, color(t::BACKGROUND_LAYER_1_COLOR));
        // the lane's head: its colour's dot, its name, how many
        let hy = lr.y0 + board::HEAD * s / 2.0; let mut x = lr.x0 + (board::LANE_PAD + 6.0) * s;
        scene.fill(peniko::Fill::NonZero, Affine::IDENTITY, hex(&data.color).unwrap_or(color(t::DISABLED_CONTENT_COLOR)), None, &kurbo::Circle::new((x + 4.0 * s, hy), 4.0 * s)); x += 16.0 * s;
        let name = fonts.layout_fit(&data.label, Family::Sans, (13.0 * s) as f32, 600.0, text, (board::LANE_W - 80.0) * s); draw_layout(scene, &name, Affine::translate((x, hy - name.height() as f64 / 2.0)));
        let n = fonts.layout(&data.cards.len().to_string(), Family::Sans, (12.0 * s) as f32, 400.0, sub); draw_layout(scene, &n, Affine::translate((lr.x1 - (board::LANE_PAD + 8.0) * s - n.width() as f64, hy - n.height() as f64 / 2.0)));
        for (k, (cr, card)) in cards.iter().zip(&data.cards).enumerate() {
            let r = at(*cr); if r.y1 < clip.y0 || r.y0 > clip.y1 { continue }
            // where a dragged card would land: a line of the accent colour above this card
            if drop == Some((i, k)) { fill(scene, Rect::new(r.x0, r.y0 - (board::CARD_GAP / 2.0 + 1.0) * s, r.x1, r.y0 - (board::CARD_GAP / 2.0 - 1.0) * s), s, color(t::ACCENT_BACKGROUND_COLOR_DEFAULT)); }
            let gone = lifted == Some(card.row);
            fill(scene, r, t::CORNER_RADIUS_MEDIUM_DEFAULT as f64 * s, color(if hover == Hit::Card(i, k) && !gone { t::ACTION_BUTTON_BACKGROUND_COLOR_HOVER } else { t::BACKGROUND_LAYER_2_COLOR }));
            if gone { continue }   // the card being dragged leaves its place empty
            if let Some(c) = hex(&card.edge) { scene.push_clip_layer(peniko::Fill::NonZero, Affine::IDENTITY, &RoundedRect::from_rect(r, t::CORNER_RADIUS_MEDIUM_DEFAULT as f64 * s)); fill(scene, Rect::new(r.x0, r.y0, r.x0 + 3.0 * s, r.y1), 0.0, c); scene.pop_layer(); }
            let mut y = r.y0; let (x0, w) = (r.x0 + board::CARD_PAD * s, r.width() - 2.0 * board::CARD_PAD * s);
            if let Some(d) = &card.cover { let ch = (board::LANE_W - 2.0 * board::LANE_PAD) * 9.0 / 16.0 * s; let pic = Rect::new(r.x0, r.y0, r.x1, r.y0 + ch);
                scene.push_clip_layer(peniko::Fill::NonZero, Affine::IDENTITY, &RoundedRect::from_rect(Rect::new(r.x0, r.y0, r.x1, r.y1), t::CORNER_RADIUS_MEDIUM_DEFAULT as f64 * s)); scene.push_clip_layer(peniko::Fill::NonZero, Affine::IDENTITY, &pic);
                let rec: serde_json::Value = serde_json::from_str(d).unwrap_or_default(); let (iw, ih) = (rec.get("w").and_then(|v| v.as_f64()).unwrap_or(16.0).max(1.0), rec.get("h").and_then(|v| v.as_f64()).unwrap_or(9.0).max(1.0));
                // the picture fills the cover's place (cropped to it about its middle)
                let k = (pic.width() / iw).max(pic.height() / ih); let (dw, dh) = (iw * k, ih * k); let dst = Rect::new(pic.center().x - dw / 2.0, pic.center().y - dh / 2.0, pic.center().x + dw / 2.0, pic.center().y + dh / 2.0);
                match cover(d, dw, dh) { Some(b) => min_ui::paint::image(scene, &b, dst), None => fill(scene, pic, 0.0, color(t::BACKGROUND_LAYER_1_COLOR)) }
                scene.pop_layer(); scene.pop_layer(); y += ch + 8.0 * s; } else { y += board::CARD_PAD * s; }
            let title = fonts.layout_fit(&card.title, Family::Sans, (13.0 * s) as f32, 500.0, text, w); draw_layout(scene, &title, Affine::translate((x0, y + (board::TITLE * s - title.height() as f64) / 2.0))); y += board::TITLE * s;
            for d in &card.details { let mid = y + board::LINE * s / 2.0;
                match d {
                    Detail::Text(v) => { let l = fonts.layout_fit(v, Family::Sans, (12.0 * s) as f32, 400.0, sub, w); draw_layout(scene, &l, Affine::translate((x0, mid - l.height() as f64 / 2.0))); }
                    Detail::Chip(v, c) => { let l = fonts.layout_fit(v, Family::Sans, (11.0 * s) as f32, 500.0, peniko::Color::WHITE, w - 14.0 * s); let pill = Rect::new(x0, mid - 8.0 * s, x0 + l.width() as f64 + 14.0 * s, mid + 8.0 * s);
                        fill(scene, pill, 8.0 * s, hex(c).unwrap_or(color(t::DISABLED_CONTENT_COLOR))); draw_layout(scene, &l, Affine::translate((x0 + 7.0 * s, mid - l.height() as f64 / 2.0))); }
                    Detail::Check(state, name) => { let bx = Rect::new(x0, mid - 6.0 * s, x0 + 12.0 * s, mid + 6.0 * s); let rr = RoundedRect::from_rect(bx, 3.0 * s); let acc = color(t::ACCENT_BACKGROUND_COLOR_DEFAULT);
                        match state { 0 => scene.stroke(&kurbo::Stroke::new(1.3 * s), Affine::IDENTITY, sub, None, &rr), 1 => { scene.stroke(&kurbo::Stroke::new(1.3 * s), Affine::IDENTITY, acc, None, &rr); fill(scene, Rect::new(bx.x0 + 3.0 * s, mid - s, bx.x1 - 3.0 * s, mid + s), 0.0, acc); } _ => scene.fill(peniko::Fill::NonZero, Affine::IDENTITY, acc, None, &rr) }
                        let l = fonts.layout_fit(name, Family::Sans, (12.0 * s) as f32, 400.0, sub, w - 20.0 * s); draw_layout(scene, &l, Affine::translate((x0 + 18.0 * s, mid - l.height() as f64 / 2.0))); }
                }
                y += board::LINE * s; }
        }
        // the foot: a card is added to this lane here; a card dragged past the last lands here
        let foot_y = cards.last().map(|c| c.y1 + board::CARD_GAP).unwrap_or(lane.y0 + board::HEAD); let fr = at(Rect::new(lane.x0 + board::LANE_PAD, foot_y, lane.x1 - board::LANE_PAD, foot_y + board::FOOT - 6.0));
        if drop == Some((i, cards.len())) { fill(scene, Rect::new(fr.x0, fr.y0 - (board::CARD_GAP / 2.0 + 1.0) * s, fr.x1, fr.y0 - (board::CARD_GAP / 2.0 - 1.0) * s), s, color(t::ACCENT_BACKGROUND_COLOR_DEFAULT)); }
        if hover == Hit::Add(i) { fill(scene, fr, t::CORNER_RADIUS_MEDIUM_DEFAULT as f64 * s, color(t::BACKGROUND_LAYER_2_COLOR)); }
        if let Some(g) = fonts.icon("plus", (14.0 * s) as f32, if hover == Hit::Add(i) { text } else { sub }) { draw_layout(scene, &g, Affine::translate((fr.center().x - g.width() as f64 / 2.0, fr.center().y - g.height() as f64 / 2.0))); }
    }
    scene.pop_layer();
}
