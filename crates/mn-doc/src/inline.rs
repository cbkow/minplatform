//! Inline formatting: spans over a block's text (the spec `docs/phase3/step1-core-spec.md` §1, with the changes row 284
//! lists). Offsets are BYTES into the block's UTF-8 text, half-open; the file's UTF-16 offsets convert at the edge
//! ([`Spans::from_utf16`], [`Spans::to_utf16`]).
//!
//! The list is always CANONICAL — sorted by kind then start; same-mark spans that touch or overlap are one span; of a
//! kind that carries a value (link, colour, highlight, choice) no two spans overlap — so two equal formattings are
//! equal values, which undo and merge compare.
use crate::text;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Mark { Bold, Italic, Code, Strike, Underline, Link(String), Color(String), Highlight(String), /// the chip's payload JSON
    Choice(String) }

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum MarkKind { Bold, Italic, Code, Strike, Underline, Link, Color, Highlight, Choice }

impl Mark {
    pub fn kind(&self) -> MarkKind {
        match self { Mark::Bold => MarkKind::Bold, Mark::Italic => MarkKind::Italic, Mark::Code => MarkKind::Code, Mark::Strike => MarkKind::Strike, Mark::Underline => MarkKind::Underline,
            Mark::Link(_) => MarkKind::Link, Mark::Color(_) => MarkKind::Color, Mark::Highlight(_) => MarkKind::Highlight, Mark::Choice(_) => MarkKind::Choice }
    }
    /// The name in the file and its value, if the kind carries one.
    pub fn name(&self) -> (&'static str, Option<&str>) {
        match self { Mark::Bold => ("bold", None), Mark::Italic => ("italic", None), Mark::Code => ("code", None), Mark::Strike => ("strike", None), Mark::Underline => ("underline", None),
            Mark::Link(u) => ("link", Some(u)), Mark::Color(u) => ("color", Some(u)), Mark::Highlight(u) => ("highlight", Some(u)), Mark::Choice(u) => ("choice", Some(u)) }
    }
    /// From the file's name and value; an unknown name (the removed "comment" among them) is None and the span is dropped.
    pub fn from_name(name: &str, value: &str) -> Option<Mark> {
        Some(match name { "bold" => Mark::Bold, "italic" => Mark::Italic, "code" => Mark::Code, "strike" => Mark::Strike, "underline" => Mark::Underline,
            "link" => Mark::Link(value.into()), "color" => Mark::Color(value.into()), "highlight" => Mark::Highlight(value.into()), "choice" => Mark::Choice(value.into()), _ => return None })
    }
}
impl MarkKind { pub fn has_value(self) -> bool { matches!(self, MarkKind::Link | MarkKind::Color | MarkKind::Highlight | MarkKind::Choice) } }

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Span { pub start: usize, pub end: usize, pub mark: Mark }

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Spans(Vec<Span>);

