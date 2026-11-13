//! Office documents as a note's blocks (canon row 335, stage 2): Word (`.docx`), Excel (`.xlsx`), OpenDocument text
//! and sheets (`.odt`, `.ods`). Each is a zip of XML parts, read with `xml.rs`.
//!
//! Word: the reader takes back what minNotes' own Word export writes (its styles: Heading1-6, Quote, Code with its
//! CodeLang line, ListParagraph with a numbering, the task boxes drawn as characters, the rule as a bottom border,
//! the NoteName header line - which is left out) and makes a decent job of other people's documents: headings by
//! style or outline level, lists by the numbering's kind, bold / italic / underline / strike, colours and highlights
//! (not greys), links, pictures by relationship, tables with header rows, merged cells and column alignment.
//! Not read: review comments, footnotes, text boxes' fallbacks, cell backgrounds, column widths.
//!
//! Sheets: every sheet with content is a table (its first row the header), under a level-2 heading with its name when
//! there are several. Dates are written as dates (the Qt app left Excel's serial numbers), a formula gives its last
//! value, a merged cell keeps its value in its first cell. Pictures in sheets are not read.
use crate::media::Places;
use crate::xml::{attr, tokens, Tok};
use mn_doc::{Block, BlockId, ColumnSpec, Kind, Mark, MarkKind, Rank, Spans};
use mn_edit::Clip;
use std::collections::HashMap;
use std::io::Read;
use std::path::Path;

fn block(kind: Kind, text: String, spans: Spans) -> Block { Block { id: BlockId::new(), parent: None, rank: Rank::first(), kind, text, spans, conflict: None, children: vec![] } }

struct Zip(zip::ZipArchive<std::fs::File>);
impl Zip {
    fn open(path: &Path) -> Result<Zip, String> { Ok(Zip(zip::ZipArchive::new(std::fs::File::open(path).map_err(|e| e.to_string())?).map_err(|_| "The file is not a document of its kind (it is not a zip).".to_string())?)) }
    fn bytes(&mut self, name: &str) -> Option<Vec<u8>> { let mut f = self.0.by_name(name.trim_start_matches('/')).ok()?; if f.size() > 1 << 30 { return None } let mut b = Vec::with_capacity(f.size() as usize); f.read_to_end(&mut b).ok()?; Some(b) }
    fn text(&mut self, name: &str) -> Option<String> { self.bytes(name).map(|b| crate::import::decode(&b)) }
}

/// Runs of code paragraphs are one code block.
fn join_code(clip: Clip) -> Clip {
    let mut out: Clip = vec![];
    for t in clip {
        if let (Some(last), true) = (out.last_mut().filter(|l| l.len() == 1), t.len() == 1) { if let (Kind::Code { .. }, Kind::Code { .. }) = (&last[0].kind, &t[0].kind) { last[0].text.push('\n'); last[0].text.push_str(&t[0].text); continue } }
        out.push(t);
    }
    out
}

// ── Word

#[derive(Default)]
struct Para { text: String, spans: Spans, style: String, outline: Option<u8>, num: Option<i64>, level: u8, left: i64, jc: u8, rule: bool, pictures: Vec<String>, mono: bool, any: bool }

struct Docx<'a> { t: &'a [Tok], i: usize, rels: HashMap<String, String>, ordered: HashMap<i64, bool>, zip: &'a mut Zip, places: &'a Places, lost: usize, lang: Option<String> }

fn on(attrs: &[(String, String)]) -> bool { !matches!(attr(attrs, "w:val"), Some("0" | "false" | "none")) }
fn highlight(name: &str) -> Option<&'static str> {
    Some(match name { "yellow" => "#ffff00", "green" => "#00ff00", "cyan" => "#00ffff", "magenta" => "#ff00ff", "blue" => "#0000ff", "red" => "#ff0000", "darkYellow" => "#808000", "darkGreen" => "#008000", "darkCyan" => "#008080", "darkMagenta" => "#800080", "darkBlue" => "#000080", "darkRed" => "#800000", _ => return None })
}

