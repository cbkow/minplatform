//! What a table shown as a board asks of the editor (row 365): a card IS a row, so every one of these is a change to
//! the table - one step, undone as any. Unlike the table's own commands they do not go by the caret (a board has no
//! cell to stand in): the table, the row and the column are named.
use mn_doc::{BlockId, Change, ChangeError, ColumnKind, Kind, Spans};
use crate::{table::row_tree, typed::{check_options, chip}, Dirty, Editor, Pos};

impl Editor {
    /// A value of the grouping column as a cell holds it: a choice's chip; a check's (its "to do" is the empty cell).
    fn board_cell(&self, table: BlockId, col: usize, value: Option<&str>) -> Option<(String, Spans)> {
        let Kind::Table { columns, .. } = &self.doc.get(table)?.kind else { return None };
        Some(match &columns.get(col)?.kind {
            ColumnKind::Choice { options } => value.filter(|v| options.iter().any(|o| o.id == *v)).map(|v| chip(options, v)).unwrap_or_default(),
            ColumnKind::Check => value.filter(|v| *v == "1" || *v == "2").map(|v| chip(&check_options(), v)).unwrap_or_default(),
            _ => return None })
    }
    /// A card goes to a lane: its row takes the lane's value in the grouping column and, when a place among the
    /// lane's cards was given, moves to stand before that card's row (None: it stays where it is in the table).
    pub fn board_move(&mut self, table: BlockId, row: BlockId, col: usize, value: Option<&str>, before: Option<BlockId>, now_ms: u64) -> Result<Dirty, ChangeError> {
        let Some((text, spans)) = self.board_cell(table, col, value) else { return Ok(Dirty::Nothing) };
        let Some(cell) = self.doc.children(Some(row)).get(col).copied() else { return Ok(Dirty::Nothing) };
        let mut b = self.batch(); self.write_cell(&mut b, cell, &text, spans)?;
        if let Some(to) = before.filter(|t| *t != row) { let rows = self.doc.children(Some(table)).to_vec();
            if let (Some(from), Some(_)) = (rows.iter().position(|r| *r == row), rows.iter().position(|r| *r == to)) {
                let at = rows.iter().filter(|r| **r != row).position(|r| *r == to).unwrap_or(0);   // where it stands once the row is out of the way
                if at != from { b.push(Change::Move { id: row, from: (Some(table), from), to: (Some(table), at) })?; } } }
        let keep = self.sel; let d = self.commit(b, None, now_ms + mn_doc::change::JOIN_MS + 1)?; self.sel = keep.and_then(|s| self.live(s)); Ok(d)
    }
    /// A card is added to a lane: a new row after `after` (None: at the table's end) with the lane's value. Its row.
    pub fn board_add(&mut self, table: BlockId, col: usize, value: Option<&str>, after: Option<BlockId>, now_ms: u64) -> Result<(Dirty, Option<BlockId>), ChangeError> {
        let Some(Kind::Table { columns, .. }) = self.doc.get(table).map(|t| t.kind.clone()) else { return Ok((Dirty::Nothing, None)) };
        let Some((text, spans)) = self.board_cell(table, col, value) else { return Ok((Dirty::Nothing, None)) };
        let rows = self.doc.children(Some(table)).to_vec(); let index = after.and_then(|a| rows.iter().position(|r| *r == a)).map(|i| i + 1).unwrap_or(rows.len());
        let (tree, _) = row_tree(table, columns.len()); let row = tree[0].id; let cell = tree[0].children.get(col).copied();
        let mut b = self.batch(); b.push(Change::Insert { index, blocks: tree })?;
        if let Some(c) = cell { self.write_cell(&mut b, c, &text, spans)?; }
        let keep = self.sel; let d = self.commit(b, None, now_ms + mn_doc::change::JOIN_MS + 1)?; self.sel = keep.and_then(|s| self.live(s)); Ok((d, Some(row)))
    }
    /// A card's title: the text of one cell of its row (plain; what else the cell held goes).
    pub fn board_title(&mut self, row: BlockId, col: usize, title: &str, now_ms: u64) -> Result<Dirty, ChangeError> {
        let Some(cell) = self.doc.children(Some(row)).get(col).copied() else { return Ok(Dirty::Nothing) };
        let mut b = self.batch(); self.write_cell(&mut b, cell, title, Spans::new())?;
        let keep = self.sel; let d = self.commit(b, None, now_ms + mn_doc::change::JOIN_MS + 1)?; self.sel = keep.and_then(|s| self.live(s)); Ok(d)
    }
    /// A card is removed: its row leaves the table (never the last row there is under the header).
    pub fn board_remove(&mut self, table: BlockId, row: BlockId, now_ms: u64) -> Result<Dirty, ChangeError> {
        let Some(Kind::Table { header_rows, .. }) = self.doc.get(table).map(|t| t.kind.clone()) else { return Ok(Dirty::Nothing) };
        let rows = self.doc.children(Some(table)); if !rows.contains(&row) || rows.len() <= header_rows as usize + 1 { return Ok(Dirty::Nothing) }
        let mut b = self.batch(); let c = Change::removal(&b.scratch, row)?; b.push(c)?;
        self.blocks = None; let d = self.commit(b, None, now_ms + mn_doc::change::JOIN_MS + 1)?; if self.sel.is_some_and(|s| self.live(s).is_none()) { self.sel = Some(crate::Selection::caret(Pos { block: self.doc.walk().iter().find(|x| !x.kind.is_container()).map(|x| x.id).unwrap_or(row), at: 0 })); } Ok(d)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mn_doc::{ChoiceOption, ColumnSpec, Doc};
    fn add(d: &mut Doc, parent: Option<BlockId>, kind: Kind, text: &str) -> BlockId { let id = BlockId::new(); let i = d.children(parent).len(); d.insert(parent, i, id, kind, text).unwrap(); id }
    #[test] fn a_card_takes_its_lanes_value_and_place_as_one_step_and_cards_are_added_named_and_removed() {
        let mut d = Doc::new(); add(&mut d, None, Kind::Paragraph, "above");
        let opts = vec![ChoiceOption { id: "a".into(), label: "Doing".into(), color: "#0189F1".into() }, ChoiceOption { id: "b".into(), label: "Done".into(), color: "#58A65C".into() }];
        let t = add(&mut d, None, Kind::Table { header_rows: 1, columns: vec![ColumnSpec::default(), ColumnSpec { kind: ColumnKind::Choice { options: opts }, board: true, ..Default::default() }] }, "");
        let mut rows = vec![]; for name in ["Shot", "one", "two", "three"] { let r = add(&mut d, Some(t), Kind::row(), ""); for (i, text) in [name, ""].iter().enumerate() { let c = add(&mut d, Some(r), Kind::cell(), ""); add(&mut d, Some(c), Kind::Paragraph, if i == 0 { text } else { "" }); } rows.push(r); }
        let mut e = Editor::new(d); let before = e.doc.clone();
        let value = |e: &Editor, r: BlockId| e.cell_value(e.doc.children(Some(r))[1]);
        // "three" goes to Done, to stand before "one": its value and its place change together, and one undo takes both back
        assert_eq!(e.board_move(t, rows[3], 1, Some("b"), Some(rows[1]), 1000).unwrap(), Dirty::Structure);
        assert_eq!((value(&e, rows[3]).as_deref(), e.doc.children(Some(t)).to_vec()), (Some("b"), vec![rows[0], rows[3], rows[1], rows[2]]));
        e.undo().unwrap(); assert_eq!(e.doc, before);
        // dropped into a lane without a place: only the value changes; "no status" empties the cell
        e.board_move(t, rows[2], 1, Some("a"), None, 9000).unwrap(); assert_eq!((value(&e, rows[2]).as_deref(), e.doc.children(Some(t)).len()), (Some("a"), 4));
        e.board_move(t, rows[2], 1, None, None, 20000).unwrap(); assert_eq!(value(&e, rows[2]), None);
        // a card added to Doing after "one", then named; then removed - but never the last row under the header
        let (_, new) = e.board_add(t, 1, Some("a"), Some(rows[1]), 30000).unwrap(); let new = new.unwrap();
        assert_eq!((e.doc.children(Some(t))[2], value(&e, new).as_deref()), (new, Some("a")));
        e.board_title(new, 0, "four", 40000).unwrap(); assert_eq!(e.doc.get(e.doc.children(Some(e.doc.children(Some(new))[0]))[0]).unwrap().text, "four");
        for r in [new, rows[1], rows[2]] { e.board_remove(t, r, 50000).unwrap(); } assert_eq!(e.doc.children(Some(t)).to_vec(), vec![rows[0], rows[3]]);
        e.board_remove(t, rows[3], 60000).unwrap(); assert_eq!(e.doc.children(Some(t)).len(), 2);
    }
}
