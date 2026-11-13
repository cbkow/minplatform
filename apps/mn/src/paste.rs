//! What ANOTHER application put on the clipboard, as blocks (canon row 309 a): formatted text (HTML, as Word, Google
//! Docs, Pages, Mail and the browsers write it) and Markdown source.
//!
//! HTML is read by a small tolerant reader of our own - the clipboard's HTML is a serialised page fragment, not a
//! page to be laid out: tags, attributes, text and entities are enough. Kept: paragraphs, headings, lists (nested,
//! numbered, task boxes), quotes, code blocks, rules, tables, line breaks, and bold / italic / underline / strike /
//! code / links - whether they come as tags or as `style`. Dropped by the ruling: colours, fonts, sizes; and for
//! now pictures (they come with pasted files). A table with one column is layout, not data: its content follows as
//! blocks.
use mn_doc::{Block, BlockId, ColumnSpec, Kind, Mark, MarkKind, Rank, Spans};
use mn_edit::Clip;

fn block(kind: Kind, text: String, spans: Spans) -> Block { Block { id: BlockId::new(), parent: None, rank: Rank::first(), kind, text, spans, conflict: None, children: vec![] } }

/// How a reader differs for a FILE that is imported (row 335) from what is pasted: an import is faithful - colours
/// are kept (but not greys: black on white is stamped on whole documents, and a grey says nothing and is lost on a
/// dark page), and a picture named by a file's path becomes a block (the importer finds and keeps the file).
#[derive(Clone, Copy, Default)]
pub struct Opts { pub colours: bool, pub local: bool }

/// A CSS colour as `#rrggbb`; None for what is no colour here (transparent, a grey, something unread).
fn colour(v: &str) -> Option<String> {
    let v = v.split_whitespace().next().unwrap_or("").trim().to_ascii_lowercase(); let v = if v.starts_with("rgb") { v } else { v.trim_end_matches("!important").to_string() };
    let rgb: [u8; 3] = if let Some(h) = v.strip_prefix('#') { let d: Vec<u8> = h.chars().filter_map(|c| c.to_digit(16).map(|d| d as u8)).collect();
            match (h.len(), d.len()) { (3, 3) | (4, 4) => [d[0] * 17, d[1] * 17, d[2] * 17], (6, 6) | (8, 8) => [d[0] * 16 + d[1], d[2] * 16 + d[3], d[4] * 16 + d[5]], _ => return None } }
        else { match v.as_str() { "red" => [255, 0, 0], "green" => [0, 128, 0], "blue" => [0, 0, 255], "yellow" => [255, 255, 0], "orange" => [255, 165, 0], "purple" => [128, 0, 128], "pink" => [255, 192, 203], "brown" => [165, 42, 42],
            "cyan" | "aqua" => [0, 255, 255], "magenta" | "fuchsia" => [255, 0, 255], "teal" => [0, 128, 128], "navy" => [0, 0, 128], "maroon" => [128, 0, 0], "olive" => [128, 128, 0], "lime" => [0, 255, 0], "gold" => [255, 215, 0], _ => return None } };
    if rgb.iter().max().unwrap() - rgb.iter().min().unwrap() < 24 { return None }
    Some(format!("#{:02x}{:02x}{:02x}", rgb[0], rgb[1], rgb[2]))
}
/// `rgb(1, 2, 3)` / `rgba(1 2 3 / 50%)` as `#rrggbb` text for [`colour`] (nothing when it is fully clear).
pub fn css_colour(v: &str) -> Option<String> {
    let v = v.trim().to_ascii_lowercase();
    if let Some(i) = v.find("rgb") { let inner = v[i..].split_once('(')?.1.split(')').next()?; let n: Vec<f64> = inner.split([',', ' ', '/']).filter(|x| !x.is_empty()).filter_map(|x| x.trim_end_matches('%').parse::<f64>().ok()).collect();
        if n.len() < 3 || n.get(3).is_some_and(|a| *a == 0.0) { return None } return colour(&format!("#{:02x}{:02x}{:02x}", n[0].clamp(0.0, 255.0) as u8, n[1].clamp(0.0, 255.0) as u8, n[2].clamp(0.0, 255.0) as u8)) }
    colour(&v)
}

// ── HTML

enum Tok { Open(String, Vec<(String, String)>), Close(String), Text(String), /// Word's list marker (between `<![if !supportLists]>` and `<![endif]>`)
    Marker(String) }