impl Docx<'_> {
    /// A paragraph, from after its opening tag to its closing one.
    fn paragraph(&mut self) -> Para {
        let mut p = Para { mono: true, ..Default::default() };
        let (mut depth, mut in_ppr, mut in_run, mut in_rpr, mut in_t, mut skip) = (0usize, false, false, false, false, 0usize);
        let mut marks: Vec<Mark> = vec![]; let mut mono = false; let mut link: Option<String> = None; let mut cx: Option<f64> = None;
        // a link written as a field: its instruction (`HYPERLINK "address"`), then the text it shows
        let (mut in_instr, mut instr) = (false, String::new()); let field = |i: &str| i.trim().strip_prefix("HYPERLINK").and_then(|r| r.split('"').nth(1)).filter(|h| !h.is_empty()).map(|h| h.to_string());
        while self.i < self.t.len() {
            let tok = &self.t[self.i]; self.i += 1;
            match tok {
                Tok::Close(n) => match n.as_str() {
                    "w:p" => { if depth == 0 { break } depth -= 1; }
                    "mc:Fallback" => skip = skip.saturating_sub(1),
                    "w:pPr" => in_ppr = false, "w:r" => { in_run = false; marks.clear(); mono = false; } "w:rPr" => in_rpr = false, "w:t" => in_t = false, "w:hyperlink" | "w:fldSimple" => link = None, "w:instrText" => in_instr = false,
                    _ => {}
                },
                Tok::Text(x) if in_instr => instr.push_str(x),
                Tok::Text(x) => if in_t && skip == 0 && depth == 0 {
                    let at = p.text.len(); p.text.push_str(x); let end = p.text.len(); if !x.trim().is_empty() { p.any = true; p.mono &= mono; }
                    for m in &marks { if link.is_some() && matches!(m.kind(), MarkKind::Color | MarkKind::Underline) { continue } p.spans.put(at, end, m.clone()); }
                    if mono { p.spans.put(at, end, Mark::Code); } if let Some(l) = &link { p.spans.put(at, end, Mark::Link(l.clone())); }
                },
                Tok::Open { name, attrs, empty } => {
                    if name == "mc:Fallback" { if !empty { skip += 1; } continue } if skip > 0 { continue }
                    if name == "w:p" { if !empty { depth += 1; } continue } if depth > 0 { continue }
                    let val = attr(attrs, "w:val");
                    match name.as_str() {
                        "w:pPr" => in_ppr = !empty,
                        "w:pStyle" if in_ppr => p.style = val.unwrap_or("").to_ascii_lowercase(),
                        "w:outlineLvl" if in_ppr => p.outline = val.and_then(|v| v.parse::<u8>().ok()).filter(|v| *v < 9).map(|v| v.min(5) + 1),
                        "w:numId" if in_ppr => p.num = val.and_then(|v| v.parse().ok()).filter(|v| *v != 0),
                        "w:ilvl" if in_ppr => p.level = val.and_then(|v| v.parse::<u8>().ok()).unwrap_or(0).min(8),
                        "w:jc" if in_ppr => p.jc = match val { Some("center") => 1, Some("right" | "end") => 2, _ => 0 },
                        "w:ind" if in_ppr => p.left = attr(attrs, "w:left").or(attr(attrs, "w:start")).and_then(|v| v.parse().ok()).unwrap_or(0),
                        "w:bottom" if in_ppr => p.rule = !matches!(val, Some("none" | "nil")),
                        "w:r" => { in_run = !empty; marks.clear(); mono = false; }
                        "w:rPr" if in_run && !in_ppr => in_rpr = !empty,
                        "w:b" if in_rpr => if on(attrs) { marks.push(Mark::Bold) },
                        "w:i" if in_rpr => if on(attrs) { marks.push(Mark::Italic) },
                        "w:u" if in_rpr => if on(attrs) { marks.push(Mark::Underline) },
                        "w:strike" | "w:dstrike" if in_rpr => if on(attrs) { marks.push(Mark::Strike) },
                        "w:color" if in_rpr => if let Some(c) = val.filter(|v| *v != "auto").and_then(|v| crate::paste::css_colour(&format!("#{v}"))) { marks.push(Mark::Color(c)) },
                        "w:highlight" if in_rpr => if let Some(c) = val.and_then(highlight) { marks.retain(|m| m.kind() != MarkKind::Highlight); marks.push(Mark::Highlight(c.into())) },
                        "w:shd" if in_rpr => if let Some(c) = attr(attrs, "w:fill").filter(|v| !v.eq_ignore_ascii_case("auto")).and_then(|v| crate::paste::css_colour(&format!("#{v}"))) { if !marks.iter().any(|m| m.kind() == MarkKind::Highlight) { marks.push(Mark::Highlight(c)) } },
                        "w:rFonts" if in_rpr => mono = attr(attrs, "w:ascii").is_some_and(|f| { let f = f.to_ascii_lowercase(); f.starts_with("courier") || f.contains("mono") || f.starts_with("consolas") || f.starts_with("menlo") }),
                        "w:t" if in_run => in_t = !empty,
                        "w:instrText" if in_run => in_instr = !empty,
                        "w:fldChar" => match attr(attrs, "w:fldCharType") { Some("begin") => instr.clear(), Some("separate") => link = field(&instr), Some("end") => { link = None; instr.clear(); } _ => {} },
                        "w:fldSimple" => link = attr(attrs, "w:instr").and_then(field),
                        "w:br" | "w:cr" if in_run => if attr(attrs, "w:type") != Some("page") && !p.text.is_empty() { p.text.push('\n') },
                        "w:tab" if in_run && !in_ppr => p.text.push('\t'),
                        "w:noBreakHyphen" if in_run => p.text.push('-'),
                        "w:hyperlink" => link = attr(attrs, "r:id").and_then(|id| self.rels.get(id)).filter(|h| !h.is_empty()).cloned(),
                        "wp:extent" => cx = attr(attrs, "cx").and_then(|v| v.parse::<f64>().ok()).map(|v| v / 9525.0),
                        "a:blip" => if let Some(target) = attr(attrs, "r:embed").and_then(|id| self.rels.get(id)).cloned() {
                            let part = if let Some(abs) = target.strip_prefix('/') { abs.to_string() } else { format!("word/{target}") };
                            let name = part.rsplit('/').next().unwrap_or("picture").to_string();
                            match self.zip.bytes(&part).ok_or(String::new()).and_then(|b| crate::media::kept(self.places, &b, &name)) {
                                Ok(rec) => p.pictures.push(sized(rec, cx.take())), Err(_) => self.lost += 1 } },
                        _ => {}
                    }
                }
            }
        }
        p
    }
    /// A read paragraph as blocks, into `out`.
    fn emit(&mut self, mut p: Para, out: &mut Clip) {
        let keep = p.text.trim_end().len(); if keep < p.text.len() { p.spans.edit(keep, p.text.len() - keep, 0); p.text.truncate(keep); }
        let style = p.style.replace(' ', "");
        // our own export's lines around a block: the note's name (left out) and a code block's language (kept for it)
        if style == "notename" { return } if style == "codelang" { self.lang = Some(p.text.trim().to_string()); return }
        let lang = self.lang.take();
        if p.text.trim().is_empty() { if p.rule && p.pictures.is_empty() { out.push(vec![block(Kind::Divider, String::new(), Spans::new())]); } }
        else {
            let heading = style.strip_prefix("heading").and_then(|n| n.parse::<u8>().ok()).filter(|n| (1..=9).contains(n)).map(|n| n.min(6)).or(if style == "title" { Some(1) } else if style == "subtitle" { Some(2) } else { None }).or(p.outline);
            let mut kind = if let Some(level) = heading { Kind::Heading { level } }
                else if style == "code" || style.starts_with("htmlpre") || style == "sourcecode" || (p.any && p.mono && p.num.is_none()) { Kind::Code { lang: lang.unwrap_or_default() } }
                else if style == "quote" || style == "intensequote" || style == "blockquote" { Kind::Quote }
                else if let Some(n) = p.num { if self.ordered.get(&n).copied().unwrap_or(false) { Kind::Numbered { depth: p.level } } else { Kind::Bullet { depth: p.level } } }
                else { Kind::Paragraph };
            // a task box drawn as a character at the line's start (our export; other people's check lists)
            if matches!(kind, Kind::Paragraph | Kind::Bullet { .. }) { for (mark, state) in [('\u{2610}', 0u8), ('\u{25EA}', 1), ('\u{25D0}', 1), ('\u{2611}', 2), ('\u{2612}', 2)] {
                if let Some(rest) = p.text.strip_prefix(mark).filter(|r| r.starts_with(['\t', ' '])) { let cut = p.text.len() - rest.len() + 1;
                    let depth = match kind { Kind::Bullet { depth } => depth, _ => (p.left / 360 - 1).clamp(0, 8) as u8 };
                    p.spans.edit(0, cut, 0); p.text.drain(..cut); kind = Kind::Task { depth, state }; break } } }
            // a list written as text: a bullet character and a tab, further in by a tab a level
            if kind == Kind::Paragraph { let t = p.text.trim_start_matches('\t'); let depth = (p.text.len() - t.len()).min(8) as u8; if let Some(rest) = ['\u{2022}', '\u{25e6}', '\u{25aa}', '\u{b7}'].iter().find_map(|g| t.strip_prefix(*g)).filter(|r| r.starts_with('\t')) { let cut = p.text.len() - rest.len() + 1; p.spans.edit(0, cut, 0); p.text.drain(..cut); kind = Kind::Bullet { depth }; } }
            // a tab is not drawn in a note's text
            if !matches!(kind, Kind::Code { .. }) { p.text = p.text.replace('\t', " "); }
            match kind { Kind::Heading { .. } => p.spans.remove(0, p.text.len(), MarkKind::Bold), Kind::Code { .. } => p.spans = Spans::new(), _ => {} }
            if !matches!(kind, Kind::Code { .. }) { let lead = p.text.len() - p.text.trim_start().len(); if lead > 0 { p.spans.edit(0, lead, 0); p.text.drain(..lead); } }
            out.push(vec![block(kind, p.text, p.spans)]);
        }
        for rec in p.pictures { out.push(vec![block(Kind::Media { descriptor: rec }, String::new(), Spans::new())]); }
    }
    /// Blocks until `until` closes (the body, a cell).
    fn blocks(&mut self, until: &str, first_jc: &mut Option<u8>) -> Clip {
        let mut out: Clip = vec![]; let mut skip = 0usize;
        while self.i < self.t.len() {
            let tok = &self.t[self.i]; self.i += 1;
            match tok {
                Tok::Close(n) if n == until && skip == 0 => break,
                Tok::Close(n) if n == "mc:Fallback" => skip = skip.saturating_sub(1),
                Tok::Open { name, empty, .. } if name == "mc:Fallback" => { if !empty { skip += 1; } }
                Tok::Open { name, empty: false, .. } if skip == 0 && name == "w:p" => { let p = self.paragraph(); first_jc.get_or_insert(p.jc); self.emit(p, &mut out); }
                Tok::Open { name, empty: false, .. } if skip == 0 && name == "w:tbl" => { let t = self.table(); out.extend(t); }
                _ => {}
            }
        }
        out
    }
    /// A table, from after its opening tag: its tree - or, of one column (layout) or inside a cell, its content.
    fn table(&mut self) -> Clip {
        let mut rows: Vec<(bool, Vec<Clip>)> = vec![]; let mut align: Vec<u8> = vec![];
        while self.i < self.t.len() {
            let tok = &self.t[self.i]; self.i += 1;
            match tok {
                Tok::Close(n) if n == "w:tbl" => break,
                Tok::Open { name, empty, .. } => match name.as_str() {
                    "w:tr" if !empty => rows.push((false, vec![])),
                    "w:tblHeader" => if let Some(r) = rows.last_mut() { r.0 = true },
                    "w:tc" if !empty && !rows.is_empty() => {
                        // the cell's own properties come first, then its content
                        let (mut span, mut covered) = (1usize, false);
                        while self.i < self.t.len() { match &self.t[self.i] {
                            Tok::Open { name, attrs, .. } if name == "w:gridSpan" => span = attr(attrs, "w:val").and_then(|v| v.parse().ok()).unwrap_or(1).clamp(1, 64),
                            Tok::Open { name, attrs, .. } if name == "w:vMerge" => covered = !matches!(attr(attrs, "w:val"), Some("restart")),
                            Tok::Open { name, .. } if name == "w:p" || name == "w:tbl" => break, Tok::Close(n) if n == "w:tc" => break, _ => {} } self.i += 1; }
                        let mut jc = None; let inner = self.blocks("w:tc", &mut jc);
                        // a cell holds leaves: a table inside it gives its text, a line a row
                        let mut cell: Clip = vec![]; for tree in inner { if tree.len() == 1 { cell.push(tree); } else { for b in tree.into_iter().filter(|b| b.kind.is_text() && !b.text.is_empty()) { cell.push(vec![block(Kind::Paragraph, b.text, b.spans)]); } } }
                        if covered { cell.clear(); }
                        let row = rows.last_mut().unwrap(); let col = row.1.len();
                        if align.len() <= col { align.resize(col + 1, 0); } if align[col] == 0 && !covered { align[col] = jc.unwrap_or(0); }
                        row.1.push(cell); for _ in 1..span { row.1.push(vec![]); }
                    }
                    _ => {}
                },
                _ => {}
            }
        }
        let cols = rows.iter().map(|r| r.1.len()).max().unwrap_or(0);
        if cols < 2 { return rows.into_iter().flat_map(|r| r.1).flatten().collect() }
        align.resize(cols, 0);
        vec![crate::paste::table_tree(rows, align.into_iter().map(|a| ColumnSpec { align: a, ..Default::default() }).collect())]
    }
}

