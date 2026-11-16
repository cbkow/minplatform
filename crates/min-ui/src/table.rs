//! Table - a virtualised table of rows (UFB's list, canon rows 451 / 463; spike S17's way): a header whose titles
//! sort and whose dividers resize, rows of one height drawn only while they are in view, the overlay [`ScrollBar`],
//! hover and selection, and in the first column an indent, a disclosure triangle and an icon - for rows that open in
//! place. The table does NOT own the rows: it asks a [`TableSource`] for what each row in view shows, every frame
//! (S17: laying a row's text out afresh costs less than keeping it).
//!
//! The columns: the FIRST takes the width the others leave; the others keep their own width (logical px). A divider
//! dragged between two of the others trades width between them; the divider after the first column gives to or takes
//! from the first. When the table is too narrow for the first column's minimum, columns are left out from the right.
//!
//! Pointer: a press chooses (see [`Selection`]), a second press in the same place activates, a press on the triangle
//! opens or closes the row, the secondary button asks for a context menu (the menu is the owner's). A press is the
//! table's only when the pointer is ON it. Keys: Up / Down, Page Up / Down, Home / End (Shift extends), Right opens a
//! closed row, Left closes an open one or goes to the row it lies in, Return activates, the command key with A
//! chooses all, and typed letters jump to the first row whose first column starts with them.
use crate::selection::{starts_with_ci, DoubleClick, Mods, Nav, Selection, ViewEvent};
use crate::{fonts::draw_layout, paint, Button, Event, Family, Fonts, Key, KeyEvent, Named, ScrollBar};
use kurbo::{Affine, Rect};
use min_theme::tokens as t;
use std::borrow::Cow;
use std::time::Instant;
use vello::Scene;

#[derive(Clone, Debug, PartialEq)]
pub struct Column { pub title: String, /// logical px (the first column's is not used: it takes what is left)
    pub width: f64, pub min: f64, /// the text ends at the column's right edge (a size)
    pub right: bool }
impl Column { pub fn new(title: &str, width: f64, min: f64) -> Column { Column { title: title.to_string(), width, min, right: false } } pub fn right(mut self) -> Column { self.right = true; self } }

/// What a row is, beside its texts.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RowInfo { /// how deep it lies inside rows opened in place
    pub indent: u16, /// it has a disclosure triangle
    pub expandable: bool, pub expanded: bool, /// drawn in the quiet colour (a hidden file)
    pub dim: bool, /// a line of text, not an entry: no triangle, no icon ("Loading..." under a row opened in place)
    pub plain: bool }

/// The rows, as the owner has them. Asked only for rows in view (and, for type-ahead, for first-column texts).
pub trait TableSource {
    fn len(&self) -> usize;
    fn row(&self, i: usize) -> RowInfo;
    fn text(&self, i: usize, col: usize) -> Cow<'_, str>;
    /// The row's icon; none draws the neutral placeholder.
    fn icon(&self, _i: usize) -> Option<&peniko::ImageBrush> { None }
}

/// Logical px.
pub const ROW_HEIGHT: f64 = 24.0; pub const HEADER_HEIGHT: f64 = 24.0;
/// How far (logical px) a press moves before it is a drag.
pub const DRAG_SLOP: f64 = 6.0;
const INDENT: f64 = 16.0; const SLOT: f64 = 16.0; const ICON: f64 = 16.0; const PAD: f64 = 8.0; const GRIP: f64 = 4.0; const GUTTER: f64 = 12.0;

