//! minNotes — the editor of one note, without a window (Phase 3 step 2). It owns the document, its history and the
//! selection; a window feeds it clicks, keys and text and redraws from what it reports changed.
//!
//! A POSITION is a leaf block and an offset in it: a byte offset on a character boundary for a text block; for a block
//! without text (a picture, a divider) 0 = before it, 1 = after it. The selection is an anchor and a focus; it may run
//! across blocks, columns and table cells. Every edit goes through `mn_doc::change` — one history, and a typed burst
//! is one undo step (row 275).
use kurbo::Rect;
use mn_doc::{Block, BlockId, Change, ChangeError, Doc, History, Kind, Rank, Spans};
use mn_layout::{NoteLayout, Part};
use parley::{Affinity, Cursor, Selection as PSel};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Pos { pub block: BlockId, pub at: usize }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Selection { pub anchor: Pos, pub focus: Pos }
impl Selection { pub fn caret(p: Pos) -> Self { Selection { anchor: p, focus: p } } pub fn is_caret(&self) -> bool { self.anchor == self.focus } }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Move { Left, Right, Up, Down, LineStart, LineEnd, WordLeft, WordRight, DocStart, DocEnd }

/// What an edit asks of the layout.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Dirty { Nothing, /// these blocks' text, formatting or kind changed: re-shape them
    Blocks(Vec<BlockId>), /// blocks came, went or moved: lay the note out again
    Structure }

pub struct Editor {
    pub doc: Doc,
    pub history: History,
    sel: Option<Selection>,
    /// the x a run of Up / Down moves keeps returning to
    goal_x: Option<f64>,
    /// the caret stands at a soft wrap and belongs to the END of the upper line (End put it there)
    upstream: bool,
    /// the selection before and after each undo step, alongside the history's steps
    marks: Vec<(Option<Selection>, Option<Selection>)>,
    redo_marks: Vec<(Option<Selection>, Option<Selection>)>,
}

/// Changes built one after another, each against the document as the ones before it leave it.
struct Batch { scratch: Doc, changes: Vec<Change> }
impl Batch {
    fn push(&mut self, c: Change) -> Result<(), ChangeError> { self.scratch.apply(&c)?; self.changes.push(c); Ok(()) }
    fn edit(&mut self, id: BlockId, at: usize, removed: usize, inserted: &str) -> Result<(), ChangeError> { let c = Change::edit(&self.scratch, id, at, removed, inserted)?; self.push(c) }
}

fn leaves(doc: &Doc) -> Vec<BlockId> { doc.walk().iter().filter(|b| !b.kind.is_container()).map(|b| b.id).collect() }
fn end_of(b: &Block) -> usize { if b.kind.is_text() { b.text.len() } else { 1 } }
fn prev_char(s: &str, at: usize) -> usize { s[..at].char_indices().next_back().map(|(i, _)| i).unwrap_or(0) }
fn next_char(s: &str, at: usize) -> usize { s[at..].chars().next().map(|c| at + c.len_utf8()).unwrap_or(s.len()) }
fn paragraph(parent: Option<BlockId>, text: &str) -> Block { Block { id: BlockId::new(), parent, rank: Rank::first(), kind: Kind::Paragraph, text: text.into(), spans: Spans::new(), conflict: None, children: vec![] } }

impl Editor {
    pub fn new(doc: Doc) -> Editor { Editor { doc, history: History::new(), sel: None, goal_x: None, upstream: false, marks: vec![], redo_marks: vec![] } }
    pub fn selection(&self) -> Option<Selection> { self.sel }
    pub fn set_selection(&mut self, s: Option<Selection>) { self.sel = s.map(|s| Selection { anchor: self.valid(s.anchor), focus: self.valid(s.focus) }); self.goal_x = None; self.upstream = false; }
    /// A position made legal: on a character boundary, inside the block, and never inside a chip.
    fn valid(&self, p: Pos) -> Pos {
        let Some(b) = self.doc.get(p.block) else { return p };
        if !b.kind.is_text() { return Pos { block: p.block, at: p.at.min(1) } }
        let mut at = p.at.min(b.text.len()); while !b.text.is_char_boundary(at) { at -= 1; }
        Pos { block: p.block, at: b.spans.snap_out_of_chip(at) }
    }
    /// The selection's two ends in document order.
    fn ordered(&self) -> Option<(Pos, Pos)> {
        let s = self.sel?; if s.anchor.block == s.focus.block { return Some(if s.anchor.at <= s.focus.at { (s.anchor, s.focus) } else { (s.focus, s.anchor) }) }
        let l = leaves(&self.doc); let (a, f) = (l.iter().position(|x| *x == s.anchor.block)?, l.iter().position(|x| *x == s.focus.block)?);
        Some(if a < f { (s.anchor, s.focus) } else { (s.focus, s.anchor) })
    }

