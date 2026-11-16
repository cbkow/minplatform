//! Selection - what a list and a grid of rows share (UFB's browser, canon rows 449 / 451; spike S17's model): one bit
//! per row, an anchor a range grows from, a cursor the keyboard moves, and the letters typed in the last second.
//!
//! The rows are POSITIONS in the view (0..len), not the owner's ids: an owner that re-orders its rows (a sort, a
//! folder opened in place) carries the chosen ids across itself (`iter` before, `set` after).
//!
//! A click chooses one row; with the command key (Ctrl on Windows) it toggles that row; with Shift it chooses the run
//! from the anchor. The keys move the cursor and choose where it lands; with Shift the run from the anchor follows.
use std::time::{Duration, Instant};

/// The modifier keys a click or a key came with (`cmd`: the command key on macOS, Ctrl elsewhere).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Mods { pub shift: bool, pub cmd: bool }

/// A step of the keyboard. `Left` / `Right` are a grid's (one tile); a table gives them its own meaning.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Nav { Up, Down, Left, Right, PageUp, PageDown, Home, End }

/// What a table or a grid tells its owner. A row is a position in the view.
#[derive(Clone, Debug, PartialEq)]
pub enum ViewEvent {
    /// A double click, or Return.
    Activated(usize),
    SelectionChanged,
    /// The column and whether it is ascending.
    SortChanged(usize, bool),
    Expand(usize),
    Collapse(usize),
    /// The secondary button: the row under it (none on the empty part) and where, in window pixels.
    ContextMenu(Option<usize>, (f64, f64)),
}

/// How long typed letters stay one word.
pub const TYPE_AHEAD: Duration = Duration::from_secs(1);

#[derive(Clone, Debug, Default)]
pub struct Selection { bits: Vec<u64>, len: usize, pub anchor: Option<usize>, pub cursor: Option<usize>, typed: String, typed_at: Option<Instant> }

