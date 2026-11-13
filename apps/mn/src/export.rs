//! A note written out for other applications (canon row 328; the Qt app's exporters are read in
//! `docs/phase3/step-export-import-spec.md`). Two forms here: HTML - the fancy one, one file with everything in it, the
//! family's export look (QCView's notes export: Geist embedded, the dark tokens, a light print, the lightbox) - and
//! Markdown, simple: a `.md` and a folder of pictures beside it.
//!
//! The writers walk the note's blocks and know nothing of files: what a media block looks like comes from a [`Source`]
//! (the application's side is [`Files`], at the end). Drawings on a picture, a PDF's page or a video's frame are a
//! layer of their own in HTML - an SVG over the clean picture, which the reader can hide - and are drawn INTO the
//! picture for Markdown, which has no layers.
use mn_doc::{Block, BlockId, ColumnKind, ColumnSpec, Kind, Mark};
use qcv_notes::export::{b64, esc};
use std::collections::HashMap;

#[path = "export_docx.rs"]
mod docx;
pub use docx::docx;
#[path = "export_pdf.rs"]
mod pdf;
pub use pdf::pdf;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Opts { /// where each referenced file is on this machine, under its name
    pub paths: bool, /// a video's notes (its frames, drawings, words) under it
    pub video_notes: bool }
impl Default for Opts { fn default() -> Self { Opts { paths: true, video_notes: true } } }

/// An encoded picture.
pub struct Pic { pub bytes: Vec<u8>, pub mime: &'static str, pub ext: &'static str, pub w: u32, pub h: u32 }
/// Drawings as SVG paths in a sheet `w` by `h`.
pub struct Ink { pub w: f64, pub h: f64, pub paths: String }
/// One picture of a media block: the picture itself, a PDF's page, a video's frame.
#[derive(Default)]
pub struct Shot { pub pic: Option<Pic>, pub ink: Option<Ink>, /// what it is (a name, a timecode)
    pub head: String, /// the smaller line beside it ("page 3 of 12", "frame 240")
    pub sub: String, /// a note's words
    pub text: String, /// a video note: addressed or not
    pub done: Option<bool> }
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum What { Picture, Pdf, Video, File }
pub struct Item { pub what: What, pub name: String, /// where the file is on this machine (empty: not known)
    pub path: String, /// "pdf · 12 pages", "1920 × 1080 · 24 fps · 0:08", "2.4 MB"
    pub meta: String, /// a picture's width as the note shows it, when the user set one
    pub dw: Option<f64>, /// why there is no picture of it
    pub problem: Option<String>, /// the first is the block's own picture; a PDF's other drawn-on pages follow
    pub shots: Vec<Shot>, /// a video's notes
    pub notes: Vec<Shot> }
/// How much of a media block a writer wants.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Mode { /// clean pictures and the drawings beside them
    Layers, /// the drawings in the pictures
    Baked, /// for paper (a Word document, a PDF): the drawings in the pictures, and every picture a PNG or a JPEG
    Paper, /// no pictures: names, paths, facts
    Brief }
pub trait Source { fn item(&mut self, descriptor: &str, mode: Mode, opts: Opts) -> Item; }

