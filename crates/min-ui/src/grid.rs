//! Grid - a virtualised grid of tiles (UFB's grid, canon rows 451 / 463; spike S17's way): a picture or an icon in a
//! square of `tile` logical px (64 - 256, changed live), the name under it on one or two centred lines, hover and
//! selection, the overlay [`ScrollBar`]. As many tiles lie side by side as the width takes, and they share what is
//! left over. Only the tiles in view are asked for and drawn; the grid does not own them ([`GridSource`]).
//!
//! Pointer and keys as the [`crate::table::Table`]'s (the same [`Selection`]); here Left / Right step one tile and
//! Up / Down one row of tiles.
use crate::selection::{starts_with_ci, DoubleClick, Mods, Nav, Selection, ViewEvent};
use crate::{fonts::{draw_layout, TextLayout}, paint, Button, Event, Family, Fonts, Key, KeyEvent, Named, ScrollBar};
use kurbo::{Affine, Rect};
use min_theme::tokens as t;
use std::borrow::Cow;
use std::time::Instant;
use vello::Scene;

pub trait GridSource {
    fn len(&self) -> usize;
    fn name(&self, i: usize) -> Cow<'_, str>;
    /// The tile's picture; none draws the neutral placeholder.
    fn image(&self, _i: usize) -> Option<&peniko::ImageBrush> { None }
    /// Drawn in the quiet colour (a hidden file).
    fn dim(&self, _i: usize) -> bool { false }
}

pub const TILE_MIN: f64 = 64.0; pub const TILE_MAX: f64 = 256.0;
/// Logical px: the room round the grid, round a tile's picture, and a line of the name.
const MARGIN: f64 = 8.0; const PAD: f64 = 8.0; const LINE: f64 = 16.0;

/// How the tiles lie, in physical px: how many side by side, a cell's size, where the first cell starts.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Geometry { pub cols: usize, pub cell_w: f64, pub cell_h: f64, pub left: f64, pub top: f64 }

/// The grid's geometry for a box `width` physical px wide, tiles of `tile` logical px and `lines` lines of name.
pub fn geometry(width: f64, tile: f64, lines: usize, scale: f64) -> Geometry {
    let tile = tile.clamp(TILE_MIN, TILE_MAX); let margin = MARGIN * scale;
    let least = (tile + 2.0 * PAD) * scale; let room = (width - 2.0 * margin).max(1.0);
    let cols = ((room / least).floor() as usize).max(1);
    // the tiles share the width that is left over; one tile in a box narrower than it takes the box
    let cell_w = if room < least { room } else { room / cols as f64 };
    Geometry { cols, cell_w, cell_h: (tile + 2.0 * PAD + lines.max(1) as f64 * LINE) * scale, left: margin, top: margin }
}
impl Geometry {
    pub fn rows(&self, n: usize) -> usize { n.div_ceil(self.cols) }
    pub fn content_height(&self, n: usize) -> f64 { self.rows(n) as f64 * self.cell_h + 2.0 * self.top }
    /// The tiles in view: the first, and one past the last.
    pub fn visible(&self, offset: f64, view_h: f64, n: usize) -> (usize, usize) {
        if view_h <= 0.0 { return (0, 0) }
        let first = (((offset - self.top).max(0.0) / self.cell_h) as usize * self.cols).min(n);
        let end = ((((offset + view_h - self.top).max(0.0) / self.cell_h).ceil() as usize) * self.cols).min(n);
        (first, end)
    }
    /// The cell of tile `i`, from the top of the content (the scroll offset is not in it).
    pub fn cell(&self, i: usize) -> Rect { let (r, c) = (i / self.cols, i % self.cols); let (x, y) = (self.left + c as f64 * self.cell_w, self.top + r as f64 * self.cell_h); Rect::new(x, y, x + self.cell_w, y + self.cell_h) }
    /// The tile at a point of the content.
    pub fn at(&self, x: f64, y: f64, n: usize) -> Option<usize> {
        if x < self.left || y < self.top { return None }
        let c = ((x - self.left) / self.cell_w) as usize; if c >= self.cols { return None }
        let i = ((y - self.top) / self.cell_h) as usize * self.cols + c; (i < n).then_some(i)
    }
}