fn entity(name: &str) -> Option<char> {
    if let Some(n) = name.strip_prefix('#') { let v = if let Some(h) = n.strip_prefix(['x', 'X']) { u32::from_str_radix(h, 16).ok()? } else { n.parse().ok()? }; return char::from_u32(v) }
    Some(match name { "amp" => '&', "lt" => '<', "gt" => '>', "quot" => '"', "apos" => '\'', "nbsp" => '\u{a0}', "ndash" => '\u{2013}', "mdash" => '\u{2014}', "hellip" => '\u{2026}', "lsquo" => '\u{2018}', "rsquo" => '\u{2019}', "ldquo" => '\u{201c}', "rdquo" => '\u{201d}',
        "copy" => '\u{a9}', "reg" => '\u{ae}', "trade" => '\u{2122}', "bull" => '\u{2022}', "middot" => '\u{b7}', "laquo" => '\u{ab}', "raquo" => '\u{bb}', "deg" => '\u{b0}', "euro" => '\u{20ac}', "pound" => '\u{a3}', "times" => '\u{d7}', "rarr" => '\u{2192}', "larr" => '\u{2190}', _ => return None })
}
fn decode(s: &str) -> String {
    let mut out = String::with_capacity(s.len()); let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]); rest = &rest[i..];
        match rest[1..].find(';').filter(|j| *j <= 10).and_then(|j| entity(&rest[1..1 + j]).map(|c| (c, j + 2))) { Some((c, n)) => { out.push(c); rest = &rest[n..]; } None => { out.push('&'); rest = &rest[1..]; } }
    }
    out.push_str(rest); out
}
fn attrs(s: &str) -> Vec<(String, String)> {
    let (mut out, b) = (vec![], s.as_bytes()); let mut i = 0;
    while i < b.len() {
        while i < b.len() && (b[i].is_ascii_whitespace() || b[i] == b'/') { i += 1; }
        let k0 = i; while i < b.len() && !b[i].is_ascii_whitespace() && b[i] != b'=' && b[i] != b'/' { i += 1; }
        if i == k0 { i += 1; continue }
        let key = s[k0..i].to_ascii_lowercase();
        while i < b.len() && b[i].is_ascii_whitespace() { i += 1; }
        let mut val = String::new();
        if i < b.len() && b[i] == b'=' { i += 1; while i < b.len() && b[i].is_ascii_whitespace() { i += 1; }
            if i < b.len() && (b[i] == b'"' || b[i] == b'\'') { let q = b[i]; i += 1; let v0 = i; while i < b.len() && b[i] != q { i += 1; } val = decode(&s[v0..i]); i += 1; }
            else { let v0 = i; while i < b.len() && !b[i].is_ascii_whitespace() { i += 1; } val = decode(&s[v0..i]); } }
        out.push((key, val));
    }
    out
}
fn tokens(html: &str) -> Vec<Tok> {
    let mut out = vec![]; let mut rest = html;
    while !rest.is_empty() {
        let Some(i) = rest.find('<') else { out.push(Tok::Text(decode(rest))); break };
        if i > 0 { out.push(Tok::Text(decode(&rest[..i]))); }
        rest = &rest[i..];
        if rest.starts_with("<!--") { rest = rest.find("-->").map(|j| &rest[j + 3..]).unwrap_or(""); continue }
        if rest.len() >= 21 && rest[..21].eq_ignore_ascii_case("<![if !supportLists]>") {
            let end = rest.to_ascii_lowercase().find("<![endif]>").unwrap_or(rest.len());
            let inner: String = tokens(&rest[21..end]).into_iter().filter_map(|t| if let Tok::Text(t) = t { Some(t) } else { None }).collect();
            out.push(Tok::Marker(inner)); rest = rest.get(end + 10..).unwrap_or(""); continue }
        if rest.starts_with("<!") || rest.starts_with("<?") { rest = rest.find('>').map(|j| &rest[j + 1..]).unwrap_or(""); continue }
        // the tag's end, not a `>` inside a quoted attribute
        let b = rest.as_bytes(); let (mut j, mut q) = (1, 0u8);
        while j < b.len() { if q != 0 { if b[j] == q { q = 0; } } else if b[j] == b'"' || b[j] == b'\'' { q = b[j]; } else if b[j] == b'>' { break } j += 1; }
        if j >= b.len() { out.push(Tok::Text(decode(rest))); break }
        let inner = &rest[1..j]; rest = &rest[j + 1..];
        if let Some(name) = inner.strip_prefix('/') { out.push(Tok::Close(name.trim().to_ascii_lowercase())); continue }
        let n = inner.find(|c: char| c.is_whitespace() || c == '/').unwrap_or(inner.len());
        let name = inner[..n].to_ascii_lowercase();
        if name.is_empty() || !name.as_bytes()[0].is_ascii_alphabetic() { out.push(Tok::Text(format!("<{}", decode(inner)))); continue }
        // what is inside these is not text of the page
        if matches!(name.as_str(), "script" | "style" | "head" | "title" | "svg" | "math" | "noscript" | "template" | "object" | "iframe" | "select" | "textarea") {
            let close = format!("</{name}"); rest = rest.to_ascii_lowercase().find(&close).and_then(|k| rest[k..].find('>').map(|e| &rest[k + e + 1..])).unwrap_or(""); continue }
        out.push(Tok::Open(name, attrs(&inner[n..])));
    }
    out
}

/// The marks an element's tag and `style` turn on, and whether it turns bold OFF (Google Docs wraps everything in a
/// `<b style="font-weight:normal">`).
fn marks_of(name: &str, at: &[(String, String)], colours: bool) -> (Vec<Mark>, bool) {
    let mut m = match name { "b" | "strong" => vec![Mark::Bold], "i" | "em" | "cite" | "dfn" | "var" => vec![Mark::Italic], "u" | "ins" => vec![Mark::Underline], "s" | "strike" | "del" => vec![Mark::Strike], "code" | "kbd" | "tt" | "samp" => vec![Mark::Code],
        "a" => at.iter().find(|a| a.0 == "href").filter(|a| !a.1.is_empty() && !a.1.starts_with('#') && !a.1.to_ascii_lowercase().starts_with("javascript:")).map(|a| vec![Mark::Link(a.1.clone())]).unwrap_or_default(), _ => vec![] };
    let mut unbold = false;
    if colours { if name == "mark" { m.push(Mark::Highlight("#ffd400".into())); } if name == "font" { if let Some(c) = at.iter().find(|a| a.0 == "color").and_then(|a| css_colour(&a.1)) { m.push(Mark::Color(c)); } } }
    if let Some((_, style)) = at.iter().find(|a| a.0 == "style") { for decl in style.split(';') { let Some((k, v)) = decl.split_once(':') else { continue }; let (k, v) = (k.trim().to_ascii_lowercase(), v.trim().to_ascii_lowercase());
        match k.as_str() {
            "font-weight" => { let heavy = v == "bold" || v == "bolder" || v.parse::<u32>().is_ok_and(|n| n >= 600); if heavy { if !m.contains(&Mark::Bold) { m.push(Mark::Bold) } } else { m.retain(|x| *x != Mark::Bold); unbold = true; } }
            "font-style" => if v.starts_with("italic") || v.starts_with("oblique") { if !m.contains(&Mark::Italic) { m.push(Mark::Italic) } } else if v == "normal" { m.retain(|x| *x != Mark::Italic) },
            "text-decoration" | "text-decoration-line" => { if v.contains("underline") && !m.contains(&Mark::Underline) { m.push(Mark::Underline) } if v.contains("line-through") && !m.contains(&Mark::Strike) { m.push(Mark::Strike) } }
            "color" if colours => if let Some(c) = css_colour(&v) { m.retain(|x| x.kind() != MarkKind::Color); m.push(Mark::Color(c)); },
            "background-color" | "background" if colours => if let Some(c) = css_colour(&v) { m.retain(|x| x.kind() != MarkKind::Highlight); m.push(Mark::Highlight(c)); },
            _ => {}
        } } }
    (m, unbold)
}