struct Walk<'a> { all: HashMap<BlockId, &'a Block>, opts: Opts }
impl<'a> Walk<'a> {
    fn new(blocks: impl IntoIterator<Item = &'a Block>, opts: Opts) -> Self { Walk { all: blocks.into_iter().map(|b| (b.id, b)).collect(), opts } }
    fn kids(&self, id: BlockId) -> &[BlockId] { self.all.get(&id).map(|b| b.children.as_slice()).unwrap_or(&[]) }
}
fn hex_of(c: &str) -> String { mn_layout::NAMED.iter().find(|(n, _)| *n == c).map(|(_, h)| h.to_string()).unwrap_or_else(|| c.to_string()) }
/// `#RRGGBB` as `r,g,b`.
fn rgb(c: &str) -> Option<String> { let h = hex_of(c); let h = h.trim_start_matches('#'); if h.len() < 6 { return None } let n = |i: usize| u8::from_str_radix(h.get(i..i + 2)?, 16).ok(); Some(format!("{},{},{}", n(0)?, n(2)?, n(4)?)) }
/// The chosen option of a chip's payload: (id, colour).
fn chip_of(payload: &str) -> (String, String) {
    let v: serde_json::Value = serde_json::from_str(payload).unwrap_or_default(); let id = v.get("v").and_then(|x| x.as_str()).unwrap_or("").to_string();
    let c = v.get("o").and_then(|o| o.as_array()).and_then(|o| o.iter().find(|o| o.get("id").and_then(|i| i.as_str()) == Some(&id))).and_then(|o| o.get("c")).and_then(|c| c.as_str()).unwrap_or("").to_string();
    (id, c)
}
/// The text cut where its formatting changes: each piece with the marks over it.
fn pieces(b: &Block) -> Vec<(&str, Vec<&Mark>)> {
    let mut cuts: Vec<usize> = vec![0, b.text.len()]; for sp in b.spans.as_slice() { cuts.push(sp.start.min(b.text.len())); cuts.push(sp.end.min(b.text.len())); }
    cuts.sort(); cuts.dedup();
    cuts.windows(2).filter(|w| w[1] > w[0] && b.text.is_char_boundary(w[0]) && b.text.is_char_boundary(w[1])).map(|w| (&b.text[w[0]..w[1]], b.spans.as_slice().iter().filter(|sp| sp.start <= w[0] && sp.end >= w[1]).map(|sp| &sp.mark).collect())).collect()
}

// -- HTML ----------------------------------------------------------------------------------------------------------

/// What a table's column gives its cells.
#[derive(Clone, Default)]
struct Col { check: bool, options: Vec<(String, String)>, mono: bool }
fn html_inline(b: &Block, col: &Col) -> String {
    let mut out = String::new();
    for (text, marks) in pieces(b) {
        let (mut open, mut close) = (String::new(), String::new());
        for m in marks {
            let (o, c) = match m { Mark::Bold => ("<strong>".to_string(), "</strong>"), Mark::Italic => ("<em>".into(), "</em>"), Mark::Underline => ("<u>".into(), "</u>"), Mark::Strike => ("<s>".into(), "</s>"), Mark::Code => ("<code>".into(), "</code>"),
                Mark::Link(u) => (format!("<a href=\"{}\">", esc(u)), "</a>"), Mark::Color(v) => (format!("<span style=\"color:{}\">", esc(&hex_of(v))), "</span>"),
                Mark::Highlight(v) => (match rgb(v) { Some(c) => format!("<mark style=\"background:rgba({c},.3)\">"), None => "<mark>".into() }, "</mark>"),
                Mark::Choice(p) => { let (id, own) = chip_of(p); let c = col.options.iter().find(|(i, _)| *i == id).map(|(_, c)| c.clone()).unwrap_or(own);
                    (match rgb(&c) { Some(c) => format!("<span class=\"chip\" style=\"background:rgba({c},.28);border-color:rgba({c},.55)\">"), None => "<span class=\"chip\">".into() }, "</span>") } };
            open.push_str(&o); close.insert_str(0, c);
        }
        out.push_str(&open); out.push_str(&esc(text).replace('\n', "<br>")); out.push_str(&close);
    }
    out
}
fn svg(ink: &Ink) -> String { format!("<svg class=\"ink\" viewBox=\"0 0 {} {}\" preserveAspectRatio=\"none\" aria-hidden=\"true\">{}</svg>", ink.w, ink.h, ink.paths) }
fn thumb(s: &Shot, cls: &str) -> String {
    let Some(p) = &s.pic else { return format!("<div class=\"nothumb {cls}\"></div>") };
    format!("<div class=\"thumb {cls}\" tabindex=\"0\" data-tc=\"{}\" data-sub=\"{}\" data-txt=\"{}\"><img class=\"base\" src=\"data:{};base64,{}\" width=\"{}\" height=\"{}\" alt=\"{}\">{}</div>",
        esc(&s.head), esc(&s.sub), esc(&s.text), p.mime, b64(&p.bytes), p.w, p.h, esc(&s.head), s.ink.as_ref().map(svg).unwrap_or_default())
}
fn html_media(it: &Item, opts: Opts, out: &mut String) {
    let lines = |out: &mut String| { out.push_str(&format!("<span class=\"name\">{}</span>", esc(&it.name)));
        if opts.paths && !it.path.is_empty() { out.push_str(&format!("<span class=\"path\">{}</span>", esc(&it.path))); }
        if !it.meta.is_empty() { out.push_str(&format!("<span class=\"meta\">{}</span>", esc(&it.meta))); }
        if let Some(p) = &it.problem { out.push_str(&format!("<span class=\"meta bad\">{}</span>", esc(p))); } };
    match (it.what, it.shots.first().filter(|s| s.pic.is_some())) {
        (What::Picture, Some(s)) => out.push_str(&format!("<figure class=\"pic\"{}>{}</figure>\n", it.dw.map(|w| format!(" style=\"width:{}px\"", w.round())).unwrap_or_default(), thumb(s, "wide"))),
        (_, first) => {
            out.push_str(&format!("<figure class=\"ref {}\">", match it.what { What::Picture => "picture", What::Pdf => "pdf", What::Video => "video", What::File => "file" }));
            if let Some(s) = first { out.push_str(&thumb(s, "wide")); }
            out.push_str("<figcaption>"); lines(out); out.push_str("</figcaption></figure>\n");
        }
    }
    for s in it.shots.iter().skip(1) { out.push_str(&format!("<figure class=\"ref page\">{}<figcaption><span class=\"name\">{}</span><span class=\"meta\">{}</span></figcaption></figure>\n", thumb(s, "wide"), esc(&s.head), esc(&s.sub))); }
    if !it.notes.is_empty() {
        out.push_str("<table class=\"vnotes\"><tbody>\n");
        for s in &it.notes {
            out.push_str(&format!("<tr class=\"{}\"><td class=\"fr\">{}</td><td class=\"tc\"><div class=\"main\">{}</div><div class=\"sub\">{}</div></td><td class=\"tx\"><div class=\"text\">{}</div></td><td class=\"st\">{}</td></tr>\n",
                if s.done == Some(true) { "done" } else { "open" }, thumb(s, ""), esc(&s.head), esc(&s.sub), if s.text.trim().is_empty() { "<span class=\"none\">No text</span>".to_string() } else { esc(s.text.trim_end()) },
                match s.done { Some(true) => "<span class=\"state done\">Addressed</span>", Some(false) => "<span class=\"state\">Open</span>", None => "" }));
        }
        out.push_str("</tbody></table>\n");
    }
}
fn html_blocks(w: &Walk, ids: &[BlockId], col: &Col, src: &mut dyn Source, out: &mut String) {
    let mut lists: Vec<&str> = vec![]; let mut li = false;
    fn close(lists: &mut Vec<&str>, li: &mut bool, out: &mut String) { if *li { out.push_str("</li>"); } if let Some(t) = lists.pop() { out.push_str(&format!("</{t}>\n")); } *li = !lists.is_empty(); }
    for id in ids { let Some(b) = w.all.get(id) else { continue };
        let item = match &b.kind { Kind::Bullet { depth } => Some(("ul", *depth, None)), Kind::Numbered { depth } => Some(("ol", *depth, None)), Kind::Task { depth, state } => Some(("ul", *depth, Some(*state))), _ => None };
        if let Some((tag, depth, task)) = item {
            let want = depth as usize + 1;
            while lists.len() > want { close(&mut lists, &mut li, out); }
            if lists.len() == want && lists[want - 1] != tag { close(&mut lists, &mut li, out); }
            if lists.len() == want && li { out.push_str("</li>\n"); li = false; }
            while lists.len() < want { if !lists.is_empty() && !li { out.push_str("<li class=\"nest\">"); } lists.push(tag); out.push_str(&format!("<{tag}>")); li = false; }
            match task { Some(st) => out.push_str(&format!("<li class=\"task s{st}\"><span class=\"box\"></span>{}", html_inline(b, col))), None => out.push_str(&format!("<li>{}", html_inline(b, col))) }
            li = true; continue
        }
        while !lists.is_empty() { close(&mut lists, &mut li, out); }
        match &b.kind {
            Kind::Paragraph if col.check => { let st = match mn_edit::typed::chosen(&b.text, &b.spans).as_deref() { Some("1") => 1, Some("2") => 2, _ => 0 }; out.push_str(&format!("<span class=\"task s{st}\"><span class=\"box\"></span></span>")); }
            Kind::Paragraph => { let t = html_inline(b, col); out.push_str(&format!("<p{}>{}</p>\n", if col.mono { " class=\"tcode\"" } else { "" }, if t.is_empty() { "&nbsp;".to_string() } else { t })) }
            Kind::Heading { level } => { let n = (*level).clamp(1, 6); out.push_str(&format!("<h{n}>{}</h{n}>\n", html_inline(b, col))) }
            Kind::Quote => out.push_str(&format!("<blockquote><p>{}</p></blockquote>\n", html_inline(b, col))),
            Kind::Code { lang } => out.push_str(&format!("<pre>{}<code>{}</code></pre>\n", if lang.is_empty() { String::new() } else { format!("<span class=\"lang\">{}</span>", esc(lang)) }, esc(&b.text))),
            Kind::Divider => out.push_str("<hr>\n"),
            Kind::Media { descriptor } => html_media(&src.item(descriptor, Mode::Layers, w.opts), w.opts, out),
            Kind::Columns => { out.push_str("<div class=\"lanes\">");
                for c in &b.children { let share = match w.all.get(c).map(|c| &c.kind) { Some(Kind::Column { width }) if *width > 0.0 => *width as f64, _ => 1.0 / b.children.len().max(1) as f64 };
                    out.push_str(&format!("<div class=\"lane\" style=\"flex:{share:.4} 1 0\">")); html_blocks(w, w.kids(*c), col, src, out); out.push_str("</div>"); }
                out.push_str("</div>\n") }
            Kind::Column { .. } => html_blocks(w, &b.children, col, src, out),
            Kind::Table { header_rows, columns } => html_table(w, b, *header_rows as usize, columns, src, out),
            _ => {}
        }
    }
    while !lists.is_empty() { close(&mut lists, &mut li, out); }
}
fn col_of(c: Option<&ColumnSpec>, header: bool) -> Col {
    match c.map(|c| &c.kind) { _ if header => Col::default(), Some(ColumnKind::Check) => Col { check: true, ..Default::default() }, Some(ColumnKind::Choice { options }) => Col { options: options.iter().map(|o| (o.id.clone(), o.color.clone())).collect(), ..Default::default() },
        Some(ColumnKind::Timecode { .. }) => Col { mono: true, ..Default::default() }, _ => Col::default() }
}
fn html_table(w: &Walk, t: &Block, header_rows: usize, columns: &[ColumnSpec], src: &mut dyn Source, out: &mut String) {
    let n = t.children.iter().map(|r| w.kids(*r).len()).max().unwrap_or(0).max(columns.len()); if n == 0 { return }
    out.push_str("<div class=\"tablewrap\"><table class=\"grid\"><colgroup>");
    for i in 0..n { match columns.get(i).and_then(|c| c.width) { Some(px) => out.push_str(&format!("<col style=\"width:{px}px\">")), None => out.push_str("<col>") } }
    out.push_str("</colgroup>\n");
    let style = |bg: &str, fg: &str, align: u8| { let mut s = String::new(); if let Some(c) = rgb(bg) { s += &format!("background:rgba({c},.22);"); } if !fg.is_empty() { s += &format!("color:{};", esc(&hex_of(fg))); } match align { 1 => s += "text-align:center;", 2 => s += "text-align:right;", _ => {} } if s.is_empty() { s } else { format!(" style=\"{s}\"") } };
    for (ri, r) in t.children.iter().enumerate() {
        let header = ri < header_rows; let (rbg, rfg) = match w.all.get(r).map(|r| &r.kind) { Some(Kind::Row { bg, fg }) => (bg.as_str(), fg.as_str()), _ => ("", "") };
        if ri == 0 && header { out.push_str("<thead>"); } if ri == header_rows { out.push_str(if ri > 0 { "</thead><tbody>" } else { "<tbody>" }); }
        out.push_str("<tr>"); let cells = w.kids(*r); let tag = if header { "th" } else { "td" };
        for i in 0..n { let spec = columns.get(i);
            let (cbg, cfg) = match cells.get(i).and_then(|c| w.all.get(c)).map(|c| &c.kind) { Some(Kind::Cell { bg, fg }) => (bg.as_str(), fg.as_str()), _ => ("", "") };
            let pick = |a: &'_ str, b: &'_ str, c: &'_ str| if !a.is_empty() { a.to_string() } else if !b.is_empty() { b.to_string() } else { c.to_string() };
            let (bg, fg) = (pick(cbg, rbg, spec.map(|s| s.bg.as_str()).unwrap_or("")), pick(cfg, rfg, spec.map(|s| s.fg.as_str()).unwrap_or("")));
            out.push_str(&format!("<{tag}{}>", style(&bg, &fg, spec.map(|s| s.align).unwrap_or(0))));
            if let Some(c) = cells.get(i) { html_blocks(w, w.kids(*c), &col_of(spec, header), src, out); }
            out.push_str(&format!("</{tag}>")); }
        out.push_str("</tr>\n");
    }
    out.push_str(if header_rows >= t.children.len() && header_rows > 0 { "</thead>" } else { "</tbody>" }); out.push_str("</table></div>\n");
}

/// The fonts an export carries: (family, weight, style, bytes).
pub type Fonts = Vec<(&'static str, u16, &'static str, &'static [u8])>;
pub fn fonts() -> Fonts { use min_theme::fonts as f; vec![(f::FAMILY_SANS, 400, "normal", f::GEIST_REGULAR), (f::FAMILY_SANS, 700, "normal", f::GEIST_BOLD), (f::FAMILY_SANS, 400, "italic", f::GEIST_ITALIC), (f::FAMILY_MONO, 400, "normal", f::GEIST_MONO_REGULAR)] }

/// The note as one HTML file. `blocks`: every block; `tops`: the ones at the top, in order. `exported`: the time, as shown.
pub fn html<'a>(blocks: impl IntoIterator<Item = &'a Block>, tops: &[BlockId], title: &str, exported: &str, opts: Opts, fonts: &Fonts, src: &mut dyn Source) -> String {
    let w = Walk::new(blocks, opts); let mut body = String::new(); html_blocks(&w, tops, &Col::default(), src, &mut body);
    let mut o = String::with_capacity(body.len() + 256 * 1024);
    o += "<!DOCTYPE html>\n<html lang=\"en\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\"><meta name=\"generator\" content=\"minNotes\">\n";
    o += &format!("<title>{}</title>\n<style>\n", esc(title));
    o += &qcv_notes::export::font_faces(fonts); o += qcv_notes::export::TOKENS; o += CSS;
    o += &format!("@media print{{\n{}\n{PRINT}}}\n", qcv_notes::export::PRINT_TOKENS);
    o += "</style></head><body>\n<main>\n";
    o += &format!("<header class=\"doc\"><div class=\"titles\"><div class=\"name\">{}</div><div class=\"meta\"><span>Exported {}</span></div></div></header>\n<article>\n", esc(title), esc(exported));
    o += &body; o += "</article>\n</main>\n"; o += LIGHTBOX; o += "</body></html>\n"; o
}

/// The note's page on the family's tokens: a readable column, quiet type, the table and the frames bordered as QCView's
/// synopsis is. Pictures and frames open in the lightbox.
const CSS: &str = r#"
*{box-sizing:border-box}
html{background:var(--base)}
body{margin:0;color:var(--g800);font:400 15px/24px var(--sans);-webkit-font-smoothing:antialiased}
::selection{background:var(--sel);color:var(--g900)}
main{padding:40px 48px 96px;max-width:960px;margin:0 auto}
header.doc{padding-bottom:16px;margin-bottom:28px;border-bottom:1px solid var(--g300)}
header.doc .name{font:500 13px/20px var(--mono);color:var(--g700);overflow-wrap:anywhere}
header.doc .meta{font:12px/18px var(--mono);color:var(--g600)}
article>*:first-child{margin-top:0}
h1,h2,h3,h4,h5,h6{color:var(--g900);font-weight:400;margin:1.5em 0 .45em;overflow-wrap:anywhere}
h1{font-size:30px;line-height:38px}h2{font-size:24px;line-height:32px}h3{font-size:20px;line-height:28px}h4{font-size:17px;line-height:26px}h5{font-size:15px;line-height:24px;font-weight:700}h6{font-size:13px;line-height:20px;font-weight:700;color:var(--g700)}
p{margin:0 0 .7em;overflow-wrap:anywhere}
a{color:var(--accent-hi);text-decoration:none}a:hover{text-decoration:underline}
strong{font-weight:700;color:var(--g900)}
mark{color:inherit;background:rgba(255,236,89,.3);border-radius:2px;padding:0 1px}
code{font:13px/20px var(--mono);background:var(--l2);border:1px solid var(--g200);border-radius:var(--rs);padding:1px 4px}
pre{position:relative;margin:0 0 1em;padding:14px 16px;background:var(--l1);border:1px solid var(--g300);border-radius:var(--r);overflow:auto}
pre code{display:block;background:none;border:0;padding:0;white-space:pre;color:var(--g800)}
pre .lang{position:absolute;top:8px;right:12px;font:11px/16px var(--mono);color:var(--g500)}
blockquote{margin:0 0 1em;padding:2px 0 2px 16px;border-left:3px solid var(--g400);color:var(--g700)}
blockquote p:last-child{margin-bottom:0}
hr{border:0;border-top:1px solid var(--g300);margin:1.6em 0}
ul,ol{margin:0 0 .8em;padding-left:26px}
li{margin:.15em 0;overflow-wrap:anywhere}li>ul,li>ol{margin:.15em 0 0}
li.nest{list-style:none}
li.task{list-style:none;position:relative}
li.task .box{position:absolute;left:-23px;top:5px}
.box{display:inline-block;width:14px;height:14px;border:1.5px solid var(--g500);border-radius:3px;vertical-align:-2px}
.task.s1 .box{border-color:var(--accent-hi);background:linear-gradient(var(--accent-hi),var(--accent-hi)) center/8px 2px no-repeat}
.task.s2 .box{border-color:var(--pos);background:var(--pos) url("data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 14 14'%3E%3Cpath d='M3 7.2l2.6 2.6L11 4.4' fill='none' stroke='%23fff' stroke-width='1.8' stroke-linecap='round' stroke-linejoin='round'/%3E%3C/svg%3E") center/12px no-repeat}
li.task.s2{color:var(--g600)}
.chip{display:inline-block;padding:0 8px;border-radius:10px;border:1px solid var(--g400);background:var(--l2);font-size:13px;line-height:20px;color:var(--g900);white-space:nowrap}
.lanes{display:flex;gap:28px;margin:0 0 .8em;align-items:flex-start}.lane{min-width:0}
.tablewrap{overflow-x:auto;margin:0 0 1.1em}
table.grid{border-collapse:separate;border-spacing:0;background:var(--l1);border:1px solid var(--g300);border-radius:var(--r);overflow:hidden;font-size:14px;line-height:21px}
table.grid th,table.grid td{padding:8px 12px;vertical-align:top;text-align:left;border-bottom:1px solid var(--g200);border-right:1px solid var(--g200)}
table.grid th{font-weight:500;color:var(--g900);background:var(--l2);border-bottom-color:var(--g300)}
table.grid tr>*:last-child{border-right:0}table.grid tbody tr:last-child>*,table.grid thead:last-child tr:last-child>*{border-bottom:0}
table.grid p{margin:0}table.grid p+p{margin-top:.4em}table.grid ul,table.grid ol{margin:0}
.tcode{font:13px/21px var(--mono);white-space:nowrap}
figure{margin:0 0 1.1em;max-width:100%}
figure.ref{background:var(--l1);border:1px solid var(--g300);border-radius:var(--r);overflow:hidden}
figure.ref .thumb{border-radius:0}figure.ref .thumb::after{border:0;border-bottom:1px solid var(--g300);border-radius:0}
figcaption{display:flex;flex-direction:column;gap:1px;padding:10px 14px}
figcaption .name{font-weight:500;color:var(--g900);overflow-wrap:anywhere}
figcaption .path{font:12px/18px var(--mono);color:var(--g600);word-break:break-all}
figcaption .meta{font:12px/18px var(--mono);color:var(--g600)}
figcaption .bad{color:#f76d74}
.thumb{position:relative;border-radius:var(--rs);overflow:hidden;background:#000;cursor:zoom-in;outline:none;line-height:0;width:248px}
.thumb.wide{width:auto;max-width:100%}
figure.pic .thumb.wide{display:inline-block;vertical-align:top}figure.pic .thumb.wide img{width:auto;max-width:100%}
figure.pic[style] .thumb.wide{display:block}figure.pic[style] .thumb.wide img{width:100%}
.thumb img{display:block;width:100%;height:auto}
.thumb .ink{position:absolute;inset:0;width:100%;height:100%;pointer-events:none;overflow:visible}
.thumb:focus-visible{box-shadow:0 0 0 2px var(--accent)}
.thumb::after{content:"";position:absolute;inset:0;border:1px solid rgba(255,255,255,.08);border-radius:var(--rs);pointer-events:none}
.nothumb{width:248px;aspect-ratio:16/9;border-radius:var(--rs);background:var(--l2)}
html.noink .ink,html.noink #lb .stage .ink{display:none}
table.vnotes{width:100%;border-collapse:separate;border-spacing:0;background:var(--l1);border:1px solid var(--g300);border-radius:var(--r);overflow:hidden;margin:-.5em 0 1.1em;font-size:14px;line-height:20px}
table.vnotes td{padding:12px;vertical-align:top;border-bottom:1px solid var(--g200)}
table.vnotes tr:last-child td{border-bottom:0}
table.vnotes td.fr{width:272px}table.vnotes td.tc{width:140px}table.vnotes td.st{width:108px;text-align:right}
td.tc .main{font:500 13px/20px var(--mono);color:var(--g900);white-space:nowrap}
td.tc .sub{font:12px/18px var(--mono);color:var(--g600)}
.text{white-space:pre-wrap;overflow-wrap:anywhere}.text .none{color:var(--g500);font-style:italic}
tr.done .text,tr.done td.tc .main{color:var(--g600)}
.state{display:inline-flex;align-items:center;gap:6px;height:24px;padding:0 9px;border-radius:12px;font:500 12px/16px var(--sans);white-space:nowrap;border:1px solid var(--g400);color:var(--g700)}
.state::before{content:"";width:6px;height:6px;border-radius:50%;background:var(--g500)}
.state.done{color:var(--g800);border-color:rgba(9,157,89,.55);background:rgba(9,157,89,.12)}.state.done::before{background:var(--pos)}
#lb{position:fixed;inset:0;background:rgba(0,0,0,.94);z-index:99;display:flex;flex-direction:column;align-items:center;justify-content:center;opacity:0;visibility:hidden;pointer-events:none;transition:opacity 160ms ease,visibility 0s linear 160ms}
#lb.open{opacity:1;visibility:visible;pointer-events:auto;transition:opacity 160ms ease}
@keyframes lbin{from{opacity:0}to{opacity:1}}
#lb .stage{position:relative;line-height:0;animation:lbin 160ms ease}
#lb .stage img{display:block;max-width:90vw;max-height:calc(100vh - 150px);width:auto;height:auto}
#lb .stage .ink{position:absolute;inset:0;width:100%;height:100%;overflow:visible}
#lb .cap{margin-top:14px;max-width:min(90vw,960px);display:flex;gap:16px;align-items:baseline;color:var(--g800);font-size:14px;line-height:20px}
#lb .cap .tc{font:500 13px var(--mono);color:var(--g900);white-space:nowrap}
#lb .cap .sub{font:12px var(--mono);color:var(--g600);white-space:nowrap}
#lb .cap .txt{white-space:pre-wrap;overflow-wrap:anywhere;max-height:72px;overflow:auto}
#lb button{position:fixed;z-index:100;background:rgba(27,27,27,.8);color:var(--g800);border:1px solid var(--g300);border-radius:var(--r);cursor:pointer;padding:0;display:flex;align-items:center;justify-content:center;transition:background-color 100ms,color 100ms}
#lb button:hover{background:var(--g300);color:var(--g900)}
#lb button svg{display:block}
#lb .prev,#lb .next{top:50%;transform:translateY(-50%);width:44px;height:64px}
#lb .prev{left:16px}#lb .next{right:16px}
#lb .close{top:14px;right:16px;width:40px;height:40px}
#lb .strokes{top:14px;right:64px;height:40px;padding:0 12px;gap:8px;font:500 13px var(--sans)}
#lb .strokes.off{color:var(--g600)}
#lb .strokes kbd{font:11px var(--mono);color:var(--g600);border:1px solid var(--g400);border-radius:3px;padding:0 4px}
#lb .cnt{position:fixed;top:24px;left:50%;transform:translateX(-50%);font:12px var(--mono);color:var(--g600)}
@media (max-width:760px){main{padding:24px 16px 64px}.lanes{flex-direction:column;gap:0}table.vnotes td.fr{width:140px}table.vnotes .thumb,table.vnotes .nothumb{width:128px}table.vnotes td.st{display:none}}
"#;
/// Inside `@media print`, after the light tokens.
const PRINT: &str = "#lb{display:none!important}main{padding:0;max-width:none}tr,figure,pre{break-inside:avoid}h1,h2,h3,h4,h5,h6{break-after:avoid}.thumb{background:none}\n";

/// QCView's export lightbox, for any picture of the page: click one; the arrows step, Esc closes, A hides the drawings.
const LIGHTBOX: &str = r#"<div id="lb" aria-hidden="true"><span class="cnt"></span>
<button class="strokes" title="Show or hide the drawings (A)">Drawings <kbd>A</kbd></button>
<button class="close" aria-label="Close"><svg viewBox="0 0 24 24" width="18" height="18" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><path d="M6 6l12 12M18 6 6 18"/></svg></button>
<button class="prev" aria-label="Previous"><svg viewBox="0 0 24 24" width="22" height="22" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M15 5 8 12l7 7"/></svg></button>
<div class="stage"></div><div class="cap"><span class="tc"></span><span class="sub"></span><span class="txt"></span></div>
<button class="next" aria-label="Next"><svg viewBox="0 0 24 24" width="22" height="22" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M9 5l7 7-7 7"/></svg></button></div>
<script>
(function(){
var root=document.documentElement,lb=document.getElementById('lb'),stage=lb.querySelector('.stage'),cnt=lb.querySelector('.cnt'),cap=lb.querySelector('.cap'),sb=lb.querySelector('.strokes'),cur=-1;
try{if(localStorage.getItem('mn.noink')==='1')root.classList.add('noink')}catch(e){}
function paintSb(){sb.classList.toggle('off',root.classList.contains('noink'))}paintSb();
function toggleInk(){root.classList.toggle('noink');try{localStorage.setItem('mn.noink',root.classList.contains('noink')?'1':'0')}catch(e){}paintSb()}
var list=[].slice.call(document.querySelectorAll('.thumb'));
function show(i){if(!list.length)return;cur=(i+list.length)%list.length;var t=list[cur];
stage.innerHTML='';[].forEach.call(t.children,function(c){stage.appendChild(c.cloneNode(true))});
stage.style.animation='none';void stage.offsetWidth;stage.style.animation='';
cap.querySelector('.tc').textContent=t.dataset.tc||'';cap.querySelector('.sub').textContent=t.dataset.sub||'';cap.querySelector('.txt').textContent=t.dataset.txt||'';
sb.style.display=t.querySelector('.ink')?'':'none';
cnt.textContent=(cur+1)+' / '+list.length;lb.classList.add('open');lb.setAttribute('aria-hidden','false')}
function close(){lb.classList.remove('open');lb.setAttribute('aria-hidden','true');cur=-1}
list.forEach(function(t,i){function go(){show(i)}t.addEventListener('click',go);t.addEventListener('keydown',function(e){if(e.key==='Enter'||e.key===' '){e.preventDefault();go()}})});
lb.addEventListener('click',function(e){if(e.target===lb||e.target===stage.parentNode)close()});
lb.querySelector('.prev').addEventListener('click',function(){show(cur-1)});
lb.querySelector('.next').addEventListener('click',function(){show(cur+1)});
lb.querySelector('.close').addEventListener('click',close);
sb.addEventListener('click',toggleInk);
document.addEventListener('keydown',function(e){if(e.metaKey||e.ctrlKey||e.altKey)return;var k=e.target&&e.target.tagName;if(k==='INPUT'||k==='TEXTAREA')return;
if(e.key==='a'||e.key==='A'){toggleInk();e.preventDefault();return}
if(cur<0)return;if(e.key==='Escape')close();else if(e.key==='ArrowLeft')show(cur-1);else if(e.key==='ArrowRight')show(cur+1);else return;e.preventDefault()});
})();
</script>
"#;

// -- Markdown ------------------------------------------------------------------------------------------------------

/// Prose with Markdown's own characters made plain.
fn md_esc(s: &str) -> String { let mut o = String::with_capacity(s.len() + 8); for c in s.chars() { if matches!(c, '\\' | '`' | '*' | '_' | '[' | ']' | '~' | '<') { o.push('\\'); } o.push(c); } o }
/// A line that would read as a block's marker (a heading, a quote, a list item, a rule, a fence) gets a backslash.
fn md_line(s: &str) -> String {
    let t = s.trim_start(); let digits = t.chars().take_while(|c| c.is_ascii_digit()).count();
    let marker = t.starts_with('#') || t.starts_with('>') || t.starts_with("- ") || t.starts_with("+ ") || t == "-" || t == "+" || (t.len() >= 3 && t.chars().all(|c| c == '-' || c == ' ') && t.matches('-').count() >= 3) || t.starts_with("===")
        || (digits > 0 && digits <= 9 && matches!(t[digits..].chars().next(), Some('.' | ')')) && matches!(t[digits + 1..].chars().next(), None | Some(' ')));
    if !marker { return s.to_string() }
    let lead = s.len() - t.len(); if digits > 0 { format!("{}{}\\{}", &s[..lead], &t[..digits], &t[digits..]) } else { format!("{}\\{}", &s[..lead], t) }
}
fn fence_for(text: &str, least: usize) -> String { let (mut run, mut long) = (0, 0); for c in text.chars() { if c == '`' { run += 1; long = long.max(run); } else { run = 0; } } "`".repeat((long + 1).max(least)) }
fn md_target(u: &str) -> String { if u.contains([' ', '(', ')']) { format!("<{u}>") } else { u.to_string() } }
/// Pieces wrapped mark by mark, the outermost first: neighbours that share a mark share its markers.
fn md_wrap(ps: &[(&str, Vec<&Mark>)], order: &[u8]) -> String {
    let Some((k, rest)) = order.split_first() else { return ps.iter().map(|(t, m)| if m.iter().any(|m| matches!(m, Mark::Code)) { t.to_string() } else { md_esc(t).replace('\n', "  \n") }).collect() };
    let key = |m: &[&Mark]| -> Option<String> { m.iter().find_map(|m| match (k, m) { (0, Mark::Link(u)) => Some(u.clone()), (1, Mark::Bold) | (2, Mark::Italic) | (3, Mark::Strike) | (4, Mark::Underline) | (5, Mark::Code) => Some(String::new()), _ => None }) };
    let (mut out, mut i) = (String::new(), 0);
    while i < ps.len() { let me = key(&ps[i].1); let mut j = i + 1; while j < ps.len() && key(&ps[j].1) == me { j += 1; }
        let inner = md_wrap(&ps[i..j], rest);
        match me { None => out.push_str(&inner),
            Some(u) => { // markers never hold the spaces at either end
                let (a, b) = (inner.len() - inner.trim_start().len(), inner.trim_end().len()); if b <= a { out.push_str(&inner); i = j; continue }
                let (lead, core, tail) = (&inner[..a], &inner[a..b], &inner[b..]);
                let wrapped = match k { 0 => format!("[{core}]({})", md_target(&u)), 1 => format!("**{core}**"), 2 => format!("*{core}*"), 3 => format!("~~{core}~~"), 4 => format!("<u>{core}</u>"),
                    _ => { let f = fence_for(core, 1); let pad = if core.starts_with('`') || core.ends_with('`') { " " } else { "" }; format!("{f}{pad}{core}{pad}{f}") } };
                out.push_str(lead); out.push_str(&wrapped); out.push_str(tail); } }
        i = j; }
    out
}
fn md_inline(b: &Block) -> String { md_wrap(&pieces(b), &[0, 1, 2, 3, 4, 5]) }
/// Where a writer's pictures go: (a name to start from, the bytes) → what the Markdown points at.
pub type Sink<'s> = &'s mut dyn FnMut(&str, &Pic) -> Option<String>;
fn stem(name: &str) -> String { let n = name.rsplit_once('.').map(|x| x.0).unwrap_or(name); if n.is_empty() { "picture".into() } else { n.to_string() } }
fn md_media(it: &Item, opts: Opts, sink: &mut Option<Sink>, out: &mut Vec<String>) {
    let keep = |s: &Shot, name: &str, sink: &mut Option<Sink>| -> Option<String> { let p = s.pic.as_ref()?; (sink.as_mut()?)(name, p) };
    let alt = |s: &str| s.replace(['[', ']'], "");
    let facts = |out: &mut Vec<String>| { let mut l = vec![it.name.clone()]; if opts.paths && !it.path.is_empty() { l.push(it.path.clone()); } if !it.meta.is_empty() { l.push(it.meta.clone()); } if let Some(p) = &it.problem { l.push(p.clone()); }
        let body = l.join("\n"); let f = fence_for(&body, 3); out.push(format!("{f}{}\n{body}\n{f}", match it.what { What::Pdf => "pdf", What::Video => "video", _ => "file" })); };
    let first = it.shots.first();
    match it.what {
        What::Picture => match first.and_then(|s| keep(s, &it.name, sink)) { Some(rel) => out.push(format!("![{}]({})", alt(&it.name), md_target(&rel))),
            // nothing written (a copy to the clipboard): the picture where it is
            None if it.problem.is_none() && !it.path.is_empty() => out.push(format!("![{}]({})", alt(&it.name), md_target(&format!("file://{}", it.path)))), None => facts(out) },
        _ => { if let Some(rel) = first.and_then(|s| keep(s, &stem(&it.name), sink)) { out.push(format!("![{}]({})", alt(&it.name), md_target(&rel))); } facts(out) }
    }
    for s in it.shots.iter().skip(1) { if let Some(rel) = keep(s, &format!("{}-{}", stem(&it.name), s.sub.replace(' ', "-")), sink) { out.push(format!("![{}]({})", alt(&s.head), md_target(&rel))); out.push(format!("*{} \u{2014} {}*", md_esc(&s.head), md_esc(&s.sub))); } }
    for s in &it.notes {
        out.push(format!("**{}** ({}){}", md_esc(&s.head), md_esc(&s.sub), if s.done == Some(true) { " \u{2705}" } else { "" }));
        if let Some(rel) = keep(s, &format!("{}-{}", stem(&it.name), s.head.replace(':', "_")), sink) { out.push(format!("![{}]({})", alt(&s.head), md_target(&rel))); }
        if !s.text.trim().is_empty() { out.push(s.text.trim_end().lines().map(|l| md_line(&md_esc(l))).collect::<Vec<_>>().join("  \n")); }
    }
}
/// A table cell on one line.
fn md_cell(w: &Walk, cell: BlockId, col: &Col, src: &mut dyn Source, sink: &mut Option<Sink>) -> String {
    let mut parts: Vec<String> = vec![];
    for id in w.kids(cell) { let Some(b) = w.all.get(id) else { continue };
        parts.push(match &b.kind {
            Kind::Paragraph if col.check => match mn_edit::typed::chosen(&b.text, &b.spans).as_deref() { Some("1") => "[/]".into(), Some("2") => "[x]".into(), _ => "[ ]".into() },
            Kind::Bullet { .. } | Kind::Numbered { .. } => format!("\u{2022} {}", md_inline(b)), Kind::Task { state, .. } => format!("{} {}", ["[ ]", "[/]", "[x]"][(*state).min(2) as usize], md_inline(b)),
            Kind::Code { .. } => { let f = fence_for(&b.text, 1); format!("{f}{}{f}", b.text.replace('\n', " ")) } Kind::Divider => "---".into(),
            Kind::Media { descriptor } => { let mut v = vec![]; md_media(&src.item(descriptor, if sink.is_some() { Mode::Baked } else { Mode::Brief }, w.opts), w.opts, sink, &mut v); v.into_iter().filter(|l| !l.starts_with("```")).collect::<Vec<_>>().join("<br>") }
            k if k.is_text() => md_inline(b), _ => continue });
    }
    parts.join("<br>").replace("  \n", "<br>").replace('\n', "<br>").replace('|', "\\|")
}
fn md_blocks(w: &Walk, ids: &[BlockId], src: &mut dyn Source, sink: &mut Option<Sink>, out: &mut Vec<String>, tight: &mut Vec<bool>) {
    let mut number: Vec<u32> = vec![];
    for id in ids { let Some(b) = w.all.get(id) else { continue };
        let before = out.len();
        let listed = matches!(b.kind, Kind::Bullet { .. } | Kind::Numbered { .. } | Kind::Task { .. });
        let item = |depth: u8, mark: String, b: &Block| { let pad = "  ".repeat(depth as usize); let body = md_inline(b); let cont = format!("{pad}{}", " ".repeat(mark.chars().count() + 1)); format!("{pad}{mark} {}", body.replace("  \n", &format!("  \n{cont}"))) };
        if let Kind::Numbered { depth } = &b.kind { let d = *depth as usize; number.resize(d + 1, 0); number[d] += 1; } else if let Kind::Bullet { depth } | Kind::Task { depth, .. } = &b.kind { number.truncate(*depth as usize + 1); if let Some(n) = number.get_mut(*depth as usize) { *n = 0; } } else { number.clear(); }
        match &b.kind {
            Kind::Paragraph => out.push(md_inline(b).split('\n').map(md_line).collect::<Vec<_>>().join("\n")),
            Kind::Heading { level } => out.push(format!("{} {}", "#".repeat((*level).clamp(1, 6) as usize), md_inline(b).replace("  \n", " "))),
            Kind::Quote => out.push(md_inline(b).split('\n').map(|l| format!("> {l}")).collect::<Vec<_>>().join("\n")),
            Kind::Code { lang } => { let f = fence_for(&b.text, 3); out.push(format!("{f}{lang}\n{}\n{f}", b.text)) }
            Kind::Bullet { depth } => out.push(item(*depth, "-".into(), b)),
            Kind::Numbered { depth } => out.push(item(*depth, format!("{}.", number[*depth as usize]), b)),
            Kind::Task { depth, state } => out.push(item(*depth, format!("- {}", ["[ ]", "[/]", "[x]"][(*state).min(2) as usize]), b)),
            Kind::Divider => out.push("---".into()),
            Kind::Media { descriptor } => md_media(&src.item(descriptor, if sink.is_some() { Mode::Baked } else { Mode::Brief }, w.opts), w.opts, sink, out),
            // Markdown has no columns: the lanes follow one another
            Kind::Columns => for c in &b.children { md_blocks(w, w.kids(*c), src, sink, out, tight); },
            Kind::Column { .. } => md_blocks(w, &b.children, src, sink, out, tight),
            Kind::Table { header_rows, columns } => {
                let n = b.children.iter().map(|r| w.kids(*r).len()).max().unwrap_or(0).max(columns.len()); if n == 0 { continue }
                let row = |w: &Walk, r: BlockId, header: bool, src: &mut dyn Source, sink: &mut Option<Sink>| { let cells = w.kids(r); format!("| {} |", (0..n).map(|i| cells.get(i).map(|c| md_cell(w, *c, &col_of(columns.get(i), header), src, sink)).unwrap_or_default()).collect::<Vec<_>>().join(" | ")) };
                let mut lines = vec![]; let has = *header_rows > 0 && !b.children.is_empty();
                lines.push(if has { row(w, b.children[0], true, src, sink) } else { format!("|{}", "  |".repeat(n)) });
                lines.push(format!("| {} |", (0..n).map(|i| match columns.get(i).map(|c| c.align) { Some(1) => ":---:", Some(2) => "---:", _ => "---" }).collect::<Vec<_>>().join(" | ")));
                for r in b.children.iter().skip(has as usize) { lines.push(row(w, *r, false, src, sink)); }
                out.push(lines.join("\n"));
            }
            _ => {}
        }
        for _ in before..out.len() { tight.push(listed); }
    }
}
/// The note as Markdown. With no `sink` nothing is written anywhere: pictures are named where they are (a copy to
/// the clipboard). `title` empty: no name at the head (a copy of some blocks).
pub fn markdown<'a>(blocks: impl IntoIterator<Item = &'a Block>, tops: &[BlockId], title: &str, opts: Opts, src: &mut dyn Source, mut sink: Option<Sink>) -> String {
    let w = Walk::new(blocks, opts); let (mut out, mut tight) = (vec![], vec![]);
    md_blocks(&w, tops, src, &mut sink, &mut out, &mut tight);
    let mut s = String::new(); if !title.is_empty() { s.push_str(&format!("`{}`\n\n", title.replace('`', ""))); }
    // list items follow one another closely; everything else is a block of its own
    for (i, b) in out.iter().enumerate() { if i > 0 { s.push_str(if tight[i] && tight[i - 1] { "\n" } else { "\n\n" }); } s.push_str(b); }
    s.push('\n'); s
}

// -- the application's side: what a media block looks like, from its file ------------------------------------------

/// Stills (a video's frame, a PDF's page) are made this wide at most.
const STILL: u32 = 1280;
/// A picture is written out this long at most on its longer side when it has to be encoded here.
const LONGEST: u32 = 2560;
/// A picture a browser shows as it is goes in whole up to this size; a larger one is reduced.
const WHOLE: u64 = 12 << 20;

pub struct Files<'a> { pub media: &'a mut crate::media::Media, pub poster: &'a mut crate::video::Poster, pub gpu: Option<(wgpu::Device, wgpu::Queue)> }

fn encode(rgba: &[u8], w: u32, h: u32) -> Option<Pic> {
    if rgba.chunks_exact(4).any(|p| p[3] < 255) { return min_image::write::png_rgba8_bytes(rgba, w, h).ok().map(|bytes| Pic { bytes, mime: "image/png", ext: "png", w, h }) }
    let rgb: Vec<u8> = rgba.chunks_exact(4).flat_map(|p| [p[0], p[1], p[2]]).collect();
    min_image::write::jpeg8_bytes(&rgb, w, h, 90).ok().map(|bytes| Pic { bytes, mime: "image/jpeg", ext: "jpg", w, h })
}
/// Drawings laid into a picture's pixels.
fn bake(rgba: &mut [u8], w: u32, h: u32, ink: &Ink) {
    use resvg::{tiny_skia, usvg};
    let doc = format!("<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{w}\" height=\"{h}\" viewBox=\"0 0 {} {}\" preserveAspectRatio=\"none\">{}</svg>", ink.w, ink.h, ink.paths);
    let Ok(tree) = usvg::Tree::from_data(doc.as_bytes(), &usvg::Options::default()) else { return };
    for p in rgba.chunks_exact_mut(4) { let a = p[3] as u32; if a < 255 { for c in &mut p[..3] { *c = ((*c as u32 * a + 127) / 255) as u8; } } }
    let Some(mut pm) = tiny_skia::PixmapMut::from_bytes(rgba, w, h) else { return };
    resvg::render(&tree, tiny_skia::Transform::identity(), &mut pm);
    for p in rgba.chunks_exact_mut(4) { let a = p[3] as u32; if a > 0 && a < 255 { for c in &mut p[..3] { *c = ((*c as u32 * 255 + a / 2) / a).min(255) as u8; } } }
}
fn size_text(n: u64) -> String { if n >= 1 << 30 { format!("{:.2} GB", n as f64 / (1u64 << 30) as f64) } else if n >= 1 << 20 { format!("{:.1} MB", n as f64 / (1u64 << 20) as f64) } else if n >= 1 << 10 { format!("{:.0} KB", n as f64 / 1024.0) } else { format!("{n} bytes") } }

impl Files<'_> {
    /// A still as a shot: its drawings beside it, or in it.
    fn shot(&self, mut rgba: Vec<u8>, w: u32, h: u32, ink: Option<Ink>, mode: Mode) -> Shot {
        let ink = ink.filter(|i| !i.paths.is_empty());
        if matches!(mode, Mode::Baked | Mode::Paper) { if let Some(i) = &ink { bake(&mut rgba, w, h, i); } }
        Shot { pic: encode(&rgba, w, h), ink: if mode == Mode::Layers { ink } else { None }, ..Default::default() }
    }
}
impl Source for Files<'_> {
    fn item(&mut self, descriptor: &str, mode: Mode, opts: Opts) -> Item {
        use serde_json::Value;
        let rec: Value = serde_json::from_str(descriptor).unwrap_or_default();
        let what = match rec.get("kind").and_then(Value::as_str) { Some("pdf") => What::Pdf, Some("video") => What::Video, Some("file") => What::File, _ => What::Picture };
        let path = self.media.locate(descriptor);
        let name = rec.get("name").and_then(Value::as_str).filter(|n| !n.is_empty()).map(str::to_string).or_else(|| path.as_ref().and_then(|p| p.file_name()).map(|n| n.to_string_lossy().into_owned())).unwrap_or_else(|| "file".into());
        // a picture made of another file (a PSD, a RAW): the file it was made of is the one a reader would look for
        let shown = rec.get("preview").and_then(|p| p.get("src")).and_then(|s| self.media.locate(&serde_json::json!({ "src": s }).to_string())).or(path.clone());
        let mut it = Item { what, name, path: shown.as_ref().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default(), meta: String::new(), dw: rec.get("dw").and_then(Value::as_f64), problem: None, shots: vec![], notes: vec![] };
        let (rw, rh) = (rec.get("w").and_then(Value::as_f64).unwrap_or(0.0), rec.get("h").and_then(Value::as_f64).unwrap_or(0.0));
        let there = path.as_ref().filter(|p| p.exists());
        match what {
            What::File => { let st = self.media.file_state(descriptor); it.meta = match (st.dir, st.found, st.size) { (true, true, _) => "folder".into(), (_, true, Some(n)) => size_text(n), _ => "(unavailable)".into() }; }
            What::Pdf => { let pages = rec.get("pages").and_then(Value::as_u64).unwrap_or(1).max(1); it.meta = format!("pdf \u{b7} {pages} page{}", if pages == 1 { "" } else { "s" }); }
            What::Video => { let (fps, ms) = (rec.get("fps").and_then(Value::as_f64).unwrap_or(0.0), rec.get("durMs").and_then(Value::as_f64).unwrap_or(0.0));
                let mut m = vec![]; if rw > 0.0 && rh > 0.0 { m.push(format!("{rw:.0} \u{d7} {rh:.0}")); } if fps > 0.0 { m.push(format!("{} fps", ((fps * 1000.0).round() / 1000.0))); } if ms > 0.0 { m.push(crate::video::length(ms)); } it.meta = m.join(" \u{b7} "); }
            What::Picture => {}
        }
        let Some(path) = there else { if what != What::File { it.problem = Some("The file was not found".into()); } return it };
        if mode == Mode::Brief { return it }
        // a page's drawings, in a sheet as wide as the record's own pixels
        let page_ink = |page: usize, w: u32, h: u32| -> Option<Ink> { let size = (if rw > 0.0 { rw } else { w as f64 }, if rw > 0.0 { rw } else { w as f64 } * h as f64 / (w as f64).max(1.0));
            let lines = crate::ink::page_lines(&rec, page, size); (!lines.is_empty()).then(|| Ink { w: size.0, h: size.1, paths: crate::ink::svg(&lines) }) };
        match what {
            What::File => {}
            What::Picture => {
                let ext = path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
                let plain = match ext.as_str() { "png" => Some(("image/png", "png")), "jpg" | "jpeg" => Some(("image/jpeg", "jpg")), "gif" => Some(("image/gif", "gif")), "webp" => Some(("image/webp", "webp")), "svg" => Some(("image/svg+xml", "svg")), _ => None };
                let has_ink = rec.get("ink").and_then(|i| i.get("0")).and_then(|e| e.get("shapes")).and_then(Value::as_array).is_some_and(|a| !a.is_empty());
                let small = std::fs::metadata(path).map(|m| m.len() <= WHOLE).unwrap_or(false);
                // the file's own bytes when a browser shows them and nothing has to be drawn into them
                if let (Some((mime, ext)), true, false) = (plain.filter(|(_, e)| mode != Mode::Paper || *e == "png"), small, matches!(mode, Mode::Baked | Mode::Paper) && has_ink) { if let Ok(bytes) = std::fs::read(path) {
                    let (w, h) = (rw.max(1.0) as u32, rh.max(1.0) as u32);
                    it.shots.push(Shot { pic: Some(Pic { bytes, mime, ext, w, h }), ink: if mode == Mode::Layers { page_ink(0, w, h) } else { None }, head: it.name.clone(), ..Default::default() }); return it } }
                match crate::media::pixels(path, LONGEST) { Ok((rgba, w, h)) => { let mut s = self.shot(rgba, w, h, page_ink(0, w, h), mode); s.head = it.name.clone(); it.shots.push(s); } Err(e) => it.problem = Some(e) }
            }
            What::Pdf => {
                let pages = rec.get("pages").and_then(Value::as_u64).unwrap_or(1).max(1) as usize; let cur = crate::ink::page_no(&rec);
                let mut list = vec![cur]; if let Some(ink) = rec.get("ink").and_then(Value::as_object) { let mut more: Vec<usize> = ink.iter().filter(|(_, e)| e.get("shapes").and_then(Value::as_array).is_some_and(|a| !a.is_empty())).filter_map(|(k, _)| k.parse().ok()).filter(|p| *p != cur).collect(); more.sort(); list.extend(more); }
                for p in list { match crate::pdf::page(path, p, STILL) { Ok((rgba, w, h)) => { let mut s = self.shot(rgba, w, h, page_ink(p, w, h), mode); s.head = it.name.clone(); s.sub = format!("page {} of {pages}", p + 1); it.shots.push(s); } Err(e) => if p == cur { it.problem = Some(e) } } }
            }
            What::Video => {
                let dev = self.gpu.clone(); let frame = rec.get("frame").and_then(Value::as_i64).unwrap_or(0) as i32; let fps = rec.get("fps").and_then(Value::as_f64).unwrap_or(24.0);
                let still = |me: &mut Self, f: i32| crate::video::still(path, f, STILL).map(|(px, meta)| me.poster.convert(dev.as_ref().map(|d| (&d.0, &d.1)), &px, &meta));
                match still(self, frame) { Ok((rgba, w, h)) => { let mut s = self.shot(rgba, w, h, None, mode); s.head = it.name.clone(); s.sub = crate::video::timecode(frame as i64, fps); it.shots.push(s); } Err(e) => it.problem = Some(e) }
                if opts.video_notes { let notes = self.media.notes(descriptor);
                    for n in notes.iter() {
                        let strokes = qcv_notes::strokes::parse(n.annotation.as_ref());
                        let ink = (!strokes.is_empty() && rw > 0.0 && rh > 0.0).then(|| Ink { w: rw, h: rh, paths: crate::ink::svg_strokes(&strokes, rw, rh) });
                        let mut s = match still(self, n.frame as i32) { Ok((rgba, w, h)) => self.shot(rgba, w, h, ink, mode), Err(_) => Shot::default() };
                        s.head = n.timecode.clone(); s.sub = format!("frame {}", n.frame); s.text = n.text.clone(); s.done = Some(n.addressed); it.notes.push(s);
                    } }
            }
        }
        it
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mn_doc::{Doc, Spans};
    struct Fake;
    impl Source for Fake { fn item(&mut self, d: &str, mode: Mode, _: Opts) -> Item {
        let pic = (mode != Mode::Brief).then(|| Pic { bytes: vec![1, 2, 3], mime: "image/png", ext: "png", w: 4, h: 2 });
        let ink = (mode == Mode::Layers).then(|| Ink { w: 4.0, h: 2.0, paths: "<path d=\"M0 0L4 2\"/>".into() });
        let what = if d.contains("video") { What::Video } else if d.contains("pdf") { What::Pdf } else { What::Picture };
        Item { what, name: "shot one.png".into(), path: "/vol/shot one.png".into(), meta: if what == What::Picture { String::new() } else { "facts".into() }, dw: None, problem: None, shots: vec![Shot { pic, ink, head: "shot one.png".into(), ..Default::default() }],
            notes: if what == What::Video { vec![Shot { head: "00:00:01:00".into(), sub: "frame 24".into(), text: "fix <this>".into(), done: Some(true), ..Default::default() }] } else { vec![] } } } }
    fn doc(list: Vec<(Kind, &str, Vec<(usize, usize, Mark)>)>) -> Doc {
        let mut d = Doc::new();
        for (i, (k, t, marks)) in list.into_iter().enumerate() { let id = BlockId::new(); d.insert(None, i, id, k, t).unwrap(); if !marks.is_empty() { let mut s = Spans::new(); for (a, b, m) in marks { s.put(a, b, m); } d.set_spans(id, s).unwrap(); } }
        d
    }
    fn md(d: &Doc) -> String { markdown(d.walk(), d.children(None), "", Opts::default(), &mut Fake, None) }
    #[test]
    fn markdown_keeps_block_kinds_and_shares_markers_between_neighbours() {
        let d = doc(vec![
            (Kind::Heading { level: 2 }, "Plan", vec![]),
            (Kind::Paragraph, "one two three", vec![(0, 7, Mark::Bold), (4, 13, Mark::Italic)]),
            (Kind::Paragraph, "# not a heading", vec![]), (Kind::Paragraph, "1. not a list", vec![]), (Kind::Paragraph, "a*b_c [x]", vec![]),
            (Kind::Bullet { depth: 0 }, "top", vec![]), (Kind::Bullet { depth: 1 }, "under", vec![]), (Kind::Numbered { depth: 0 }, "first", vec![]), (Kind::Numbered { depth: 0 }, "second", vec![]),
            (Kind::Task { depth: 0, state: 1 }, "doing", vec![]), (Kind::Task { depth: 0, state: 2 }, "done", vec![]),
            (Kind::Code { lang: "rust".into() }, "let a = `x`;\n```", vec![]), (Kind::Quote, "said\nso", vec![]), (Kind::Divider, "", vec![]),
            (Kind::Paragraph, "see here now", vec![(4, 8, Mark::Link("https://example.org/a b".into()))]),
        ]);
        let m = md(&d);
        assert!(m.starts_with("## Plan\n\n**one *two*** *three*\n\n\\# not a heading\n\n1\\. not a list\n\na\\*b\\_c \\[x\\]\n\n"), "{m}");
        assert!(m.contains("- top\n  - under\n1. first\n2. second\n- [/] doing\n- [x] done\n\n````rust\nlet a = `x`;\n```\n````\n\n> said  \n> so\n\n---\n\nsee [here](<https://example.org/a b>) now\n"), "{m}");
    }
    #[test]
    fn html_nests_lists_escapes_and_layers_the_drawings() {
        let d = doc(vec![
            (Kind::Paragraph, "a <b> & c", vec![(0, 1, Mark::Highlight("yellow".into())), (2, 5, Mark::Color("#FF0000".into()))]),
            (Kind::Bullet { depth: 0 }, "top", vec![]), (Kind::Bullet { depth: 1 }, "under", vec![]), (Kind::Task { depth: 0, state: 2 }, "done", vec![]),
            (Kind::Media { descriptor: "{\"src\":\"x\"}".into() }, "", vec![]), (Kind::Media { descriptor: "{\"kind\":\"video\"}".into() }, "", vec![]),
        ]);
        let h = html(d.walk(), d.children(None), "My <note>", "today", Opts { paths: false, video_notes: true }, &vec![], &mut Fake);
        assert!(h.contains("<title>My &lt;note&gt;</title>") && h.contains("<mark style=\"background:rgba(255,236,89,.3)\">a</mark> <span style=\"color:#FF0000\">&lt;b&gt;</span> &amp; c"), "{h}");
        assert!(h.contains("<ul><li>top<ul><li>under</li></ul>\n</li>\n<li class=\"task s2\"><span class=\"box\"></span>done</li></ul>"), "{h}");
        assert!(h.contains("<figure class=\"pic\"><div class=\"thumb wide\"") && h.contains("data:image/png;base64,AQID") && h.contains("<svg class=\"ink\" viewBox=\"0 0 4 2\""));
        assert!(h.contains("<figure class=\"ref video\">") && h.contains("fix &lt;this&gt;") && h.contains("Addressed") && !h.contains("/vol/shot one.png"), "paths are left out when asked");
        let with = html(d.walk(), d.children(None), "n", "today", Opts::default(), &vec![], &mut Fake); assert!(with.contains("<span class=\"path\">/vol/shot one.png</span>"));
    }
    #[test]
    fn markdown_writes_pictures_through_the_sink_and_tables_as_pipes() {
        let mut d = doc(vec![(Kind::Media { descriptor: "{\"src\":\"x\"}".into() }, "", vec![]), (Kind::Media { descriptor: "{\"kind\":\"pdf\"}".into() }, "", vec![])]);
        let t = BlockId::new(); d.insert(None, 2, t, Kind::Table { header_rows: 1, columns: vec![ColumnSpec::default(), ColumnSpec { align: 2, kind: ColumnKind::Check, ..Default::default() }] }, "").unwrap();
        for (r, cells) in [["Name", "Done"], ["a|b", ""]].iter().enumerate() { let row = BlockId::new(); d.insert(Some(t), r, row, Kind::row(), "").unwrap();
            for (c, text) in cells.iter().enumerate() { let cell = BlockId::new(); d.insert(Some(row), c, cell, Kind::cell(), "").unwrap(); d.insert(Some(cell), 0, BlockId::new(), Kind::Paragraph, *text).unwrap(); } }
        let mut seen = vec![]; let mut sink = |name: &str, p: &Pic| { seen.push((name.to_string(), p.bytes.len())); Some(format!("n.assets/{name}")) };
        let m = markdown(d.walk(), d.children(None), "My `note`", Opts::default(), &mut Fake, Some(&mut sink));
        assert!(m.starts_with("`My note`\n\n![shot one.png](<n.assets/shot one.png>)\n\n![shot one.png](<n.assets/shot one>)\n\n```pdf\nshot one.png\n/vol/shot one.png\nfacts\n```\n\n| Name | Done |\n| --- | ---: |\n| a\\|b | [ ] |\n"), "{m}");
        assert_eq!(seen.len(), 2);
        // nothing written: the picture is named where it is
        assert!(md(&d).starts_with("![shot one.png](<file:///vol/shot one.png>)"));
    }
}