pub struct Grid {
    /// The picture's square, logical px (64 - 256).
    pub tile: f64,
    /// Lines of name under a picture (1 or 2).
    pub lines: usize,
    pub sel: Selection,
    pub bar: ScrollBar,
    pub rect: Rect,
    pub focused: bool,
    n: usize, scale: f64, geom: Geometry, hover: Option<usize>, dbl: DoubleClick,
    /// a primary press on a tile that may become a drag: (the tile, where, whether it was already chosen)
    press: Option<(usize, f64, f64, bool)>,
}

impl Grid {
    pub fn new(tile: f64) -> Grid { Grid { tile: tile.clamp(TILE_MIN, TILE_MAX), lines: 2, sel: Selection::new(), bar: ScrollBar::default(), rect: Rect::ZERO, focused: false, n: 0, scale: 1.0, geom: geometry(1.0, tile, 2, 1.0), hover: None, dbl: DoubleClick::default(), press: None } }
    /// Place the grid and tell it how many tiles there are (every frame, before `event` / `paint`).
    pub fn layout(&mut self, rect: Rect, n: usize, scale: f64) {
        self.tile = self.tile.clamp(TILE_MIN, TILE_MAX);
        self.rect = rect; self.n = n; self.scale = scale; self.sel.resize(n); self.geom = geometry(rect.width(), self.tile, self.lines, scale);
        self.bar.set(rect, self.geom.content_height(n), scale);
        if self.hover.is_some_and(|h| h >= n) { self.hover = None; }
    }
    pub fn geometry(&self) -> Geometry { self.geom }
    pub fn hit(&self, x: f64, y: f64) -> bool { self.rect.contains(kurbo::Point::new(x, y)) }
    pub fn hovered_tile(&self) -> Option<usize> { self.hover }
    pub fn tile_at(&self, x: f64, y: f64) -> Option<usize> { if !self.hit(x, y) { return None } self.geom.at(x - self.rect.x0, y - self.rect.y0 + self.bar.offset, self.n) }
    /// Where tile `i`'s cell lies now (it may be out of view).
    pub fn cell_rect(&self, i: usize) -> Rect { let c = self.geom.cell(i); Rect::new(c.x0 + self.rect.x0, c.y0 + self.rect.y0 - self.bar.offset, c.x1 + self.rect.x0, c.y1 + self.rect.y0 - self.bar.offset) }
    fn page(&self) -> usize { ((self.rect.height() / self.geom.cell_h) as usize).max(1) * self.geom.cols }
    /// Scroll so that tile `i` is in view.
    /// Where the NAME of tile `i` is written (under its picture, the tile's width): the place of a field that renames it.
    pub fn label_rect(&self, i: usize) -> Rect {
        let c = self.cell_rect(i); let s = self.scale; let side = (self.tile * s).min(c.width() - 2.0 * s); let y = c.y0 + PAD * s + side + PAD * s / 2.0;
        Rect::new(c.x0 + 2.0 * s, y, c.x1 - 2.0 * s, y + LINE * s)
    }
    pub fn reveal(&mut self, i: usize) {
        let c = self.geom.cell(i); let h = self.rect.height();
        let o = if c.y0 - self.geom.top < self.bar.offset { c.y0 - self.geom.top } else if c.y1 + self.geom.top > self.bar.offset + h { c.y1 + self.geom.top - h } else { self.bar.offset };
        self.bar.offset = o.clamp(0.0, self.bar.max());
    }
    pub fn wheel(&mut self, dy: f64, now: Instant) -> bool { self.bar.wheel(dy, now) }
    pub fn animating(&self, now: Instant) -> bool { self.bar.animating(now) }

