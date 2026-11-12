//! Typed columns (canon row 317, stage 3): a choice, a check and a timecode column. A typed cell is what the 2.0 files
//! hold - ONE paragraph: a chip (choice, check) or plain text (timecode); an empty paragraph is "no value" (and a
//! check's "to do"). The column's option list is the truth; every chip still carries a copy (the Qt app reads it).
//! Header rows are plain text in every kind.
use mn_doc::{BlockId, Change, ChangeError, ChoiceOption, ColumnKind, Doc, Kind, Mark, Spans};
use crate::{paragraph, Dirty, Editor, Pos};

/// The colours a new option takes in turn.
pub const PALETTE: [&str; 8] = ["#8A8A8A", "#0189F1", "#58A65C", "#E5A33B", "#D9534F", "#9B6BD6", "#2BB3A6", "#C6689D"];
/// A check's three states.
pub fn check_options() -> Vec<ChoiceOption> { [("0", "To do", "#8A8A8A"), ("1", "Doing", "#0189F1"), ("2", "Done", "#58A65C")].iter().map(|(i, l, c)| ChoiceOption { id: i.to_string(), label: l.to_string(), color: c.to_string() }).collect() }
/// A label as an option takes it: no line breaks, no Markdown-active characters, trimmed.
pub fn clean_label(s: &str) -> String { s.chars().filter(|c| !matches!(c, '\n' | '\r' | '\t' | '*' | '_' | '`' | '[' | ']' | '|' | '~')).collect::<String>().trim().to_string() }

/// A chip's payload, as the files hold it: the whole option list and the chosen id.
pub fn payload(options: &[ChoiceOption], v: &str) -> String {
    serde_json::json!({ "o": options.iter().map(|o| serde_json::json!({ "c": o.color, "id": o.id, "l": o.label })).collect::<Vec<_>>(), "v": v }).to_string()
}
/// The chosen option's id in a paragraph that is a chip.
pub fn chosen(text: &str, spans: &Spans) -> Option<String> {
    let sp = spans.as_slice().iter().find(|s| matches!(s.mark, Mark::Choice(_)))?; if text.is_empty() { return None }
    let Mark::Choice(p) = &sp.mark else { return None }; serde_json::from_str::<serde_json::Value>(p).ok()?.get("v")?.as_str().map(str::to_string)
}
pub(crate) fn chip(options: &[ChoiceOption], id: &str) -> (String, Spans) {
    let Some(o) = options.iter().find(|o| o.id == id) else { return (String::new(), Spans::new()) };
    let mut s = Spans::new(); if !o.label.is_empty() { s.put(0, o.label.len(), Mark::Choice(payload(options, id))); } (o.label.clone(), s)
}

// -- timecode ------------------------------------------------------------------------------------------------------

/// (frames a second as shown, drop-frame) for a column's rate: 29.97 and 59.94 are drop-frame, 23.976 is not.
pub fn rate(fps: f64) -> (u64, bool) {
    if (fps - 23.976).abs() < 0.01 { (24, false) } else if (fps - 29.97).abs() < 0.01 { (30, true) } else if (fps - 59.94).abs() < 0.01 { (60, true) } else { ((fps.round() as u64).max(1), false) }
}
/// A cell's text as frames: a bare number is frames; else `hh:mm:ss:ff` (`;` reads as `:`). None: not a timecode.
pub fn parse_timecode(text: &str, fps: f64) -> Option<u64> {
    let t = text.trim(); if t.is_empty() { return None }
    if let Ok(n) = t.parse::<i64>() { return Some(n.max(0) as u64) }
    let f: Vec<u64> = t.replace(';', ":").split(':').map(|x| x.trim().parse::<u64>().ok()).collect::<Option<Vec<_>>>()?;
    let (fr, drop) = rate(fps); let [h, m, s, ff] = f[..] else { return None }; if m > 59 || s > 59 || ff >= fr { return None }
    let plain = ((h * 60 + m) * 60 + s) * fr + ff;
    if !drop { return Some(plain) }
    let d = fr / 15; let mins = h * 60 + m; Some(plain - d * (mins - mins / 10))
}
pub fn format_timecode(frames: u64, fps: f64) -> String {
    let (fr, drop) = rate(fps); let mut n = frames;
    if drop { let d = fr / 15; let (per10, per1) = (fr * 600 - 9 * d, fr * 60 - d); let (tens, m) = (n / per10, n % per10); n += d * 9 * tens + if m > d { d * ((m - d) / per1) } else { 0 }; }
    let (ff, s) = (n % fr, n / fr); format!("{:02}:{:02}:{:02}{}{:02}", s / 3600, s / 60 % 60, s % 60, if drop { ';' } else { ':' }, ff)
}
/// The text as its timecode; None: unreadable (it is left as typed) or empty.
pub fn normalise_timecode(text: &str, fps: f64) -> Option<String> { parse_timecode(text, fps).map(|n| format_timecode(n, fps)) }

