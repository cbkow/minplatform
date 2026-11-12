//! minNotes — the editor of one note, without a window (Phase 3 step 2). It owns the document, its history and the
//! selection; a window feeds it clicks, keys and text and redraws from what it reports changed.
//!
//! A POSITION is a leaf block and an offset in it: a byte offset on a character boundary for a text block; for a block
//! without text (a picture, a divider) 0 = before it, 1 = after it. The selection is an anchor and a focus; it may run
//! across blocks, columns and table cells. Every edit goes through `mn_doc::change` — one history, and a typed burst
//! is one undo step (row 275).
use kurbo::Rect;
use mn_doc::{Block, BlockId, Change, ChangeError, Doc, History, Kind, Mark, Rank, Spans};

mod markdown;
mod blocks;
mod table;
pub mod typed;
pub mod cells;
mod columns;
pub mod links;
pub mod chips;
pub mod find;
pub use blocks::{Clip, Drop};
use mn_layout::{NoteLayout, Part};
use parley::{Affinity, Cursor, Selection as PSel};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Pos { pub block: BlockId, pub at: usize }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Selection { pub anchor: Pos, pub focus: Pos }
impl Selection { pub fn caret(p: Pos) -> Self { Selection { anchor: p, focus: p } } pub fn is_caret(&self) -> bool { self.anchor == self.focus } }

