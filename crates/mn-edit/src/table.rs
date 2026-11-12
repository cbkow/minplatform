//! Tables and columns made and walked in place (canon row 317, stage 1 of the step; `docs/phase3/step-tables-columns-
//! spec.md`). A table is rows of cells, a Columns block is columns; a cell or a column holds ordinary blocks, so typing,
//! formatting, pasting and Up / Down (which go by what is seen) need nothing of their own. Here: making the two, Tab
//! from cell to cell, and a row added below.
use mn_doc::{Block, BlockId, Change, ChangeError, ColumnSpec, Kind, Rank, Spans};
use crate::{end_of, paragraph, Dirty, Editor, Pos};

fn bare(kind: Kind, parent: Option<BlockId>) -> Block { Block { id: BlockId::new(), parent, rank: Rank::first(), kind, text: String::new(), spans: Spans::new(), conflict: None, children: vec![] } }
/// A row of `n` cells, each with an empty paragraph, as an insert takes it (the row first); and its cells' paragraphs.
fn row_tree(table: BlockId, n: usize) -> (Vec<Block>, Vec<BlockId>) {
    let mut row = bare(Kind::row(), Some(table)); let (mut rest, mut firsts) = (vec![], vec![]);
    for _ in 0..n { let mut cell = bare(Kind::cell(), Some(row.id)); let p = paragraph(Some(cell.id), ""); row.children.push(cell.id); cell.children.push(p.id); firsts.push(p.id); rest.push(cell); rest.push(p); }
    let mut tree = vec![row]; tree.extend(rest); (tree, firsts)
}

