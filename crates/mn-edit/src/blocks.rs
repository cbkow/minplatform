//! Whole blocks (canon row 304, stage A of row 303): selecting them, moving them by key, cutting, copying and pasting
//! them. A selection of blocks is a run of SIBLINGS; a list item brings the deeper items that follow it (its children —
//! list nesting is a depth on the item), so they travel with it. While blocks are selected the text selection spans
//! them from first to last, so everything that works on a text selection (formatting, colours, indent, the plain text
//! that Copy writes) works on them too.
use crate::{end_of, paragraph, Batch, Dirty, Editor, Pos, Selection};
use kurbo::Rect;
use mn_layout::NoteLayout;
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

/// Where dragged blocks would land (stage B): child `index` of `parent` — an index among the children as they stand
/// BEFORE the move — and, for list items, the depth the run's first item takes (the rest keep their distance to it).
#[derive(Clone, Debug, PartialEq)]
pub struct Drop { pub parent: Option<BlockId>, pub index: usize, pub depth: Option<u8> }

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
    /// The block a handle stands beside: the first of the run the selection stands for.
    pub fn handle_block(&self) -> Option<BlockId> { if self.preedit.is_some() { return None } let (p, i, _) = self.run()?; self.doc.children(p).get(i).copied() }
    /// How many blocks a drag would carry (children counted).
    pub fn run_len(&self) -> usize { self.run().map(|(_, i, j)| j - i + 1).unwrap_or(0) }
    /// The changes that put the run at `d`. None: it is where it already is. An error: the place does not take it (a
    /// block into itself, columns into a column, a table into a cell — the tree's rules, row 273).
    fn plan_drop(&self, d: &Drop) -> Result<Option<Batch>, ChangeError> {
        let Some((parent, i, j)) = self.run() else { return Ok(None) };
        let sibs = self.doc.children(parent).to_vec(); let mut b = self.batch();
        if d.parent == parent {
            if d.index > j + 1 { let m = d.index - 1; for (t, k) in (i..=j).rev().enumerate() { b.push(Change::Move { id: sibs[k], from: (parent, k), to: (parent, m - t) })?; } }
            else if d.index < i { for (t, k) in (i..=j).enumerate() { b.push(Change::Move { id: sibs[k], from: (parent, k), to: (parent, d.index + t) })?; } }
        } else {
            for t in 0..=(j - i) { b.push(Change::Move { id: sibs[i + t], from: (parent, i), to: (d.parent, d.index + t) })?; }
            // a column or a cell the run leaves empty keeps a paragraph to type in
            if let Some(p) = parent { if b.scratch.children(Some(p)).is_empty() { b.push(Change::Insert { index: 0, blocks: vec![paragraph(Some(p), "")] })?; } }
        }
        if let (Some(want), Some(d0)) = (d.depth, depth(&self.doc, sibs[i])) { let delta = want as i16 - d0 as i16; if delta != 0 { for id in &sibs[i..=j] {
            let old = self.doc.get(*id).unwrap().kind.clone(); let step = |x: u8| (x as i16 + delta).clamp(0, 8) as u8;
            let new = match &old { Kind::Bullet { depth } => Kind::Bullet { depth: step(*depth) }, Kind::Numbered { depth } => Kind::Numbered { depth: step(*depth) }, Kind::Task { depth, state } => Kind::Task { depth: step(*depth), state: *state }, _ => continue };
            if new != old { b.push(Change::Kind { id: *id, old, new })?; } } } }
        Ok((!b.changes.is_empty()).then_some(b))
    }
    /// The dragged run lands at `d`, as one undo step; the selection stays on it.
    pub fn drop_blocks(&mut self, d: &Drop, now_ms: u64) -> Result<Dirty, ChangeError> {
        let Some(b) = self.plan_drop(d)? else { return Ok(Dirty::Nothing) };
        let (keep, blocks) = (self.sel, self.blocks);
        let dirty = self.commit(b, None, now_ms)?;
        self.sel = keep; self.blocks = blocks; if blocks.is_some() { self.span_blocks(); }
        if let Some(m) = self.marks.last_mut() { m.1 = self.sel; }
        Ok(dirty)
    }
    /// Where the run would land if released at a point of the page, with the line that shows it (the layout's
    /// coordinates). None: nowhere — the place does not take it, or it is where it already is.
    pub fn drop_at(&self, layout: &NoteLayout, x: f64, y: f64) -> Option<(Drop, Rect)> {
        let (run_parent, ri, rj) = self.run()?; let head = self.doc.children(run_parent)[ri];
        let s = layout.scale as f64; let edge = 6.0 * s;
        let mut parent: Option<BlockId> = None;
        let (index, kids, rects) = loop {
            let kids = self.doc.children(parent).to_vec();
            let rects: Vec<Rect> = kids.iter().filter_map(|id| layout.rect(&self.doc, *id)).collect();
            if kids.is_empty() || rects.len() != kids.len() { break (0, kids, rects) }
            let across = matches!(parent.and_then(|p| self.doc.get(p)).map(|b| &b.kind), Some(Kind::Columns | Kind::Row { .. }));
            let dist = |r: &Rect| if across { if x < r.x0 { r.x0 - x } else if x > r.x1 { x - r.x1 } else { 0.0 } } else if y < r.y0 { r.y0 - y } else if y > r.y1 { y - r.y1 } else { 0.0 };
            let k = (0..kids.len()).min_by(|a, b| dist(&rects[*a]).total_cmp(&dist(&rects[*b]))).unwrap();
            let (id, r) = (kids[k], rects[k]);
            if across { parent = Some(id); continue }
            match self.doc.get(id).map(|b| &b.kind) {
                // into a container, unless the pointer is at its very top or bottom: then beside it
                Some(Kind::Columns | Kind::Table { .. }) if y > r.y0 + edge && y < r.y1 - edge => { parent = Some(id); continue }
                Some(Kind::Row { .. }) => { parent = Some(id); continue }
                _ => break (if y < r.center().y { k } else { k + 1 }, kids, rects),
            }
        };
        // a list item's depth: between what the item below needs and one deeper than the item above, by how far right the pointer is
        let frame = match parent { None => Rect::new(0.0, 0.0, layout.width as f64, layout.height as f64), Some(p) => layout.rect(&self.doc, p)? };
        let indent = layout.style.list_indent as f64 * s;
        let depth_here = depth(&self.doc, head).map(|_| {
            let outside = |k: usize| run_parent != parent || k < ri || k > rj;
            let above = (0..index).rev().find(|k| outside(*k)).map(|k| kids[k]); let below = (index..kids.len()).find(|k| outside(*k)).map(|k| kids[k]);
            let min = below.and_then(|b| depth(&self.doc, b)).unwrap_or(0); let max = above.and_then(|a| depth(&self.doc, a)).map(|d| d + 1).unwrap_or(0).max(min).min(8);
            (((x - frame.x0 - layout.style.marker as f64 * s) / indent).round().max(0.0) as u8).clamp(min, max)
        });
        let d = Drop { parent, index, depth: depth_here };
        if !matches!(self.plan_drop(&d), Ok(Some(_))) { return None }
        let ly = if rects.is_empty() { frame.y0 } else if index == 0 { rects[0].y0 - 3.0 * s } else if index >= rects.len() { rects[rects.len() - 1].y1 + 3.0 * s } else { (rects[index - 1].y1 + rects[index].y0) / 2.0 };
        Some((d.clone(), Rect::new(frame.x0 + d.depth.unwrap_or(0) as f64 * indent, ly - s, frame.x1, ly + s)))
    }
    /// A copy of the run right after it (the caret's block with its children when no blocks are selected), as one undo
    /// step; the copies end up selected.
    pub fn duplicate_blocks(&mut self, now_ms: u64) -> Result<Dirty, ChangeError> {
        if !self.in_blocks() && !self.toggle_blocks() { return Ok(Dirty::Nothing) }
        let Some(clip) = self.copy_blocks() else { return Ok(Dirty::Nothing) };
        self.paste_blocks(&clip, now_ms).map(|(d, _)| d)
    }
    /// Whether a block is among the selected blocks or inside one of them.
    pub fn in_block_selection(&self, id: BlockId) -> bool {
        let picked = self.block_selection(); let mut up = Some(id);
        while let Some(x) = up { if picked.contains(&x) { return true } up = self.doc.get(x).and_then(|b| b.parent); }
        false
    }
    /// Blocks from elsewhere (another note) are added at the END of this note with fresh identities, as one undo step
    /// that never joins the step before it; a note that holds nothing but one empty paragraph is replaced by them.
    /// Returns how many the note would not take.
    pub fn append_blocks(&mut self, clip: &Clip, now_ms: u64) -> Result<usize, ChangeError> {
        if self.preedit.is_some() || clip.is_empty() { return Ok(0) }
        let root = self.doc.children(None).to_vec();
        let lone = (root.len() == 1).then(|| self.doc.get(root[0])).flatten().filter(|x| x.kind == Kind::Paragraph && x.text.is_empty()).map(|x| x.id);
        let mut b = self.batch(); let (mut at, mut refused, mut any) = (root.len(), 0usize, false);
        for tree in clip {
            let ids: HashMap<BlockId, BlockId> = tree.iter().map(|x| (x.id, BlockId::new())).collect();
            let mut blocks: Vec<Block> = tree.iter().map(|x| { let mut n = x.clone(); n.id = ids[&x.id]; n.parent = x.parent.and_then(|p| ids.get(&p).copied()); n.children = x.children.iter().map(|c| ids[c]).collect(); n.conflict = None; n }).collect();
            let Some(top) = blocks.first_mut() else { continue }; top.parent = None; top.rank = Rank::first();
            match b.push(Change::Insert { index: at, blocks }) { Ok(()) => { at += 1; any = true; } Err(ChangeError::Tree(_)) => refused += 1, Err(e) => return Err(e) }
        }
        if !any { return Ok(refused) }
        if let Some(id) = lone { let c = Change::removal(&b.scratch, id)?; b.push(c)?; }
        let keep = self.sel;
        self.commit(b, None, now_ms + mn_doc::change::JOIN_MS + 1)?;
        self.sel = keep.and_then(|s| self.live(s)); if let Some(m) = self.marks.last_mut() { m.1 = self.sel; }
        Ok(refused)
    }
}