#[derive(Default)]
struct Cur { kind: Option<Kind>, text: String, spans: Spans }
/// One table being read: its rows, each a header-or-not and its cells' blocks.
#[derive(Default)]
struct Table { rows: Vec<(bool, Vec<Clip>)>, in_head: bool }
#[derive(Default)]
struct Reader {
    /// where finished blocks go: the result, or the cell being read (one entry per open cell)
    sinks: Vec<Clip>, cur: Cur, inline: Vec<(String, Vec<Mark>, bool)>, lists: Vec<bool>, item: Option<Kind>, quote: usize, heading: Option<u8>, pre: usize, tables: Vec<Table>, word_list: Option<u8>, opts: Opts,
}
impl Reader {
    fn active(&self) -> Vec<Mark> {
        let mut on: Vec<Mark> = vec![];
        for (_, m, unbold) in &self.inline { if *unbold { on.retain(|x| *x != Mark::Bold); } for x in m { if !on.iter().any(|o| o.kind() == x.kind()) { on.push(x.clone()); } } }
        // a link is drawn underlined already
        if on.iter().any(|x| x.kind() == MarkKind::Link) { on.retain(|x| *x != Mark::Underline); }
        on
    }
    fn kind_here(&mut self) -> Kind {
        if self.pre > 0 { return Kind::Code { lang: String::new() } }
        if let Some(k) = self.item.take() { return k }
        if let Some(d) = self.word_list.take() { return Kind::Bullet { depth: d } }
        if let Some(l) = self.heading { return Kind::Heading { level: l } }
        if self.quote > 0 { Kind::Quote } else { Kind::Paragraph }
    }
    fn text(&mut self, t: &str) {
        let pre = self.pre > 0; let mut s = String::with_capacity(t.len());
        if pre { s.push_str(&t.replace("\r\n", "\n").replace('\r', "\n")); } else {
            // runs of white space are one space; none at a block's or a line's start
            let mut space = self.cur.kind.is_none() || self.cur.text.ends_with([' ', '\n']) || self.cur.text.is_empty();
            for c in t.chars() { let c = if c == '\u{a0}' { ' ' } else { c }; if c.is_whitespace() { if !space { s.push(' '); space = true; } } else { s.push(c); space = false; } }
        }
        if s.is_empty() { return }
        if self.cur.kind.is_none() { if s.trim().is_empty() && !pre { return } self.cur.kind = Some(self.kind_here()); }
        let at = self.cur.text.len(); self.cur.text.push_str(&s);
        if !pre { for m in self.active() { self.cur.spans.put(at, at + s.len(), m); } }
    }
    fn flush(&mut self) {
        let Cur { kind, mut text, mut spans } = std::mem::take(&mut self.cur); let Some(mut kind) = kind else { return };
        let keep = text.trim_end_matches([' ', '\n']).len(); if keep < text.len() { spans.edit(keep, text.len() - keep, 0); text.truncate(keep); }
        if !matches!(kind, Kind::Code { .. }) { let lead = text.len() - text.trim_start_matches([' ', '\n']).len(); if lead > 0 { spans.edit(0, lead, 0); text.drain(..lead); } }
        if text.is_empty() { return }
        // a box drawn as a character at an item's start (our own HTML, and some sites) is a task
        if let Kind::Bullet { depth } = kind { for (mark, state) in [("\u{2610} ", 0u8), ("\u{2611} ", 2), ("\u{2612} ", 2), ("[ ] ", 0), ("[x] ", 2)] { if text.starts_with(mark) { spans.edit(0, mark.len(), 0); text.drain(..mark.len()); kind = Kind::Task { depth, state }; break } } }
        self.push(vec![block(kind, text, spans)]);
    }
    fn push(&mut self, tree: Vec<Block>) { if let Some(s) = self.sinks.last_mut() { s.push(tree); } }
    fn open(&mut self, name: &str, at: &[(String, String)]) {
        match name {
            // a picture: a block of its own, named by its address - it is fetched and kept when it is shown (row 314).
            // Very small ones are symbols in the text (an emoji drawn as a picture): their words stand in, or nothing
            "img" => {
                let get = |k: &str| at.iter().find(|a| a.0 == k).map(|a| a.1.trim().to_string()).filter(|v| !v.is_empty());
                let num = |k: &str| get(k).and_then(|v| v.trim_end_matches("px").parse::<f64>().ok()).filter(|v| *v > 0.0);
                let Some(src) = get("src").or_else(|| get("data-src")) else { return };
                let src = if let Some(r) = src.strip_prefix("//") { format!("https://{r}") } else { src };
                if !crate::media::remote(&src) && !self.opts.local { return }
                let (w, h) = (num("width"), num("height"));
                if w.is_some_and(|w| w <= 32.0) && h.is_some_and(|h| h <= 32.0) { if let Some(alt) = get("alt").filter(|a| a.chars().count() <= 8) { self.text(&alt); } return }
                self.flush();
                // a file's picture: the importer measures it; only a width the document asked for is passed on
                if !crate::media::remote(&src) { let d = match w { Some(w) => serde_json::json!({ "src": src, "w": w.round() as u32 }), None => serde_json::json!({ "src": src }) }; self.push(vec![block(Kind::Media { descriptor: d.to_string() }, String::new(), Spans::new())]); return }
                let (w, h) = match (w, h) { (Some(w), Some(h)) => (w.round() as u32, h.round() as u32), _ => (480, 300) };
                self.push(vec![block(Kind::Media { descriptor: serde_json::json!({ "src": src, "w": w, "h": h }).to_string() }, String::new(), Spans::new())]);
            }
            "br" => { if self.cur.kind.is_some() && !self.cur.text.is_empty() { self.cur.text.push('\n'); } }
            "hr" => { self.flush(); self.push(vec![block(Kind::Divider, String::new(), Spans::new())]); }
            "p" | "div" | "section" | "article" | "header" | "footer" | "main" | "aside" | "nav" | "figure" | "figcaption" | "dl" | "dt" | "dd" | "address" | "center" | "body" | "html" | "caption" | "details" | "summary" => {
                self.flush();
                // Word writes a list item as a paragraph that names its list and level in its style
                if name == "p" { if let Some(l) = at.iter().find(|a| a.0 == "style").and_then(|a| a.1.to_ascii_lowercase().find("mso-list:").map(|i| a.1[i..].to_ascii_lowercase())).and_then(|s| s.find("level").and_then(|i| s[i + 5..].chars().next()).and_then(|c| c.to_digit(10))) { self.word_list = Some((l.max(1) - 1).min(8) as u8); } }
            }
            "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => { self.flush(); self.heading = Some(name.as_bytes()[1] - b'0'); }
            "blockquote" => { self.flush(); self.quote += 1; }
            "pre" => { self.flush(); self.pre += 1; }
            "ul" | "ol" | "menu" => { self.flush(); self.lists.push(name == "ol"); }
            "li" => { self.flush(); let depth = self.lists.len().saturating_sub(1).min(8) as u8; self.item = Some(if self.lists.last() == Some(&true) { Kind::Numbered { depth } } else { Kind::Bullet { depth } }); }
            "input" => if at.iter().any(|a| a.0 == "type" && a.1.eq_ignore_ascii_case("checkbox")) { let done = at.iter().any(|a| a.0 == "checked");
                let depth = match (&self.item, &self.cur.kind) { (Some(Kind::Bullet { depth } | Kind::Numbered { depth }), _) | (_, Some(Kind::Bullet { depth } | Kind::Numbered { depth })) => Some(*depth), _ => None };
                if let Some(depth) = depth { let k = Kind::Task { depth, state: if done { 2 } else { 0 } }; if self.cur.kind.is_some() { self.cur.kind = Some(k) } else { self.item = Some(k) } } },
            "table" => { self.flush(); self.tables.push(Table::default()); }
            "thead" => if let Some(t) = self.tables.last_mut() { t.in_head = true },
            "tbody" | "tfoot" => if let Some(t) = self.tables.last_mut() { t.in_head = false },
            "tr" => { self.flush(); if let Some(t) = self.tables.last_mut() { let head = t.in_head; t.rows.push((head, vec![])); } }
            // a row is a header row while every cell of it is a `th` (or it is in the table's head)
            "td" | "th" => { self.flush(); if self.tables.last().is_some_and(|t| !t.rows.is_empty()) {
                if let Some(r) = self.tables.last_mut().and_then(|t| t.rows.last_mut()) { if name == "th" && r.1.is_empty() { r.0 = true; } }
                self.sinks.push(vec![]); self.inline.push((format!("/{name}"), vec![], false)); } }
            _ => { let (m, unbold) = marks_of(name, at, self.opts.colours); self.inline.push((name.to_string(), m, unbold)); }
        }
    }
    fn close(&mut self, name: &str) {
        match name {
            "p" | "div" | "section" | "article" | "header" | "footer" | "main" | "aside" | "nav" | "figure" | "figcaption" | "dl" | "dt" | "dd" | "address" | "center" | "body" | "html" | "caption" | "details" | "summary" => { self.flush(); self.word_list = None; }
            "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => { self.flush(); self.heading = None; }
            "blockquote" => { self.flush(); self.quote = self.quote.saturating_sub(1); }
            "pre" => { self.flush(); self.pre = self.pre.saturating_sub(1); }
            "ul" | "ol" | "menu" => { self.flush(); self.lists.pop(); self.item = None; }
            "li" => { self.flush(); self.item = None; }
            "td" | "th" => { self.flush();
                // the cell's own entry on the inline stack marks where its sink was opened
                if let Some(i) = self.inline.iter().rposition(|x| x.0 == format!("/{name}")) { self.inline.truncate(i); let cell = self.sinks.pop().unwrap_or_default();
                    let in_head = self.tables.last().is_some_and(|t| t.in_head);
                    if let Some(r) = self.tables.last_mut().and_then(|t| t.rows.last_mut()) { if name == "td" && !in_head { r.0 = false; } r.1.push(cell); } } }
            "tr" => self.flush(),
            "table" => { self.flush(); if let Some(t) = self.tables.pop() { self.table(t); } }
            _ => if let Some(i) = self.inline.iter().rposition(|x| x.0 == name) { self.inline.truncate(i); },
        }
    }
    /// A finished table: a Table block of rows of cells - unless it has one column (layout), or sits inside another
    /// table's cell (a cell holds no table): then its content follows as blocks.
    fn table(&mut self, t: Table) {
        let cols = t.rows.iter().map(|r| r.1.len()).max().unwrap_or(0);
        if cols < 2 || !self.tables.is_empty() { for (_, cells) in t.rows { for cell in cells { for tree in cell { self.push(tree); } } } return }
        let tree = table_tree(t.rows, vec![ColumnSpec::default(); cols]); self.push(tree);
    }
}