/// Where the columns lie in a width of `total`: (x0, x1) for each column SHOWN. The first takes what the others
/// leave; columns are left out from the right while that is less than the first's minimum.
pub fn layout_columns(widths: &[f64], mins: &[f64], total: f64) -> Vec<(f64, f64)> {
    if widths.is_empty() { return vec![] }
    let mut shown = widths.len();
    while shown > 1 && total - widths[1..shown].iter().sum::<f64>() < mins[0] { shown -= 1; }
    let first = (total - widths[1..shown].iter().sum::<f64>()).max(0.0);
    let mut out = Vec::with_capacity(shown); let mut x = 0.0;
    for i in 0..shown { let w = if i == 0 { first } else { widths[i] }; out.push((x, x + w)); x += w; }
    out
}
/// The widths after the divider BEFORE column `divider` (1..shown) was dragged by `dx` from the widths `start`: the
/// divider follows the pointer until a column on either side is at its minimum.
pub fn drag_divider(start: &[f64], mins: &[f64], total: f64, shown: usize, divider: usize, dx: f64) -> Vec<f64> {
    let mut w = start.to_vec();
    if divider == 0 || divider >= shown.min(w.len()) { return w }
    let give = (start[divider] - mins[divider]).max(0.0);
    if divider == 1 {
        let first = total - start[1..shown].iter().sum::<f64>();
        let dx = dx.clamp(-(first - mins[0]).max(0.0), give); w[1] = start[1] - dx;
    } else {
        let dx = dx.clamp(-(start[divider - 1] - mins[divider - 1]).max(0.0), give); w[divider - 1] = start[divider - 1] + dx; w[divider] = start[divider] - dx;
    }
    w
}
/// The rows in view: the first, and one past the last.
pub fn visible_rows(offset: f64, view_h: f64, row_h: f64, n: usize) -> (usize, usize) {
    if row_h <= 0.0 || view_h <= 0.0 { return (0, 0) }
    let first = ((offset.max(0.0) / row_h) as usize).min(n);
    (first, (((offset.max(0.0) + view_h) / row_h).ceil() as usize).min(n))
}
/// The scroll offset that brings row `i` wholly into view (unchanged when it is).
pub fn reveal_offset(offset: f64, view_h: f64, row_h: f64, i: usize) -> f64 {
    let (top, bottom) = (i as f64 * row_h, (i + 1) as f64 * row_h);
    if top < offset { top } else if bottom > offset + view_h { bottom - view_h } else { offset }
}

pub struct Table {
    pub columns: Vec<Column>,
    /// The column the rows are sorted by, and whether ascending (the owner sorts; the header shows it).
    pub sort: (usize, bool),
    pub sel: Selection,
    pub bar: ScrollBar,
    pub rect: Rect,
    /// The table holds the keyboard: its selection is drawn in the stronger colour.
    pub focused: bool,
    rows: usize, scale: f64, cols: Vec<(f64, f64)>,
    hover: Option<usize>, hover_head: Option<usize>, hover_div: Option<usize>, press_head: Option<usize>,
    /// the divider in hand: its column, the pointer's x at the press, the widths then
    drag: Option<(usize, f64, Vec<f64>)>, dbl: DoubleClick,
    /// a primary press on a row that may become a drag: (the row, where, whether it was already chosen)
    press: Option<(usize, f64, f64, bool)>,
}

impl Table {
    pub fn new(columns: Vec<Column>) -> Table { Table { columns, sort: (0, true), sel: Selection::new(), bar: ScrollBar::default(), rect: Rect::ZERO, focused: false, rows: 0, scale: 1.0, cols: vec![], hover: None, hover_head: None, hover_div: None, press_head: None, drag: None, dbl: DoubleClick::default(), press: None } }
    fn widths(&self) -> Vec<f64> { self.columns.iter().map(|c| c.width).collect() }
    fn mins(&self) -> Vec<f64> { self.columns.iter().map(|c| c.min).collect() }
    /// The width the columns share, logical px (the scroll bar's gutter is left free).
    fn total(&self) -> f64 { (self.rect.width() / self.scale - GUTTER).max(0.0) }
    /// Place the table and tell it how many rows there are (every frame, before `event` / `paint`).
    pub fn layout(&mut self, rect: Rect, rows: usize, scale: f64) {
        self.rect = rect; self.rows = rows; self.scale = scale; self.sel.resize(rows);
        self.cols = layout_columns(&self.widths(), &self.mins(), self.total()).into_iter().map(|(a, b)| (rect.x0 + a * scale, rect.x0 + b * scale)).collect();
        self.bar.set(self.body(), rows as f64 * ROW_HEIGHT * scale, scale);
        if self.hover.is_some_and(|h| h >= rows) { self.hover = None; }
    }
    pub fn header(&self) -> Rect { Rect::new(self.rect.x0, self.rect.y0, self.rect.x1, (self.rect.y0 + HEADER_HEIGHT * self.scale).min(self.rect.y1)) }
    pub fn body(&self) -> Rect { Rect::new(self.rect.x0, self.header().y1, self.rect.x1, self.rect.y1) }
    pub fn hit(&self, x: f64, y: f64) -> bool { self.rect.contains(kurbo::Point::new(x, y)) }
    /// A divider is under the pointer or in hand (the owner shows the resize cursor).
    pub fn resizing(&self) -> bool { self.hover_div.is_some() || self.drag.is_some() }
    pub fn hovered_row(&self) -> Option<usize> { self.hover }
    fn rh(&self) -> f64 { ROW_HEIGHT * self.scale }
    pub fn row_at(&self, x: f64, y: f64) -> Option<usize> {
        let b = self.body(); if !b.contains(kurbo::Point::new(x, y)) { return None }
        let i = ((y - b.y0 + self.bar.offset) / self.rh()) as usize; (i < self.rows).then_some(i)
    }
    /// Where the NAME of row `i` is written (the first column, after its indent, its triangle and its icon): the place
    /// of a field that renames it.
    pub fn name_rect(&self, i: usize, info: &RowInfo) -> Rect {
        let r = self.row_rect(i); let s = self.scale; let (x0, x1) = self.cols.first().copied().unwrap_or((r.x0, r.x1));
        let x = x0 + PAD * s + info.indent as f64 * INDENT * s + SLOT * s + if info.plain { 0.0 } else { (ICON + 6.0) * s };
        Rect::new(x.min(x1), r.y0, x1.max(x), r.y1)
    }
    /// Where row `i` lies now (it may be out of view).
    pub fn row_rect(&self, i: usize) -> Rect { let b = self.body(); let y = b.y0 + i as f64 * self.rh() - self.bar.offset; Rect::new(b.x0, y, b.x1, y + self.rh()) }
    fn page(&self) -> usize { ((self.body().height() / self.rh()) as usize).saturating_sub(1).max(1) }
    /// Scroll so that row `i` is in view.
    pub fn reveal(&mut self, i: usize) { self.bar.offset = reveal_offset(self.bar.offset, self.body().height(), self.rh(), i).clamp(0.0, self.bar.max()); }
    pub fn wheel(&mut self, dy: f64, now: Instant) -> bool { self.bar.wheel(dy, now) }
    fn head_at(&self, x: f64, y: f64) -> Option<usize> { if !self.header().contains(kurbo::Point::new(x, y)) { return None } self.cols.iter().position(|(a, b)| x >= *a && x < *b) }
    fn div_at(&self, x: f64, y: f64) -> Option<usize> { if !self.header().contains(kurbo::Point::new(x, y)) { return None } let g = GRIP * self.scale; (1..self.cols.len()).find(|d| (x - self.cols[*d].0).abs() <= g) }
    /// The disclosure triangle's slot in row `i` (window px).
    fn triangle(&self, i: usize, indent: u16) -> Rect { let r = self.row_rect(i); let x = self.cols.first().map(|c| c.0).unwrap_or(r.x0) + (PAD + indent as f64 * INDENT) * self.scale; Rect::new(x, r.y0, x + SLOT * self.scale, r.y1) }

