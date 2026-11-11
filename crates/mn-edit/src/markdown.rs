//! Markdown as you type (canon row 289). The characters are typed first, as ordinary text; when one of them COMPLETES
//! a pattern the pattern is converted — by the changes built here, which the editor commits as a step of their own, so
//! one undo gives the typed characters back (for the writer who wanted the asterisks).
//!
//! Inline, on the closing marker: `**bold**` `__bold__` `*italic*` `_italic_` `` `code` `` `~~strike~~` `[label](url)`.
//! At the start of a paragraph, on the space after it: `#`…`######`, `>`, `-` `*` `+`, `[]` `[ ]` `[x]` `[/]` (also after
//! `- `), `1.` (any number). On Enter: a line that is only ``` or ```lang becomes a code block, `---` `***` `___` a divider;
//! an EMPTY list item steps out one level, and at the first level becomes a paragraph.
//! Never inside a code block, never inside inline code.
use mn_doc::{Block, BlockId, Change, ChangeError, Kind, Mark, MarkKind, Rank, Spans};
use crate::{Batch, Pos};

fn boundary_before(text: &str, at: usize) -> bool { text[..at].chars().next_back().is_none_or(|c| c.is_whitespace() || (c.is_ascii_punctuation() && c != '_' && c != '*')) }

/// The opener of a pair that `close` (the marker just typed, ending at `at`) completes: its byte offset.
fn opener(text: &str, at: usize, marker: &str, word_bound: bool) -> Option<usize> {
    let close = at.checked_sub(marker.len())?; if &text[close..at] != marker { return None }
    let single = marker.len() == 1; let m = marker.chars().next().unwrap();
    // a lone `*` must not be half of a `**`
    if single && text[..close].ends_with(m) { return None }
    let mut search = close;
    while let Some(o) = text[..search].rfind(marker) {
        let inner = &text[o + marker.len()..close];
        let lone = !single || (!text[..o].ends_with(m) && !inner.starts_with(m));
        if lone && !inner.is_empty() && !inner.starts_with(char::is_whitespace) && !inner.ends_with(char::is_whitespace) && !inner.contains('\n') && (!word_bound || boundary_before(text, o)) { return Some(o) }
        search = o;
    }
    None
}

/// After `typed` (one character) went in before `p`: the conversion it completes, if any. Returns the caret afterwards.
pub(crate) fn on_char(b: &mut Batch, p: Pos, typed: &str) -> Result<Option<Pos>, ChangeError> {
    let Some(blk) = b.scratch.get(p.block).cloned() else { return Ok(None) };
    if !blk.kind.is_text() || matches!(blk.kind, Kind::Code { .. }) || p.at > blk.text.len() { return Ok(None) }
    let text = &blk.text; let at = p.at;
    let in_code = |i: usize| blk.spans.as_slice().iter().any(|s| s.mark.kind() == MarkKind::Code && s.start < i && i <= s.end);
    if in_code(at) { return Ok(None) }
    // a block prefix, on its space, in a paragraph
    if typed == " " && blk.kind == Kind::Paragraph {
        let head = &text[..at - 1];
        let kind = match head {
            "#" | "##" | "###" | "####" | "#####" | "######" => Some(Kind::Heading { level: head.len() as u8 }),
            ">" => Some(Kind::Quote),
            "-" | "*" | "+" => Some(Kind::Bullet { depth: 0 }),
            "[]" | "[ ]" | "- [ ]" | "- []" => Some(Kind::Task { depth: 0, state: 0 }),
            "[/]" | "- [/]" => Some(Kind::Task { depth: 0, state: 1 }),
            "[x]" | "[X]" | "- [x]" | "- [X]" => Some(Kind::Task { depth: 0, state: 2 }),
            h if h.len() >= 2 && h.len() <= 4 && h.ends_with('.') && h[..h.len() - 1].bytes().all(|c| c.is_ascii_digit()) => Some(Kind::Numbered { depth: 0 }),
            _ => None,
        };
        if let Some(kind) = kind { b.edit(p.block, 0, at, "")?; b.push(Change::Kind { id: p.block, old: Kind::Paragraph, new: kind })?; return Ok(Some(Pos { block: p.block, at: 0 })) }
    }
    // `- ` already made a bullet; `[ ] ` typed in it makes it a task
    if typed == " " { if let Kind::Bullet { depth } = blk.kind { let state = match &text[..at - 1] { "[]" | "[ ]" => Some(0), "[/]" => Some(1), "[x]" | "[X]" => Some(2), _ => None };
        if let Some(state) = state { b.edit(p.block, 0, at, "")?; b.push(Change::Kind { id: p.block, old: blk.kind.clone(), new: Kind::Task { depth, state } })?; return Ok(Some(Pos { block: p.block, at: 0 })) } } }
    // an inline pair, on its closing marker
    let pairs: [(&str, Mark, bool); 6] = [("**", Mark::Bold, false), ("__", Mark::Bold, true), ("~~", Mark::Strike, false), ("`", Mark::Code, false), ("*", Mark::Italic, false), ("_", Mark::Italic, true)];
    for (marker, mark, word_bound) in pairs {
        if !typed.ends_with(marker.chars().next().unwrap()) { continue }
        let Some(o) = opener(text, at, marker, word_bound) else { continue };
        if in_code(o + 1) { continue }
        let (m, close) = (marker.len(), at - marker.len());
        let inner = text[o + m..close].to_string();
        // one change: the marked stretch is replaced by its inside, and the mark is put on it
        let mut c = Change::edit(&b.scratch, p.block, o, at - o, &inner)?;
        if let Change::Text { new_spans, .. } = &mut c { *new_spans = blk.spans.clone(); new_spans.edit(close, m, 0); new_spans.edit(o, m, 0); new_spans.put(o, o + inner.len(), mark); }
        b.push(c)?;
        return Ok(Some(Pos { block: p.block, at: o + inner.len() }))
    }
    // [label](url), on the closing parenthesis
    if typed == ")" {
        if let Some(mid) = text[..at - 1].rfind("](") { if let Some(o) = text[..mid].rfind('[') {
            let (label, url) = (text[o + 1..mid].to_string(), text[mid + 2..at - 1].to_string());
            if !label.is_empty() && !url.is_empty() && !url.contains(char::is_whitespace) && !label.contains('\n') && !in_code(o + 1) {
                let mut c = Change::edit(&b.scratch, p.block, o, at - o, &label)?;
                if let Change::Text { new_spans, .. } = &mut c { *new_spans = blk.spans.clone(); new_spans.edit(mid, at - mid, 0); new_spans.edit(o, 1, 0); new_spans.put(o, o + label.len(), Mark::Link(url)); }
                b.push(c)?;
                return Ok(Some(Pos { block: p.block, at: o + label.len() }))
            }
        } }
    }
    Ok(None)
}

