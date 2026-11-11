//! Reading a page of a 2.0 file (`mnd2`) into a note (rows 273, 283, 284; the encoding is in
//! `docs/phase3/step1-core-spec.md` §3.6–§5).
//!
//! The old file holds a page as ONE flat list. Side-by-side layout and tables are not stored as such: a `split` block
//! (a "record") is followed by blocks tagged `cell = n`, and a record with `header > 0` starts a table that runs over
//! the records directly below it. Here that becomes explicit: a record outside a table is a `Columns` block, a run of
//! table records is a `Table` of `Row`s of `Cell`s.
//!
//! Row 284: text is taken AS STORED (the old app consumed Markdown markers when it opened a file; this does not), and
//! a malformed list converts to what the old app would have shown after its own load repair — except that a table
//! cell stays in the column its number names (the old repair could close a gap by moving it left).
use std::collections::BTreeMap;
use serde_json::{Map, Value};
use crate::{id::BlockId, inline::Spans, tree::{ChoiceOption, ColumnKind, ColumnSpec, Doc, Kind}};

/// One row of the old `blocks` table, in the page's order.
#[derive(Clone, Debug, Default)]
pub struct OldBlock { pub id: String, pub depth: i64, pub kind: String, pub attrs: Option<String>, pub content: String }

#[derive(Debug)]
pub struct Converted {
    pub doc: Doc,
    /// What had to be repaired or could not be carried, in the page's order; empty for a well-formed page.
    pub notes: Vec<String>,
}

struct Parsed { id: BlockId, split: bool, kind: Kind, text: String, spans: Spans, cell: Option<usize>, attrs: Map<String, Value> }

fn str_of(m: &Map<String, Value>, k: &str) -> String { m.get(k).and_then(Value::as_str).unwrap_or("").to_string() }

fn parse(b: &OldBlock, notes: &mut Vec<String>) -> Parsed {
    let attrs = b.attrs.as_deref().and_then(|a| serde_json::from_str::<Value>(a).ok()).and_then(|v| match v { Value::Object(m) => Some(m), _ => None }).unwrap_or_default();
    let depth = b.depth.clamp(0, 8) as u8;
    let split = b.kind == "split";
    let kind = match b.kind.as_str() {
        "heading" => Kind::Heading { level: attrs.get("level").and_then(Value::as_i64).unwrap_or(1).clamp(1, 6) as u8 },
        "code" => Kind::Code { lang: str_of(&attrs, "lang") },
        "media" => Kind::Media { descriptor: b.content.clone() },
        "quote" => Kind::Quote,
        "list_item" => Kind::Bullet { depth },
        "ordered_item" => Kind::Numbered { depth },
        "task_item" => Kind::Task { depth, state: attrs.get("state").and_then(Value::as_i64).unwrap_or(0).clamp(0, 2) as u8 },
        "divider" => Kind::Divider,
        _ => Kind::Paragraph,   // anything unknown, the retired "table" among them; a split is replaced below
    };
    let text = if kind.is_text() { b.content.clone() } else { String::new() };
    let spans = if kind.is_text() && !matches!(kind, Kind::Code { .. }) {
        let raw: Vec<(usize, usize, &str, &str)> = attrs.get("spans").and_then(Value::as_array).map(|a| a.iter().filter_map(|s| {
            let o = s.as_object()?; Some((o.get("s")?.as_i64()?.max(0) as usize, o.get("e")?.as_i64()?.max(0) as usize, o.get("k")?.as_str()?, o.get("u").and_then(Value::as_str).unwrap_or("")))
        }).collect()).unwrap_or_default();
        Spans::from_utf16(&text, raw)
    } else { Spans::new() };
    let cell = attrs.get("cell").and_then(Value::as_f64).map(|c| c.clamp(0.0, 63.0) as usize);
    let id = BlockId::parse(&b.id).unwrap_or_else(|| { notes.push(format!("block id {:?} is not a ULID: a new id was given", b.id)); BlockId::new() });
    Parsed { id, split, kind, text, spans, cell, attrs }
}

enum Item { Leaf(Parsed), Layout { record: Parsed, lanes: Vec<(usize, Vec<Parsed>)> }, TableRow { record: Parsed, head: bool, cells: BTreeMap<usize, Vec<Parsed>> } }