    /// A pointer event. `src` are the rows as laid out. Returns what happened and whether a repaint is needed.
    pub fn event(&mut self, e: &Event, src: &dyn TableSource, mods: Mods, now: Instant) -> (Vec<ViewEvent>, bool) {
        let mut out = vec![];
        let (moved, mine) = self.bar.event(e, now); if mine { return (out, true) }
        if let Some((d, x0, start)) = self.drag.clone() {
            match *e {
                Event::Move { x, .. } => { let w = drag_divider(&start, &self.mins(), self.total(), self.cols.len(), d, (x - x0) / self.scale); for (c, w) in self.columns.iter_mut().zip(w) { c.width = w; } }
                Event::Up { button: Button::Primary, .. } | Event::Leave => self.drag = None,
                _ => {}
            }
            return (out, true)
        }
        let before = (self.hover, self.hover_head, self.hover_div, self.press_head);
        match *e {
            Event::Move { x, y } => { self.hover_div = self.div_at(x, y); self.hover_head = if self.hover_div.is_some() { None } else { self.head_at(x, y) }; self.hover = self.row_at(x, y); }
            _ => {}
        }
        if matches!(e, Event::Down { .. }) { self.press = None; }
        // a press on a row, moved: the drag begins (once)
        if let (Event::Move { x, y }, Some((i, x0, y0, _))) = (*e, self.press) { if (x - x0).hypot(y - y0) > DRAG_SLOP * self.scale { self.press = None; out.push(ViewEvent::DragStart(i)); } }
        match *e {
            Event::Move { .. } => {}
            Event::Leave => { self.hover = None; self.hover_head = None; self.hover_div = None; self.press_head = None; }
            Event::Down { x, y, button } if self.hit(x, y) => {
                if let (Some(d), Button::Primary) = (self.div_at(x, y), button) { self.drag = Some((d, x, self.widths())); return (out, true) }
                if let Some(c) = self.head_at(x, y) { if button == Button::Primary { self.press_head = Some(c); } }
                else if self.body().contains(kurbo::Point::new(x, y)) {
                    match (self.row_at(x, y), button) {
                        (Some(i), Button::Primary) => {
                            let info = src.row(i);
                            if info.expandable && !info.plain && self.triangle(i, info.indent).contains(kurbo::Point::new(x, y)) { self.dbl.forget(); out.push(if info.expanded { ViewEvent::Collapse(i) } else { ViewEvent::Expand(i) }); }
                            else {
                                // a plain press on a row that is ALREADY chosen keeps the choice - a drag carries all of it; let go without moving, that row alone is chosen
                                let was = self.sel.is_selected(i) && !mods.shift && !mods.cmd;
                                if !was && self.sel.click(i, mods) { out.push(ViewEvent::SelectionChanged); }
                                if !mods.shift && !mods.cmd { self.press = Some((i, x, y, was)); }
                                if self.dbl.press(i, (x, y), now, self.scale) && !mods.shift && !mods.cmd { out.push(ViewEvent::Activated(i)); }
                            }
                        }
                        (Some(i), Button::Secondary) => { if !self.sel.is_selected(i) && self.sel.select_only(i) { out.push(ViewEvent::SelectionChanged); } out.push(ViewEvent::ContextMenu(Some(i), (x, y))); }
                        (None, Button::Primary) => { self.dbl.forget(); if !mods.shift && !mods.cmd && self.sel.clear() { out.push(ViewEvent::SelectionChanged); } }
                        (None, Button::Secondary) => out.push(ViewEvent::ContextMenu(None, (x, y))),
                        _ => {}
                    }
                }
            }
            Event::Up { x, y, button: Button::Primary } => {
                if let Some((i, _, _, was)) = self.press.take() { if was && self.sel.select_only(i) { out.push(ViewEvent::SelectionChanged); } }
                if let Some(c) = self.press_head.take() { if self.head_at(x, y) == Some(c) { self.sort = if self.sort.0 == c { (c, !self.sort.1) } else { (c, true) }; out.push(ViewEvent::SortChanged(self.sort.0, self.sort.1)); } }
            }
            _ => {}
        }
        let repaint = moved || !out.is_empty() || before != (self.hover, self.hover_head, self.hover_div, self.press_head);
        (out, repaint)
    }

