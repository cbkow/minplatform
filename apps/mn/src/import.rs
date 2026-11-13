//! A file of another application becomes a NOTE of the open collection (canon row 335; File ▸ Import…). The first
//! stage's families: Markdown, HTML, plain text, source code, and CSV / TSV. A file is told by its extension alone
//! (as the Qt app's importer did). Reading, and the keeping of the pictures a document names, is off the window's
//! thread; what comes back is the note's blocks, which the window lands beside the selected entry.
//!
//! Markdown and HTML go through the readers of `paste.rs` with an import's two differences ([`crate::paste::Opts`]):
//! colours are kept, and a picture named by a path is found - against the file's folder - and COPIED into the
//! collection, so the note does not depend on the folder it came from. A picture on the web is fetched when it is
//! first shown, as a pasted one is (row 314).
use crate::media::Places;
use mn_doc::{Block, BlockId, ColumnSpec, Doc, Kind, Rank, Spans};
use mn_edit::Clip;
use std::path::{Path, PathBuf};

/// A table of more cells than this is not imported whole (every cell is two blocks): the window asks - its first
/// rows, the file as a chip, or nothing. Set from a measurement of the rebuild (row 335), not from the Qt app's
/// 100 000: a whole layout of a table costs about 6 ms per 1000 cells here and a cell about 11 KB (19 200 cells: 91 ms;
/// 76 800: 445 ms and 0.9 GB), so 10 000 cells is the size whose whole layout still fits in about three frames.
pub const CAP: usize = 10_000;
/// A text file larger than this is not read as a document at all. A web page may be far larger: its pictures are
/// often inside it (minNotes' own HTML export is one file with everything in it).
const LARGEST: u64 = 32 << 20;
const LARGEST_PAGE: u64 = 1 << 30;

#[derive(Clone, Debug, PartialEq)]
pub enum Family { Markdown, Html, Text, /// one code block; the language is the extension
    Code(String), /// a table; the separator (None: found from the first line)
    Table(Option<char>) }

const CODE: [&str; 38] = ["py", "js", "ts", "jsx", "tsx", "c", "cc", "cpp", "cxx", "h", "hpp", "m", "mm", "cs", "java", "kt", "swift", "go", "rs", "rb", "php", "sh", "bash", "zsh", "sql", "json", "yaml", "yml", "toml", "xml", "ini", "css", "scss", "lua", "pl", "r", "cmake", "ps1"];
/// What the file panel offers, a family at a time.
pub const FILTERS: [(&str, &[&str]); 5] = [("Markdown", &["md", "markdown", "mdown"]), ("Web page", &["html", "htm"]), ("Plain text", &["txt", "text", "log"]), ("Table (CSV, TSV)", &["csv", "tsv", "tab"]), ("Source code", &CODE)];
pub fn extensions() -> Vec<&'static str> { FILTERS.iter().flat_map(|f| f.1.iter().copied()).collect() }

pub fn family(path: &Path) -> Option<Family> {
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    Some(match ext.as_str() { "md" | "markdown" | "mdown" => Family::Markdown, "html" | "htm" => Family::Html, "txt" | "text" => Family::Text, "csv" => Family::Table(None), "tsv" | "tab" => Family::Table(Some('\t')),
        // a log is lines to be read as they are, in one face: a code block of no language
        "log" => Family::Code(String::new()), e if CODE.contains(&e) => Family::Code(ext), _ => return None })
}

/// What a file was read into.
pub struct Done {
    /// the note's name: the file's, without its extension
    pub title: String, pub clip: Clip,
    /// pictures the document names that were not found (they are left out)
    pub lost: usize,
    /// the largest table's rows and cells, when it is over [`CAP`]
    pub big: Option<(usize, usize)>,
}

fn block(kind: Kind, text: String) -> Block { Block { id: BlockId::new(), parent: None, rank: Rank::first(), kind, text, spans: Spans::new(), conflict: None, children: vec![] } }