/// The body cell of a typed column a block is in.
#[derive(Clone, Debug, PartialEq)]
pub struct Typed { pub table: BlockId, pub row: usize, pub col: usize, pub cell: BlockId, pub kind: ColumnKind }

/// The column a block is in: (its table, its row's place, its column's place, the cell, in a header row, the kind).
pub fn column_of(doc: &Doc, block: BlockId) -> Option<(BlockId, usize, usize, BlockId, bool, ColumnKind)> {
    let mut up = Some(block);
    while let Some(id) = up { let b = doc.get(id)?;
        if matches!(b.kind, Kind::Cell { .. }) { let row = b.parent?; let table = doc.get(row)?.parent?;
            let Kind::Table { header_rows, columns } = &doc.get(table)?.kind else { return None };
            let (r, c) = (doc.children(Some(table)).iter().position(|x| *x == row)?, doc.children(Some(row)).iter().position(|x| *x == id)?);
            return Some((table, r, c, id, r < *header_rows as usize, columns.get(c).map(|s| s.kind.clone()).unwrap_or(ColumnKind::Text))) }
        up = b.parent; }
    None
}
/// A typed column's body cell a block is in (None in a text column or a header row).
pub fn typed_of(doc: &Doc, block: BlockId) -> Option<Typed> {
    let (table, row, col, cell, header, kind) = column_of(doc, block)?; if header || kind == ColumnKind::Text { return None }
    Some(Typed { table, row, col, cell, kind })
}
/// A cell's content as one line of plain text (its blocks joined with a space).
fn plain(doc: &Doc, cell: BlockId) -> String {
    fn walk(doc: &Doc, id: BlockId, out: &mut Vec<String>) { let Some(b) = doc.get(id) else { return }; if b.kind.is_text() && !b.text.trim().is_empty() { out.push(b.text.split_whitespace().collect::<Vec<_>>().join(" ")); } for c in &b.children { walk(doc, *c, out); } }
    let mut out = vec![]; for c in doc.children(Some(cell)) { walk(doc, *c, &mut out); } out.join(" ")
}
/// A cell holds more than a typed value can keep: several blocks, a block that is not a paragraph, or a line break.
fn rich(doc: &Doc, cell: BlockId) -> bool {
    let kids = doc.children(Some(cell)); kids.len() > 1 || kids.iter().any(|k| doc.get(*k).is_some_and(|b| b.kind != Kind::Paragraph || !b.children.is_empty() || b.text.contains('\n')))
}

impl Editor {
    /// The typed cell the caret is in.
    pub fn typed_cell(&self) -> Option<Typed> { typed_of(&self.doc, self.sel?.focus.block) }
    /// The chosen option of a typed cell (a check's "to do" is None).
    pub fn cell_value(&self, cell: BlockId) -> Option<String> { let p = self.doc.get(*self.doc.children(Some(cell)).first()?)?; chosen(&p.text, &p.spans) }

