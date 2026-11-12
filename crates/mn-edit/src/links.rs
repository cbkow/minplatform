//! Links in text (canon row 320): made over a selection or at the caret, changed, removed; a typed or pasted bare
//! address becomes a link; what was typed as an address is put right (`www.` gets `https://`, an e-mail address
//! `mailto:`). A link is a span (`Mark::Link`) - never in a code block.
use mn_doc::{BlockId, ChangeError, Kind, Mark, MarkKind};
use crate::{Dirty, Editor, Pos, Selection};

fn scheme(s: &str) -> Option<&str> { let i = s.find(':')?; let (a, _) = s.split_at(i); (a.len() > 1 && a.chars().next()?.is_ascii_alphabetic() && a.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '.' | '-'))).then_some(a) }
/// An address as it is kept: a scheme is added where none was typed.
pub fn normalise_url(s: &str) -> String {
    let t = s.trim(); if t.is_empty() { return String::new() }
    if scheme(t).is_some() { return t.to_string() }
    if t.starts_with('/') { return format!("file://{t}") }
    if t.contains('@') && !t.contains('/') && !t.contains(char::is_whitespace) { return format!("mailto:{t}") }
    format!("https://{t}")
}
/// The app opens it (the web, mail, a file, and the family's own links); anything else is kept but not opened.
pub fn openable(url: &str) -> bool { scheme(url).is_some_and(|s| matches!(s.to_ascii_lowercase().as_str(), "http" | "https" | "mailto" | "file" | "minnotes" | "qcview" | "ufb")) }
/// A word that is an address on its face.
pub fn looks_like_url(w: &str) -> bool {
    if w.contains(char::is_whitespace) { return false } let l = w.to_ascii_lowercase();
    ["http://", "https://", "www."].iter().any(|p| l.starts_with(p) && l.len() > p.len() + 2 && l[p.len()..].contains('.') && !l.ends_with('.'))
}

