//! Whole blocks (canon row 304, stage A of row 303): selecting them, moving them by key, cutting, copying and pasting
//! them. A selection of blocks is a run of SIBLINGS; a list item brings the deeper items that follow it (its children —
//! list nesting is a depth on the item), so they travel with it. While blocks are selected the text selection spans
//! them from first to last, so everything that works on a text selection (formatting, colours, indent, the plain text
//! that Copy writes) works on them too.
use crate::{end_of, paragraph, Dirty, Editor, Pos, Selection};
use mn_doc::{Block, BlockId, Change, ChangeError, Doc, Kind, Rank};
use std::collections::HashMap;

/// A list item's depth; None for everything else.
fn depth(doc: &Doc, id: BlockId) -> Option<u8> { match doc.get(id)?.kind { Kind::Bullet { depth } | Kind::Numbered { depth } | Kind::Task { depth, .. } => Some(depth), _ => None } }
fn first_leaf(doc: &Doc, mut id: BlockId) -> BlockId { while let Some(c) = doc.get(id).and_then(|b| b.children.first().copied()) { id = c; } id }
fn last_leaf(doc: &Doc, mut id: BlockId) -> BlockId { while let Some(c) = doc.get(id).and_then(|b| b.children.last().copied()) { id = c; } id }
/// The last sibling index of the run starting at `j` once the list children of `sibs[j]` are taken in.
fn with_children(doc: &Doc, sibs: &[BlockId], mut j: usize) -> usize {
    if let Some(d) = depth(doc, sibs[j]) { while j + 1 < sibs.len() && depth(doc, sibs[j + 1]).is_some_and(|x| x > d) { j += 1; } }
    j
}

/// Copied blocks: each entry is one block with everything under it, the top block first (as `Change::Insert` takes it).
pub type Clip = Vec<Vec<Block>>;