/// A picture's record with the width the document shows it at (when that is smaller than the picture).
fn sized(rec: String, shown: Option<f64>) -> String {
    let (Some(w), Ok(mut v)) = (shown.filter(|w| *w >= 16.0), serde_json::from_str::<serde_json::Value>(&rec)) else { return rec };
    if let (Some(pw), Some(ph)) = (v.get("w").and_then(|x| x.as_f64()), v.get("h").and_then(|x| x.as_f64())) { if pw > 0.0 && w < pw { v["w"] = serde_json::json!(w.round()); v["h"] = serde_json::json!((w * ph / pw).round()); } }
    v.to_string()
}

pub fn docx(path: &Path, places: &Places) -> Result<(Clip, usize), String> {
    let mut zip = Zip::open(path)?;
    let doc = zip.text("word/document.xml").ok_or("The file has no Word document in it.")?;
    let mut rels = HashMap::new();
    if let Some(x) = zip.text("word/_rels/document.xml.rels") { for t in tokens(&x) { if let Tok::Open { name, attrs, .. } = t { if name == "Relationship" { if let (Some(id), Some(target)) = (attr(&attrs, "Id"), attr(&attrs, "Target")) { rels.insert(id.to_string(), target.to_string()); } } } } }
    // which numberings count: a numbering's kind is its first level's format (the Qt reader's rule)
    let mut ordered: HashMap<i64, bool> = HashMap::new();
    match zip.text("word/numbering.xml") {
        None => { ordered.insert(1, false); ordered.insert(2, true); }
        Some(x) => { let (mut abs, mut of): (HashMap<i64, bool>, HashMap<i64, i64>) = Default::default(); let (mut cur, mut seen, mut num) = (None, false, None);
            for t in tokens(&x) { let Tok::Open { name, attrs, .. } = t else { continue }; let val = attr(&attrs, "w:val");
                match name.as_str() {
                    "w:abstractNum" => { cur = attr(&attrs, "w:abstractNumId").and_then(|v| v.parse::<i64>().ok()); seen = false; num = None; }
                    "w:numFmt" if !seen => if let Some(a) = cur { abs.insert(a, val != Some("bullet")); seen = true; },
                    "w:num" => { num = attr(&attrs, "w:numId").and_then(|v| v.parse::<i64>().ok()); cur = None; }
                    "w:abstractNumId" => if let (Some(n), Some(a)) = (num, val.and_then(|v| v.parse::<i64>().ok())) { of.insert(n, a); },
                    _ => {} } }
            for (n, a) in of { ordered.insert(n, abs.get(&a).copied().unwrap_or(false)); } }
    }
    let toks = tokens(&doc);
    let start = toks.iter().position(|t| matches!(t, Tok::Open { name, .. } if name == "w:body")).map(|i| i + 1).unwrap_or(0);
    let mut d = Docx { t: &toks, i: start, rels, ordered, zip: &mut zip, places, lost: 0, lang: None };
    let clip = d.blocks("w:body", &mut None);
    Ok((join_code(clip), d.lost))
}

// ── sheets (Excel, OpenDocument)

/// One sheet: its name and its cells that hold something (row, column, text).
struct Sheet { name: String, cells: Vec<(usize, usize, String)> }

/// Sheets as blocks: a table each (the first row its header), a heading before each when there are several.
fn sheets(all: Vec<Sheet>) -> Result<Clip, String> {
    let all: Vec<Sheet> = all.into_iter().filter(|s| !s.cells.is_empty()).collect(); let many = all.len() > 1; let mut out: Clip = vec![];
    for s in all {
        let rows = s.cells.iter().map(|c| c.0).max().unwrap_or(0) + 1; let cols = s.cells.iter().map(|c| c.1).max().unwrap_or(0) + 1;
        if rows.saturating_mul(cols) > 4_000_000 { return Err(format!("The sheet \u{201c}{}\u{201d} is too large to be a note ({rows} rows, {cols} columns).", s.name)) }
        let mut grid: Vec<Vec<Clip>> = vec![vec![vec![]; cols]; rows];
        for (r, c, text) in s.cells { grid[r][c] = vec![vec![block(Kind::Paragraph, text, Spans::new())]]; }
        if many { out.push(vec![block(Kind::Heading { level: 2 }, s.name, Spans::new())]); }
        out.push(crate::paste::table_tree(grid.into_iter().enumerate().map(|(i, r)| (i == 0 && rows > 1, r)).collect(), vec![ColumnSpec::default(); cols]));
    }
    Ok(out)
}