/// A text file's bytes as text: a byte-order mark says UTF-8 or UTF-16; without one it is UTF-8, and bytes that are
/// not are read as Latin-1 (nothing is refused for its encoding). Line ends become `\n`.
pub fn decode(b: &[u8]) -> String {
    let wide = |b: &[u8], le: bool| String::from_utf16_lossy(&b.chunks_exact(2).map(|c| if le { u16::from_le_bytes([c[0], c[1]]) } else { u16::from_be_bytes([c[0], c[1]]) }).collect::<Vec<_>>());
    let s = if let Some(r) = b.strip_prefix(&[0xef, 0xbb, 0xbf]) { String::from_utf8_lossy(r).into_owned() } else if let Some(r) = b.strip_prefix(&[0xff, 0xfe]) { wide(r, true) } else if let Some(r) = b.strip_prefix(&[0xfe, 0xff]) { wide(r, false) }
        else { match std::str::from_utf8(b) { Ok(s) => s.to_string(), Err(_) => b.iter().map(|c| *c as char).collect() } };
    s.replace("\r\n", "\n").replace('\r', "\n")
}

/// Separated values as rows of cells: a quoted cell may hold the separator, line ends, and `""` for a quote.
pub fn rows_of(text: &str, sep: char) -> Vec<Vec<String>> {
    let (mut rows, mut row, mut cell, mut quoted, mut was_quoted) = (vec![], vec![], String::new(), false, false);
    let mut it = text.chars().peekable();
    while let Some(c) = it.next() {
        if quoted { if c == '"' { if it.peek() == Some(&'"') { cell.push('"'); it.next(); } else { quoted = false; } } else { cell.push(c); } continue }
        if c == '"' && cell.is_empty() && !was_quoted { quoted = true; was_quoted = true; }
        else if c == sep { row.push(std::mem::take(&mut cell)); was_quoted = false; }
        else if c == '\n' { row.push(std::mem::take(&mut cell)); was_quoted = false; rows.push(std::mem::take(&mut row)); }
        else { cell.push(c); }
    }
    if !cell.is_empty() || !row.is_empty() { row.push(cell); rows.push(row); }
    // rows and columns of nothing at the end are not the table's
    while rows.last().is_some_and(|r: &Vec<String>| r.iter().all(|c| c.trim().is_empty())) { rows.pop(); }
    let cols = rows.iter().map(|r| r.iter().rposition(|c| !c.trim().is_empty()).map_or(0, |i| i + 1)).max().unwrap_or(0);
    for r in rows.iter_mut() { r.truncate(cols); }
    rows
}
/// A `.csv`'s separator: the one of `,` `;` and a tab there is most of in its first line (outside quotes).
fn separator(text: &str) -> char {
    let first = text.lines().next().unwrap_or(""); let mut q = false; let mut n = [0usize; 3];
    for c in first.chars() { if c == '"' { q = !q; } else if !q { match c { ',' => n[0] += 1, ';' => n[1] += 1, '\t' => n[2] += 1, _ => {} } } }
    if n[1] > n[0] && n[1] >= n[2] { ';' } else if n[2] > n[0] { '\t' } else { ',' }
}
fn table(rows: Vec<Vec<String>>) -> Clip {
    let cols = rows.iter().map(|r| r.len()).max().unwrap_or(0); if cols == 0 { return vec![] }
    let n = rows.len();
    let rows = rows.into_iter().enumerate().map(|(i, r)| (i == 0 && n > 1, r.into_iter().map(|c| { let c = c.trim().to_string(); if c.is_empty() { vec![] } else { vec![vec![block(Kind::Paragraph, c)]] } }).collect())).collect();
    vec![crate::paste::table_tree(rows, vec![ColumnSpec::default(); cols])]
}