/// The widths of a layout row's surviving lanes (spec §4.5, the ratios table). `lanes` are the lanes' original numbers.
fn widths(old: &[f64], lanes: &[usize]) -> Vec<f32> {
    let n = lanes.len(); let equal = vec![1.0 / n as f32; n];
    let mut seen = lanes.to_vec(); seen.sort(); seen.dedup();
    let top = lanes.iter().copied().max().unwrap_or(0);
    if seen.len() != n || old.is_empty() || old.len() < top + 1 || old.iter().any(|r| !(*r > 0.0)) || (old.iter().sum::<f64>() - 1.0).abs() > 0.01 { return equal }
    if old.len() == n && lanes.iter().enumerate().all(|(i, l)| i == *l) { return old.iter().map(|r| *r as f32).collect() }
    let mut w: Vec<f64> = lanes.iter().map(|l| old[*l]).collect();
    for (gone, r) in old.iter().enumerate().filter(|(i, _)| !lanes.contains(i)) {
        let to = lanes.iter().enumerate().filter(|(_, l)| **l < gone).max_by_key(|(_, l)| **l).map(|(i, _)| i).unwrap_or(0);
        w[to] += r;
    }
    let total: f64 = w.iter().sum();
    w.iter().map(|r| (r / total) as f32).collect()
}

fn column_spec(v: &Value) -> ColumnSpec {
    let Some(o) = v.as_object() else { return ColumnSpec::default() };
    let kind = match o.get("k").and_then(Value::as_i64).unwrap_or(0) {
        1 => ColumnKind::Choice { options: o.get("o").and_then(Value::as_array).map(|a| a.iter().filter_map(|x| x.as_object()).map(|x| ChoiceOption { id: str_of(x, "id"), label: str_of(x, "l"), color: str_of(x, "c") }).collect()).unwrap_or_default() },
        2 => ColumnKind::Check,
        3 => ColumnKind::Timecode { fps: o.get("fps").and_then(Value::as_f64).unwrap_or(24.0) },
        _ => ColumnKind::Text,
    };
    ColumnSpec { width: o.get("w").and_then(Value::as_u64).map(|w| w as u32), align: o.get("a").and_then(Value::as_u64).unwrap_or(0).min(2) as u8, bg: str_of(o, "bg"), fg: str_of(o, "fg"), kind }
}

