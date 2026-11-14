//! A TABLE SHOWN AS A BOARD (row 365): the rows of a table as cards, in a lane for each value of one of its columns -
//! a choice column's options, or a check column's three states. This is the board as DATA and as PLACES (what is in
//! which lane, how tall it stands, where each lane and card is); the application paints it and takes its presses.
//!
//! Which column groups is the column's own (`ColumnSpec::board`), so it is saved with the note. The table is never
//! changed by being shown this way: a card IS its row.
use mn_doc::{BlockId, ColumnKind, ColumnSpec, Doc, Kind, Mark};

/// One line of a card under its title.
#[derive(Clone, Debug, PartialEq)]
pub enum Detail { Text(String), /// a choice's label and colour
    Chip(String, String), /// a check: its state (0 to do, 1 doing, 2 done) and the column's name
    Check(u8, String) }
#[derive(Clone, Debug, PartialEq)]
pub struct Card { pub row: BlockId, pub title: String, pub details: Vec<Detail>, /// the first picture in the row (its record)
    pub cover: Option<String>, /// the row's own colour, as an edge
    pub edge: String }
#[derive(Clone, Debug, PartialEq)]
pub struct Lane { /// the option's id ("" = no value; a check's "0" / "1" / "2")
    pub key: String, pub label: String, pub color: String, pub cards: Vec<Card> }
#[derive(Clone, Debug, PartialEq)]
pub struct Board { pub table: BlockId, /// the grouping column's index
    pub column: usize, pub check: bool, pub lanes: Vec<Lane> }

// the board's measures, in logical points
pub const LANE_W: f64 = 264.0;
pub const LANE_GAP: f64 = 12.0;
pub const LANE_PAD: f64 = 8.0;
pub const HEAD: f64 = 38.0;
pub const FOOT: f64 = 34.0;
pub const CARD_GAP: f64 = 8.0;
pub const CARD_PAD: f64 = 10.0;
pub const TITLE: f64 = 20.0;
pub const LINE: f64 = 20.0;
/// The bar a board has along its top: what it is grouped by, and the way back to the table.
pub const BAR: f64 = 34.0;

/// The colours of a check's three states (the picker's own: `mn_edit::typed::check_options`).
const CHECK: [(&str, &str, &str); 3] = [("0", "To do", "#8A8A8A"), ("1", "Doing", "#0189F1"), ("2", "Done", "#58A65C")];

/// The column a table is shown as a board by: the first that says so and can (a choice or a check column).
pub fn column(columns: &[ColumnSpec]) -> Option<usize> { columns.iter().position(|c| c.board && matches!(c.kind, ColumnKind::Choice { .. } | ColumnKind::Check)) }

fn first_line(doc: &Doc, cell: BlockId) -> String { doc.children(Some(cell)).iter().filter_map(|b| doc.get(*b)).find(|b| !matches!(b.kind, Kind::Media { .. })).map(|b| b.text.lines().next().unwrap_or("").trim().to_string()).unwrap_or_default() }
/// A typed cell's value: the id its chip carries (None: nothing chosen).
fn value(doc: &Doc, cell: BlockId) -> Option<String> {
    let b = doc.children(Some(cell)).first().and_then(|b| doc.get(*b))?; if b.text.is_empty() { return None }
    let sp = b.spans.as_slice().iter().find(|s| matches!(s.mark, Mark::Choice(_)))?; let Mark::Choice(p) = &sp.mark else { return None };
    serde_json::from_str::<serde_json::Value>(p).ok()?.get("v")?.as_str().map(str::to_string)
}

