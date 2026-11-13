//! Spike S11 (dies with its verdict): a note laid out by `mn-layout` at a page's width, cut into pages between lines,
//! written as a PDF whose text is the layout's own glyphs with their characters (so it can be selected and searched).
//! `s11-notepdf <out.pdf> [blocks]`
use krilla::{color::rgb, geom::{Point, Size}, num::NormalizedF32, page::PageSettings, paint::{Fill, FillRule}, text::{Font, GlyphId, KrillaGlyph}, Document};
use mn_doc::{BlockId, Doc, Kind};
use mn_layout::{NoteLayout, Part, Style};
use parley::PositionedLayoutItem;
use std::collections::HashMap;

/// One thing that must stay whole on a page: a line of text (its glyph runs), or a rule.
struct Atom { top: f32, bottom: f32, runs: Vec<Run>, rule: Option<(f32, f32)> }
struct Run { font: (u64, u32), size: f32, x: f32, baseline: f32, color: [u8; 3], glyphs: Vec<KrillaGlyph>, text: String }

fn sample(n: usize) -> Doc {
    let mut d = Doc::new(); let mut i = 0;
    let mut put = |d: &mut Doc, k: Kind, t: String| { let id = BlockId::new(); d.insert(None, i, id, k, t).unwrap(); i += 1; id };
    put(&mut d, Kind::Heading { level: 1 }, "Review notes \u{2014} second pass".into());
    for s in 0..n {
        if s % 9 == 0 { put(&mut d, Kind::Heading { level: 2 }, format!("Section {}", s / 9 + 1)); }
        match s % 9 {
            2 => { for k in 0..3 { put(&mut d, Kind::Bullet { depth: (k == 1) as u8 }, format!("A list item, number {k}, long enough that it may wrap onto a second line at the width of a printed page")); } }
            4 => { put(&mut d, Kind::Code { lang: "sh".into() }, "ffmpeg -i in.mov -c:v prores_ks -profile:v 4 out.mov\n# a second line of code".into()); }
            6 => { put(&mut d, Kind::Quote, "Each version was compared against the previous one, frame by frame.".into()); }
            7 => { for k in 0..2 { put(&mut d, Kind::Task { depth: 0, state: k * 2 }, format!("A task in state {k}")); } }
            _ => { put(&mut d, Kind::Paragraph, format!("Paragraph {s}: the grade on the wide shot holds up well against the reference, nothing in the audio mix stood out on the small speakers, and the titles sit inside the safe area on every format except the vertical cut, which needs a second look before it goes out. Caf\u{e9}, na\u{ef}ve, \u{201c}quotes\u{201d}.")); }
        }
    }
    // a table: two columns, three rows
    let t = BlockId::new(); d.insert(None, i, t, Kind::Table { header_rows: 1, columns: vec![Default::default(), Default::default()] }, "").unwrap();
    for (r, cells) in [["Shot", "State"], ["The wide shot at the top of the closing sequence", "Open"], ["Titles", "Closed"]].iter().enumerate() { let row = BlockId::new(); d.insert(Some(t), r, row, Kind::row(), "").unwrap();
        for (c, text) in cells.iter().enumerate() { let cell = BlockId::new(); d.insert(Some(row), c, cell, Kind::cell(), "").unwrap(); d.insert(Some(cell), 0, BlockId::new(), Kind::Paragraph, *text).unwrap(); } }
    d
}

