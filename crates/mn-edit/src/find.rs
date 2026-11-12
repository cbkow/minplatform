//! Find in a note (canon row 321): every occurrence of some text, case and accents set aside (or the case kept), in
//! the note's own order; replacing them. The same matching serves the search across a collection.
use mn_doc::{BlockId, Change, ChangeError, ColumnKind, Kind, Mark};
use crate::{leaves, typed::typed_of, Dirty, Editor};

/// A letter without its accent (the Latin letters a note is likely to hold; others are themselves).
fn base(c: char) -> char {
    match c {
        'à'..='å' | 'ā' | 'ă' | 'ą' => 'a', 'ç' | 'ć' | 'ĉ' | 'ċ' | 'č' => 'c', 'ď' | 'đ' => 'd', 'è'..='ë' | 'ē' | 'ĕ' | 'ė' | 'ę' | 'ě' => 'e', 'ĝ' | 'ğ' | 'ġ' | 'ģ' => 'g', 'ĥ' | 'ħ' => 'h',
        'ì'..='ï' | 'ĩ' | 'ī' | 'ĭ' | 'į' | 'ı' => 'i', 'ĵ' => 'j', 'ķ' => 'k', 'ĺ' | 'ļ' | 'ľ' | 'ŀ' | 'ł' => 'l', 'ñ' | 'ń' | 'ņ' | 'ň' => 'n', 'ò'..='ö' | 'ø' | 'ō' | 'ŏ' | 'ő' => 'o',
        'ŕ' | 'ŗ' | 'ř' => 'r', 'ś' | 'ŝ' | 'ş' | 'š' => 's', 'ţ' | 'ť' | 'ŧ' => 't', 'ù'..='ü' | 'ũ' | 'ū' | 'ŭ' | 'ů' | 'ű' | 'ų' => 'u', 'ŵ' => 'w', 'ý' | 'ÿ' | 'ŷ' => 'y', 'ź' | 'ż' | 'ž' => 'z', c => c,
    }
}
/// Text as it is compared, and for each of its bytes the (start, end) in the original of the character it came from.
pub fn fold(s: &str, case: bool) -> (String, Vec<(usize, usize)>) {
    let (mut out, mut map) = (String::with_capacity(s.len()), Vec::with_capacity(s.len()));
    for (i, c) in s.char_indices() { let end = i + c.len_utf8();
        if case { out.push(c); map.extend(std::iter::repeat_n((i, end), c.len_utf8())); continue }
        if ('\u{300}'..='\u{36f}').contains(&c) { continue }   // a combining accent
        for l in c.to_lowercase() { let b = base(l); out.push(b); map.extend(std::iter::repeat_n((i, end), b.len_utf8())); } }
    (out, map)
}
/// Every occurrence of `query` in `text` (one after another, never overlapping), as byte ranges of `text`.
pub fn occurrences(text: &str, query: &str, case: bool) -> Vec<(usize, usize)> {
    let (q, _) = fold(query, case); if q.is_empty() { return vec![] } let (t, map) = fold(text, case);
    let (mut out, mut from) = (vec![], 0);
    while let Some(i) = t[from..].find(&q) { let (a, z) = (from + i, from + i + q.len()); out.push((map[a].0, map[z - 1].1)); from = z; }
    out
}
/// Does the text hold it?
pub fn holds(text: &str, query: &str, case: bool) -> bool { let (q, _) = fold(query, case); !q.is_empty() && fold(text, case).0.contains(&q) }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HitKind { /// in the block's text
    Text, /// a link's address (the hit is the link's words)
    Link, /// the block as a whole (a file's or a picture's name, a video's note)
    Block }
/// One occurrence: a range of a block's text (`Block`: the whole block).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Hit { pub block: BlockId, pub start: usize, pub end: usize, pub kind: HitKind }

/// Every occurrence in a note, in its order: in the text of its blocks (cells, code and chips too), in links'
/// addresses, in the names of its pictures and files.
pub fn find_in(doc: &mn_doc::Doc, query: &str, case: bool) -> Vec<Hit> {
        let mut out = vec![]; if fold(query, case).0.is_empty() { return out }
        for id in leaves(doc) { let Some(b) = doc.get(id) else { continue };
            if let Kind::Media { descriptor } = &b.kind { let name = serde_json::from_str::<serde_json::Value>(descriptor).ok().and_then(|v| v.get("name").and_then(|n| n.as_str()).map(str::to_string)).unwrap_or_default();
                if holds(&name, query, case) { out.push(Hit { block: id, start: 0, end: 0, kind: HitKind::Block }); } continue }
            if !b.kind.is_text() { continue }
            let mut here: Vec<Hit> = occurrences(&b.text, query, case).into_iter().map(|(s, e)| Hit { block: id, start: s, end: e, kind: HitKind::Text }).collect();
            for sp in b.spans.as_slice() { if let Mark::Link(u) = &sp.mark { if holds(u, query, case) && !here.iter().any(|h| h.start < sp.end && h.end > sp.start) { here.push(Hit { block: id, start: sp.start, end: sp.end, kind: HitKind::Link }); } } }
            here.sort_by_key(|h| h.start); out.extend(here); }
        out
}