    /// A pointer event. Returns what happened and whether a repaint is needed.
    pub fn event(&mut self, e: &Event, mods: Mods, now: Instant) -> (Vec<ViewEvent>, bool) {
        let mut out = vec![];
        let (moved, mine) = self.bar.event(e, now); if mine { return (out, true) }
        let before = self.hover;
        if matches!(e, Event::Down { .. }) { self.press = None; }
        match *e {
            Event::Move { x, y } => { self.hover = self.tile_at(x, y); if let Some((i, x0, y0, _)) = self.press { if (x - x0).hypot(y - y0) > crate::table::DRAG_SLOP * self.scale { self.press = None; out.push(ViewEvent::DragStart(i)); } } }
            Event::Up { button: Button::Primary, .. } => { if let Some((i, _, _, was)) = self.press.take() { if was && self.sel.select_only(i) { out.push(ViewEvent::SelectionChanged); } } }
            Event::Leave => self.hover = None,
            Event::Down { x, y, button } if self.hit(x, y) => match (self.tile_at(x, y), button) {
                (Some(i), Button::Primary) => {
                    // (as the table: a plain press on a chosen tile keeps the choice for a drag; let go without moving, that tile alone)
                    let was = self.sel.is_selected(i) && !mods.shift && !mods.cmd;
                    if !was && self.sel.click(i, mods) { out.push(ViewEvent::SelectionChanged); }
                    if !mods.shift && !mods.cmd { self.press = Some((i, x, y, was)); }
                    if self.dbl.press(i, (x, y), now, self.scale) && !mods.shift && !mods.cmd { out.push(ViewEvent::Activated(i)); }
                }
                (Some(i), Button::Secondary) => { if !self.sel.is_selected(i) && self.sel.select_only(i) { out.push(ViewEvent::SelectionChanged); } out.push(ViewEvent::ContextMenu(Some(i), (x, y))); }
                (None, Button::Primary) => { self.dbl.forget(); if !mods.shift && !mods.cmd && self.sel.clear() { out.push(ViewEvent::SelectionChanged); } }
                (None, Button::Secondary) => out.push(ViewEvent::ContextMenu(None, (x, y))),
                _ => {}
            },
            _ => {}
        }
        let repaint = moved || !out.is_empty() || before != self.hover;
        (out, repaint)
    }

    /// A key. Returns what happened and whether the key was the grid's.
    pub fn key(&mut self, k: &KeyEvent, src: &dyn GridSource, now: Instant) -> (Vec<ViewEvent>, bool) {
        let mut out = vec![];
        let nav = match &k.key {
            Key::Named(Named::Up) => Some(Nav::Up), Key::Named(Named::Down) => Some(Nav::Down), Key::Named(Named::Left) => Some(Nav::Left), Key::Named(Named::Right) => Some(Nav::Right), Key::Named(Named::Home) => Some(Nav::Home), Key::Named(Named::End) => Some(Nav::End),
            Key::Char(c) if c == "PageUp" => Some(Nav::PageUp), Key::Char(c) if c == "PageDown" => Some(Nav::PageDown), _ => None };
        if let Some(nav) = nav {
            if k.cmd { return (out, false) }
            if let Some(to) = self.sel.nav(nav, k.shift, self.geom.cols, self.page()) { self.reveal(to); out.push(ViewEvent::SelectionChanged); }
            return (out, true)
        }
        match &k.key {
            Key::Named(Named::Enter) if !k.cmd => { if let Some(c) = self.sel.cursor.filter(|c| self.sel.is_selected(*c)) { out.push(ViewEvent::Activated(c)); } (out, true) }
            Key::Char(c) if k.cmd && c.eq_ignore_ascii_case("a") && !k.shift => { if self.sel.select_all() { out.push(ViewEvent::SelectionChanged); } (out, true) }
            _ if !k.cmd => match k.text.as_deref().filter(|t| !t.is_empty() && !t.chars().any(char::is_control) && (*t != " " || !self.sel.typed(now).is_empty())) {
                Some(text) => { let before = self.sel.cursor; if let Some(hit) = self.sel.type_ahead(text, now, |i, w| starts_with_ci(&src.name(i), w)) { self.reveal(hit); if before != Some(hit) { out.push(ViewEvent::SelectionChanged); } } (out, true) }
                None => (out, false),
            },
            _ => (out, false),
        }
    }