/// `BC12` as (row, column), from 0.
fn cell_ref(r: &str) -> Option<(usize, usize)> {
    let letters = r.bytes().take_while(|b| b.is_ascii_alphabetic()).count(); if letters == 0 || letters > 3 { return None }
    let col = r[..letters].bytes().fold(0usize, |a, b| a * 26 + (b.to_ascii_uppercase() - b'A') as usize + 1); let row: usize = r[letters..].parse().ok()?;
    Some((row.checked_sub(1)?, col - 1))
}
/// A day count from 1970-01-01 as a date (the civil calendar).
fn civil(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468; let era = z.div_euclid(146_097); let doe = z.rem_euclid(146_097); let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); let mp = (5 * doy + 2) / 153; let d = (doy - (153 * mp + 2) / 5 + 1) as u32; let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (yoe + era * 400 + if m <= 2 { 1 } else { 0 }, m, d)
}
/// Excel's number for a moment, written as a date, a time, or both.
fn serial(v: f64, from_1904: bool) -> String {
    let secs = (v * 86_400.0).round() as i64; let (days, rest) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    let time = if rest % 60 == 0 { format!("{:02}:{:02}", rest / 3600, rest / 60 % 60) } else { format!("{:02}:{:02}:{:02}", rest / 3600, rest / 60 % 60, rest % 60) };
    if days == 0 && !from_1904 { return time }
    let (y, m, d) = civil(days - 25_569 + if from_1904 { 1462 } else { 0 });
    if rest == 0 { format!("{y:04}-{m:02}-{d:02}") } else { format!("{y:04}-{m:02}-{d:02} {time}") }
}
/// A number format that shows a date or a time (Excel's built-in ones by their number, the others by their letters).
fn dated(id: u32, code: Option<&str>) -> bool {
    if matches!(id, 14..=22 | 27..=36 | 45..=47 | 50..=58) { return true }
    let Some(code) = code else { return false }; let (mut quoted, mut bracket) = (false, false);
    code.chars().any(|c| { match c { '"' => quoted = !quoted, '[' => bracket = true, ']' => bracket = false, _ => {} } !quoted && !bracket && matches!(c.to_ascii_lowercase(), 'y' | 'd' | 'h' | 's' | 'm') })
}
/// A number as Excel stored it, without the noise of its binary form (`0.30000000000000004`).
fn number(v: &str) -> String {
    if v.len() < 16 || !v.contains('.') { return v.to_string() }
    match v.parse::<f64>() { Ok(f) => { let s = format!("{:.10}", f); s.trim_end_matches('0').trim_end_matches('.').to_string() } Err(_) => v.to_string() }
}

pub fn xlsx(path: &Path) -> Result<Clip, String> {
    let mut zip = Zip::open(path)?;
    let book = zip.text("xl/workbook.xml").ok_or("The file has no workbook in it.")?;
    let mut rels: HashMap<String, String> = HashMap::new();
    if let Some(x) = zip.text("xl/_rels/workbook.xml.rels") { for t in tokens(&x) { if let Tok::Open { name, attrs, .. } = t { if name == "Relationship" { if let (Some(id), Some(target)) = (attr(&attrs, "Id"), attr(&attrs, "Target")) { rels.insert(id.to_string(), target.to_string()); } } } } }
    // the texts cells share: each `si`'s text runs, without the phonetic guides
    let mut shared: Vec<String> = vec![];
    if let Some(x) = zip.text("xl/sharedStrings.xml") { let (mut cur, mut in_t, mut guide) = (None::<String>, false, 0usize);
        for t in tokens(&x) { match t {
            Tok::Open { name, empty, .. } => match name.as_str() { "si" => { if empty { shared.push(String::new()) } else { cur = Some(String::new()) } } "rPh" if !empty => guide += 1, "t" if !empty => in_t = true, _ => {} },
            Tok::Close(n) => match n.as_str() { "si" => shared.push(cur.take().unwrap_or_default()), "rPh" => guide = guide.saturating_sub(1), "t" => in_t = false, _ => {} },
            Tok::Text(x) => if in_t && guide == 0 { if let Some(c) = cur.as_mut() { c.push_str(&x); } },
        } } }
    // which cell styles show dates
    let mut date_style: Vec<bool> = vec![];
    if let Some(x) = zip.text("xl/styles.xml") { let mut codes: HashMap<u32, String> = HashMap::new(); let mut in_xfs = false;
        for t in tokens(&x) { match t {
            Tok::Open { name, attrs, empty } => match name.as_str() {
                "numFmt" => if let (Some(id), Some(code)) = (attr(&attrs, "numFmtId").and_then(|v| v.parse().ok()), attr(&attrs, "formatCode")) { codes.insert(id, code.to_string()); },
                "cellXfs" => in_xfs = !empty,
                "xf" if in_xfs => { let id = attr(&attrs, "numFmtId").and_then(|v| v.parse::<u32>().ok()).unwrap_or(0); date_style.push(dated(id, codes.get(&id).map(|s| s.as_str()))); }
                _ => {} },
            Tok::Close(n) if n == "cellXfs" => in_xfs = false, _ => {} } } }
    let from_1904 = book.contains("date1904=\"1\"") || book.contains("date1904=\"true\"");
    let mut all = vec![];
    for t in tokens(&book) {
        let Tok::Open { name, attrs, .. } = t else { continue }; if name != "sheet" { continue }
        let Some(target) = attr(&attrs, "r:id").and_then(|id| rels.get(id)) else { continue };
        let part = if let Some(abs) = target.strip_prefix('/') { abs.to_string() } else { format!("xl/{target}") };
        let Some(x) = zip.text(&part) else { continue };
        let mut sheet = Sheet { name: attr(&attrs, "name").unwrap_or("Sheet").to_string(), cells: vec![] };
        let (mut row, mut col) = (0usize, 0usize); let (mut kind, mut style, mut at) = (String::new(), None::<usize>, None::<(usize, usize)>); let (mut in_v, mut in_is, mut text) = (false, false, String::new());
        for t in tokens(&x) { match t {
            Tok::Open { name, attrs, empty } => match name.as_str() {
                "row" => { row = attr(&attrs, "r").and_then(|v| v.parse::<usize>().ok()).map(|r| r.saturating_sub(1)).unwrap_or(if sheet.cells.is_empty() && at.is_none() { 0 } else { row + 1 }); col = 0; }
                "c" => { let here = attr(&attrs, "r").and_then(cell_ref).unwrap_or((row, col)); col = here.1 + 1; row = here.0;
                    if !empty { at = Some(here); kind = attr(&attrs, "t").unwrap_or("n").to_string(); style = attr(&attrs, "s").and_then(|v| v.parse().ok()); text.clear(); } }
                "v" if at.is_some() && !empty => in_v = true, "is" if at.is_some() && !empty => in_is = true, _ => {} },
            Tok::Text(x) => if in_v || in_is { text.push_str(&x) },
            Tok::Close(n) => match n.as_str() { "v" => in_v = false, "is" => in_is = false,
                "c" => if let Some(here) = at.take() { let raw = std::mem::take(&mut text);
                    let shown = match kind.as_str() { "s" => raw.trim().parse::<usize>().ok().and_then(|i| shared.get(i).cloned()).unwrap_or_default(), "b" => if raw.trim() == "1" { "TRUE".into() } else { "FALSE".into() }, "str" | "inlineStr" | "e" => raw,
                        _ => match (style.and_then(|s| date_style.get(s).copied()).unwrap_or(false), raw.trim().parse::<f64>()) { (true, Ok(v)) if v >= 0.0 => serial(v, from_1904), _ => number(raw.trim()) } };
                    if !shown.trim().is_empty() && here.0 < 1_048_576 && here.1 < 16_384 { sheet.cells.push((here.0, here.1, shown.trim_end().to_string())); } },
                _ => {} },
        } }
        all.push(sheet);
    }
    sheets(all)
}

// ── OpenDocument

#[derive(Clone, Default)]
struct Style { marks: Vec<Mark>, parent: String, code: bool, quote: bool }

struct Odf<'a> { t: &'a [Tok], i: usize, styles: HashMap<String, Style>, ordered: HashMap<String, bool>, zip: &'a mut Zip, places: &'a Places, lost: usize, lists: Vec<bool> }

