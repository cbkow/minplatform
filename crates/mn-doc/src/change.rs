//! Edits as records (rows 275, 283). Every change to a document is one of these; each knows its inverse, so ONE history
//! per document serves undo and redo across typing and structure, and the set of blocks a history touched is what a
//! save has to merge.
use std::collections::BTreeSet;
use crate::{id::BlockId, tree::{Block, Doc, Kind, TreeError}};

#[derive(Clone, Debug, PartialEq)]
pub enum Change {
    /// A block with everything under it (the top block first) becomes child `index` of the top block's parent.
    Insert { index: usize, blocks: Vec<Block> },
    /// The inverse of Insert: the same subtree leaves.
    Remove { index: usize, blocks: Vec<Block> },
    Move { id: BlockId, from: (Option<BlockId>, usize), to: (Option<BlockId>, usize) },
    /// In `id`'s text, `removed` at byte `at` becomes `inserted`.
    Text { id: BlockId, at: usize, removed: String, inserted: String },
    Kind { id: BlockId, old: Kind, new: Kind },
}

#[derive(Debug, PartialEq)]
pub enum ChangeError { Tree(TreeError), TextMismatch { id: BlockId, at: usize }, Stale(BlockId) }
impl From<TreeError> for ChangeError { fn from(e: TreeError) -> Self { ChangeError::Tree(e) } }

impl Change {
    pub fn inverse(&self) -> Change {
        match self.clone() {
            Change::Insert { index, blocks } => Change::Remove { index, blocks },
            Change::Remove { index, blocks } => Change::Insert { index, blocks },
            Change::Move { id, from, to } => Change::Move { id, from: to, to: from },
            Change::Text { id, at, removed, inserted } => Change::Text { id, at, removed: inserted, inserted: removed },
            Change::Kind { id, old, new } => Change::Kind { id, old: new, new: old },
        }
    }
    /// The blocks this change touches (a subtree counts with every block in it).
    pub fn touched(&self, into: &mut BTreeSet<BlockId>) {
        match self {
            Change::Insert { blocks, .. } | Change::Remove { blocks, .. } => into.extend(blocks.iter().map(|b| b.id)),
            Change::Move { id, .. } | Change::Text { id, .. } | Change::Kind { id, .. } => { into.insert(*id); }
        }
    }
    /// Records the removal of `id` with its subtree, as the document stands now.
    pub fn removal(doc: &Doc, id: BlockId) -> Result<Change, ChangeError> {
        let index = doc.index_of(id).ok_or(TreeError::UnknownBlock(id))?;
        let mut blocks = vec![]; let mut todo = vec![id];
        while let Some(x) = todo.pop() { let b = doc.get(x).ok_or(TreeError::UnknownBlock(x))?; todo.extend(b.children.iter().rev().copied()); blocks.push(b.clone()); }
        Ok(Change::Remove { index, blocks })
    }
}

impl Doc {
    /// Applies a change, checking that the document is in the state the change was recorded against (a record applied
    /// to anything else is refused, never guessed at).
    pub fn apply(&mut self, c: &Change) -> Result<(), ChangeError> {
        match c {
            Change::Insert { index, blocks } => self.insert_tree(*index, blocks)?,
            Change::Remove { index, blocks } => {
                let Some(top) = blocks.first() else { return Ok(()) };
                if self.index_of(top.id) != Some(*index) || self.get(top.id).map(|b| b.parent) != Some(top.parent) { return Err(ChangeError::Stale(top.id)) }
                self.remove(top.id)?;
            }
            Change::Move { id, from, to } => {
                if self.get(*id).map(|b| b.parent) != Some(from.0) || self.index_of(*id) != Some(from.1) { return Err(ChangeError::Stale(*id)) }
                self.move_to(*id, to.0, to.1)?;
            }
            Change::Text { id, at, removed, inserted } => {
                let b = self.get(*id).ok_or(TreeError::UnknownBlock(*id))?;
                let end = at + removed.len();
                if b.text.get(*at..end) != Some(removed.as_str()) { return Err(ChangeError::TextMismatch { id: *id, at: *at }) }
                let mut t = String::with_capacity(b.text.len() + inserted.len());
                t.push_str(&b.text[..*at]); t.push_str(inserted); t.push_str(&b.text[end..]);
                self.set_text(*id, t)?;
            }
            Change::Kind { id, old, new } => {
                if self.get(*id).map(|b| &b.kind) != Some(old) { return Err(ChangeError::Stale(*id)) }
                self.set_kind(*id, new.clone())?;
            }
        }
        Ok(())
    }
}