/// Converts one page's blocks (in the page's order) into a note.
pub fn convert_page(blocks: &[OldBlock]) -> Converted {
    let mut notes = Vec::new();
    let mut queue: std::collections::VecDeque<Parsed> = blocks.iter().map(|b| parse(b, &mut notes)).collect();

    // 1. the flat list → top-level items, with the old load repair's rules (spec §4.5)
    let mut items: Vec<Item> = Vec::new(); let mut in_table = false;
    while let Some(b) = queue.pop_front() {
        if !b.split {
            if b.cell.is_some() { notes.push(format!("{}: a block marked as inside a row follows no row; it stands on its own", b.id)); }
            in_table = false; items.push(Item::Leaf(b)); continue
        }
        if b.cell.is_some() { notes.push(format!("{}: a row marked as inside another row; it starts its own", b.id)); }
        let mut inside = Vec::new();
        while queue.front().is_some_and(|n| !n.split && n.cell.is_some()) { inside.push(queue.pop_front().unwrap()); }
        if inside.is_empty() { notes.push(format!("{}: a row with nothing in it was dropped", b.id)); continue }
        let head = b.attrs.get("header").and_then(Value::as_f64).is_some_and(|h| h >= 1.0);
        if head || in_table {
            // a table row: a cell's number IS its column (row 284 — never renumbered)
            let mut cells: BTreeMap<usize, Vec<Parsed>> = BTreeMap::new();
            for p in inside { cells.entry(p.cell.unwrap()).or_default().push(p); }
            in_table = true; items.push(Item::TableRow { record: b, head, cells });
        } else {
            // a layout row: every run of one number is a lane, numbered as they come
            let mut lanes: Vec<(usize, Vec<Parsed>)> = Vec::new();
            for p in inside { match lanes.last_mut() { Some((n, v)) if *n == p.cell.unwrap() => v.push(p), _ => { if lanes.len() == 64 { lanes.last_mut().unwrap().1.push(p) } else { lanes.push((p.cell.unwrap(), vec![p])) } } } }
            if lanes.len() == 1 { notes.push(format!("{}: a side-by-side row with one side was unwrapped", b.id)); items.extend(lanes.pop().unwrap().1.into_iter().map(Item::Leaf)); }
            else { items.push(Item::Layout { record: b, lanes }); }
        }
    }

    // 2. the items → the tree
    let mut doc = Doc::new();
    fn leaf(doc: &mut Doc, parent: Option<BlockId>, p: Parsed) {
        let at = doc.children(parent).len();
        if doc.insert(parent, at, p.id, p.kind, p.text).is_ok() { let _ = doc.set_spans(p.id, p.spans); }
    }
    fn fresh(doc: &Doc, wanted: BlockId) -> BlockId { if doc.get(wanted).is_none() { wanted } else { BlockId::new() } }
    let mut table: Option<(BlockId, usize)> = None;   // the open table and its column count
    let mut items = items.into_iter().peekable();
    while let Some(item) = items.next() {
        match item {
            Item::Leaf(p) => { table = None; leaf(&mut doc, None, p); }
            Item::Layout { record, lanes } => {
                table = None;
                let old: Vec<f64> = record.attrs.get("ratios").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_f64).collect()).unwrap_or_default();
                let w = widths(&old, &lanes.iter().map(|(n, _)| *n).collect::<Vec<_>>());
                let at = doc.children(None).len();
                doc.insert(None, at, record.id, Kind::Columns, "").expect("a Columns block at the top level");
                for (k, (_, blocks)) in lanes.into_iter().enumerate() {
                    let col = fresh(&doc, record.id.derived(1 + k as u128));
                    doc.insert(Some(record.id), k, col, Kind::Column { width: w[k] }, "").expect("a column in a Columns block");
                    for p in blocks { leaf(&mut doc, Some(col), p); }
                }
            }
            Item::TableRow { record, head, cells } => {
                let style = record.attrs.get("table").and_then(Value::as_object).cloned().unwrap_or_default();
                if head || table.is_none() {
                    // the extent: this row and the headerless table rows directly below it — known now, so the column
                    // count and the header count can be settled before the rows go in
                    let cols: Vec<ColumnSpec> = style.get("cols").and_then(Value::as_array).map(|a| a.iter().map(column_spec).collect()).unwrap_or_default();
                    let widest = cells.keys().next_back().map(|c| c + 1).unwrap_or(1);
                    let id = fresh(&doc, record.id.derived(1000));
                    let at = doc.children(None).len();
                    doc.insert(None, at, id, Kind::Table { header_rows: record.attrs.get("header").and_then(Value::as_f64).unwrap_or(0.0).clamp(0.0, 255.0) as u8, columns: cols.clone() }, "").expect("a table at the top level");
                    table = Some((id, cols.len().max(widest)));
                }
                let (tid, _) = table.unwrap();
                // widen the table if this row reaches further than any before it
                let widest = cells.keys().next_back().map(|c| c + 1).unwrap_or(1);
                if widest > table.unwrap().1 { table = Some((tid, widest)); }
                let at = doc.children(Some(tid)).len();
                doc.insert(Some(tid), at, record.id, Kind::Row { bg: str_of(&style, "bg"), fg: str_of(&style, "fg") }, "").expect("a row in a table");
                let colour = |key: &str, c: usize| style.get(key).and_then(Value::as_array).and_then(|a| a.get(c)).and_then(Value::as_str).unwrap_or("").to_string();
                let mut cells = cells;
                for c in 0..=cells.keys().next_back().copied().unwrap_or(0) {
                    let cid = fresh(&doc, record.id.derived(1 + c as u128));
                    doc.insert(Some(record.id), c, cid, Kind::Cell { bg: colour("cbg", c), fg: colour("cfg", c) }, "").expect("a cell in a row");
                    for p in cells.remove(&c).unwrap_or_default() { leaf(&mut doc, Some(cid), p); }
                }
                // the table ends here unless another headerless table row follows
                if !matches!(items.peek(), Some(Item::TableRow { head: false, .. })) { finish_table(&mut doc, tid, table.unwrap().1); table = None; }
            }
        }
    }
    doc.rerank();
    Converted { doc, notes }
}