/// The block types a control can ask for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BlockType { Paragraph, Heading(u8), Bullet, Numbered, Task, Quote, Code }
impl BlockType {
    pub fn is(&self, k: &Kind) -> bool { matches!((self, k), (BlockType::Paragraph, Kind::Paragraph) | (BlockType::Bullet, Kind::Bullet { .. }) | (BlockType::Numbered, Kind::Numbered { .. }) | (BlockType::Task, Kind::Task { .. }) | (BlockType::Quote, Kind::Quote) | (BlockType::Code, Kind::Code { .. })) || matches!((self, k), (BlockType::Heading(n), Kind::Heading { level }) if n == level) }
}
/// What is "on" where the caret or the selection is.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct State { pub kind: Option<Kind>, pub marks: Vec<mn_doc::MarkKind>, pub can_undo: bool, pub can_redo: bool, /// the text's colour and the colour behind it where typing would go (a name or a hex; None: the default)
    pub color: Option<String>, pub highlight: Option<String> }

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
    /// whole rows (false) or whole columns (true) of a table are selected - while the selection is still this one
    whole: Option<(Selection, bool)>,
    /// the x a run of Up / Down moves keeps returning to
    goal_x: Option<f64>,
    /// the caret stands at a soft wrap and belongs to the END of the upper line (End put it there)
    upstream: bool,
    /// the selection before and after each undo step, alongside the history's steps
    marks: Vec<(Option<Selection>, Option<Selection>)>,
    redo_marks: Vec<(Option<Selection>, Option<Selection>)>,
    /// Marks switched on (true) or off (false) for the next typed run, with no selection (Cmd+B and then typing);
    /// forgotten when the caret moves.
    armed: Vec<(Mark, bool)>,
    /// An input method is composing: its text stands in the block (so it is laid out and drawn in place) but is NOT in
    /// the history — the block as it was is kept here and put back when the composition ends or changes.
    preedit: Option<Preedit>,
    /// Whole blocks are selected (see `blocks.rs`): the two ends of a run of siblings, as chosen (anchor, focus). The
    /// text selection spans the run while this holds; any ordinary caret move or edit ends it.
    blocks: Option<(BlockId, BlockId)>,
    /// how many NEW steps this editor has recorded (undo and redo do not count): what tells that nothing was edited since
    commits: u64,
}
struct Preedit { block: BlockId, at: usize, len: usize, text: String, spans: Spans }

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
    pub fn new(doc: Doc) -> Editor { Editor { doc, history: History::new(), sel: None, whole: None, goal_x: None, upstream: false, marks: vec![], redo_marks: vec![], armed: vec![], preedit: None, blocks: None, commits: 0 } }
    pub fn selection(&self) -> Option<Selection> { self.sel }
    /// (new steps recorded so far, steps that can be undone): together they say whether the last step is still the one a caller remembers.
    pub fn stamp(&self) -> (u64, usize) { (self.commits, self.marks.len()) }
    pub fn set_selection(&mut self, s: Option<Selection>) { self.sel = s.map(|s| Selection { anchor: self.valid(s.anchor), focus: self.valid(s.focus) }); self.goal_x = None; self.upstream = false; self.armed.clear(); self.blocks = None; }
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

    // ── composing (an input method: accents by long press, dead keys, Chinese / Japanese / Korean input)
    /// Takes the composing text out again; the caret is back where the composition began. Every other edit and every
    /// caret move ends a composition this way first.
    fn end_preedit(&mut self) -> Option<BlockId> {
        let p = self.preedit.take()?;
        let _ = self.doc.set_text(p.block, p.text); let _ = self.doc.set_spans(p.block, p.spans);
        self.sel = Some(Selection::caret(Pos { block: p.block, at: p.at })); Some(p.block)
    }
    /// The composition's text as it now stands (empty: the composition was abandoned), with the input method's caret
    /// inside it (byte offsets). It is shown in place of the selection, underlined by the painter; nothing is recorded
    /// until [`Editor::commit_preedit`].
    pub fn set_preedit(&mut self, text: &str, cursor: Option<(usize, usize)>, now_ms: u64) -> Result<Dirty, ChangeError> {
        let mut dirty = Dirty::Nothing;
        let restored = self.end_preedit();
        if restored.is_none() && self.sel.is_some_and(|s| !s.is_caret()) { let mut b = self.batch(); let p = self.delete_selection(&mut b)?; dirty = self.commit(b, p, now_ms)?; }
        let Some(p) = self.sel.map(|s| s.focus) else { return Ok(dirty) };
        let Some(blk) = self.doc.get(p.block).filter(|b| b.kind.is_text()).cloned() else { return Ok(dirty) };
        if !text.is_empty() {
            let at = blk.spans.snap_out_of_chip(p.at.min(blk.text.len()));
            let mut shown = String::with_capacity(blk.text.len() + text.len()); shown.push_str(&blk.text[..at]); shown.push_str(text); shown.push_str(&blk.text[at..]);
            let mut spans = blk.spans.clone(); spans.edit(at, 0, text.len());
            let _ = self.doc.set_text(p.block, shown); let _ = self.doc.set_spans(p.block, spans);
            let caret = cursor.map(|c| c.1.min(text.len())).unwrap_or(text.len());
            self.sel = Some(Selection::caret(Pos { block: p.block, at: at + caret }));
            self.preedit = Some(Preedit { block: p.block, at, len: text.len(), text: blk.text, spans: blk.spans });
        }
        Ok(match dirty { Dirty::Structure => Dirty::Structure, Dirty::Blocks(mut v) => { if !v.contains(&p.block) { v.push(p.block); } Dirty::Blocks(v) } Dirty::Nothing => if restored.is_some() || !text.is_empty() { Dirty::Blocks(vec![p.block]) } else { Dirty::Nothing } })
    }
    /// The input method's result: it is typed in, as one edit.
    pub fn commit_preedit(&mut self, text: &str, now_ms: u64) -> Result<Dirty, ChangeError> {
        let restored = self.end_preedit();
        let d = self.insert(text, now_ms)?;
        Ok(match (d, restored) { (Dirty::Nothing, Some(b)) => Dirty::Blocks(vec![b]), (d, _) => d })
    }
    /// The composing text's place: (block, start, end) in bytes.
    pub fn preedit(&self) -> Option<(BlockId, usize, usize)> { self.preedit.as_ref().map(|p| (p.block, p.at, p.at + p.len)) }

    // ── pointing
    /// A press at a point of the note (the layout's coordinates); `extend` keeps the anchor (Shift, or a drag).
    pub fn point(&mut self, layout: &NoteLayout, x: f64, y: f64, extend: bool) {
        if self.preedit.is_some() { return }   // the input method owns the caret until it ends
        let Some((block, at)) = layout.hit(&self.doc, x, y) else { return };
        let p = self.valid(Pos { block, at });
        self.blocks = None;
        self.sel = Some(match (extend, self.sel) { (true, Some(s)) => Selection { anchor: s.anchor, focus: p }, _ => Selection::caret(p) });
        self.goal_x = None; self.upstream = false; self.armed.clear();
    }
    /// The word under a point (a double click).
    pub fn point_word(&mut self, layout: &NoteLayout, x: f64, y: f64) {
        self.blocks = None;
        let Some((block, at)) = layout.hit(&self.doc, x, y) else { return };
        let (Some(b), Some(Part::Text { layout: l, .. })) = (self.doc.get(block), layout.get(block).map(|b| &b.part)) else { self.sel = Some(Selection { anchor: Pos { block, at: 0 }, focus: Pos { block, at: 1 } }); return };
        let c = Cursor::from_byte_index(l, at.min(b.text.len()), Affinity::Downstream);
        let (s, e) = (c.next_visual_word(l).previous_visual_word(l).index(), c.previous_visual_word(l).next_visual_word(l).index());
        let (s, e) = if s <= at && at <= e && s < e { (s, e) } else { (c.previous_visual_word(l).index().min(at), c.next_visual_word(l).index().max(at)) };
        self.sel = Some(Selection { anchor: self.valid(Pos { block, at: s }), focus: self.valid(Pos { block, at: e }) }); self.goal_x = None;
    }

    // ── moving
    pub fn move_caret(&mut self, layout: &NoteLayout, m: Move, extend: bool) {
        self.blocks = None;
        if self.preedit.is_some() { return }
        let Some(s) = self.sel else { return };
        self.armed.clear();
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
        self.blocks = None;
        let l = leaves(&self.doc);
        if let (Some(a), Some(z)) = (l.first(), l.last()) { self.sel = Some(Selection { anchor: Pos { block: *a, at: 0 }, focus: Pos { block: *z, at: end_of(self.doc.get(*z).unwrap()) } }); }
    }

    // ── geometry for the painter
    pub fn caret_rect(&self, layout: &NoteLayout, width: f32) -> Option<Rect> { let s = self.sel?; layout.caret_side(&self.doc, s.focus.block, s.focus.at, width, self.upstream) }
    /// The rectangles of the selection, block by block (none for a caret).
    pub fn selection_rects(&self, layout: &NoteLayout) -> Vec<Rect> {
        if let Some(cr) = self.cell_rect() { return self.cells_in(&cr).into_iter().filter_map(|c| layout.rect(&self.doc, c)).collect() }
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
        // cells of a table are selected: nothing is joined across cells (the cells' own commands empty or remove them)
        if self.cell_rect().is_some() { return Ok(self.sel.map(|x| x.focus)) }
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
        let before = self.sel; self.blocks = None; self.commits += 1;
        let joined = self.history.commit(&mut self.doc, b.changes, now_ms)?;
        self.sel = after.map(|p| Selection::caret(self.valid(p))); self.goal_x = None; self.upstream = false; self.redo_marks.clear();
        if joined { if let Some(m) = self.marks.last_mut() { m.1 = self.sel; } } else { self.marks.push((before, self.sel)); }
        Ok(if structural { Dirty::Structure } else { Dirty::Blocks(ids) })
    }
    /// Typed or pasted text replaces the selection. In a block without text it starts a paragraph beside it. A line
    /// break inside the text splits the block (stays a line break in a code block). Armed marks (Cmd+B with no
    /// selection) apply to what is typed; a typed Markdown marker that completes a pattern converts it (as a step of its
    /// own, so one undo gives the typed characters back).
    pub fn insert(&mut self, text: &str, now_ms: u64) -> Result<Dirty, ChangeError> {
        self.end_preedit();
        let mut b = self.batch();
        let Some(mut p) = self.delete_selection(&mut b)? else { return Ok(Dirty::Nothing) };
        let armed = std::mem::take(&mut self.armed);
        for (n, line) in text.replace("\r\n", "\n").replace('\r', "\n").split('\n').enumerate() {
            let blk = b.scratch.get(p.block).ok_or(mn_doc::TreeError::UnknownBlock(p.block))?.clone();
            if !blk.kind.is_text() {
                let index = b.scratch.index_of(p.block).unwrap() + p.at.min(1); let para = paragraph(blk.parent, line); let id = para.id;
                b.push(Change::Insert { index, blocks: vec![para] })?; p = Pos { block: id, at: line.len() }; continue
            }
            if n > 0 {
                if matches!(blk.kind, Kind::Code { .. }) { b.edit(p.block, p.at, 0, "\n")?; p.at += 1; }
                else if let Some(done) = markdown::on_enter(&mut b, p)? { p = done; }
                else { let (c, new) = Change::split(&b.scratch, p.block, p.at)?; for x in c { b.push(x)?; } p = Pos { block: new, at: 0 }; }
            }
            let at = b.scratch.get(p.block).map(|x| x.spans.snap_out_of_chip(p.at)).unwrap_or(p.at);
            if !line.is_empty() {
                let mut c = Change::edit(&b.scratch, p.block, at, 0, line)?;
                // the armed marks ride in the same change, so a typed run stays one undo step
                if let Change::Text { new_spans, .. } = &mut c { if !matches!(b.scratch.get(p.block).map(|x| &x.kind), Some(Kind::Code { .. })) { for (m, on) in &armed { if *on { new_spans.put(at, at + line.len(), m.clone()) } else { new_spans.remove(at, at + line.len(), m.kind()) } } } }
                b.push(c)?;
            }
            p = Pos { block: p.block, at: at + line.len() };
        }
        let typed_one = text.chars().count() == 1;
        let d = self.commit(b, Some(p), now_ms)?;
        self.armed = armed;   // still armed while the run goes on
        // Markdown as you type: only a single typed character can complete a pattern (never a paste)
        if typed_one { if let Some(d2) = self.markdown(text, now_ms)? { return Ok(match (d, d2) { (Dirty::Structure, _) | (_, Dirty::Structure) => Dirty::Structure, (Dirty::Blocks(mut a), Dirty::Blocks(x)) => { a.extend(x); a.dedup(); Dirty::Blocks(a) } (a, Dirty::Nothing) => a, (Dirty::Nothing, x) => x }) } }
        Ok(d)
    }
    fn markdown(&mut self, typed: &str, now_ms: u64) -> Result<Option<Dirty>, ChangeError> {
        let Some(s) = self.sel.filter(|s| s.is_caret()) else { return Ok(None) };
        let mut b = self.batch();
        let Some(after) = markdown::on_char(&mut b, s.focus, typed)? else { return Ok(None) };
        let armed = std::mem::take(&mut self.armed);
        // far enough in time from the typing that the two never join: the conversion is its own undo step
        let d = self.commit(b, Some(after), now_ms + mn_doc::change::JOIN_MS + 1)?; self.armed = armed;
        Ok(Some(d))
    }
    /// Cmd+B and its kin. With a selection: the mark goes ON over all of it unless it already covers all of it, then
    /// OFF (one decision for the whole selection, across blocks). With a caret: the mark is armed for what is typed next.
    pub fn toggle_mark(&mut self, mark: Mark, now_ms: u64) -> Result<Dirty, ChangeError> {
        let Some((a, z)) = self.ordered() else { return Ok(Dirty::Nothing) };
        if a == z {
            let here = self.doc.get(a.block).map(|blk| a.at > 0 && blk.spans.covers(prev_char(&blk.text, a.at), a.at, &mark) && blk.spans.covers(a.at, next_char(&blk.text, a.at).max(a.at + 1), &mark)).unwrap_or(false);
            match self.armed.iter().position(|(m, _)| m.kind() == mark.kind()) { Some(i) => { self.armed.remove(i); } None => self.armed.push((mark, !here)) }
            return Ok(Dirty::Nothing)
        }
        let ranges = self.text_ranges(a, z);
        let all = !ranges.is_empty() && ranges.iter().all(|(id, s, e)| self.doc.get(*id).is_some_and(|blk| blk.spans.covers(*s, *e, &mark)));
        self.format(ranges, |sp, s, e| if all { sp.remove(s, e, mark.kind()) } else { sp.put(s, e, mark.clone()) }, now_ms)
    }
    /// A colour for the text (`MarkKind::Color`) or behind it (`MarkKind::Highlight`): `Some` sets it, `None` takes
    /// it away (the role's default). With a selection it goes over all of it; with a caret it is armed for what is
    /// typed next. The other role is never touched.
    pub fn set_colour(&mut self, kind: mn_doc::MarkKind, value: Option<&str>, now_ms: u64) -> Result<Dirty, ChangeError> {
        let make = |v: &str| if kind == mn_doc::MarkKind::Color { Mark::Color(v.to_string()) } else { Mark::Highlight(v.to_string()) };
        let Some((a, z)) = self.ordered() else { return Ok(Dirty::Nothing) };
        if a == z { self.armed.retain(|(m, _)| m.kind() != kind); self.armed.push(match value { Some(v) => (make(v), true), None => (make(""), false) }); return Ok(Dirty::Nothing) }
        let ranges = self.text_ranges(a, z);
        self.format(ranges, |sp, s, e| match value { Some(v) => sp.put(s, e, make(v)), None => sp.remove(s, e, kind) }, now_ms)
    }
    /// The colour in effect for a role where typing would go (a caret: armed, else the run it is in; a selection: at its start).
    fn colour_at(&self, kind: mn_doc::MarkKind) -> Option<String> {
        let (a, z) = self.ordered()?;
        if a == z { if let Some((m, on)) = self.armed.iter().find(|(m, _)| m.kind() == kind) { return if *on { m.name().1.map(str::to_string) } else { None } } }
        let blk = self.doc.get(a.block)?;
        blk.spans.as_slice().iter().find(|sp| sp.mark.kind() == kind && if a == z { sp.start < a.at && sp.end >= a.at } else { sp.start <= a.at && sp.end > a.at }).and_then(|sp| sp.mark.name().1.map(str::to_string))
    }
    /// Cmd+\: every mark but choice chips leaves the selection.
    pub fn clear_formatting(&mut self, now_ms: u64) -> Result<Dirty, ChangeError> {
        let Some((a, z)) = self.ordered() else { return Ok(Dirty::Nothing) }; if a == z { self.armed.clear(); return Ok(Dirty::Nothing) }
        let ranges = self.text_ranges(a, z); self.format(ranges, |sp, s, e| sp.clear(s, e), now_ms)
    }
    /// The selected stretch of every text block in the selection that takes formatting (not code blocks).
    fn text_ranges(&self, a: Pos, z: Pos) -> Vec<(BlockId, usize, usize)> {
        let l = leaves(&self.doc); let (Some(i), Some(j)) = (l.iter().position(|x| *x == a.block), l.iter().position(|x| *x == z.block)) else { return vec![] };
        l[i..=j].iter().filter_map(|id| { let blk = self.doc.get(*id)?; if !blk.kind.is_text() || matches!(blk.kind, Kind::Code { .. }) { return None } let (s, e) = (if *id == a.block { a.at } else { 0 }, if *id == z.block { z.at } else { blk.text.len() }); (e > s).then_some((*id, s, e)) }).collect()
    }
    fn format(&mut self, ranges: Vec<(BlockId, usize, usize)>, f: impl Fn(&mut Spans, usize, usize), now_ms: u64) -> Result<Dirty, ChangeError> {
        let mut b = self.batch();
        for (id, s, e) in ranges { let old = b.scratch.get(id).unwrap().spans.clone(); let mut new = old.clone(); f(&mut new, s, e); if new != old { b.push(Change::Format { id, old, new })?; } }
        let keep = self.sel; let d = self.commit(b, None, now_ms)?; self.sel = keep;
        if let Some(m) = self.marks.last_mut() { if d != Dirty::Nothing { m.1 = keep; } }
        Ok(d)
    }
    /// What a block-type control asks for.
    /// The text blocks in the selection (or the caret's block) become `want`; when every one of them already is, they
    /// become plain paragraphs (the control toggles). A list keeps each item's depth; a block that becomes a code block
    /// loses its formatting. Blocks without text are left alone.
    pub fn set_block_kind(&mut self, want: BlockType, now_ms: u64) -> Result<Dirty, ChangeError> {
        let Some((a, z)) = self.ordered() else { return Ok(Dirty::Nothing) };
        let l = leaves(&self.doc); let (Some(i), Some(j)) = (l.iter().position(|x| *x == a.block), l.iter().position(|x| *x == z.block)) else { return Ok(Dirty::Nothing) };
        let ids: Vec<BlockId> = l[i..=j].iter().copied().filter(|id| self.doc.get(*id).is_some_and(|b| b.kind.is_text())).collect();
        let all = !ids.is_empty() && ids.iter().all(|id| want.is(&self.doc.get(*id).unwrap().kind));
        let mut b = self.batch();
        for id in ids {
            let blk = self.doc.get(id).unwrap(); let old = blk.kind.clone();
            let depth = match &old { Kind::Bullet { depth } | Kind::Numbered { depth } | Kind::Task { depth, .. } => *depth, _ => 0 };
            let new = if all { Kind::Paragraph } else { match want { BlockType::Paragraph => Kind::Paragraph, BlockType::Heading(n) => Kind::Heading { level: n.clamp(1, 6) }, BlockType::Bullet => Kind::Bullet { depth }, BlockType::Numbered => Kind::Numbered { depth }, BlockType::Task => Kind::Task { depth, state: 0 }, BlockType::Quote => Kind::Quote, BlockType::Code => Kind::Code { lang: String::new() } } };
            if new == old { continue }
            if matches!(new, Kind::Code { .. }) && !blk.spans.is_empty() { b.push(Change::Format { id, old: blk.spans.clone(), new: Spans::new() })?; }
            b.push(Change::Kind { id, old, new })?;
        }
        let keep = self.sel; let d = self.commit(b, None, now_ms)?; self.sel = keep; if let Some(m) = self.marks.last_mut() { if d != Dirty::Nothing { m.1 = keep; } }
        Ok(d)
    }
    /// A divider goes in below the caret's block, with the caret in a paragraph after it (a new one if none follows).
    pub fn insert_divider(&mut self, now_ms: u64) -> Result<Dirty, ChangeError> {
        let Some(p) = self.sel.map(|s| s.focus) else { return Ok(Dirty::Nothing) };
        let Some(blk) = self.doc.get(p.block).cloned() else { return Ok(Dirty::Nothing) };
        let index = self.doc.index_of(p.block).unwrap() + 1; let mut b = self.batch();
        let mut div = paragraph(blk.parent, ""); div.kind = Kind::Divider; b.push(Change::Insert { index, blocks: vec![div] })?;
        let next = self.doc.children(blk.parent).get(index).copied().filter(|n| self.doc.get(*n).is_some_and(|x| x.kind.is_text()));
        let caret = match next { Some(n) => n, None => { let para = paragraph(blk.parent, ""); let id = para.id; b.push(Change::Insert { index: index + 1, blocks: vec![para] })?; id } };
        self.commit(b, Some(Pos { block: caret, at: 0 }), now_ms)
    }
    /// What a toolbar shows as "on": the kind of the caret's block, the marks that cover the selection (at a caret: the
    /// marks typing would get — those around it, as the armed ones change them), and whether undo / redo have a step.
    pub fn state(&self) -> State {
        let mut st = State { can_undo: self.history.can_undo(), can_redo: self.history.can_redo(), ..State::default() };
        let Some((a, z)) = self.ordered() else { return st };
        st.color = self.colour_at(mn_doc::MarkKind::Color); st.highlight = self.colour_at(mn_doc::MarkKind::Highlight);
        st.kind = self.doc.get(self.sel.unwrap().focus.block).map(|b| b.kind.clone());
        let kinds = [mn_doc::MarkKind::Bold, mn_doc::MarkKind::Italic, mn_doc::MarkKind::Underline, mn_doc::MarkKind::Strike, mn_doc::MarkKind::Code, mn_doc::MarkKind::Highlight];
        if a == z {
            if let Some(blk) = self.doc.get(a.block) { for k in kinds { let around = a.at > 0 && blk.spans.as_slice().iter().any(|sp| sp.mark.kind() == k && sp.start < a.at && sp.end > a.at); let on = match self.armed.iter().find(|(m, _)| m.kind() == k) { Some((_, on)) => *on, None => around }; if on { st.marks.push(k); } } }
        } else {
            let ranges = self.text_ranges(a, z);
            for k in kinds { if !ranges.is_empty() && ranges.iter().all(|(id, s, e)| { let blk = self.doc.get(*id).unwrap(); let mut at = *s; for sp in blk.spans.as_slice().iter().filter(|sp| sp.mark.kind() == k) { if sp.start > at { break } at = at.max(sp.end); } at >= *e }) { st.marks.push(k); } }
        }
        st
    }
    /// Tab / Shift+Tab: the list items in the selection go one level in or out (0…8). False when there was no list item
    /// (the caller may then give the key another meaning).
    pub fn indent(&mut self, out: bool, now_ms: u64) -> Result<Option<Dirty>, ChangeError> {
        let Some((a, z)) = self.ordered() else { return Ok(None) };
        let l = leaves(&self.doc); let (Some(i), Some(j)) = (l.iter().position(|x| *x == a.block), l.iter().position(|x| *x == z.block)) else { return Ok(None) };
        let mut b = self.batch(); let mut any = false;
        for id in &l[i..=j] {
            let old = self.doc.get(*id).unwrap().kind.clone();
            let step = |d: u8| if out { d.saturating_sub(1) } else { (d + 1).min(8) };
            let new = match &old { Kind::Bullet { depth } => Kind::Bullet { depth: step(*depth) }, Kind::Numbered { depth } => Kind::Numbered { depth: step(*depth) }, Kind::Task { depth, state } => Kind::Task { depth: step(*depth), state: *state }, _ => continue };
            any = true; if new != old { b.push(Change::Kind { id: *id, old, new })?; }
        }
        if !any { return Ok(None) }
        let keep = self.sel; let d = self.commit(b, None, now_ms)?; self.sel = keep; if let Some(m) = self.marks.last_mut() { if d != Dirty::Nothing { m.1 = keep; } }
        Ok(Some(d))
    }
    /// Enter.
    pub fn enter(&mut self, now_ms: u64) -> Result<Dirty, ChangeError> { self.insert("\n", now_ms) }
    /// Backspace (`forward` false) or Delete (true).
    pub fn delete(&mut self, forward: bool, now_ms: u64) -> Result<Dirty, ChangeError> {
        if self.preedit.is_some() { return Ok(Dirty::Nothing) }
        let Some(s) = self.sel else { return Ok(Dirty::Nothing) };
        if !forward && s.is_caret() { if let Some(d) = self.column_backspace(now_ms)? { return Ok(d) } }
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
        if self.end_preedit().is_some() { return Ok(Dirty::Structure) }
        if !self.history.undo(&mut self.doc)? { return Ok(Dirty::Nothing) }
        self.blocks = None;
        let m = self.marks.pop().unwrap_or((None, None)); self.sel = m.0.and_then(|s| self.live(s)); self.redo_marks.push(m); self.goal_x = None; Ok(Dirty::Structure)
    }
    pub fn redo(&mut self) -> Result<Dirty, ChangeError> {
        if self.end_preedit().is_some() { return Ok(Dirty::Structure) }
        if !self.history.redo(&mut self.doc)? { return Ok(Dirty::Nothing) }
        self.blocks = None;
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

    // ── row 289: formatting by command and by Markdown
    fn blank() -> T { let mut d = Doc::new(); let ids = vec![add(&mut d, None, Kind::Paragraph, "")]; let mut f = Fonts::new(); let l = NoteLayout::build(&d, &mut f, &Style::default(), 400.0, 1.0); let mut t = T { e: Editor::new(d), f, l, ids, now: 0 }; t.at(0, 0); t }
    impl T {
        /// Typed one character at a time, 20 ms apart (a run).
        fn keys(&mut self, s: &str) { self.now += 1000; for ch in s.chars() { self.now += 20; let d = if ch == '\n' { self.e.enter(self.now) } else { self.e.insert(&ch.to_string(), self.now) }.unwrap(); self.relay(d); } }
        fn cur(&self) -> &Block { self.e.doc.get(self.e.selection().unwrap().focus.block).unwrap() }
        fn marks(&self) -> Vec<(usize, usize, Mark)> { self.cur().spans.as_slice().iter().map(|s| (s.start, s.end, s.mark.clone())).collect() }
    }
    #[test]
    fn inline_markdown_converts_on_its_closing_marker_and_one_undo_gives_the_characters_back() {
        let mut t = blank();
        t.keys("a **bold** b");
        assert_eq!((t.cur().text.as_str(), t.marks()), ("a bold b", vec![(2, 6, Mark::Bold)]));
        t.keys(" *it* `co de` ~~gone~~ _em_ __strong__");
        assert_eq!(t.cur().text, "a bold b it co de gone em strong");
        let kinds: Vec<Mark> = t.marks().into_iter().map(|m| m.2).collect();
        assert_eq!(kinds, [Mark::Bold, Mark::Bold, Mark::Italic, Mark::Italic, Mark::Code, Mark::Strike]);
        t.keys(" [site](https://example.com)");
        assert!(t.cur().text.ends_with(" site") && t.marks().iter().any(|m| m.2 == Mark::Link("https://example.com".into()) && m.1 - m.0 == 4));
        // one undo: the link is literal Markdown again; the caret after it
        t.e.undo().unwrap(); assert!(t.cur().text.ends_with("[site](https://example.com)"));
        // what must NOT convert: a lone or spaced asterisk, snake_case, a marker inside inline code, anything in a code block
        let mut t = blank(); t.keys("2 * 3 * 4 and a*b snake_case_name x_ y_");
        assert_eq!((t.cur().text.as_str(), t.marks()), ("2 * 3 * 4 and a*b snake_case_name x_ y_", vec![]));
        let mut t = blank(); t.keys("`a*b` c*"); assert_eq!(t.cur().text, "a*b c*");
        let mut t = blank(); t.keys("```\n**no**"); assert_eq!((&t.cur().kind, t.cur().text.as_str()), (&Kind::Code { lang: String::new() }, "**no**"));
    }
    #[test]
    fn block_prefixes_convert_on_their_space_and_lists_behave() {
        for (typed, kind) in [("# ", Kind::Heading { level: 1 }), ("### ", Kind::Heading { level: 3 }), ("> ", Kind::Quote), ("- ", Kind::Bullet { depth: 0 }), ("* ", Kind::Bullet { depth: 0 }), ("12. ", Kind::Numbered { depth: 0 }), ("[] ", Kind::Task { depth: 0, state: 0 }), ("- [x] ", Kind::Task { depth: 0, state: 2 }), ("- [/] ", Kind::Task { depth: 0, state: 1 })] {
            let mut t = blank(); t.keys(typed); t.keys("x");
            assert_eq!((&t.cur().kind, t.cur().text.as_str()), (&kind, "x"), "{typed:?}");
        }
        // not in the middle of a line, and one undo gives the prefix back
        let mut t = blank(); t.keys("a # b"); assert_eq!((&t.cur().kind, t.cur().text.as_str()), (&Kind::Paragraph, "a # b"));
        let mut t = blank(); t.keys("# "); t.e.undo().unwrap(); assert_eq!((&t.cur().kind, t.cur().text.as_str()), (&Kind::Paragraph, "# "));
        // a list: Enter continues it, Tab nests, Enter on an empty item steps out and then leaves the list
        let mut t = blank(); t.keys("- one\ntwo"); assert_eq!((&t.cur().kind, t.e.doc.children(None).len()), (&Kind::Bullet { depth: 0 }, 2));
        t.now += 1000; let d = t.e.indent(false, t.now).unwrap().unwrap(); t.relay(d); assert_eq!(t.cur().kind, Kind::Bullet { depth: 1 });
        t.keys("\n"); assert_eq!((&t.cur().kind, t.cur().text.as_str()), (&Kind::Bullet { depth: 1 }, ""));
        t.keys("\n"); assert_eq!(t.cur().kind, Kind::Bullet { depth: 0 });
        t.keys("\n"); assert_eq!((&t.cur().kind, t.e.doc.children(None).len()), (&Kind::Paragraph, 3));
        assert_eq!(t.e.indent(false, t.now).unwrap(), None);                                            // Tab in a paragraph is not an indent
        // Enter after ```lang makes a code block; after --- a divider with a paragraph to go on in
        let mut t = blank(); t.keys("```rust\nlet x;"); assert_eq!((&t.cur().kind, t.cur().text.as_str(), t.e.doc.children(None).len()), (&Kind::Code { lang: "rust".into() }, "let x;", 1));
        let mut t = blank(); t.keys("---\nnext");
        let kinds: Vec<Kind> = t.e.doc.children(None).iter().map(|c| t.e.doc.get(*c).unwrap().kind.clone()).collect();
        assert_eq!((kinds, t.cur().text.as_str()), (vec![Kind::Divider, Kind::Paragraph], "next"));
    }
    #[test]
    fn a_command_toggles_a_mark_over_the_selection_or_arms_it_for_typing() {
        let mut t = setup();
        // across two blocks: on over all of it; again: off
        t.e.set_selection(Some(Selection { anchor: Pos { block: t.ids[0], at: 2 }, focus: Pos { block: t.ids[1], at: 9 } }));
        let d = t.e.toggle_mark(Mark::Bold, 1000).unwrap(); assert!(matches!(d, Dirty::Blocks(ref v) if v.len() == 2)); t.relay(d);
        let covered = |t: &T| (t.e.doc.get(t.ids[0]).unwrap().spans.covers(2, 5, &Mark::Bold), t.e.doc.get(t.ids[1]).unwrap().spans.covers(0, 9, &Mark::Bold));
        assert_eq!(covered(&t), (true, true));
        assert_eq!(t.e.selection().unwrap().focus, Pos { block: t.ids[1], at: 9 });                    // the selection stays
        // partly covered → on over all; fully covered → off
        t.e.set_selection(Some(Selection { anchor: Pos { block: t.ids[1], at: 5 }, focus: Pos { block: t.ids[1], at: 20 } }));
        let d = t.e.toggle_mark(Mark::Bold, 2000).unwrap(); t.relay(d); assert!(t.e.doc.get(t.ids[1]).unwrap().spans.covers(0, 20, &Mark::Bold));
        let d = t.e.toggle_mark(Mark::Bold, 3000).unwrap(); t.relay(d); assert!(!t.e.doc.get(t.ids[1]).unwrap().spans.covers(5, 6, &Mark::Bold) && t.e.doc.get(t.ids[1]).unwrap().spans.covers(0, 5, &Mark::Bold));
        t.e.set_selection(Some(Selection { anchor: Pos { block: t.ids[0], at: 0 }, focus: Pos { block: t.ids[1], at: 30 } }));
        let d = t.e.clear_formatting(4000).unwrap(); t.relay(d); assert!(t.e.doc.get(t.ids[0]).unwrap().spans.is_empty() && t.e.doc.get(t.ids[1]).unwrap().spans.is_empty());
        t.e.undo().unwrap(); assert!(t.e.doc.get(t.ids[1]).unwrap().spans.covers(0, 5, &Mark::Bold));
        // armed: Cmd+B, Cmd+I, type — the run is bold italic and ONE undo step; moving the caret disarms
        let mut t = blank(); t.keys("plain ");
        t.e.toggle_mark(Mark::Bold, t.now).unwrap(); t.e.toggle_mark(Mark::Italic, t.now).unwrap();
        let steps = t.e.history.steps(); t.keys("loud");
        assert_eq!((t.cur().text.as_str(), t.marks(), t.e.history.steps()), ("plain loud", vec![(6, 10, Mark::Bold), (6, 10, Mark::Italic)], steps + 1));
        t.e.toggle_mark(Mark::Bold, t.now).unwrap(); t.keys("er");                                      // Cmd+B again inside the run: off for what follows
        assert_eq!(t.marks(), vec![(6, 10, Mark::Bold), (6, 12, Mark::Italic)]);
        t.mv(Move::Left, false); t.mv(Move::Right, false); t.keys("!"); assert_eq!(t.marks(), vec![(6, 10, Mark::Bold), (6, 12, Mark::Italic)]);
    }
    /// An input method composes in place; only its result is an edit.
    #[test]
    fn a_composition_shows_in_place_and_only_its_result_is_recorded() {
        let mut t = setup(); t.at(4, 4); let steps = t.e.history.steps();
        let d = t.e.set_preedit("ni", Some((2, 2)), 100).unwrap(); t.relay(d);
        assert_eq!((t.text(4), t.pos(), t.e.preedit()), ("lastni".into(), (4, 6), Some((t.ids[4], 4, 6))));
        let d = t.e.set_preedit("\u{4f60}\u{597d}", None, 200).unwrap(); t.relay(d);                     // the candidate replaces the letters
        assert_eq!((t.text(4), t.e.history.steps()), ("last\u{4f60}\u{597d}".into(), steps));
        t.mv(Move::Left, false); assert_eq!(t.pos(), (4, 10));                                           // the caret is the input method's while it composes
        let d = t.e.commit_preedit("\u{4f60}\u{597d}", 300).unwrap(); t.relay(d);
        assert_eq!((t.text(4), t.e.preedit(), t.e.history.steps()), ("last\u{4f60}\u{597d}".into(), None, steps + 1));
        t.e.undo().unwrap(); assert_eq!((t.text(4), t.pos()), ("last".into(), (4, 4)));
        // abandoned: the block is as it was, nothing recorded
        let d = t.e.set_preedit("e\u{301}", None, 400).unwrap(); t.relay(d); let d = t.e.set_preedit("", None, 500).unwrap(); t.relay(d);
        assert_eq!((t.text(4), t.e.history.steps(), t.pos()), ("last".into(), steps, (4, 4)));
        // over a selection: the selection goes (one step), then the composition stands where it was
        t.e.set_selection(Some(Selection { anchor: Pos { block: t.ids[4], at: 1 }, focus: Pos { block: t.ids[4], at: 3 } }));
        let d = t.e.set_preedit("x", None, 600).unwrap(); t.relay(d);
        assert_eq!((t.text(4), t.e.history.steps()), ("lxt".into(), steps + 1));
        let d = t.e.commit_preedit("X", 700).unwrap(); t.relay(d); assert_eq!(t.text(4), "lXt");
    }
    #[test]
    fn a_block_type_control_sets_toggles_and_reports() {
        let mut t = setup();
        t.e.set_selection(Some(Selection { anchor: Pos { block: t.ids[0], at: 1 }, focus: Pos { block: t.ids[3], at: 1 } }));
        let d = t.e.set_block_kind(BlockType::Bullet, 100).unwrap(); t.relay(d);
        assert_eq!((t.e.doc.get(t.ids[0]).unwrap().kind.clone(), t.e.doc.get(t.ids[1]).unwrap().kind.clone(), matches!(t.e.doc.get(t.ids[2]).unwrap().kind, Kind::Media { .. })), (Kind::Bullet { depth: 0 }, Kind::Bullet { depth: 0 }, true));
        assert!(matches!(t.e.state().kind, Some(Kind::Bullet { .. })));
        let d = t.e.set_block_kind(BlockType::Bullet, 2000).unwrap(); t.relay(d);                       // all of them already: back to paragraphs
        assert_eq!(t.e.doc.get(t.ids[0]).unwrap().kind, Kind::Paragraph);
        t.e.undo().unwrap(); t.e.undo().unwrap(); assert_eq!(t.e.doc.get(t.ids[0]).unwrap().kind, Kind::Heading { level: 1 });
        // marks: on over a covered selection, on at a caret inside a run, and armed marks show before anything is typed
        t.e.set_selection(Some(Selection { anchor: Pos { block: t.ids[1], at: 4 }, focus: Pos { block: t.ids[1], at: 9 } }));
        t.e.toggle_mark(Mark::Bold, 3000).unwrap(); assert_eq!(t.e.state().marks, [mn_doc::MarkKind::Bold]);
        t.at(1, 6); assert_eq!(t.e.state().marks, [mn_doc::MarkKind::Bold]);
        t.at(1, 20); assert!(t.e.state().marks.is_empty()); t.e.toggle_mark(Mark::Italic, 3100).unwrap(); assert_eq!(t.e.state().marks, [mn_doc::MarkKind::Italic]);
        // a code block drops formatting; a divider goes in below with the caret after it
        t.at(1, 6); let d = t.e.set_block_kind(BlockType::Code, 4000).unwrap(); t.relay(d);
        assert!(t.e.doc.get(t.ids[1]).unwrap().spans.is_empty() && matches!(t.e.doc.get(t.ids[1]).unwrap().kind, Kind::Code { .. }));
        t.at(4, 2); let n = t.e.doc.children(None).len(); let d = t.e.insert_divider(5000).unwrap(); t.relay(d);
        let kids = t.e.doc.children(None).to_vec(); let at = kids.iter().position(|k| *k == t.ids[4]).unwrap();
        // what follows is not text (the Columns block), so a paragraph is made for the caret too
        assert_eq!((kids.len(), t.e.doc.get(kids[at + 1]).unwrap().kind.clone(), t.e.doc.get(kids[at + 2]).unwrap().kind.clone(), t.e.selection().unwrap().focus.block), (n + 2, Kind::Divider, Kind::Paragraph, kids[at + 2]));
        assert!(t.e.state().can_undo);
    }
    #[test]
    fn a_colour_is_set_per_role_and_default_takes_only_that_role_away() {
        use mn_doc::{Mark, MarkKind};
        let mut d = Doc::new(); let b = BlockId::new(); d.insert(None, 0, b, Kind::Paragraph, "one two three").unwrap();
        let mut e = Editor::new(d);
        e.set_selection(Some(Selection { anchor: Pos { block: b, at: 4 }, focus: Pos { block: b, at: 7 } }));
        e.set_colour(MarkKind::Color, Some("red"), 1).unwrap(); e.set_colour(MarkKind::Highlight, Some("#112233"), 2000).unwrap();
        let has = |e: &Editor, m: Mark| e.doc.get(b).unwrap().spans.covers(4, 7, &m);
        assert!(has(&e, Mark::Color("red".into())) && has(&e, Mark::Highlight("#112233".into())));
        let st = e.state(); assert_eq!((st.color.as_deref(), st.highlight.as_deref()), (Some("red"), Some("#112233")));
        // Default on one role leaves the other
        e.set_colour(MarkKind::Highlight, None, 4000).unwrap();
        assert!(has(&e, Mark::Color("red".into())) && !e.doc.get(b).unwrap().spans.as_slice().iter().any(|s| s.mark.kind() == MarkKind::Highlight));
        // a caret: armed for what is typed, and reported before anything is typed
        e.set_selection(Some(Selection { anchor: Pos { block: b, at: 13 }, focus: Pos { block: b, at: 13 } }));
        e.set_colour(MarkKind::Color, Some("blue"), 6000).unwrap(); assert_eq!(e.state().color.as_deref(), Some("blue"));
        e.insert("!", 6001).unwrap(); assert!(e.doc.get(b).unwrap().spans.covers(13, 14, &Mark::Color("blue".into())));
    }
    #[test]
    fn whole_blocks_are_selected_moved_with_their_children_cut_and_pasted() {
        let mut d = Doc::new();
        let ids: Vec<BlockId> = [(Kind::Paragraph, "intro"), (Kind::Bullet { depth: 0 }, "A"), (Kind::Bullet { depth: 1 }, "a1"), (Kind::Bullet { depth: 1 }, "a2"), (Kind::Bullet { depth: 0 }, "B"), (Kind::Bullet { depth: 1 }, "b1"), (Kind::Paragraph, "end")]
            .into_iter().map(|(k, t)| add(&mut d, None, k, t)).collect();
        let mut e = Editor::new(d);
        let order = |e: &Editor| e.doc.children(None).iter().map(|id| e.doc.get(*id).unwrap().text.clone()).collect::<Vec<_>>().join(" ");
        // Escape on "A": the item and its children are the selection; the text selection spans them
        e.set_selection(Some(Selection::caret(Pos { block: ids[1], at: 0 })));
        assert!(e.toggle_blocks()); assert_eq!(e.block_selection(), vec![ids[1], ids[2], ids[3]]); assert_eq!(e.selected_text(), "A\na1\na2");
        // down: A with its children steps over B WITH b1, as one undo step
        e.move_blocks(true, 1000).unwrap(); assert_eq!(order(&e), "intro B b1 A a1 a2 end"); assert!(e.in_blocks());
        e.undo().unwrap(); assert_eq!(order(&e), "intro A a1 a2 B b1 end");
        // a child moves among its parent's children, and down past the last becomes the next item's first child
        e.set_selection(Some(Selection::caret(Pos { block: ids[2], at: 1 })));
        e.move_blocks(true, 3000).unwrap(); assert_eq!(order(&e), "intro A a2 a1 B b1 end");
        assert_eq!(e.selection().unwrap().focus, Pos { block: ids[2], at: 1 }, "a caret stays in its block");
        e.move_blocks(true, 5000).unwrap(); assert_eq!(order(&e), "intro A a2 B a1 b1 end");
        // up: "end" steps over B and its children, not in between them
        e.set_selection(Some(Selection::caret(Pos { block: ids[6], at: 0 })));
        e.move_blocks(false, 7000).unwrap(); assert_eq!(order(&e), "intro A a2 end B a1 b1");
        // Shift+Down grows the selection by whole units; cut = copy + delete in one step; paste gives fresh identities
        e.set_selection(Some(Selection::caret(Pos { block: ids[1], at: 0 }))); e.toggle_blocks(); e.step_blocks(true, true);
        assert_eq!(e.block_selection(), vec![ids[1], ids[3], ids[6]]);
        let clip = e.copy_blocks().unwrap(); assert_eq!(clip.len(), 3);
        e.delete_blocks(9000).unwrap(); assert_eq!(order(&e), "intro B a1 b1"); assert_eq!(e.selection().unwrap().focus, Pos { block: ids[4], at: 0 });
        e.set_selection(Some(Selection::caret(Pos { block: ids[0], at: 2 })));
        let (_, refused) = e.paste_blocks(&clip, 11000).unwrap(); assert_eq!(refused, 0);
        assert_eq!(order(&e), "intro A a2 end B a1 b1"); assert_eq!(e.block_selection().len(), 3);
        assert!(e.block_selection().iter().all(|id| !ids.contains(id)), "pasted blocks are new blocks");
        e.undo().unwrap(); assert_eq!(order(&e), "intro B a1 b1");
        // Escape again: back to a caret
        e.set_selection(Some(Selection::caret(Pos { block: ids[0], at: 2 }))); assert!(e.toggle_blocks()); assert!(!e.toggle_blocks()); assert!(e.selection().unwrap().is_caret());
    }
    #[test]
    fn a_dragged_run_lands_where_it_is_dropped_takes_the_depth_asked_and_refuses_itself() {
        let mut d = Doc::new();
        let ids: Vec<BlockId> = [(Kind::Paragraph, "intro"), (Kind::Bullet { depth: 0 }, "A"), (Kind::Bullet { depth: 1 }, "a1"), (Kind::Bullet { depth: 0 }, "B"), (Kind::Paragraph, "end")].into_iter().map(|(k, t)| add(&mut d, None, k, t)).collect();
        let cols = add(&mut d, None, Kind::Columns, ""); let c1 = add(&mut d, Some(cols), Kind::Column { width: 0.5 }, ""); let c2 = add(&mut d, Some(cols), Kind::Column { width: 0.5 }, "");
        let in1 = add(&mut d, Some(c1), Kind::Paragraph, "left"); let _in2 = add(&mut d, Some(c2), Kind::Paragraph, "right");
        let mut e = Editor::new(d);
        let order = |e: &Editor, p: Option<BlockId>| e.doc.children(p).iter().map(|id| e.doc.get(*id).unwrap().text.clone()).collect::<Vec<_>>().join(" ");
        // A (with a1) dropped after B, one level in: A becomes B's child and a1 keeps its distance
        e.set_selection(Some(Selection::caret(Pos { block: ids[1], at: 0 })));
        e.drop_blocks(&Drop { parent: None, index: 4, depth: Some(1), beside: None }, 1000).unwrap();
        assert_eq!(order(&e, None), "intro B A a1 end "); assert_eq!(e.doc.get(ids[1]).unwrap().kind, Kind::Bullet { depth: 1 }); assert_eq!(e.doc.get(ids[2]).unwrap().kind, Kind::Bullet { depth: 2 });
        e.undo().unwrap(); assert_eq!(order(&e, None), "intro A a1 B end "); assert_eq!(e.doc.get(ids[2]).unwrap().kind, Kind::Bullet { depth: 1 });
        // into a column; the column it is later emptied of goes, and the one column left unwraps (row 317, ruling 10)
        e.set_selection(Some(Selection::caret(Pos { block: ids[4], at: 0 })));
        e.drop_blocks(&Drop { parent: Some(c1), index: 0, depth: None, beside: None }, 3000).unwrap(); assert_eq!(order(&e, Some(c1)), "end left");
        e.set_selection(Some(Selection { anchor: Pos { block: ids[4], at: 0 }, focus: Pos { block: in1, at: 0 } }));
        e.drop_blocks(&Drop { parent: None, index: 0, depth: None, beside: None }, 5000).unwrap(); assert_eq!(order(&e, None), "end left intro A a1 B right"); assert!(e.doc.get(c1).is_none(), "the emptied column is gone");
        e.undo().unwrap(); assert_eq!(order(&e, Some(c1)), "end left");
        // the columns block cannot be dropped into its own column; where it already is, nothing happens
        e.set_selection(Some(Selection::caret(Pos { block: e.doc.children(Some(c1))[0], at: 0 }))); e.toggle_blocks();
        let cols = e.doc.get(c1).unwrap().parent.unwrap(); e.set_selection(Some(Selection::caret(Pos { block: e.doc.children(None)[0], at: 0 }))); e.toggle_blocks();
        let at = e.doc.index_of(cols).unwrap(); let _ = at;
        assert_eq!(e.drop_blocks(&Drop { parent: None, index: 0, depth: None, beside: None }, 7000).unwrap(), Dirty::Nothing);
        assert_eq!(e.drop_blocks(&Drop { parent: None, index: 1, depth: None, beside: None }, 7000).unwrap(), Dirty::Nothing);
    }
    #[test]
    fn a_fragment_merges_at_the_caret_splits_the_block_keeps_formatting_and_leaves_a_heading_alone() {
        use mn_doc::{Block, Rank};
        let mut d = Doc::new(); let a = add(&mut d, None, Kind::Paragraph, "Hello world"); let h = add(&mut d, None, Kind::Heading { level: 2 }, "Title"); let c = add(&mut d, None, Kind::Code { lang: String::new() }, "let x;");
        let mut e = Editor::new(d);
        let blk = |kind: Kind, text: &str| Block { id: BlockId::new(), parent: None, rank: Rank::first(), kind, text: text.into(), spans: Spans::new(), conflict: None, children: vec![] };
        let at = |e: &mut Editor, block: BlockId, at: usize| e.set_selection(Some(Selection::caret(Pos { block, at })));
        let top = |e: &Editor| -> Vec<(String, Kind)> { e.doc.children(None).iter().map(|id| { let b = e.doc.get(*id).unwrap(); (b.text.clone(), b.kind.clone()) }).collect() };
        // one text block goes in where the caret is, bold and all
        let mut one = blk(Kind::Paragraph, "big "); one.spans.put(0, 3, Mark::Bold);
        at(&mut e, a, 6); e.paste_fragment(&vec![vec![one]], 1_000).unwrap();
        assert_eq!(e.doc.get(a).unwrap().text, "Hello big world"); assert!(e.doc.get(a).unwrap().spans.covers(6, 9, &Mark::Bold));
        assert_eq!(e.selection().unwrap().focus, Pos { block: a, at: 10 });
        // several: the first joins what is before the caret, the last takes what was after it
        at(&mut e, a, 5); e.paste_fragment(&vec![vec![blk(Kind::Paragraph, "!")], vec![blk(Kind::Bullet { depth: 0 }, "item")], vec![blk(Kind::Paragraph, "And")]], 9_000).unwrap();
        let t = top(&e); assert_eq!(t.iter().map(|x| x.0.as_str()).collect::<Vec<_>>(), ["Hello!", "item", "And big world", "Title", "let x;"]);
        assert_eq!(t[1].1, Kind::Bullet { depth: 0 }); assert_eq!(t[2].1, Kind::Paragraph);
        let f = e.selection().unwrap().focus; assert_eq!((e.doc.index_of(f.block), f.at), (Some(2), 3));
        assert!(e.doc.get(f.block).unwrap().spans.covers(4, 7, &Mark::Bold), "the bold moved with the text it was on");
        e.undo().unwrap(); assert_eq!(top(&e)[0].0, "Hello big world"); assert_eq!(top(&e).len(), 3); e.redo().unwrap();
        // at the start of a heading the blocks go in before it and it stays a heading
        at(&mut e, h, 0); e.paste_fragment(&vec![vec![blk(Kind::Paragraph, "x")], vec![blk(Kind::Paragraph, "y")]], 20_000).unwrap();
        let t = top(&e); assert_eq!(t.iter().map(|x| x.0.as_str()).collect::<Vec<_>>(), ["Hello!", "item", "And big world", "x", "y", "Title", "let x;"]); assert_eq!(t[5].1, Kind::Heading { level: 2 });
        // into code: the text alone, lines kept; into an empty paragraph: it takes the kind
        let mut bold = blk(Kind::Heading { level: 1 }, "A"); bold.spans.put(0, 1, Mark::Bold);
        at(&mut e, c, 6); e.paste_fragment(&vec![vec![bold.clone()], vec![blk(Kind::Paragraph, "B")]], 30_000).unwrap();
        assert_eq!(e.doc.get(c).unwrap().text, "let x;A\nB"); assert!(e.doc.get(c).unwrap().spans.is_empty());
        let empty = { let id = BlockId::new(); let n = e.doc.children(None).len(); e.doc.insert(None, n, id, Kind::Paragraph, "").unwrap(); id };
        at(&mut e, empty, 0); e.paste_fragment(&vec![vec![bold]], 40_000).unwrap();
        assert_eq!(e.doc.get(empty).unwrap().kind, Kind::Heading { level: 1 }); assert_eq!(e.doc.get(empty).unwrap().text, "A");
    }
    #[test]
    fn a_table_and_columns_are_made_in_place_and_tab_walks_the_cells() {
        let mut d = Doc::new(); let a = add(&mut d, None, Kind::Paragraph, "Above"); let e0 = add(&mut d, None, Kind::Paragraph, "");
        let mut e = Editor::new(d);
        let at = |e: &mut Editor, block: BlockId, at: usize| e.set_selection(Some(Selection::caret(Pos { block, at })));
        // a table in the place of an empty paragraph: 3 x 2, its first row a header, the caret in its first cell
        at(&mut e, e0, 0); e.insert_table(3, 2, 1_000).unwrap();
        let top = e.doc.children(None).to_vec(); assert_eq!(top.len(), 3, "the text above, the table, a paragraph to get below it"); let table = top[1];
        assert!(matches!(&e.doc.get(table).unwrap().kind, Kind::Table { header_rows: 1, columns } if columns.len() == 2));
        let cells = |e: &Editor| -> Vec<BlockId> { e.doc.children(Some(table)).iter().flat_map(|r| e.doc.children(Some(*r)).to_vec()).collect() };
        assert_eq!((e.doc.children(Some(table)).len(), cells(&e).len()), (3, 6));
        let here = |e: &Editor| { let f = e.selection().unwrap().focus; let (_, _, c) = e.cell_of(f.block).expect("in a cell"); cells(e).iter().position(|x| *x == c).unwrap() };
        assert_eq!(here(&e), 0);
        // typing in a cell is ordinary typing; Tab goes on, Shift+Tab back, to the END of what the cell holds
        e.insert("Shot", 2_000).unwrap(); e.tab_cell(false, 3_000).unwrap(); assert_eq!(here(&e), 1); e.insert("Note", 4_000).unwrap();
        e.tab_cell(true, 5_000).unwrap(); assert_eq!(here(&e), 0); assert_eq!(e.selection().unwrap().focus.at, 4);
        assert_eq!(e.tab_cell(true, 5_500).unwrap(), Some(Dirty::Nothing), "the first cell: Shift+Tab stays");
        // past the last cell a row is added; Cmd+Enter's row goes below the caret's row, the caret into the same column
        for _ in 0..5 { e.tab_cell(false, 6_000).unwrap(); } assert_eq!(here(&e), 5);
        e.tab_cell(false, 7_000).unwrap(); assert_eq!((e.doc.children(Some(table)).len(), here(&e)), (4, 6));
        e.tab_cell(false, 8_000).unwrap(); assert_eq!(here(&e), 7); e.add_row_below(9_000).unwrap(); assert_eq!((e.doc.children(Some(table)).len(), here(&e)), (5, 8), "the last cell of the last row: the new row's first cell");
        let in_fourth = e.doc.children(Some(cells(&e)[3])).first().copied().unwrap(); at(&mut e, in_fourth, 0); e.add_row_below(10_000).unwrap(); assert_eq!((e.doc.children(Some(table)).len(), here(&e)), (6, 5), "under the cell it was in");
        e.undo().unwrap(); assert_eq!(e.doc.children(Some(table)).len(), 5);
        // no table in a cell, no columns in a cell; outside a table Tab is not the table's
        assert_eq!(e.insert_table(2, 2, 11_000).unwrap(), Dirty::Nothing); assert_eq!(e.insert_columns(11_500).unwrap(), Dirty::Nothing);
        at(&mut e, a, 5); assert_eq!(e.tab_cell(false, 12_000).unwrap(), None);
        // columns: the caret's block becomes the first of two, the caret goes into the second; again there: a third
        e.insert_columns(13_000).unwrap();
        let cols = e.doc.children(None)[0]; assert_eq!(e.doc.get(cols).unwrap().kind, Kind::Columns);
        let cs = e.doc.children(Some(cols)).to_vec(); assert_eq!(cs.len(), 2); assert_eq!(e.doc.children(Some(cs[0])), &[a]);
        let f = e.selection().unwrap().focus; assert_eq!(e.doc.get(f.block).unwrap().parent, Some(cs[1]));
        e.insert_columns(14_000).unwrap(); let cs = e.doc.children(Some(cols)).to_vec(); assert_eq!(cs.len(), 3);
        assert!(cs.iter().all(|c| matches!(e.doc.get(*c).unwrap().kind, Kind::Column { width } if (width - 1.0 / 3.0).abs() < 1e-6)));
        e.undo().unwrap(); e.undo().unwrap(); assert_eq!(e.doc.children(None)[0], a, "one undo per making");
    }
    #[test]
    fn rows_and_columns_are_added_removed_moved_and_copied_and_the_columns_settings_follow() {
        let mut d = Doc::new(); let e0 = add(&mut d, None, Kind::Paragraph, "");
        let mut e = Editor::new(d); e.set_selection(Some(Selection::caret(Pos { block: e0, at: 0 })));
        e.insert_table(2, 2, 1_000).unwrap(); let table = e.doc.children(None)[0];
        // a1 b1 / a2 b2
        for (i, w) in ["a1", "b1", "a2", "b2"].iter().enumerate() { e.insert(w, 2_000 + i as u64 * 2_000).unwrap(); if i < 3 { e.tab_cell(false, 2_500 + i as u64 * 2_000).unwrap(); } }
        let grid = |e: &Editor| -> Vec<Vec<String>> { e.doc.children(Some(table)).iter().map(|r| e.doc.children(Some(*r)).iter().map(|c| e.doc.children(Some(*c)).iter().map(|b| e.doc.get(*b).unwrap().text.clone()).collect::<Vec<_>>().join("|")).collect()).collect() };
        let spec = |e: &Editor| e.table_spec(table).unwrap();
        assert_eq!(grid(&e), [["a1", "b1"], ["a2", "b2"]]); assert_eq!(e.table_pos(), Some((table, 1, 1)));
        // the caret's column is set right; a column goes in left of it and the setting stays with ITS column
        e.table_column(|c| c.align = 2, 20_000).unwrap();
        e.table_insert(true, false, 21_000).unwrap(); assert_eq!(grid(&e), [["a1", "", "b1"], ["a2", "", "b2"]]);
        assert_eq!(spec(&e).1.iter().map(|c| c.align).collect::<Vec<_>>(), [0, 0, 2]); assert_eq!(e.table_pos(), Some((table, 1, 1)), "the caret is in the new column, its row");
        e.insert("n", 22_000).unwrap();
        // the column moves right: its cells and its settings swap places with the next
        e.table_move(true, true, 23_000).unwrap(); assert_eq!(grid(&e), [["a1", "b1", ""], ["a2", "b2", "n"]]); assert_eq!(spec(&e).1.iter().map(|c| c.align).collect::<Vec<_>>(), [0, 2, 0]); assert_eq!(e.table_pos(), Some((table, 1, 2)));
        // a row above, a copy of a row, a row moved up
        e.table_insert(false, false, 24_000).unwrap(); assert_eq!(grid(&e), [["a1", "b1", ""], ["", "", ""], ["a2", "b2", "n"]]); assert_eq!(e.table_pos(), Some((table, 1, 2)));
        e.table_remove(false, 25_000).unwrap(); assert_eq!(grid(&e), [["a1", "b1", ""], ["a2", "b2", "n"]]); assert_eq!(e.table_pos(), Some((table, 1, 2)));
        e.table_duplicate(false, 26_000).unwrap(); assert_eq!(grid(&e), [["a1", "b1", ""], ["a2", "b2", "n"], ["a2", "b2", "n"]]); assert_eq!(e.table_pos(), Some((table, 2, 2)));
        e.insert("!", 27_000).unwrap(); e.table_move(false, false, 28_000).unwrap(); assert_eq!(grid(&e)[1], ["a2", "b2", "!n"]); assert_eq!(e.table_pos(), Some((table, 1, 2)));
        // a copy of a column carries its settings; the header is switched off and on; removing the header row counts it down
        e.table_duplicate(true, 29_000).unwrap(); assert_eq!(grid(&e)[1], ["a2", "b2", "!n", "!n"]); assert_eq!(spec(&e).1.len(), 4);
        assert_eq!(spec(&e).0, 1); e.table_header(30_000).unwrap(); assert_eq!(spec(&e).0, 0); e.table_header(31_000).unwrap(); assert_eq!(spec(&e).0, 1);
        e.table_remove(true, 32_000).unwrap(); assert_eq!(grid(&e)[1], ["a2", "b2", "!n"]);
        // the last row and the last column stay
        e.table_remove(false, 33_000).unwrap(); e.table_remove(false, 34_000).unwrap(); assert_eq!(grid(&e).len(), 1); assert_eq!(e.table_remove(false, 35_000).unwrap(), Dirty::Nothing);
        e.table_remove(true, 36_000).unwrap(); e.table_remove(true, 37_000).unwrap(); assert_eq!(grid(&e)[0].len(), 1); assert_eq!(e.table_remove(true, 38_000).unwrap(), Dirty::Nothing);
        // every step undoes by itself
        e.undo().unwrap(); assert_eq!(grid(&e)[0].len(), 2);
    }
    #[test]
    fn blocks_from_another_note_are_appended_as_one_step_and_the_stamp_tells_edits_from_undo() {
        let mut a = Doc::new(); let x = add(&mut a, None, Kind::Bullet { depth: 0 }, "X"); add(&mut a, None, Kind::Bullet { depth: 1 }, "x1"); add(&mut a, None, Kind::Paragraph, "stay");
        let mut src = Editor::new(a); src.set_selection(Some(Selection::caret(Pos { block: x, at: 0 }))); src.toggle_blocks();
        let clip = src.copy_blocks().unwrap(); assert_eq!(clip.len(), 2);
        // a note with one empty paragraph is replaced; one with text gets the blocks at its end
        let mut e = Doc::new(); add(&mut e, None, Kind::Paragraph, ""); let mut empty = Editor::new(e);
        assert_eq!(empty.append_blocks(&clip, 10).unwrap(), 0); assert_eq!(empty.doc.children(None).len(), 2);
        let mut d = Doc::new(); add(&mut d, None, Kind::Paragraph, "there"); let mut dest = Editor::new(d);
        let before = dest.stamp(); dest.append_blocks(&clip, 10).unwrap();
        let texts = |e: &Editor| e.doc.children(None).iter().map(|id| e.doc.get(*id).unwrap().text.clone()).collect::<Vec<_>>().join(" ");
        assert_eq!(texts(&dest), "there X x1"); let after = dest.stamp(); assert_ne!(before, after);
        dest.undo().unwrap(); assert_eq!(texts(&dest), "there"); assert_eq!(dest.stamp().0, after.0, "undo is not a new step");
        dest.redo().unwrap(); assert_eq!(dest.stamp(), after);
    }
}
