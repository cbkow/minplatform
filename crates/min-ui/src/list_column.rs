//! A list column (QCView's Color-panel columns — `ColorPanel.qml`'s scroll-and-pick lists): a caption, rows with optional
//! section headers and a right-hand tag, one selected row in the loaded-row treatment (canon row 162), hover, wheel
//! scrolling with the rails' overlay scrollbar, and a right-click that reports the row (the shell opens a menu). The rows sit
//! on a layer-2 card under their caption (Spectrum 2's layering by tone, as the rails' section tiles — canon row 174).
use crate::{anim::Ramp, paint, Button, Event, FieldResponse, Fonts, ImeEvent, KeyEvent, ScrollBar, TextField};
use kurbo::{Affine, Rect};
use min_theme::tokens as t;
use std::time::Instant;
use vello::Scene;

#[derive(Clone, Debug, PartialEq)]
pub enum ListRow {
    /// A section caption inside the list ("minColor", "Yours"); not selectable.
    Header(String),
    /// A pickable row: its label, a quiet tag at the right ("OCIO", "SDR"), dimmed (shown but not quite available).
    Item { label: String, tag: String, dim: bool },
}
impl ListRow {
    pub fn item(label: impl Into<String>) -> ListRow { ListRow::Item { label: label.into(), tag: String::new(), dim: false } }
    pub fn tagged(label: impl Into<String>, tag: impl Into<String>) -> ListRow { ListRow::Item { label: label.into(), tag: tag.into(), dim: false } }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ListResponse {
    /// a row was picked (primary press)
    pub clicked: Option<usize>,
    /// a row was right-clicked, at (x, y)
    pub context: Option<(usize, f64, f64)>,
    pub dirty: bool,
    pub consumed: bool,
}

pub struct ListColumn {
    pub title: String,
    rows: Vec<ListRow>,
    pub selected: Option<usize>,
    pub rect: Rect,
    /// a row the caller marks as "in use but not the pick" (a chain's OpenDRT look shown while the list offers others)
    sb: ScrollBar,
    /// QCView's per-column "Filter…" field (long lists: an OCIO config's inputs); `shown` = the rows that pass, by index
    filter: Option<TextField>,
    shown: Vec<usize>,
    /// the pick still has to be scrolled into view (set before the column had a height)
    reveal_pending: bool,
    hover: Option<usize>,
    hover_ramp: Ramp,
    scale: f64,
}

impl ListColumn {
    pub fn new(title: impl Into<String>) -> ListColumn { ListColumn { title: title.into(), rows: vec![], selected: None, rect: Rect::ZERO, sb: ScrollBar::default(), filter: None, shown: vec![], reveal_pending: false, hover: None, hover_ramp: Ramp::new(0.0, t::ANIMATION_DURATION_HOVER), scale: 1.0 } }
    pub fn rows(&self) -> &[ListRow] { &self.rows }
    /// Give the column a filter field (QCView's "Filter…").
    pub fn with_filter(mut self) -> ListColumn { let mut f = TextField::new(crate::Family::Sans, t::RAIL_LABEL_SIZE); f.placeholder = "Filter…".into(); self.filter = Some(f); self }
    pub fn set_filter_placeholder(&mut self, p: &str) { if let Some(f) = self.filter.as_mut() { f.placeholder = p.to_string(); } }
    fn refilter(&mut self) {
        let q = self.filter.as_ref().map(|f| f.text().trim().to_lowercase()).unwrap_or_default();
        self.shown = (0..self.rows.len()).filter(|&i| q.is_empty() || matches!(&self.rows[i], ListRow::Item { label, .. } if label.to_lowercase().contains(&q))).collect();
        self.sb.offset = 0.0; self.reveal_pending = true;
    }
    pub fn focused(&self) -> bool { self.filter.as_ref().is_some_and(|f| f.focused) }
    pub fn blur(&mut self) { if let Some(f) = self.filter.as_mut() { f.focused = false; } }
    /// Keys while the filter has focus; true when used. Escape clears the filter.
    pub fn key(&mut self, k: &KeyEvent, fonts: &mut Fonts) -> bool {
        let Some(f) = self.filter.as_mut() else { return false };
        if !f.focused { return false }
        match f.key(k, fonts) { FieldResponse::Changed => self.refilter(), FieldResponse::Cancelled => { f.clear(); f.focused = false; self.refilter(); } FieldResponse::Accepted => f.focused = false, FieldResponse::None => {} }
        true
    }
    pub fn ime(&mut self, e: &ImeEvent, fonts: &mut Fonts) { if let Some(f) = self.filter.as_mut() { if f.focused { f.ime(e, fonts); self.refilter(); } } }
    /// New rows; the scroll stays where it was when the list is the same length.
    pub fn set_rows(&mut self, rows: Vec<ListRow>) -> bool { if rows == self.rows { return false } if rows.len() != self.rows.len() { self.sb.offset = 0.0; } self.rows = rows; let off = self.sb.offset; self.refilter(); self.sb.offset = off; true }
    pub fn set_selected(&mut self, s: Option<usize>) -> bool { if self.selected == s { false } else { self.selected = s; self.reveal_pending = true; self.reveal_selected(); true } }
    fn row_h(&self) -> f64 { t::COMPONENT_HEIGHT_75 as f64 * self.scale }
    fn caption_h(&self) -> f64 { t::COMPONENT_HEIGHT_75 as f64 * self.scale }
    /// The card under the caption, and the rows' box inside its padding.
    pub fn card(&self) -> Rect { Rect::new(self.rect.x0, self.rect.y0 + self.caption_h(), self.rect.x1, self.rect.y1) }
    fn field_h(&self) -> f64 { if self.filter.is_some() { t::FIELD_HEIGHT_S as f64 * self.scale + t::SPACING_75 as f64 * self.scale } else { 0.0 } }
    fn body(&self) -> Rect { let p = t::SPACING_75 as f64 * self.scale; let c = self.card().inset(-p); Rect::new(c.x0, c.y0 + self.field_h(), c.x1, c.y1) }
    fn max_scroll(&self) -> f64 { (self.shown.len() as f64 * self.row_h() - self.body().height()).max(0.0) }
    fn reveal_selected(&mut self) {
        let Some(sel) = self.selected else { return };
        let Some(i) = self.shown.iter().position(|&k| k == sel) else { return };
        // not laid out yet (a zero rect inverts, and inset() then reports a small positive height): wait for the layout
        let c = self.card(); if c.y1 - c.y0 <= 2.0 * t::SPACING_75 as f64 * self.scale + self.row_h() { return }
        let (y0, y1) = (i as f64 * self.row_h(), (i + 1) as f64 * self.row_h()); let h = self.body().height();
        self.reveal_pending = false;
        if y0 < self.sb.offset { self.sb.offset = y0; } else if y1 > self.sb.offset + h { self.sb.offset = y1 - h; }
        self.sb.offset = self.sb.offset.clamp(0.0, self.max_scroll());
    }
    pub fn layout_with(&mut self, fonts: &mut Fonts, rect: Rect, scale: f64) {
        self.layout(rect, scale);
        let p = t::SPACING_75 as f64 * scale; let c = self.card().inset(-p);
        if let Some(f) = self.filter.as_mut() { f.layout(fonts, Rect::new(c.x0, c.y0, c.x1, c.y0 + t::FIELD_HEIGHT_S as f64 * scale), scale); }
    }
    pub fn layout(&mut self, rect: Rect, scale: f64) { self.rect = rect; self.scale = scale; let n = self.shown.len() as f64 * self.row_h(); let b = self.body(); self.sb.set(Rect::new(b.x0, b.y0, self.card().x1, b.y1), n, scale); self.sb.offset = self.sb.offset.clamp(0.0, self.max_scroll()); if self.reveal_pending { self.reveal_selected(); } }
    pub fn hit(&self, x: f64, y: f64) -> bool { self.rect.contains(kurbo::Point::new(x, y)) }
    fn row_at(&self, x: f64, y: f64) -> Option<usize> {
        let b = self.body();
        if !b.contains(kurbo::Point::new(x, y)) { return None }
        let i = ((y - b.y0 + self.sb.offset) / self.row_h()).floor() as usize;
        self.shown.get(i).copied()
    }
    fn pickable(&self, i: usize) -> bool { matches!(self.rows.get(i), Some(ListRow::Item { .. })) }
    pub fn event(&mut self, e: &Event, now: Instant, fonts: Option<&mut Fonts>) -> ListResponse {
        let mut r = ListResponse::default();
        if let (Some(f), Some(fonts)) = (self.filter.as_mut(), fonts) {
            if let Event::Down { x, y, .. } = *e { let on = f.hit(x, y); if on != f.focused { f.focused = on; r.dirty = true; } }
            if f.focused || matches!(e, Event::Move { .. }) { if f.pointer(e, fonts) { r.dirty = true; } }
            if e.pos().is_some_and(|(x, y)| f.hit(x, y)) && matches!(e, Event::Down { .. } | Event::Up { .. }) { r.consumed = true; return r }
        }
        let (moved, mine) = self.sb.event(e, now);
        if moved { r.dirty = true; }
        if mine { r.consumed = true; return r }
        let over = e.pos().and_then(|(x, y)| self.row_at(x, y)).filter(|i| self.pickable(*i));
        if over != self.hover { self.hover = over; self.hover_ramp = Ramp::new(0.0, t::ANIMATION_DURATION_HOVER); self.hover_ramp.set_target(1.0, now); r.dirty = true; }
        match *e {
            Event::Down { x, y, button } if self.hit(x, y) => {
                r.consumed = true;
                if let Some(i) = over {
                    match button { Button::Secondary => r.context = Some((i, x, y)), _ => { r.clicked = Some(i); if self.selected != Some(i) { self.selected = Some(i); r.dirty = true; } } }
                }
            }
            Event::Move { x, y } | Event::Up { x, y, .. } if self.hit(x, y) => r.consumed = matches!(e, Event::Up { .. }),
            _ => {}
        }
        r
    }
    /// The wheel over the column; true when it scrolled.
    pub fn wheel(&mut self, x: f64, y: f64, dy_px: f64) -> bool {
        if !self.hit(x, y) { return false }
        self.sb.wheel(dy_px, Instant::now())
    }
    pub fn animating(&self, now: Instant) -> bool { self.hover_ramp.animating(now) || self.sb.animating(now) }
    pub fn paint(&mut self, scene: &mut Scene, fonts: &mut Fonts, now: Instant) {
        let s = self.scale; let pad = t::SPACING_100 as f64 * s;
        let cap = fonts.detail(&self.title, t::RAIL_GROUP_HEADER_SIZE * s as f32, paint::color(t::RAIL_GROUP_HEADER_COLOR));
        crate::fonts::draw_layout(scene, &cap, Affine::translate((self.rect.x0 + pad / 2.0, self.rect.y0 + (self.caption_h() - cap.height() as f64) / 2.0)));
        let b = self.body();
        if b.height() <= 0.0 { return }
        paint::rounded(scene, self.card(), t::CORNER_RADIUS_500 as f64 * s, t::BACKGROUND_LAYER_2_COLOR);
        if let Some(f) = self.filter.as_mut() { f.paint(scene, fonts, s); }
        scene.push_clip_layer(peniko::Fill::NonZero, Affine::IDENTITY, &b);
        let rh = self.row_h(); let radius = t::CORNER_RADIUS_SMALL_DEFAULT as f64 * s;
        let first = (self.sb.offset / rh).floor() as usize;
        for (pos, &i) in self.shown.iter().enumerate().skip(first) {
            let y = b.y0 + pos as f64 * rh - self.sb.offset;
            if y > b.y1 { break }
            let r = Rect::new(b.x0, y, b.x1, y + rh);
            match &self.rows[i] {
                ListRow::Header(h) => {
                    let l = fonts.detail(h, t::RAIL_GROUP_HEADER_SIZE * s as f32, paint::color(t::RAIL_GROUP_HEADER_COLOR));
                    crate::fonts::draw_layout(scene, &l, Affine::translate((r.x0 + pad / 2.0, r.y0 + (rh - l.height() as f64) / 2.0)));
                }
                ListRow::Item { label, tag, dim } => {
                    let sel = self.selected == Some(i);
                    let bg = if sel { t::RAIL_ROW_ACTIVE_FILL } else if self.hover == Some(i) { crate::anim::mix(t::HOVER_BACKGROUND_COLOR_IDLE, t::HOVER_BACKGROUND_COLOR, self.hover_ramp.value(now)) } else { t::HOVER_BACKGROUND_COLOR_IDLE };
                    if bg.a > 0.0 { paint::rounded(scene, r.inset((0.0, -1.0 * s)), radius, bg); }
                    let c = if *dim { t::DISABLED_CONTENT_COLOR } else if sel { t::RAIL_ROW_ACTIVE_LABEL } else { t::RAIL_LABEL_COLOR };
                    let mut right = r.x1 - pad;
                    if !tag.is_empty() {
                        let tl = fonts.detail(tag, t::RAIL_GROUP_HEADER_SIZE * s as f32, paint::color(if sel { t::RAIL_ROW_ACTIVE_LABEL } else { t::RAIL_GROUP_HEADER_COLOR }));
                        let tx = right - tl.width() as f64; crate::fonts::draw_layout(scene, &tl, Affine::translate((tx, r.y0 + (rh - tl.height() as f64) / 2.0))); right = tx - pad;
                    }
                    let weight = if sel { t::FONT_WEIGHT_BOLD } else { t::FONT_WEIGHT_REGULAR } as f32;
                    let l = fonts.layout_width(label, crate::Family::Sans, t::RAIL_LABEL_SIZE * s as f32, weight, paint::color(c), None);
                    scene.push_clip_layer(peniko::Fill::NonZero, Affine::IDENTITY, &Rect::new(r.x0, r.y0, right, r.y1));
                    crate::fonts::draw_layout(scene, &l, Affine::translate((r.x0 + pad, r.y0 + (rh - l.height() as f64) / 2.0)));
                    scene.pop_layer();
                }
            }
        }
        scene.pop_layer();
        self.sb.paint(scene, now);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    impl ListColumn { fn event_t(&mut self, e: &Event, now: Instant) -> ListResponse { self.event(e, now, None) } }
    #[test]
    fn pick_context_headers_and_scroll() {
        let now = Instant::now();
        let mut l = ListColumn::new("Input");
        l.set_rows(vec![ListRow::Header("minColor".into()), ListRow::item("A"), ListRow::tagged("B", "OCIO"), ListRow::item("C")]);
        l.layout(Rect::new(0.0, 0.0, 200.0, 24.0 + 48.0 + 12.0), 1.0);   // caption + the card's 6-px padding + two rows
        let row_y = |i: usize| 24.0 + 6.0 + i as f64 * 24.0 + 12.0;
        assert_eq!(l.event_t(&Event::Down { x: 10.0, y: row_y(0), button: Button::Primary }, now).clicked, None, "a header is not pickable");
        let r = l.event_t(&Event::Down { x: 10.0, y: row_y(1), button: Button::Primary }, now);
        assert_eq!((r.clicked, l.selected, r.consumed), (Some(1), Some(1), true));
        assert_eq!(l.event_t(&Event::Down { x: 10.0, y: row_y(1), button: Button::Secondary }, now).context.map(|c| c.0), Some(1));
        assert!(l.wheel(10.0, 40.0, 100.0), "two rows of overflow scroll");
        let r = l.event_t(&Event::Down { x: 10.0, y: row_y(1), button: Button::Primary }, now);
        assert_eq!(r.clicked, Some(3), "scrolled by two rows (clamped)");
        l.set_selected(Some(1)); assert!(l.event_t(&Event::Move { x: 10.0, y: row_y(0) }, now).dirty);
    }
}
#[cfg(test)]
mod reveal_test {
    use super::*;
    #[test]
    fn the_pick_is_fully_inside_the_rows_box() {
        let mut l = ListColumn::new("T");
        l.set_rows((0..16).map(|i| ListRow::item(format!("r{i}"))).collect());
        l.set_selected(Some(14));
        l.layout(Rect::new(0.0, 0.0, 200.0, 300.0), 2.0);
        let b = l.body(); let rh = l.row_h();
        let bottom = b.y0 + 15.0 * rh - l.sb.offset;
        assert!(bottom <= b.y1 + 0.5, "row bottom {bottom} vs box bottom {} (offset {}, max {})", b.y1, l.sb.offset, l.max_scroll());
    }
}
#[cfg(test)]
mod filter_test {
    use super::*;
    #[test]
    fn the_filter_keeps_matching_items_and_picks_by_original_index() {
        let now = Instant::now();
        let mut l = ListColumn::new("Input").with_filter();
        l.set_rows(vec![ListRow::Header("ACES".into()), ListRow::item("ACEScg"), ListRow::item("ACES2065-1"), ListRow::item("Linear Rec.709"), ListRow::item("sRGB")]);
        l.filter.as_mut().unwrap().set_text("rec"); l.refilter();
        assert_eq!(l.shown, vec![3], "case-insensitive substring, headers hidden");
        l.layout(Rect::new(0.0, 0.0, 200.0, 200.0), 1.0);
        let y = l.body().y0 + 12.0;
        assert_eq!(l.event(&Event::Down { x: 20.0, y, button: Button::Primary }, now, None).clicked, Some(3));
    }
}