/// A table tree's rows and cells (None for any other tree).
pub fn size(tree: &[Block]) -> Option<(usize, usize)> { match &tree.first()?.kind { Kind::Table { columns, .. } => Some((tree[0].children.len(), tree[0].children.len() * columns.len())), _ => None } }
/// The rows of a table of `cols` columns that fit the cap.
pub fn rows_that_fit(cols: usize) -> usize { (CAP / cols.max(1)).max(1) }
/// Every table over the cap keeps its first rows only; what follows a table stays (the Qt app dropped it).
pub fn cut(clip: &mut Clip) {
    for tree in clip.iter_mut() {
        let Some((rows, cells)) = size(tree) else { continue }; if cells <= CAP { continue }
        let keep = rows_that_fit(cells / rows.max(1)); let Some(first_out) = tree[0].children.get(keep).copied() else { continue };
        if let Some(i) = tree.iter().position(|b| b.id == first_out) { tree.truncate(i); } tree[0].children.truncate(keep);
        if let Kind::Table { header_rows, .. } = &mut tree[0].kind { *header_rows = (*header_rows).min(keep.saturating_sub(1) as u8); }
    }
}

/// A picture's address in a document as a file: `file://` and percent-escapes undone, a relative one against `folder`.
fn local(addr: &str, folder: &Path) -> PathBuf {
    let a = addr.strip_prefix("file://").map(|r| r.strip_prefix("localhost").unwrap_or(r)).unwrap_or(addr); let a = a.split(['?', '#']).next().unwrap_or(a);
    let (b, mut out, mut i) = (a.as_bytes(), Vec::with_capacity(a.len()), 0);
    while i < b.len() { if b[i] == b'%' && i + 3 <= b.len() && a.is_char_boundary(i + 1) && a.is_char_boundary(i + 3) { if let Ok(v) = u8::from_str_radix(&a[i + 1..i + 3], 16) { out.push(v); i += 3; continue } } out.push(b[i]); i += 1; }
    let p = PathBuf::from(String::from_utf8_lossy(&out).into_owned());
    if p.is_absolute() { p } else { folder.join(p) }
}

/// The pictures a document names by a path are found and kept with the collection; one that is not found is left
/// out (a cell that held only it stays, empty). Returns how many were not found.
fn keep_pictures(clip: &mut Clip, folder: &Path, places: &Places) -> usize {
    let mut lost = 0;
    for tree in clip.iter_mut() { for b in tree.iter_mut() {
        let Kind::Media { descriptor } = &b.kind else { continue };
        let Ok(v) = serde_json::from_str::<serde_json::Value>(descriptor) else { continue };
        let Some(src) = v.get("src").and_then(|s| s.as_str()) else { continue };
        // a picture inside the document itself is kept now, so that the note does not carry its bytes as text
        if src.starts_with("data:") { match crate::media::fetched(places, src).ok().and_then(|r| serde_json::from_str::<serde_json::Value>(&r).ok()) {
            Some(mut rec) => { if let (Some(w), Some(h)) = (v.get("w").and_then(|w| w.as_f64()).filter(|_| v.get("h").is_some() && !(v["w"] == 480 && v["h"] == 300)), v.get("h").and_then(|h| h.as_f64())) { rec["w"] = serde_json::json!(w.round()); rec["h"] = serde_json::json!(h.round()); } b.kind = Kind::Media { descriptor: rec.to_string() }; }
            None => { lost += 1; b.kind = Kind::Paragraph; } } continue }
        if crate::media::remote(src) { continue }
        match places.picture_copied(&local(src, folder)) {
            Ok(mut rec) => { // the size the document gave it is kept in proportion: its width, the picture's own shape
                if let (Some(w), Some(pw), Some(ph)) = (v.get("w").and_then(|w| w.as_f64()).filter(|w| *w >= 16.0), rec.get("w").and_then(|w| w.as_f64()), rec.get("h").and_then(|h| h.as_f64())) { if pw > 0.0 && w < pw { rec["w"] = serde_json::json!(w.round()); rec["h"] = serde_json::json!((w * ph / pw).round()); } }
                b.kind = Kind::Media { descriptor: rec.to_string() }; }
            Err(_) => { lost += 1; b.kind = Kind::Paragraph; }
        }
    } }
    // a picture that was not found and stood alone leaves nothing
    clip.retain(|t| !(t.len() == 1 && t[0].kind == Kind::Paragraph && t[0].text.is_empty()));
    lost
}

