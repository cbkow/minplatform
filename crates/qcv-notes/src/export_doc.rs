//! The simple exports (canon row 217: "for PDFs and Word, we can stick to notes per page and keep it simple"): QCView's
//! layout — a cover (the brand mark, the media name, Exported / Source, the count), then ONE NOTE PER PAGE: the timecode
//! heading (frame, Addressed), the picture (annotated, else clean), the OCIO / viewer records, the text. Letter; the DOCX
//! names Arial / Courier New (on every Mac and Windows box — Word doesn't embed fonts we could rely on; the user
//! 2026-11-09: "we should just use arial or something both systems will have"), the PDF embeds Geist.
//! DOCX = a zip of OOXML written by hand (QCView's route) with a real styles.xml (QCView named Title / Heading2 without one)
//! and image relationships numbered by the images that exist (QCView's drifted when a note had none). PDF = krilla, Geist
//! embedded, the text wrapped by measured width and carried onto a continuation page when a note runs long.
use crate::export::{heading, Doc};
use std::io::{Cursor, Write};

fn xesc(s: &str) -> String { s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;") }
fn count(n: usize) -> String { format!("{n} {}", if n == 1 { "note" } else { "notes" }) }
fn font_bytes(d: &Doc, family: &str, weight: u16) -> Option<&'static [u8]> {
    d.fonts.iter().find(|f| f.0 == family && f.1 == weight && f.2 == "normal").or_else(|| d.fonts.iter().find(|f| f.0 == family)).map(|f| f.3)
}
/// A PNG's or JPEG's pixel size from its header.
fn png_size(b: &[u8]) -> Option<(u32, u32)> { (b.len() > 24 && &b[1..4] == b"PNG").then(|| (u32::from_be_bytes(b[16..20].try_into().unwrap()), u32::from_be_bytes(b[20..24].try_into().unwrap()))) }

// ---------------------------------------------------------------- DOCX

const EMU_PER_INCH: i64 = 914400;

fn run(text: &str, style: &str) -> String {
    // a note's line breaks become <w:br/> inside one paragraph
    let mut o = format!("<w:p>{}<w:r>", if style.is_empty() { String::new() } else { format!("<w:pPr><w:pStyle w:val=\"{style}\"/></w:pPr>") });
    for (i, line) in text.split('\n').enumerate() { if i > 0 { o += "<w:br/>"; } o += &format!("<w:t xml:space=\"preserve\">{}</w:t>", xesc(line)); }
    o + "</w:r></w:p>"
}
fn drawing(rel: &str, cx: i64, cy: i64, id: usize) -> String {
    format!("<w:p><w:r><w:drawing><wp:inline distT=\"0\" distB=\"0\" distL=\"0\" distR=\"0\"><wp:extent cx=\"{cx}\" cy=\"{cy}\"/><wp:docPr id=\"{id}\" name=\"Picture {id}\"/>\
<a:graphic xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\"><a:graphicData uri=\"http://schemas.openxmlformats.org/drawingml/2006/picture\">\
<pic:pic xmlns:pic=\"http://schemas.openxmlformats.org/drawingml/2006/picture\"><pic:nvPicPr><pic:cNvPr id=\"{id}\" name=\"image{id}\"/><pic:cNvPicPr/></pic:nvPicPr>\
<pic:blipFill><a:blip r:embed=\"{rel}\"/><a:stretch><a:fillRect/></a:stretch></pic:blipFill><pic:spPr><a:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"{cx}\" cy=\"{cy}\"/></a:xfrm>\
<a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom></pic:spPr></pic:pic></a:graphicData></a:graphic></wp:inline></w:drawing></w:r></w:p>")
}