/// Rows of cells (each a header row or not, each cell its blocks) as a Table's tree: the table, then every row with
/// its cells and their leaves. A row short of cells is filled; an empty cell holds an empty paragraph.
pub fn table_tree(rows: Vec<(bool, Vec<Clip>)>, columns: Vec<ColumnSpec>) -> Vec<Block> {
    let cols = columns.len();
    let header_rows = rows.iter().take_while(|r| r.0).count().min(rows.len().saturating_sub(1)).min(255) as u8;
    let mut table = block(Kind::Table { header_rows, columns }, String::new(), Spans::new()); let mut all: Vec<Block> = vec![];
    for (_, mut cells) in rows {
        cells.resize_with(cols, Vec::new);
        let mut row = block(Kind::row(), String::new(), Spans::new()); row.parent = Some(table.id); table.children.push(row.id); let mut under: Vec<Block> = vec![];
        for cell_trees in cells {
            let mut cell = block(Kind::cell(), String::new(), Spans::new()); cell.parent = Some(row.id); row.children.push(cell.id); let mut inner: Vec<Block> = vec![];
            // a cell holds leaves; a list item's depth has no meaning there but is kept
            for mut tree in cell_trees { if tree.len() != 1 { continue } let mut b = tree.remove(0); b.parent = Some(cell.id); cell.children.push(b.id); inner.push(b); }
            if inner.is_empty() { let mut p = block(Kind::Paragraph, String::new(), Spans::new()); p.parent = Some(cell.id); cell.children.push(p.id); inner.push(p); }
            under.push(cell); under.extend(inner);
        }
        all.push(row); all.extend(under);
    }
    let mut tree = vec![table]; tree.extend(all); tree
}