/// Reads `path` into a note's blocks. `places`: where the collection keeps files (pictures are copied there).
pub fn read(path: &Path, places: &Places) -> Result<Done, String> {
    let fam = family(path).ok_or("minNotes does not import this kind of file")?;
    let len = std::fs::metadata(path).map_err(|e| e.to_string())?.len();
    if len > if fam == Family::Html { LARGEST_PAGE } else { LARGEST } { return Err("The file is too large to be a note \u{2014} drop it into a note to attach it instead.".into()) }
    let text = decode(&std::fs::read(path).map_err(|e| e.to_string())?);
    let folder = path.parent().unwrap_or(Path::new("."));
    let opts = crate::paste::Opts { colours: true, local: true };
    let mut clip: Clip = match fam {
        Family::Markdown => crate::paste::from_markdown_with(&text, true),
        Family::Html => crate::paste::from_html_with(&text, opts),
        // lines as they are, a paragraph each; the empty lines between them are what parted them
        Family::Text => text.lines().filter(|l| !l.trim().is_empty()).map(|l| vec![block(Kind::Paragraph, l.trim_end().replace('\t', "    "))]).collect(),
        Family::Code(lang) => { let t = text.trim_end_matches('\n'); if t.trim().is_empty() { vec![] } else { vec![vec![block(Kind::Code { lang }, t.to_string())]] } }
        Family::Table(sep) => table(rows_of(&text, sep.unwrap_or_else(|| separator(&text)))),
    };
    let lost = keep_pictures(&mut clip, folder, places);
    if clip.is_empty() { return Err("Nothing in the file could be read as a note.".into()) }
    let big = clip.iter().filter_map(|t| size(t)).filter(|s| s.1 > CAP).max_by_key(|s| s.1);
    Ok(Done { title: path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default(), clip, lost, big })
}