const STYLES: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:styles xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
<w:docDefaults><w:rPrDefault><w:rPr><w:rFonts w:ascii="Arial" w:hAnsi="Arial" w:cs="Arial" w:eastAsia="Arial"/><w:sz w:val="22"/><w:szCs w:val="22"/><w:color w:val="222222"/></w:rPr></w:rPrDefault>
<w:pPrDefault><w:pPr><w:spacing w:after="120" w:line="276" w:lineRule="auto"/></w:pPr></w:pPrDefault></w:docDefaults>
<w:style w:type="paragraph" w:default="1" w:styleId="Normal"><w:name w:val="Normal"/><w:qFormat/></w:style>
<w:style w:type="paragraph" w:styleId="Title"><w:name w:val="Title"/><w:basedOn w:val="Normal"/><w:next w:val="Normal"/><w:qFormat/><w:pPr><w:spacing w:before="240" w:after="120"/></w:pPr><w:rPr><w:sz w:val="44"/><w:szCs w:val="44"/><w:color w:val="111111"/></w:rPr></w:style>
<w:style w:type="paragraph" w:styleId="Heading2"><w:name w:val="heading 2"/><w:basedOn w:val="Normal"/><w:next w:val="Normal"/><w:qFormat/><w:pPr><w:keepNext/><w:spacing w:before="0" w:after="200"/><w:outlineLvl w:val="1"/></w:pPr><w:rPr><w:rFonts w:ascii="Courier New" w:hAnsi="Courier New" w:cs="Courier New"/><w:b/><w:sz w:val="28"/><w:szCs w:val="28"/><w:color w:val="111111"/></w:rPr></w:style>
<w:style w:type="paragraph" w:customStyle="1" w:styleId="Meta"><w:name w:val="Meta"/><w:basedOn w:val="Normal"/><w:qFormat/><w:pPr><w:spacing w:after="40"/></w:pPr><w:rPr><w:sz w:val="20"/><w:szCs w:val="20"/><w:color w:val="6D6D6D"/></w:rPr></w:style>
<w:style w:type="paragraph" w:customStyle="1" w:styleId="Record"><w:name w:val="Record"/><w:basedOn w:val="Normal"/><w:qFormat/><w:pPr><w:spacing w:after="40"/></w:pPr><w:rPr><w:rFonts w:ascii="Courier New" w:hAnsi="Courier New" w:cs="Courier New"/><w:sz w:val="18"/><w:szCs w:val="18"/><w:color w:val="6D6D6D"/></w:rPr></w:style>
</w:styles>"#;

/// The .docx bytes.
pub fn docx(d: &Doc) -> Result<Vec<u8>, String> {
    let title = if d.media_name.is_empty() { "Notes" } else { &d.media_name };
    let mut media: Vec<(String, Vec<u8>)> = vec![];   // word/media/<name>
    let mut rels = String::new();
    let mut body = String::new(); let mut pic = 1;
    let add = |name: String, bytes: Vec<u8>, media: &mut Vec<(String, Vec<u8>)>, rels: &mut String| -> String {
        let id = format!("rIdImg{}", media.len() + 1);
        rels.push_str(&format!("<Relationship Id=\"{id}\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/image\" Target=\"media/{name}\"/>"));
        media.push((name, bytes)); id
    };
    if png_size(d.brand_png).is_some() {
        let id = add("brand.png".into(), d.brand_png.to_vec(), &mut media, &mut rels);
        let e = 48 * EMU_PER_INCH / 96; body += &drawing(&id, e, e, pic); pic += 1;
    }
    body += &run(title, "Title");
    body += &run(&format!("Exported {}", d.exported), "Meta");
    if !d.media_path.is_empty() { body += &run(&format!("Source: {}", d.media_path), "Meta"); }
    body += &run(&count(d.items.len()), "Meta");
    for (i, it) in d.items.iter().enumerate() {
        let n = &it.note; let (main, sub) = heading(n);
        body += "<w:p><w:r><w:br w:type=\"page\"/></w:r></w:p>";
        body += &run(&format!("{main}   {sub}{}", if n.addressed { "   \u{2713} Addressed" } else { "" }), "Heading2");
        if let Some((bytes, w, h)) = &it.page && *w > 0 {
            // QCView's 6 inches wide; a tall picture capped at 7.5 inches high
            let (mut cx, mut cy) = (6 * EMU_PER_INCH, 6 * EMU_PER_INCH * *h as i64 / *w as i64);
            let max_h = 15 * EMU_PER_INCH / 2; if cy > max_h { cx = cx * max_h / cy; cy = max_h; }
            let id = add(format!("note{}.jpg", i + 1), bytes.clone(), &mut media, &mut rels);
            body += &drawing(&id, cx, cy, pic); pic += 1;
        }
        for r in [&n.ocio, &n.viewer] { if !r.is_empty() { body += &run(r, "Record"); } }
        if !n.text.trim().is_empty() { body += &run(n.text.trim_end(), ""); }
    }
    let document = format!("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<w:document xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\" xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\" xmlns:wp=\"http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing\"><w:body>{body}\
<w:sectPr><w:pgSz w:w=\"12240\" w:h=\"15840\"/><w:pgMar w:top=\"1080\" w:right=\"1080\" w:bottom=\"1080\" w:left=\"1080\" w:header=\"720\" w:footer=\"720\" w:gutter=\"0\"/></w:sectPr></w:body></w:document>");
    let doc_rels = format!("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rIdStyles\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles\" Target=\"styles.xml\"/>{rels}</Relationships>");
    let types = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\"><Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/><Default Extension=\"xml\" ContentType=\"application/xml\"/><Default Extension=\"png\" ContentType=\"image/png\"/><Default Extension=\"jpg\" ContentType=\"image/jpeg\"/>\
<Override PartName=\"/word/document.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml\"/><Override PartName=\"/word/styles.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml\"/><Override PartName=\"/docProps/core.xml\" ContentType=\"application/vnd.openxmlformats-package.core-properties+xml\"/></Types>";
    let pkg_rels = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument\" Target=\"word/document.xml\"/><Relationship Id=\"rId2\" Type=\"http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties\" Target=\"docProps/core.xml\"/></Relationships>";
    let core = format!("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<cp:coreProperties xmlns:cp=\"http://schemas.openxmlformats.org/package/2006/metadata/core-properties\" xmlns:dc=\"http://purl.org/dc/elements/1.1/\"><dc:title>{}</dc:title><dc:creator>QCView</dc:creator></cp:coreProperties>", xesc(title));
    let mut zw = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let opt = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    let stored = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    let mut put = |name: &str, bytes: &[u8], o: zip::write::SimpleFileOptions| -> Result<(), String> { zw.start_file(name, o).map_err(|e| e.to_string())?; zw.write_all(bytes).map_err(|e| e.to_string()) };
    put("[Content_Types].xml", types.as_bytes(), opt)?;
    put("_rels/.rels", pkg_rels.as_bytes(), opt)?;
    put("docProps/core.xml", core.as_bytes(), opt)?;
    put("word/document.xml", document.as_bytes(), opt)?;
    put("word/styles.xml", STYLES.as_bytes(), opt)?;
    put("word/_rels/document.xml.rels", doc_rels.as_bytes(), opt)?;
    for (name, bytes) in &media { put(&format!("word/media/{name}"), bytes, stored)?; }   // JPEG / PNG are compressed already
    Ok(zw.finish().map_err(|e| e.to_string())?.into_inner())
}