/// Formatted text from another application as blocks. Empty: nothing in it is text.
pub fn from_html(html: &str) -> Clip { from_html_with(html, Opts::default()) }
pub fn from_html_with(html: &str, opts: Opts) -> Clip {
    // what was copied is between these marks when a browser on Windows wrote them
    let html = match (html.find("<!--StartFragment-->"), html.find("<!--EndFragment-->")) { (Some(a), Some(b)) if a + 20 <= b => &html[a + 20..b], _ => html };
    let mut r = Reader { sinks: vec![vec![]], opts, ..Default::default() };
    for t in tokens(html) { match t {
        Tok::Text(t) => r.text(&t),
        // Word's own marker: the item is numbered when the marker is a number or a letter with a stop
        Tok::Marker(m) => { let m = m.trim(); let numbered = m.chars().next().is_some_and(|c| c.is_alphanumeric()) && m.ends_with(['.', ')']); if let Some(d) = r.word_list.take() { r.item = Some(if numbered { Kind::Numbered { depth: d } } else { Kind::Bullet { depth: d } }); } }
        Tok::Open(name, at) => r.open(&name, &at),
        Tok::Close(name) => r.close(&name),
    } }
    r.flush(); while let Some(t) = r.tables.pop() { r.table(t); }
    r.sinks.into_iter().flatten().collect()
}

// ── Markdown

/// Inline Markdown as text with formatting: `code`, **bold** / __bold__, *italic* / _italic_, ~~strike~~,
/// [label](url), <https://…>, and a backslash before a mark.
fn inline(src: &str) -> (String, Spans) {
    fn walk(s: &str, out: &mut String, spans: &mut Spans, on: &[Mark]) {
        let put = |out: &mut String, spans: &mut Spans, t: &str, on: &[Mark]| { let at = out.len(); out.push_str(t); for m in on { spans.put(at, out.len(), m.clone()); } };
        let b = s.as_bytes(); let (mut i, mut plain) = (0usize, 0usize);
        while i < b.len() {
            let rest = &s[i..];
            let hit: Option<(usize, &str, Option<Mark>)> =
                if b[i] == b'\\' && i + 1 < b.len() && b[i + 1].is_ascii_punctuation() { Some((2, &s[i + 1..i + 2], None)) }
                else if b[i] == b'`' { rest[1..].find('`').filter(|j| *j > 0).map(|j| (j + 2, &rest[1..1 + j], Some(Mark::Code))) }
                else if b[i] == b'[' { rest.find("](").and_then(|j| rest[j + 2..].find(')').map(|k| (j, k))).filter(|(j, k)| *j > 1 && *k > 0 && !rest[1..*j].contains('\n')).map(|(j, k)| (j + 2 + k + 1, &rest[1..j], Some(Mark::Link(rest[j + 2..j + 2 + k].trim().to_string())))) }
                else if b[i] == b'<' { rest.find('>').filter(|j| rest[1..*j].starts_with("http://") || rest[1..*j].starts_with("https://")).filter(|j| !rest[1..*j].contains(char::is_whitespace)).map(|j| (j + 1, &rest[1..j], Some(Mark::Link(rest[1..j].to_string())))) }
                else { [("**", Mark::Bold), ("__", Mark::Bold), ("~~", Mark::Strike), ("*", Mark::Italic), ("_", Mark::Italic)].into_iter().find_map(|(m, mark)| {
                    if !rest.starts_with(m) { return None }
                    // a word's inner underscore is not a mark
                    if m.starts_with('_') && i > 0 && (b[i - 1].is_ascii_alphanumeric()) { return None }
                    let inner = &rest[m.len()..]; if inner.starts_with(char::is_whitespace) || inner.is_empty() { return None }
                    let mut from = 0; let j = loop { let j = inner[from..].find(m)? + from; if j > 0 && !inner[..j].ends_with(char::is_whitespace) && !(m.len() == 1 && inner[j..].starts_with(&m.repeat(2)) && !inner[..j].ends_with(m)) { break j } from = j + m.len(); if from >= inner.len() { return None } };
                    if inner[..j].contains("\n\n") { return None }
                    Some((m.len() + j + m.len(), &inner[..j], Some(mark))) }) };
            match hit {
                Some((len, text, mark)) => { put(out, spans, &s[plain..i], on);
                    match mark { None => put(out, spans, text, on), Some(Mark::Code) => { let mut v = on.to_vec(); v.push(Mark::Code); put(out, spans, text, &v) } Some(m) => { let mut v = on.to_vec(); v.push(m); walk(text, out, spans, &v) } }
                    i += len; plain = i; }
                None => { i += rest.chars().next().map(|c| c.len_utf8()).unwrap_or(1); }
            }
        }
        put(out, spans, &s[plain..], on);
    }
    let (mut out, mut spans) = (String::new(), Spans::new()); walk(src, &mut out, &mut spans, &[]); (out, spans)
}