    // ── pointing
    /// A press at a point of the note (the layout's coordinates); `extend` keeps the anchor (Shift, or a drag).
    pub fn point(&mut self, layout: &NoteLayout, x: f64, y: f64, extend: bool) {
        let Some((block, at)) = layout.hit(&self.doc, x, y) else { return };
        let p = self.valid(Pos { block, at });
        self.sel = Some(match (extend, self.sel) { (true, Some(s)) => Selection { anchor: s.anchor, focus: p }, _ => Selection::caret(p) });
        self.goal_x = None; self.upstream = false;
    }
    /// The word under a point (a double click).
    pub fn point_word(&mut self, layout: &NoteLayout, x: f64, y: f64) {
        let Some((block, at)) = layout.hit(&self.doc, x, y) else { return };
        let (Some(b), Some(Part::Text { layout: l, .. })) = (self.doc.get(block), layout.get(block).map(|b| &b.part)) else { self.sel = Some(Selection { anchor: Pos { block, at: 0 }, focus: Pos { block, at: 1 } }); return };
        let c = Cursor::from_byte_index(l, at.min(b.text.len()), Affinity::Downstream);
        let (s, e) = (c.next_visual_word(l).previous_visual_word(l).index(), c.previous_visual_word(l).next_visual_word(l).index());
        let (s, e) = if s <= at && at <= e && s < e { (s, e) } else { (c.previous_visual_word(l).index().min(at), c.next_visual_word(l).index().max(at)) };
        self.sel = Some(Selection { anchor: self.valid(Pos { block, at: s }), focus: self.valid(Pos { block, at: e }) }); self.goal_x = None;
    }