/// Enter at `p`: what Markdown or a list makes of it instead of a plain split. Returns the caret afterwards.
pub(crate) fn on_enter(b: &mut Batch, p: Pos) -> Result<Option<Pos>, ChangeError> {
    let Some(blk) = b.scratch.get(p.block).cloned() else { return Ok(None) };
    // an empty list item steps out
    if blk.text.is_empty() {
        let new = match &blk.kind { Kind::Bullet { depth } if *depth > 0 => Some(Kind::Bullet { depth: depth - 1 }), Kind::Numbered { depth } if *depth > 0 => Some(Kind::Numbered { depth: depth - 1 }), Kind::Task { depth, state } if *depth > 0 => Some(Kind::Task { depth: depth - 1, state: *state }),
            Kind::Bullet { .. } | Kind::Numbered { .. } | Kind::Task { .. } | Kind::Quote => Some(Kind::Paragraph), _ => None };
        if let Some(new) = new { b.push(Change::Kind { id: p.block, old: blk.kind.clone(), new })?; return Ok(Some(Pos { block: p.block, at: 0 })) }
    }
    if blk.kind != Kind::Paragraph || p.at != blk.text.len() { return Ok(None) }
    if let Some(lang) = blk.text.strip_prefix("```").filter(|l| !l.contains(char::is_whitespace) && !l.contains('`')) {
        b.edit(p.block, 0, blk.text.len(), "")?; b.push(Change::Kind { id: p.block, old: Kind::Paragraph, new: Kind::Code { lang: lang.to_string() } })?;
        return Ok(Some(Pos { block: p.block, at: 0 }))
    }
    if matches!(blk.text.as_str(), "---" | "***" | "___") {
        // the line becomes a divider, and writing goes on in a new paragraph below it
        let index = b.scratch.index_of(p.block).unwrap();
        let block = |kind: Kind| Block { id: BlockId::new(), parent: blk.parent, rank: Rank::first(), kind, text: String::new(), spans: Spans::new(), conflict: None, children: vec![] };
        let (div, para) = (block(Kind::Divider), block(Kind::Paragraph)); let next = para.id;
        let gone = Change::removal(&b.scratch, p.block)?; b.push(gone)?;
        b.push(Change::Insert { index, blocks: vec![div] })?; b.push(Change::Insert { index: index + 1, blocks: vec![para] })?;
        return Ok(Some(Pos { block: next, at: 0 }))
    }
    Ok(None)
}