/// The window within which a pure insert continues the previous step (the spike S9, step 3h: a writing tool types one
/// correction as several key events 1–86 ms apart, on Windows one character per event).
pub const JOIN_MS: u64 = 300;

#[derive(Clone, Debug, PartialEq)]
struct Step { changes: Vec<Change>, at_ms: u64 }

/// One undo history per document (row 275).
#[derive(Clone, Debug, Default)]
pub struct History { undo: Vec<Step>, redo: Vec<Step>, touched: BTreeSet<BlockId> }

impl History {
    pub fn new() -> Self { Self::default() }
    /// Applies `changes` as ONE undo step and records it. `now_ms` is any clock that only moves forward.
    /// A step that is a single pure insert of text exactly where the previous step's last insert ended, in the same
    /// block and within [`JOIN_MS`], joins that step instead of starting a new one.
    pub fn commit(&mut self, doc: &mut Doc, changes: Vec<Change>, now_ms: u64) -> Result<(), ChangeError> {
        for (i, c) in changes.iter().enumerate() {
            if let Err(e) = doc.apply(c) { for done in changes[..i].iter().rev() { let _ = doc.apply(&done.inverse()); } return Err(e) }
        }
        for c in &changes { c.touched(&mut self.touched); }
        self.redo.clear();
        if let [Change::Text { id, at, removed, inserted }] = changes.as_slice() {
            if let Some(Step { changes: prev, at_ms }) = self.undo.last_mut() {
                if let Some(Change::Text { id: pid, at: pat, inserted: pins, .. }) = prev.last_mut() {
                    if removed.is_empty() && pid == id && *pat + pins.len() == *at && now_ms.saturating_sub(*at_ms) < JOIN_MS {
                        pins.push_str(inserted); *at_ms = now_ms; return Ok(())
                    }
                }
            }
        }
        if !changes.is_empty() { self.undo.push(Step { changes, at_ms: now_ms }); }
        Ok(())
    }
    /// Takes the last step back; false when there is none.
    pub fn undo(&mut self, doc: &mut Doc) -> Result<bool, ChangeError> {
        let Some(step) = self.undo.pop() else { return Ok(false) };
        for c in step.changes.iter().rev() { doc.apply(&c.inverse())?; }
        self.redo.push(step); Ok(true)
    }
    pub fn redo(&mut self, doc: &mut Doc) -> Result<bool, ChangeError> {
        let Some(mut step) = self.redo.pop() else { return Ok(false) };
        for c in &step.changes { doc.apply(c)?; }
        step.at_ms = 0;   // a redone step is closed: typing after it starts a new one
        self.undo.push(step); Ok(true)
    }
    pub fn can_undo(&self) -> bool { !self.undo.is_empty() }
    pub fn can_redo(&self) -> bool { !self.redo.is_empty() }
    pub fn steps(&self) -> usize { self.undo.len() }
    /// Every block any recorded change touched since the history began (undone ones included: a block changed and
    /// changed back is still compared at save).
    pub fn touched(&self) -> &BTreeSet<BlockId> { &self.touched }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn doc() -> (Doc, BlockId, BlockId) {
        let mut d = Doc::new(); let (a, b) = (BlockId::new(), BlockId::new());
        d.insert(None, 0, a, Kind::Paragraph, "We was in a hurry").unwrap();
        d.insert(None, 1, b, Kind::Paragraph, "second").unwrap();
        (d, a, b)
    }
    fn text(d: &Doc, id: BlockId) -> &str { &d.get(id).unwrap().text }
    fn ins(id: BlockId, at: usize, s: &str) -> Vec<Change> { vec![Change::Text { id, at, removed: String::new(), inserted: s.into() }] }