impl Editor {
    /// The cell a block is in: (the table, the row, the cell). None: it is not in a table.
    pub fn cell_of(&self, block: BlockId) -> Option<(BlockId, BlockId, BlockId)> {
        let mut up = self.doc.get(block)?.parent;
        while let Some(id) = up { let b = self.doc.get(id)?; if matches!(b.kind, Kind::Cell { .. }) { let row = b.parent?; return Some((self.doc.get(row)?.parent?, row, id)) } up = b.parent; }
        None
    }
    /// A table of `rows` x `cols` (its first row a header) goes in after the caret's block - in its place when that is
    /// an empty paragraph - and the caret into its first cell. Not inside a cell (a cell holds no table): Nothing.
    pub fn insert_table(&mut self, rows: usize, cols: usize, now_ms: u64) -> Result<Dirty, ChangeError> {
        if self.preedit.is_some() { return Ok(Dirty::Nothing) }
        let Some(p) = self.sel.map(|s| s.focus) else { return Ok(Dirty::Nothing) }; let Some(blk) = self.doc.get(p.block).cloned() else { return Ok(Dirty::Nothing) };
        if self.cell_of(p.block).is_some() { return Ok(Dirty::Nothing) }
        let (rows, cols) = (rows.clamp(1, 50), cols.clamp(1, 20));
        let mut table = bare(Kind::Table { header_rows: if rows > 1 { 1 } else { 0 }, columns: vec![ColumnSpec::default(); cols] }, blk.parent); let (mut rest, mut first) = (vec![], None);
        for _ in 0..rows { let (tree, firsts) = row_tree(table.id, cols); table.children.push(tree[0].id); first = first.or(firsts.first().copied()); rest.extend(tree); }
        let mut tree = vec![table]; tree.extend(rest);
        let index = self.doc.index_of(p.block).unwrap() + 1; let mut b = self.batch();
        b.push(Change::Insert { index, blocks: tree })?;
        // a paragraph follows a table at a list's end, so that the caret can get below it
        if self.doc.children(blk.parent).get(index).is_none() { b.push(Change::Insert { index: index + 1, blocks: vec![paragraph(blk.parent, "")] })?; }
        if blk.kind == Kind::Paragraph && blk.text.is_empty() { let c = Change::removal(&b.scratch, blk.id)?; b.push(c)?; }
        self.commit(b, first.map(|f| Pos { block: f, at: 0 }), now_ms + mn_doc::change::JOIN_MS + 1)
    }
    /// The caret's block gets a column beside it: it becomes the first of two columns and the caret goes into the
    /// second, an empty paragraph. In a column already, another column is added after it. In a table's cell: Nothing
    /// (a cell holds no columns).
    pub fn insert_columns(&mut self, now_ms: u64) -> Result<Dirty, ChangeError> {
        if self.preedit.is_some() { return Ok(Dirty::Nothing) }
        let Some(p) = self.sel.map(|s| s.focus) else { return Ok(Dirty::Nothing) }; let Some(blk) = self.doc.get(p.block).cloned() else { return Ok(Dirty::Nothing) };
        if self.cell_of(p.block).is_some() { return Ok(Dirty::Nothing) }
        let mut b = self.batch();
        if let Some(col) = blk.parent.filter(|c| self.doc.get(*c).is_some_and(|x| matches!(x.kind, Kind::Column { .. }))) {
            // one more column: every column gets the same share
            let cols = self.doc.get(col).and_then(|c| c.parent); let sibs = self.doc.children(cols).to_vec(); if sibs.len() >= 6 { return Ok(Dirty::Nothing) }
            let share = 1.0 / (sibs.len() + 1) as f32;
            for s in &sibs { let old = self.doc.get(*s).unwrap().kind.clone(); let new = Kind::Column { width: share }; if old != new { b.push(Change::Kind { id: *s, old, new })?; } }
            let mut c = bare(Kind::Column { width: share }, cols); let para = paragraph(Some(c.id), ""); c.children.push(para.id); let caret = para.id;
            b.push(Change::Insert { index: sibs.iter().position(|x| *x == col).unwrap() + 1, blocks: vec![c, para] })?;
            return self.commit(b, Some(Pos { block: caret, at: 0 }), now_ms + mn_doc::change::JOIN_MS + 1)
        }
        if blk.parent.is_some() { return Ok(Dirty::Nothing) }
        let index = self.doc.index_of(p.block).unwrap();
        let mut cols = bare(Kind::Columns, None); let mut left = bare(Kind::Column { width: 0.5 }, Some(cols.id)); let mut right = bare(Kind::Column { width: 0.5 }, Some(cols.id));
        let (hold, para) = (paragraph(Some(left.id), ""), paragraph(Some(right.id), "")); let (hold_id, left_id, caret) = (hold.id, left.id, para.id);
        cols.children = vec![left.id, right.id]; left.children.push(hold.id); right.children.push(para.id);
        b.push(Change::Insert { index: index + 1, blocks: vec![cols, left, hold, right, para] })?;
        // the caret's block moves into the first column, in the place of the paragraph that held it open
        b.push(Change::Move { id: p.block, from: (None, index), to: (Some(left_id), 0) })?;
        let c = Change::removal(&b.scratch, hold_id)?; b.push(c)?;
        self.commit(b, Some(Pos { block: caret, at: 0 }), now_ms + mn_doc::change::JOIN_MS + 1)
    }
    /// Tab in a table: the caret goes to the next cell (Shift: the one before), at the end of what it holds; past the
    /// last cell a row is added and the caret goes into its first cell. None: the caret is not in a table (Tab is the
    /// list's then).
    pub fn tab_cell(&mut self, back: bool, now_ms: u64) -> Result<Option<Dirty>, ChangeError> {
        if self.preedit.is_some() { return Ok(None) }
        let Some(p) = self.sel.map(|s| s.focus) else { return Ok(None) }; let Some((table, _, cell)) = self.cell_of(p.block) else { return Ok(None) };
        let cells: Vec<BlockId> = self.doc.children(Some(table)).iter().flat_map(|r| self.doc.children(Some(*r)).to_vec()).collect();
        let Some(i) = cells.iter().position(|c| *c == cell) else { return Ok(None) };
        let end = |me: &Editor, c: BlockId| -> Option<Pos> { let mut id = *me.doc.children(Some(c)).last()?; while let Some(n) = me.doc.get(id).and_then(|b| b.children.last().copied()) { id = n; } Some(Pos { block: id, at: me.doc.get(id).map(end_of).unwrap_or(0) }) };
        let to = if back { i.checked_sub(1).map(|j| cells[j]) } else { cells.get(i + 1).copied() };
        self.blocks = None; self.goal_x = None; self.upstream = false; self.armed.clear();
        match to {
            Some(c) => { if let Some(pos) = end(self, c) { self.sel = Some(crate::Selection::caret(pos)); } Ok(Some(Dirty::Nothing)) }
            None if back => Ok(Some(Dirty::Nothing)),
            None => self.add_row_below(now_ms).map(Some),
        }
    }
    /// A row is added below the caret's row, with as many cells; the caret goes into the cell under the one it was in
    /// (Cmd+Enter; and Tab past the last cell - there the new row's first cell). Not in a table: Nothing.
    pub fn add_row_below(&mut self, now_ms: u64) -> Result<Dirty, ChangeError> {
        let Some(p) = self.sel.map(|s| s.focus) else { return Ok(Dirty::Nothing) }; let Some((table, row, cell)) = self.cell_of(p.block) else { return Ok(Dirty::Nothing) };
        let rows = self.doc.children(Some(table)).to_vec(); let at = rows.iter().position(|r| *r == row).unwrap_or(rows.len().saturating_sub(1));
        let cells = self.doc.children(Some(row)).to_vec(); let col = cells.iter().position(|c| *c == cell).unwrap_or(0);
        let last_cell = at + 1 == rows.len() && col + 1 == cells.len();
        let (tree, firsts) = row_tree(table, cells.len().max(1));
        let mut b = self.batch(); b.push(Change::Insert { index: at + 1, blocks: tree })?;
        let caret = firsts.get(if last_cell { 0 } else { col }).or(firsts.first()).copied();
        self.commit(b, caret.map(|c| Pos { block: c, at: 0 }), now_ms + mn_doc::change::JOIN_MS + 1)
    }
}
