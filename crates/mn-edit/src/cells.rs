//! Cells of a table selected and carried (canon row 319; row 317 stage 4). A cell rectangle is DERIVED: a selection whose
//! ends stand in different cells of one table is the rectangle between them, so a drag from cell to cell and Shift +
//! arrow past a cell's edge make one with nothing new to keep. Whole rows / columns (a handle's click) are the same
//! selection with a mark. Delete empties cells, or removes whole rows / columns; cells go out as a small table (and as
//! the text a spreadsheet reads) and come in from the caret's cell rightwards and downwards - rows are added, columns
//! never.
use mn_doc::{Block, BlockId, Change, ChangeError, ChoiceOption, ColumnKind, ColumnSpec, Doc, Kind, Rank, Spans};
use crate::{end_of, paragraph, table::{bare, row_tree}, typed::{self, column_of}, Clip, Dirty, Editor, Pos, Selection};

/// Rows `r0..=r1` and columns `c0..=c1` of a table.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CellRect { pub table: BlockId, pub r0: usize, pub r1: usize, pub c0: usize, pub c1: usize }
/// What a cell takes when cells are put in: a line or more of plain text, or blocks (a cell's, as an insert takes them).
pub enum Source { Text(String), Blocks(Vec<Block>) }

/// Text as a spreadsheet writes it: fields by tabs, rows by lines, a field with a tab, a line break or a quote in quotes.
pub fn parse_tsv(text: &str) -> Vec<Vec<String>> {
    let (mut rows, mut row, mut f, mut quoted) = (vec![], vec![], String::new(), false); let mut it = text.chars().peekable();
    while let Some(c) = it.next() {
        if quoted { if c == '"' { if it.peek() == Some(&'"') { it.next(); f.push('"'); } else { quoted = false; } } else { f.push(c); } continue }
        match c { '"' if f.is_empty() => quoted = true, '\t' => row.push(std::mem::take(&mut f)), '\r' => {}, '\n' => { row.push(std::mem::take(&mut f)); rows.push(std::mem::take(&mut row)); } c => f.push(c) }
    }
    if !f.is_empty() || !row.is_empty() { row.push(f); rows.push(row); }
    rows
}
fn tsv_field(s: &str) -> String { if s.contains(['\t', '\n', '"']) { format!("\"{}\"", s.replace('"', "\"\"")) } else { s.to_string() } }
/// A cell's text, a line for each of its text blocks.
fn lines(doc: &Doc, cell: BlockId) -> String {
    fn walk(doc: &Doc, id: BlockId, out: &mut Vec<String>) { let Some(b) = doc.get(id) else { return }; if b.kind.is_text() { out.push(b.text.clone()); } for c in &b.children { walk(doc, *c, out); } }
    let mut out = vec![]; for c in doc.children(Some(cell)) { walk(doc, *c, &mut out); } out.join("\n")
}
/// Blocks (trees, each top first) under fresh identities, their tops under `parent`.
fn fresh(blocks: &[Block], parent: BlockId) -> Vec<Block> {
    let ids: std::collections::HashMap<BlockId, BlockId> = blocks.iter().map(|x| (x.id, BlockId::new())).collect();
    blocks.iter().map(|x| { let mut n = x.clone(); n.id = ids[&x.id]; n.parent = Some(x.parent.and_then(|q| ids.get(&q).copied()).unwrap_or(parent)); n.children = x.children.iter().filter_map(|c| ids.get(c).copied()).collect(); n.conflict = None; n }).collect()
}
fn spec_of(doc: &Doc, table: BlockId) -> (u8, Vec<ColumnSpec>) {
    let Some(Kind::Table { header_rows, columns }) = doc.get(table).map(|t| &t.kind) else { return (0, vec![]) };
    let n = doc.children(Some(table)).iter().map(|r| doc.children(Some(*r)).len()).max().unwrap_or(0).max(columns.len());
    let mut c = columns.clone(); c.resize(n, ColumnSpec::default()); (*header_rows, c)
}

