//! A note as a Word document (canon row 329; the simple one). A zip of OOXML written by hand, as QCView's notes export
//! is (`qcv_notes::export_doc`): Arial and Courier New - on every Mac and Windows machine; Word embeds no font to rely
//! on - with REAL styles (Heading 1-6 with outline levels, so Word's navigation and its table of contents see them; the
//! Qt app wrote bold text at a size), a numbering of its own for every numbered list (the Qt app shared one, so a
//! second list went on counting), task boxes as text, links as real hyperlinks. Drawings are drawn into the pictures.
use super::{chip_of, col_of, hex_of, pieces, Col, Item, Mode, Opts, Shot, Source, Walk, What};
use mn_doc::{Block, BlockId, ColumnSpec, Kind, Mark};
use std::io::{Cursor, Write};

const EMU_PER_PX: i64 = 9525;
/// The text column: Letter with 0.75 in margins, in twips and in EMU.
const PAGE_TW: i64 = 12240 - 2 * 1080;
const PAGE_EMU: i64 = PAGE_TW * 635;

fn xesc(s: &str) -> String { s.chars().filter(|c| !c.is_control() || matches!(c, '\n' | '\t')).collect::<String>().replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;") }
/// `#RRGGBB` (or a named colour) as Word's six digits.
fn hex6(c: &str) -> Option<String> { let h = hex_of(c); let h = h.trim_start_matches('#'); (h.len() >= 6 && h[..6].chars().all(|c| c.is_ascii_hexdigit())).then(|| h[..6].to_uppercase()) }
/// A colour laid over white paper at a share: what a highlight, a chip or a filled cell prints as.
fn tint(c: &str, share: f64) -> Option<String> { let h = hex6(c)?; let n = |i: usize| u8::from_str_radix(&h[i..i + 2], 16).unwrap_or(0) as f64; let m = |v: f64| (255.0 + (v - 255.0) * share).round() as u8; Some(format!("{:02X}{:02X}{:02X}", m(n(0)), m(n(2)), m(n(4)))) }

#[derive(Default)]
struct Out { body: String, rels: String, media: Vec<(String, Vec<u8>)>, links: usize, pics: usize, /// numbered lists met so far (each has a numbering of its own)
    lists: usize, exts: Vec<&'static str> }
impl Out {
    fn link(&mut self, url: &str) -> String { self.links += 1; let id = format!("rIdLink{}", self.links);
        self.rels.push_str(&format!("<Relationship Id=\"{id}\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink\" Target=\"{}\" TargetMode=\"External\"/>", xesc(url))); id }
    /// A picture as a run, no wider than `max` (EMU).
    fn picture(&mut self, s: &Shot, max: i64) -> Option<String> {
        let p = s.pic.as_ref()?; if p.w == 0 || p.h == 0 || !matches!(p.ext, "png" | "jpg" | "gif") { return None }
        self.pics += 1; let id = self.pics; let name = format!("image{id}.{}", p.ext); let rel = format!("rIdImg{id}"); if !self.exts.contains(&p.ext) { self.exts.push(p.ext); }
        self.rels.push_str(&format!("<Relationship Id=\"{rel}\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/image\" Target=\"media/{name}\"/>"));
        self.media.push((name, p.bytes.clone()));
        let (mut cx, mut cy) = (p.w as i64 * EMU_PER_PX, p.h as i64 * EMU_PER_PX); if cx > max { cy = cy * max / cx; cx = max; }
        // a tall picture stays on one page
        let tall = 8 * 914400; if cy > tall { cx = cx * tall / cy; cy = tall; }
        Some(format!("<w:r><w:drawing><wp:inline distT=\"0\" distB=\"0\" distL=\"0\" distR=\"0\"><wp:extent cx=\"{cx}\" cy=\"{cy}\"/><wp:docPr id=\"{id}\" name=\"Picture {id}\"/>\
<a:graphic xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\"><a:graphicData uri=\"http://schemas.openxmlformats.org/drawingml/2006/picture\">\
<pic:pic xmlns:pic=\"http://schemas.openxmlformats.org/drawingml/2006/picture\"><pic:nvPicPr><pic:cNvPr id=\"{id}\" name=\"image{id}\"/><pic:cNvPicPr/></pic:nvPicPr>\
<pic:blipFill><a:blip r:embed=\"{rel}\"/><a:stretch><a:fillRect/></a:stretch></pic:blipFill><pic:spPr><a:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"{cx}\" cy=\"{cy}\"/></a:xfrm>\
<a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom></pic:spPr></pic:pic></a:graphicData></a:graphic></wp:inline></w:drawing></w:r>"))
    }
}
/// Text as a run: line breaks and tabs as Word's own.
fn text_run(text: &str, rpr: &str) -> String {
    let mut o = format!("<w:r>{}", if rpr.is_empty() { String::new() } else { format!("<w:rPr>{rpr}</w:rPr>") });
    for (i, line) in text.split('\n').enumerate() { if i > 0 { o += "<w:br/>"; }
        for (j, part) in line.split('\t').enumerate() { if j > 0 { o += "<w:tab/>"; } if !part.is_empty() { o += &format!("<w:t xml:space=\"preserve\">{}</w:t>", xesc(part)); } } }
    o + "</w:r>"
}
const MONO: &str = "<w:rFonts w:ascii=\"Courier New\" w:hAnsi=\"Courier New\" w:cs=\"Courier New\"/>";
/// A block's text as runs: every run with its whole formatting (Word has no nesting); a link is a real hyperlink.
fn inline(b: &Block, col: &Col, out: &mut Out) -> String {
    let mut o = String::new();
    for (text, marks) in pieces(b) {
        let (mut rpr, mut link, mut shade) = (String::new(), None, None);
        // (Word's schema wants a run's properties in its own order: fonts, bold, italic, strike, colour, size, underline, shading)
        if marks.iter().any(|m| matches!(m, Mark::Code)) { rpr += MONO; shade = Some("EFEFEF".to_string()); }
        if marks.iter().any(|m| matches!(m, Mark::Bold)) { rpr += "<w:b/>"; } if marks.iter().any(|m| matches!(m, Mark::Italic)) { rpr += "<w:i/>"; } if marks.iter().any(|m| matches!(m, Mark::Strike)) { rpr += "<w:strike/>"; }
        for m in &marks { match m { Mark::Link(u) => link = Some(u.clone()), Mark::Highlight(v) => shade = tint(v, 0.45).or(Some("FFF2A8".into())),
            Mark::Choice(p) => { let (id, own) = chip_of(p); let c = col.options.iter().find(|(i, _)| *i == id).map(|(_, c)| c.clone()).unwrap_or(own); shade = tint(&c, 0.28).or(Some("EFEFEF".into())); } _ => {} } }
        match (marks.iter().find_map(|m| match m { Mark::Color(v) => hex6(v), _ => None }), &link) { (Some(c), _) => rpr += &format!("<w:color w:val=\"{c}\"/>"), (None, Some(_)) => rpr += "<w:color w:val=\"0563C1\"/>", _ => {} }
        if marks.iter().any(|m| matches!(m, Mark::Code)) { rpr += "<w:sz w:val=\"19\"/><w:szCs w:val=\"19\"/>"; }
        if link.is_some() || marks.iter().any(|m| matches!(m, Mark::Underline)) { rpr += "<w:u w:val=\"single\"/>"; }
        if let Some(s) = shade { rpr += &format!("<w:shd w:val=\"clear\" w:color=\"auto\" w:fill=\"{s}\"/>"); }
        let run = text_run(text, &rpr);
        match link { Some(u) => { let id = out.link(&u); o += &format!("<w:hyperlink r:id=\"{id}\" w:history=\"1\">{run}</w:hyperlink>"); } None => o += &run }
    }
    o
}
fn para(ppr: &str, runs: &str) -> String { format!("<w:p>{}{runs}</w:p>", if ppr.is_empty() { String::new() } else { format!("<w:pPr>{ppr}</w:pPr>") }) }
fn styled(style: &str, text: &str) -> String { para(&format!("<w:pStyle w:val=\"{style}\"/>"), &text_run(text, "")) }

fn media(it: &Item, opts: Opts, max: i64, out: &mut Out) {
    let first = it.shots.first();
    if let Some(r) = first.and_then(|s| out.picture(s, match it.dw { Some(w) if it.what == What::Picture => max.min(w.round() as i64 * EMU_PER_PX), _ => max })) { out.body += &para("<w:pStyle w:val=\"Figure\"/>", &r); }
    if it.what != What::Picture || it.problem.is_some() || first.is_none_or(|s| s.pic.is_none()) {
        out.body += &styled("RefName", &it.name);
        if opts.paths && !it.path.is_empty() { out.body += &styled("RefLine", &it.path); }
        if !it.meta.is_empty() { out.body += &styled("RefLine", &it.meta); }
        if let Some(p) = &it.problem { out.body += &styled("RefLine", p); }
    }
    for s in it.shots.iter().skip(1) { if let Some(r) = out.picture(s, max) { out.body += &para("<w:pStyle w:val=\"Figure\"/>", &r); out.body += &styled("RefLine", &format!("{} \u{2014} {}", s.head, s.sub)); } }
    for s in &it.notes {
        out.body += &para("<w:pStyle w:val=\"NoteHead\"/>", &format!("{}{}", text_run(&s.head, &format!("{MONO}<w:b/>")), text_run(&format!("   {}{}", s.sub, if s.done == Some(true) { "   \u{2713} Addressed" } else { "" }), MONO)));
        if let Some(r) = out.picture(s, max.min(PAGE_EMU * 2 / 3)) { out.body += &para("<w:pStyle w:val=\"Figure\"/>", &r); }
        if !s.text.trim().is_empty() { out.body += &para("", &text_run(s.text.trim_end(), "")); }
    }
}
/// A table of the note, `width` twips across.
fn table(w: &Walk, t: &Block, header_rows: usize, columns: &[ColumnSpec], width: i64, src: &mut dyn Source, out: &mut Out) {
    let n = t.children.iter().map(|r| w.kids(*r).len()).max().unwrap_or(0).max(columns.len()); if n == 0 { return }
    // the columns' own widths in proportion (a column without one takes 160 px), scaled to the space there is
    let px: Vec<f64> = (0..n).map(|i| columns.get(i).and_then(|c| c.width).map(|w| w as f64).unwrap_or(160.0).max(24.0)).collect(); let total: f64 = px.iter().sum();
    let k = (width as f64 / (total * 15.0)).min(1.0); let tw: Vec<i64> = px.iter().map(|p| (p * 15.0 * k).round() as i64).collect();
    out.body += &format!("<w:tbl><w:tblPr><w:tblW w:w=\"{}\" w:type=\"dxa\"/><w:tblBorders>{}</w:tblBorders><w:tblLayout w:type=\"fixed\"/><w:tblCellMar><w:top w:w=\"60\" w:type=\"dxa\"/><w:left w:w=\"100\" w:type=\"dxa\"/><w:bottom w:w=\"60\" w:type=\"dxa\"/><w:right w:w=\"100\" w:type=\"dxa\"/></w:tblCellMar></w:tblPr><w:tblGrid>{}</w:tblGrid>",
        tw.iter().sum::<i64>(), ["top", "left", "bottom", "right", "insideH", "insideV"].iter().map(|s| format!("<w:{s} w:val=\"single\" w:sz=\"4\" w:space=\"0\" w:color=\"C6C6C6\"/>")).collect::<String>(), tw.iter().map(|x| format!("<w:gridCol w:w=\"{x}\"/>")).collect::<String>());
    for (ri, r) in t.children.iter().enumerate() {
        let header = ri < header_rows; let (rbg, rfg) = match w.all.get(r).map(|r| &r.kind) { Some(Kind::Row { bg, fg }) => (bg.as_str(), fg.as_str()), _ => ("", "") };
        out.body += &format!("<w:tr><w:trPr><w:cantSplit/>{}</w:trPr>", if header { "<w:tblHeader/>" } else { "" }); let cells = w.kids(*r);
        for i in 0..n { let spec = columns.get(i);
            let (cbg, cfg) = match cells.get(i).and_then(|c| w.all.get(c)).map(|c| &c.kind) { Some(Kind::Cell { bg, fg }) => (bg.as_str(), fg.as_str()), _ => ("", "") };
            let pick = |a: &str, b: &str, c: &str| if !a.is_empty() { a.to_string() } else if !b.is_empty() { b.to_string() } else { c.to_string() };
            let (bg, fg) = (pick(cbg, rbg, spec.map(|s| s.bg.as_str()).unwrap_or("")), pick(cfg, rfg, spec.map(|s| s.fg.as_str()).unwrap_or("")));
            let shade = tint(&bg, 0.3).or(header.then(|| "F4F4F4".to_string()));
            out.body += &format!("<w:tc><w:tcPr><w:tcW w:w=\"{}\" w:type=\"dxa\"/>{}</w:tcPr>", tw[i], shade.map(|s| format!("<w:shd w:val=\"clear\" w:color=\"auto\" w:fill=\"{s}\"/>")).unwrap_or_default());
            let before = out.body.len();
            let cell = Ctx { style: if header { "CellHead" } else { "Cell" }, align: spec.map(|s| s.align).unwrap_or(0), color: hex6(&fg), width: tw[i] - 200 };
            if let Some(c) = cells.get(i) { blocks(w, w.kids(*c), &col_of(spec, header), &cell, src, out); }
            // a cell must end with a paragraph
            if out.body.len() == before || out.body.ends_with("</w:tbl>") { out.body += &styled("Cell", ""); }
            out.body += "</w:tc>"; }
        out.body += "</w:tr>";
    }
    out.body += "</w:tbl>";
}
/// Where blocks are being written: the paragraph style plain text takes there, its alignment and colour, the room across (twips).
struct Ctx { style: &'static str, align: u8, color: Option<String>, width: i64 }
fn blocks(w: &Walk, ids: &[BlockId], col: &Col, cx: &Ctx, src: &mut dyn Source, out: &mut Out) {
    let mut in_list = false; let mut num = 0;
    let base = |cx: &Ctx| { let mut p = String::new(); if !cx.style.is_empty() { p += &format!("<w:pStyle w:val=\"{}\"/>", cx.style); } p };
    let tail = |cx: &Ctx| match cx.align { 1 => "<w:jc w:val=\"center\"/>", 2 => "<w:jc w:val=\"right\"/>", _ => "" };
    // a cell's text colour rides on every run of the cell that has none of its own
    let tinted = |runs: String, cx: &Ctx| match &cx.color { Some(c) => runs.replace("<w:r><w:t", &format!("<w:r><w:rPr><w:color w:val=\"{c}\"/></w:rPr><w:t")), None => runs };
    for id in ids { let Some(b) = w.all.get(id) else { continue };
        let listed = matches!(b.kind, Kind::Bullet { .. } | Kind::Numbered { .. } | Kind::Task { .. });
        if let Kind::Numbered { .. } = b.kind { if !in_list || num == 0 { out.lists += 1; num = out.lists + 1; } } if !listed { num = 0; } in_list = listed;
        let runs = if b.kind.is_text() && !matches!(b.kind, Kind::Code { .. }) { inline(b, col, out) } else { String::new() };
        match &b.kind {
            Kind::Paragraph if col.check => { let st = match mn_edit::typed::chosen(&b.text, &b.spans).as_deref() { Some("1") => "\u{25EA}", Some("2") => "\u{2611}", _ => "\u{2610}" }; out.body += &para(&format!("{}{}", base(cx), tail(cx)), &text_run(st, "")); }
            Kind::Paragraph => out.body += &para(&format!("{}{}", base(cx), tail(cx)), &tinted(if col.mono { runs.clone().replace("<w:r><w:t", &format!("<w:r><w:rPr>{MONO}</w:rPr><w:t")) } else { runs.clone() }, cx)),
            Kind::Heading { level } => out.body += &para(&format!("<w:pStyle w:val=\"Heading{}\"/>", (*level).clamp(1, 6)), &runs.clone()),
            Kind::Quote => out.body += &para("<w:pStyle w:val=\"Quote\"/>", &runs.clone()),
            Kind::Code { lang } => { if !lang.is_empty() { out.body += &styled("CodeLang", lang); } out.body += &para("<w:pStyle w:val=\"Code\"/>", &mn_layout::syntax::pieces(lang, &b.text).into_iter().map(|(t, k)| text_run(t, &k.map(|k| format!("<w:color w:val=\"{}\"/>{}", &mn_layout::syntax::LIGHT[k.index()][1..], if k == mn_layout::syntax::Tok::Comment { "<w:i/>" } else { "" })).unwrap_or_default())).collect::<String>()); }
            Kind::Bullet { depth } => out.body += &para(&format!("<w:pStyle w:val=\"ListParagraph\"/><w:numPr><w:ilvl w:val=\"{}\"/><w:numId w:val=\"1\"/></w:numPr>", (*depth).min(8)), &tinted(runs.clone(), cx)),
            Kind::Numbered { depth } => out.body += &para(&format!("<w:pStyle w:val=\"ListParagraph\"/><w:numPr><w:ilvl w:val=\"{}\"/><w:numId w:val=\"{num}\"/></w:numPr>", (*depth).min(8)), &tinted(runs.clone(), cx)),
            Kind::Task { depth, state } => { let left = 360 * (*depth as i64 + 1);
                out.body += &para(&format!("<w:pStyle w:val=\"ListParagraph\"/><w:ind w:left=\"{left}\" w:hanging=\"360\"/>"), &format!("{}{}", text_run(&format!("{}\t", ["\u{2610}", "\u{25EA}", "\u{2611}"][(*state).min(2) as usize]), ""), tinted(runs.clone(), cx))) }
            Kind::Divider => out.body += "<w:p><w:pPr><w:pBdr><w:bottom w:val=\"single\" w:sz=\"6\" w:space=\"1\" w:color=\"C6C6C6\"/></w:pBdr></w:pPr></w:p>",
            Kind::Media { descriptor } => media(&src.item(descriptor, Mode::Paper, w.opts), w.opts, cx.width * 635, out),
            Kind::Columns => { let shares: Vec<f64> = b.children.iter().map(|c| match w.all.get(c).map(|c| &c.kind) { Some(Kind::Column { width }) if *width > 0.0 => *width as f64, _ => 1.0 / b.children.len().max(1) as f64 }).collect(); let sum: f64 = shares.iter().sum::<f64>().max(1e-6);
                let gap = 180; let tw: Vec<i64> = shares.iter().map(|s| (cx.width as f64 * s / sum).round() as i64).collect();
                out.body += &format!("<w:tbl><w:tblPr><w:tblW w:w=\"{}\" w:type=\"dxa\"/><w:tblLayout w:type=\"fixed\"/><w:tblCellMar><w:left w:w=\"0\" w:type=\"dxa\"/><w:right w:w=\"{gap}\" w:type=\"dxa\"/></w:tblCellMar></w:tblPr><w:tblGrid>{}</w:tblGrid><w:tr>", cx.width, tw.iter().map(|x| format!("<w:gridCol w:w=\"{x}\"/>")).collect::<String>());
                for (i, c) in b.children.iter().enumerate() { out.body += &format!("<w:tc><w:tcPr><w:tcW w:w=\"{}\" w:type=\"dxa\"/></w:tcPr>", tw[i]); let before = out.body.len();
                    blocks(w, w.kids(*c), col, &Ctx { style: cx.style, align: 0, color: cx.color.clone(), width: tw[i] - gap }, src, out);
                    if out.body.len() == before || out.body.ends_with("</w:tbl>") { out.body += &para("", ""); } out.body += "</w:tc>"; }
                out.body += "</w:tr></w:tbl>"; out.body += &para("<w:spacing w:after=\"0\" w:line=\"120\" w:lineRule=\"exact\"/>", ""); }
            Kind::Column { .. } => blocks(w, &b.children, col, cx, src, out),
            Kind::Table { header_rows, columns } => { table(w, b, *header_rows as usize, columns, cx.width, src, out); out.body += &para("<w:spacing w:after=\"0\" w:line=\"120\" w:lineRule=\"exact\"/>", ""); }
            _ => {}
        }
    }
}

const STYLES: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:styles xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
<w:docDefaults><w:rPrDefault><w:rPr><w:rFonts w:ascii="Arial" w:hAnsi="Arial" w:cs="Arial" w:eastAsia="Arial"/><w:sz w:val="21"/><w:szCs w:val="21"/><w:color w:val="222222"/></w:rPr></w:rPrDefault>
<w:pPrDefault><w:pPr><w:spacing w:after="120" w:line="288" w:lineRule="auto"/></w:pPr></w:pPrDefault></w:docDefaults>
<w:style w:type="paragraph" w:default="1" w:styleId="Normal"><w:name w:val="Normal"/><w:qFormat/></w:style>
<w:style w:type="paragraph" w:styleId="Heading1"><w:name w:val="heading 1"/><w:basedOn w:val="Normal"/><w:next w:val="Normal"/><w:qFormat/><w:pPr><w:keepNext/><w:spacing w:before="360" w:after="120"/><w:outlineLvl w:val="0"/></w:pPr><w:rPr><w:sz w:val="40"/><w:szCs w:val="40"/><w:color w:val="111111"/></w:rPr></w:style>
<w:style w:type="paragraph" w:styleId="Heading2"><w:name w:val="heading 2"/><w:basedOn w:val="Normal"/><w:next w:val="Normal"/><w:qFormat/><w:pPr><w:keepNext/><w:spacing w:before="320" w:after="100"/><w:outlineLvl w:val="1"/></w:pPr><w:rPr><w:sz w:val="34"/><w:szCs w:val="34"/><w:color w:val="111111"/></w:rPr></w:style>
<w:style w:type="paragraph" w:styleId="Heading3"><w:name w:val="heading 3"/><w:basedOn w:val="Normal"/><w:next w:val="Normal"/><w:qFormat/><w:pPr><w:keepNext/><w:spacing w:before="280" w:after="80"/><w:outlineLvl w:val="2"/></w:pPr><w:rPr><w:sz w:val="29"/><w:szCs w:val="29"/><w:color w:val="111111"/></w:rPr></w:style>
<w:style w:type="paragraph" w:styleId="Heading4"><w:name w:val="heading 4"/><w:basedOn w:val="Normal"/><w:next w:val="Normal"/><w:qFormat/><w:pPr><w:keepNext/><w:spacing w:before="240" w:after="80"/><w:outlineLvl w:val="3"/></w:pPr><w:rPr><w:b/><w:sz w:val="24"/><w:szCs w:val="24"/><w:color w:val="111111"/></w:rPr></w:style>
<w:style w:type="paragraph" w:styleId="Heading5"><w:name w:val="heading 5"/><w:basedOn w:val="Normal"/><w:next w:val="Normal"/><w:qFormat/><w:pPr><w:keepNext/><w:spacing w:before="200" w:after="60"/><w:outlineLvl w:val="4"/></w:pPr><w:rPr><w:b/><w:sz w:val="22"/><w:szCs w:val="22"/><w:color w:val="111111"/></w:rPr></w:style>
<w:style w:type="paragraph" w:styleId="Heading6"><w:name w:val="heading 6"/><w:basedOn w:val="Normal"/><w:next w:val="Normal"/><w:qFormat/><w:pPr><w:keepNext/><w:spacing w:before="200" w:after="60"/><w:outlineLvl w:val="5"/></w:pPr><w:rPr><w:b/><w:sz w:val="20"/><w:szCs w:val="20"/><w:color w:val="464646"/></w:rPr></w:style>
<w:style w:type="paragraph" w:styleId="ListParagraph"><w:name w:val="List Paragraph"/><w:basedOn w:val="Normal"/><w:qFormat/><w:pPr><w:spacing w:after="60"/><w:tabs><w:tab w:val="left" w:pos="360"/></w:tabs></w:pPr></w:style>
<w:style w:type="paragraph" w:styleId="Quote"><w:name w:val="Quote"/><w:basedOn w:val="Normal"/><w:qFormat/><w:pPr><w:pBdr><w:left w:val="single" w:sz="18" w:space="10" w:color="C6C6C6"/></w:pBdr><w:ind w:left="360"/></w:pPr><w:rPr><w:i/><w:color w:val="464646"/></w:rPr></w:style>
<w:style w:type="paragraph" w:customStyle="1" w:styleId="Code"><w:name w:val="Code"/><w:basedOn w:val="Normal"/><w:qFormat/><w:pPr><w:shd w:val="clear" w:color="auto" w:fill="F4F4F4"/><w:spacing w:after="160" w:line="260" w:lineRule="auto"/></w:pPr><w:rPr><w:rFonts w:ascii="Courier New" w:hAnsi="Courier New" w:cs="Courier New"/><w:sz w:val="18"/><w:szCs w:val="18"/></w:rPr></w:style>
<w:style w:type="paragraph" w:customStyle="1" w:styleId="CodeLang"><w:name w:val="Code Language"/><w:basedOn w:val="Normal"/><w:pPr><w:keepNext/><w:spacing w:after="0"/><w:jc w:val="right"/></w:pPr><w:rPr><w:rFonts w:ascii="Courier New" w:hAnsi="Courier New" w:cs="Courier New"/><w:sz w:val="16"/><w:szCs w:val="16"/><w:color w:val="8F8F8F"/></w:rPr></w:style>
<w:style w:type="paragraph" w:customStyle="1" w:styleId="NoteName"><w:name w:val="Note Name"/><w:basedOn w:val="Normal"/><w:pPr><w:pBdr><w:bottom w:val="single" w:sz="4" w:space="6" w:color="C6C6C6"/></w:pBdr><w:spacing w:after="240"/></w:pPr><w:rPr><w:rFonts w:ascii="Courier New" w:hAnsi="Courier New" w:cs="Courier New"/><w:sz w:val="18"/><w:szCs w:val="18"/><w:color w:val="6D6D6D"/></w:rPr></w:style>
<w:style w:type="paragraph" w:customStyle="1" w:styleId="Figure"><w:name w:val="Figure"/><w:basedOn w:val="Normal"/><w:pPr><w:keepNext/><w:spacing w:before="80" w:after="80" w:line="240" w:lineRule="auto"/></w:pPr></w:style>
<w:style w:type="paragraph" w:customStyle="1" w:styleId="RefName"><w:name w:val="Reference Name"/><w:basedOn w:val="Normal"/><w:pPr><w:keepNext/><w:spacing w:after="20"/></w:pPr><w:rPr><w:b/></w:rPr></w:style>
<w:style w:type="paragraph" w:customStyle="1" w:styleId="RefLine"><w:name w:val="Reference Line"/><w:basedOn w:val="Normal"/><w:pPr><w:spacing w:after="20"/></w:pPr><w:rPr><w:rFonts w:ascii="Courier New" w:hAnsi="Courier New" w:cs="Courier New"/><w:sz w:val="17"/><w:szCs w:val="17"/><w:color w:val="6D6D6D"/></w:rPr></w:style>
<w:style w:type="paragraph" w:customStyle="1" w:styleId="NoteHead"><w:name w:val="Video Note"/><w:basedOn w:val="Normal"/><w:pPr><w:keepNext/><w:spacing w:before="200" w:after="60"/></w:pPr><w:rPr><w:sz w:val="19"/><w:szCs w:val="19"/></w:rPr></w:style>
<w:style w:type="paragraph" w:customStyle="1" w:styleId="Cell"><w:name w:val="Table Cell"/><w:basedOn w:val="Normal"/><w:pPr><w:spacing w:after="40" w:line="252" w:lineRule="auto"/></w:pPr><w:rPr><w:sz w:val="18"/><w:szCs w:val="18"/></w:rPr></w:style>
<w:style w:type="paragraph" w:customStyle="1" w:styleId="CellHead"><w:name w:val="Table Heading"/><w:basedOn w:val="Cell"/><w:rPr><w:b/><w:color w:val="111111"/></w:rPr></w:style>
</w:styles>"#;

/// Bullets (numbering 1) and, for each numbered list, a numbering of its own that starts at 1.
fn numbering(lists: usize) -> String {
    let mut o = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<w:numbering xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\"><w:abstractNum w:abstractNumId=\"0\"><w:multiLevelType w:val=\"hybridMultilevel\"/>");
    for l in 0..9 { o += &format!("<w:lvl w:ilvl=\"{l}\"><w:start w:val=\"1\"/><w:numFmt w:val=\"bullet\"/><w:lvlText w:val=\"{}\"/><w:lvlJc w:val=\"left\"/><w:pPr><w:ind w:left=\"{}\" w:hanging=\"360\"/></w:pPr><w:rPr><w:rFonts w:ascii=\"Arial\" w:hAnsi=\"Arial\" w:cs=\"Arial\"/></w:rPr></w:lvl>", ["\u{2022}", "\u{25E6}", "\u{25AA}"][l % 3], 360 * (l + 1)); }
    o += "</w:abstractNum><w:abstractNum w:abstractNumId=\"1\"><w:multiLevelType w:val=\"multilevel\"/>";
    for l in 0..9 { o += &format!("<w:lvl w:ilvl=\"{l}\"><w:start w:val=\"1\"/><w:numFmt w:val=\"{}\"/><w:lvlText w:val=\"%{}.\"/><w:lvlJc w:val=\"left\"/><w:pPr><w:ind w:left=\"{}\" w:hanging=\"360\"/></w:pPr></w:lvl>", ["decimal", "lowerLetter", "lowerRoman"][l % 3], l + 1, 360 * (l + 1)); }
    o += "</w:abstractNum><w:num w:numId=\"1\"><w:abstractNumId w:val=\"0\"/></w:num>";
    for i in 0..lists { o += &format!("<w:num w:numId=\"{}\"><w:abstractNumId w:val=\"1\"/>{}</w:num>", i + 2, (0..9).map(|l| format!("<w:lvlOverride w:ilvl=\"{l}\"><w:startOverride w:val=\"1\"/></w:lvlOverride>")).collect::<String>()); }
    o + "</w:numbering>"
}

/// The note as a `.docx`.
pub fn docx<'a>(blocks_in: impl IntoIterator<Item = &'a Block>, tops: &[BlockId], title: &str, opts: Opts, src: &mut dyn Source) -> Result<Vec<u8>, String> {
    let w = Walk::new(blocks_in, opts); let mut out = Out::default();
    if !title.is_empty() { out.body += &styled("NoteName", title); }
    blocks(&w, tops, &Col::default(), &Ctx { style: "", align: 0, color: None, width: PAGE_TW }, src, &mut out);
    let document = format!("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<w:document xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\" xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\" xmlns:wp=\"http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing\"><w:body>{}\
<w:sectPr><w:pgSz w:w=\"12240\" w:h=\"15840\"/><w:pgMar w:top=\"1080\" w:right=\"1080\" w:bottom=\"1080\" w:left=\"1080\" w:header=\"720\" w:footer=\"720\" w:gutter=\"0\"/></w:sectPr></w:body></w:document>", out.body);
    let doc_rels = format!("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rIdStyles\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles\" Target=\"styles.xml\"/><Relationship Id=\"rIdNumbering\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/numbering\" Target=\"numbering.xml\"/>{}</Relationships>", out.rels);
    let types = format!("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\"><Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/><Default Extension=\"xml\" ContentType=\"application/xml\"/>{}\
<Override PartName=\"/word/document.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml\"/><Override PartName=\"/word/styles.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml\"/><Override PartName=\"/word/numbering.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.numbering+xml\"/><Override PartName=\"/docProps/core.xml\" ContentType=\"application/vnd.openxmlformats-package.core-properties+xml\"/></Types>",
        out.exts.iter().map(|e| format!("<Default Extension=\"{e}\" ContentType=\"{}\"/>", match *e { "png" => "image/png", "gif" => "image/gif", _ => "image/jpeg" })).collect::<String>());
    let pkg_rels = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument\" Target=\"word/document.xml\"/><Relationship Id=\"rId2\" Type=\"http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties\" Target=\"docProps/core.xml\"/></Relationships>";
    let core = format!("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<cp:coreProperties xmlns:cp=\"http://schemas.openxmlformats.org/package/2006/metadata/core-properties\" xmlns:dc=\"http://purl.org/dc/elements/1.1/\"><dc:title>{}</dc:title><dc:creator>minNotes</dc:creator></cp:coreProperties>", xesc(title));
    let mut zw = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let opt = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    let stored = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    let mut put = |name: &str, bytes: &[u8], o: zip::write::SimpleFileOptions| -> Result<(), String> { zw.start_file(name, o).map_err(|e| e.to_string())?; zw.write_all(bytes).map_err(|e| e.to_string()) };
    put("[Content_Types].xml", types.as_bytes(), opt)?;
    put("_rels/.rels", pkg_rels.as_bytes(), opt)?;
    put("docProps/core.xml", core.as_bytes(), opt)?;
    put("word/document.xml", document.as_bytes(), opt)?;
    put("word/styles.xml", STYLES.as_bytes(), opt)?;
    put("word/numbering.xml", numbering(out.lists).as_bytes(), opt)?;
    put("word/_rels/document.xml.rels", doc_rels.as_bytes(), opt)?;
    for (name, bytes) in &out.media { put(&format!("word/media/{name}"), bytes, stored)?; }
    Ok(zw.finish().map_err(|e| e.to_string())?.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;
    use mn_doc::Doc;
    struct Fake;
    impl Source for Fake { fn item(&mut self, _: &str, mode: Mode, _: Opts) -> Item { assert_eq!(mode, Mode::Paper);
        Item { what: What::Pdf, name: "plan.pdf".into(), path: "/vol/plan.pdf".into(), meta: "pdf \u{b7} 2 pages".into(), dw: None, problem: None, shots: vec![Shot { pic: Some(super::super::Pic { bytes: vec![9; 8], mime: "image/jpeg", ext: "jpg", w: 1280, h: 640 }), ..Default::default() }], notes: vec![], href: None } } }
    fn part(zip_bytes: &[u8], name: &str) -> String { use std::io::Read; let mut z = zip::ZipArchive::new(Cursor::new(zip_bytes)).unwrap(); let mut s = String::new(); z.by_name(name).unwrap().read_to_string(&mut s).unwrap(); s }
    #[test]
    fn headings_are_styles_each_numbered_list_starts_again_and_links_are_real() {
        let mut d = Doc::new(); let mut i = 0;
        let mut put = |d: &mut Doc, k: Kind, t: &str| { let id = BlockId::new(); d.insert(None, i, id, k, t).unwrap(); i += 1; id };
        put(&mut d, Kind::Heading { level: 2 }, "Plan <one>");
        put(&mut d, Kind::Numbered { depth: 0 }, "a"); put(&mut d, Kind::Numbered { depth: 0 }, "b"); put(&mut d, Kind::Paragraph, "between");
        put(&mut d, Kind::Numbered { depth: 0 }, "c"); put(&mut d, Kind::Task { depth: 0, state: 2 }, "done");
        let l = put(&mut d, Kind::Paragraph, "see here"); let mut s = mn_doc::Spans::new(); s.put(4, 8, Mark::Link("https://example.org/?a=1&b=2".into())); s.put(0, 3, Mark::Bold); d.set_spans(l, s).unwrap();
        put(&mut d, Kind::Media { descriptor: "{}".into() }, "");
        let z = docx(d.walk(), d.children(None), "My note", Opts::default(), &mut Fake).unwrap();
        let doc = part(&z, "word/document.xml");
        assert!(doc.contains("<w:pStyle w:val=\"NoteName\"/>") && doc.contains("<w:pStyle w:val=\"Heading2\"/></w:pPr><w:r><w:t xml:space=\"preserve\">Plan &lt;one&gt;</w:t>"), "{doc}");
        // two lists, two numberings; the task is a box and its text
        assert!(doc.contains("<w:numId w:val=\"2\"/></w:numPr></w:pPr><w:r><w:t xml:space=\"preserve\">b</w:t>") && doc.contains("<w:numId w:val=\"3\"/></w:numPr></w:pPr><w:r><w:t xml:space=\"preserve\">c</w:t>") && doc.contains("\u{2611}</w:t><w:tab/></w:r>"), "{doc}");
        assert!(doc.contains("<w:r><w:rPr><w:b/></w:rPr><w:t xml:space=\"preserve\">see</w:t></w:r>") && doc.contains("<w:hyperlink r:id=\"rIdLink1\""));
        let rels = part(&z, "word/_rels/document.xml.rels"); assert!(rels.contains("Target=\"https://example.org/?a=1&amp;b=2\" TargetMode=\"External\"") && rels.contains("Target=\"media/image1.jpg\""));
        // the page's picture is fitted to the column; its facts follow
        assert!(doc.contains(&format!("<wp:extent cx=\"{PAGE_EMU}\" cy=\"{}\"/>", PAGE_EMU / 2)) && doc.contains(">plan.pdf</w:t>") && doc.contains(">/vol/plan.pdf</w:t>"));
        assert!(part(&z, "word/numbering.xml").matches("<w:num w:numId").count() == 3 && part(&z, "[Content_Types].xml").contains("Extension=\"jpg\""));
    }
}
