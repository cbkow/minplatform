//! The three-way merge of a note by block id (rows 272, 283, 285): `base` is the note as it was opened, `mine` what it
//! has become here, `theirs` what someone else saved meanwhile. Nothing either side wrote is dropped silently — where
//! the two cannot both stand, both are kept or the choice is REPORTED.
//!
//! The rules, per block:
//! 1. CONTENT (kind, text, formatting). Changed on one side: that side's. Changed the same way on both: once. Changed
//!    differently on both: a text block is kept TWICE — mine, then theirs directly after it under a new id and marked
//!    as in conflict with mine; for a block that cannot be doubled (a media item's descriptor, a container's own
//!    attributes) mine stands and the case is reported.
//! 2. DELETION. Deleted on one side and untouched on the other (the block and everything under it): gone. Deleted on
//!    one side but changed, moved, or added to on the other: it stays, as the other side has it, and is reported.
//! 3. ADDITION. A block either side added is kept.
//! 4. PLACE. A block's parent is the side's that moved it; moved by both to different parents: mine, reported. Within a
//!    list the order is mine, and every block THEY added or moved (and I did not) goes after the block it follows on
//!    their side. Two additions at the same place: mine first.
//! 5. TABLES are squared off afterwards (a column added on one side and a row on the other leave short rows): every
//!    row gets a cell per column, at the END of the row; reported.
use std::collections::{HashMap, HashSet};
use crate::{id::BlockId, tree::{Block, ColumnSpec, Doc, Kind}};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Note {
    /// Both changed this text block: `mine` stands, `theirs` follows it as a new block marked in conflict.
    BothEdited { mine: BlockId, theirs: BlockId },
    /// Both changed a block that cannot be doubled: mine stands.
    MineKept(BlockId),
    /// One side deleted it, the other changed it (or something under it): it stays.
    KeptOverDelete(BlockId),
    /// Both moved it to different parents: it is where mine put it.
    MovedByBoth(BlockId),
    /// Its place could not be honoured (its parent is gone, or the rules of what may hold what refuse it): it was put
    /// at the end of the note.
    Misplaced(BlockId),
    /// A block that could not be placed anywhere (a table part without its table).
    Lost(BlockId),
    /// A table whose rows were of different lengths after the merge: short rows were filled at their end.
    Squared(BlockId),
}

#[derive(Debug)]
pub struct Merged { pub doc: Doc, pub notes: Vec<Note> }

fn same(a: &Block, b: &Block) -> bool { a.kind == b.kind && a.text == b.text && a.spans == b.spans && a.conflict == b.conflict }

struct Side<'a> { doc: &'a Doc, base: &'a Doc, memo: HashMap<BlockId, bool> }
impl Side<'_> {
    /// The block itself differs from the base: its content, or where it hangs.
    fn changed(&self, id: BlockId) -> bool { match (self.doc.get(id), self.base.get(id)) { (Some(a), Some(b)) => !same(a, b) || a.parent != b.parent, (Some(_), None) => true, _ => false } }
    /// Anything in the subtree differs from the base (a block added or moved in counts).
    fn touched(&mut self, id: BlockId) -> bool {
        if let Some(t) = self.memo.get(&id) { return *t }
        let t = self.changed(id) || self.doc.children(Some(id)).to_vec().into_iter().any(|c| self.touched(c));
        self.memo.insert(id, t); t
    }
}

/// The ids of `list` that are NOT part of the longest run that kept its relative order from `base` — the ones a side
/// actually moved within a list (plus the ones `base` never had there).
fn moved(base: &[BlockId], list: &[BlockId]) -> HashSet<BlockId> {
    let pos: HashMap<BlockId, usize> = base.iter().enumerate().map(|(i, b)| (*b, i)).collect();
    let seq: Vec<(usize, BlockId)> = list.iter().filter_map(|x| pos.get(x).map(|p| (*p, *x))).collect();
    // longest increasing subsequence of the base positions
    let mut tails: Vec<usize> = Vec::new(); let mut prev: Vec<Option<usize>> = vec![None; seq.len()];
    for i in 0..seq.len() {
        let k = tails.partition_point(|&t| seq[t].0 < seq[i].0);
        if k > 0 { prev[i] = Some(tails[k - 1]); }
        if k == tails.len() { tails.push(i) } else { tails[k] = i }
    }
    let mut kept = HashSet::new(); let mut at = tails.last().copied();
    while let Some(i) = at { kept.insert(seq[i].1); at = prev[i]; }
    list.iter().filter(|x| !kept.contains(*x)).copied().collect()
}

