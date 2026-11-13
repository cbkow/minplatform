//! The parts of an office document (Word, Excel, OpenDocument: zips of XML) as a flat run of tags and text - all
//! the importers need (canon row 335, stage 2). A reader of our own, as `paste.rs` has for HTML: names are kept as
//! written (with their prefix, `w:p`, `text:p`), attributes are decoded, comments, processing instructions and
//! doctypes are skipped, and a CDATA section is text.

pub enum Tok { Open { name: String, attrs: Vec<(String, String)>, /// `<a/>`: nothing is inside it and no Close follows
    empty: bool }, Close(String), Text(String) }

fn decode(s: &str) -> String {
    if !s.contains('&') { return s.to_string() }
    let mut out = String::with_capacity(s.len()); let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]); rest = &rest[i..];
        let hit = rest[1..].find(';').filter(|j| *j <= 10).and_then(|j| { let n = &rest[1..1 + j];
            let c = if let Some(d) = n.strip_prefix('#') { if let Some(h) = d.strip_prefix(['x', 'X']) { u32::from_str_radix(h, 16).ok().and_then(char::from_u32) } else { d.parse().ok().and_then(char::from_u32) } }
                else { match n { "amp" => Some('&'), "lt" => Some('<'), "gt" => Some('>'), "quot" => Some('"'), "apos" => Some('\''), _ => None } };
            c.map(|c| (c, j + 2)) });
        match hit { Some((c, n)) => { out.push(c); rest = &rest[n..]; } None => { out.push('&'); rest = &rest[1..]; } }
    }
    out.push_str(rest); out
}

fn attrs(s: &str) -> Vec<(String, String)> {
    let mut out = vec![]; let mut rest = s.trim_start();
    while let Some(eq) = rest.find('=') {
        let name = rest[..eq].trim().to_string(); let after = rest[eq + 1..].trim_start();
        let Some(q) = after.chars().next().filter(|c| *c == '"' || *c == '\'') else { break };
        let Some(end) = after[1..].find(q) else { break };
        if !name.is_empty() { out.push((name, decode(&after[1..1 + end]))); }
        rest = after[end + 2..].trim_start();
    }
    out
}

pub fn tokens(xml: &str) -> Vec<Tok> {
    let mut out = vec![]; let mut rest = xml;
    while !rest.is_empty() {
        let Some(i) = rest.find('<') else { out.push(Tok::Text(decode(rest))); break };
        if i > 0 { out.push(Tok::Text(decode(&rest[..i]))); }
        rest = &rest[i..];
        if let Some(r) = rest.strip_prefix("<!--") { rest = r.find("-->").map(|j| &r[j + 3..]).unwrap_or(""); continue }
        if let Some(r) = rest.strip_prefix("<![CDATA[") { let j = r.find("]]>").unwrap_or(r.len()); out.push(Tok::Text(r[..j].to_string())); rest = r.get(j + 3..).unwrap_or(""); continue }
        if rest.starts_with("<?") { rest = rest.find("?>").map(|j| &rest[j + 2..]).unwrap_or(""); continue }
        if rest.starts_with("<!") { rest = rest.find('>').map(|j| &rest[j + 1..]).unwrap_or(""); continue }
        // the tag's end, not a `>` inside a quoted attribute
        let b = rest.as_bytes(); let (mut j, mut q) = (1, 0u8);
        while j < b.len() { if q != 0 { if b[j] == q { q = 0; } } else if b[j] == b'"' || b[j] == b'\'' { q = b[j]; } else if b[j] == b'>' { break } j += 1; }
        if j >= b.len() { break }
        let inner = &rest[1..j]; rest = &rest[j + 1..];
        if let Some(name) = inner.strip_prefix('/') { out.push(Tok::Close(name.trim().to_string())); continue }
        let (inner, empty) = match inner.strip_suffix('/') { Some(x) => (x, true), None => (inner, false) };
        let n = inner.find(char::is_whitespace).unwrap_or(inner.len());
        out.push(Tok::Open { name: inner[..n].to_string(), attrs: attrs(&inner[n..]), empty });
    }
    out
}

/// An attribute's value.
pub fn attr<'a>(attrs: &'a [(String, String)], name: &str) -> Option<&'a str> { attrs.iter().find(|a| a.0 == name).map(|a| a.1.as_str()) }

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn tags_text_and_attributes_are_read_as_written() {
        let t = tokens("<?xml version=\"1.0\"?><!-- c --><w:p a='1 &amp; 2' b=\"x>y\"><w:b/>Tom &amp; &#x4a;erry &lt;<![CDATA[<raw>]]></w:p>");
        let s: Vec<String> = t.iter().map(|t| match t { Tok::Open { name, attrs, empty } => format!("<{name} {attrs:?} {empty}>"), Tok::Close(n) => format!("</{n}>"), Tok::Text(x) => x.clone() }).collect();
        assert_eq!(s, ["<w:p [(\"a\", \"1 & 2\"), (\"b\", \"x>y\")] false>", "<w:b [] true>", "Tom & Jerry <", "<raw>", "</w:p>"]);
    }
}