/// Squares a table off: every row gets a cell for every column, every cell at least an empty paragraph (a caret has
/// to be able to enter it), the column list is as long as the table is wide, and the header count fits the rows.
fn finish_table(doc: &mut Doc, table: BlockId, width: usize) {
    let rows: Vec<BlockId> = doc.children(Some(table)).to_vec();
    for row in &rows {
        for c in doc.children(Some(*row)).len()..width { let _ = doc.insert(Some(*row), c, BlockId::new(), Kind::cell(), ""); }
        for cell in doc.children(Some(*row)).to_vec() { if doc.children(Some(cell)).is_empty() { let _ = doc.insert(Some(cell), 0, BlockId::new(), Kind::Paragraph, ""); } }
    }
    if let Some(Kind::Table { header_rows, columns }) = doc.get(table).map(|b| b.kind.clone()) {
        let mut columns = columns; columns.resize(width.max(columns.len()), ColumnSpec::default());
        let _ = doc.set_kind(table, Kind::Table { header_rows: header_rows.min(rows.len() as u8), columns });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inline::Mark;
    fn ulid(n: u32) -> String { BlockId::from_u128((0x0190_0000_0000u128 << 80) + (n as u128) * 1_000_000).to_string() }
    fn id(n: u32) -> BlockId { BlockId::parse(&ulid(n)).unwrap() }
    fn b(n: u32, kind: &str, attrs: &str, content: &str) -> OldBlock { OldBlock { id: ulid(n), depth: 0, kind: kind.into(), attrs: (!attrs.is_empty()).then(|| attrs.into()), content: content.into() } }
    /// The tree as text: one line per block, indented.
    fn show(d: &Doc) -> String {
        fn go(d: &Doc, p: Option<BlockId>, depth: usize, out: &mut String) {
            for c in d.children(p) { let x = d.get(*c).unwrap();
                let k = match &x.kind { Kind::Paragraph => "p".to_string(), Kind::Bullet { depth } => format!("bullet{depth}"), Kind::Media { .. } => "media".into(), Kind::Columns => "COLUMNS".into(), Kind::Column { width } => format!("column {width:.2}"),
                    Kind::Table { header_rows, columns } => format!("TABLE header {header_rows} columns {}", columns.len()), Kind::Row { bg, .. } => format!("row{}", if bg.is_empty() { String::new() } else { format!(" bg {bg}") }), Kind::Cell { bg, .. } => format!("cell{}", if bg.is_empty() { String::new() } else { format!(" bg {bg}") }), other => format!("{other:?}") };
                out.push_str(&format!("{}{k}{}\n", "  ".repeat(depth), if x.text.is_empty() { String::new() } else { format!(" \"{}\"", x.text) }));
                go(d, Some(*c), depth + 1, out); }
        }
        let mut s = String::new(); go(d, None, 0, &mut s); s
    }

    /// Spec §4.6, example A.
    #[test]
    fn a_layout_row_becomes_columns() {
        let mut v = vec![b(1, "paragraph", "", "intro"), b(2, "split", r#"{"ratios":[0.4,0.6]}"#, ""), b(3, "paragraph", r#"{"cell":0}"#, "left one"), b(4, "list_item", r#"{"cell":1}"#, "right one"), b(5, "list_item", r#"{"cell":1}"#, "right two"), b(6, "paragraph", "", "outro")];
        v[4].depth = 1;
        let c = convert_page(&v);
        assert_eq!(show(&c.doc), "p \"intro\"\nCOLUMNS\n  column 0.40\n    p \"left one\"\n  column 0.60\n    bullet0 \"right one\"\n    bullet1 \"right two\"\np \"outro\"\n");
        assert!(c.notes.is_empty());
        assert_eq!(c.doc.children(None)[1], id(2));                       // the Columns block keeps the record's id
        assert_eq!(convert_page(&v).doc, c.doc);                          // the invented ids are the same every time
    }
    /// Spec §4.6, example B: a header row, a typed column, a two-block cell, colours, a ragged row.
    #[test]
    fn records_under_a_head_become_one_table() {
        let chip = r#"{\"o\":[],\"v\":\"OPT-A\"}"#;
        let v = vec![
            b(1, "split", r##"{"header":1,"ratios":[0.5,0.5],"table":{"cols":[{"w":200},{"k":1,"o":[{"c":"#58A65C","id":"OPT-A","l":"Open"},{"c":"#D9534F","id":"OPT-B","l":"Closed"}]}]}}"##, ""),
            b(2, "paragraph", r#"{"cell":0}"#, "Item"), b(3, "paragraph", r#"{"cell":1}"#, "State"),
            b(4, "split", r##"{"ratios":[0.5,0.5],"table":{"bg":"#222222","cbg":["","#334455"]}}"##, ""),
            b(5, "paragraph", r#"{"cell":0}"#, "first"), b(6, "media", r#"{"cell":0}"#, r#"{"h":300,"src":".minnotes/example.png","w":400}"#),
            b(7, "paragraph", &format!(r#"{{"cell":1,"spans":[{{"e":4,"k":"choice","s":0,"u":"{chip}"}}]}}"#), "Open"),
            b(8, "split", r#"{"ratios":[1]}"#, ""), b(9, "paragraph", r#"{"cell":0}"#, "second"),
            b(10, "paragraph", "", "after"),
        ];
        let c = convert_page(&v);
        assert_eq!(show(&c.doc), "TABLE header 1 columns 2\n  row\n    cell\n      p \"Item\"\n    cell\n      p \"State\"\n  row bg #222222\n    cell\n      p \"first\"\n      media\n    cell bg #334455\n      p \"Open\"\n  row\n    cell\n      p \"second\"\n    cell\n      p\np \"after\"\n");
        assert!(c.notes.is_empty(), "{:?}", c.notes);
        let Kind::Table { columns, .. } = &c.doc.get(c.doc.children(None)[0]).unwrap().kind else { panic!() };
        assert_eq!(columns[0].width, Some(200));
        assert!(matches!(&columns[1].kind, ColumnKind::Choice { options } if options.len() == 2 && options[1].label == "Closed"));
        assert!(c.doc.get(id(7)).unwrap().spans.chip_at(0).is_some());                      // the typed cell keeps its chip
        assert!(matches!(&c.doc.get(id(6)).unwrap().kind, Kind::Media { descriptor } if descriptor.contains("example.png")));
        assert_eq!(c.doc.get(id(4)).unwrap().kind, Kind::Row { bg: "#222222".into(), fg: String::new() });   // a row keeps its record's id
    }
    /// Spec §4.6, example C: the old load repair's cases.
    #[test]
    fn a_malformed_page_converts_to_what_the_old_app_would_have_shown() {
        let v = vec![b(0, "paragraph", r#"{"cell":0}"#, "p0"), b(1, "split", r#"{"ratios":[0.5,0.5]}"#, ""), b(2, "paragraph", "", "p2"), b(3, "split", r#"{"ratios":[1]}"#, ""),
            b(4, "paragraph", r#"{"cell":0}"#, "p4"), b(5, "paragraph", r#"{"cell":0}"#, "p5"), b(6, "split", r#"{"ratios":[0.2]}"#, ""), b(7, "paragraph", r#"{"cell":0}"#, "p7"), b(8, "paragraph", r#"{"cell":2}"#, "p8"), b(9, "paragraph", "", "p9")];
        let c = convert_page(&v);
        assert_eq!(show(&c.doc), "p \"p0\"\np \"p2\"\np \"p4\"\np \"p5\"\nCOLUMNS\n  column 0.50\n    p \"p7\"\n  column 0.50\n    p \"p8\"\np \"p9\"\n");
        assert_eq!(c.notes.len(), 3);
    }
    /// Row 284 (d): in a TABLE a cell's number is its column — a gap stays a gap (an empty cell), nothing moves left.
    #[test]
    fn a_table_cell_is_never_moved_to_another_column() {
        let v = vec![b(1, "split", r##"{"header":1,"table":{"cbg":["","","#ff0000"]}}"##, ""), b(2, "paragraph", r#"{"cell":0}"#, "a"), b(3, "paragraph", r#"{"cell":2}"#, "c")];
        let c = convert_page(&v);
        assert_eq!(show(&c.doc), "TABLE header 1 columns 3\n  row\n    cell\n      p \"a\"\n    cell\n      p\n    cell bg #ff0000\n      p \"c\"\n");
    }
    /// Spec §4.6, example D, and a header count larger than the table.
    #[test]
    fn a_second_head_starts_a_second_table_and_a_block_between_ends_one() {
        let row = |n: u32, attrs: &str| vec![b(n, "split", attrs, ""), b(n + 1, "paragraph", r#"{"cell":0}"#, "x"), b(n + 2, "paragraph", r#"{"cell":1}"#, "y")];
        let mut v = vec![]; v.extend(row(10, r#"{"header":9}"#)); v.extend(row(20, "")); v.extend(row(30, r#"{"header":1}"#)); v.extend(row(40, ""));
        v.push(b(50, "paragraph", "", "between")); v.extend(row(60, r#"{"ratios":[0.3,0.7]}"#));
        let s = show(&convert_page(&v).doc);
        assert_eq!(s.matches("TABLE").collect::<Vec<_>>().len(), 2);
        assert!(s.starts_with("TABLE header 2 columns 2\n"));                                // 9 header rows of 2: clamped
        assert!(s.ends_with("p \"between\"\nCOLUMNS\n  column 0.30\n    p \"x\"\n  column 0.70\n    p \"y\"\n"));
    }
    /// Row 284 (b): what the old app would have rewritten on open is read as it is stored; spans come over in bytes.
    #[test]
    fn text_is_taken_as_stored_and_spans_convert_from_utf16() {
        let v = vec![b(1, "paragraph", r#"{"spans":[{"s":3,"e":7,"k":"bold"},{"s":0,"e":2,"k":"comment"},{"s":8,"e":9,"k":"link","u":"https://example.com"}]}"#, "\u{1F600} bold x 2*3*4"),
            b(2, "code", r#"{"lang":"rust","spans":[{"s":0,"e":2,"k":"bold"}]}"#, "fn x()"), b(3, "task_item", r#"{"state":7}"#, "do"), b(4, "table", "", "old type"), b(5, "heading", r#"{"level":0}"#, "h")];
        let d = convert_page(&v).doc;
        let p = d.get(id(1)).unwrap();
        assert_eq!(p.text, "\u{1F600} bold x 2*3*4");
        assert!(p.spans.covers(5, 9, &Mark::Bold) && p.spans.covers(10, 11, &Mark::Link("https://example.com".into())) && p.spans.as_slice().len() == 2);
        assert!(d.get(id(2)).unwrap().spans.is_empty());                                      // a code block carries no formatting
        assert_eq!((&d.get(id(2)).unwrap().kind, &d.get(id(3)).unwrap().kind, &d.get(id(4)).unwrap().kind, &d.get(id(5)).unwrap().kind),
            (&Kind::Code { lang: "rust".into() }, &Kind::Task { depth: 0, state: 2 }, &Kind::Paragraph, &Kind::Heading { level: 1 }));
    }
    #[test]
    fn widths_follow_the_old_repairs_table() {
        assert_eq!(widths(&[0.2, 0.3, 0.5], &[0, 1, 2]), [0.2, 0.3, 0.5]);
        assert_eq!(widths(&[0.2, 0.3, 0.5], &[0, 2]), [0.5, 0.5]);                             // lane 1 gone: its share goes to lane 0
        assert_eq!(widths(&[0.2, 0.3, 0.5], &[1, 2]), [0.5, 0.5]);                             // lane 0 gone: to the first survivor
        assert_eq!(widths(&[0.2], &[0, 2]), [0.5, 0.5]);                                       // too short
        assert_eq!(widths(&[0.9, 0.9], &[0, 1]), [0.5, 0.5]);                                  // does not sum to 1
        assert_eq!(widths(&[0.0, 1.0], &[0, 1]), [0.5, 0.5]);                                  // a ratio that is not positive
        assert_eq!(widths(&[0.5, 0.5], &[0, 1, 0]), [1.0 / 3.0; 3]);                           // a lane number twice
    }
}