impl Editor {
    pub fn in_blocks(&self) -> bool { self.blocks.is_some() }
    /// The run the selection stands for: (parent, first index, last index) among the parent's children. In block mode it
    /// is the selected run; otherwise the caret's block, or the blocks a text selection touches when they are siblings.
    /// Children of the last list item are included.
    fn run(&self) -> Option<(Option<BlockId>, usize, usize)> {
        let (a, z) = match self.blocks { Some(x) => x, None => { let (a, z) = self.ordered()?; (a.block, z.block) } };
        let parent = self.doc.get(a)?.parent; if self.doc.get(z)?.parent != parent { return None }
        let sibs = self.doc.children(parent); let (i, j) = (sibs.iter().position(|x| *x == a)?, sibs.iter().position(|x| *x == z)?);
        let (i, j) = (i.min(j), i.max(j));
        Some((parent, i, with_children(&self.doc, sibs, j)))
    }
    /// The selected blocks (block mode only), for drawing.
    pub fn block_selection(&self) -> Vec<BlockId> {
        if self.blocks.is_none() { return vec![] }
        match self.run() { Some((p, i, j)) => self.doc.children(p)[i..=j].to_vec(), None => vec![] }
    }
    /// Sets the text selection to span the selected run (first block's start to the last block's end).
    fn span_blocks(&mut self) {
        let Some((p, i, j)) = self.run() else { self.blocks = None; return };
        let sibs = self.doc.children(p); let (f, l) = (first_leaf(&self.doc, sibs[i]), last_leaf(&self.doc, sibs[j]));
        let end = self.doc.get(l).map(end_of).unwrap_or(0);
        self.sel = Some(Selection { anchor: Pos { block: f, at: 0 }, focus: Pos { block: l, at: end } });
        self.goal_x = None; self.upstream = false; self.armed.clear();
    }
    /// Escape: from a caret or a text selection to whole blocks (the caret's block; the blocks a selection touches when
    /// they are siblings) — and from whole blocks back to a caret at the end of the last. True: blocks are now selected.
    pub fn toggle_blocks(&mut self) -> bool {
        if self.preedit.is_some() { return false }
        if self.blocks.take().is_some() { if let Some(s) = self.sel { self.sel = Some(Selection::caret(s.focus)); } return false }
        let Some((a, z)) = self.ordered() else { return false };
        // inside a container the unit is the leaf itself; a selection across containers has no run of siblings
        if self.doc.get(a.block).map(|b| b.parent) != self.doc.get(z.block).map(|b| b.parent) { return false }
        self.blocks = Some((a.block, z.block)); self.span_blocks(); self.blocks.is_some()
    }
    /// Every top-level block (Cmd+A while blocks are selected).
    pub fn select_all_blocks(&mut self) { if let (Some(a), Some(z)) = (self.doc.children(None).first().copied(), self.doc.children(None).last().copied()) { self.blocks = Some((a, z)); self.span_blocks(); } }
    /// Up / Down with blocks selected: the selection goes to the neighbour (a parent item's children are stepped over
    /// with it), or with `extend` grows / shrinks towards it.
    pub fn step_blocks(&mut self, down: bool, extend: bool) {
        let Some((anchor, focus)) = self.blocks else { return };
        let parent = self.doc.get(focus).and_then(|b| b.parent); let sibs = self.doc.children(parent).to_vec();
        let Some(f) = sibs.iter().position(|x| *x == focus) else { return };
        // the focus is a unit's head; its end is the head with its children
        let next = if down { let e = with_children(&self.doc, &sibs, f); (e + 1 < sibs.len()).then(|| e + 1) }
            else { f.checked_sub(1).map(|mut k| { let d = depth(&self.doc, focus).unwrap_or(0); while k > 0 && depth(&self.doc, sibs[k]).is_some_and(|x| x > d) { k -= 1; } k }) };
        let Some(n) = next else { return };
        self.blocks = Some(if extend { (anchor, sibs[n]) } else { (sibs[n], sibs[n]) }); self.span_blocks();
    }
    /// Ctrl+Cmd+Up / Down: the run moves one place among its siblings, as one undo step. A list item steps over a whole
    /// neighbouring item with ITS children (it does not land between a parent and its child); a deeper item moved down
    /// past a shallower one becomes that one's first child. Stage A stays inside the run's own container.
    pub fn move_blocks(&mut self, down: bool, now_ms: u64) -> Result<Dirty, ChangeError> {
        if self.preedit.is_some() { return Ok(Dirty::Nothing) }
        let Some((parent, i, j)) = self.run() else { return Ok(Dirty::Nothing) };
        let sibs = self.doc.children(parent).to_vec(); let d = depth(&self.doc, sibs[i]).unwrap_or(0);
        let mut b = self.batch();
        if down {
            if j + 1 >= sibs.len() { return Ok(Dirty::Nothing) }
            // after the next block — and after its children, unless the run is deeper than it (then it becomes its first child)
            let next = j + 1; let m = if depth(&self.doc, sibs[next]).is_some_and(|x| x < d) { next } else { with_children(&self.doc, &sibs, next) };
            for (t, k) in (i..=j).rev().enumerate() { b.push(Change::Move { id: sibs[k], from: (parent, k), to: (parent, m - t) })?; }
        } else {
            if i == 0 { return Ok(Dirty::Nothing) }
            // before the previous block — or before the item it belongs to, when that block is someone's child deeper than the run
            let mut k = i - 1; while k > 0 && depth(&self.doc, sibs[k]).is_some_and(|x| x > d) { k -= 1; }
            if depth(&self.doc, sibs[k]).is_none() && k < i - 1 && depth(&self.doc, sibs[k + 1]).is_some_and(|x| x > d) { k += 1; }
            for (t, s) in (i..=j).enumerate() { b.push(Change::Move { id: sibs[s], from: (parent, s), to: (parent, k + t) })?; }
        }
        let (keep, blocks) = (self.sel, self.blocks);
        let dirty = self.commit(b, None, now_ms)?;
        self.sel = keep; self.blocks = blocks; if blocks.is_some() { self.span_blocks(); }
        if let Some(m) = self.marks.last_mut() { m.1 = self.sel; }
        Ok(dirty)
    }
    /// The selected blocks, each with everything under it (block mode only): what Copy keeps.
    pub fn copy_blocks(&self) -> Option<Clip> {
        if self.blocks.is_none() { return None }
        let (p, i, j) = self.run()?;
        self.doc.children(p)[i..=j].iter().map(|id| match Change::removal(&self.doc, *id) { Ok(Change::Remove { blocks, .. }) => Some(blocks), _ => None }).collect()
    }
    /// The selected blocks go (Backspace, Delete, the second half of Cut), as one undo step. The caret lands at the
    /// start of what followed them, else at the end of what came before; a container left empty gets an empty paragraph.
    pub fn delete_blocks(&mut self, now_ms: u64) -> Result<Dirty, ChangeError> {
        if self.blocks.is_none() { return Ok(Dirty::Nothing) }
        let Some((parent, i, j)) = self.run() else { return Ok(Dirty::Nothing) };
        let sibs = self.doc.children(parent).to_vec(); let mut b = self.batch();
        for id in &sibs[i..=j] { let c = Change::removal(&b.scratch, *id)?; b.push(c)?; }
        let after = if let Some(n) = sibs.get(j + 1) { Pos { block: first_leaf(&self.doc, *n), at: 0 } }
            else if i > 0 { let l = last_leaf(&self.doc, sibs[i - 1]); Pos { block: l, at: self.doc.get(l).map(end_of).unwrap_or(0) } }
            else { let para = paragraph(parent, ""); let id = para.id; b.push(Change::Insert { index: 0, blocks: vec![para] })?; Pos { block: id, at: 0 } };
        self.commit(b, Some(after), now_ms)
    }
    /// Pastes copied blocks AFTER the run the selection stands for (the caret's block with its children), under the
    /// same parent, with fresh identities, as one undo step; an empty paragraph the caret stood in is replaced by them.
    /// A block the place does not take (a table inside a table's cell, columns inside a column) is left out: the
    /// number left out is returned with what the layout must do. The pasted blocks end up selected.
    pub fn paste_blocks(&mut self, clip: &Clip, now_ms: u64) -> Result<(Dirty, usize), ChangeError> {
        if self.preedit.is_some() || clip.is_empty() { return Ok((Dirty::Nothing, 0)) }
        let Some((parent, i, j)) = self.run() else { return Ok((Dirty::Nothing, 0)) };
        let sibs = self.doc.children(parent).to_vec();
        let replace = (self.blocks.is_none() && i == j).then(|| self.doc.get(sibs[i])).flatten().filter(|x| x.kind == Kind::Paragraph && x.text.is_empty()).map(|x| x.id);
        let mut b = self.batch(); let (mut at, mut refused, mut tops) = (j + 1, 0usize, vec![]);
        for tree in clip {
            let ids: HashMap<BlockId, BlockId> = tree.iter().map(|x| (x.id, BlockId::new())).collect();
            let mut blocks: Vec<Block> = tree.iter().map(|x| { let mut n = x.clone(); n.id = ids[&x.id]; n.parent = x.parent.and_then(|p| ids.get(&p).copied()); n.children = x.children.iter().map(|c| ids[c]).collect(); n.conflict = None; n }).collect();
            let Some(top) = blocks.first_mut() else { continue }; top.parent = parent; top.rank = Rank::first(); let top_id = top.id;
            match b.push(Change::Insert { index: at, blocks }) { Ok(()) => { at += 1; tops.push(top_id); } Err(ChangeError::Tree(_)) => refused += 1, Err(e) => return Err(e) }
        }
        if tops.is_empty() { return Ok((Dirty::Nothing, refused)) }
        if let Some(id) = replace { let c = Change::removal(&b.scratch, id)?; b.push(c)?; }
        let dirty = self.commit(b, None, now_ms)?;
        self.blocks = Some((tops[0], *tops.last().unwrap())); self.span_blocks();
        if let Some(m) = self.marks.last_mut() { m.1 = self.sel; }
        Ok((dirty, refused))
    }
}
