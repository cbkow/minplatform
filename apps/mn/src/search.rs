//! Search across the collection (canon row 321): the sidebar's field. Every note is read and looked through a few at a
//! time between frames (no index, nothing added to the file); a note is a result when it holds EVERY word of what was
//! typed, in any order ("words in quotes" stay together). Notes whose name holds the words come first, the rest in the
//! tree's order.
use mn_doc::{BlockId, Doc, Kind};
use mn_edit::find::{find_in, holds, HitKind};
use mn_store::{Collection, DocKind};

/// A line of a note around an occurrence: its text, the occurrence within it, and how to get to it (the word found
/// and which of its occurrences in the note this is).
#[derive(Clone, Debug)]
pub struct Snip { pub text: String, pub bold: (usize, usize), pub term: String, pub ord: usize }
/// A note that holds the words.
#[derive(Clone, Debug)]
pub struct Found { pub id: BlockId, pub title: String, pub in_title: bool, pub count: usize, pub snippets: Vec<Snip> }

/// What was typed, as the words to find: quoted runs stay whole.
pub fn terms(q: &str) -> Vec<String> {
    let (mut out, mut cur, mut quoted) = (vec![], String::new(), false);
    for c in q.chars() { match c { '"' | '\u{201c}' | '\u{201d}' => { if !cur.trim().is_empty() { out.push(cur.trim().to_string()); } cur.clear(); quoted = !quoted; } c if c.is_whitespace() && !quoted => { if !cur.is_empty() { out.push(std::mem::take(&mut cur)); } } c => cur.push(c) } }
    if !cur.trim().is_empty() { out.push(cur.trim().to_string()); }
    out
}
/// One note looked through. `extra`: other text that counts as the note's (its videos' notes). None: not a result.
pub fn look(id: BlockId, title: &str, doc: &Doc, extra: &[String], terms: &[String]) -> Option<Found> {
    if terms.is_empty() { return None }
    let mut all = String::new(); for b in doc.walk() { if b.kind.is_text() { all.push_str(&b.text); all.push('\n'); } if let Kind::Media { descriptor } = &b.kind { all.push_str(descriptor); all.push('\n'); } }
    if !terms.iter().all(|t| holds(title, t, false) || holds(&all, t, false) || extra.iter().any(|x| holds(x, t, false))) { return None }
    let order: Vec<BlockId> = doc.walk().iter().map(|b| b.id).collect(); let place = |b: BlockId| order.iter().position(|x| *x == b).unwrap_or(0);
    let mut hits: Vec<(usize, usize, usize, &String, usize, HitKind, BlockId)> = vec![];   // (the block's place, start, end, the word, which occurrence, its kind, the block)
    for t in terms { for (ord, h) in find_in(doc, t, false).into_iter().enumerate() { hits.push((place(h.block), h.start, h.end, t, ord, h.kind, h.block)); } }
    hits.sort_by_key(|h| (h.0, h.1));
    let snippets = hits.iter().filter(|h| h.5 == HitKind::Text).take(3).filter_map(|h| { let text = &doc.get(h.6)?.text; let (mut a, mut z) = (h.1.saturating_sub(28), (h.2 + 44).min(text.len()));
        while !text.is_char_boundary(a) { a -= 1; } while !text.is_char_boundary(z) { z += 1; }
        let lead = if a > 0 { "\u{2026}" } else { "" }; let clean = |s: &str| s.replace(['\n', '\t'], " ");
        let before = format!("{lead}{}", clean(&text[a..h.1]).trim_start()); let mid = clean(&text[h.1..h.2]);
        Some(Snip { bold: (before.len(), before.len() + mid.len()), text: format!("{before}{mid}{}{}", clean(&text[h.2..z]), if z < text.len() { "\u{2026}" } else { "" }), term: h.3.clone(), ord: h.4 }) }).collect();
    Some(Found { id, title: title.to_string(), in_title: terms.iter().all(|t| holds(title, t, false)), count: hits.len(), snippets })
}

/// A search under way: the notes still to read, and what was found so far.
pub struct Search { pub query: String, terms: Vec<String>, todo: Vec<(BlockId, String)>, at: usize, pub found: Vec<Found> }
impl Search {
    /// Begins: the collection's notes in the tree's order.
    pub fn new(query: &str, col: &Collection) -> Search {
        fn walk(col: &Collection, parent: Option<BlockId>, out: &mut Vec<(BlockId, String)>) { for e in col.list(parent).unwrap_or_default() {
            if e.kind == DocKind::Note { let t = if !e.title.is_empty() { e.title.clone() } else { col.first_line(e.id).unwrap_or_default() }; out.push((e.id, if t.is_empty() { "Untitled".to_string() } else { t })); }
            if e.kind == DocKind::Folder { walk(col, Some(e.id), out); } } }
        let mut todo = vec![]; walk(col, None, &mut todo);
        Search { query: query.to_string(), terms: terms(query), todo, at: 0, found: vec![] }
    }
    pub fn done(&self) -> bool { self.at >= self.todo.len() }
    /// The next note to look through.
    pub fn next(&self) -> Option<(BlockId, &str)> { self.todo.get(self.at).map(|(id, t)| (*id, t.as_str())) }
    /// It was looked through (`doc`: None when it could not be read).
    pub fn took(&mut self, doc: Option<&Doc>, extra: &[String]) {
        let Some((id, title)) = self.todo.get(self.at).cloned() else { return }; self.at += 1;
        if let Some(f) = doc.and_then(|d| look(id, &title, d, extra, &self.terms)) { self.found.push(f); }
    }
    /// What was found, the notes named by the words first.
    pub fn results(&self) -> Vec<Found> { let mut v = self.found.clone(); v.sort_by_key(|f| !f.in_title); v }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn a_note_is_a_result_when_it_holds_every_word() {
        assert_eq!(terms(r#"audio "small speakers"  mix"#), vec!["audio", "small speakers", "mix"]); assert!(terms("  ").is_empty());
        let mut d = Doc::new(); let (a, b) = (BlockId::new(), BlockId::new());
        d.insert(None, 0, a, Kind::Paragraph, "Check the audio mix on small speakers before the review.").unwrap(); d.insert(None, 1, b, Kind::Paragraph, "The mix again, and the Mix once more.").unwrap();
        let f = look(a, "Review notes", &d, &[], &terms("mix audio")).expect("both words are there"); assert_eq!(f.count, 4); assert!(!f.in_title);
        assert_eq!(f.snippets.len(), 3); let s = &f.snippets[0]; assert_eq!(&s.text[s.bold.0..s.bold.1], "audio"); assert_eq!((s.term.as_str(), s.ord), ("audio", 0));
        assert_eq!((f.snippets[2].term.as_str(), f.snippets[2].ord), ("mix", 1));
        assert!(look(a, "Review notes", &d, &[], &terms("mix grade")).is_none()); assert!(look(a, "Review notes", &d, &[], &terms(r#""speakers small""#)).is_none());
        // a word in the name, a word in a video's note
        let f = look(a, "Review notes", &d, &["hold the grade".into()], &terms("review grade")).unwrap(); assert_eq!(f.count, 1); assert!(!f.in_title);
        assert!(look(a, "Review notes", &d, &[], &terms("notes review")).unwrap().in_title);
    }
}