    // ── moving
    pub fn move_caret(&mut self, layout: &NoteLayout, m: Move, extend: bool) {
        let Some(s) = self.sel else { return };
        // without Shift, Left / Right on a range go to that end of it
        if !extend && !s.is_caret() && matches!(m, Move::Left | Move::Right) { let (a, b) = self.ordered().unwrap(); self.sel = Some(Selection::caret(if m == Move::Left { a } else { b })); self.goal_x = None; return }
        let from = s.focus; let l = leaves(&self.doc);
        let aff = if self.upstream { Affinity::Upstream } else { Affinity::Downstream }; self.upstream = false;
        let Some(i) = l.iter().position(|x| *x == from.block) else { return };
        let b = self.doc.get(from.block).unwrap();
        let text = match layout.get(from.block).map(|b| &b.part) { Some(Part::Text { layout, .. }) => Some(layout), _ => None };
        let step_out = |fwd: bool| -> Pos { if fwd { l.get(i + 1).map(|n| Pos { block: *n, at: 0 }).unwrap_or(from) } else { i.checked_sub(1).map(|p| Pos { block: l[p], at: end_of(self.doc.get(l[p]).unwrap()) }).unwrap_or(from) } };
        let mut keep_goal = false;
        let to = match m {
            Move::Left | Move::WordLeft => if from.at == 0 { step_out(false) } else { match text { Some(t) => { let c = Cursor::from_byte_index(t, from.at, Affinity::Downstream); let n = if m == Move::Left { c.previous_visual(t) } else { c.previous_visual_word(t) }.index(); Pos { block: from.block, at: if n < from.at { n } else { prev_char(&b.text, from.at) } } } None => Pos { block: from.block, at: 0 } } },
            Move::Right | Move::WordRight => if from.at >= end_of(b) { step_out(true) } else { match text { Some(t) => { let c = Cursor::from_byte_index(t, from.at, Affinity::Downstream); let n = if m == Move::Right { c.next_visual(t) } else { c.next_visual_word(t) }.index(); Pos { block: from.block, at: if n > from.at { n } else { next_char(&b.text, from.at) } } } None => Pos { block: from.block, at: 1 } } },
            Move::LineStart | Move::LineEnd => match text { Some(t) => { let sel = PSel::from_byte_index(t, from.at, aff); self.upstream = m == Move::LineEnd; Pos { block: from.block, at: if m == Move::LineStart { sel.line_start(t, false) } else { sel.line_end(t, false) }.focus().index() } } None => Pos { block: from.block, at: if m == Move::LineStart { 0 } else { 1 } } },
            Move::DocStart => l.first().map(|f| Pos { block: *f, at: 0 }).unwrap_or(from),
            Move::DocEnd => l.last().map(|f| Pos { block: *f, at: end_of(self.doc.get(*f).unwrap()) }).unwrap_or(from),
            Move::Up | Move::Down => {
                keep_goal = true;
                let Some(c) = layout.caret_side(&self.doc, from.block, from.at, 1.0, aff == Affinity::Upstream) else { return };
                let x = *self.goal_x.get_or_insert(c.x0);
                // a line up or down: probe just outside the caret's line; at the note's edge go to the block's end
                let mut y = if m == Move::Up { c.y0 - 2.0 } else { c.y1 + 2.0 }; let mut found = from;
                for _ in 0..40 { match layout.hit(&self.doc, x, y) { Some((blk, at)) => { let p = Pos { block: blk, at }; let moved = layout.caret(&self.doc, blk, at, 1.0).is_some_and(|r| if m == Move::Up { r.y0 < c.y0 - 0.5 } else { r.y0 > c.y0 + 0.5 }); if moved { found = p; break } } None => break } y += if m == Move::Up { -6.0 } else { 6.0 }; }
                if found == from { if m == Move::Up { Pos { block: from.block, at: 0 } } else { Pos { block: from.block, at: end_of(b) } } } else { found }
            }
        };
        let to = self.valid(to);
        self.sel = Some(if extend { Selection { anchor: s.anchor, focus: to } } else { Selection::caret(to) });
        if !keep_goal { self.goal_x = None; }
    }
    pub fn select_all(&mut self) {
        let l = leaves(&self.doc);
        if let (Some(a), Some(z)) = (l.first(), l.last()) { self.sel = Some(Selection { anchor: Pos { block: *a, at: 0 }, focus: Pos { block: *z, at: end_of(self.doc.get(*z).unwrap()) } }); }
    }

    // ── geometry for the painter
    pub fn caret_rect(&self, layout: &NoteLayout, width: f32) -> Option<Rect> { let s = self.sel?; layout.caret_side(&self.doc, s.focus.block, s.focus.at, width, self.upstream) }
    /// The rectangles of the selection, block by block (none for a caret).
    pub fn selection_rects(&self, layout: &NoteLayout) -> Vec<Rect> {
        let Some((a, b)) = self.ordered() else { return vec![] }; if a == b { return vec![] }
        if a.block == b.block { return layout.range_rects(&self.doc, a.block, a.at, b.at) }
        let l = leaves(&self.doc); let (Some(i), Some(j)) = (l.iter().position(|x| *x == a.block), l.iter().position(|x| *x == b.block)) else { return vec![] };
        let mut out = vec![];
        for id in &l[i..=j] { let blk = self.doc.get(*id).unwrap(); let (s, e) = (if *id == a.block { a.at } else { 0 }, if *id == b.block { b.at } else { end_of(blk) }); out.extend(layout.range_rects(&self.doc, *id, s, e)); }
        out
    }
    /// The selected text, blocks joined by a line break (what Copy puts on the clipboard as plain text).
    pub fn selected_text(&self) -> String {
        let Some((a, b)) = self.ordered() else { return String::new() }; let l = leaves(&self.doc);
        let (Some(i), Some(j)) = (l.iter().position(|x| *x == a.block), l.iter().position(|x| *x == b.block)) else { return String::new() };
        l[i..=j].iter().filter_map(|id| { let blk = self.doc.get(*id)?; if !blk.kind.is_text() { return Some(String::new()) } let (s, e) = (if *id == a.block { a.at } else { 0 }, if *id == b.block { b.at } else { blk.text.len() }); Some(blk.text[s..e].to_string()) }).collect::<Vec<_>>().join("\n")
    }

