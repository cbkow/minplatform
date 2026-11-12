//! Choice chips in running text (canon row 320): a chip is a span (`Mark::Choice`) over its label, carrying its own
//! options (`{"o": [...], "v": id}`) - as the 2.0 files have it. A new chip takes the options of the chip before it
//! in the note (else To do / Doing / Done); a set can be changed on every chip that shares it. A table's typed cells
//! are not these (their column owns the options: `typed.rs`).
use mn_doc::{BlockId, Change, ChangeError, ChoiceOption, Kind, Mark, Spans};
use crate::{leaves, typed::{self, payload, typed_of}, Dirty, Editor, Pos, Selection};

/// A chip's payload read: its options and the chosen id.
pub fn read(p: &str) -> (Vec<ChoiceOption>, String) {
    let v: serde_json::Value = serde_json::from_str(p).unwrap_or_default(); let s = |x: &serde_json::Value, k: &str| x.get(k).and_then(|y| y.as_str()).unwrap_or("").to_string();
    (v.get("o").and_then(|o| o.as_array()).map(|o| o.iter().map(|x| ChoiceOption { id: s(x, "id"), label: s(x, "l"), color: s(x, "c") }).collect()).unwrap_or_default(), s(&v, "v"))
}
/// A chip in a text: (the block, its start, its end, its options, the chosen id).
pub type Chip = (BlockId, usize, usize, Vec<ChoiceOption>, String);
fn same_set(a: &[ChoiceOption], b: &[ChoiceOption]) -> bool { a.len() == b.len() && a.iter().all(|x| b.iter().any(|y| y.id == x.id)) }