impl Editor {
    /// The cells selected: the selection's ends stand in different cells of one table (or whole rows / columns are marked).
    pub fn cell_rect(&self) -> Option<CellRect> {
        if self.blocks.is_some() { return None } let s = self.sel?;
        let (ta, ra, ca, cell_a, ..) = column_of(&self.doc, s.anchor.block)?; let (tb, rb, cb, cell_b, ..) = column_of(&self.doc, s.focus.block)?;
        if ta != tb || (cell_a == cell_b && self.cells_whole().is_none()) { return None }
        Some(CellRect { table: ta, r0: ra.min(rb), r1: ra.max(rb), c0: ca.min(cb), c1: ca.max(cb) })
    }
    /// Whole rows (false) or whole columns (true) are selected.
    pub fn cells_whole(&self) -> Option<bool> { self.whole.filter(|(s, _)| Some(*s) == self.sel).map(|x| x.1) }
    pub(crate) fn cells_in(&self, r: &CellRect) -> Vec<BlockId> {
        self.doc.children(Some(r.table)).iter().enumerate().filter(|(i, _)| *i >= r.r0 && *i <= r.r1).flat_map(|(_, row)| self.doc.children(Some(*row)).iter().enumerate().filter(|(j, _)| *j >= r.c0 && *j <= r.c1).map(|(_, c)| *c).collect::<Vec<_>>()).collect()
    }
    fn last_in(&self, cell: BlockId) -> Option<Pos> { let mut id = *self.doc.children(Some(cell)).last()?; while let Some(n) = self.doc.get(id).and_then(|b| b.children.last().copied()) { id = n; } Some(Pos { block: id, at: self.doc.get(id).map(end_of).unwrap_or(0) }) }
    fn cell_at(&self, table: BlockId, r: usize, c: usize) -> Option<BlockId> { self.doc.children(Some(*self.doc.children(Some(table)).get(r)?)).get(c).copied() }
    /// The cells from (r_a, c_a) to (r_b, c_b) of a table are selected.
    pub fn select_cells(&mut self, table: BlockId, a: (usize, usize), b: (usize, usize)) -> bool {
        let (Some(ca), Some(cb)) = (self.cell_at(table, a.0, a.1), self.cell_at(table, b.0, b.1)) else { return false };
        let (Some(pa), Some(pb)) = (self.first_in(ca), self.last_in(cb)) else { return false };
        self.blocks = None; self.goal_x = None; self.sel = Some(Selection { anchor: pa, focus: pb }); true
    }
    /// The caret's whole row (or column) is selected; `extend`: from the rows (columns) selected already to this one.
    pub fn select_line(&mut self, column: bool, extend: bool) -> bool {
        let Some((table, r, c)) = self.table_pos() else { return false };
        let rows = self.doc.children(Some(table)).len(); let cols = self.doc.children(Some(*self.doc.children(Some(table)).first().unwrap())).len(); if rows == 0 || cols == 0 { return false }
        let from = if extend && self.cells_whole() == Some(column) { self.sel.and_then(|s| column_of(&self.doc, s.anchor.block)).map(|x| if column { x.2 } else { x.1 }) } else { None }.unwrap_or(if column { c } else { r });
        let ok = if column { self.select_cells(table, (0, from), (rows - 1, c)) } else { self.select_cells(table, (from, 0), (r, cols - 1)) };
        if ok { self.whole = self.sel.map(|s| (s, column)); } ok
    }
    /// The selected cells are emptied.
    pub fn cells_clear(&mut self, now_ms: u64) -> Result<Dirty, ChangeError> {
        let Some(r) = self.cell_rect() else { return Ok(Dirty::Nothing) }; let mut b = self.batch(); let mut caret = None;
        for cell in self.cells_in(&r) { let p = self.write_cell(&mut b, cell, "", Spans::new())?; caret = caret.or(Some(p)); }
        self.finish(b, caret, now_ms)
    }
    /// Delete on selected cells: whole rows or columns are removed (all of them: the table goes), other cells are emptied.
    pub fn cells_delete(&mut self, now_ms: u64) -> Result<Dirty, ChangeError> {
        let Some(r) = self.cell_rect() else { return Ok(Dirty::Nothing) }; let Some(column) = self.cells_whole() else { return self.cells_clear(now_ms) };
        let rows = self.doc.children(Some(r.table)).to_vec(); let (head, mut cols) = spec_of(&self.doc, r.table); let mut b = self.batch();
        let all = if column { r.c0 == 0 && r.c1 + 1 >= cols.len() } else { r.r0 == 0 && r.r1 + 1 >= rows.len() };
        if all { // the table goes; a paragraph stands in its place when nothing else would
            let t = self.doc.get(r.table).unwrap(); let (parent, index) = (t.parent, self.doc.index_of(r.table).unwrap());
            let para = paragraph(parent, ""); let id = para.id; b.push(Change::Insert { index, blocks: vec![para] })?; let c = Change::removal(&b.scratch, r.table)?; b.push(c)?;
            return self.finish(b, Some(Pos { block: id, at: 0 }), now_ms) }
        let caret_cell = if column {
            for row in &rows { for c in (r.c0..=r.c1).rev() { if let Some(cell) = self.doc.children(Some(*row)).get(c) { let ch = Change::removal(&b.scratch, *cell)?; b.push(ch)?; } } }
            let old = b.scratch.get(r.table).unwrap().kind.clone(); cols.drain(r.c0..=r.c1.min(cols.len() - 1)); b.push(Change::Kind { id: r.table, old, new: Kind::Table { header_rows: head, columns: cols } })?;
            let left = b.scratch.children(Some(rows[r.r0.min(rows.len() - 1)])); left.get(r.c0).or(left.last()).copied()
        } else {
            for i in (r.r0..=r.r1).rev() { let ch = Change::removal(&b.scratch, rows[i])?; b.push(ch)?; }
            let gone = (r.r0..=r.r1).filter(|i| (*i as u8) < head).count() as u8;
            if gone > 0 { let old = b.scratch.get(r.table).unwrap().kind.clone(); b.push(Change::Kind { id: r.table, old, new: Kind::Table { header_rows: head - gone, columns: cols } })?; }
            let left = b.scratch.children(Some(r.table)); left.get(r.r0).or(left.last()).and_then(|row| b.scratch.children(Some(*row)).first().copied())
        };
        let caret = caret_cell.and_then(|cell| { let mut id = *b.scratch.children(Some(cell)).first()?; while let Some(n) = b.scratch.get(id).and_then(|x| x.children.first().copied()) { id = n; } Some(Pos { block: id, at: 0 }) });
        self.finish(b, caret, now_ms)
    }
    /// The selected cells as a spreadsheet reads them.
    pub fn cells_text(&self) -> String {
        let Some(r) = self.cell_rect() else { return String::new() };
        (r.r0..=r.r1).map(|i| (r.c0..=r.c1).map(|j| self.cell_at(r.table, i, j).map(|c| tsv_field(&lines(&self.doc, c))).unwrap_or_default()).collect::<Vec<_>>().join("\t")).collect::<Vec<_>>().join("\n")
    }
    /// The selected cells as a small table of their own (fresh identities), for the clipboard.
    pub fn copy_cells(&self) -> Option<Clip> {
        let r = self.cell_rect()?; let (head, cols) = spec_of(&self.doc, r.table);
        let mut table = bare(Kind::Table { header_rows: (r.r0..=r.r1).filter(|i| (*i as u8) < head).count() as u8, columns: (r.c0..=r.c1).map(|c| cols.get(c).cloned().unwrap_or_default()).collect() }, None); let mut rest = vec![];
        for i in r.r0..=r.r1 { let mut row = bare(Kind::row(), Some(table.id)); if let Some(src) = self.doc.children(Some(r.table)).get(i).and_then(|x| self.doc.get(*x)) { row.kind = src.kind.clone(); } let mut cells = vec![];
            for j in r.c0..=r.c1 { let Some(cell) = self.cell_at(r.table, i, j) else { continue }; let Ok(copy) = self.copy_of(cell, Some(row.id)) else { continue }; if let Some(top) = copy.first() { row.children.push(top.id); } cells.extend(copy); }
            table.children.push(row.id); rest.push(row); rest.extend(cells); }
        let mut tree = vec![table]; tree.extend(rest); Some(vec![tree])
    }
    /// A clip that is ONE table, with the caret in a table: its cells go in from the caret's cell (None: not that case).
    pub fn paste_table_clip(&mut self, clip: &Clip, now_ms: u64) -> Option<Result<(Dirty, usize), ChangeError>> {
        let [tree] = &clip[..] else { return None }; let top = tree.first()?; if !matches!(top.kind, Kind::Table { .. }) || self.blocks.is_some() || self.table_pos().is_none() { return None }
        let by: std::collections::HashMap<BlockId, &Block> = tree.iter().map(|b| (b.id, b)).collect();
        let under = |cell: BlockId| -> Vec<Block> { tree.iter().filter(|b| { let mut up = b.parent; while let Some(p) = up { if p == cell { return true } up = by.get(&p).and_then(|x| x.parent); } false }).cloned().collect() };
        let grid: Vec<Vec<Source>> = top.children.iter().filter_map(|r| by.get(r)).map(|row| row.children.iter().map(|c| Source::Blocks(under(*c))).collect()).collect();
        Some(self.paste_sources(grid, now_ms))
    }
    /// Text with tabs: in a table its fields fill cells from the caret's; elsewhere it becomes a table of its own.
    pub fn paste_grid_text(&mut self, text: &str, now_ms: u64) -> Result<(Dirty, usize), ChangeError> {
        let grid = parse_tsv(text); if grid.is_empty() { return Ok((Dirty::Nothing, 0)) }
        if self.table_pos().is_none() { let cols = grid.iter().map(|r| r.len()).max().unwrap_or(1); if self.insert_table(grid.len(), cols, now_ms)? == Dirty::Nothing { return Ok((Dirty::Nothing, 0)) } }
        self.paste_sources(grid.into_iter().map(|r| r.into_iter().map(Source::Text).collect()).collect(), now_ms + 1)
    }
    /// Cells are filled from the selected cells' first (or the caret's cell), rightwards and downwards: rows are added
    /// as needed, columns never - what would go past the last column is left out (the second value: how many columns).
    /// A typed column's cell takes the value of the text; a text column's cell the blocks (or a paragraph a line).
    pub fn paste_sources(&mut self, grid: Vec<Vec<Source>>, now_ms: u64) -> Result<(Dirty, usize), ChangeError> {
        let Some((table, r0, c0)) = self.cell_rect().map(|r| (r.table, r.r0, r.c0)).or(self.table_pos()) else { return Ok((Dirty::Nothing, 0)) };
        let (head, mut cols) = spec_of(&self.doc, table); let m = cols.len(); let mut b = self.batch(); let (mut left_out, mut first, mut grown) = (0usize, None, std::collections::BTreeSet::new());
        for (i, row) in grid.into_iter().enumerate() {
            let n = b.scratch.children(Some(table)).len(); if r0 + i >= n { let (tree, _) = row_tree(table, m.max(1)); b.push(Change::Insert { index: n, blocks: tree })?; }
            let rid = b.scratch.children(Some(table))[r0 + i]; let len = row.len();
            for (j, src) in row.into_iter().enumerate() {
                if c0 + j >= m { left_out = left_out.max(len - j); break }
                let Some(cell) = b.scratch.children(Some(rid)).get(c0 + j).copied() else { continue };
                let text = |doc_cell: &Source| match doc_cell { Source::Text(t) => t.split_whitespace().collect::<Vec<_>>().join(" "), Source::Blocks(v) => v.iter().filter(|x| x.kind.is_text() && !x.text.trim().is_empty()).map(|x| x.text.split_whitespace().collect::<Vec<_>>().join(" ")).collect::<Vec<_>>().join(" ") };
                let body = r0 + i >= head as usize;
                let at = match (&mut cols[c0 + j].kind, body) {
                    (ColumnKind::Choice { options }, true) => { let v = typed::clean_label(&text(&src));
                        let id = if v.is_empty() { None } else { Some(match options.iter().find(|o| o.label.trim().to_lowercase() == v.to_lowercase()) { Some(o) => o.id.clone(),
                            None => { let id = BlockId::new().to_string(); options.push(ChoiceOption { id: id.clone(), label: v, color: typed::PALETTE[options.len() % typed::PALETTE.len()].to_string() }); grown.insert(c0 + j); id } }) };
                        let (t, sp) = id.map(|id| typed::chip(options, &id)).unwrap_or_default(); self.write_cell(&mut b, cell, &t, sp)? }
                    (ColumnKind::Check, true) => { let st = typed::check_state(&text(&src)); let (t, sp) = if st == "0" { Default::default() } else { typed::chip(&typed::check_options(), st) }; self.write_cell(&mut b, cell, &t, sp)? }
                    (ColumnKind::Timecode { fps }, true) => { let v = text(&src); let t = typed::normalise_timecode(&v, *fps).unwrap_or(v); self.write_cell(&mut b, cell, &t, Spans::new())? }
                    _ => { let old = b.scratch.children(Some(cell)).to_vec();
                        let new: Vec<Block> = match src { Source::Text(t) => t.split('\n').map(|l| paragraph(Some(cell), l)).collect(), Source::Blocks(v) if !v.is_empty() => fresh(&v, cell), Source::Blocks(_) => vec![paragraph(Some(cell), "")] };
                        let mut new = new; for x in new.iter_mut().filter(|x| x.parent == Some(cell)) { x.rank = Rank::first(); } let top = new.first().map(|x| x.id);
                        b.push(Change::Insert { index: 0, blocks: new })?; for k in old { let c = Change::removal(&b.scratch, k)?; b.push(c)?; }
                        Pos { block: top.unwrap_or(cell), at: 0 } } };
                first = first.or(Some(at));
            }
        }
        // a choice column that gained options: its list is written, and every chip of the column carries the new list
        if !grown.is_empty() { let old = b.scratch.get(table).unwrap().kind.clone(); b.push(Change::Kind { id: table, old, new: Kind::Table { header_rows: head, columns: cols.clone() } })?;
            for c in grown { let ColumnKind::Choice { options } = &cols[c].kind else { continue };
                for row in b.scratch.children(Some(table)).iter().skip(head as usize).copied().collect::<Vec<_>>() { let Some(cell) = b.scratch.children(Some(row)).get(c).copied() else { continue };
                    let v = b.scratch.children(Some(cell)).first().and_then(|p| b.scratch.get(*p)).and_then(|p| typed::chosen(&p.text, &p.spans)); if let Some(v) = v { let (t, sp) = typed::chip(options, &v); self.write_cell(&mut b, cell, &t, sp)?; } } } }
        // the caret goes into the first cell filled (a block that is not text: its start)
        let caret = first.map(|p| { let mut id = p.block; while let Some(n) = b.scratch.get(id).filter(|x| !x.kind.is_text()).and_then(|x| x.children.first().copied()) { id = n; } Pos { block: id, at: 0 } });
        Ok((self.finish(b, caret, now_ms)?, left_out))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn spreadsheet_text_is_read_with_its_quotes() {
        assert_eq!(parse_tsv("a\tb\nc\td\n"), vec![vec!["a", "b"], vec!["c", "d"]]);
        assert_eq!(parse_tsv("\"two\nlines\"\t\"a \"\"q\"\"\"\r\nx\t"), vec![vec!["two\nlines", "a \"q\""], vec!["x", ""]]);
        assert_eq!(tsv_field("a\tb"), "\"a\tb\"");
    }
    #[test]
    fn cells_are_selected_emptied_removed_copied_and_filled() {
        let mut e = Editor::new({ let mut d = Doc::new(); d.insert(None, 0, BlockId::new(), Kind::Paragraph, "").unwrap(); d });
        let first = e.doc.children(None)[0]; e.set_selection(Some(Selection::caret(Pos { block: first, at: 0 })));
        let mut t = 10_000u64; let mut now = || { t += 5_000; t };
        e.paste_grid_text("Shot\tStatus\tNote\n010\tok\tfirst\n020\tredo\tsecond\n030\tok\tthird", now()).unwrap(); let table = e.doc.children(None)[0];
        let cell = |e: &Editor, r: usize, c: usize| e.doc.children(Some(e.doc.children(Some(table))[r]))[c];
        let text = |e: &Editor, r: usize, c: usize| lines(&e.doc, cell(e, r, c));
        assert_eq!((e.doc.children(Some(table)).len(), text(&e, 3, 2)), (4, "third".to_string()));
        // a rectangle: its text for a spreadsheet, its copy a 2 x 2 table; emptied by Delete
        assert!(e.select_cells(table, (1, 0), (2, 1))); assert_eq!(e.cell_rect(), Some(CellRect { table, r0: 1, r1: 2, c0: 0, c1: 1 }));
        assert_eq!(e.cells_text(), "010\tok\n020\tredo"); let clip = e.copy_cells().unwrap(); assert!(matches!(&clip[0][0].kind, Kind::Table { header_rows: 0, columns } if columns.len() == 2));
        e.cells_delete(now()).unwrap(); assert_eq!((text(&e, 1, 0), text(&e, 2, 1), text(&e, 1, 2)), (String::new(), String::new(), "first".to_string())); assert_eq!(e.doc.children(Some(table)).len(), 4);
        // the copy goes back in from another cell: rows are added, the column past the last is left out
        let p = e.first_in(cell(&e, 3, 2)).unwrap(); e.set_selection(Some(Selection::caret(p)));
        let (_, left) = e.paste_table_clip(&clip, now()).unwrap().unwrap(); assert_eq!(left, 1); assert_eq!(e.doc.children(Some(table)).len(), 5); assert_eq!((text(&e, 3, 2), text(&e, 4, 2)), ("010".to_string(), "020".to_string()));
        // a typed column takes values: unknown names become options
        let p = e.first_in(cell(&e, 1, 1)).unwrap(); e.set_selection(Some(Selection::caret(p))); e.table_set_kind(ColumnKind::Choice { options: vec![] }, now()).unwrap();
        let p = e.first_in(cell(&e, 1, 1)).unwrap(); e.set_selection(Some(Selection::caret(p))); e.paste_grid_text("held\tx\nok\ty", now()).unwrap();
        let (_, cols) = spec_of(&e.doc, table); let ColumnKind::Choice { options } = &cols[1].kind else { panic!() }; assert!(options.iter().any(|o| o.label == "held")); assert_eq!((text(&e, 1, 1), text(&e, 2, 1), text(&e, 1, 2)), ("held".to_string(), "ok".to_string(), "x".to_string()));
        // whole rows: Delete removes them; whole columns too; all of them: the table goes
        let p = e.first_in(cell(&e, 2, 0)).unwrap(); e.set_selection(Some(Selection::caret(p))); assert!(e.select_line(false, false)); assert_eq!(e.cells_whole(), Some(false));
        let p = e.first_in(cell(&e, 3, 1)).unwrap(); let keep = e.selection().unwrap(); e.set_selection(Some(Selection::caret(p))); e.set_selection(Some(keep)); assert_eq!(e.cells_whole(), Some(false));
        e.cells_delete(now()).unwrap(); assert_eq!(e.doc.children(Some(table)).len(), 4);
        let p = e.first_in(cell(&e, 1, 2)).unwrap(); e.set_selection(Some(Selection::caret(p))); assert!(e.select_line(true, false)); e.cells_delete(now()).unwrap();
        assert!(matches!(&e.doc.get(table).unwrap().kind, Kind::Table { columns, .. } if columns.len() == 2)); assert_eq!(e.doc.children(Some(e.doc.children(Some(table))[0])).len(), 2);
        e.undo().unwrap(); assert_eq!(e.doc.children(Some(e.doc.children(Some(table))[0])).len(), 3);
        let p = e.first_in(cell(&e, 0, 0)).unwrap(); e.set_selection(Some(Selection::caret(p))); e.select_line(true, false); let p = e.first_in(cell(&e, 0, 2)).unwrap();
        let s = e.selection().unwrap(); e.set_selection(Some(Selection { anchor: s.anchor, focus: p })); e.whole = e.sel.map(|s| (s, true)); e.cells_delete(now()).unwrap();
        assert!(e.doc.get(table).is_none()); assert_eq!(e.doc.children(None).len(), 2);
    }
}