    // ── editing
    fn batch(&self) -> Batch { Batch { scratch: self.doc.clone(), changes: vec![] } }
    /// Removes the selection (into `b`) and returns where the caret is afterwards.
    fn delete_selection(&self, b: &mut Batch) -> Result<Option<Pos>, ChangeError> {
        let Some((s, e)) = self.ordered() else { return Ok(None) }; if s == e { return Ok(Some(s)) }
        if s.block == e.block {
            let blk = self.doc.get(s.block).unwrap();
            if blk.kind.is_text() { b.edit(s.block, s.at, e.at - s.at, "")?; return Ok(Some(s)) }
            // a selected picture or divider: it goes; the caret lands on a neighbour, or in a new empty paragraph
            let (parent, index) = (blk.parent, self.doc.index_of(s.block).unwrap());
            let l = leaves(&self.doc); let i = l.iter().position(|x| *x == s.block).unwrap();
            let c = Change::removal(&b.scratch, s.block)?; b.push(c)?;
            if let Some(n) = l.get(i + 1).filter(|n| self.doc.get(**n).is_some_and(|x| x.parent == parent)) { return Ok(Some(Pos { block: *n, at: 0 })) }
            if let Some(p) = i.checked_sub(1).map(|p| l[p]).filter(|p| self.doc.get(*p).is_some_and(|x| x.parent == parent)) { return Ok(Some(Pos { block: p, at: end_of(self.doc.get(p).unwrap()) })) }
            let para = paragraph(parent, ""); let id = para.id; b.push(Change::Insert { index, blocks: vec![para] })?;
            return Ok(Some(Pos { block: id, at: 0 }))
        }
        let (first, last) = (self.doc.get(s.block).unwrap(), self.doc.get(e.block).unwrap());
        if first.parent == last.parent {
            // one list: the ends are trimmed, everything between goes (containers too), the ends join when both are text
            let sibs = self.doc.children(first.parent).to_vec(); let (i, j) = (sibs.iter().position(|x| *x == s.block).unwrap(), sibs.iter().position(|x| *x == e.block).unwrap());
            for mid in &sibs[i + 1..j] { let c = Change::removal(&b.scratch, *mid)?; b.push(c)?; }
            match (first.kind.is_text(), last.kind.is_text()) {
                (true, true) => { b.edit(s.block, s.at, first.text.len() - s.at, "")?; b.edit(e.block, 0, e.at, "")?; let j = Change::join(&b.scratch, s.block, e.block)?; for c in j { b.push(c)?; } }
                (true, false) => { b.edit(s.block, s.at, first.text.len() - s.at, "")?; if e.at == 1 { let c = Change::removal(&b.scratch, e.block)?; b.push(c)?; } }
                (false, true) => { b.edit(e.block, 0, e.at, "")?; if s.at == 0 { let c = Change::removal(&b.scratch, s.block)?; b.push(c)?; return Ok(Some(Pos { block: e.block, at: 0 })) } }
                (false, false) => { if e.at == 1 { let c = Change::removal(&b.scratch, e.block)?; b.push(c)?; } }
            }
            return Ok(Some(s))
        }
        // across containers (two columns, two cells): the structure stays; the text in the range goes
        let l = leaves(&self.doc); let (i, j) = (l.iter().position(|x| *x == s.block).unwrap(), l.iter().position(|x| *x == e.block).unwrap());
        for id in &l[i..=j] { let blk = self.doc.get(*id).unwrap(); if !blk.kind.is_text() { continue }
            let (from, to) = (if *id == s.block { s.at } else { 0 }, if *id == e.block { e.at } else { blk.text.len() }); if to > from { b.edit(*id, from, to - from, "")?; } }
        Ok(Some(s))
    }
    fn commit(&mut self, b: Batch, after: Option<Pos>, now_ms: u64) -> Result<Dirty, ChangeError> {
        if b.changes.is_empty() { if let Some(p) = after { self.sel = Some(Selection::caret(self.valid(p))); } return Ok(Dirty::Nothing) }
        let structural = b.changes.iter().any(|c| matches!(c, Change::Insert { .. } | Change::Remove { .. } | Change::Move { .. }));
        let mut ids: Vec<BlockId> = b.changes.iter().filter_map(|c| match c { Change::Text { id, .. } | Change::Format { id, .. } | Change::Kind { id, .. } => Some(*id), _ => None }).collect(); ids.dedup();
        let before = self.sel;
        let joined = self.history.commit(&mut self.doc, b.changes, now_ms)?;
        self.sel = after.map(|p| Selection::caret(self.valid(p))); self.goal_x = None; self.upstream = false; self.redo_marks.clear();
        if joined { if let Some(m) = self.marks.last_mut() { m.1 = self.sel; } } else { self.marks.push((before, self.sel)); }
        Ok(if structural { Dirty::Structure } else { Dirty::Blocks(ids) })
    }
    /// Typed or pasted text replaces the selection. In a block without text it starts a paragraph beside it. A line
    /// break inside the text splits the block (stays a line break in a code block).
    pub fn insert(&mut self, text: &str, now_ms: u64) -> Result<Dirty, ChangeError> {
        let mut b = self.batch();
        let Some(mut p) = self.delete_selection(&mut b)? else { return Ok(Dirty::Nothing) };
        for (n, line) in text.replace("\r\n", "\n").replace('\r', "\n").split('\n').enumerate() {
            let blk = b.scratch.get(p.block).ok_or(mn_doc::TreeError::UnknownBlock(p.block))?.clone();
            if !blk.kind.is_text() {
                let index = b.scratch.index_of(p.block).unwrap() + p.at.min(1); let para = paragraph(blk.parent, line); let id = para.id;
                b.push(Change::Insert { index, blocks: vec![para] })?; p = Pos { block: id, at: line.len() }; continue
            }
            if n > 0 {
                if matches!(blk.kind, Kind::Code { .. }) { b.edit(p.block, p.at, 0, "\n")?; p.at += 1; }
                else { let (c, new) = Change::split(&b.scratch, p.block, p.at)?; for x in c { b.push(x)?; } p = Pos { block: new, at: 0 }; }
            }
            let at = b.scratch.get(p.block).map(|x| x.spans.snap_out_of_chip(p.at)).unwrap_or(p.at);
            if !line.is_empty() { b.edit(p.block, at, 0, line)?; }
            p = Pos { block: p.block, at: at + line.len() };
        }
        self.commit(b, Some(p), now_ms)
    }
    /// Enter.
    pub fn enter(&mut self, now_ms: u64) -> Result<Dirty, ChangeError> { self.insert("\n", now_ms) }
    /// Backspace (`forward` false) or Delete (true).
    pub fn delete(&mut self, forward: bool, now_ms: u64) -> Result<Dirty, ChangeError> {
        let Some(s) = self.sel else { return Ok(Dirty::Nothing) };
        let mut b = self.batch();
        if !s.is_caret() { let p = self.delete_selection(&mut b)?; return self.commit(b, p, now_ms) }
        let p = s.focus; let blk = self.doc.get(p.block).ok_or(mn_doc::TreeError::UnknownBlock(p.block))?.clone();
        let sibs = self.doc.children(blk.parent).to_vec(); let i = sibs.iter().position(|x| *x == p.block).unwrap();
        // inside the block
        if blk.kind.is_text() && ((!forward && p.at > 0) || (forward && p.at < blk.text.len())) {
            let (mut from, mut to) = if forward { (p.at, next_char(&blk.text, p.at)) } else { (prev_char(&blk.text, p.at), p.at) };
            // a chip goes whole
            if let Some(chip) = blk.spans.as_slice().iter().find(|c| c.mark.kind() == mn_doc::MarkKind::Choice && c.start < to && c.end > from) { from = from.min(chip.start); to = to.max(chip.end); }
            b.edit(p.block, from, to - from, "")?;
            return self.commit(b, Some(Pos { block: p.block, at: from }), now_ms)
        }
        if !blk.kind.is_text() && ((!forward && p.at == 1) || (forward && p.at == 0)) { self.sel = Some(Selection { anchor: Pos { block: p.block, at: 0 }, focus: Pos { block: p.block, at: 1 } }); return self.delete(forward, now_ms) }
        // at the block's edge: the neighbour in the same list
        let other = if forward { sibs.get(i + 1).copied() } else { i.checked_sub(1).map(|k| sibs[k]) };
        let Some(o) = other.and_then(|o| self.doc.get(o)).cloned() else {
            // nothing before it: a list item, heading or quote becomes a plain paragraph
            if !forward && blk.kind.is_text() && !matches!(blk.kind, Kind::Paragraph | Kind::Code { .. }) { b.push(Change::Kind { id: p.block, old: blk.kind.clone(), new: Kind::Paragraph })?; return self.commit(b, Some(p), now_ms) }
            return Ok(Dirty::Nothing)
        };
        if o.kind.is_container() || !blk.kind.is_text() { return Ok(Dirty::Nothing) }
        if !o.kind.is_text() { self.sel = Some(Selection { anchor: Pos { block: o.id, at: 0 }, focus: Pos { block: o.id, at: 1 } }); return Ok(Dirty::Nothing) }   // a picture is selected first, deleted by the next press
        let (first, second) = if forward { (blk.id, o.id) } else { (o.id, blk.id) };
        let at = self.doc.get(first).unwrap().text.len();
        for c in Change::join(&b.scratch, first, second)? { b.push(c)?; }
        self.commit(b, Some(Pos { block: first, at }), now_ms)
    }
    pub fn undo(&mut self) -> Result<Dirty, ChangeError> {
        if !self.history.undo(&mut self.doc)? { return Ok(Dirty::Nothing) }
        let m = self.marks.pop().unwrap_or((None, None)); self.sel = m.0.and_then(|s| self.live(s)); self.redo_marks.push(m); self.goal_x = None; Ok(Dirty::Structure)
    }
    pub fn redo(&mut self) -> Result<Dirty, ChangeError> {
        if !self.history.redo(&mut self.doc)? { return Ok(Dirty::Nothing) }
        let m = self.redo_marks.pop().unwrap_or((None, None)); self.sel = m.1.and_then(|s| self.live(s)); self.marks.push(m); self.goal_x = None; Ok(Dirty::Structure)
    }
    fn live(&self, s: Selection) -> Option<Selection> { (self.doc.get(s.anchor.block).is_some() && self.doc.get(s.focus.block).is_some()).then(|| Selection { anchor: self.valid(s.anchor), focus: self.valid(s.focus) }) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use min_ui::fonts::Fonts;
    use mn_layout::Style;
    struct T { e: Editor, f: Fonts, l: NoteLayout, ids: Vec<BlockId>, now: u64 }
    fn add(d: &mut Doc, parent: Option<BlockId>, kind: Kind, text: &str) -> BlockId { let id = BlockId::new(); let at = d.children(parent).len(); d.insert(parent, at, id, kind, text).unwrap(); id }
    const LONG: &str = "The grade on the wide shot holds up well against the reference, and the titles sit inside the safe area on every format we checked this morning.";
    fn setup() -> T {
        let mut d = Doc::new();
        let mut ids = vec![add(&mut d, None, Kind::Heading { level: 1 }, "Title"), add(&mut d, None, Kind::Paragraph, LONG), add(&mut d, None, Kind::Media { descriptor: r#"{"w":200,"h":100}"#.into() }, ""), add(&mut d, None, Kind::Bullet { depth: 0 }, "item"), add(&mut d, None, Kind::Paragraph, "last")];
        let cols = add(&mut d, None, Kind::Columns, ""); let (c1, c2) = (add(&mut d, Some(cols), Kind::column(), ""), add(&mut d, Some(cols), Kind::column(), ""));
        ids.push(add(&mut d, Some(c1), Kind::Paragraph, "left")); ids.push(add(&mut d, Some(c2), Kind::Paragraph, "right"));
        let mut f = Fonts::new(); let l = NoteLayout::build(&d, &mut f, &Style::default(), 400.0, 1.0);
        T { e: Editor::new(d), f, l, ids, now: 0 }
    }
    impl T {
        fn relay(&mut self, d: Dirty) { match d { Dirty::Blocks(ids) => for id in ids { self.l.update_block(&self.e.doc, &mut self.f, id) }, Dirty::Structure => self.l = NoteLayout::build(&self.e.doc, &mut self.f, &Style::default(), 400.0, 1.0), Dirty::Nothing => {} } }
        fn at(&mut self, i: usize, at: usize) { self.e.set_selection(Some(Selection::caret(Pos { block: self.ids[i], at }))); }
        fn type_(&mut self, s: &str) { self.now += 1000; let d = self.e.insert(s, self.now).unwrap(); self.relay(d); }
        fn del(&mut self, fwd: bool) { self.now += 1000; let d = self.e.delete(fwd, self.now).unwrap(); self.relay(d); }
        fn mv(&mut self, m: Move, ext: bool) { self.e.move_caret(&self.l, m, ext); }
        fn pos(&self) -> (usize, usize) { let p = self.e.selection().unwrap().focus; (self.ids.iter().position(|x| *x == p.block).unwrap_or(99), p.at) }
        fn text(&self, i: usize) -> String { self.e.doc.get(self.ids[i]).map(|b| b.text.clone()).unwrap_or_else(|| "<gone>".into()) }
    }

    #[test]
    fn typing_enter_and_backspace_shape_the_note_and_undo_restores_text_and_caret() {
        let mut t = setup(); let before = t.e.doc.clone();
        t.at(0, 5); t.type_("!"); assert_eq!((t.text(0), t.pos()), ("Title!".into(), (0, 6)));
        // Enter in the middle of a heading: the rest becomes a paragraph below, the caret at its start
        t.at(0, 2); t.now += 1000; let d = t.e.enter(t.now).unwrap(); assert_eq!(d, Dirty::Structure); t.relay(d);
        let new = t.e.selection().unwrap().focus;
        assert_eq!((t.text(0), t.e.doc.get(new.block).unwrap().text.as_str(), new.at, &t.e.doc.get(new.block).unwrap().kind), ("Ti".into(), "tle!", 0, &Kind::Paragraph));
        // Backspace at its start joins it back; the caret sits at the seam
        t.del(false); assert_eq!((t.text(0), t.pos()), ("Title!".into(), (0, 2)));
        // Backspace after a picture selects it; again deletes it
        t.at(3, 0); t.del(false); assert_eq!(t.e.selection().unwrap(), Selection { anchor: Pos { block: t.ids[2], at: 0 }, focus: Pos { block: t.ids[2], at: 1 } });
        t.del(false); assert!(t.e.doc.get(t.ids[2]).is_none()); assert_eq!(t.pos(), (3, 0));
        // Backspace at the start of the first list item with text before it joins; at the very first block it un-lists
        t.at(0, 0); t.del(false); assert_eq!(t.e.doc.get(t.ids[0]).unwrap().kind, Kind::Paragraph);
        // undo, step by step, back to the start — the caret returns to where each step began
        let mut n = 0; while t.e.undo().unwrap() != Dirty::Nothing { n += 1; }
        assert_eq!((n, &t.e.doc), (5, &before));
        assert_eq!(t.e.selection().map(|s| s.focus), Some(Pos { block: t.ids[0], at: 5 }));
        t.e.redo().unwrap(); assert_eq!((t.text(0), t.e.selection().unwrap().focus.at), ("Title!".into(), 6));
    }
    /// Row 275 and the spike S9: a burst of typed chunks is one step, and the caret after undo is where the burst began.
    #[test]
    fn a_typed_burst_is_one_undo_step() {
        let mut t = setup(); t.at(4, 4);
        for (i, ch) in [" ", "w", "o", "r", "d"].iter().enumerate() { let d = t.e.insert(ch, 5000 + i as u64 * 20).unwrap(); t.relay(d); }
        assert_eq!((t.text(4), t.e.history.steps()), ("last word".into(), 1));
        t.e.undo().unwrap(); assert_eq!((t.text(4), t.pos()), ("last".into(), (4, 4)));
    }
    #[test]
    fn a_selection_across_blocks_is_replaced_by_what_is_typed() {
        let mut t = setup();
        // from inside the heading to inside "item": the paragraph and the picture between them go, the ends join
        t.e.set_selection(Some(Selection { anchor: Pos { block: t.ids[0], at: 2 }, focus: Pos { block: t.ids[3], at: 2 } }));
        assert_eq!(t.e.selected_text(), format!("tle\n{LONG}\n\nit"));
        assert!(t.e.selection_rects(&t.l).len() >= 4);
        t.type_("X");
        assert_eq!((t.text(0), t.pos(), t.e.doc.children(None).len()), ("TiXem".into(), (0, 3), 3));
        assert_eq!(t.e.doc.get(t.ids[0]).unwrap().kind, Kind::Heading { level: 1 });
        t.e.undo().unwrap(); assert_eq!((t.text(0), t.text(3), t.e.doc.children(None).len()), ("Title".into(), "item".into(), 6));
        // across two columns: the structure stays, the selected text goes
        t.e.set_selection(Some(Selection { anchor: Pos { block: t.ids[5], at: 1 }, focus: Pos { block: t.ids[6], at: 3 } }));
        t.del(false); assert_eq!((t.text(5), t.text(6)), ("l".into(), "ht".into()));
        // pasted lines become blocks
        t.at(4, 4); t.type_(" a\nb\nc"); assert_eq!(t.text(4), "last a");
        let tail: Vec<String> = t.e.doc.children(None).iter().filter_map(|c| t.e.doc.get(*c)).filter(|b| b.kind.is_text()).map(|b| b.text.clone()).collect();
        assert_eq!(&tail[tail.len() - 3..], ["last a", "b", "c"]);
        // select all + type: one paragraph remains at the top level
        t.e.select_all(); t.type_("fresh");
        assert_eq!(t.e.doc.walk().iter().filter(|b| b.kind.is_text() && !b.text.is_empty()).map(|b| b.text.as_str()).collect::<Vec<_>>(), ["fresh"]);
    }
    #[test]
    fn arrows_walk_through_blocks_lines_and_pictures() {
        let mut t = setup();
        t.at(0, 5); t.mv(Move::Right, false); assert_eq!(t.pos(), (1, 0));                 // off the heading's end, into the paragraph
        t.mv(Move::Left, false); assert_eq!(t.pos(), (0, 5));
        t.at(1, LONG.len()); t.mv(Move::Right, false); assert_eq!(t.pos(), (2, 0));        // before the picture
        t.mv(Move::Right, false); assert_eq!(t.pos(), (2, 1)); t.mv(Move::Right, false); assert_eq!(t.pos(), (3, 0));
        // Down through a wrapped paragraph moves a line at a time and keeps its column; Up comes back
        t.at(1, 10); let x0 = t.e.caret_rect(&t.l, 1.0).unwrap();
        t.mv(Move::Down, false); let x1 = t.e.caret_rect(&t.l, 1.0).unwrap();
        assert!(t.pos().0 == 1 && t.pos().1 > 10 && x1.y0 > x0.y0 && (x1.x0 - x0.x0).abs() < 8.0, "{:?} {x0:?} {x1:?}", t.pos());
        t.mv(Move::Up, false); assert_eq!(t.pos(), (1, 10));
        t.mv(Move::Up, false); assert_eq!(t.pos().0, 0);                                    // into the heading
        t.mv(Move::Up, false); assert_eq!(t.pos(), (0, 0));                                 // nothing above: the start
        // line and word moves, with Shift extending
        t.at(1, 10); t.mv(Move::LineEnd, true); let s = t.e.selection().unwrap(); assert!(s.anchor.at == 10 && s.focus.at > 30 && s.focus.at < LONG.len());
        t.mv(Move::LineStart, false); assert_eq!(t.pos(), (1, 0));
        t.mv(Move::WordRight, false); assert!(t.pos().1 >= 3 && t.pos().1 <= 4);
        t.mv(Move::DocEnd, false); assert_eq!(t.pos(), (6, 5)); t.mv(Move::DocStart, false); assert_eq!(t.pos(), (0, 0));
        // a range collapses to its end on Left / Right
        t.e.set_selection(Some(Selection { anchor: Pos { block: t.ids[1], at: 3 }, focus: Pos { block: t.ids[3], at: 2 } }));
        t.mv(Move::Left, false); assert_eq!(t.pos(), (1, 3));
    }
    #[test]
    fn a_click_a_drag_and_a_double_click_select() {
        let mut t = setup();
        let c = t.l.caret(&t.e.doc, t.ids[1], 20, 1.0).unwrap(); let d = t.l.caret(&t.e.doc, t.ids[3], 2, 1.0).unwrap();
        t.e.point(&t.l, c.x0 + 0.2, c.center().y, false); assert_eq!(t.pos(), (1, 20));
        t.e.point(&t.l, d.x0 + 0.2, d.center().y, true);
        assert_eq!(t.e.selection().unwrap(), Selection { anchor: Pos { block: t.ids[1], at: 20 }, focus: Pos { block: t.ids[3], at: 2 } });
        let w = t.l.caret(&t.e.doc, t.ids[1], LONG.find("wide").unwrap() + 2, 1.0).unwrap();
        t.e.point_word(&t.l, w.x0, w.center().y);
        assert_eq!(t.e.selected_text().trim(), "wide");
    }
}