impl Spans {
    pub fn new() -> Self { Self::default() }
    pub fn as_slice(&self) -> &[Span] { &self.0 }
    pub fn is_empty(&self) -> bool { self.0.is_empty() }
    /// Any list of spans, made canonical: empty ones go; of a valued kind a LATER span takes its range from earlier ones.
    pub fn from_vec(v: Vec<Span>) -> Self { let mut s = Spans::default(); for sp in v { s.put(sp.start, sp.end, sp.mark); } s }
    fn tidy(&mut self) {
        self.0.retain(|s| s.end > s.start);
        self.0.sort_by(|a, b| (a.mark.kind(), a.start, a.end).cmp(&(b.mark.kind(), b.start, b.end)));
        let mut out: Vec<Span> = Vec::with_capacity(self.0.len());
        for s in self.0.drain(..) {
            match out.last_mut() { Some(p) if p.mark == s.mark && s.start <= p.end => p.end = p.end.max(s.end), _ => out.push(s) }
        }
        self.0 = out;
    }
    /// Removes `kind` from [start, end): a span reaching past either side keeps its remainder (and its value).
    pub fn remove(&mut self, start: usize, end: usize, kind: MarkKind) {
        if end <= start { return }
        let mut out = Vec::with_capacity(self.0.len() + 1);
        for s in self.0.drain(..) {
            if s.mark.kind() != kind || s.end <= start || s.start >= end { out.push(s); continue }
            if s.start < start { out.push(Span { start: s.start, end: start, mark: s.mark.clone() }); }
            if s.end > end { out.push(Span { start: end, end: s.end, mark: s.mark }); }
        }
        self.0 = out; self.tidy();
    }
    /// Puts `mark` on [start, end). A plain mark joins what it touches; a valued one REPLACES its kind over the range
    /// (the rest of an older run keeps its value) and joins neighbours of the identical value. A chip is never cut: a
    /// chip the range only partly covers loses its chip-ness whole.
    pub fn put(&mut self, start: usize, end: usize, mark: Mark) {
        if end <= start { return }
        let kind = mark.kind();
        if kind == MarkKind::Choice { self.0.retain(|s| s.mark.kind() != MarkKind::Choice || s.end <= start || s.start >= end); }
        else if kind.has_value() { self.remove(start, end, kind); }
        self.0.push(Span { start, end, mark }); self.tidy();
    }
    /// True when spans of exactly `mark` cover [start, end) without a gap (an empty range is not covered).
    pub fn covers(&self, start: usize, end: usize, mark: &Mark) -> bool {
        if end <= start { return false }
        let mut at = start;
        for s in self.0.iter().filter(|s| &s.mark == mark) { if s.start > at { break } at = at.max(s.end); if at >= end { return true } }
        false
    }
    /// A plain mark's toggle: off when it covers the whole range, otherwise on over all of it. Returns the new state.
    pub fn toggle(&mut self, start: usize, end: usize, mark: Mark) -> bool {
        if self.covers(start, end, &mark) { self.remove(start, end, mark.kind()); false } else { self.put(start, end, mark); true }
    }
    /// Clear formatting: every kind except choice chips.
    pub fn clear(&mut self, start: usize, end: usize) {
        for k in [MarkKind::Bold, MarkKind::Italic, MarkKind::Code, MarkKind::Strike, MarkKind::Underline, MarkKind::Link, MarkKind::Color, MarkKind::Highlight] { self.remove(start, end, k); }
    }
    /// The chip holding `at` strictly inside or at its start (`start <= at < end`).
    pub fn chip_at(&self, at: usize) -> Option<&Span> { self.0.iter().find(|s| s.mark.kind() == MarkKind::Choice && s.start <= at && at < s.end) }
    /// An offset strictly inside a chip moves to the chip's end (a caret, a split point or an insertion never lands in one).
    pub fn snap_out_of_chip(&self, at: usize) -> usize { self.0.iter().find(|s| s.mark.kind() == MarkKind::Choice && s.start < at && at < s.end).map(|s| s.end).unwrap_or(at) }
    /// Follows a text edit: [at, at + removed) became `inserted` bytes.
    /// - a span that ends at or before `at` stays (typing at a span's end does not extend it);
    /// - a span that starts at or after the removed range shifts (typing at a span's start is outside it);
    /// - a span that covers the range stretches or shrinks with it (a corrected word keeps its bold or its link);
    /// - a span cut at one side keeps the part outside the range;
    /// - a CHIP the edit reaches into at all stops being a chip (its text is no longer the option's label).
    pub fn edit(&mut self, at: usize, removed: usize, inserted: usize) {
        let (s, e) = (at, at + removed); let delta = inserted as isize - removed as isize;
        let sh = |x: usize| (x as isize + delta) as usize;
        let mut out = Vec::with_capacity(self.0.len());
        for mut sp in self.0.drain(..) {
            if sp.end <= s { out.push(sp); continue }
            if sp.start >= e { sp.start = sh(sp.start); sp.end = sh(sp.end); out.push(sp); continue }
            if sp.mark.kind() == MarkKind::Choice { continue }
            if sp.start <= s && sp.end >= e { sp.end = sh(sp.end); }
            else if sp.start < s { sp.end = s; }
            else { sp.start = s + inserted; sp.end = sh(sp.end); }
            out.push(sp);
        }
        self.0 = out; self.tidy();
    }
    /// Splits at `at`: what lies before stays, what lies after is returned re-based to 0; a span across the point is
    /// on both sides. A chip is never cut (`at` is expected snapped; a chip across it stays whole on the left).
    pub fn split(&mut self, at: usize) -> Spans {
        let mut right = Vec::new(); let mut left = Vec::with_capacity(self.0.len());
        for sp in self.0.drain(..) {
            if sp.mark.kind() == MarkKind::Choice && sp.start < at && sp.end > at { left.push(sp); continue }
            if sp.end > at { right.push(Span { start: sp.start.max(at) - at, end: sp.end - at, mark: sp.mark.clone() }); }
            if sp.start < at { left.push(Span { start: sp.start, end: sp.end.min(at), mark: sp.mark }); }
        }
        self.0 = left; self.tidy();
        let mut r = Spans(right); r.tidy(); r
    }
    /// Joins `other` (the formatting of text appended at byte `len`) onto this; spans that now touch become one.
    pub fn append(&mut self, len: usize, other: &Spans) {
        self.0.extend(other.0.iter().map(|s| Span { start: s.start + len, end: s.end + len, mark: s.mark.clone() }));
        self.tidy();
    }
    /// From the file's form: (start, end) in UTF-16 units of `text`, a kind name and its value. Offsets are clamped to
    /// the text and moved off the middle of a character; unknown kinds and empty spans are dropped.
    pub fn from_utf16<'a>(text: &str, spans: impl IntoIterator<Item = (usize, usize, &'a str, &'a str)>) -> Spans {
        let mut v = Vec::new();
        for (s, e, name, value) in spans { if let Some(mark) = Mark::from_name(name, value) { v.push(Span { start: text::to8(text, s), end: text::to8(text, e), mark }); } }
        Spans::from_vec(v)
    }
    /// To the file's form: (start, end) in UTF-16 units, the kind's name, its value.
    pub fn to_utf16<'a>(&'a self, text: &str) -> Vec<(usize, usize, &'static str, Option<&'a str>)> {
        self.0.iter().map(|s| { let (n, v) = s.mark.name(); (text::to16(text, s.start), text::to16(text, s.end), n, v) }).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn sp(v: &[(usize, usize, Mark)]) -> Spans { Spans::from_vec(v.iter().cloned().map(|(start, end, mark)| Span { start, end, mark }).collect()) }
    fn list(s: &Spans) -> Vec<(usize, usize, Mark)> { s.as_slice().iter().map(|s| (s.start, s.end, s.mark.clone())).collect() }
    fn link(u: &str) -> Mark { Mark::Link(u.into()) }
    fn color(u: &str) -> Mark { Mark::Color(u.into()) }

    /// The old engine's cases (spec §1.8, test 66).
    #[test]
    fn plain_marks_join_what_they_touch_and_a_removal_can_split_one() {
        let mut s = sp(&[(2, 5, Mark::Bold), (5, 8, Mark::Bold)]);
        assert_eq!(list(&s), [(2, 8, Mark::Bold)]);
        s.remove(4, 6, MarkKind::Bold);
        assert_eq!(list(&s), [(2, 4, Mark::Bold), (6, 8, Mark::Bold)]);
        assert!(s.toggle(3, 7, Mark::Bold));                                    // partly covered → on over all of it
        assert_eq!(list(&s), [(2, 8, Mark::Bold)]);
        assert!(!s.toggle(2, 8, Mark::Bold));
        assert!(s.is_empty());
        assert!(!s.covers(3, 3, &Mark::Bold));
    }
    /// One rule for valued kinds (row 284): recolouring part of a run keeps the rest of the run its colour.
    #[test]
    fn a_valued_mark_takes_its_range_and_joins_an_equal_neighbour() {
        let mut s = sp(&[(0, 10, color("#f00"))]);
        s.put(4, 6, color("#00f"));
        assert_eq!(list(&s), [(0, 4, color("#f00")), (4, 6, color("#00f")), (6, 10, color("#f00"))]);
        s.put(4, 6, color("#f00"));
        assert_eq!(list(&s), [(0, 10, color("#f00"))]);
        s.put(2, 4, link("a")); s.put(4, 6, link("a")); s.put(6, 8, link("b"));
        assert_eq!(s.as_slice().iter().filter(|x| x.mark.kind() == MarkKind::Link).count(), 2);
        assert!(s.covers(2, 6, &link("a")) && !s.covers(2, 8, &link("a")));
        s.clear(0, 10);
        assert!(s.is_empty());
    }
    #[test]
    fn an_edit_moves_spans_as_typing_expects() {
        let base = sp(&[(4, 8, Mark::Bold), (4, 8, link("u"))]);
        let after = |at, removed, inserted| { let mut s = base.clone(); s.edit(at, removed, inserted); list(&s) };
        assert_eq!(after(4, 0, 2), [(6, 10, Mark::Bold), (6, 10, link("u"))]);       // at the start: outside, shifts
        assert_eq!(after(6, 0, 2), [(4, 10, Mark::Bold), (4, 10, link("u"))]);       // inside: grows
        assert_eq!(after(8, 0, 2), [(4, 8, Mark::Bold), (4, 8, link("u"))]);         // at the end: does not extend
        assert_eq!(after(4, 4, 6), [(4, 10, Mark::Bold), (4, 10, link("u"))]);       // the whole word corrected: keeps both
        assert_eq!(after(2, 4, 0), [(2, 4, Mark::Bold), (2, 4, link("u"))]);         // a cut over the head
        assert_eq!(after(6, 4, 0), [(4, 6, Mark::Bold), (4, 6, link("u"))]);         // a cut over the tail
        assert_eq!(after(2, 8, 0), []);                                                // the whole span deleted
        assert_eq!(after(5, 2, 1), [(4, 7, Mark::Bold), (4, 7, link("u"))]);         // a cut inside shrinks it
        // two bold runs whose gap is deleted become one (the old engine left two)
        let mut s = sp(&[(0, 3, Mark::Bold), (5, 8, Mark::Bold)]); s.edit(3, 2, 0);
        assert_eq!(list(&s), [(0, 6, Mark::Bold)]);
    }
    #[test]
    fn a_chip_is_whole_or_not_a_chip() {
        let chip = Mark::Choice("{\"o\":[],\"v\":\"1\"}".into());
        let base = sp(&[(2, 6, chip.clone()), (0, 8, Mark::Bold)]);
        let after = |at, removed, inserted| { let mut s = base.clone(); s.edit(at, removed, inserted); s };
        assert_eq!(after(2, 0, 1).chip_at(3).map(|c| (c.start, c.end)), Some((3, 7)));   // typing before it moves it
        assert_eq!(after(6, 0, 1).chip_at(2).map(|c| (c.start, c.end)), Some((2, 6)));   // typing after it leaves it
        assert!(after(4, 0, 1).chip_at(4).is_none());                                    // typing inside: no longer a chip
        assert!(after(5, 3, 0).chip_at(2).is_none());                                    // a partial delete
        assert!(after(2, 4, 4).chip_at(2).is_none());                                    // its text replaced
        assert_eq!(list(&after(4, 0, 1)), [(0, 9, Mark::Bold)]);                         // other formatting follows as usual
        assert_eq!((base.snap_out_of_chip(4), base.snap_out_of_chip(2), base.snap_out_of_chip(6)), (6, 2, 6));
        let mut s = base.clone(); s.clear(0, 8);
        assert_eq!(list(&s), [(2, 6, chip)]);
    }
    #[test]
    fn a_split_and_a_join_are_inverse() {
        let base = sp(&[(1, 7, Mark::Italic), (3, 5, link("u")), (6, 9, color("#0f0"))]);
        let mut left = base.clone(); let right = left.split(4);
        assert_eq!(list(&left), [(1, 4, Mark::Italic), (3, 4, link("u"))]);
        assert_eq!(list(&right), [(0, 3, Mark::Italic), (0, 1, link("u")), (2, 5, color("#0f0"))]);
        left.append(4, &right);
        assert_eq!(left, base);
    }
    #[test]
    fn the_files_utf16_offsets_convert_at_the_edge() {
        let text = "a\u{1F600}b bold";   // the emoji: 4 bytes, 2 UTF-16 units
        let s = Spans::from_utf16(text, [(5, 9, "bold", ""), (1, 3, "link", "u"), (0, 99, "comment", ""), (1, 2, "italic", ""), (7, 7, "code", "")]);
        assert_eq!(list(&s), [(7, 11, Mark::Bold), (1, 5, link("u"))]);              // "comment" unknown; half an emoji and an empty span are nothing
        assert_eq!(s.to_utf16(text), [(5, 9, "bold", None), (1, 3, "link", Some("u"))]);
    }
}