/// The styles a document's content names: what each turns on, and which list styles count.
fn odf_styles(toks: &[Tok], styles: &mut HashMap<String, Style>, ordered: &mut HashMap<String, bool>) {
    let (mut cur, mut list): (Option<(String, Style)>, Option<String>) = (None, None);
    for t in toks { match t {
        Tok::Open { name, attrs, empty } => match name.as_str() {
            "style:style" => { let n = attr(attrs, "style:name").unwrap_or("").to_string(); let parent = attr(attrs, "style:parent-style-name").unwrap_or("").to_string();
                let s = Style { code: n.starts_with("Preformatted") || parent.starts_with("Preformatted") || n.starts_with("Source_20_Text"), quote: n.starts_with("Quotations") || parent.starts_with("Quotations"), parent, ..Default::default() };
                if *empty { styles.insert(n, s); } else { cur = Some((n, s)); } }
            "style:text-properties" => if let Some((_, s)) = cur.as_mut() {
                if attr(attrs, "fo:font-weight").is_some_and(|v| v == "bold" || v.parse::<u32>().is_ok_and(|n| n >= 600)) { s.marks.push(Mark::Bold); }
                if attr(attrs, "fo:font-style").is_some_and(|v| v == "italic" || v == "oblique") { s.marks.push(Mark::Italic); }
                if attr(attrs, "style:text-underline-style").is_some_and(|v| v != "none") { s.marks.push(Mark::Underline); }
                if attr(attrs, "style:text-line-through-style").is_some_and(|v| v != "none") { s.marks.push(Mark::Strike); }
                if let Some(c) = attr(attrs, "fo:color").and_then(crate::paste::css_colour) { s.marks.push(Mark::Color(c)); }
                if let Some(c) = attr(attrs, "fo:background-color").and_then(crate::paste::css_colour) { s.marks.push(Mark::Highlight(c)); }
                if attr(attrs, "style:font-name").is_some_and(|f| { let f = f.to_ascii_lowercase(); f.contains("mono") || f.starts_with("courier") }) { s.marks.push(Mark::Code); } },
            "text:list-style" => list = attr(attrs, "style:name").map(|s| s.to_string()),
            "text:list-level-style-number" | "text:list-level-style-bullet" => if let (Some(l), Some("1")) = (&list, attr(attrs, "text:level")) { ordered.insert(l.clone(), name.ends_with("number")); },
            _ => {} },
        Tok::Close(n) => match n.as_str() { "style:style" => if let Some((n, s)) = cur.take() { styles.insert(n, s); }, "text:list-style" => list = None, _ => {} },
        _ => {}
    } }
}

impl Odf<'_> {
    fn style(&self, name: Option<&str>) -> Style {
        let Some(mut s) = name.and_then(|n| self.styles.get(n)).cloned() else { return Style::default() };
        // what its parent turns on, one step up (automatic styles sit on named ones)
        if let Some(p) = self.styles.get(&s.parent) { for m in &p.marks { if !s.marks.iter().any(|x| x.kind() == m.kind()) { s.marks.push(m.clone()); } } s.code |= p.code; s.quote |= p.quote; }
        s
    }
    /// A paragraph or a heading, from after its opening tag (`name`) to its close: its text, marks and pictures.
    fn paragraph(&mut self, name: &str) -> (String, Spans, Vec<String>) {
        let (mut text, mut spans, mut pictures) = (String::new(), Spans::new(), vec![]); let (mut depth, mut skip) = (0usize, 0usize); let mut on: Vec<(String, Vec<Mark>)> = vec![];
        while self.i < self.t.len() {
            let tok = &self.t[self.i]; self.i += 1;
            match tok {
                Tok::Close(n) => { if n == name && skip == 0 { if depth == 0 { break } depth -= 1; } else if n == "text:note" || n == "draw:text-box" || n == "office:annotation" { skip = skip.saturating_sub(1); } else if let Some(k) = on.iter().rposition(|x| &x.0 == n) { on.truncate(k); } }
                Tok::Text(x) => if skip == 0 { let at = text.len(); text.push_str(x); let mut seen: Vec<MarkKind> = vec![]; let linked = on.iter().any(|x| x.1.iter().any(|m| m.kind() == MarkKind::Link)); for m in on.iter().rev().flat_map(|x| x.1.iter()) { if linked && matches!(m.kind(), MarkKind::Color | MarkKind::Underline) { continue } if !seen.contains(&m.kind()) { seen.push(m.kind()); spans.put(at, text.len(), m.clone()); } } },
                Tok::Open { name: n, attrs, empty } => {
                    if matches!(n.as_str(), "text:note" | "draw:text-box" | "office:annotation") { if !empty { skip += 1; } continue } if skip > 0 { continue }
                    match n.as_str() {
                        x if x == name => { if !empty { depth += 1; } }
                        "text:span" if !empty => { let s = self.style(attr(attrs, "text:style-name")); on.push((n.clone(), s.marks)); }
                        "text:a" if !empty => on.push((n.clone(), attr(attrs, "xlink:href").filter(|h| !h.is_empty() && !h.starts_with('#')).map(|h| vec![Mark::Link(h.to_string())]).unwrap_or_default())),
                        "text:s" => { let c = attr(attrs, "text:c").and_then(|v| v.parse::<usize>().ok()).unwrap_or(1).min(200); text.push_str(&" ".repeat(c)); }
                        "text:tab" => text.push(' '), "text:line-break" => text.push('\n'),
                        "draw:image" => if let Some(href) = attr(attrs, "xlink:href").filter(|h| !h.contains("://")) { let name = href.rsplit('/').next().unwrap_or("picture").to_string();
                            match self.zip.bytes(href.trim_start_matches("./")).ok_or(String::new()).and_then(|b| crate::media::kept(self.places, &b, &name)) { Ok(rec) => pictures.push(rec), Err(_) => self.lost += 1 } },
                        _ => {}
                    }
                }
            }
        }
        (text, spans, pictures)
    }
    fn blocks(&mut self, until: &str) -> Clip {
        let mut out: Clip = vec![]; let mut inherited: Vec<Option<bool>> = vec![];
        while self.i < self.t.len() {
            let tok = &self.t[self.i]; self.i += 1;
            match tok {
                Tok::Close(n) if n == until => break,
                Tok::Close(n) if n == "text:list" => { self.lists.pop(); inherited.pop(); }
                Tok::Open { name, attrs, empty: false } => match name.as_str() {
                    "text:list" => { let own = attr(attrs, "text:style-name").and_then(|s| self.ordered.get(s).copied()); let kind = own.or(inherited.last().copied().flatten()); inherited.push(kind); self.lists.push(kind.unwrap_or(false)); }
                    "text:p" | "text:h" => {
                        let st = self.style(attr(attrs, "text:style-name")); let level = attr(attrs, "text:outline-level").and_then(|v| v.parse::<u8>().ok()).unwrap_or(1).clamp(1, 6);
                        let (mut text, mut spans, pictures) = self.paragraph(name);
                        let keep = text.trim_end().len(); if keep < text.len() { spans.edit(keep, text.len() - keep, 0); text.truncate(keep); }
                        if !text.trim().is_empty() {
                            let kind = if name == "text:h" { Kind::Heading { level } } else if st.code { Kind::Code { lang: String::new() } } else if let Some(ordered) = self.lists.last() { let depth = (self.lists.len() - 1).min(8) as u8; if *ordered { Kind::Numbered { depth } } else { Kind::Bullet { depth } } } else if st.quote { Kind::Quote } else { Kind::Paragraph };
                            match kind { Kind::Code { .. } => spans = Spans::new(), Kind::Heading { .. } => {} _ => { let len = text.len(); for m in st.marks { if !spans.as_slice().iter().any(|s| s.mark.kind() == m.kind()) { spans.put(0, len, m); } } } }
                            if !matches!(kind, Kind::Code { .. }) { let lead = text.len() - text.trim_start().len(); if lead > 0 { spans.edit(0, lead, 0); text.drain(..lead); } }
                            out.push(vec![block(kind, text, spans)]);
                        }
                        for rec in pictures { out.push(vec![block(Kind::Media { descriptor: rec }, String::new(), Spans::new())]); }
                    }
                    "table:table" => { let t = self.table(); out.extend(t); }
                    _ => {}
                },
                _ => {}
            }
        }
        out
    }
    fn table(&mut self) -> Clip {
        let mut rows: Vec<(bool, Vec<Clip>)> = vec![]; let mut head = 0usize;
        while self.i < self.t.len() {
            let tok = &self.t[self.i]; self.i += 1;
            match tok {
                Tok::Close(n) if n == "table:table" => break,
                Tok::Close(n) if n == "table:table-header-rows" => head = head.saturating_sub(1),
                Tok::Open { name, attrs, empty } => match name.as_str() {
                    "table:table-header-rows" if !empty => head += 1,
                    "table:table-row" if !empty => rows.push((head > 0, vec![])),
                    "table:table-cell" | "table:covered-table-cell" if !rows.is_empty() => {
                        let saved = std::mem::take(&mut self.lists);
                        let inner = if *empty { vec![] } else { self.blocks(name) }; self.lists = saved;
                        let mut cell: Clip = vec![]; for tree in inner { if tree.len() == 1 { cell.push(tree); } else { for b in tree.into_iter().filter(|b| b.kind.is_text() && !b.text.is_empty()) { cell.push(vec![block(Kind::Paragraph, b.text, b.spans)]); } } }
                        let row = rows.last_mut().unwrap(); row.1.push(cell);
                        for _ in 1..attr(attrs, "table:number-columns-repeated").and_then(|v| v.parse::<usize>().ok()).unwrap_or(1).min(64) { row.1.push(vec![]); }
                    }
                    _ => {}
                },
                _ => {}
            }
        }
        let cols = rows.iter().map(|r| r.1.len()).max().unwrap_or(0);
        if cols < 2 { return rows.into_iter().flat_map(|r| r.1).flatten().collect() }
        vec![crate::paste::table_tree(rows, vec![ColumnSpec::default(); cols])]
    }
}