    /// The spike S9's finding: a correction is a replacement of the selection followed by more typed chunks.
    #[test]
    fn a_typed_burst_is_one_undo_step() {
        let (mut d, a, _) = doc(); let mut h = History::new();
        h.commit(&mut d, vec![Change::Text { id: a, at: 3, removed: "was".into(), inserted: "w".into() }], 1000).unwrap();
        for (i, ch) in ["e", "r", "e"].iter().enumerate() { h.commit(&mut d, ins(a, 4 + i, ch), 1001 + i as u64).unwrap(); }
        assert_eq!((text(&d, a), h.steps()), ("We were in a hurry", 1));
        assert!(h.undo(&mut d).unwrap());
        assert_eq!(text(&d, a), "We was in a hurry");
        assert!(h.redo(&mut d).unwrap());
        assert_eq!(text(&d, a), "We were in a hurry");
    }
    #[test]
    fn a_pause_a_jump_another_block_or_a_deletion_starts_a_new_step() {
        let (mut d, a, b) = doc(); let mut h = History::new();
        h.commit(&mut d, ins(a, 0, "x"), 0).unwrap();
        h.commit(&mut d, ins(a, 1, "y"), 299).unwrap();                       // joins (within the window of the LAST key)
        h.commit(&mut d, ins(a, 2, "z"), 299 + JOIN_MS).unwrap();              // a pause
        h.commit(&mut d, ins(a, 0, "q"), 299 + JOIN_MS + 1).unwrap();          // not where the last insert ended
        h.commit(&mut d, ins(b, 0, "r"), 299 + JOIN_MS + 2).unwrap();          // another block
        h.commit(&mut d, vec![Change::Text { id: b, at: 0, removed: "r".into(), inserted: String::new() }], 299 + JOIN_MS + 3).unwrap();
        assert_eq!(h.steps(), 5);
        while h.undo(&mut d).unwrap() {}
        assert_eq!((text(&d, a), text(&d, b)), ("We was in a hurry", "second"));
    }
    #[test]
    fn structure_undoes_with_its_subtree_and_a_step_is_all_or_nothing() {
        let (mut d, a, b) = doc(); let mut h = History::new(); let before = d.clone();
        let (cols, col, p) = (BlockId::new(), BlockId::new(), BlockId::new());
        let block = |id, parent, kind: Kind, children: Vec<BlockId>| Block { id, parent, kind, text: String::new(), children };
        // one step: a Columns block with one column appears, and `b` moves into it
        h.commit(&mut d, vec![
            Change::Insert { index: 2, blocks: vec![block(cols, None, Kind::Columns, vec![col]), block(col, Some(cols), Kind::Column, vec![])] },
            Change::Move { id: b, from: (None, 1), to: (Some(col), 0) },
        ], 0).unwrap();
        assert_eq!(d.children(Some(col)), &[b]);
        let removal = Change::removal(&d, cols).unwrap();
        h.commit(&mut d, vec![removal], 10).unwrap();
        assert_eq!((d.len(), d.get(b)), (1, None));
        h.undo(&mut d).unwrap();
        assert_eq!((d.len(), d.children(Some(col))), (4, &[b][..]));
        h.undo(&mut d).unwrap();
        assert_eq!(d, before);
        // a step whose second change cannot apply leaves the document as it was
        let bad = vec![Change::Kind { id: a, old: Kind::Paragraph, new: Kind::Quote }, Change::Insert { index: 0, blocks: vec![block(p, Some(a), Kind::Paragraph, vec![])] }];
        assert!(h.commit(&mut d, bad, 20).is_err());
        assert_eq!(d, before);
        assert!(h.touched().contains(&cols) && h.touched().contains(&b));
    }
    #[test]
    fn a_record_is_refused_on_a_document_it_was_not_made_for() {
        let (mut d, a, _) = doc();
        assert_eq!(d.apply(&Change::Text { id: a, at: 3, removed: "were".into(), inserted: "was".into() }), Err(ChangeError::TextMismatch { id: a, at: 3 }));
        assert_eq!(d.apply(&Change::Move { id: a, from: (None, 1), to: (None, 0) }), Err(ChangeError::Stale(a)));
        assert_eq!(d.apply(&Change::Kind { id: a, old: Kind::Quote, new: Kind::Paragraph }), Err(ChangeError::Stale(a)));
        assert!(d.apply(&Change::Text { id: a, at: 1, removed: "\u{e9}".into(), inserted: String::new() }).is_err());
    }
}