impl Selection {
    pub fn new() -> Selection { Selection::default() }
    pub fn len(&self) -> usize { self.len }
    pub fn is_empty(&self) -> bool { self.len == 0 }
    /// The view has `len` rows now. Rows past the end are let go; the rest stay as they were.
    pub fn resize(&mut self, len: usize) {
        if len == self.len { return }
        self.bits.resize(len.div_ceil(64), 0); self.len = len; self.trim();
        self.anchor = self.anchor.filter(|a| *a < len); self.cursor = self.cursor.filter(|c| *c < len);
    }
    fn trim(&mut self) { if self.len % 64 != 0 { if let Some(l) = self.bits.last_mut() { *l &= (1u64 << (self.len % 64)) - 1; } } }
    pub fn is_selected(&self, i: usize) -> bool { i < self.len && self.bits[i / 64] >> (i % 64) & 1 == 1 }
    pub fn count(&self) -> usize { self.bits.iter().map(|w| w.count_ones() as usize).sum() }
    /// The chosen rows, in order.
    pub fn iter(&self) -> impl Iterator<Item = usize> + '_ {
        self.bits.iter().enumerate().flat_map(|(w, bits)| { let mut b = *bits; std::iter::from_fn(move || { if b == 0 { return None } let t = b.trailing_zeros() as usize; b &= b - 1; Some(w * 64 + t) }) })
    }
    pub fn clear(&mut self) -> bool { let had = self.bits.iter().any(|w| *w != 0); for w in self.bits.iter_mut() { *w = 0; } had }
    /// Forget everything: no row chosen, no anchor, no cursor, nothing typed.
    pub fn reset(&mut self) { self.clear(); self.anchor = None; self.cursor = None; self.typed.clear(); self.typed_at = None; }
    pub fn select_all(&mut self) -> bool { let before = self.count(); for w in self.bits.iter_mut() { *w = !0; } self.trim(); before != self.len }
    pub fn insert(&mut self, i: usize) { if i < self.len { self.bits[i / 64] |= 1 << (i % 64); } }
    pub fn remove(&mut self, i: usize) { if i < self.len { self.bits[i / 64] &= !(1 << (i % 64)); } }
    /// Exactly these rows (an owner that re-ordered its rows puts the choice back with this).
    pub fn set(&mut self, rows: impl IntoIterator<Item = usize>) { self.clear(); for i in rows { self.insert(i); } }
    fn insert_range(&mut self, a: usize, b: usize) { let (a, b) = (a.min(b), a.max(b).min(self.len.saturating_sub(1))); for i in a..=b { self.insert(i); } }
    /// Exactly one row; it becomes the anchor and the cursor.
    pub fn select_only(&mut self, i: usize) -> bool {
        if i >= self.len { return false }
        let same = self.count() == 1 && self.is_selected(i);
        self.clear(); self.insert(i); self.anchor = Some(i); self.cursor = Some(i); !same
    }
    /// A click on row `i`. True when the choice changed.
    pub fn click(&mut self, i: usize, mods: Mods) -> bool {
        if i >= self.len { return false }
        if mods.shift { return self.extend_to(i) }
        if mods.cmd { if self.is_selected(i) { self.remove(i); } else { self.insert(i); } self.anchor = Some(i); self.cursor = Some(i); return true }
        self.select_only(i)
    }
    /// The run from the anchor to `i` (the anchor stays; with none, `i` is it).
    pub fn extend_to(&mut self, i: usize) -> bool {
        if i >= self.len { return false }
        let a = self.anchor.or(self.cursor).unwrap_or(i); let before = self.bits.clone();
        self.clear(); self.insert_range(a, i); self.anchor = Some(a); self.cursor = Some(i); before != self.bits
    }
    /// The cursor goes to `i`, as a key takes it: alone it is chosen, with Shift the run from the anchor is.
    pub fn move_to(&mut self, i: usize, shift: bool) -> bool { if shift { self.extend_to(i) } else { self.select_only(i) } }
    /// Where a key takes the cursor: `cols` rows lie side by side (1 in a table), `page` rows are a page.
    pub fn target(&self, nav: Nav, cols: usize, page: usize) -> Option<usize> {
        if self.len == 0 { return None }
        let (last, cols, page) = (self.len - 1, cols.max(1), page.max(1));
        let Some(c) = self.cursor.or(self.anchor) else { return Some(match nav { Nav::Up | Nav::Left | Nav::End | Nav::PageUp => if matches!(nav, Nav::PageUp) { 0 } else { last }, _ => 0 }) };
        Some(match nav {
            Nav::Up => if c >= cols { c - cols } else { c },
            Nav::Down => if c + cols <= last { c + cols } else if cols > 1 && c / cols < last / cols { last } else { c },
            Nav::Left => c.saturating_sub(1),
            Nav::Right => (c + 1).min(last),
            Nav::PageUp => { let rows = (page / cols).max(1) * cols; if c >= rows { c - rows } else { c % cols } }
            Nav::PageDown => { let rows = (page / cols).max(1) * cols; if c + rows <= last { c + rows } else if c / cols < last / cols { ((last / cols) * cols + c % cols).min(last) } else { c } }
            Nav::Home => 0,
            Nav::End => last,
        })
    }
    /// A key's step: the cursor moves and the choice follows. Returns the row it landed on when anything changed.
    pub fn nav(&mut self, nav: Nav, shift: bool, cols: usize, page: usize) -> Option<usize> {
        let to = self.target(nav, cols, page)?; let was = self.cursor;
        let changed = self.move_to(to, shift); (changed || was != Some(to)).then_some(to)
    }
    /// Letters typed: within a second of the last they are one word. Returns the first row whose name starts with the
    /// word (`starts(row, word)` answers; see [`starts_with_ci`]) - the row is chosen. A word with no row is kept, so
    /// that the next letter does not start a new one.
    pub fn type_ahead(&mut self, text: &str, now: Instant, starts: impl Fn(usize, &str) -> bool) -> Option<usize> {
        if text.is_empty() || text.chars().any(|c| c.is_control()) { return None }
        if self.typed_at.is_none_or(|t| now.duration_since(t) > TYPE_AHEAD) { self.typed.clear(); }
        self.typed.push_str(text); self.typed_at = Some(now);
        let hit = (0..self.len).find(|i| starts(*i, &self.typed))?;
        self.select_only(hit); Some(hit)
    }
    /// The word being typed (empty when the second has passed).
    pub fn typed(&self, now: Instant) -> &str { if self.typed_at.is_some_and(|t| now.duration_since(t) <= TYPE_AHEAD) { &self.typed } else { "" } }
}