pub fn odt(path: &Path, places: &Places) -> Result<(Clip, usize), String> {
    let mut zip = Zip::open(path)?;
    let content = zip.text("content.xml").ok_or("The file has no OpenDocument content in it.")?;
    let (mut styles, mut ordered) = (HashMap::new(), HashMap::new());
    if let Some(x) = zip.text("styles.xml") { odf_styles(&tokens(&x), &mut styles, &mut ordered); }
    let toks = tokens(&content); odf_styles(&toks, &mut styles, &mut ordered);
    let start = toks.iter().position(|t| matches!(t, Tok::Open { name, .. } if name == "office:text")).map(|i| i + 1).ok_or("The file is not a text document.")?;
    let mut d = Odf { t: &toks, i: start, styles, ordered, zip: &mut zip, places, lost: 0, lists: vec![] };
    let clip = d.blocks("office:text");
    Ok((join_code(clip), d.lost))
}

pub fn ods(path: &Path) -> Result<Clip, String> {
    let mut zip = Zip::open(path)?;
    let content = zip.text("content.xml").ok_or("The file has no OpenDocument content in it.")?;
    let toks = tokens(&content); let mut all: Vec<Sheet> = vec![];
    let (mut row, mut col, mut row_rep, mut col_rep) = (0usize, 0usize, 1usize, 1usize); let (mut in_cell, mut skip, mut text, mut paras) = (false, 0usize, String::new(), 0usize);
    let mut row_cells: Vec<(usize, String)> = vec![];
    for t in &toks { match t {
        Tok::Open { name, attrs, empty } => { let rep = |k: &str| attr(attrs, k).and_then(|v| v.parse::<usize>().ok()).unwrap_or(1).max(1);
            match name.as_str() {
                "table:table" => { all.push(Sheet { name: attr(attrs, "table:name").unwrap_or("Sheet").to_string(), cells: vec![] }); row = 0; }
                "table:table-row" => { row_rep = rep("table:number-rows-repeated"); col = 0; row_cells.clear(); if *empty { row += row_rep; } }
                "table:table-cell" | "table:covered-table-cell" => { col_rep = rep("table:number-columns-repeated"); if *empty { col += col_rep; } else { in_cell = true; text.clear(); paras = 0; } }
                "office:annotation" if !empty => skip += 1,
                "text:p" if in_cell && skip == 0 => { if paras > 0 { text.push('\n'); } paras += 1; }
                "text:s" if in_cell && skip == 0 => { let c = attr(attrs, "text:c").and_then(|v| v.parse::<usize>().ok()).unwrap_or(1).min(200); text.push_str(&" ".repeat(c)); }
                "text:tab" if in_cell && skip == 0 => text.push('\t'), "text:line-break" if in_cell && skip == 0 => text.push('\n'),
                _ => {} } }
        Tok::Text(x) => if in_cell && skip == 0 && paras > 0 { text.push_str(x) },
        Tok::Close(n) => match n.as_str() {
            "office:annotation" => skip = skip.saturating_sub(1),
            "table:table-cell" | "table:covered-table-cell" => { in_cell = false; let shown = text.trim_end().to_string();
                // a value repeated across cells is written out (within reason); empty ones only move the place on
                if !shown.trim().is_empty() { for k in 0..col_rep.min(256) { row_cells.push((col + k, shown.clone())); } } col += col_rep; }
            "table:table-row" => { if let Some(s) = all.last_mut() { if !row_cells.is_empty() { for k in 0..row_rep.min(1000) { for (c, x) in &row_cells { if *c < 16_384 { s.cells.push((row + k, *c, x.clone())); } } } } } row += row_rep; row_cells.clear(); }
            _ => {} },
    } }
    sheets(all)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    fn zip_of(path: &Path, parts: &[(&str, &[u8])]) { let mut z = zip::ZipWriter::new(std::fs::File::create(path).unwrap()); for (n, b) in parts { z.start_file(*n, zip::write::SimpleFileOptions::default()).unwrap(); z.write_all(b).unwrap(); } z.finish().unwrap(); }
    fn kinds(c: &Clip) -> Vec<(Kind, &str)> { c.iter().map(|t| (t[0].kind.clone(), t[0].text.as_str())).collect() }
    fn dir(name: &str) -> std::path::PathBuf { let d = std::env::temp_dir().join(format!("mn-office-{name}-{}", std::process::id())); let _ = std::fs::remove_dir_all(&d); std::fs::create_dir_all(d.join("staged")).unwrap(); d }

    #[test]
    fn a_word_document_becomes_blocks_with_its_formatting_lists_table_and_picture() {
        let d = dir("docx"); let places = Places::at(Some(d.clone())).staged_in(d.join("staged"));
        let png = min_image::write::png_rgba8_bytes(&[200u8; 4 * 40 * 20], 40, 20).unwrap();
        let p = |ppr: &str, runs: &str| format!("<w:p><w:pPr>{ppr}</w:pPr>{runs}</w:p>"); let r = |rpr: &str, t: &str| format!("<w:r><w:rPr>{rpr}</w:rPr><w:t xml:space=\"preserve\">{t}</w:t></w:r>");
        let body = [p("<w:pStyle w:val=\"NoteName\"/>", &r("", "the header")), p("<w:pStyle w:val=\"Heading2\"/><w:rPr><w:i/></w:rPr>", &r("<w:b/>", "Plan &amp; more")),
            p("", &format!("{}{}<w:hyperlink r:id=\"rL\">{}</w:hyperlink>{}", r("", "Some "), r("<w:b/><w:i w:val=\"0\"/><w:color w:val=\"C81E1E\"/>", "bold red"), r("<w:u w:val=\"single\"/><w:color w:val=\"0563C1\"/><w:strike/>", " link"), r("<w:highlight w:val=\"yellow\"/><w:color w:val=\"333333\"/>", " marked"))),
            p("<w:numPr><w:ilvl w:val=\"1\"/><w:numId w:val=\"1\"/></w:numPr>", &r("", "a bullet")), p("<w:numPr><w:ilvl w:val=\"0\"/><w:numId w:val=\"3\"/></w:numPr>", &r("", "a number")),
            p("<w:pStyle w:val=\"ListParagraph\"/><w:ind w:left=\"720\" w:hanging=\"360\"/>", &format!("{}{}", r("", "\u{2611}\t"), r("", "done"))),
            p("<w:pStyle w:val=\"Quote\"/>", &format!("{}<w:r><w:fldChar w:fldCharType=\"begin\"/></w:r><w:r><w:instrText> HYPERLINK \"https://example.org/f\" </w:instrText></w:r><w:r><w:fldChar w:fldCharType=\"separate\"/></w:r>{}<w:r><w:fldChar w:fldCharType=\"end\"/></w:r>", r("", "said "), r("", "here"))), p("", &r("", "\t\u{2022}\ttext bullet")), p("<w:pStyle w:val=\"CodeLang\"/>", &r("", "rust")), p("<w:pStyle w:val=\"Code\"/>", "<w:r><w:t>fn a() {}</w:t><w:br/><w:t>  b</w:t></w:r>"), p("", &r("<w:rFonts w:ascii=\"Courier New\"/>", "more code")),
            "<w:p><w:pPr><w:pBdr><w:bottom w:val=\"single\"/></w:pBdr></w:pPr></w:p>".to_string(),
            "<w:tbl><w:tr><w:trPr><w:tblHeader/></w:trPr><w:tc><w:tcPr><w:gridSpan w:val=\"2\"/></w:tcPr><w:p><w:r><w:t>Wide</w:t></w:r></w:p></w:tc><w:tc><w:p><w:pPr><w:jc w:val=\"right\"/></w:pPr><w:r><w:t>N</w:t></w:r></w:p></w:tc></w:tr><w:tr><w:tc><w:p><w:r><w:t>a</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:vMerge/></w:tcPr><w:p><w:r><w:t>hidden</w:t></w:r></w:p></w:tc><w:tc><w:p/></w:tc></w:tr></w:tbl>".to_string(),
            "<w:p><w:r><w:drawing><wp:inline><wp:extent cx=\"190500\" cy=\"95250\"/><a:graphic><a:blip r:embed=\"rP\"/></a:graphic></wp:inline></w:drawing></w:r><mc:AlternateContent><mc:Fallback><w:r><w:t>not this</w:t></w:r></mc:Fallback></mc:AlternateContent></w:p>".to_string()].concat();
        zip_of(&d.join("a.docx"), &[("word/document.xml", format!("<?xml version=\"1.0\"?><w:document><w:body>{body}<w:sectPr/></w:body></w:document>").as_bytes()),
            ("word/_rels/document.xml.rels", b"<Relationships><Relationship Id=\"rL\" Target=\"https://example.com\" TargetMode=\"External\"/><Relationship Id=\"rP\" Target=\"media/image1.png\"/></Relationships>"),
            ("word/numbering.xml", b"<w:numbering><w:abstractNum w:abstractNumId=\"0\"><w:lvl w:ilvl=\"0\"><w:numFmt w:val=\"bullet\"/></w:lvl><w:lvl w:ilvl=\"1\"><w:numFmt w:val=\"decimal\"/></w:lvl></w:abstractNum><w:abstractNum w:abstractNumId=\"1\"><w:lvl w:ilvl=\"0\"><w:numFmt w:val=\"decimal\"/></w:lvl></w:abstractNum><w:num w:numId=\"1\"><w:abstractNumId w:val=\"0\"/></w:num><w:num w:numId=\"3\"><w:abstractNumId w:val=\"1\"/></w:num></w:numbering>"),
            ("word/media/image1.png", &png)]);
        let (c, lost) = docx(&d.join("a.docx"), &places).unwrap(); assert_eq!(lost, 0);
        assert_eq!(kinds(&c)[..9], [(Kind::Heading { level: 2 }, "Plan & more"), (Kind::Paragraph, "Some bold red link marked"), (Kind::Bullet { depth: 1 }, "a bullet"), (Kind::Numbered { depth: 0 }, "a number"), (Kind::Task { depth: 1, state: 2 }, "done"), (Kind::Quote, "said here"), (Kind::Bullet { depth: 1 }, "text bullet"),
            (Kind::Code { lang: "rust".into() }, "fn a() {}\n  b\nmore code"), (Kind::Divider, "")]);
        assert!(c[5][0].spans.covers(5, 9, &Mark::Link("https://example.org/f".into())) && !c[5][0].spans.covers(0, 5, &Mark::Link("https://example.org/f".into())));
        assert!(c[0][0].spans.is_empty(), "a heading's bold is its own"); let s = &c[1][0].spans;
        assert!(s.covers(5, 13, &Mark::Bold) && s.covers(5, 13, &Mark::Color("#c81e1e".into())) && !s.covers(5, 13, &Mark::Italic));
        assert!(s.covers(13, 18, &Mark::Link("https://example.com".into())) && s.covers(13, 18, &Mark::Strike) && !s.covers(13, 18, &Mark::Underline) && !s.as_slice().iter().any(|x| x.mark == Mark::Color("#0563c1".into())));
        assert!(s.covers(18, 25, &Mark::Highlight("#ffff00".into())) && s.as_slice().iter().filter(|x| x.mark.kind() == MarkKind::Color).count() == 1, "a grey is no colour");
        assert!(matches!(&c[9][0].kind, Kind::Table { header_rows: 1, columns } if columns.len() == 3 && columns[2].align == 2));
        let texts: Vec<&str> = c[9].iter().filter(|b| b.kind.is_text()).map(|b| b.text.as_str()).collect(); assert_eq!(texts, ["Wide", "", "N", "a", "", ""]);
        assert!(matches!(&c[10][0].kind, Kind::Media { descriptor } if descriptor.contains(".minnotes/image1-") && descriptor.contains("\"w\":20")), "{:?}", c[10][0].kind); assert_eq!(c.len(), 11);
        let _ = std::fs::remove_dir_all(&d);
    }
    #[test]
    fn a_workbook_becomes_a_table_a_sheet_with_dates_as_dates() {
        let d = dir("xlsx");
        zip_of(&d.join("a.xlsx"), &[("xl/workbook.xml", b"<workbook><sheets><sheet name=\"Shots\" sheetId=\"1\" r:id=\"r1\"/><sheet name=\"Empty\" sheetId=\"2\" r:id=\"r2\"/><sheet name=\"More\" sheetId=\"3\" r:id=\"r3\"/></sheets></workbook>"),
            ("xl/_rels/workbook.xml.rels", b"<Relationships><Relationship Id=\"r1\" Target=\"worksheets/sheet1.xml\"/><Relationship Id=\"r2\" Target=\"worksheets/sheet2.xml\"/><Relationship Id=\"r3\" Target=\"/xl/worksheets/sheet3.xml\"/></Relationships>"),
            ("xl/sharedStrings.xml", b"<sst><si><t>Shot</t></si><si><r><t>Due </t></r><r><t>date</t></r><rPh><t>guide</t></rPh></si><si><t>010</t></si></sst>"),
            ("xl/styles.xml", b"<styleSheet><numFmts><numFmt numFmtId=\"164\" formatCode=\"yyyy\\-mm\\-dd\\ hh:mm\"/><numFmt numFmtId=\"165\" formatCode=\"0.0 &quot;days&quot;\"/></numFmts><cellStyleXfs><xf numFmtId=\"14\"/></cellStyleXfs><cellXfs><xf numFmtId=\"0\"/><xf numFmtId=\"14\"/><xf numFmtId=\"164\"/><xf numFmtId=\"165\"/></cellXfs></styleSheet>"),
            ("xl/worksheets/sheet1.xml", b"<worksheet><sheetData><row r=\"1\"><c r=\"A1\" t=\"s\"><v>0</v></c><c r=\"B1\" t=\"s\"><v>1</v></c><c r=\"D1\" t=\"inlineStr\"><is><t>Note</t></is></c></row><row r=\"3\"><c r=\"A3\" t=\"s\"><v>2</v></c><c r=\"B3\" s=\"1\"><v>46339</v></c><c r=\"C3\" s=\"2\"><v>46339.75</v></c><c r=\"D3\" s=\"3\"><v>0.30000000000000004</v></c><c r=\"E3\" t=\"b\"><v>1</v></c><c r=\"F3\"><f>A1</f><v>12</v></c></row></sheetData></worksheet>"),
            ("xl/worksheets/sheet2.xml", b"<worksheet><sheetData/></worksheet>"), ("xl/worksheets/sheet3.xml", b"<worksheet><sheetData><row><c><v>1</v></c><c><v>2</v></c></row></sheetData></worksheet>")]);
        let c = xlsx(&d.join("a.xlsx")).unwrap(); assert_eq!(c.len(), 4);
        assert_eq!(c[0][0].text, "Shots"); assert_eq!(c[2][0].text, "More"); assert!(matches!(&c[1][0].kind, Kind::Table { header_rows: 1, columns } if columns.len() == 6)); assert_eq!(c[1][0].children.len(), 3);
        let texts: Vec<&str> = c[1].iter().filter(|b| b.kind.is_text()).map(|b| b.text.as_str()).collect();
        assert_eq!(texts, ["Shot", "Due date", "", "Note", "", "", "", "", "", "", "", "", "010", "2026-11-13", "2026-11-13 18:00", "0.3", "TRUE", "12"]);
        assert_eq!(cell_ref("BC12"), Some((11, 54))); assert_eq!(serial(0.5, false), "12:00"); assert_eq!(serial(1.0, true), "1904-01-02"); assert!(dated(165, Some("0.0 \"days\"")) == false && dated(0, Some("[h]:mm")));
        let _ = std::fs::remove_dir_all(&d);
    }
    #[test]
    fn opendocument_text_and_sheets_are_read() {
        let d = dir("odf"); let places = Places::at(Some(d.clone())).staged_in(d.join("staged"));
        let png = min_image::write::png_rgba8_bytes(&[90u8; 4 * 4 * 4], 4, 4).unwrap();
        zip_of(&d.join("a.odt"), &[("content.xml", b"<office:document-content><office:automatic-styles><style:style style:name=\"T1\" style:family=\"text\"><style:text-properties fo:font-weight=\"bold\" fo:color=\"#c81e1e\"/></style:style><style:style style:name=\"P9\" style:family=\"paragraph\" style:parent-style-name=\"Preformatted_20_Text\"/><text:list-style style:name=\"L1\"><text:list-level-style-number text:level=\"1\"/><text:list-level-style-bullet text:level=\"2\"/></text:list-style></office:automatic-styles><office:body><office:text><text:sequence-decls/><text:h text:outline-level=\"2\">A heading</text:h><text:p>Plain <text:span text:style-name=\"T1\">bold red</text:span><text:s text:c=\"2\"/><text:a xlink:href=\"https://example.com\">link</text:a><text:note><text:note-body><text:p>a footnote</text:p></text:note-body></text:note><text:line-break/>next</text:p><text:list text:style-name=\"L1\"><text:list-item><text:p>one</text:p><text:list><text:list-item><text:p>inner</text:p></text:list-item></text:list></text:list-item></text:list><text:p text:style-name=\"P9\">code 1</text:p><text:p text:style-name=\"P9\">code 2</text:p><table:table><table:table-header-rows><table:table-row><table:table-cell><text:p>H1</text:p></table:table-cell><table:table-cell><text:p>H2</text:p></table:table-cell></table:table-row></table:table-header-rows><table:table-row><table:table-cell><text:p>a</text:p></table:table-cell><table:covered-table-cell/></table:table-row></table:table><text:p><draw:frame><draw:image xlink:href=\"Pictures/p.png\"/></draw:frame></text:p></office:text></office:body></office:document-content>"), ("Pictures/p.png", &png)]);
        let (c, lost) = odt(&d.join("a.odt"), &places).unwrap(); assert_eq!(lost, 0);
        assert_eq!(kinds(&c)[..5], [(Kind::Heading { level: 2 }, "A heading"), (Kind::Paragraph, "Plain bold red  link\nnext"), (Kind::Numbered { depth: 0 }, "one"), (Kind::Numbered { depth: 1 }, "inner"), (Kind::Code { lang: String::new() }, "code 1\ncode 2")]);
        let s = &c[1][0].spans; assert!(s.covers(6, 14, &Mark::Bold) && s.covers(6, 14, &Mark::Color("#c81e1e".into())) && s.covers(16, 20, &Mark::Link("https://example.com".into())));
        assert!(matches!(&c[5][0].kind, Kind::Table { header_rows: 1, columns } if columns.len() == 2)); assert!(matches!(&c[6][0].kind, Kind::Media { .. })); assert_eq!(c.len(), 7);
        zip_of(&d.join("a.ods"), &[("content.xml", b"<office:document-content><office:body><office:spreadsheet><table:table table:name=\"One\"><table:table-column table:number-columns-repeated=\"1024\"/><table:table-row><table:table-cell office:value-type=\"string\"><text:p>Name</text:p></table:table-cell><table:table-cell table:number-columns-repeated=\"2\"/><table:table-cell><text:p>Date</text:p></table:table-cell><table:table-cell table:number-columns-repeated=\"1020\"/></table:table-row><table:table-row table:number-rows-repeated=\"2\"><table:table-cell><text:p>x</text:p><text:p>y</text:p></table:table-cell><table:table-cell table:number-columns-repeated=\"2\"/><table:table-cell office:value-type=\"date\" office:date-value=\"2026-11-13\"><text:p>13/11/26</text:p></table:table-cell></table:table-row><table:table-row table:number-rows-repeated=\"1048573\"><table:table-cell table:number-columns-repeated=\"1024\"/></table:table-row></table:table></office:spreadsheet></office:body></office:document-content>")]);
        let c = ods(&d.join("a.ods")).unwrap(); assert_eq!(c.len(), 1); assert!(matches!(&c[0][0].kind, Kind::Table { header_rows: 1, columns } if columns.len() == 4)); assert_eq!(c[0][0].children.len(), 3);
        let texts: Vec<&str> = c[0].iter().filter(|b| b.kind.is_text()).map(|b| b.text.as_str()).collect(); assert_eq!(texts, ["Name", "", "", "Date", "x\ny", "", "", "13/11/26", "x\ny", "", "", "13/11/26"]);
        let _ = std::fs::remove_dir_all(&d);
    }
}