/// The board a table shows, when one of its columns says it is shown as one.
pub fn build(doc: &Doc, table: BlockId) -> Option<Board> {
    let Kind::Table { header_rows, columns } = &doc.get(table)?.kind else { return None }; let col = column(columns)?;
    let rows = doc.children(Some(table)); let head = (*header_rows as usize).min(rows.len());
    let names: Vec<String> = match rows.first().filter(|_| head > 0) { Some(r) => doc.children(Some(*r)).iter().map(|c| first_line(doc, *c)).collect(), None => vec![] };
    let (check, mut lanes): (bool, Vec<Lane>) = match &columns[col].kind {
        ColumnKind::Choice { options } => (false, options.iter().map(|o| Lane { key: o.id.clone(), label: o.label.clone(), color: o.color.clone(), cards: vec![] }).chain(std::iter::once(Lane { key: String::new(), label: "No status".into(), color: String::new(), cards: vec![] })).collect()),
        _ => (true, CHECK.iter().map(|(k, l, c)| Lane { key: k.to_string(), label: l.to_string(), color: c.to_string(), cards: vec![] }).collect()),
    };
    let title_col = columns.iter().enumerate().position(|(i, c)| i != col && matches!(c.kind, ColumnKind::Text)).or_else(|| (0..columns.len()).find(|i| *i != col));
    for (n, row) in rows.iter().enumerate().skip(head) {
        let cells = doc.children(Some(*row)); let v = cells.get(col).and_then(|c| value(doc, *c));
        let key = if check { v.filter(|v| v == "1" || v == "2").unwrap_or_else(|| "0".into()) } else { v.filter(|v| lanes.iter().any(|l| &l.key == v)).unwrap_or_default() };
        let title = title_col.and_then(|i| cells.get(i)).map(|c| first_line(doc, *c)).filter(|t| !t.is_empty()).unwrap_or_else(|| format!("Row {}", n + 1 - head));
        let mut details = vec![];
        for (i, c) in cells.iter().enumerate() { if i == col || Some(i) == title_col { continue }
            match columns.get(i).map(|c| &c.kind) {
                Some(ColumnKind::Choice { options }) => { if let Some(o) = value(doc, *c).and_then(|v| options.iter().find(|o| o.id == v)) { details.push(Detail::Chip(o.label.clone(), o.color.clone())); } }
                Some(ColumnKind::Check) => details.push(Detail::Check(value(doc, *c).and_then(|v| v.parse().ok()).unwrap_or(0), names.get(i).cloned().unwrap_or_default())),
                _ => { let t = first_line(doc, *c); if !t.is_empty() { details.push(Detail::Text(t)); } } } }
        let cover = cells.iter().flat_map(|c| doc.children(Some(*c))).filter_map(|b| doc.get(*b)).find_map(|b| match &b.kind { Kind::Media { descriptor } if !descriptor.contains("\"kind\"") => Some(descriptor.clone()), _ => None });
        let edge = match doc.get(*row).map(|r| &r.kind) { Some(Kind::Row { bg, .. }) => bg.clone(), _ => String::new() };
        if let Some(l) = lanes.iter_mut().find(|l| l.key == key) { l.cards.push(Card { row: *row, title, details, cover, edge }); }
    }
    // the lane of rows without a value shows only while it has some
    if !check && lanes.last().is_some_and(|l| l.cards.is_empty()) { lanes.pop(); }
    Some(Board { table, column: col, check, lanes })
}

/// The grouping column's name (its header cell's first line), or what kind of column it is.
pub fn column_name(doc: &Doc, b: &Board) -> String {
    let Some(Kind::Table { header_rows, .. }) = doc.get(b.table).map(|t| &t.kind) else { return String::new() };
    let name = if *header_rows > 0 { doc.children(Some(b.table)).first().and_then(|r| doc.children(Some(*r)).get(b.column).copied()).map(|c| first_line(doc, c)).unwrap_or_default() } else { String::new() };
    if name.is_empty() { if b.check { "check".into() } else { "choice".into() } } else { name }
}
/// A card's height: its cover (16 : 9 across the card), its title, a line for each detail.
pub fn card_height(c: &Card) -> f64 { 2.0 * CARD_PAD + if c.cover.is_some() { (LANE_W - 2.0 * LANE_PAD) * 9.0 / 16.0 + 8.0 - CARD_PAD } else { 0.0 } + TITLE + c.details.len() as f64 * LINE }
pub fn lane_height(l: &Lane) -> f64 { HEAD + l.cards.iter().map(|c| card_height(c) + CARD_GAP).sum::<f64>() + FOOT }
/// The whole board's size in logical points: every lane side by side, as tall as the tallest, under its bar.
pub fn size(b: &Board) -> (f64, f64) { (b.lanes.len() as f64 * (LANE_W + LANE_GAP) - LANE_GAP, BAR + b.lanes.iter().map(lane_height).fold(HEAD + FOOT, f64::max)) }