/// `name` starts with `prefix`, whatever the case of either.
pub fn starts_with_ci(name: &str, prefix: &str) -> bool {
    let mut n = name.chars().flat_map(char::to_lowercase);
    prefix.chars().flat_map(char::to_lowercase).all(|p| n.next() == Some(p))
}

/// Two presses are one double click: the same place (within 4 px), within 400 ms.
#[derive(Clone, Copy, Debug, Default)]
pub struct DoubleClick { last: Option<(usize, Instant, (f64, f64))> }
impl DoubleClick {
    /// A press on `row`. True when it is the second of a double click (a third press starts again).
    pub fn press(&mut self, row: usize, at: (f64, f64), now: Instant, scale: f64) -> bool {
        let double = self.last.is_some_and(|(r, t, p)| r == row && now.duration_since(t) <= Duration::from_millis(400) && (p.0 - at.0).abs() <= 4.0 * scale && (p.1 - at.1).abs() <= 4.0 * scale);
        self.last = if double { None } else { Some((row, now, at)) }; double
    }
    pub fn forget(&mut self) { self.last = None; }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn sel(n: usize) -> Selection { let mut s = Selection::new(); s.resize(n); s }
    fn rows(s: &Selection) -> Vec<usize> { s.iter().collect() }
    const NONE: Mods = Mods { shift: false, cmd: false }; const SHIFT: Mods = Mods { shift: true, cmd: false }; const CMD: Mods = Mods { shift: false, cmd: true };