    /// A cell is made ONE paragraph with this text and these spans; where the caret may stand in it.
    pub(crate) fn write_cell(&self, b: &mut crate::Batch, cell: BlockId, text: &str, spans: Spans) -> Result<Pos, ChangeError> {
        let kids = b.scratch.children(Some(cell)).to_vec();
        if let [one] = kids[..] { let blk = b.scratch.get(one).unwrap().clone();
            if blk.kind == Kind::Paragraph && blk.children.is_empty() {
                if blk.text != text || blk.spans != spans { b.push(Change::Text { id: one, at: 0, removed: blk.text.clone(), inserted: text.to_string(), old_spans: blk.spans.clone(), new_spans: spans })?; }
                return Ok(Pos { block: one, at: 0 }) } }
        let mut p = paragraph(Some(cell), text); p.spans = spans; let id = p.id;
        b.push(Change::Insert { index: 0, blocks: vec![p] })?;
        for k in kids { let c = Change::removal(&b.scratch, k)?; b.push(c)?; }
        Ok(Pos { block: id, at: 0 })
    }
    fn body_cells(&self, doc: &Doc, table: BlockId, col: usize) -> Vec<BlockId> {
        let head = match &doc.get(table).map(|t| &t.kind) { Some(Kind::Table { header_rows, .. }) => *header_rows as usize, _ => 0 };
        doc.children(Some(table)).iter().skip(head).filter_map(|r| doc.children(Some(*r)).get(col).copied()).collect()
    }
    fn set_spec(&self, b: &mut crate::Batch, table: BlockId, col: usize, kind: ColumnKind) -> Result<(), ChangeError> {
        let Some((head, mut cols)) = self.table_spec(table) else { return Ok(()) }; let Some(spec) = cols.get_mut(col) else { return Ok(()) }; spec.kind = kind;
        let old = b.scratch.get(table).unwrap().kind.clone(); let new = Kind::Table { header_rows: head, columns: cols }; if old != new { b.push(Change::Kind { id: table, old, new })?; } Ok(())
    }
    pub(crate) fn finish(&mut self, b: crate::Batch, caret: Option<Pos>, now_ms: u64) -> Result<Dirty, ChangeError> {
        let keep = self.sel; let d = self.commit(b, caret, now_ms + mn_doc::change::JOIN_MS + 1)?;
        if caret.is_none() { self.sel = keep.and_then(|s| self.live(s)).or_else(|| self.sel); if let Some(m) = self.marks.last_mut() { m.1 = self.sel; } }
        Ok(d)
    }