impl Editor {
    /// Every occurrence in the note (`find_in`).
    pub fn find(&self, query: &str, case: bool) -> Vec<Hit> { find_in(&self.doc, query, case) }
    /// A hit's text may be replaced: it is text, not in a chip, not in a choice or a check cell.
    pub fn replaceable(&self, h: &Hit) -> bool {
        if h.kind != HitKind::Text { return false } let Some(b) = self.doc.get(h.block) else { return false };
        if b.spans.as_slice().iter().any(|s| matches!(s.mark, Mark::Choice(_)) && s.start < h.end && s.end > h.start) { return false }
        !matches!(typed_of(&self.doc, h.block).map(|t| t.kind), Some(ColumnKind::Choice { .. } | ColumnKind::Check))
    }
    /// The hits that may be are replaced by `with`, as ONE undo step; how many were.
    pub fn replace_hits(&mut self, hits: &[Hit], with: &str, now_ms: u64) -> Result<(Dirty, usize), ChangeError> {
        let mut todo: Vec<Hit> = hits.iter().copied().filter(|h| self.replaceable(h)).collect(); if todo.is_empty() { return Ok((Dirty::Nothing, 0)) }
        let order = leaves(&self.doc); let place = |id: BlockId| order.iter().position(|x| *x == id).unwrap_or(0);
        todo.sort_by(|a, b| (place(b.block), b.start).cmp(&(place(a.block), a.start)));   // from the end: the earlier ones keep their places
        let n = todo.len(); let mut b = self.batch();
        for h in &todo { let blk = b.scratch.get(h.block).unwrap().clone(); if h.end > blk.text.len() || !blk.text.is_char_boundary(h.start) || !blk.text.is_char_boundary(h.end) { continue }
            let mut spans = blk.spans.clone(); spans.edit(h.start, h.end - h.start, with.len());
            b.push(Change::Text { id: h.block, at: h.start, removed: blk.text[h.start..h.end].to_string(), inserted: with.to_string(), old_spans: blk.spans.clone(), new_spans: spans })?; }
        let keep = self.sel; let d = self.commit(b, None, now_ms + mn_doc::change::JOIN_MS + 1)?; self.sel = keep.and_then(|s| self.live(s)); if let Some(m) = self.marks.last_mut() { m.1 = self.sel; }
        Ok((d, n))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Pos, Selection};
    use mn_doc::{Doc, Spans};
    #[test]
    fn occurrences_set_case_and_accents_aside_and_keep_their_places() {
        assert_eq!(occurrences("Café au lait, CAFE noir, cafe", "cafe", false), vec![(0, 5), (15, 19), (26, 30)]);
        assert_eq!(occurrences("Café au lait, CAFE noir, cafe", "cafe", true), vec![(26, 30)]);
        assert_eq!(occurrences("aaaa", "aa", false), vec![(0, 2), (2, 4)]); assert!(occurrences("abc", "", false).is_empty()); assert!(holds("Ünïcode", "unic", false) && !holds("abc", "", false));
    }
    #[test]
    fn a_note_is_searched_in_order_and_its_text_replaced_in_one_step() {
        let mut d = Doc::new(); let (p, q, m) = (BlockId::new(), BlockId::new(), BlockId::new());
        d.insert(None, 0, p, Kind::Paragraph, "The grade holds; see the site.").unwrap(); d.insert(None, 1, m, Kind::Media { descriptor: r#"{"kind":"file","name":"grade-notes.txt","src":"x"}"#.into() }, "").unwrap(); d.insert(None, 2, q, Kind::Paragraph, "Grade again: Done").unwrap();
        let mut s = Spans::new(); s.put(21, 29, Mark::Link("https://example.com/grade".into())); d.set_spans(p, s).unwrap();
        let mut s = Spans::new(); s.put(13, 17, Mark::Choice(r#"{"o":[{"c":"","id":"2","l":"Done"}],"v":"2"}"#.into())); d.set_spans(q, s).unwrap();
        let mut e = Editor::new(d); e.set_selection(Some(Selection::caret(Pos { block: p, at: 0 })));
        let hits = e.find("grade", false);
        assert_eq!(hits.iter().map(|h| (h.block, h.start, h.kind)).collect::<Vec<_>>(), vec![(p, 4, HitKind::Text), (p, 21, HitKind::Link), (m, 0, HitKind::Block), (q, 0, HitKind::Text)]);
        // only the text is replaced: not the link's address, not the file's name; a chip's label is left alone
        let (_, n) = e.replace_hits(&hits, "colour", 1_000).unwrap(); assert_eq!(n, 2); assert_eq!(e.doc.get(p).unwrap().text, "The colour holds; see the site."); assert_eq!(e.doc.get(q).unwrap().text, "colour again: Done");
        assert!(e.doc.get(p).unwrap().spans.as_slice().iter().any(|s| s.start == 22 && s.end == 30), "the link moved with its words");
        let done = e.find("done", false); assert_eq!(done.len(), 1); assert!(!e.replaceable(&done[0])); assert_eq!(e.replace_hits(&done, "x", 9_000).unwrap().1, 0);
        e.undo().unwrap(); assert_eq!(e.doc.get(q).unwrap().text, "Grade again: Done"); assert_eq!(e.doc.get(p).unwrap().text, "The grade holds; see the site.");
    }
}