    #[test]
    fn a_click_chooses_one_the_command_key_toggles_shift_takes_the_run() {
        let mut s = sel(200);
        assert!(s.click(5, NONE)); assert_eq!(rows(&s), [5]);
        assert!(!s.click(5, NONE), "the same row again changes nothing");
        assert!(s.click(70, CMD)); assert_eq!(rows(&s), [5, 70]);
        assert!(s.click(5, CMD)); assert_eq!(rows(&s), [70]); assert_eq!(s.anchor, Some(5));
        s.click(10, NONE); assert!(s.click(13, SHIFT)); assert_eq!(rows(&s), [10, 11, 12, 13]);
        assert!(s.click(8, SHIFT)); assert_eq!(rows(&s), [8, 9, 10], "the run turns round the anchor"); assert_eq!((s.anchor, s.cursor), (Some(10), Some(8)));
        s.click(60, NONE); s.click(130, SHIFT); assert_eq!(s.count(), 71, "a run across the words of the bitset");
        assert!(!s.click(500, NONE), "a row that is not there");
    }
    #[test]
    fn select_all_resize_and_set() {
        let mut s = sel(130);
        assert!(s.select_all()); assert_eq!(s.count(), 130); assert!(!s.select_all());
        s.resize(70); assert_eq!(s.count(), 70); s.resize(200); assert_eq!(s.count(), 70, "new rows come unchosen");
        s.set([3, 150, 999]); assert_eq!(rows(&s), [3, 150]);
        assert!(s.clear()); assert!(!s.clear()); assert_eq!(s.count(), 0);
        let mut e = sel(0); assert!(!e.select_all()); assert_eq!(e.target(Nav::Down, 1, 10), None);
    }
    #[test]
    fn the_keys_in_a_table() {
        let mut s = sel(100);
        assert_eq!(s.nav(Nav::Down, false, 1, 10), Some(0), "with no cursor Down starts at the top");
        assert_eq!(s.nav(Nav::Down, false, 1, 10), Some(1)); assert_eq!(s.nav(Nav::Up, false, 1, 10), Some(0)); assert_eq!(s.nav(Nav::Up, false, 1, 10), None, "the top holds");
        assert_eq!(s.nav(Nav::PageDown, false, 1, 10), Some(10)); assert_eq!(s.nav(Nav::End, false, 1, 10), Some(99)); assert_eq!(s.nav(Nav::PageDown, false, 1, 10), None);
        assert_eq!(s.nav(Nav::PageUp, false, 1, 10), Some(89)); assert_eq!(s.nav(Nav::Home, false, 1, 10), Some(0)); assert_eq!(rows(&s), [0]);
        s.click(20, NONE);
        assert_eq!(s.nav(Nav::Down, true, 1, 10), Some(21)); assert_eq!(s.nav(Nav::Down, true, 1, 10), Some(22)); assert_eq!(rows(&s), [20, 21, 22]);
        assert_eq!(s.nav(Nav::Up, true, 1, 10), Some(21)); assert_eq!(rows(&s), [20, 21], "Shift back gives the row up");
        assert_eq!(s.nav(Nav::Home, true, 1, 10), Some(0)); assert_eq!(s.count(), 21);
        let mut e = sel(5); assert_eq!(e.nav(Nav::Up, false, 1, 10), Some(4), "with no cursor Up starts at the end");
    }
    #[test]
    fn the_keys_in_a_grid() {
        // 10 tiles, 4 a row: 0 1 2 3 / 4 5 6 7 / 8 9
        let mut s = sel(10); s.click(1, NONE);
        assert_eq!(s.nav(Nav::Right, false, 4, 8), Some(2)); assert_eq!(s.nav(Nav::Down, false, 4, 8), Some(6));
        assert_eq!(s.nav(Nav::Down, false, 4, 8), Some(9), "no tile below in the last, short row: its end");
        assert_eq!(s.nav(Nav::Down, false, 4, 8), None); assert_eq!(s.nav(Nav::Up, false, 4, 8), Some(5)); assert_eq!(s.nav(Nav::Left, false, 4, 8), Some(4));
        assert_eq!(s.nav(Nav::Up, false, 4, 8), Some(0)); assert_eq!(s.nav(Nav::Left, false, 4, 8), None);
        s.click(1, NONE); assert_eq!(s.nav(Nav::PageDown, false, 4, 8), Some(9)); assert_eq!(s.nav(Nav::PageUp, false, 4, 8), Some(1));
        s.click(2, NONE); assert_eq!(s.nav(Nav::Down, true, 4, 8), Some(6)); assert_eq!(rows(&s), [2, 3, 4, 5, 6]);
    }
    #[test]
    fn typed_letters_within_a_second_are_one_word() {
        let names = ["Alpha", "beta", "Shot_2", "shot_10", "SHOW"]; let t0 = Instant::now();
        let starts = |i: usize, w: &str| starts_with_ci(names[i], w);
        let mut s = sel(names.len());
        assert_eq!(s.type_ahead("s", t0, starts), Some(2)); assert_eq!(rows(&s), [2]);
        assert_eq!(s.type_ahead("h", t0 + Duration::from_millis(300), starts), Some(2));
        assert_eq!(s.type_ahead("o", t0 + Duration::from_millis(600), starts), Some(2));
        assert_eq!(s.type_ahead("w", t0 + Duration::from_millis(900), starts), Some(4), "\"show\" - whatever the case");
        assert_eq!(s.typed(t0 + Duration::from_millis(1000)), "show");
        assert_eq!(s.type_ahead("b", t0 + Duration::from_millis(2500), starts), Some(1), "after a second a new word begins");
        assert_eq!(s.type_ahead("x", t0 + Duration::from_millis(2600), starts), None); assert_eq!(rows(&s), [1], "no row: the choice stays");
        assert_eq!(s.type_ahead("\u{8}", t0, starts), None);
        assert!(starts_with_ci("Éclair.mov", "éc") && !starts_with_ci("ab", "abc") && starts_with_ci("x", ""));
    }
    #[test]
    fn a_double_click_is_two_presses_in_one_place() {
        let t0 = Instant::now(); let mut d = DoubleClick::default();
        assert!(!d.press(3, (10.0, 10.0), t0, 1.0)); assert!(d.press(3, (11.0, 12.0), t0 + Duration::from_millis(200), 1.0));
        assert!(!d.press(3, (11.0, 12.0), t0 + Duration::from_millis(300), 1.0), "the third press starts again");
        assert!(!d.press(4, (11.0, 12.0), t0 + Duration::from_millis(350), 1.0), "another row");
        assert!(!d.press(4, (11.0, 12.0), t0 + Duration::from_millis(900), 1.0), "too late");
    }
}