/// Where things are, from the board's top left, in logical points: each lane's panel, and its cards.
pub struct Placed { pub lanes: Vec<(kurbo::Rect, Vec<kurbo::Rect>)>, pub width: f64, pub height: f64 }
pub fn place(b: &Board) -> Placed {
    let (width, height) = size(b); let mut lanes = Vec::with_capacity(b.lanes.len());
    for (i, l) in b.lanes.iter().enumerate() { let x = i as f64 * (LANE_W + LANE_GAP); let mut y = BAR + HEAD; let mut cards = Vec::with_capacity(l.cards.len());
        for c in &l.cards { let h = card_height(c); cards.push(kurbo::Rect::new(x + LANE_PAD, y, x + LANE_W - LANE_PAD, y + h)); y += h + CARD_GAP; }
        lanes.push((kurbo::Rect::new(x, BAR, x + LANE_W, height), cards)); }
    Placed { lanes, width, height }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mn_doc::{ChoiceOption, Spans};
    fn add(d: &mut Doc, parent: Option<BlockId>, kind: Kind, text: &str) -> BlockId { let id = BlockId::new(); let i = d.children(parent).len(); d.insert(parent, i, id, kind, text).unwrap(); id }
    #[test] fn rows_stand_in_the_lane_of_their_value_and_a_board_is_as_tall_as_its_tallest_lane() {
        let mut d = Doc::new();
        let opts = vec![ChoiceOption { id: "a".into(), label: "Doing".into(), color: "#0189F1".into() }, ChoiceOption { id: "b".into(), label: "Done".into(), color: "#58A65C".into() }];
        let t = add(&mut d, None, Kind::Table { header_rows: 1, columns: vec![ColumnSpec::default(), ColumnSpec { kind: ColumnKind::Choice { options: opts }, board: true, ..Default::default() }] }, "");
        let mut row = |d: &mut Doc, name: &str, v: Option<&str>| { let r = add(d, Some(t), Kind::Row { bg: String::new(), fg: String::new() }, ""); let c1 = add(d, Some(r), Kind::Cell { bg: String::new(), fg: String::new() }, ""); add(d, Some(c1), Kind::Paragraph, name);
            let c2 = add(d, Some(r), Kind::Cell { bg: String::new(), fg: String::new() }, ""); let p = add(d, Some(c2), Kind::Paragraph, v.map(|_| "x").unwrap_or(""));
            if let Some(v) = v { let mut s = Spans::new(); s.put(0, 1, Mark::Choice(format!("{{\"v\":\"{v}\"}}"))); d.set_spans(p, s).unwrap(); } r };
        row(&mut d, "Shot", None); let (r1, r2, r3) = (row(&mut d, "TW_010", Some("b")), row(&mut d, "TW_020", Some("a")), row(&mut d, "TW_030", None));
        let b = build(&d, t).unwrap();
        assert_eq!(b.lanes.iter().map(|l| (l.label.as_str(), l.cards.iter().map(|c| c.row).collect::<Vec<_>>())).collect::<Vec<_>>(), vec![("Doing", vec![r2]), ("Done", vec![r1]), ("No status", vec![r3])]);
        assert_eq!(b.lanes[0].cards[0].title, "TW_020");
        let p = place(&b); assert_eq!(p.lanes.len(), 3); assert!((p.height - (BAR + HEAD + card_height(&b.lanes[0].cards[0]) + CARD_GAP + FOOT)).abs() < 1e-9);
        // a table none of whose columns says so is no board
        if let Some(Kind::Table { header_rows, mut columns }) = d.get(t).map(|x| x.kind.clone()) { columns[1].board = false; d.set_kind(t, Kind::Table { header_rows, columns }).unwrap(); } assert!(build(&d, t).is_none());
    }
}