pub fn merge(base: &Doc, mine: &Doc, theirs: &Doc) -> Merged {
    let mut notes = Vec::new();
    let (mut m, mut t) = (Side { doc: mine, base, memo: HashMap::new() }, Side { doc: theirs, base, memo: HashMap::new() });

    // who survives (rules 2 and 3)
    let mut alive: HashSet<BlockId> = HashSet::new();
    // a container kept over the other side's deletion is kept WHOLE, as its side has it (walk() gives parents first)
    let mut whole: HashSet<BlockId> = HashSet::new();
    for b in mine.walk() {
        let id = b.id;
        if theirs.get(id).is_some() || base.get(id).is_none() { alive.insert(id); }
        else if b.parent.is_some_and(|p| whole.contains(&p)) || m.touched(id) { alive.insert(id); whole.insert(id); if m.changed(id) { notes.push(Note::KeptOverDelete(id)); } }
    }
    for b in theirs.walk() {
        let id = b.id;
        if mine.get(id).is_some() { continue }
        if base.get(id).is_none() { alive.insert(id); }
        else if b.parent.is_some_and(|p| whole.contains(&p)) || t.touched(id) { alive.insert(id); whole.insert(id); if t.changed(id) { notes.push(Note::KeptOverDelete(id)); } }
    }

    // content (rule 1) and parent (rule 4)
    let mut content: HashMap<BlockId, Block> = HashMap::new(); let mut parent: HashMap<BlockId, Option<BlockId>> = HashMap::new();
    let mut doubles: Vec<(BlockId, Block)> = Vec::new();
    for id in &alive {
        let (a, b, o) = (mine.get(*id), theirs.get(*id), base.get(*id));
        let pick = match (a, b) {
            (Some(a), Some(b)) => {
                let (am, bm) = (o.is_none_or(|o| !same(a, o)), o.is_none_or(|o| !same(b, o)));
                if am && bm && !same(a, b) {
                    if a.kind.is_text() && b.kind.is_text() { let mut copy = b.clone(); copy.id = BlockId::new(); copy.conflict = Some(*id); copy.children.clear(); notes.push(Note::BothEdited { mine: *id, theirs: copy.id }); doubles.push((*id, copy)); }
                    else { notes.push(Note::MineKept(*id)); }
                    a
                } else if am { a } else { b }
            }
            (Some(a), None) => a, (None, Some(b)) => b, (None, None) => continue,
        };
        content.insert(*id, pick.clone());
        let base_parent = o.map(|o| o.parent);
        let p = match (a, b) {
            (Some(a), Some(b)) => { let (am, bm) = (Some(a.parent) != base_parent, Some(b.parent) != base_parent); if am && bm && a.parent != b.parent { notes.push(Note::MovedByBoth(*id)); } if am { a.parent } else { b.parent } }
            (Some(a), None) => a.parent, (None, Some(b)) => b.parent, _ => None,
        };
        parent.insert(*id, p);
    }
    // a parent that did not survive: the block goes to the end of the note
    let mut strays: Vec<BlockId> = Vec::new();
    for (id, p) in parent.iter_mut() { if p.is_some_and(|p| !alive.contains(&p)) { *p = None; strays.push(*id); } }
    strays.sort();

    // order within each list (rule 4)
    let order = |p: Option<BlockId>| -> Vec<BlockId> {
        let here = |d: &Doc| -> Vec<BlockId> { d.children(p).iter().filter(|c| alive.contains(*c) && parent.get(*c) == Some(&p) && !strays.contains(*c)).copied().collect() };
        let (ml, tl, bl) = (here(mine), here(theirs), base.children(p));
        let (mm, tm) = (moved(bl, &ml), moved(bl, &tl));
        let replace: HashSet<BlockId> = tl.iter().filter(|x| tm.contains(*x) && !mm.contains(*x)).copied().collect();
        let mut out: Vec<BlockId> = ml.into_iter().filter(|x| !replace.contains(x)).collect();
        let mut after: Option<BlockId> = None;
        for x in tl {
            if !out.contains(&x) { let at = after.and_then(|a| out.iter().position(|y| *y == a)).map(|i| i + 1).unwrap_or(0);
                // mine first: step over blocks I added at this same place
                let mut at = at; while at < out.len() && base.get(out[at]).is_none() && theirs.get(out[at]).is_none() { at += 1; }
                out.insert(at, x); }
            after = Some(x);
        }
        out
    };

    // build, top down
    let mut doc = Doc::new(); let mut todo: Vec<Option<BlockId>> = vec![None]; let mut failed: Vec<BlockId> = Vec::new();
    let put = |doc: &mut Doc, p: Option<BlockId>, b: &Block| -> bool {
        let at = doc.children(p).len();
        if doc.insert(p, at, b.id, b.kind.clone(), b.text.clone()).is_err() { return false }
        let _ = doc.set_spans(b.id, b.spans.clone()); let _ = doc.set_conflict(b.id, b.conflict); true
    };
    while let Some(p) = todo.pop() {
        for id in order(p) {
            let b = &content[&id];
            if put(&mut doc, p, b) { todo.push(Some(id)); for (_, copy) in doubles.iter().filter(|(of, _)| *of == id) { put(&mut doc, p, copy); } } else { failed.push(id); }
        }
    }
    for id in strays.iter().copied().chain(failed) {
        if doc.get(id).is_some() { continue }
        if put(&mut doc, None, &content[&id]) { notes.push(Note::Misplaced(id)); let mut sub = vec![Some(id)];
            while let Some(p) = sub.pop() { for c in order(p) { if put(&mut doc, p, &content[&c]) { sub.push(Some(c)); } else { notes.push(Note::Lost(c)); } } } }
        else { notes.push(Note::Lost(id)); }
    }

    // tables (rule 5)
    let tables: Vec<BlockId> = doc.walk().iter().filter(|b| matches!(b.kind, Kind::Table { .. })).map(|b| b.id).collect();
    for table in tables {
        let rows: Vec<BlockId> = doc.children(Some(table)).to_vec();
        let Some(Kind::Table { header_rows, mut columns }) = doc.get(table).map(|b| b.kind.clone()) else { continue };
        let width = rows.iter().map(|r| doc.children(Some(*r)).len()).max().unwrap_or(0).max(columns.len());
        let mut squared = false;
        for row in &rows { for c in doc.children(Some(*row)).len()..width { let cell = BlockId::new(); if doc.insert(Some(*row), c, cell, Kind::cell(), "").is_ok() { let _ = doc.insert(Some(cell), 0, BlockId::new(), Kind::Paragraph, ""); squared = true; } } }
        if columns.len() < width || header_rows as usize > rows.len() { columns.resize(width, ColumnSpec::default()); let _ = doc.set_kind(table, Kind::Table { header_rows: header_rows.min(rows.len() as u8), columns }); }
        if squared { notes.push(Note::Squared(table)); }
    }
    notes.sort_by_key(|n| format!("{n:?}")); notes.dedup();
    Merged { doc, notes }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inline::{Mark, Spans};
    fn doc(texts: &[&str]) -> (Doc, Vec<BlockId>) {
        let mut d = Doc::new(); let ids: Vec<BlockId> = texts.iter().map(|_| BlockId::new()).collect();
        for (i, t) in texts.iter().enumerate() { d.insert(None, i, ids[i], Kind::Paragraph, *t).unwrap(); }
        (d, ids)
    }
    fn add(d: &mut Doc, parent: Option<BlockId>, at: usize, kind: Kind, text: &str) -> BlockId { let id = BlockId::new(); d.insert(parent, at, id, kind, text).unwrap(); id }
    fn texts(d: &Doc) -> Vec<String> { d.walk().iter().filter(|b| b.kind.is_text()).map(|b| b.text.clone()).collect() }

    #[test]
    fn with_one_side_unchanged_the_merge_is_the_other_side() {
        let (base, ids) = doc(&["a", "b", "c"]);
        let mut x = base.clone(); x.set_text(ids[1], "B").unwrap(); x.remove(ids[2]).unwrap(); add(&mut x, None, 0, Kind::Heading { level: 1 }, "new"); x.move_to(ids[0], None, 2).unwrap();
        for (m, t) in [(&x, &base), (&base, &x), (&x, &x)] { let r = merge(&base, m, t); assert_eq!((r.doc, r.notes), (x.clone(), vec![])); }
    }
    #[test]
    fn edits_to_different_blocks_both_land() {
        let (base, ids) = doc(&["a", "b", "c"]);
        let (mut m, mut t) = (base.clone(), base.clone());
        m.set_text(ids[0], "A mine").unwrap();
        t.set_text(ids[2], "C theirs").unwrap(); let mut s = Spans::new(); s.put(0, 1, Mark::Bold); t.set_spans(ids[1], s.clone()).unwrap(); t.set_kind(ids[1], Kind::Quote).unwrap();
        let r = merge(&base, &m, &t);
        assert_eq!((texts(&r.doc), r.notes.len()), (vec!["A mine".to_string(), "b".into(), "C theirs".into()], 0));
        assert_eq!((&r.doc.get(ids[1]).unwrap().kind, &r.doc.get(ids[1]).unwrap().spans), (&Kind::Quote, &s));
    }
    /// Row 283, ruling 4.
    #[test]
    fn two_edits_of_one_block_are_both_kept_the_second_marked() {
        let (base, ids) = doc(&["a", "b", "c"]);
        let (mut m, mut t) = (base.clone(), base.clone());
        m.set_text(ids[1], "b, mine").unwrap(); t.set_text(ids[1], "b, theirs").unwrap();
        let r = merge(&base, &m, &t);
        assert_eq!(texts(&r.doc), ["a", "b, mine", "b, theirs", "c"]);
        let copy = r.doc.children(None)[2];
        assert_eq!((r.doc.get(copy).unwrap().conflict, r.doc.get(ids[1]).unwrap().conflict), (Some(ids[1]), None));
        assert_eq!(r.notes, [Note::BothEdited { mine: ids[1], theirs: copy }]);
        // the same edit on both sides is one edit
        t.set_text(ids[1], "b, mine").unwrap();
        let r = merge(&base, &m, &t); assert_eq!((texts(&r.doc).len(), r.notes.len()), (3, 0));
    }
    #[test]
    fn a_deletion_loses_to_an_edit_and_wins_over_an_untouched_block() {
        let (base, ids) = doc(&["a", "b", "c", "d"]);
        let (mut m, mut t) = (base.clone(), base.clone());
        m.remove(ids[0]).unwrap(); t.set_text(ids[0], "a, edited").unwrap();      // I delete, they edit → stays
        t.remove(ids[1]).unwrap(); m.set_text(ids[1], "b, edited").unwrap();      // they delete, I edit → stays
        m.remove(ids[2]).unwrap();                                                // I delete, they left it → gone
        m.remove(ids[3]).unwrap(); t.remove(ids[3]).unwrap();                     // both delete
        let r = merge(&base, &m, &t);
        assert_eq!(texts(&r.doc), ["a, edited", "b, edited"]);
        assert_eq!(r.notes.len(), 2);
        assert!(r.notes.contains(&Note::KeptOverDelete(ids[0])) && r.notes.contains(&Note::KeptOverDelete(ids[1])));
    }
    #[test]
    fn additions_keep_their_places_and_mine_come_first_at_a_shared_place() {
        let (base, ids) = doc(&["a", "b", "c"]);
        let (mut m, mut t) = (base.clone(), base.clone());
        add(&mut m, None, 1, Kind::Paragraph, "mine after a"); add(&mut m, None, 4, Kind::Paragraph, "mine at the end");
        add(&mut t, None, 1, Kind::Paragraph, "theirs after a"); add(&mut t, None, 0, Kind::Paragraph, "theirs first"); add(&mut t, None, 4, Kind::Paragraph, "theirs after b");
        let r = merge(&base, &m, &t);
        assert_eq!(texts(&r.doc), ["theirs first", "a", "mine after a", "theirs after a", "b", "theirs after b", "c", "mine at the end"]);
        assert!(r.notes.is_empty());
        let _ = ids;
    }
    #[test]
    fn a_move_on_one_side_meets_an_edit_on_the_other() {
        let (base, ids) = doc(&["a", "b", "c", "d", "e"]);
        let (mut m, mut t) = (base.clone(), base.clone());
        m.move_to(ids[4], None, 0).unwrap();                                      // I move e to the top
        t.set_text(ids[4], "e, edited").unwrap();                                 // they edit it
        t.move_to(ids[0], None, 2).unwrap();                                      // they move a after c
        let r = merge(&base, &m, &t);
        assert_eq!(texts(&r.doc), ["e, edited", "b", "c", "a", "d"]);
        assert!(r.notes.is_empty());
        // both move the same block: mine stands
        let (mut m, mut t) = (base.clone(), base.clone());
        m.move_to(ids[0], None, 4).unwrap(); t.move_to(ids[0], None, 2).unwrap();
        assert_eq!(texts(&merge(&base, &m, &t).doc), ["b", "c", "d", "e", "a"]);
    }
    #[test]
    fn a_deleted_container_keeps_what_the_other_side_did_inside_it() {
        let mut base = Doc::new();
        let cols = add(&mut base, None, 0, Kind::Columns, ""); let (c1, c2) = (add(&mut base, Some(cols), 0, Kind::column(), ""), add(&mut base, Some(cols), 1, Kind::column(), ""));
        let (x, y) = (add(&mut base, Some(c1), 0, Kind::Paragraph, "x"), add(&mut base, Some(c2), 0, Kind::Paragraph, "y"));
        let tail = add(&mut base, None, 1, Kind::Paragraph, "tail");
        // they delete the whole Columns block; I edited one paragraph inside it → all of it stays
        let (mut m, mut t) = (base.clone(), base.clone());
        t.remove(cols).unwrap(); m.set_text(x, "x, edited").unwrap();
        let r = merge(&base, &m, &t);
        assert_eq!((texts(&r.doc), r.notes), (vec!["x, edited".to_string(), "y".into(), "tail".into()], vec![Note::KeptOverDelete(x)]));
        assert_eq!(r.doc.get(y).unwrap().parent, Some(c2));
        // they delete it; I only moved y out of it → y lives on where I put it, the rest goes
        let (mut m, mut t) = (base.clone(), base.clone());
        t.remove(cols).unwrap(); m.move_to(y, None, 2).unwrap();
        let r = merge(&base, &m, &t);
        assert_eq!((texts(&r.doc), r.doc.get(cols)), (vec!["tail".to_string(), "y".into()], None));
        // I move a paragraph INTO a container they delete → it survives, at the end of the note
        let (mut m, mut t) = (base.clone(), base.clone());
        m.move_to(tail, Some(c1), 1).unwrap(); t.remove(cols).unwrap();
        let r = merge(&base, &m, &t);
        assert!(r.doc.get(tail).is_some() && r.doc.get(cols).is_some());            // my change inside keeps the container
    }
    /// Rule 5: I add a column, they add a row.
    #[test]
    fn a_table_is_squared_off_after_a_merge() {
        let mut base = Doc::new();
        let table = add(&mut base, None, 0, Kind::Table { header_rows: 1, columns: vec![ColumnSpec::default(); 2] }, "");
        let row = |d: &mut Doc, at: usize, cells: &[&str]| -> BlockId { let r = add(d, Some(table), at, Kind::row(), ""); for (i, c) in cells.iter().enumerate() { let cell = add(d, Some(r), i, Kind::cell(), ""); add(d, Some(cell), 0, Kind::Paragraph, c); } r };
        let (r0, r1) = (row(&mut base, 0, &["h1", "h2"]), row(&mut base, 1, &["a1", "a2"]));
        let (mut m, mut t) = (base.clone(), base.clone());
        for (r, text) in [(r0, "h3"), (r1, "a3")] { let cell = add(&mut m, Some(r), 2, Kind::cell(), ""); add(&mut m, Some(cell), 0, Kind::Paragraph, text); }
        m.set_kind(table, Kind::Table { header_rows: 1, columns: vec![ColumnSpec::default(); 3] }).unwrap();
        let r2 = row(&mut t, 2, &["b1", "b2"]);
        let r = merge(&base, &m, &t);
        assert_eq!(texts(&r.doc), ["h1", "h2", "h3", "a1", "a2", "a3", "b1", "b2", ""]);
        assert_eq!((r.doc.children(Some(r2)).len(), r.notes), (3, vec![Note::Squared(table)]));
        assert!(matches!(&r.doc.get(table).unwrap().kind, Kind::Table { columns, .. } if columns.len() == 3));
    }
    #[test]
    fn the_moved_set_is_what_left_the_longest_kept_order() {
        let ids: Vec<BlockId> = (0..6).map(|_| BlockId::new()).collect();
        let list = [ids[5], ids[0], ids[1], ids[3], ids[2], ids[4]];
        let m = moved(&ids, &list);
        assert!(m.contains(&ids[5]) && m.len() == 2 && (m.contains(&ids[2]) || m.contains(&ids[3])));
        let new = BlockId::new();
        assert_eq!(moved(&ids[..2], &[ids[0], new, ids[1]]), HashSet::from([new]));
    }
}