impl Editor {
    /// The link at a place in a block: (its start, its end, its address).
    pub fn link_span_at(&self, block: BlockId, at: usize) -> Option<(usize, usize, String)> {
        self.doc.get(block)?.spans.as_slice().iter().find_map(|s| match &s.mark { Mark::Link(u) if s.start <= at && at <= s.end => Some((s.start, s.end, u.clone())), _ => None })
    }
    /// The link the caret is in (or the selection starts in): (the block, its start, its end, its address, its text).
    pub fn link_here(&self) -> Option<(BlockId, usize, usize, String, String)> {
        let (a, z) = self.ordered()?; let (s, e, u) = self.link_span_at(a.block, a.at)?; if a.block != z.block || z.at > e { return None }
        Some((a.block, s, e, u, self.doc.get(a.block)?.text.get(s..e)?.to_string()))
    }
    fn linked(&mut self, text: &str, url: &str, now_ms: u64) -> Result<Dirty, ChangeError> { self.armed = vec![(Mark::Link(url.to_string()), true)]; let d = self.insert(text, now_ms); self.armed.clear(); d }
    /// The selection (or the link the caret is in) gets this address; `text`: its words too. At a bare caret the
    /// words (or the address) are put in as a link. An empty address removes the link.
    pub fn set_link(&mut self, url: &str, text: Option<&str>, now_ms: u64) -> Result<Dirty, ChangeError> {
        let url = normalise_url(url); if url.is_empty() { return self.remove_link(now_ms) }
        let Some((a, z)) = self.ordered() else { return Ok(Dirty::Nothing) }; if matches!(self.doc.get(a.block).map(|b| &b.kind), Some(Kind::Code { .. })) { return Ok(Dirty::Nothing) }
        let text = text.map(str::trim).filter(|t| !t.is_empty());
        if let Some((id, s, e, _, was)) = self.link_here() {
            if let Some(t) = text.filter(|t| *t != was) { self.sel = Some(Selection { anchor: Pos { block: id, at: s }, focus: Pos { block: id, at: e } }); return self.linked(t, &url, now_ms) }
            return self.format(vec![(id, s, e)], |sp, s, e| sp.put(s, e, Mark::Link(url.clone())), now_ms) }
        if a == z { return self.linked(text.unwrap_or(&url), &url, now_ms) }
        if let Some(t) = text.filter(|t| a.block == z.block && *t != self.selected_text()) { return self.linked(t, &url, now_ms) }
        let ranges = self.text_ranges(a, z); self.format(ranges, |sp, s, e| sp.put(s, e, Mark::Link(url.clone())), now_ms)
    }
    /// The link the caret is in, or the links in the selection, become plain text.
    pub fn remove_link(&mut self, now_ms: u64) -> Result<Dirty, ChangeError> {
        if let Some((id, s, e, ..)) = self.link_here() { return self.format(vec![(id, s, e)], |sp, s, e| sp.remove(s, e, MarkKind::Link), now_ms) }
        let Some((a, z)) = self.ordered() else { return Ok(Dirty::Nothing) }; let ranges = self.text_ranges(a, z); self.format(ranges, |sp, s, e| sp.remove(s, e, MarkKind::Link), now_ms)
    }
    /// The word that ends at the caret (`after_space`: before the space just typed) becomes a link when it is an
    /// address on its face - a step of its own, so one undo makes it text again.
    pub fn autolink(&mut self, after_space: bool, now_ms: u64) -> Result<Dirty, ChangeError> {
        let Some(s) = self.sel.filter(|s| s.is_caret()) else { return Ok(Dirty::Nothing) }; let Some(blk) = self.doc.get(s.focus.block) else { return Ok(Dirty::Nothing) };
        if !blk.kind.is_text() || matches!(blk.kind, Kind::Code { .. }) { return Ok(Dirty::Nothing) }
        let mut end = s.focus.at.min(blk.text.len()); if after_space { match blk.text[..end].chars().next_back() { Some(c) if c.is_whitespace() => end -= c.len_utf8(), _ => return Ok(Dirty::Nothing) } }
        let start = blk.text[..end].rfind(char::is_whitespace).map(|i| i + blk.text[i..].chars().next().map(char::len_utf8).unwrap_or(1)).unwrap_or(0);
        let word = blk.text[start..end].trim_end_matches(['.', ',', ';', ':', '!', '?', ')']); let end = start + word.len();
        if !looks_like_url(word) || blk.spans.as_slice().iter().any(|sp| matches!(sp.mark, Mark::Link(_) | Mark::Code | Mark::Choice(_)) && sp.start < end && sp.end > start) { return Ok(Dirty::Nothing) }
        let url = normalise_url(word); let id = blk.id;
        self.format(vec![(id, start, end)], |sp, s, e| sp.put(s, e, Mark::Link(url.clone())), now_ms + mn_doc::change::JOIN_MS + 1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mn_doc::Doc;
    #[test]
    fn addresses_are_put_right_and_links_are_made_changed_and_removed() {
        assert_eq!(normalise_url(" www.example.com "), "https://www.example.com"); assert_eq!(normalise_url("someone@example.com"), "mailto:someone@example.com");
        assert_eq!(normalise_url("http://a.b"), "http://a.b"); assert_eq!(normalise_url("/tmp/x"), "file:///tmp/x"); assert_eq!(normalise_url(""), "");
        assert!(openable("https://a.b") && openable("mailto:x@y.z") && !openable("javascript:alert(1)") && !openable("no scheme"));
        assert!(looks_like_url("www.example.com") && looks_like_url("https://example.com/a?b") && !looks_like_url("example.com") && !looks_like_url("www.") && !looks_like_url("https://x"));
        let mut d = Doc::new(); let p = BlockId::new(); d.insert(None, 0, p, Kind::Paragraph, "see the site today").unwrap(); let c = BlockId::new(); d.insert(None, 1, c, Kind::Code { lang: String::new() }, "www.example.com").unwrap();
        let mut e = Editor::new(d); let link = |e: &Editor| e.doc.get(p).unwrap().spans.as_slice().iter().find_map(|s| match &s.mark { Mark::Link(u) => Some((s.start, s.end, u.clone())), _ => None });
        // over a selection; the caret inside it finds it; a new address and new words replace it; removed
        e.set_selection(Some(Selection { anchor: Pos { block: p, at: 8 }, focus: Pos { block: p, at: 12 } })); e.set_link("example.com", None, 1_000).unwrap(); assert_eq!(link(&e), Some((8, 12, "https://example.com".into())));
        e.set_selection(Some(Selection::caret(Pos { block: p, at: 10 }))); assert_eq!(e.link_here().map(|x| x.4), Some("site".into()));
        e.set_link("https://example.org", Some("web site"), 9_000).unwrap(); assert_eq!(e.doc.get(p).unwrap().text, "see the web site today"); assert_eq!(link(&e), Some((8, 16, "https://example.org".into())));
        e.set_selection(Some(Selection::caret(Pos { block: p, at: 9 }))); e.remove_link(17_000).unwrap(); assert_eq!(link(&e), None);
        // at a bare caret the address is its own words; typing after it is not linked
        e.set_selection(Some(Selection::caret(Pos { block: p, at: 0 }))); e.set_link("www.example.com", None, 25_000).unwrap(); e.insert(" x ", 33_000).unwrap();
        assert!(e.doc.get(p).unwrap().text.starts_with("https://www.example.com x see")); assert_eq!(link(&e), Some((0, 23, "https://www.example.com".into())));
        // a typed address becomes a link after its space, as a step of its own; not in a code block
        let end = e.doc.get(p).unwrap().text.len(); e.set_selection(Some(Selection::caret(Pos { block: p, at: end }))); e.insert(" www.minnotes.app. ", 41_000).unwrap(); assert_ne!(e.autolink(true, 41_000).unwrap(), Dirty::Nothing);
        let t = e.doc.get(p).unwrap().text.clone(); let s = t.find("www.minnotes").unwrap(); assert!(e.doc.get(p).unwrap().spans.as_slice().iter().any(|x| x.start == s && x.end == s + 16 && x.mark == Mark::Link("https://www.minnotes.app".into())));
        e.undo().unwrap(); assert!(!e.doc.get(p).unwrap().spans.as_slice().iter().any(|x| x.start == s)); assert_eq!(e.doc.get(p).unwrap().text, t);
        e.set_selection(Some(Selection::caret(Pos { block: c, at: 15 }))); assert_eq!(e.autolink(false, 50_000).unwrap(), Dirty::Nothing); assert_eq!(e.set_link("a.b", None, 51_000).unwrap(), Dirty::Nothing);
    }
}