    pub fn paint(&self, scene: &mut Scene, fonts: &mut Fonts, src: &dyn GridSource, now: Instant) {
        let s = self.scale; if self.rect.width() <= 0.0 || self.rect.height() <= 0.0 { return }
        let (ink, sub) = (paint::color(t::NEUTRAL_CONTENT_COLOR_DEFAULT), paint::color(t::NEUTRAL_SUBDUED_CONTENT_COLOR_DEFAULT));
        scene.push_clip_layer(peniko::Fill::NonZero, Affine::IDENTITY, &self.rect);
        let (first, end) = self.geom.visible(self.bar.offset, self.rect.height(), self.n.min(src.len())); let px = t::BODY_SIZE_S * s as f32; let side = self.tile * s;
        for i in first..end {
            let c = self.cell_rect(i); let back = c.inflate(-2.0 * s, -2.0 * s); let radius = t::CORNER_RADIUS_MEDIUM_DEFAULT as f64 * s;
            if self.sel.is_selected(i) { paint::rounded(scene, back, radius, if self.focused { t::SELECTION_BACKGROUND_COLOR_EMPHASIZED } else { t::SELECTION_BACKGROUND_COLOR }); }
            else if self.hover == Some(i) { paint::rounded(scene, back, radius, t::HOVER_BACKGROUND_COLOR); }
            let side = side.min(c.width() - 2.0 * s); let pic = Rect::new(c.x0 + (c.width() - side) / 2.0, c.y0 + PAD * s, c.x0 + (c.width() + side) / 2.0, c.y0 + PAD * s + side);
            match src.image(i) {
                // the picture keeps its shape, centred in the square
                Some(img) => { let (iw, ih) = (img.image.width.max(1) as f64, img.image.height.max(1) as f64); let k = (side / iw).min(side / ih); let (w, h) = (iw * k, ih * k); paint::image(scene, img, Rect::new(pic.x0 + (side - w) / 2.0, pic.y0 + (side - h) / 2.0, pic.x0 + (side + w) / 2.0, pic.y0 + (side + h) / 2.0)); }
                None => paint::rounded(scene, pic.inflate(-side * 0.2, -side * 0.14), (side * 0.06).max(2.0), t::GRAY_300),
            }
            let name = src.name(i); let color = if src.dim(i) { sub } else { ink }; let mut y = pic.y1 + PAD * s / 2.0;
            for l in wrap(fonts, &name, px, color, c.width() - 2.0 * PAD * s, self.lines) { draw_layout(scene, &l, Affine::translate((c.x0 + (c.width() - l.width() as f64) / 2.0, y + (LINE * s - l.height() as f64) / 2.0))); y += LINE * s; }
        }
        scene.pop_layer();
        self.bar.paint(scene, now);
    }
}

/// Where a name is cut for its first line: `fit` characters fit; a separator within the last few of them is a better
/// place (after it). Returns how many characters the first line takes.
pub fn break_at(chars: &[char], fit: usize) -> usize {
    let fit = fit.clamp(1, chars.len());
    if fit == chars.len() { return fit }
    (fit.saturating_sub(5).max(1)..=fit).rev().find(|k| matches!(chars[*k - 1], '_' | '-' | '.' | ' ')).unwrap_or(fit)
}