/// The blocks as a note.
pub fn doc_of(clip: &Clip) -> Doc {
    let mut d = Doc::new(); let mut i = 0;
    for tree in clip { if d.insert_tree(i, tree).is_ok() { i += 1; } }
    if i == 0 { let _ = d.insert(None, 0, BlockId::new(), Kind::Paragraph, ""); }
    d
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn a_file_is_told_by_its_extension() {
        assert_eq!(family(Path::new("a/Read Me.MD")), Some(Family::Markdown)); assert_eq!(family(Path::new("x.rs")), Some(Family::Code("rs".into()))); assert_eq!(family(Path::new("x.log")), Some(Family::Code(String::new())));
        assert_eq!(family(Path::new("x.tsv")), Some(Family::Table(Some('\t')))); assert_eq!(family(Path::new("x.mov")), None); assert_eq!(family(Path::new("noext")), None);
        assert_eq!(decode(&[0xef, 0xbb, 0xbf, b'a', b'\r', b'\n', b'b']), "a\nb"); assert_eq!(decode(&[0xff, 0xfe, b'h', 0, b'i', 0]), "hi"); assert_eq!(decode(&[b'c', 0xe9]), "c\u{e9}");
    }
    #[test]
    fn separated_values_become_a_table_with_its_first_row_the_header() {
        let rows = rows_of("Shot,Note,\n010,\"ok, \"\"good\"\"\nsecond line\",\n020,,\n,,\n", ',');
        assert_eq!(rows, [vec!["Shot", "Note"], vec!["010", "ok, \"good\"\nsecond line"], vec!["020", ""]]);
        assert_eq!(separator("a;b;c\n1;2;3"), ';'); assert_eq!(separator("a,b\tc,d"), ',');
        let clip = table(rows); assert_eq!(size(&clip[0]), Some((3, 6))); assert!(matches!(&clip[0][0].kind, Kind::Table { header_rows: 1, columns } if columns.len() == 2));
        let d = doc_of(&clip); assert_eq!(d.children(None).len(), 1);
    }
    #[test]
    fn a_table_over_the_cap_keeps_its_first_rows_and_what_follows_it() {
        let wide = 4; let n = CAP / wide + 50;
        let mut clip = table((0..n).map(|r| (0..wide).map(|c| format!("{r}.{c}")).collect()).collect()); clip.push(vec![block(Kind::Paragraph, "after".into())]);
        assert_eq!(size(&clip[0]), Some((n, n * wide))); cut(&mut clip);
        assert_eq!(size(&clip[0]), Some((CAP / wide, CAP))); assert_eq!(clip[1][0].text, "after");
        let d = doc_of(&clip); assert_eq!(d.children(None).len(), 2, "the cut table is still a whole tree");
        assert_eq!(clip[0].len(), 1 + (CAP / wide) * (1 + wide * 2));
    }
    #[test]
    fn markdown_and_html_files_keep_tables_colours_and_their_own_pictures() {
        let dir = std::env::temp_dir().join(format!("mn-import-{}", std::process::id())); let _ = std::fs::remove_dir_all(&dir); std::fs::create_dir_all(dir.join("pics")).unwrap(); std::fs::create_dir_all(dir.join("staged")).unwrap();
        std::fs::write(dir.join("pics/a b.png"), min_image::write::png_rgba8_bytes(&[255u8; 4 * 6 * 4], 6, 4).unwrap()).unwrap();
        std::fs::write(dir.join("n.md"), "# Title\n\n| Shot | Note |\n|:--|--:|\n| 010 | **ok** \\| fine |\n| 020 |\n\nText ![p](pics/a%20b.png) and ![gone](nope.png).\n").unwrap();
        let places = Places::at(Some(dir.clone())).staged_in(dir.join("staged"));
        let d = read(&dir.join("n.md"), &places).unwrap(); assert_eq!(d.title, "n"); assert_eq!(d.lost, 1); assert!(d.big.is_none());
        assert_eq!(d.clip.len(), 4); assert!(matches!(&d.clip[1][0].kind, Kind::Table { header_rows: 1, columns } if columns.len() == 2 && columns[1].align == 2));
        let texts: Vec<&str> = d.clip[1].iter().filter(|b| b.kind.is_text()).map(|b| b.text.as_str()).collect(); assert_eq!(texts, ["Shot", "Note", "010", "ok | fine", "020", ""]);
        assert!(matches!(&d.clip[3][0].kind, Kind::Media { descriptor } if descriptor.contains(".minnotes/a-b-") && descriptor.contains("\"w\":6")), "{:?}", d.clip[3][0].kind);
        assert_eq!(std::fs::read_dir(dir.join("staged")).unwrap().count(), 1);
        std::fs::write(dir.join("p.html"), "<html><head><title>T</title><style>p{}</style></head><body><p style='color:#333'>grey <span style='color: rgb(200, 30, 30)'>red</span> <mark>marked</mark> <a href='https://example.com'><b>bold link</b></a></p><img src='pics/a b.png' width=3><img src='pics/a b.png'></body></html>").unwrap();
        let d = read(&dir.join("p.html"), &places).unwrap(); assert_eq!(d.clip.len(), 3); let p = &d.clip[0][0]; assert_eq!(p.text, "grey red marked bold link");
        use mn_doc::Mark; assert!(p.spans.covers(5, 8, &Mark::Color("#c81e1e".into())) && !p.spans.as_slice().iter().any(|s| s.start == 0), "{:?}", p.spans.as_slice());
        assert!(p.spans.covers(9, 15, &Mark::Highlight("#ffd400".into())) && p.spans.covers(16, 25, &Mark::Bold) && p.spans.covers(16, 25, &Mark::Link("https://example.com".into())));
        assert!(matches!(&d.clip[1][0].kind, Kind::Media { descriptor } if descriptor.contains("\"w\":6")));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
