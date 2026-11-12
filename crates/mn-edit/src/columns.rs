//! Columns made and unmade by what is done to their blocks (canon row 319; row 317 rulings 10 and stage 4): blocks
//! dropped BESIDE a block make two columns of them (beside a column: one more column, six at most); a column that is
//! emptied goes, and one column left alone unwraps into ordinary blocks.
use kurbo::Rect;
use mn_doc::{BlockId, Change, ChangeError, Kind};
use mn_layout::NoteLayout;
use crate::{end_of, paragraph, table::bare, Batch, Dirty, Drop, Editor, Pos};

impl Editor {
    /// A container the run left empty: a cell keeps a paragraph to type in, a column goes.
    pub(crate) fn emptied(&self, b: &mut Batch, p: BlockId) -> Result<(), ChangeError> {
        match b.scratch.get(p).map(|x| (x.kind.clone(), x.parent)) {
            Some((Kind::Column { .. }, Some(cols))) => self.tidy_columns(b, cols),
            Some(_) => b.push(Change::Insert { index: 0, blocks: vec![paragraph(Some(p), "")] }),
            None => Ok(()),
        }
    }
    /// A Columns block is put right: its empty columns go; none left - a paragraph in its place; one left - its blocks
    /// take the Columns block's place; several - their widths share the whole again.
    pub(crate) fn tidy_columns(&self, b: &mut Batch, cols: BlockId) -> Result<(), ChangeError> {
        for c in b.scratch.children(Some(cols)).to_vec() { if b.scratch.children(Some(c)).is_empty() { let ch = Change::removal(&b.scratch, c)?; b.push(ch)?; } }
        let left = b.scratch.children(Some(cols)).to_vec(); let Some(me) = b.scratch.get(cols) else { return Ok(()) }; let (parent, at) = (me.parent, b.scratch.index_of(cols).unwrap_or(0));
        match left.len() {
            0 => { b.push(Change::Insert { index: at, blocks: vec![paragraph(parent, "")] })?; let ch = Change::removal(&b.scratch, cols)?; b.push(ch) }
            1 => { for (k, kid) in b.scratch.children(Some(left[0])).to_vec().into_iter().enumerate() { b.push(Change::Move { id: kid, from: (Some(left[0]), 0), to: (parent, at + 1 + k) })?; }
                let ch = Change::removal(&b.scratch, cols)?; b.push(ch) }
            n => { let w: Vec<f32> = left.iter().map(|c| match b.scratch.get(*c).map(|x| &x.kind) { Some(Kind::Column { width }) if *width > 0.0 => *width, _ => 1.0 / n as f32 }).collect(); let total: f32 = w.iter().sum();
                for (c, w) in left.iter().zip(w) { let old = b.scratch.get(*c).unwrap().kind.clone(); let new = Kind::Column { width: w / total }; if old != new { b.push(Change::Kind { id: *c, old, new })?; } } Ok(()) }
        }
    }
    /// The changes that put the run beside `target` (see `Drop::beside`). None: there is nothing to do, or no room.
    pub(crate) fn plan_beside(&self, target: BlockId, at: usize) -> Result<Option<Batch>, ChangeError> {
        let Some((parent, i, j)) = self.run() else { return Ok(None) }; let sibs = self.doc.children(parent).to_vec(); let run = &sibs[i..=j];
        let Some(t) = self.doc.get(target) else { return Ok(None) }; if run.contains(&target) || t.parent.is_some() { return Ok(None) }
        let mut b = self.batch();
        let (col, hold) = if t.kind == Kind::Columns {
            let cols = self.doc.children(Some(target)).to_vec(); let n = cols.len(); if n >= 6 || n == 0 { return Ok(None) }
            // the run is the whole of one of its columns and would only be put back where it is
            if let Some(p) = parent { if let Some(k) = cols.iter().position(|c| *c == p) { if run.len() == self.doc.children(Some(p)).len() && (at == k || at == k + 1) { return Ok(None) } } }
            for c in &cols { let old = self.doc.get(*c).unwrap().kind.clone(); let w = match &old { Kind::Column { width } if *width > 0.0 => *width, _ => 1.0 / n as f32 }; b.push(Change::Kind { id: *c, old, new: Kind::Column { width: w * n as f32 / (n + 1) as f32 } })?; }
            let mut c = bare(Kind::Column { width: 1.0 / (n + 1) as f32 }, Some(target)); let hold = paragraph(Some(c.id), ""); c.children.push(hold.id); let ids = (c.id, hold.id);
            b.push(Change::Insert { index: at.min(n), blocks: vec![c, hold] })?; ids
        } else {
            let index = self.doc.index_of(target).unwrap();
            let mut cols = bare(Kind::Columns, None); let mut left = bare(Kind::Column { width: 0.5 }, Some(cols.id)); let mut right = bare(Kind::Column { width: 0.5 }, Some(cols.id));
            let (lh, rh) = (paragraph(Some(left.id), ""), paragraph(Some(right.id), "")); cols.children = vec![left.id, right.id]; left.children.push(lh.id); right.children.push(rh.id);
            // `at` 0: the run on the left, the block it was dropped beside on the right
            let (mine, theirs) = if at == 0 { ((left.id, lh.id), (right.id, rh.id)) } else { ((right.id, rh.id), (left.id, lh.id)) };
            b.push(Change::Insert { index: index + 1, blocks: vec![cols, left, lh, right, rh] })?;
            b.push(Change::Move { id: target, from: (None, index), to: (Some(theirs.0), 0) })?; let c = Change::removal(&b.scratch, theirs.1)?; b.push(c)?;
            mine
        };
        for (k, id) in run.iter().enumerate() { let from = b.scratch.index_of(*id).unwrap(); b.push(Change::Move { id: *id, from: (parent, from), to: (Some(col), k) })?; }
        let c = Change::removal(&b.scratch, hold)?; b.push(c)?;
        if let Some(p) = parent { if b.scratch.get(p).is_some() && b.scratch.children(Some(p)).is_empty() { self.emptied(&mut b, p)?; } }
        Ok(Some(b))
    }
    /// Where the run would land BESIDE a block, with the upright line that shows it: at the left or the right end of
    /// a top-level block (the pointer level with it, not at its very top or bottom), or at an edge of a column.
    pub fn beside_at(&self, layout: &NoteLayout, x: f64, y: f64) -> Option<(Drop, Rect)> {
        self.run()?; let s = layout.scale as f64;
        let (id, r) = self.doc.children(None).iter().filter_map(|id| layout.rect(&self.doc, *id).map(|r| (*id, r))).find(|(_, r)| { let e = (6.0 * s).min(r.height() * 0.25); y > r.y0 + e && y < r.y1 - e })?;
        let (at, lx) = if self.doc.get(id)?.kind == Kind::Columns {
            if layout.is_stacked(id) { return None } let near = 10.0 * s;
            let rects: Vec<Rect> = self.doc.children(Some(id)).iter().filter_map(|c| layout.rect(&self.doc, *c)).collect(); let n = rects.len(); if n == 0 { return None }
            let gap = if n > 1 { rects[1].x0 - rects[0].x1 } else { 8.0 * s };
            match rects.iter().position(|q| (x - q.x0).abs() <= near) { Some(k) => (k, rects[k].x0 - gap / 2.0), None if (x - rects[n - 1].x1).abs() <= near => (n, rects[n - 1].x1 + gap / 2.0), None => return None }
        } else if x < r.x0 + 20.0 * s { (0, r.x0 - 4.0 * s) } else if x > r.x1 - (r.width() * 0.12).max(40.0 * s) { (1, r.x1 + 4.0 * s) } else { return None };
        let d = Drop { parent: None, index: at, depth: None, beside: Some((id, at)) };
        if !matches!(self.plan_beside(id, at), Ok(Some(_))) { return None }
        Some((d, Rect::new(lx - s, r.y0, lx + s, r.y1)))
    }
    /// The way out of a table or of columns that are the note's first or last block (the user 2026-11-12): the caret
    /// could go no further up / down, so a paragraph is made before / after the container and the caret goes into it.
    /// Nothing when the caret is not in such a container, or there is a block to go to.
    pub fn exit_container(&mut self, down: bool, now_ms: u64) -> Result<Dirty, ChangeError> {
        let Some(s) = self.sel.filter(|s| s.is_caret()) else { return Ok(Dirty::Nothing) }; if self.blocks.is_some() || self.preedit.is_some() { return Ok(Dirty::Nothing) }
        let mut top = s.focus.block; while let Some(p) = self.doc.get(top).and_then(|b| b.parent) { top = p; }
        if top == s.focus.block || !self.doc.get(top).is_some_and(|b| matches!(b.kind, Kind::Table { .. } | Kind::Columns)) { return Ok(Dirty::Nothing) }
        let sibs = self.doc.children(None); let Some(i) = sibs.iter().position(|x| *x == top) else { return Ok(Dirty::Nothing) };
        if (down && i + 1 != sibs.len()) || (!down && i != 0) { return Ok(Dirty::Nothing) }
        let para = paragraph(None, ""); let id = para.id; let mut b = self.batch(); b.push(Change::Insert { index: if down { i + 1 } else { 0 }, blocks: vec![para] })?;
        self.commit(b, Some(Pos { block: id, at: 0 }), now_ms + mn_doc::change::JOIN_MS + 1)
    }
    /// Backspace in the one empty paragraph a column holds: the column goes (the caret to the end of the column before
    /// it, or the start of the one after). None: not that case.
    pub(crate) fn column_backspace(&mut self, now_ms: u64) -> Result<Option<Dirty>, ChangeError> {
        let Some(p) = self.sel.map(|s| s.focus) else { return Ok(None) }; let Some(blk) = self.doc.get(p.block) else { return Ok(None) };
        if blk.kind != Kind::Paragraph || !blk.text.is_empty() || !blk.children.is_empty() { return Ok(None) }
        let Some(col) = blk.parent.filter(|c| self.doc.get(*c).is_some_and(|x| matches!(x.kind, Kind::Column { .. })) && self.doc.children(Some(*c)).len() == 1) else { return Ok(None) };
        let Some(cols) = self.doc.get(col).and_then(|c| c.parent) else { return Ok(None) }; let all = self.doc.children(Some(cols)).to_vec(); let k = all.iter().position(|c| *c == col).unwrap_or(0);
        let leaf = |me: &Editor, c: BlockId, last: bool| -> Option<Pos> { let mut id = if last { *me.doc.children(Some(c)).last()? } else { *me.doc.children(Some(c)).first()? };
            while let Some(n) = me.doc.get(id).and_then(|b| if last { b.children.last() } else { b.children.first() }.copied()) { id = n; } Some(Pos { block: id, at: if last { me.doc.get(id).map(end_of).unwrap_or(0) } else { 0 } }) };
        let caret = k.checked_sub(1).and_then(|q| leaf(self, all[q], true)).or_else(|| all.get(k + 1).and_then(|c| leaf(self, *c, false)));
        let Some(caret) = caret else { return Ok(None) };
        let mut b = self.batch(); let c = Change::removal(&b.scratch, p.block)?; b.push(c)?; self.tidy_columns(&mut b, cols)?;
        Ok(Some(self.commit(b, Some(caret), now_ms + mn_doc::change::JOIN_MS + 1)?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Selection;
    use mn_doc::Doc;
    #[test]
    fn blocks_dropped_beside_make_columns_and_emptied_columns_go() {
        let mut d = Doc::new(); let ids: Vec<BlockId> = ["a", "b", "c", "d"].iter().enumerate().map(|(i, t)| { let id = BlockId::new(); d.insert(None, i, id, Kind::Paragraph, *t).unwrap(); id }).collect();
        let mut e = Editor::new(d); let pick = |e: &mut Editor, id: BlockId| { e.set_selection(Some(Selection::caret(Pos { block: id, at: 0 }))); e.toggle_blocks(); };
        let shape = |e: &Editor| -> String { e.doc.children(None).iter().map(|t| { let b = e.doc.get(*t).unwrap(); if b.kind == Kind::Columns { format!("[{}]", e.doc.children(Some(*t)).iter().map(|c| e.doc.children(Some(*c)).iter().map(|x| e.doc.get(*x).unwrap().text.clone()).collect::<Vec<_>>().join("+")).collect::<Vec<_>>().join("|")) } else { b.text.clone() } }).collect::<Vec<_>>().join(" ") };
        let beside = |t: BlockId, at: usize| Drop { parent: None, index: at, depth: None, beside: Some((t, at)) };
        // d beside b, on its right: two columns in b's place
        pick(&mut e, ids[3]); e.drop_blocks(&beside(ids[1], 1), 1_000).unwrap(); assert_eq!(shape(&e), "a [b|d] c");
        // c beside the columns, at their start: a third column, the widths sharing the whole
        let cols = e.doc.children(None)[1]; pick(&mut e, ids[2]); e.drop_blocks(&beside(cols, 0), 9_000).unwrap(); assert_eq!(shape(&e), "a [c|b|d]");
        let w: f32 = e.doc.children(Some(cols)).iter().map(|c| match e.doc.get(*c).unwrap().kind { Kind::Column { width } => width, _ => 0.0 }).sum(); assert!((w - 1.0).abs() < 1e-4);
        // b dragged out to the top: its column goes; then d: one column is left and unwraps
        pick(&mut e, ids[1]); e.drop_blocks(&Drop { parent: None, index: 0, depth: None, beside: None }, 17_000).unwrap(); assert_eq!(shape(&e), "b a [c|d]");
        pick(&mut e, ids[3]); e.drop_blocks(&Drop { parent: None, index: 0, depth: None, beside: None }, 25_000).unwrap(); assert_eq!(shape(&e), "d b a c");
        e.undo().unwrap(); assert_eq!(shape(&e), "b a [c|d]");
        // Backspace in a column's one empty paragraph: the column goes
        e.set_selection(Some(Selection::caret(Pos { block: ids[3], at: 0 }))); e.delete(true, 33_000).unwrap(); assert_eq!(e.doc.get(ids[3]).unwrap().text, "");
        e.delete(false, 41_000).unwrap(); assert_eq!(shape(&e), "b a c"); assert_eq!(e.selection().unwrap().focus, Pos { block: ids[2], at: 1 });
        // the way out of a table that is the note's last (and then its first) block: a paragraph is made beyond it
        e.set_selection(Some(Selection::caret(Pos { block: ids[2], at: 1 }))); assert_eq!(e.exit_container(true, 49_000).unwrap(), Dirty::Nothing, "an ordinary block");
        e.insert("\n", 50_000).unwrap(); e.insert_table(2, 2, 57_000).unwrap(); let n = e.doc.children(None).len(); let last = *e.doc.children(None).last().unwrap();
        let c = Change::removal(&e.doc, last).unwrap(); let mut b = e.batch(); b.push(c).unwrap(); let keep = e.selection().map(|s| s.focus); e.commit(b, keep, 65_000).unwrap(); assert_eq!(e.doc.children(None).len(), n - 1);
        assert_ne!(e.exit_container(true, 73_000).unwrap(), Dirty::Nothing); assert_eq!(e.doc.children(None).len(), n); let f = e.selection().unwrap().focus; assert_eq!(Some(&f.block), e.doc.children(None).last()); assert_eq!(e.doc.get(f.block).unwrap().kind, Kind::Paragraph);
        assert_eq!(e.exit_container(true, 81_000).unwrap(), Dirty::Nothing);
    }
}
