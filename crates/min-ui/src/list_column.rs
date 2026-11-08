//! A list column (QCView's Color-panel columns — `ColorPanel.qml`'s scroll-and-pick lists): a caption, rows with optional
//! section headers and a right-hand tag, one selected row in the loaded-row treatment (canon row 162), hover, wheel
//! scrolling, and a right-click that reports the row (the shell opens a menu). Painting is clipped to the column.
use crate::{anim::Ramp, paint, Button, Event, Fonts};
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
    scroll: f64,
    /// the pick still has to be scrolled into view (set before the column had a height)
    reveal_pending: bool,
    hover: Option<usize>,
    hover_ramp: Ramp,
    scale: f64,
}

impl ListColumn {
    pub fn new(title: impl Into<String>) -> ListColumn { ListColumn { title: title.into(), rows: vec![], selected: None, rect: Rect::ZERO, scroll: 0.0, reveal_pending: false, hover: None, hover_ramp: Ramp::new(0.0, t::ANIMATION_DURATION_HOVER), scale: 1.0 } }
    pub fn rows(&self) -> &[ListRow] { &self.rows }
    /// New rows; the scroll stays where it was when the list is the same length.
    pub fn set_rows(&mut self, rows: Vec<ListRow>) -> bool { if rows == self.rows { return false } if rows.len() != self.rows.len() { self.scroll = 0.0; } self.rows = rows; true }
    pub fn set_selected(&mut self, s: Option<usize>) -> bool { if self.selected == s { false } else { self.selected = s; self.reveal_pending = true; self.reveal_selected(); true } }
    fn row_h(&self) -> f64 { t::COMPONENT_HEIGHT_75 as f64 * self.scale }
    fn caption_h(&self) -> f64 { t::COMPONENT_HEIGHT_75 as f64 * self.scale }
    fn body(&self) -> Rect { Rect::new(self.rect.x0, self.rect.y0 + self.caption_h(), self.rect.x1, self.rect.y1) }
    fn max_scroll(&self) -> f64 { (self.rows.len() as f64 * self.row_h() - self.body().height()).max(0.0) }
    fn reveal_selected(&mut self) {
        let Some(i) = self.selected else { return };
        let (y0, y1) = (i as f64 * self.row_h(), (i + 1) as f64 * self.row_h()); let h = self.body().height();
        if h <= 0.0 { return }
        self.reveal_pending = false;
        if y0 < self.scroll { self.scroll = y0; } else if y1 > self.scroll + h { self.scroll = y1 - h; }
        self.scroll = self.scroll.clamp(0.0, self.max_scroll());
    }
    pub fn layout(&mut self, rect: Rect, scale: f64) { self.rect = rect; self.scale = scale; self.scroll = self.scroll.clamp(0.0, self.max_scroll()); if self.reveal_pending { self.reveal_selected(); } }
    pub fn hit(&self, x: f64, y: f64) -> bool { self.rect.contains(kurbo::Point::new(x, y)) }
    fn row_at(&self, x: f64, y: f64) -> Option<usize> {
        let b = self.body();
        if !b.contains(kurbo::Point::new(x, y)) { return None }
        let i = ((y - b.y0 + self.scroll) / self.row_h()).floor() as usize;
        (i < self.rows.len()).then_some(i)
    }
    fn pickable(&self, i: usize) -> bool { matches!(self.rows.get(i), Some(ListRow::Item { .. })) }
    pub fn event(&mut self, e: &Event, now: Instant) -> ListResponse {
        let mut r = ListResponse::default();
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
        let s = (self.scroll + dy_px).clamp(0.0, self.max_scroll());
        if (s - self.scroll).abs() < 0.5 { return false }
        self.scroll = s; true
    }
    pub fn animating(&self, now: Instant) -> bool { self.hover_ramp.animating(now) }
    pub fn paint(&self, scene: &mut Scene, fonts: &mut Fonts, now: Instant) {
        let s = self.scale; let pad = t::SPACING_100 as f64 * s;
        let cap = fonts.detail(&self.title, t::RAIL_GROUP_HEADER_SIZE * s as f32, paint::color(t::RAIL_GROUP_HEADER_COLOR));
        crate::fonts::draw_layout(scene, &cap, Affine::translate((self.rect.x0 + pad / 2.0, self.rect.y0 + (self.caption_h() - cap.height() as f64) / 2.0)));
        let b = self.body();
        if b.height() <= 0.0 { return }
        scene.push_clip_layer(peniko::Fill::NonZero, Affine::IDENTITY, &b);
        let rh = self.row_h(); let radius = t::CORNER_RADIUS_SMALL_DEFAULT as f64 * s;
        let first = (self.scroll / rh).floor() as usize;
        for i in first..self.rows.len() {
            let y = b.y0 + i as f64 * rh - self.scroll;
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
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pick_context_headers_and_scroll() {
        let now = Instant::now();
        let mut l = ListColumn::new("Input");
        l.set_rows(vec![ListRow::Header("minColor".into()), ListRow::item("A"), ListRow::tagged("B", "OCIO"), ListRow::item("C")]);
        l.layout(Rect::new(0.0, 0.0, 200.0, 24.0 + 48.0), 1.0);   // caption + two rows visible
        let row_y = |i: usize| 24.0 + i as f64 * 24.0 + 12.0;
        assert_eq!(l.event(&Event::Down { x: 10.0, y: row_y(0), button: Button::Primary }, now).clicked, None, "a header is not pickable");
        let r = l.event(&Event::Down { x: 10.0, y: row_y(1), button: Button::Primary }, now);
        assert_eq!((r.clicked, l.selected, r.consumed), (Some(1), Some(1), true));
        assert_eq!(l.event(&Event::Down { x: 10.0, y: row_y(1), button: Button::Secondary }, now).context.map(|c| c.0), Some(1));
        assert!(l.wheel(10.0, 40.0, 100.0), "two rows of overflow scroll");
        let r = l.event(&Event::Down { x: 10.0, y: row_y(1), button: Button::Primary }, now);
        assert_eq!(r.clicked, Some(3), "scrolled by two rows (clamped)");
        l.set_selected(Some(1)); assert!(l.event(&Event::Move { x: 10.0, y: row_y(0) }, now).dirty);
    }
}