fn main() {
    let args: Vec<String> = std::env::args().collect(); let out = args.get(1).expect("s11-notepdf <out.pdf> [blocks]"); let n: usize = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(120);
    let doc = sample(n);
    // Letter, 0.75 in margins; the note is laid out 640 units wide and drawn at the scale that makes that the column
    let (pw, ph, m) = (612.0f32, 792.0f32, 54.0f32); let (cw, chh) = (pw - 2.0 * m, ph - 2.0 * m); let lay_w = 640.0f32; let k = cw / lay_w;
    let style = Style { text: peniko::Color::from_rgb8(0x11, 0x11, 0x11), link: peniko::Color::from_rgb8(0x05, 0x63, 0xC1), ..Default::default() };
    let t0 = std::time::Instant::now();
    let mut fonts = min_ui::fonts::Fonts::new();
    let lay = NoteLayout::build(&doc, &mut fonts, &style, lay_w, 1.0);
    let t_layout = t0.elapsed();
    // every line of every text block, and every divider, as atoms with their place in the laid-out note
    let mut atoms: Vec<Atom> = vec![]; let mut faces: HashMap<(u64, u32), Font> = HashMap::new();
    for b in doc.walk() {
        let (Some(bx), Some(r)) = (lay.get(b.id), lay.rect(&doc, b.id)) else { continue };
        match &bx.part {
            Part::Text { layout, dx, dy } => {
                let (ox, oy) = (r.x0 as f32 + dx, r.y0 as f32 + dy);
                for line in layout.lines() {
                    let lm = line.metrics(); let mut a = Atom { top: oy + lm.block_min_coord, bottom: oy + lm.block_max_coord, runs: vec![], rule: None };
                    for item in line.items() { let PositionedLayoutItem::GlyphRun(gr) = item else { continue };
                        let run = gr.run(); let f = run.font(); let key = (f.data.id(), f.index); let size = run.font_size();
                        faces.entry(key).or_insert_with(|| Font::new(f.data.data().to_vec().into(), f.index).expect("font"));
                        let c = match &gr.style().brush { peniko::Brush::Solid(c) => { let c = c.to_rgba8(); [c.r, c.g, c.b] } _ => [0x11; 3] };
                        // the glyphs cluster by cluster, each with the bytes of the text it stands for
                        let mut glyphs = vec![];
                        for cl in run.visual_clusters() { let range = cl.text_range(); let mut first = true;
                            for g in cl.glyphs() { let rg = if first { range.clone() } else { range.end..range.end }; first = false;
                                glyphs.push(KrillaGlyph::new(GlyphId::new(g.id as u32), g.advance / size, g.x / size, g.y / size, 0.0, rg, None)); } }
                        a.runs.push(Run { font: key, size, x: ox + gr.offset(), baseline: oy + gr.baseline(), color: c, glyphs, text: b.text.clone() });
                    }
                    atoms.push(a);
                }
            }
            Part::Divider => atoms.push(Atom { top: r.y0 as f32, bottom: r.y1 as f32, runs: vec![], rule: Some((r.x0 as f32, r.x1 as f32)) }),
            _ => {}
        }
    }
    atoms.sort_by(|a, b| a.top.partial_cmp(&b.top).unwrap());
    // pages: an atom that would cross the bottom starts the next page at its own top
    let page_h = chh / k; let mut pages: Vec<(f32, Vec<&Atom>)> = vec![]; let mut cut = 0usize;
    for a in &atoms { let start = pages.last().map(|p| p.0).unwrap_or(0.0);
        if pages.is_empty() || a.bottom - start > page_h { pages.push((if pages.is_empty() { 0.0 } else { a.top }, vec![])); }
        let p = pages.last_mut().unwrap(); if a.bottom - p.0 > page_h { cut += 1; } p.1.push(a); }
    let t_pages = t0.elapsed();
    let mut pdf = Document::new();
    for (start, list) in &pages {
        let mut page = pdf.start_page_with(PageSettings::new(Size::from_wh(pw, ph).unwrap())); let mut s = page.surface();
        for a in list {
            if let Some((x0, x1)) = a.rule { let y = m + ((a.top + a.bottom) / 2.0 - start) * k; let mut pb = krilla::geom::PathBuilder::new(); pb.move_to(m + x0 * k, y); pb.line_to(m + x1 * k, y); if let Some(p) = pb.finish() { s.set_fill(None); s.set_stroke(Some(krilla::paint::Stroke { paint: rgb::Color::new(0xC6, 0xC6, 0xC6).into(), width: 0.75, ..Default::default() })); s.draw_path(&p); s.set_stroke(None); } }
            for r in &a.runs { s.set_fill(Some(Fill { paint: rgb::Color::new(r.color[0], r.color[1], r.color[2]).into(), opacity: NormalizedF32::ONE, rule: FillRule::NonZero }));
                s.draw_glyphs(Point::from_xy(m + r.x * k, m + (r.baseline - start) * k), &r.glyphs, faces[&r.font].clone(), &r.text, r.size * k, false); }
        }
        s.finish(); page.finish();
    }
    let bytes = pdf.finish().expect("pdf"); let t_all = t0.elapsed();
    std::fs::write(out, &bytes).unwrap();
    println!("blocks {} lines {} pages {} | layout {:.0} ms, pagination {:.0} ms, written {:.0} ms in all | {} KB | atoms taller than a page: {cut}", doc.len(), atoms.len(), pages.len(), t_layout.as_secs_f64() * 1e3, (t_pages - t_layout).as_secs_f64() * 1e3, t_all.as_secs_f64() * 1e3, bytes.len() / 1024);
}