/// Pictures in Markdown (`![words](address "title")`) are taken out of the text: each follows its paragraph as a
/// block, when its address can be fetched from (a file's path has no place to be looked for from here).
fn images(s: &str, local: bool) -> (String, Vec<String>) {
    let (mut out, mut found, mut rest) = (String::new(), vec![], s);
    while let Some(i) = rest.find("![") {
        let Some((j, k)) = rest[i..].find("](").and_then(|j| rest[i + j + 2..].find(')').map(|k| (i + j, k))) else { break };
        out.push_str(&rest[..i]);
        let addr = rest[j + 2..j + 2 + k].split_whitespace().next().unwrap_or("").trim_matches(['<', '>']).to_string();
        if crate::media::remote(&addr) || (local && !addr.is_empty()) { found.push(addr); rest = &rest[j + 2 + k + 1..]; if out.ends_with(' ') && rest.starts_with(' ') { rest = &rest[1..]; } continue }
        out.push_str(&rest[i + 2..j]); rest = &rest[j + 2 + k + 1..];
    }
    out.push_str(rest); (out, found)
}

/// Markdown source as blocks: headings, quotes, lists (nested by their indent; task boxes), fenced code, rules,
/// paragraphs (lines of one paragraph are joined by a space; two spaces or a backslash at a line's end keep the break).
pub fn from_markdown(text: &str) -> Clip { from_markdown_with(text, false) }
/// A table's line as its cells (`| a | b |`, the outer bars optional; `\|` is a bar in a cell).
fn md_cells(line: &str) -> Vec<String> {
    let t = line.trim(); let t = t.strip_prefix('|').unwrap_or(t); let t = if t.ends_with('|') && !t.ends_with("\\|") { &t[..t.len() - 1] } else { t };
    let (mut cells, mut cur, mut esc) = (vec![], String::new(), false);
    for c in t.chars() { if esc { if c != '|' { cur.push('\\'); } cur.push(c); esc = false; } else if c == '\\' { esc = true; } else if c == '|' { cells.push(cur.trim().to_string()); cur.clear(); } else { cur.push(c); } }
    if esc { cur.push('\\'); } cells.push(cur.trim().to_string()); cells
}
/// The line under a table's header (`|---|:--:|--:|`) as each column's alignment; None when it is not one.
fn md_rule(line: &str) -> Option<Vec<u8>> {
    if !line.contains('-') || !line.contains('|') { return None }
    md_cells(line).iter().map(|c| { let d = c.trim_matches(':'); if d.is_empty() || !d.chars().all(|x| x == '-') { return None } Some(match (c.starts_with(':'), c.ends_with(':')) { (true, true) => 1, (false, true) => 2, _ => 0 }) }).collect()
}
/// `local`: pictures named by a file's path become blocks too (an imported file's; the importer finds them).
pub fn from_markdown_with(text: &str, local: bool) -> Clip {
    let mut out: Clip = vec![]; let mut para: Vec<String> = vec![]; let mut indents: Vec<usize> = vec![];
    let flush = |out: &mut Clip, para: &mut Vec<String>, kind: Kind| { if para.is_empty() { return }
        let mut joined = String::new(); for (i, l) in para.iter().enumerate() { let hard = l.ends_with("  ") || l.ends_with('\\'); joined.push_str(l.trim_end().trim_end_matches('\\')); if i + 1 < para.len() { joined.push(if hard { '\n' } else { ' ' }); } }
        para.clear(); let (joined, pictures) = images(&joined, local); let (t, s) = inline(joined.trim()); if !t.is_empty() { out.push(vec![block(kind, t, s)]); }
        for src in pictures { out.push(vec![block(Kind::Media { descriptor: if crate::media::remote(&src) { serde_json::json!({ "src": src, "w": 480, "h": 300 }) } else { serde_json::json!({ "src": src }) }.to_string() }, String::new(), Spans::new())]); } };
    let mut lines = text.lines().map(|l| l.replace('\t', "    ")).peekable();
    while let Some(line) = lines.next() {
        let t = line.trim_start(); let indent = line.len() - t.len();
        if let Some(lang) = t.strip_prefix("```").or_else(|| t.strip_prefix("~~~")) {
            flush(&mut out, &mut para, Kind::Paragraph); let fence = &t[..3]; let mut code: Vec<String> = vec![];
            for l in lines.by_ref() { if l.trim_start().starts_with(fence) && l.trim() == fence { break } code.push(l.chars().skip(indent.min(l.len() - l.trim_start().len())).collect()); }
            out.push(vec![block(Kind::Code { lang: lang.trim().to_string() }, code.join("\n"), Spans::new())]); indents.clear(); continue
        }
        if t.is_empty() { flush(&mut out, &mut para, Kind::Paragraph); continue }
        // a table: a line with bars, and under it the line of dashes that makes it the header
        if t.contains('|') { if let Some(align) = lines.peek().and_then(|n| md_rule(n)).filter(|a| a.len() == md_cells(t).len()) {
            flush(&mut out, &mut para, Kind::Paragraph); lines.next(); let cols = align.len();
            let cell = |c: &str| -> Clip { let (x, s) = inline(c); if x.is_empty() { vec![] } else { vec![vec![block(Kind::Paragraph, x, s)]] } };
            let mut rows = vec![(true, md_cells(t).iter().map(|c| cell(c)).collect::<Vec<_>>())];
            while let Some(n) = lines.peek().filter(|n| n.contains('|') && !n.trim().is_empty()) { let mut c: Vec<Clip> = md_cells(n).iter().map(|c| cell(c)).collect(); c.truncate(cols); rows.push((false, c)); lines.next(); }
            out.push(table_tree(rows, align.into_iter().map(|a| ColumnSpec { align: a, ..Default::default() }).collect())); indents.clear(); continue } }
        let rule = t.len() >= 3 && ["-", "*", "_"].iter().any(|c| t.replace(' ', "").chars().all(|x| x.to_string() == *c)) && t.replace(' ', "").len() >= 3;
        if rule { flush(&mut out, &mut para, Kind::Paragraph); out.push(vec![block(Kind::Divider, String::new(), Spans::new())]); indents.clear(); continue }
        let hashes = t.bytes().take_while(|c| *c == b'#').count();
        if (1..=6).contains(&hashes) && t[hashes..].starts_with(' ') { flush(&mut out, &mut para, Kind::Paragraph); let (x, s) = inline(t[hashes..].trim().trim_end_matches('#').trim_end()); if !x.is_empty() { out.push(vec![block(Kind::Heading { level: hashes as u8 }, x, s)]); } indents.clear(); continue }
        if let Some(q) = t.strip_prefix('>') { flush(&mut out, &mut para, Kind::Paragraph); let mut q = vec![q.trim().to_string()]; while let Some(n) = lines.peek().and_then(|l| l.trim_start().strip_prefix('>').map(|x| x.trim().to_string())) { q.push(n); lines.next(); } flush(&mut out, &mut q, Kind::Quote); indents.clear(); continue }
        // a list item: a bullet or a number with a stop, then a space
        let digits = t.bytes().take_while(|c| c.is_ascii_digit()).count();
        let item = if (t.starts_with("- ") || t.starts_with("* ") || t.starts_with("+ ")) && t.len() > 2 { Some((false, &t[2..])) } else if (1..=9).contains(&digits) && (t[digits..].starts_with(". ") || t[digits..].starts_with(") ")) { Some((true, &t[digits + 2..])) } else { None };
        if let Some((numbered, body)) = item {
            flush(&mut out, &mut para, Kind::Paragraph);
            while indents.last().is_some_and(|i| *i > indent) { indents.pop(); }
            if indents.last().is_none_or(|i| *i < indent) { indents.push(indent); }
            let depth = (indents.len() - 1).min(8) as u8; let body = body.trim_start();
            let (kind, body) = match body.get(..4) { Some("[ ] ") => (Kind::Task { depth, state: 0 }, &body[4..]), Some("[x] " | "[X] ") => (Kind::Task { depth, state: 2 }, &body[4..]), Some("[/] ") => (Kind::Task { depth, state: 1 }, &body[4..]), _ => (if numbered { Kind::Numbered { depth } } else { Kind::Bullet { depth } }, body) };
            let (x, s) = inline(body.trim()); if !x.is_empty() { out.push(vec![block(kind, x, s)]); } continue
        }
        if para.is_empty() { indents.clear(); }
        para.push(t.to_string());
    }
    flush(&mut out, &mut para, Kind::Paragraph);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    fn kinds(c: &Clip) -> Vec<(Kind, &str)> { c.iter().map(|t| (t[0].kind.clone(), t[0].text.as_str())).collect() }
    #[test]
    fn html_from_other_applications_becomes_blocks_with_their_formatting() {
        let c = from_html("<meta charset='utf-8'><h2>A  title</h2>\n<p>Some <b>bold</b>, <em>slanted</em> &amp; <a href=\"https://example.com/x\">a <u>link</u></a>.<br>Second line</p><ul><li>one</li><li>two<ul><li>inner</li></ul></li></ul><ol><li><input type=checkbox checked> done</li></ol><blockquote><p>said</p></blockquote><pre><code>let a = 1;\n  b</code></pre><hr><p style='color:red'>plain</p><script>x()</script>");
        assert_eq!(kinds(&c), [(Kind::Heading { level: 2 }, "A title"), (Kind::Paragraph, "Some bold, slanted & a link.\nSecond line"), (Kind::Bullet { depth: 0 }, "one"), (Kind::Bullet { depth: 0 }, "two"), (Kind::Bullet { depth: 1 }, "inner"),
            (Kind::Task { depth: 0, state: 2 }, "done"), (Kind::Quote, "said"), (Kind::Code { lang: String::new() }, "let a = 1;\n  b"), (Kind::Divider, ""), (Kind::Paragraph, "plain")]);
        let p = &c[1][0]; assert!(p.spans.covers(5, 9, &Mark::Bold) && p.spans.covers(11, 18, &Mark::Italic) && p.spans.covers(21, 27, &Mark::Link("https://example.com/x".into())));
        assert!(!p.spans.covers(23, 27, &Mark::Underline), "a link's underline is its own"); assert!(c[9][0].spans.is_empty(), "colours are dropped");
        // Google Docs: a bold wrapper that is not bold, the formatting in styles
        let c = from_html("<b style=\"font-weight:normal;\" id=\"docs-internal-guid-1\"><p dir=\"ltr\"><span style=\"font-weight:700;font-style:italic\">Heavy</span><span style=\"font-weight:400;text-decoration:line-through\"> light</span></p></b>");
        assert_eq!(kinds(&c), [(Kind::Paragraph, "Heavy light")]); let s = &c[0][0].spans;
        assert!(s.covers(0, 5, &Mark::Bold) && s.covers(0, 5, &Mark::Italic) && !s.covers(5, 11, &Mark::Bold) && s.covers(6, 11, &Mark::Strike));
        // Word: list items are paragraphs with a marker of their own
        let c = from_html("<p class=MsoListParagraph style='mso-list:l0 level1 lfo1'><![if !supportLists]><span>&middot;<span>&nbsp;&nbsp;</span></span><![endif]>First</p><p class=MsoListParagraph style='mso-list:l1 level2 lfo2'><![if !supportLists]><span>1.<span>&nbsp;</span></span><![endif]>Second</p><p class=MsoNormal>After<o:p></o:p></p>");
        assert_eq!(kinds(&c), [(Kind::Bullet { depth: 0 }, "First"), (Kind::Numbered { depth: 1 }, "Second"), (Kind::Paragraph, "After")]);
        // a bare run of text, and what a Windows browser marks as the fragment
        assert_eq!(kinds(&from_html("<html><body>before<!--StartFragment--><span>just <i>this</i></span><!--EndFragment-->after</body></html>")), [(Kind::Paragraph, "just this")]);
        assert!(from_html("<img src='x.png'>  ").is_empty());
        // pictures: blocks of their own, named by their address; a tiny one is a symbol and its words stand in
        let c = from_html("<p>Before <img src=\"https://example.com/a/pic.png?x=1\" width=\"640\" height=\"360\"> after <img src='https://example.com/e.png' width=16 height=16 alt=':)'></p><img src='//example.com/b.jpg'>");
        assert_eq!(c.len(), 4); assert_eq!(c[0][0].text, "Before"); assert_eq!(c[2][0].text, "after :)");
        assert!(matches!(&c[1][0].kind, Kind::Media { descriptor } if descriptor.contains("https://example.com/a/pic.png?x=1") && descriptor.contains("\"w\":640")));
        assert!(matches!(&c[3][0].kind, Kind::Media { descriptor } if descriptor.contains("https://example.com/b.jpg") && descriptor.contains("\"h\":300")));
        let c = from_markdown("Text ![a chart](https://example.com/chart.png \"t\") goes on, ![local](./x.png) too.");
        assert_eq!(c.len(), 2); assert_eq!(c[0][0].text, "Text goes on, local too."); assert!(matches!(&c[1][0].kind, Kind::Media { descriptor } if descriptor.contains("chart.png")));
    }
    #[test]
    fn a_table_of_data_is_a_table_and_a_table_of_one_column_is_its_content() {
        let c = from_html("<table><thead><tr><th>Shot</th><th>Note</th></tr></thead><tbody><tr><td>010</td><td><b>ok</b></td></tr><tr><td>020</td></tr></tbody></table><table><tr><td><p>only</p><p>layout</p></td></tr></table>");
        assert_eq!(c.len(), 3); let t = &c[0];
        assert!(matches!(&t[0].kind, Kind::Table { header_rows: 1, columns } if columns.len() == 2)); assert_eq!(t[0].children.len(), 3);
        let texts: Vec<&str> = t.iter().filter(|b| b.kind.is_text()).map(|b| b.text.as_str()).collect(); assert_eq!(texts, ["Shot", "Note", "010", "ok", "020", ""]);
        // every block names its parent, and parents come before their children (as an insert takes them)
        for (i, b) in t.iter().enumerate().skip(1) { let p = t.iter().position(|x| Some(x.id) == b.parent).expect("a parent in the tree"); assert!(p < i && t[p].children.contains(&b.id)); }
        assert_eq!(kinds(&c[1..].to_vec()), [(Kind::Paragraph, "only"), (Kind::Paragraph, "layout")]);
    }
    #[test]
    fn markdown_source_becomes_blocks() {
        let c = from_markdown("# Title *one*\n\nA line\nwrapped, with **bold**, `co*de`, [a link](https://example.com) and snake_case_word.  \nkept break\n\n- item\n  - inner [x] no\n  - [x] done\n1. first\n\n> quoted\n> more\n\n```rust\nfn a() {}\n\n  b\n```\n---\nlast \\*plain\\*");
        assert_eq!(kinds(&c), [(Kind::Heading { level: 1 }, "Title one"), (Kind::Paragraph, "A line wrapped, with bold, co*de, a link and snake_case_word.\nkept break"), (Kind::Bullet { depth: 0 }, "item"), (Kind::Bullet { depth: 1 }, "inner [x] no"), (Kind::Task { depth: 1, state: 2 }, "done"),
            (Kind::Numbered { depth: 0 }, "first"), (Kind::Quote, "quoted more"), (Kind::Code { lang: "rust".into() }, "fn a() {}\n\n  b"), (Kind::Divider, ""), (Kind::Paragraph, "last *plain*")]);
        assert!(c[0][0].spans.covers(6, 9, &Mark::Italic)); let p = &c[1][0];
        assert!(p.spans.covers(21, 25, &Mark::Bold) && p.spans.covers(27, 32, &Mark::Code) && p.spans.covers(34, 40, &Mark::Link("https://example.com".into())));
        assert_eq!(p.spans.as_slice().len(), 3, "the word's underscores are not marks");
    }
}
