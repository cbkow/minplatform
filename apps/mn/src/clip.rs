//! What goes to the system's clipboard and comes back (canon row 308, stage E of row 303). A copy writes two forms:
//! formatted text (HTML) for other applications, and plain text. Copied BLOCKS also ride in the HTML — as a JSON
//! payload in an attribute of its first element — so a paste in minNotes gives the blocks back whole (their kinds,
//! formatting, nesting), in this window or another, however long ago they were copied. No private clipboard type is
//! used: the clipboard library has none, and HTML is carried by every platform.
use mn_doc::{Block, BlockId, Kind, Mark, Rank};
use mn_edit::Clip;
use serde_json::{json, Value};
use std::collections::HashMap;

const ATTR: &str = "data-mn-blocks=\"";

fn esc(s: &str) -> String { s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;") }
fn unesc(s: &str) -> String { s.replace("&quot;", "\"").replace("&gt;", ">").replace("&lt;", "<").replace("&amp;", "&") }

/// The blocks as JSON: each tree a list of its blocks, the top first, a block naming its parent by its place in the list.
pub fn payload(clip: &Clip) -> String {
    let trees: Vec<Value> = clip.iter().map(|tree| { let at: HashMap<BlockId, usize> = tree.iter().enumerate().map(|(i, b)| (b.id, i)).collect();
        Value::Array(tree.iter().map(|b| { let (k, a, t) = mn_store::codec::encode(b); json!({ "p": b.parent.and_then(|p| at.get(&p).copied()), "k": k, "a": a, "t": t }) }).collect()) }).collect();
    json!({ "mn": 1, "blocks": trees }).to_string()
}
/// The inverse (fresh identities; a paste gives them new ones again).
pub fn parse(s: &str) -> Option<Clip> {
    let v: Value = serde_json::from_str(s).ok()?; if v.get("mn")?.as_u64()? != 1 { return None }
    let mut out = vec![];
    for tree in v.get("blocks")?.as_array()? {
        let items = tree.as_array()?; let ids: Vec<BlockId> = items.iter().map(|_| BlockId::new()).collect(); let mut blocks: Vec<Block> = vec![];
        for (i, it) in items.iter().enumerate() {
            let (kind, text, spans, _) = mn_store::codec::decode(it.get("k")?.as_str()?, it.get("a").and_then(|a| a.as_str()), it.get("t").and_then(|t| t.as_str()).unwrap_or("").to_string());
            let parent = it.get("p").and_then(|p| p.as_u64()).map(|p| p as usize).filter(|p| *p < i);
            if let Some(p) = parent { blocks[p].children.push(ids[i]); }
            blocks.push(Block { id: ids[i], parent: parent.map(|p| ids[p]), rank: Rank::first(), kind, text, spans, conflict: None, children: vec![] });
        }
        if !blocks.is_empty() { out.push(blocks); }
    }
    (!out.is_empty()).then_some(out)
}
/// Blocks found in HTML that minNotes wrote.
pub fn from_html(html: &str) -> Option<Clip> { let i = html.find(ATTR)? + ATTR.len(); let j = html[i..].find('"')?; parse(&unesc(&html[i..i + j])) }

/// A block's text with its formatting as HTML.
fn inline(b: &Block) -> String {
    let mut cuts: Vec<usize> = vec![0, b.text.len()]; for sp in b.spans.as_slice() { cuts.push(sp.start.min(b.text.len())); cuts.push(sp.end.min(b.text.len())); }
    cuts.sort(); cuts.dedup(); let mut out = String::new();
    for w in cuts.windows(2) { let (s, e) = (w[0], w[1]); if e <= s || !b.text.is_char_boundary(s) || !b.text.is_char_boundary(e) { continue }
        let (mut open, mut close) = (String::new(), String::new());
        for sp in b.spans.as_slice().iter().filter(|sp| sp.start <= s && sp.end >= e) {
            let colour = |c: &str| mn_layout::NAMED.iter().find(|(n, _)| *n == c).map(|(_, h)| h.to_string()).unwrap_or_else(|| c.to_string());
            let (o, c) = match &sp.mark { Mark::Bold => ("<b>".to_string(), "</b>"), Mark::Italic => ("<i>".into(), "</i>"), Mark::Underline => ("<u>".into(), "</u>"), Mark::Strike => ("<s>".into(), "</s>"), Mark::Code => ("<code>".into(), "</code>"),
                Mark::Link(u) => (format!("<a href=\"{}\">", esc(u)), "</a>"), Mark::Color(v) => (format!("<span style=\"color:{}\">", esc(&colour(v))), "</span>"), Mark::Highlight(v) => (format!("<span style=\"background-color:{}\">", esc(&colour(v))), "</span>"), Mark::Choice(_) => (String::new(), "") };
            open.push_str(&o); close.insert_str(0, c);
        }
        out.push_str(&open); out.push_str(&esc(&b.text[s..e]).replace('\n', "<br>")); out.push_str(&close);
    }
    out
}
/// Sibling blocks as HTML; list items are gathered into lists, nested by their depth.
fn siblings(ids: &[BlockId], all: &HashMap<BlockId, &Block>, out: &mut String) {
    let mut lists: Vec<&str> = vec![];
    let close_to = |lists: &mut Vec<&str>, n: usize, out: &mut String| while lists.len() > n { let t = lists.pop().unwrap(); out.push_str(&format!("</{t}>")); };
    for id in ids { let Some(b) = all.get(id) else { continue };
        let item = match &b.kind { Kind::Bullet { depth } => Some(("ul", *depth, "")), Kind::Numbered { depth } => Some(("ol", *depth, "")), Kind::Task { depth, state } => Some(("ul", *depth, if *state == 2 { "\u{2611} " } else { "\u{2610} " })), _ => None };
        if let Some((tag, depth, mark)) = item {
            let want = depth as usize + 1; close_to(&mut lists, want, out);
            if lists.len() == want && lists[want - 1] != tag { close_to(&mut lists, want - 1, out); }
            while lists.len() < want { lists.push(tag); out.push_str(&format!("<{tag}>")); }
            out.push_str(&format!("<li>{mark}{}</li>", inline(b))); continue
        }
        close_to(&mut lists, 0, out);
        match &b.kind {
            Kind::Paragraph => out.push_str(&format!("<p>{}</p>", inline(b))), Kind::Heading { level } => { let n = (*level).clamp(1, 6); out.push_str(&format!("<h{n}>{}</h{n}>", inline(b))) }
            Kind::Quote => out.push_str(&format!("<blockquote>{}</blockquote>", inline(b))), Kind::Code { .. } => out.push_str(&format!("<pre><code>{}</code></pre>", esc(&b.text))), Kind::Divider => out.push_str("<hr>"),
            // columns have no meaning elsewhere: their content follows, column after column
            Kind::Columns | Kind::Column { .. } => siblings(&b.children, all, out),
            Kind::Table { .. } => { out.push_str("<table>"); for r in &b.children { out.push_str("<tr>"); for c in all.get(r).map(|r| r.children.as_slice()).unwrap_or(&[]) { out.push_str("<td>"); if let Some(c) = all.get(c) { siblings(&c.children, all, out); } out.push_str("</td>"); } out.push_str("</tr>"); } out.push_str("</table>") }
            _ => {}
        }
    }
    close_to(&mut lists, 0, out);
}
/// The HTML a copy writes. `whole`: the blocks were copied as blocks — the payload rides along.
pub fn html(clip: &Clip, whole: bool) -> String {
    let all: HashMap<BlockId, &Block> = clip.iter().flatten().map(|b| (b.id, b)).collect();
    let tops: Vec<BlockId> = clip.iter().filter_map(|t| t.first().map(|b| b.id)).collect();
    let mut body = String::new(); siblings(&tops, &all, &mut body);
    if whole { format!("<meta charset=\"utf-8\"><div {ATTR}{}\">{body}</div>", esc(&payload(clip))) } else { format!("<meta charset=\"utf-8\">{body}") }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mn_doc::Spans;
    fn block(kind: Kind, text: &str) -> Block { Block { id: BlockId::new(), parent: None, rank: Rank::first(), kind, text: text.into(), spans: Spans::new(), conflict: None, children: vec![] } }
    #[test]
    fn copied_blocks_survive_the_trip_through_html_and_other_applications_get_real_markup() {
        let mut h = block(Kind::Heading { level: 2 }, "Title & more"); h.spans.put(0, 5, Mark::Bold);
        let mut p = block(Kind::Paragraph, "a \"quoted\" <word>"); p.spans.put(2, 10, Mark::Color("red".into())); p.spans.put(2, 10, Mark::Italic);
        let (a, a1, n) = (block(Kind::Bullet { depth: 0 }, "A"), block(Kind::Bullet { depth: 1 }, "a1"), block(Kind::Numbered { depth: 0 }, "one"));
        let mut cols = block(Kind::Columns, ""); let mut col = block(Kind::Column { width: 1.0 }, ""); let mut inner = block(Kind::Paragraph, "inside");
        col.parent = Some(cols.id); inner.parent = Some(col.id); cols.children = vec![col.id]; col.children = vec![inner.id];
        let clip: Clip = vec![vec![h], vec![p], vec![a], vec![a1], vec![n], vec![cols, col, inner]];
        let html = html(&clip, true);
        for part in ["<h2><b>Title</b> &amp; more</h2>", "<ul><li>A</li><ul><li>a1</li></ul></ul><ol><li>one</li></ol>", "<p>inside</p>", "color:#FF5768", "&lt;word&gt;"] { assert!(html.contains(part), "{part} not in {html}"); }
        let back = from_html(&html).expect("the payload is found");
        assert_eq!(back.len(), 6);
        let texts: Vec<&str> = back.iter().map(|t| t[0].text.as_str()).collect(); assert_eq!(texts, ["Title & more", "a \"quoted\" <word>", "A", "a1", "one", ""]);
        assert_eq!(back[0][0].kind, Kind::Heading { level: 2 }); assert!(back[1][0].spans.covers(2, 10, &Mark::Color("red".into())));
        let c = &back[5]; assert_eq!(c.len(), 3); assert_eq!(c[0].children, vec![c[1].id]); assert_eq!(c[2].parent, Some(c[1].id)); assert_eq!(c[2].text, "inside");
        // a text selection's HTML carries no payload: it pastes back as text
        assert!(from_html(&super::html(&clip, false)).is_none());
        assert!(from_html("<p>from elsewhere</p>").is_none());
    }
}