    /// The caret's choice cell takes an option (None clears it).
    pub fn cell_choose(&mut self, option: Option<&str>, now_ms: u64) -> Result<Dirty, ChangeError> {
        let Some(t) = self.typed_cell() else { return Ok(Dirty::Nothing) }; let ColumnKind::Choice { options } = &t.kind else { return Ok(Dirty::Nothing) };
        let (text, spans) = option.map(|id| chip(options, id)).unwrap_or_default();
        let mut b = self.batch(); let at = self.write_cell(&mut b, t.cell, &text, spans)?; self.finish(b, Some(at), now_ms)
    }
    /// The caret's choice cell takes a NEW option of this label (or the one there is of that label), added to its column.
    pub fn cell_choose_new(&mut self, label: &str, now_ms: u64) -> Result<Dirty, ChangeError> {
        let Some(t) = self.typed_cell() else { return Ok(Dirty::Nothing) }; let ColumnKind::Choice { options } = &t.kind else { return Ok(Dirty::Nothing) };
        let label = clean_label(label); if label.is_empty() { return Ok(Dirty::Nothing) }
        if let Some(o) = options.iter().find(|o| o.label.trim().to_lowercase() == label.to_lowercase()) { let id = o.id.clone(); return self.cell_choose(Some(&id), now_ms) }
        let mut all = options.clone(); let id = BlockId::new().to_string(); all.push(ChoiceOption { id: id.clone(), label, color: PALETTE[options.len() % PALETTE.len()].to_string() });
        let mut b = self.batch(); self.put_options(&mut b, t.table, t.col, &all)?;
        let (text, spans) = chip(&all, &id); let at = self.write_cell(&mut b, t.cell, &text, spans)?; self.finish(b, Some(at), now_ms)
    }
    /// The column's list and every chip of the column (each carries the list); a cell whose option went is cleared.
    fn put_options(&self, b: &mut crate::Batch, table: BlockId, col: usize, options: &[ChoiceOption]) -> Result<(), ChangeError> {
        self.set_spec(b, table, col, ColumnKind::Choice { options: options.to_vec() })?;
        for cell in self.body_cells(&b.scratch, table, col) {
            let v = b.scratch.children(Some(cell)).first().and_then(|p| b.scratch.get(*p)).and_then(|p| chosen(&p.text, &p.spans));
            let (text, spans) = v.map(|v| chip(options, &v)).unwrap_or_default(); self.write_cell(b, cell, &text, spans)?; }
        Ok(())
    }
    /// The caret's column's options are replaced (renamed, recoloured, reordered, removed, added): ONE undo step. Ids
    /// that stay keep their cells' values.
    pub fn table_set_options(&mut self, options: Vec<ChoiceOption>, now_ms: u64) -> Result<Dirty, ChangeError> {
        let Some((table, _, c)) = self.table_pos() else { return Ok(Dirty::Nothing) };
        if !matches!(self.table_spec(table).and_then(|(_, cols)| cols.get(c).map(|s| s.kind.clone())), Some(ColumnKind::Choice { .. })) { return Ok(Dirty::Nothing) }
        let options: Vec<ChoiceOption> = options.into_iter().filter_map(|mut o| { o.label = clean_label(&o.label); (!o.label.is_empty()).then_some(o) }).collect();
        let mut b = self.batch(); self.put_options(&mut b, table, c, &options)?; self.finish(b, None, now_ms)
    }
    /// The caret's check cell goes to its next state: to do, doing, done, to do.
    pub fn cell_check_cycle(&mut self, now_ms: u64) -> Result<Dirty, ChangeError> {
        let Some(t) = self.typed_cell() else { return Ok(Dirty::Nothing) }; if t.kind != ColumnKind::Check { return Ok(Dirty::Nothing) }
        let next = match self.cell_value(t.cell).as_deref() { Some("1") => "2", Some("2") => "0", _ => "1" };
        let (text, spans) = if next == "0" { Default::default() } else { chip(&check_options(), next) };
        let mut b = self.batch(); let at = self.write_cell(&mut b, t.cell, &text, spans)?; self.finish(b, Some(at), now_ms)
    }
    /// A paragraph of a timecode cell is written as its timecode (when the caret leaves the cell). Unreadable text stays.
    pub fn settle_timecode(&mut self, block: BlockId, now_ms: u64) -> Result<Dirty, ChangeError> {
        let Some(t) = typed_of(&self.doc, block) else { return Ok(Dirty::Nothing) }; let ColumnKind::Timecode { fps } = t.kind else { return Ok(Dirty::Nothing) };
        let Some(blk) = self.doc.get(block) else { return Ok(Dirty::Nothing) }; if blk.kind != Kind::Paragraph { return Ok(Dirty::Nothing) }
        let Some(n) = normalise_timecode(&blk.text, fps) else { return Ok(Dirty::Nothing) }; if n == blk.text { return Ok(Dirty::Nothing) }
        let mut b = self.batch(); b.push(Change::Text { id: block, at: 0, removed: blk.text.clone(), inserted: n, old_spans: blk.spans.clone(), new_spans: Spans::new() })?; self.finish(b, None, now_ms)
    }
    /// Text put into the caret's typed cell (a paste): a choice takes the option of that label, or a new one; a check
    /// its state (`[x]`, `[/]`, `[ ]` or a state's name); a timecode the text, written as a timecode. None: not typed.
    pub fn cell_put_text(&mut self, text: &str, now_ms: u64) -> Result<Option<Dirty>, ChangeError> {
        let Some(t) = self.typed_cell() else { return Ok(None) }; let v = text.split_whitespace().collect::<Vec<_>>().join(" ");
        match &t.kind {
            ColumnKind::Choice { .. } => if v.is_empty() { self.cell_choose(None, now_ms).map(Some) } else { self.cell_choose_new(&v, now_ms).map(Some) },
            ColumnKind::Check => { let st = check_state(&v); let (text, spans) = if st == "0" { Default::default() } else { chip(&check_options(), st) };
                let mut b = self.batch(); let at = self.write_cell(&mut b, t.cell, &text, spans)?; self.finish(b, Some(at), now_ms).map(Some) }
            ColumnKind::Timecode { fps } => { let n = normalise_timecode(&v, *fps).unwrap_or(v); let mut b = self.batch(); let mut at = self.write_cell(&mut b, t.cell, &n, Spans::new())?; at.at = n.len(); self.finish(b, Some(at), now_ms).map(Some) }
            ColumnKind::Text => Ok(None),
        }
    }
    /// How many body cells of the caret's column would lose content if the column became typed.
    pub fn table_kind_loss(&self) -> usize {
        let Some((table, _, c)) = self.table_pos() else { return 0 }; self.body_cells(&self.doc, table, c).into_iter().filter(|cell| rich(&self.doc, *cell)).count()
    }
    /// The caret's column becomes another kind: ONE undo step. Every body cell is cut down to one line: to a choice the
    /// options are the column's distinct values in order of first appearance; to a check a state's name is that state
    /// and anything else is "to do"; to a timecode the text, as a timecode; to text a chip is its label.
    pub fn table_set_kind(&mut self, kind: ColumnKind, now_ms: u64) -> Result<Dirty, ChangeError> {
        let Some((table, _, c)) = self.table_pos() else { return Ok(Dirty::Nothing) }; let Some((_, cols)) = self.table_spec(table) else { return Ok(Dirty::Nothing) };
        let was = cols.get(c).map(|s| s.kind.clone()).unwrap_or(ColumnKind::Text);
        if std::mem::discriminant(&was) == std::mem::discriminant(&kind) { // the same kind: only a timecode's rate may change
            let ColumnKind::Timecode { fps } = kind else { return Ok(Dirty::Nothing) }; if was == kind { return Ok(Dirty::Nothing) }
            let mut b = self.batch(); self.set_spec(&mut b, table, c, kind)?;
            for cell in self.body_cells(&self.doc, table, c) { let v = plain(&self.doc, cell); if let Some(n) = normalise_timecode(&v, fps) { self.write_cell(&mut b, cell, &n, Spans::new())?; } }
            return self.finish(b, None, now_ms) }
        let cells = self.body_cells(&self.doc, table, c); let values: Vec<String> = cells.iter().map(|cell| plain(&self.doc, *cell)).collect();
        let mut b = self.batch();
        let kind = match kind {
            ColumnKind::Choice { .. } => { let mut options: Vec<ChoiceOption> = vec![];
                for v in &values { let l = clean_label(v); if l.is_empty() || options.iter().any(|o| o.label.to_lowercase() == l.to_lowercase()) { continue } options.push(ChoiceOption { id: BlockId::new().to_string(), label: l, color: PALETTE[options.len() % PALETTE.len()].to_string() }); }
                ColumnKind::Choice { options } }
            k => k };
        self.set_spec(&mut b, table, c, kind.clone())?;
        for (cell, v) in cells.iter().zip(&values) {
            let (text, spans) = match &kind {
                ColumnKind::Choice { options } => { let l = clean_label(v).to_lowercase(); options.iter().find(|o| o.label.to_lowercase() == l).map(|o| chip(options, &o.id)).unwrap_or_default() }
                ColumnKind::Check => { let st = check_state(v); if st == "0" { Default::default() } else { chip(&check_options(), st) } }
                ColumnKind::Timecode { fps } => (normalise_timecode(v, *fps).unwrap_or(v.clone()), Spans::new()),
                ColumnKind::Text => (v.clone(), Spans::new()) };
            self.write_cell(&mut b, *cell, &text, spans)?; }
        let caret = self.sel.map(|s| s.focus).and_then(|f| self.cell_of(f.block)).and_then(|(_, _, cell)| b.scratch.children(Some(cell)).first().copied()).map(|p| Pos { block: p, at: 0 });
        self.finish(b, caret, now_ms)
    }
}
pub(crate) fn check_state(v: &str) -> &'static str {
    match v.trim().to_lowercase().as_str() { "[x]" | "done" | "2" => "2", "[/]" | "doing" | "1" => "1", _ => "0" }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Selection;
    #[test]
    fn timecodes_read_and_write_at_each_rate() {
        assert_eq!(normalise_timecode("1:2:3:4", 24.0).as_deref(), Some("01:02:03:04"));
        assert_eq!(normalise_timecode("48", 24.0).as_deref(), Some("00:00:02:00"));
        assert_eq!(normalise_timecode("-5", 25.0).as_deref(), Some("00:00:00:00"));
        assert_eq!(normalise_timecode("1:02", 24.0), None); assert_eq!(normalise_timecode("00:00:00:24", 24.0), None); assert_eq!(normalise_timecode("", 24.0), None);
        // drop-frame: frame 1800 at 29.97 is 00:01:00;02, and what is written reads back as the same frame
        assert_eq!(format_timecode(1800, 29.97), "00:01:00;02"); assert_eq!(format_timecode(17982, 29.97), "00:10:00;00");
        for n in [0u64, 1, 1799, 1800, 1801, 17981, 17982, 107892, 215999] { assert_eq!(parse_timecode(&format_timecode(n, 29.97), 29.97), Some(n), "{n}"); assert_eq!(parse_timecode(&format_timecode(n, 59.94), 59.94), Some(n)); }
        assert_eq!(format_timecode(24 * 3600, 23.976), "01:00:00:00");
    }
    #[test]
    fn a_column_becomes_typed_and_its_cells_take_values() {
        let mut e = Editor::new({ let mut d = Doc::new(); d.insert(None, 0, BlockId::new(), Kind::Paragraph, "").unwrap(); d });
        let first = e.doc.children(None)[0]; e.set_selection(Some(Selection::caret(Pos { block: first, at: 0 })));
        e.insert_table(4, 2, 1_000).unwrap(); let table = e.doc.children(None)[0];
        let cell = |e: &Editor, r: usize, c: usize| e.doc.children(Some(e.doc.children(Some(table))[r]))[c];
        let para = |e: &Editor, r: usize, c: usize| e.doc.children(Some(cell(e, r, c)))[0];
        let go = |e: &mut Editor, r: usize, c: usize| { let p = para(e, r, c); e.set_selection(Some(Selection::caret(Pos { block: p, at: 0 }))); };
        let mut t = 10_000u64; let mut now = || { t += 5_000; t };
        for (r, w) in [(0, "Status"), (1, "Open"), (2, "closed"), (3, "open")] { go(&mut e, r, 1); e.insert(w, now()).unwrap(); }
        // to a choice: the distinct values become the options, the header stays text
        go(&mut e, 1, 1); assert_eq!(e.table_kind_loss(), 0); e.table_set_kind(ColumnKind::Choice { options: vec![] }, now()).unwrap();
        let Some(Typed { kind: ColumnKind::Choice { options }, .. }) = e.typed_cell() else { panic!("a choice cell") };
        assert_eq!(options.iter().map(|o| o.label.as_str()).collect::<Vec<_>>(), ["Open", "closed"]);
        assert_eq!(e.cell_value(cell(&e, 3, 1)), Some(options[0].id.clone())); assert_eq!(e.doc.get(para(&e, 3, 1)).unwrap().text, "Open");
        assert!(e.doc.get(para(&e, 0, 1)).unwrap().spans.as_slice().is_empty()); go(&mut e, 0, 1); assert!(e.typed_cell().is_none(), "a header cell is text");
        // a pick, a clear, a new option (every chip of the column carries the new list)
        go(&mut e, 2, 1); e.cell_choose(Some(&options[0].id), now()).unwrap(); assert_eq!(e.doc.get(para(&e, 2, 1)).unwrap().text, "Open");
        e.cell_choose(None, now()).unwrap(); assert_eq!(e.cell_value(cell(&e, 2, 1)), None);
        e.cell_choose_new("Held", now()).unwrap(); assert_eq!(e.doc.get(para(&e, 2, 1)).unwrap().text, "Held");
        let Mark::Choice(p) = &e.doc.get(para(&e, 1, 1)).unwrap().spans.as_slice()[0].mark else { panic!() }; assert!(p.contains("Held"));
        // the options edited: a rename reaches the cells, a removed option clears its cells; one undo brings all back
        let Some(Typed { kind: ColumnKind::Choice { options }, .. }) = e.typed_cell() else { panic!() };
        let mut o2 = options.clone(); o2[0].label = "Active".into(); o2.remove(2); e.table_set_options(o2, now()).unwrap();
        assert_eq!(e.doc.get(para(&e, 1, 1)).unwrap().text, "Active"); assert_eq!(e.doc.get(para(&e, 2, 1)).unwrap().text, "");
        e.undo().unwrap(); assert_eq!(e.doc.get(para(&e, 1, 1)).unwrap().text, "Open"); assert_eq!(e.doc.get(para(&e, 2, 1)).unwrap().text, "Held");
        // back to text: a chip is its label
        go(&mut e, 1, 1); e.table_set_kind(ColumnKind::Text, now()).unwrap(); assert_eq!(e.doc.get(para(&e, 2, 1)).unwrap().text, "Held"); assert!(e.doc.get(para(&e, 2, 1)).unwrap().spans.as_slice().is_empty());
        // a check: cycles through its three states, "to do" is the empty cell
        go(&mut e, 1, 0); e.table_set_kind(ColumnKind::Check, now()).unwrap(); go(&mut e, 1, 0);
        e.cell_check_cycle(now()).unwrap(); assert_eq!(e.cell_value(cell(&e, 1, 0)).as_deref(), Some("1"));
        e.cell_check_cycle(now()).unwrap(); assert_eq!(e.doc.get(para(&e, 1, 0)).unwrap().text, "Done");
        e.cell_check_cycle(now()).unwrap(); assert_eq!(e.doc.get(para(&e, 1, 0)).unwrap().text, "");
        e.cell_put_text("[x]", now()).unwrap(); assert_eq!(e.cell_value(cell(&e, 1, 0)).as_deref(), Some("2"));
        // a timecode: settles when asked, an unreadable one stays; the rate changes what is written
        go(&mut e, 1, 0); e.table_set_kind(ColumnKind::Timecode { fps: 24.0 }, now()).unwrap();
        go(&mut e, 2, 0); e.insert("1:0:0:0", now()).unwrap(); let p = para(&e, 2, 0); e.settle_timecode(p, now()).unwrap(); assert_eq!(e.doc.get(p).unwrap().text, "01:00:00:00");
        go(&mut e, 3, 0); e.insert("soon", now()).unwrap(); let q = para(&e, 3, 0); e.settle_timecode(q, now()).unwrap(); assert_eq!(e.doc.get(q).unwrap().text, "soon");
        go(&mut e, 2, 0); e.table_set_kind(ColumnKind::Timecode { fps: 29.97 }, now()).unwrap(); assert_eq!(e.doc.get(para(&e, 2, 0)).unwrap().text, "01:00:00;00");
        // a cell with two blocks is counted before a kind change
        go(&mut e, 1, 1); e.insert("a\n", now()).unwrap(); e.insert("b", now()).unwrap(); go(&mut e, 2, 1); assert_eq!(e.table_kind_loss(), 1);
    }
}