/// A name on at most `lines` lines of `max_w`: the last line is cut with an ellipsis. The first line's length is found
/// from an estimate and a few layouts, not a search over the whole name (a grid lays out every tile in view each frame).
fn wrap(fonts: &mut Fonts, text: &str, px: f32, color: peniko::Color, max_w: f64, lines: usize) -> Vec<TextLayout> {
    let w = t::FONT_WEIGHT_REGULAR as f32;
    let full = fonts.layout(text, Family::Sans, px, w, color);
    if full.width() as f64 <= max_w { return vec![full] }
    let chars: Vec<char> = text.chars().collect();
    if lines < 2 || chars.len() < 2 || max_w <= 0.0 { return vec![fonts.layout_fit(text, Family::Sans, px, w, color, max_w)] }
    let line = |fonts: &mut Fonts, k: usize| fonts.layout(&chars[..k].iter().collect::<String>(), Family::Sans, px, w, color);
    let mut n = ((chars.len() as f64 * max_w / full.width() as f64) as usize).clamp(1, chars.len() - 1);
    if line(fonts, n).width() as f64 > max_w { while n > 1 { n -= 1; if line(fonts, n).width() as f64 <= max_w { break } } }
    else { while n + 1 < chars.len() && line(fonts, n + 1).width() as f64 <= max_w { n += 1; } }
    let n = break_at(&chars, n);
    let first = line(fonts, n); let rest: String = chars[n..].iter().collect::<String>().trim_start().to_string();
    vec![first, fonts.layout_fit(&rest, Family::Sans, px, w, color, max_w)]
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Names(Vec<&'static str>);
    impl GridSource for Names { fn len(&self) -> usize { self.0.len() } fn name(&self, i: usize) -> Cow<'_, str> { Cow::Borrowed(self.0[i]) } }
    const NONE: Mods = Mods { shift: false, cmd: false };

    #[test]
    fn the_column_count_follows_the_width_and_the_tile() {
        // a tile of 128 needs 144; a box of 1000 leaves 984: six, sharing the rest
        let g = geometry(1000.0, 128.0, 2, 1.0); assert_eq!(g.cols, 6); assert_eq!(g.cell_w, 164.0); assert_eq!(g.cell_h, 128.0 + 16.0 + 32.0); assert_eq!(g.left, 8.0);
        assert_eq!(geometry(1000.0, 64.0, 2, 1.0).cols, 12); assert_eq!(geometry(1000.0, 256.0, 2, 1.0).cols, 3);
        assert_eq!(geometry(160.0, 128.0, 2, 1.0).cols, 1); assert_eq!(geometry(159.9, 128.0, 2, 1.0).cols, 1);
        let narrow = geometry(100.0, 128.0, 2, 1.0); assert_eq!((narrow.cols, narrow.cell_w), (1, 84.0), "a box narrower than a tile: one, as wide as the box");
        // at a scale of 2 the same box in logical px holds the same count
        assert_eq!(geometry(2000.0, 128.0, 2, 2.0).cols, 6); assert_eq!(geometry(2000.0, 128.0, 2, 2.0).cell_w, 328.0);
        // the tile is held to 64 - 256
        assert_eq!(geometry(1000.0, 10.0, 2, 1.0), geometry(1000.0, 64.0, 2, 1.0)); assert_eq!(geometry(1000.0, 900.0, 1, 1.0), geometry(1000.0, 256.0, 1, 1.0));
        assert_eq!(geometry(1000.0, 128.0, 1, 1.0).cell_h, 128.0 + 16.0 + 16.0);
    }
    #[test]
    fn rows_cells_and_what_is_in_view() {
        let g = geometry(1000.0, 128.0, 2, 1.0);   // 6 a row, cells 164 x 176, 8 px round
        assert_eq!(g.rows(0), 0); assert_eq!(g.rows(6), 1); assert_eq!(g.rows(7), 2); assert_eq!(g.content_height(13), 3.0 * 176.0 + 16.0);
        assert_eq!(g.cell(0), Rect::new(8.0, 8.0, 172.0, 184.0)); assert_eq!(g.cell(7), Rect::new(172.0, 184.0, 336.0, 360.0));
        assert_eq!(g.at(180.0, 190.0, 100), Some(7)); assert_eq!(g.at(2.0, 190.0, 100), None); assert_eq!(g.at(995.0, 20.0, 100), None, "past the last column"); assert_eq!(g.at(180.0, 190.0, 7), None, "no tile there");
        assert_eq!(g.visible(0.0, 400.0, 100_000), (0, 18)); assert_eq!(g.visible(200.0, 400.0, 100_000), (6, 24)); assert_eq!(g.visible(0.0, 400.0, 4), (0, 4));
        let end = g.content_height(100_000) - 400.0; let (a, b) = g.visible(end, 400.0, 100_000); assert!(b == 100_000 && b - a <= 18, "{a}..{b}");
    }
    #[test]
    fn presses_and_keys() {
        let now = Instant::now(); let src = Names(vec!["a", "b", "c", "d", "e", "f", "g", "h", "plate", "Plate_2"]);
        let mut g = Grid::new(128.0); g.layout(Rect::new(0.0, 100.0, 1000.0, 500.0), src.len(), 1.0);
        let down = |x: f64, y: f64| Event::Down { x, y, button: Button::Primary };
        // tile 7 is the second of the second row: x 172..336, y 100 + 184..
        assert_eq!(g.tile_at(200.0, 300.0), Some(7)); assert_eq!(g.tile_at(200.0, 50.0), None);
        assert_eq!(g.event(&down(200.0, 300.0), NONE, now).0, [ViewEvent::SelectionChanged]);
        assert_eq!(g.event(&down(200.0, 300.0), NONE, now + std::time::Duration::from_millis(100)).0, [ViewEvent::Activated(7)]);
        assert_eq!(g.event(&down(20.0, 120.0), Mods { shift: false, cmd: true }, now).0, [ViewEvent::SelectionChanged]); assert_eq!(g.sel.count(), 2);
        assert!(g.event(&down(200.0, 50.0), NONE, now).0.is_empty(), "a press above the grid is not the grid's"); assert_eq!(g.sel.count(), 2);
        assert_eq!(g.event(&down(900.0, 480.0), NONE, now).0, [ViewEvent::SelectionChanged], "the empty part lets the choice go");
        let key = |k: Key| KeyEvent { key: k, text: None, shift: false, cmd: false };
        g.sel.select_only(1);
        g.key(&key(Key::Named(Named::Down)), &src, now); assert_eq!(g.sel.cursor, Some(7)); g.key(&key(Key::Named(Named::Right)), &src, now); assert_eq!(g.sel.cursor, Some(8));
        g.key(&key(Key::Named(Named::Up)), &src, now); assert_eq!(g.sel.cursor, Some(2)); g.key(&key(Key::Named(Named::End)), &src, now); assert_eq!(g.sel.cursor, Some(9));
        assert_eq!(g.key(&key(Key::Named(Named::Enter)), &src, now).0, [ViewEvent::Activated(9)]);
        let typed = |c: &str| KeyEvent { key: Key::Char(c.into()), text: Some(c.into()), shift: false, cmd: false };
        g.key(&typed("p"), &src, now); assert_eq!(g.sel.cursor, Some(8)); for c in ["l", "a", "t", "e", "_"] { g.key(&typed(c), &src, now); } assert_eq!(g.sel.cursor, Some(9));
        // the size changed live: the count of tiles a row follows at the next layout
        g.tile = 64.0; g.layout(Rect::new(0.0, 100.0, 1000.0, 500.0), src.len(), 1.0); assert_eq!(g.geometry().cols, 12);
    }
    #[test]
    fn a_long_name_breaks_after_a_separator_near_the_end_of_the_line() {
        let c: Vec<char> = "shot_010_comp_v003.exr".chars().collect();
        assert_eq!(break_at(&c, 15), 14, "after the underscore of \"comp_\""); assert_eq!(break_at(&c, 12), 9, "after \"shot_010_\""); assert_eq!(break_at(&c, 22), 22);
        let plain: Vec<char> = "abcdefghijklmnop".chars().collect(); assert_eq!(break_at(&plain, 10), 10, "no separator: where it fits");
        let mut f = Fonts::new(); let ink = peniko::Color::WHITE;
        let two = wrap(&mut f, "shot_010_comp_v003_final_final.exr", 12.0, ink, 110.0, 2); assert_eq!(two.len(), 2); assert!(two.iter().all(|l| l.width() <= 110.0));
        assert_eq!(wrap(&mut f, "a.exr", 12.0, ink, 110.0, 2).len(), 1); assert_eq!(wrap(&mut f, "shot_010_comp_v003_final_final.exr", 12.0, ink, 110.0, 1).len(), 1);
    }
}