    /// A key. Returns what happened and whether the key was the table's.
    pub fn key(&mut self, k: &KeyEvent, src: &dyn TableSource, now: Instant) -> (Vec<ViewEvent>, bool) {
        let mut out = vec![];
        let nav = match &k.key {
            Key::Named(Named::Up) => Some(Nav::Up), Key::Named(Named::Down) => Some(Nav::Down), Key::Named(Named::Home) => Some(Nav::Home), Key::Named(Named::End) => Some(Nav::End),
            Key::Char(c) if c == "PageUp" => Some(Nav::PageUp), Key::Char(c) if c == "PageDown" => Some(Nav::PageDown), _ => None };
        if let Some(nav) = nav {
            if k.cmd { return (out, false) }
            if let Some(to) = self.sel.nav(nav, k.shift, 1, self.page()) { self.reveal(to); out.push(ViewEvent::SelectionChanged); }
            return (out, true)
        }
        match &k.key {
            Key::Named(Named::Right) if !k.cmd => { if let Some(c) = self.sel.cursor { let info = src.row(c); if info.expandable && !info.expanded { out.push(ViewEvent::Expand(c)); } } (out, true) }
            Key::Named(Named::Left) if !k.cmd => {
                if let Some(c) = self.sel.cursor { let info = src.row(c);
                    if info.expandable && info.expanded { out.push(ViewEvent::Collapse(c)); }
                    else if info.indent > 0 { if let Some(p) = (0..c).rev().find(|i| src.row(*i).indent < info.indent) { if self.sel.select_only(p) { out.push(ViewEvent::SelectionChanged); } self.reveal(p); } } }
                (out, true)
            }
            Key::Named(Named::Enter) if !k.cmd => { if let Some(c) = self.sel.cursor.filter(|c| self.sel.is_selected(*c)) { out.push(ViewEvent::Activated(c)); } (out, true) }
            Key::Char(c) if k.cmd && c.eq_ignore_ascii_case("a") && !k.shift => { if self.sel.select_all() { out.push(ViewEvent::SelectionChanged); } (out, true) }
            _ if !k.cmd => match k.text.as_deref().filter(|t| !t.is_empty() && !t.chars().any(char::is_control) && (*t != " " || !self.sel.typed(now).is_empty())) {
                Some(text) => { let before = self.sel.cursor; if let Some(hit) = self.sel.type_ahead(text, now, |i, w| starts_with_ci(&src.text(i, 0), w)) { self.reveal(hit); if before != Some(hit) { out.push(ViewEvent::SelectionChanged); } } (out, true) }
                None => (out, false),
            },
            _ => (out, false),
        }
    }
    pub fn animating(&self, now: Instant) -> bool { self.bar.animating(now) }