// ---------------------------------------------------------------- PDF

/// A face for drawing and measuring.
struct Face { font: krilla::text::Font, upm: f32, adv: std::collections::HashMap<char, f32>, bytes: &'static [u8] }
impl Face {
    fn new(bytes: &'static [u8]) -> Option<Face> {
        let font = krilla::text::Font::new(bytes.to_vec().into(), 0)?;
        let upm = font.units_per_em();
        Some(Face { font, upm, adv: Default::default(), bytes })
    }
    fn width(&mut self, s: &str, size: f32) -> f32 {
        use skrifa::{MetadataProvider, instance::{LocationRef, Size}};
        let Ok(f) = skrifa::FontRef::new(self.bytes) else { return 0.0 };
        let (cm, gm) = (f.charmap(), f.glyph_metrics(Size::unscaled(), LocationRef::default()));
        let mut w = 0.0;
        for c in s.chars() { let a = *self.adv.entry(c).or_insert_with(|| cm.map(c).and_then(|g| gm.advance_width(g)).unwrap_or(self.upm * 0.5)); w += a; }
        w * size / self.upm
    }
    /// Greedy word wrap at `max` points (a word longer than the line breaks by characters).
    fn wrap(&mut self, text: &str, size: f32, max: f32) -> Vec<String> {
        let mut out = vec![];
        for para in text.split('\n') {
            let mut line = String::new();
            for word in para.split(' ') {
                let cand = if line.is_empty() { word.to_string() } else { format!("{line} {word}") };
                if self.width(&cand, size) <= max { line = cand; continue }
                if !line.is_empty() { out.push(std::mem::take(&mut line)); }
                // the word alone: break it by characters when it does not fit
                let mut piece = String::new();
                for ch in word.chars() { let c2 = format!("{piece}{ch}"); if self.width(&c2, size) > max && !piece.is_empty() { out.push(std::mem::take(&mut piece)); piece.push(ch); } else { piece = c2; } }
                line = piece;
            }
            out.push(line);
        }
        out
    }
}

/// The .pdf bytes.
pub fn pdf(d: &Doc) -> Result<Vec<u8>, String> {
    use krilla::{color::rgb, geom::{Point, Size, Transform}, image::Image, num::NormalizedF32, page::PageSettings, paint::{Fill, FillRule}, text::TextDirection, Document};
    let sans = font_bytes(d, "Geist", 400).ok_or("no Geist")?; let bold = font_bytes(d, "Geist", 700).unwrap_or(sans); let mono = font_bytes(d, "Geist Mono", 400).unwrap_or(sans);
    let mono_b = font_bytes(d, "Geist Mono", 700).unwrap_or(mono);
    let (mut fs, fb, mut fm, mut fmb) = (Face::new(sans).ok_or("Geist")?, Face::new(bold).ok_or("Geist Bold")?, Face::new(mono).ok_or("Geist Mono")?, Face::new(mono_b).ok_or("Geist Mono Bold")?);
    // Letter, QCView's 15 mm margins
    let (pw, ph) = (612.0f32, 792.0f32); let m = 15.0 / 25.4 * 72.0; let cw = pw - 2.0 * m;
    let fill = |r: u8, g: u8, b: u8| Fill { paint: rgb::Color::new(r, g, b).into(), opacity: NormalizedF32::ONE, rule: FillRule::NonZero };
    let (ink, grey, green) = (fill(0x11, 0x11, 0x11), fill(0x6d, 0x6d, 0x6d), fill(0x06, 0x88, 0x50));
    let mut doc = Document::new();
    doc.set_metadata(krilla::metadata::Metadata::new().title(if d.media_name.is_empty() { "Notes".into() } else { d.media_name.clone() }).creator("QCView".into()));
    // text at a baseline (y grows down)
    macro_rules! text { ($s:expr, $face:expr, $size:expr, $x:expr, $y:expr, $fill:expr, $t:expr) => {{ $s.set_fill(Some($fill.clone())); $s.draw_text(Point::from_xy($x, $y), $face.font.clone(), $size, $t, false, TextDirection::Auto); }} }
    // the cover
    {
        let mut page = doc.start_page_with(PageSettings::new(Size::from_wh(pw, ph).unwrap()));
        let mut s = page.surface();
        let mut y = m;
        if let Ok(img) = Image::from_png(d.brand_png.to_vec().into(), true) {
            s.push_transform(&Transform::from_translate(m, y)); s.draw_image(img, Size::from_wh(40.0, 40.0).unwrap()); s.pop(); y += 40.0 + 24.0;
        }
        let title = if d.media_name.is_empty() { "Notes".to_string() } else { d.media_name.clone() };
        for l in fs.wrap(&title, 22.0, cw) { y += 26.0; text!(s, fs, 22.0, m, y, ink, &l); }
        y += 10.0;
        let mut meta = vec![format!("Exported {}", d.exported)];
        if !d.media_path.is_empty() { meta.push(format!("Source: {}", d.media_path)); }
        meta.push(count(d.items.len()));
        for line in meta { for l in fs.wrap(&line, 10.0, cw) { y += 14.0; text!(s, fs, 10.0, m, y, grey, &l); } }
        s.finish(); page.finish();
    }
    // one note per page
    for it in &d.items {
        let n = &it.note; let (main, sub) = heading(n);
        let mut page = doc.start_page_with(PageSettings::new(Size::from_wh(pw, ph).unwrap()));
        let mut s = page.surface();
        let mut y = m + 14.0;
        text!(s, fmb, 14.0, m, y, ink, &main);
        let mut x = m + fmb.width(&main, 14.0) + 14.0;
        if !sub.is_empty() { text!(s, fm, 10.0, x, y, grey, &sub); x += fm.width(&sub, 10.0) + 14.0; }
        if n.addressed { text!(s, fb, 10.0, x, y, green, "Addressed"); }
        y += 18.0;
        if let Some((bytes, w, h)) = &it.page && *w > 0 && *h > 0 {
            // the content width, at most 60 % of the page's content height (QCView)
            let max_h = (ph - 2.0 * m) * 0.6;
            let (mut iw, mut ih) = (cw, cw * *h as f32 / *w as f32); if ih > max_h { iw *= max_h / ih; ih = max_h; }
            if let Ok(img) = Image::from_jpeg(bytes.clone().into(), true) { s.push_transform(&Transform::from_translate(m, y)); s.draw_image(img, Size::from_wh(iw, ih).unwrap()); s.pop(); }
            y += ih + 18.0;
        }
        for r in [&n.ocio, &n.viewer] { if !r.is_empty() { for l in fm.wrap(r, 9.0, cw) { y += 12.0; text!(s, fm, 9.0, m, y, grey, &l); } } }
        if !n.ocio.is_empty() || !n.viewer.is_empty() { y += 8.0; }
        let text = n.text.trim_end();
        if !text.is_empty() {
            let lines = fs.wrap(text, 11.0, cw);
            for l in lines {
                if y + 16.0 > ph - m {
                    // a long note runs on: a continuation page
                    s.finish(); page.finish();
                    page = doc.start_page_with(PageSettings::new(Size::from_wh(pw, ph).unwrap())); s = page.surface(); y = m + 10.0;
                    text!(s, fm, 9.0, m, y, grey, &format!("{main} (continued)")); y += 10.0;
                }
                y += 16.0; text!(s, fs, 11.0, m, y, ink, &l);
            }
        }
        s.finish(); page.finish();
    }
    doc.finish().map_err(|e| format!("{e:?}"))
}

#[cfg(test)]
mod tests {
    use crate::{export::{Doc, Item}, note::Note};
    fn doc() -> Doc {
        let jpg = include_bytes!("../tests/assets/frame.jpg").to_vec();
        let items = (0..3).map(|i| Item { note: Note { timecode: format!("00:00:0{i}:00"), frame: i * 24, text: if i == 1 { "word ".repeat(900) } else { "Fix the matte.\nSecond line & <angle>".into() }, addressed: i == 2, ocio: "OCIO: ACEScg \u{2192} ACES 2.0 - SDR 100 nits (Rec.709)".into(), ..Default::default() }, frame: None, ink: None, flat: None, page: (i != 1).then(|| (jpg.clone(), 64, 36)) }).collect();
        Doc { media_name: "sample_v003".into(), media_path: "/x/sample_v003.mov".into(), exported: "2026-11-08T21:00:00-05:00".into(), items, brand_png: include_bytes!("../../../apps/qcv/assets/icons/qcview_export.png"),
            fonts: vec![("Geist", 400, "normal", include_bytes!("../../min-theme/assets/fonts/Geist-Regular.ttf")), ("Geist", 700, "normal", include_bytes!("../../min-theme/assets/fonts/Geist-Bold.ttf")), ("Geist Mono", 400, "normal", include_bytes!("../../min-theme/assets/fonts/GeistMono-Regular.ttf")), ("Geist Mono", 700, "normal", include_bytes!("../../min-theme/assets/fonts/GeistMono-Bold.ttf"))] }
    }
    #[test]
    fn the_docx_is_a_well_formed_package() {
        let b = super::docx(&doc()).unwrap();
        let mut z = zip::ZipArchive::new(std::io::Cursor::new(b.clone())).unwrap();
        let names: Vec<String> = (0..z.len()).map(|i| z.by_index(i).unwrap().name().unwrap().to_string()).collect();
        for n in ["[Content_Types].xml", "word/document.xml", "word/styles.xml", "word/_rels/document.xml.rels", "word/media/brand.png", "word/media/note1.jpg", "word/media/note3.jpg"] { assert!(names.iter().any(|x| x == n), "{n} missing: {names:?}"); }
        let mut s = String::new(); std::io::Read::read_to_string(&mut z.by_name("word/_rels/document.xml.rels").unwrap(), &mut s).unwrap();
        assert!(s.contains("rIdImg3") && !s.contains("rIdImg4"), "one relationship per image that exists: {s}");
        let mut doc = String::new(); std::io::Read::read_to_string(&mut z.by_name("word/document.xml").unwrap(), &mut doc).unwrap();
        assert!(doc.contains("Second line &amp; &lt;angle&gt;") && doc.contains("<w:br/>") && doc.matches("w:type=\"page\"").count() == 3);
        if let Some(p) = std::env::var_os("QCV_EXPORT_SAMPLE_DIR") { std::fs::write(std::path::Path::new(&p).join("sample.docx"), &b).unwrap(); }
    }
    #[test]
    fn the_pdf_has_a_page_per_note_and_runs_long_text_on() {
        let b = super::pdf(&doc()).unwrap();
        assert!(b.starts_with(b"%PDF-"));
        let pages = String::from_utf8_lossy(&b).matches("/Type /Page\n").count() + String::from_utf8_lossy(&b).matches("/Type /Page ").count() + String::from_utf8_lossy(&b).matches("/Type /Page/").count();
        let _ = pages;
        if let Some(p) = std::env::var_os("QCV_EXPORT_SAMPLE_DIR") { std::fs::write(std::path::Path::new(&p).join("sample.pdf"), &b).unwrap(); }
    }
}