impl Editor {
    /// The chip at a place of a block's text (its ends count) - not a typed cell's.
    pub fn chip_span_at(&self, block: BlockId, at: usize) -> Option<Chip> {
        if typed_of(&self.doc, block).is_some() { return None }
        self.doc.get(block)?.spans.as_slice().iter().find_map(|s| match &s.mark { Mark::Choice(p) if s.start <= at && at <= s.end => { let (o, v) = read(p); Some((block, s.start, s.end, o, v)) } _ => None })
    }
    /// The chip the caret stands just after.
    pub fn chip_before_caret(&self) -> Option<Chip> { let s = self.sel.filter(|s| s.is_caret())?; self.chip_span_at(s.focus.block, s.focus.at).filter(|c| c.2 == s.focus.at) }
    /// A chip goes in at the caret, with the options of the chip before it in the note (else To do / Doing / Done,
    /// under fresh ids) and its first option chosen; the caret after it. Not in a code block or a typed cell.
    pub fn insert_chip(&mut self, now_ms: u64) -> Result<Dirty, ChangeError> {
        let Some(s) = self.sel else { return Ok(Dirty::Nothing) }; let Some(blk) = self.doc.get(s.focus.block) else { return Ok(Dirty::Nothing) };
        if !blk.kind.is_text() || matches!(blk.kind, Kind::Code { .. }) || typed_of(&self.doc, blk.id).is_some() || self.blocks.is_some() || self.cell_rect().is_some() { return Ok(Dirty::Nothing) }
        let l = leaves(&self.doc); let here = l.iter().position(|x| *x == s.focus.block).unwrap_or(0);
        let before = l[..=here].iter().rev().find_map(|id| { let b = self.doc.get(*id)?; if typed_of(&self.doc, *id).is_some() { return None }
            b.spans.as_slice().iter().rev().find_map(|sp| match &sp.mark { Mark::Choice(p) if *id != s.focus.block || sp.end <= s.focus.at => Some(read(p).0), _ => None }) }).filter(|o| !o.is_empty());
        let options = before.unwrap_or_else(|| typed::check_options().into_iter().map(|mut o| { o.id = BlockId::new().to_string(); o }).collect());
        let first = options[0].clone(); self.armed = vec![(Mark::Choice(payload(&options, &first.id)), true)]; let d = self.insert(&first.label, now_ms + mn_doc::change::JOIN_MS + 1); self.armed.clear(); d
    }
    /// A chip takes an option (None: the chip goes), and - `options` - a new set of its own. The caret after it.
    pub fn chip_set(&mut self, chip: &Chip, option: Option<&str>, options: Option<&[ChoiceOption]>, now_ms: u64) -> Result<Dirty, ChangeError> {
        let (id, s, e, old, _) = chip; let set = options.unwrap_or(old);
        self.blocks = None; self.sel = Some(Selection { anchor: Pos { block: *id, at: *s }, focus: Pos { block: *id, at: *e } });
        match option.and_then(|o| set.iter().find(|x| x.id == o)) {
            Some(o) => { self.armed = vec![(Mark::Choice(payload(set, &o.id)), true)]; let label = o.label.clone(); let d = self.insert(&label, now_ms + mn_doc::change::JOIN_MS + 1); self.armed.clear(); d }
            None => self.delete(true, now_ms + mn_doc::change::JOIN_MS + 1),
        }
    }
    /// Every chip of the note that shares `old`'s set takes `new` (ONE undo step): its label and colour follow; a
    /// chip whose option is gone goes. Typed cells are left alone.
    pub fn chips_replace_set(&mut self, old: &[ChoiceOption], new: &[ChoiceOption], now_ms: u64) -> Result<Dirty, ChangeError> {
        let mut b = self.batch();
        for id in leaves(&self.doc) { if typed_of(&self.doc, id).is_some() { continue } let Some(blk) = self.doc.get(id) else { continue };
            let chips: Vec<(usize, usize, String)> = blk.spans.as_slice().iter().filter_map(|sp| match &sp.mark { Mark::Choice(p) => { let (o, v) = read(p); same_set(&o, old).then_some((sp.start, sp.end, v)) } _ => None }).collect();
            for (s, e, v) in chips.into_iter().rev() {   // from the end: the earlier ones keep their places
                let cur = b.scratch.get(id).unwrap().clone(); let label = new.iter().find(|o| o.id == v).map(|o| o.label.clone()).unwrap_or_default();
                let mut text = cur.text.clone(); text.replace_range(s..e, &label); let mut spans: Spans = cur.spans.clone(); spans.edit(s, e - s, 0); spans.edit(s, 0, label.len());
                if !label.is_empty() { spans.put(s, s + label.len(), Mark::Choice(payload(new, &v))); }
                b.push(Change::Text { id, at: 0, removed: cur.text.clone(), inserted: text, old_spans: cur.spans.clone(), new_spans: spans })?; } }
        let keep = self.sel; let d = self.commit(b, None, now_ms + mn_doc::change::JOIN_MS + 1)?; self.sel = keep.and_then(|s| self.live(s)); if let Some(m) = self.marks.last_mut() { m.1 = self.sel; } Ok(d)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mn_doc::Doc;
    #[test]
    fn chips_go_in_take_options_share_a_set_and_go() {
        let mut d = Doc::new(); let (p, q) = (BlockId::new(), BlockId::new()); d.insert(None, 0, p, Kind::Paragraph, "state: ").unwrap(); d.insert(None, 1, q, Kind::Paragraph, "and ").unwrap();
        let mut e = Editor::new(d); let mut t = 0u64; let mut now = || { t += 9_000; t };
        e.set_selection(Some(Selection::caret(Pos { block: p, at: 7 }))); e.insert_chip(now()).unwrap();
        assert_eq!(e.doc.get(p).unwrap().text, "state: To do"); let c = e.chip_before_caret().expect("the caret is after the chip"); assert_eq!((c.1, c.2, c.3.len()), (7, 12, 3)); assert_eq!(c.4, c.3[0].id);
        // another option: the label follows; typing after the chip is not part of it
        e.chip_set(&c, Some(&c.3[2].id), None, now()).unwrap(); assert_eq!(e.doc.get(p).unwrap().text, "state: Done"); e.insert(" ok", now()).unwrap();
        let c = e.chip_span_at(p, 8).unwrap(); assert_eq!((c.1, c.2), (7, 11));
        // a second chip takes the first's options (the same ids)
        e.set_selection(Some(Selection::caret(Pos { block: q, at: 4 }))); e.insert_chip(now()).unwrap(); let c2 = e.chip_before_caret().unwrap(); assert!(same_set(&c.3, &c2.3)); assert_eq!(e.doc.get(q).unwrap().text, "and To do");
        // the set is changed on both: a rename reaches the chip that has it, the chip whose option went goes
        let mut set = c.3.clone(); set[2].label = "Finished".into(); set.remove(0); e.chips_replace_set(&c.3, &set, now()).unwrap();
        assert_eq!(e.doc.get(p).unwrap().text, "state: Finished ok"); assert_eq!(e.doc.get(q).unwrap().text, "and "); assert_eq!(e.chip_span_at(p, 8).unwrap().3.len(), 2);
        e.undo().unwrap(); assert_eq!(e.doc.get(q).unwrap().text, "and To do"); assert_eq!(e.doc.get(p).unwrap().text, "state: Done ok");
        // removed
        let c = e.chip_span_at(p, 8).unwrap(); e.chip_set(&c, None, None, now()).unwrap(); assert_eq!(e.doc.get(p).unwrap().text, "state:  ok"); assert!(e.chip_span_at(p, 7).is_none());
    }
}