    pub fn paint(&self, scene: &mut Scene, fonts: &mut Fonts, src: &dyn TableSource, now: Instant) {
        let s = self.scale; if self.rect.width() <= 0.0 || self.rect.height() <= 0.0 { return }
        let (ink, sub) = (paint::color(t::NEUTRAL_CONTENT_COLOR_DEFAULT), paint::color(t::NEUTRAL_SUBDUED_CONTENT_COLOR_DEFAULT));
        let (head, body) = (self.header(), self.body()); let pad = PAD * s;
        scene.push_clip_layer(peniko::Fill::NonZero, Affine::IDENTITY, &self.rect);
        // -- the header: the titles, the sort's arrow, the dividers
        for (c, (x0, x1)) in self.cols.iter().enumerate() {
            let col = &self.columns[c]; let sorted = self.sort.0 == c; let lit = sorted || self.hover_head == Some(c) || self.press_head == Some(c);
            let arrow = if sorted { fonts.icon(if self.sort.1 { "caret-up" } else { "caret-down" }, 10.0 * s as f32, ink) } else { None };
            let room = x1 - x0 - 2.0 * pad - arrow.as_ref().map(|a| a.width() as f64 + 4.0 * s).unwrap_or(0.0);
            if room > 4.0 {
                let l = fonts.layout_fit(&col.title, Family::Sans, t::DETAIL_SIZE_M * s as f32, if sorted { t::FONT_WEIGHT_BOLD } else { t::FONT_WEIGHT_REGULAR } as f32, if lit { ink } else { sub }, room);
                let tx = if col.right { x1 - pad - l.width() as f64 - arrow.as_ref().map(|a| a.width() as f64 + 4.0 * s).unwrap_or(0.0) } else { x0 + pad };
                draw_layout(scene, &l, Affine::translate((tx, head.y0 + (head.height() - l.height() as f64) / 2.0)));
                if let Some(a) = &arrow { draw_layout(scene, a, Affine::translate((tx + l.width() as f64 + 4.0 * s, head.y0 + (head.height() - a.height() as f64) / 2.0))); }
            }
            if c > 0 { let hot = self.hover_div == Some(c) || self.drag.as_ref().is_some_and(|d| d.0 == c); let inset = if hot { 0.0 } else { 5.0 * s };
                paint::rect(scene, Rect::new(x0 - s / 2.0, head.y0 + inset, x0 + s / 2.0 + if hot { s } else { 0.0 }, head.y1 - inset), if hot { t::RAIL_SPLITTER_COLOR_HOVER } else { t::DIVIDER_COLOR_MEDIUM }); }
        }
        paint::hline(scene, head.x0, head.x1, head.y1 - s, s, t::DIVIDER_COLOR_MEDIUM);
        scene.pop_layer();
        if body.height() <= 0.0 { return }
        // -- the rows in view
        scene.push_clip_layer(peniko::Fill::NonZero, Affine::IDENTITY, &body);
        let rh = self.rh(); let (first, end) = visible_rows(self.bar.offset, body.height(), rh, self.rows.min(src.len())); let px = t::BODY_SIZE_S * s as f32;
        for i in first..end {
            let r = self.row_rect(i); let info = src.row(i); let selected = self.sel.is_selected(i);
            if selected { paint::rect(scene, r, if self.focused { t::SELECTION_BACKGROUND_COLOR_EMPHASIZED } else { t::SELECTION_BACKGROUND_COLOR }); }
            else if self.hover == Some(i) { paint::rect(scene, r, t::HOVER_BACKGROUND_COLOR); }
            for (c, (x0, x1)) in self.cols.iter().enumerate() {
                let mut x = x0 + pad;
                if c == 0 {
                    x += info.indent as f64 * INDENT * s;
                    if info.expandable { if let Some(g) = fonts.icon(if info.expanded { "caret-down" } else { "caret-right" }, 11.0 * s as f32, sub) { draw_layout(scene, &g, Affine::translate((x + (SLOT * s - g.width() as f64) / 2.0, r.y0 + (rh - g.height() as f64) / 2.0))); } }
                    x += SLOT * s;
                    if !info.plain {
                        let ir = Rect::new(x, r.y0 + (rh - ICON * s) / 2.0, x + ICON * s, r.y0 + (rh + ICON * s) / 2.0);
                        if x1 - ir.x1 > 0.0 { match src.icon(i) { Some(img) => paint::image(scene, img, ir), None => paint::rounded(scene, ir.inflate(-1.5 * s, -0.5 * s), 2.0 * s, t::GRAY_300) } }
                        x += (ICON + 6.0) * s;
                    }
                }
                let room = x1 - x - pad; if room < 6.0 * s { continue }
                let text = src.text(i, c); if text.is_empty() { continue }
                // (on the strong selection colour every cell is in the full ink: the quiet grey does not read there)
                let l = fonts.layout_fit(&text, Family::Sans, px, t::FONT_WEIGHT_REGULAR as f32, if (c == 0 && !info.dim) || (selected && self.focused) { ink } else { sub }, room);
                let tx = if self.columns[c].right { x1 - pad - l.width() as f64 } else { x };
                draw_layout(scene, &l, Affine::translate((tx, r.y0 + (rh - l.height() as f64) / 2.0)));
            }
        }
        scene.pop_layer();
        self.bar.paint(scene, now);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Rows(Vec<(&'static str, u16, bool, bool)>);
    impl TableSource for Rows {
        fn len(&self) -> usize { self.0.len() }
        fn row(&self, i: usize) -> RowInfo { let r = self.0[i]; RowInfo { indent: r.1, expandable: r.2, expanded: r.3, dim: false, plain: false } }
        fn text(&self, i: usize, col: usize) -> Cow<'_, str> { if col == 0 { Cow::Borrowed(self.0[i].0) } else { Cow::Borrowed("") } }
    }
    fn table(rows: usize) -> Table { let mut t = Table::new(vec![Column::new("Name", 0.0, 120.0), Column::new("Date Modified", 150.0, 60.0), Column::new("Size", 80.0, 50.0).right(), Column::new("Kind", 140.0, 60.0)]); t.layout(Rect::new(0.0, 0.0, 812.0, 24.0 + 240.0), rows, 1.0); t }
    fn key(k: Key) -> KeyEvent { KeyEvent { key: k, text: None, shift: false, cmd: false } }
    const NONE: Mods = Mods { shift: false, cmd: false };

    #[test]
    fn the_first_column_takes_what_the_others_leave() {
        let (w, m) = ([0.0, 150.0, 80.0, 140.0], [120.0, 60.0, 50.0, 60.0]);
        assert_eq!(layout_columns(&w, &m, 800.0), [(0.0, 430.0), (430.0, 580.0), (580.0, 660.0), (660.0, 800.0)]);
        assert_eq!(layout_columns(&w, &m, 490.0), [(0.0, 120.0), (120.0, 270.0), (270.0, 350.0), (350.0, 490.0)], "exactly the minimum: all four");
        assert_eq!(layout_columns(&w, &m, 400.0), [(0.0, 170.0), (170.0, 320.0), (320.0, 400.0)], "too narrow: the last column is left out");
        assert_eq!(layout_columns(&w, &m, 200.0), [(0.0, 200.0)]);
        assert_eq!(layout_columns(&w, &m, 50.0), [(0.0, 50.0)], "the first column is never left out");
        assert!(layout_columns(&[], &[], 100.0).is_empty());
    }
    #[test]
    fn a_dragged_divider_trades_width_and_stops_at_the_minimums() {
        let (w, m) = ([0.0, 150.0, 80.0, 140.0], [120.0, 60.0, 50.0, 60.0]);
        // the divider after the first column: the first grows, the second shrinks - to its minimum at most
        assert_eq!(drag_divider(&w, &m, 800.0, 4, 1, 40.0), [0.0, 110.0, 80.0, 140.0]);
        assert_eq!(drag_divider(&w, &m, 800.0, 4, 1, 500.0), [0.0, 60.0, 80.0, 140.0]);
        // to the left the first column gives, down to ITS minimum (430 - 120 = 310 to give)
        assert_eq!(drag_divider(&w, &m, 800.0, 4, 1, -100.0), [0.0, 250.0, 80.0, 140.0]);
        assert_eq!(drag_divider(&w, &m, 800.0, 4, 1, -900.0), [0.0, 460.0, 80.0, 140.0]);
        // between two of the others: one gives what the other takes, the sum stays
        assert_eq!(drag_divider(&w, &m, 800.0, 4, 2, 20.0), [0.0, 170.0, 60.0, 140.0]);
        assert_eq!(drag_divider(&w, &m, 800.0, 4, 2, 200.0), [0.0, 180.0, 50.0, 140.0]);
        assert_eq!(drag_divider(&w, &m, 800.0, 4, 3, -500.0), [0.0, 150.0, 50.0, 170.0]);
        // no such divider: nothing changes
        assert_eq!(drag_divider(&w, &m, 800.0, 4, 0, 30.0), w); assert_eq!(drag_divider(&w, &m, 400.0, 3, 3, 30.0), w);
    }
    #[test]
    fn only_the_rows_in_view() {
        assert_eq!(visible_rows(0.0, 240.0, 24.0, 100_000), (0, 10));
        assert_eq!(visible_rows(36.0, 240.0, 24.0, 100_000), (1, 12), "a row cut at each end is drawn");
        assert_eq!(visible_rows(2_399_760.0, 240.0, 24.0, 100_000), (99_990, 100_000));
        assert_eq!(visible_rows(0.0, 240.0, 24.0, 3), (0, 3)); assert_eq!(visible_rows(0.0, 0.0, 24.0, 3), (0, 0));
        assert_eq!(reveal_offset(0.0, 240.0, 24.0, 5), 0.0); assert_eq!(reveal_offset(0.0, 240.0, 24.0, 10), 24.0); assert_eq!(reveal_offset(480.0, 240.0, 24.0, 3), 72.0);
    }
    #[test]
    fn a_press_moved_is_a_drag_and_carries_the_whole_choice() {
        let now = Instant::now(); let src = Rows(vec![("a", 0, false, false), ("b", 0, false, false), ("c", 0, false, false)]); let mut t = table(3);
        let (down, up, mv) = (|x: f64, y: f64| Event::Down { x, y, button: Button::Primary }, |x: f64, y: f64| Event::Up { x, y, button: Button::Primary }, |x: f64, y: f64| Event::Move { x, y });
        // rows 0..2 chosen (row i lies at y 24 + 24 i); a plain press on one of them keeps all three
        t.sel.resize(3); t.sel.select_all(); assert_eq!(t.event(&down(200.0, 60.0), &src, NONE, now).0, []); assert!(t.sel.is_selected(0) && t.sel.is_selected(2));
        // moved a little: nothing; past the slop: the drag begins, once
        assert_eq!(t.event(&mv(203.0, 61.0), &src, NONE, now).0, []); assert_eq!(t.event(&mv(212.0, 66.0), &src, NONE, now).0, [ViewEvent::DragStart(1)]); assert_eq!(t.event(&mv(260.0, 90.0), &src, NONE, now).0, []);
        assert!(t.sel.is_selected(0) && t.sel.is_selected(1) && t.sel.is_selected(2), "the drag carries the whole choice");
        // let go WITHOUT moving on a row of a larger choice: that row alone
        let later = now + std::time::Duration::from_secs(2);
        assert_eq!(t.event(&down(200.0, 60.0), &src, NONE, later).0, []); assert_eq!(t.event(&up(200.0, 60.0), &src, NONE, later).0, [ViewEvent::SelectionChanged]); assert!(t.sel.is_selected(1) && !t.sel.is_selected(0) && !t.sel.is_selected(2));
        // a press on a row that is not chosen chooses it, and it can be dragged at once
        let later = later + std::time::Duration::from_secs(2);
        assert_eq!(t.event(&down(200.0, 84.0), &src, NONE, later).0, [ViewEvent::SelectionChanged]); assert_eq!(t.event(&mv(230.0, 84.0), &src, NONE, later).0, [ViewEvent::DragStart(2)]); assert!(t.sel.is_selected(2) && !t.sel.is_selected(1));
    }
    #[test]
    fn presses_choose_activate_open_and_sort() {
        let now = Instant::now(); let src = Rows(vec![("a", 0, true, false), ("b", 0, false, false), ("c", 0, false, false)]); let mut t = table(3);
        let down = |x: f64, y: f64| Event::Down { x, y, button: Button::Primary };
        // row 1 lies at y 48..72 (the header is 24 high)
        assert_eq!(t.event(&down(200.0, 60.0), &src, NONE, now).0, [ViewEvent::SelectionChanged]);
        assert_eq!(t.event(&down(200.0, 60.0), &src, NONE, now + std::time::Duration::from_millis(150)).0, [ViewEvent::Activated(1)]);
        // the triangle of row 0 (x 8..24) opens it and chooses nothing
        assert_eq!(t.event(&down(14.0, 30.0), &src, NONE, now).0, [ViewEvent::Expand(0)]); assert!(t.sel.is_selected(1) && !t.sel.is_selected(0));
        // the secondary button on a row that is not chosen chooses it first
        assert_eq!(t.event(&Event::Down { x: 200.0, y: 90.0, button: Button::Secondary }, &src, NONE, now).0, [ViewEvent::SelectionChanged, ViewEvent::ContextMenu(Some(2), (200.0, 90.0))]);
        // the empty part lets the choice go
        assert_eq!(t.event(&down(200.0, 200.0), &src, NONE, now).0, [ViewEvent::SelectionChanged]); assert_eq!(t.sel.count(), 0);
        // a press outside the table is not the table's
        assert!(t.event(&down(200.0, 900.0), &src, NONE, now).0.is_empty());
        // a header: ascending first, then descending; another column starts ascending
        let click = |t: &mut Table, x: f64| { t.event(&down(x, 10.0), &src, NONE, now); t.event(&Event::Up { x, y: 10.0, button: Button::Primary }, &src, NONE, now).0 };
        assert_eq!(click(&mut t, 500.0), [ViewEvent::SortChanged(1, true)]); assert_eq!(click(&mut t, 500.0), [ViewEvent::SortChanged(1, false)]); assert_eq!(click(&mut t, 100.0), [ViewEvent::SortChanged(0, true)]);
        // a divider (the one before "Size" lies at 800 - 140 - 80 = 580): dragged 30 to the right, the date grows, the size shrinks
        t.event(&down(581.0, 10.0), &src, NONE, now); assert!(t.resizing()); t.event(&Event::Move { x: 611.0, y: 10.0 }, &src, NONE, now); t.event(&Event::Up { x: 611.0, y: 10.0, button: Button::Primary }, &src, NONE, now);
        assert_eq!((t.columns[1].width, t.columns[2].width), (180.0, 50.0)); assert_eq!(t.sort, (0, true), "a drag is not a click on a title");
    }
    #[test]
    fn keys_move_open_close_and_find() {
        let now = Instant::now();
        let src = Rows(vec![("alpha", 0, true, true), ("inner", 1, true, false), ("leaf", 1, false, false), ("beta", 0, true, false), ("Shot_2", 0, false, false), ("shot_10", 0, false, false)]); let mut t = table(6);
        assert_eq!(t.key(&key(Key::Named(Named::Down)), &src, now), (vec![ViewEvent::SelectionChanged], true)); assert_eq!(t.sel.cursor, Some(0));
        assert_eq!(t.key(&key(Key::Named(Named::Left)), &src, now).0, [ViewEvent::Collapse(0)], "Left closes an open row");
        t.sel.select_only(2); assert_eq!(t.key(&key(Key::Named(Named::Left)), &src, now).0, [ViewEvent::SelectionChanged]); assert_eq!(t.sel.cursor, Some(0), "Left on a row inside goes to the row it lies in");
        t.sel.select_only(3); assert_eq!(t.key(&key(Key::Named(Named::Right)), &src, now).0, [ViewEvent::Expand(3)]);
        t.sel.select_only(4); assert!(t.key(&key(Key::Named(Named::Right)), &src, now).0.is_empty());
        assert_eq!(t.key(&key(Key::Named(Named::Enter)), &src, now).0, [ViewEvent::Activated(4)]);
        let typed = |c: &str| KeyEvent { key: Key::Char(c.into()), text: Some(c.into()), shift: false, cmd: false };
        assert_eq!(t.key(&typed("b"), &src, now).0, [ViewEvent::SelectionChanged]); assert_eq!(t.sel.cursor, Some(3));
        let later = now + std::time::Duration::from_secs(3);
        t.key(&typed("s"), &src, later); t.key(&typed("h"), &src, later); t.key(&typed("o"), &src, later); t.key(&typed("t"), &src, later); t.key(&typed("_"), &src, later); t.key(&typed("1"), &src, later);
        assert_eq!(t.sel.cursor, Some(5), "\"shot_1\" finds shot_10");
        assert_eq!(t.key(&KeyEvent { key: Key::Char("a".into()), text: None, shift: false, cmd: true }, &src, now).0, [ViewEvent::SelectionChanged]); assert_eq!(t.sel.count(), 6);
        assert_eq!(t.key(&KeyEvent { key: Key::Named(Named::Down), text: None, shift: false, cmd: true }, &src, now), (vec![], false), "the command key with an arrow is the owner's");
        // a page: the body is 240 high, ten rows - a page is nine
        let many = Rows((0..100).map(|_| ("x", 0, false, false)).collect()); let mut t = table(100); t.sel.select_only(0);
        t.key(&KeyEvent { key: Key::Char("PageDown".into()), text: None, shift: false, cmd: false }, &many, now); assert_eq!(t.sel.cursor, Some(9));
        t.key(&key(Key::Named(Named::End)), &many, now); assert_eq!(t.sel.cursor, Some(99)); assert_eq!(t.bar.offset, 100.0 * 24.0 - 240.0, "the cursor is brought into view");
    }
}
