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

    // -- structure (row 317, stage 2): each of these is one undo step and leaves the caret in a cell

    /// Where the caret is in a table: (the table, its row's place, its column's place).
    pub fn table_pos(&self) -> Option<(BlockId, usize, usize)> {
        let (table, row, cell) = self.cell_of(self.sel?.focus.block)?;
        Some((table, self.doc.children(Some(table)).iter().position(|r| *r == row)?, self.doc.children(Some(row)).iter().position(|c| *c == cell)?))
    }
    /// A table's header rows and its columns' settings, one per column there is (padded with defaults).
    pub fn table_spec(&self, table: BlockId) -> Option<(u8, Vec<ColumnSpec>)> {
        let Kind::Table { header_rows, columns } = &self.doc.get(table)?.kind else { return None };
        let n = self.doc.children(Some(table)).iter().map(|r| self.doc.children(Some(*r)).len()).max().unwrap_or(0).max(columns.len());
        let mut c = columns.clone(); c.resize(n, ColumnSpec::default()); Some((*header_rows, c))
    }
    fn first_in(&self, cell: BlockId) -> Option<Pos> { let mut id = *self.doc.children(Some(cell)).first()?; while let Some(n) = self.doc.get(id).and_then(|b| b.children.first().copied()) { id = n; } Some(Pos { block: id, at: 0 }) }
    /// The caret's blocks under fresh identities (a row or a cell with all it holds), its top under `parent`.
    fn copy_of(&self, id: BlockId, parent: Option<BlockId>) -> Result<Vec<Block>, ChangeError> {
        let Change::Remove { blocks, .. } = Change::removal(&self.doc, id)? else { return Ok(vec![]) };
        let ids: std::collections::HashMap<BlockId, BlockId> = blocks.iter().map(|x| (x.id, BlockId::new())).collect();
        let mut out: Vec<Block> = blocks.iter().map(|x| { let mut n = x.clone(); n.id = ids[&x.id]; n.parent = x.parent.and_then(|q| ids.get(&q).copied()); n.children = x.children.iter().map(|c| ids[c]).collect(); n.conflict = None; n }).collect();
        if let Some(top) = out.first_mut() { top.parent = parent; top.rank = Rank::first(); }
        Ok(out)
    }
    fn settle(&mut self, b: crate::Batch, caret: Option<Pos>, now_ms: u64) -> Result<Dirty, ChangeError> {
        let keep = self.sel; let d = self.commit(b, caret, now_ms + mn_doc::change::JOIN_MS + 1)?;
        if caret.is_none() { self.sel = keep.and_then(|s| self.live(s)); if let Some(m) = self.marks.last_mut() { m.1 = self.sel; } }
        Ok(d)
    }
    /// A row above or below the caret's row, or a column left or right of its column (`column`); `after` = below /
    /// right. The caret goes into the new row's or column's cell beside the one it was in.
    pub fn table_insert(&mut self, column: bool, after: bool, now_ms: u64) -> Result<Dirty, ChangeError> {
        let Some((table, r, c)) = self.table_pos() else { return Ok(Dirty::Nothing) }; let Some((head, mut cols)) = self.table_spec(table) else { return Ok(Dirty::Nothing) };
        let rows = self.doc.children(Some(table)).to_vec(); let mut b = self.batch(); let mut caret = None;
        if column {
            if cols.len() >= 40 { return Ok(Dirty::Nothing) }
            let at = c + after as usize;
            for (i, row) in rows.iter().enumerate() { let mut cell = bare(Kind::cell(), Some(*row)); let p = paragraph(Some(cell.id), ""); cell.children.push(p.id); if i == r { caret = Some(Pos { block: p.id, at: 0 }); }
                let n = self.doc.children(Some(*row)).len(); b.push(Change::Insert { index: at.min(n), blocks: vec![cell, p] })?; }
            let old = self.doc.get(table).unwrap().kind.clone(); cols.insert(at.min(cols.len()), ColumnSpec::default()); b.push(Change::Kind { id: table, old, new: Kind::Table { header_rows: head, columns: cols } })?;
        } else {
            let (tree, firsts) = row_tree(table, self.doc.children(Some(rows[r])).len().max(1)); caret = firsts.get(c).or(firsts.first()).map(|f| Pos { block: *f, at: 0 });
            b.push(Change::Insert { index: r + after as usize, blocks: tree })?;
            // a row put in above the header rows' last stays a body row: the header keeps its count unless it is put in among them
        }
        self.settle(b, caret, now_ms)
    }
    /// The caret's row or column goes. The last row and the last column stay (the table goes as a block). The caret
    /// lands in the cell that takes the place.
    pub fn table_remove(&mut self, column: bool, now_ms: u64) -> Result<Dirty, ChangeError> {
        let Some((table, r, c)) = self.table_pos() else { return Ok(Dirty::Nothing) }; let Some((head, mut cols)) = self.table_spec(table) else { return Ok(Dirty::Nothing) };
        let rows = self.doc.children(Some(table)).to_vec(); let mut b = self.batch();
        let caret_cell = if column {
            if cols.len() <= 1 { return Ok(Dirty::Nothing) }
            for row in &rows { if let Some(cell) = self.doc.children(Some(*row)).get(c) { let ch = Change::removal(&b.scratch, *cell)?; b.push(ch)?; } }
            let old = self.doc.get(table).unwrap().kind.clone(); cols.remove(c); b.push(Change::Kind { id: table, old, new: Kind::Table { header_rows: head, columns: cols } })?;
            let left = self.doc.children(Some(rows[r])); left.get(c + 1).or(c.checked_sub(1).and_then(|i| left.get(i))).copied()
        } else {
            if rows.len() <= 1 { return Ok(Dirty::Nothing) }
            let ch = Change::removal(&b.scratch, rows[r])?; b.push(ch)?;
            if (r as u8) < head { let old = self.doc.get(table).unwrap().kind.clone(); b.push(Change::Kind { id: table, old, new: Kind::Table { header_rows: head - 1, columns: cols } })?; }
            let to = rows.get(r + 1).or(r.checked_sub(1).and_then(|i| rows.get(i))).copied();
            to.and_then(|row| self.doc.children(Some(row)).get(c).or(self.doc.children(Some(row)).last()).copied())
        };
        let caret = caret_cell.and_then(|cell| self.first_in(cell));
        self.settle(b, caret, now_ms)
    }
    /// The caret's row moves up / down, or its column left / right (`forward` = down / right), the caret with it.
    pub fn table_move(&mut self, column: bool, forward: bool, now_ms: u64) -> Result<Dirty, ChangeError> {
        let Some((table, r, c)) = self.table_pos() else { return Ok(Dirty::Nothing) }; let Some((head, mut cols)) = self.table_spec(table) else { return Ok(Dirty::Nothing) };
        let rows = self.doc.children(Some(table)).to_vec(); let mut b = self.batch();
        if column {
            let to = if forward { c + 1 } else { match c.checked_sub(1) { Some(t) => t, None => return Ok(Dirty::Nothing) } }; if to >= cols.len() { return Ok(Dirty::Nothing) }
            for row in &rows { let cells = self.doc.children(Some(*row)); if let (Some(cell), true) = (cells.get(c), to < cells.len()) { b.push(Change::Move { id: *cell, from: (Some(*row), c), to: (Some(*row), to) })?; } }
            let old = self.doc.get(table).unwrap().kind.clone(); cols.swap(c, to); b.push(Change::Kind { id: table, old, new: Kind::Table { header_rows: head, columns: cols } })?;
        } else {
            let to = if forward { r + 1 } else { match r.checked_sub(1) { Some(t) => t, None => return Ok(Dirty::Nothing) } }; if to >= rows.len() { return Ok(Dirty::Nothing) }
            b.push(Change::Move { id: rows[r], from: (Some(table), r), to: (Some(table), to) })?;
        }
        self.settle(b, None, now_ms)
    }
    /// A copy of the caret's row below it, or of its column to its right; the caret goes into the copy.
    pub fn table_duplicate(&mut self, column: bool, now_ms: u64) -> Result<Dirty, ChangeError> {
        let Some((table, r, c)) = self.table_pos() else { return Ok(Dirty::Nothing) }; let Some((head, mut cols)) = self.table_spec(table) else { return Ok(Dirty::Nothing) };
        let rows = self.doc.children(Some(table)).to_vec(); let mut b = self.batch(); let mut caret = None;
        if column {
            if cols.len() >= 40 { return Ok(Dirty::Nothing) }
            for (i, row) in rows.iter().enumerate() { if let Some(cell) = self.doc.children(Some(*row)).get(c) { let tree = self.copy_of(*cell, Some(*row))?; if i == r { caret = tree.iter().find(|x| !x.kind.is_container()).map(|x| Pos { block: x.id, at: 0 }); } b.push(Change::Insert { index: c + 1, blocks: tree })?; } }
            let old = self.doc.get(table).unwrap().kind.clone(); let spec = cols[c].clone(); cols.insert(c + 1, spec); b.push(Change::Kind { id: table, old, new: Kind::Table { header_rows: head, columns: cols } })?;
        } else {
            let tree = self.copy_of(rows[r], Some(table))?;
            // the copy's cell under the caret's: the c-th cell of the copied row
            let cell = tree.first().and_then(|row| row.children.get(c).copied()); caret = cell.and_then(|cell| tree.iter().find(|x| x.parent == Some(cell) ).map(|x| Pos { block: x.id, at: 0 }));
            b.push(Change::Insert { index: r + 1, blocks: tree })?;
        }
        self.settle(b, caret, now_ms)
    }
    /// The table's first row is a header, or is not (a table may have none).
    pub fn table_header(&mut self, now_ms: u64) -> Result<Dirty, ChangeError> {
        let Some((table, _, _)) = self.table_pos() else { return Ok(Dirty::Nothing) }; let Some((head, cols)) = self.table_spec(table) else { return Ok(Dirty::Nothing) };
        let old = self.doc.get(table).unwrap().kind.clone(); let mut b = self.batch();
        b.push(Change::Kind { id: table, old, new: Kind::Table { header_rows: if head > 0 { 0 } else { 1 }, columns: cols } })?;
        self.settle(b, None, now_ms)
    }
    /// A setting of the caret's column changes (its alignment, its width back to automatic ...).
    pub fn table_column(&mut self, f: impl FnOnce(&mut ColumnSpec), now_ms: u64) -> Result<Dirty, ChangeError> {
        let Some((table, _, c)) = self.table_pos() else { return Ok(Dirty::Nothing) }; let Some((head, mut cols)) = self.table_spec(table) else { return Ok(Dirty::Nothing) };
        let Some(spec) = cols.get_mut(c) else { return Ok(Dirty::Nothing) }; f(spec);
        let old = self.doc.get(table).unwrap().kind.clone(); let new = Kind::Table { header_rows: head, columns: cols }; if old == new { return Ok(Dirty::Nothing) }
        let mut b = self.batch(); b.push(Change::Kind { id: table, old, new })?;
        self.settle(b, None, now_ms)
    }
}
