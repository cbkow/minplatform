//! A note as a PDF (canon row 330; spike S11's verdict). The note is laid out by the editor's own layout (`mn-layout`)
//! at a page's width with dark text, cut into pages BETWEEN lines, and written with krilla - the layout's own glyphs,
//! each with the characters it stands for (the text can be selected and searched), Geist embedded.
//!
//! Three kinds of thing are placed:
//! - ATOMS stay whole on a page: a line of text with what belongs to it (its marker, a highlight behind it, an
//!   underline, a link), a divider, a picture, a line of a caption. A table's row is kept together when it fits a page.
//! - SPANS run down the note and are cut at a page's edge: a code block's ground, a quote's bar, a table's fills and
//!   grid.
//! - A media block is laid out HERE (its picture as wide as its place, its facts under it, a PDF's drawn-on pages, a
//!   clip's notes): it is taller or shorter than the editor's box, so what is under it moves by the difference. A table
//!   wider than the page is drawn smaller, to fit; what is under it moves up.
use super::{chip_of, Item, Mode, Opts, Shot, Source, What};
use krilla::{color::rgb, geom::{PathBuilder, Point, Rect as KRect, Size, Transform}, image::Image, num::NormalizedF32, page::PageSettings, paint::{Fill, FillRule, LineCap, LineJoin, Stroke}, text::{Font, GlyphId, KrillaGlyph}, Document};
use min_ui::{fonts::Fonts, Family};
use mn_doc::{BlockId, ColumnKind, Doc, Kind, Mark, MarkKind};
use mn_layout::{NoteLayout, Part, Style};
use parley::{Layout, PositionedLayoutItem};
use peniko::{Brush, Color};
use std::collections::{HashMap, HashSet};

/// The note is laid out this wide (the editor's units); the page's column is that, scaled.
const LAY_W: f32 = 640.0;
type Rgb = [u8; 3];
const INK: Rgb = [0x11, 0x11, 0x11]; const DIM: Rgb = [0x6d, 0x6d, 0x6d]; const RULE: Rgb = [0xc6, 0xc6, 0xc6]; const GROUND: Rgb = [0xf4, 0xf4, 0xf4]; const ACCENT: Rgb = [0x26, 0x80, 0xeb];

enum Draw {
    Glyphs { font: (u64, u32), size: f32, x: f32, y: f32, color: Rgb, glyphs: Vec<KrillaGlyph>, text: String },
    Fill { x0: f32, y0: f32, x1: f32, y1: f32, color: Rgb, radius: f32 },
    Outline { x0: f32, y0: f32, x1: f32, y1: f32, color: Rgb, radius: f32, width: f32 },
    Line { x0: f32, y0: f32, x1: f32, y1: f32, color: Rgb, width: f32 },
    Disc { cx: f32, cy: f32, r: f32, color: Rgb, hollow: bool },
    Tick { pts: [(f32, f32); 3], width: f32 },
    Picture { img: Image, x: f32, y: f32, w: f32, h: f32 },
    Link { x0: f32, y0: f32, x1: f32, y1: f32, url: String },
}
/// `anchor`: a height in the laid-out note (it moves with what is above it); `off`: further down from there, fixed.
/// Its drawings' `y` are from its top; every `x` is the laid-out note's.
struct Atom { anchor: f32, off: f32, h: f32, unit: Option<usize>, draws: Vec<Draw> }
struct Span { x0: f32, x1: f32, y0: f32, y1: f32, fill: Option<Rgb>, stroke: Option<Rgb> }

fn rgb_of(c: Color) -> Rgb { let c = c.to_rgba8(); [c.r, c.g, c.b] }
/// A colour laid over white paper at a share.
fn tint(c: Rgb, share: f32) -> Rgb { c.map(|v| (255.0 + (v as f32 - 255.0) * share).round() as u8) }
fn colour(s: &str) -> Option<Rgb> { mn_layout::colour(s).map(rgb_of) }

struct Paper<'a> { doc: &'a Doc, lay: &'a NoteLayout, fonts: Fonts, faces: HashMap<(u64, u32), Font>, atoms: Vec<Atom>, spans: Vec<Span>,
    /// (below this height, everything moves by this much)
    inserts: Vec<(f32, f32)>, /// (from, to, scale): a table drawn smaller
    squeezes: Vec<(f32, f32, f32)>, /// things kept together on a page when they fit one (a table's row, a picture with its caption): top and bottom, each a height of the laid-out note and a way down from it
    units: Vec<(f32, f32, f32, f32)>, row_unit: HashMap<BlockId, usize>, /// a wide table at the top of the note: its left edge and its scale
    wide: HashMap<BlockId, (f32, f32)> }

impl Paper<'_> {
    /// A shaped text's lines: (top, bottom, drawings) with `x` from `ox` and each drawing's `y` from its line's top, at scale `f`.
    fn lines(&mut self, layout: &Layout<Brush>, ox: f32, f: f32, text: &str) -> Vec<(f32, f32, Vec<Draw>)> {
        let mut out = vec![];
        for line in layout.lines() {
            let lm = line.metrics(); let mut draws = vec![];
            // (a run of one face may come as several glyph runs - one per change of colour or mark; each takes the next
            // glyphs of the run, so the run's clusters are walked on from where the last one stopped)
            let mut taken: HashMap<(usize, usize), usize> = HashMap::new();
            for item in line.items() { let PositionedLayoutItem::GlyphRun(gr) = item else { continue };
                let run = gr.run(); let fnt = run.font(); let key = (fnt.data.id(), fnt.index); let size = run.font_size();
                if !self.faces.contains_key(&key) { let Some(face) = Font::new(fnt.data.data().to_vec().into(), fnt.index) else { continue }; self.faces.insert(key, face); }
                let color = match &gr.style().brush { Brush::Solid(c) => rgb_of(*c), _ => INK };
                let mut all = vec![];
                for cl in run.visual_clusters() { let range = cl.text_range(); let range = range.start.min(text.len())..range.end.min(text.len()); let mut first = true;
                    for g in cl.glyphs() { let rg = if first { range.clone() } else { range.end..range.end }; first = false; all.push(KrillaGlyph::new(GlyphId::new(g.id as u32), g.advance / size, g.x / size, g.y / size, 0.0, rg, None)); } }
                let tr = run.text_range(); let from = taken.entry((tr.start, tr.end)).or_insert(0); let n = gr.glyphs().count();
                let glyphs: Vec<KrillaGlyph> = all.into_iter().skip(*from).take(n).collect(); *from += n;
                draws.push(Draw::Glyphs { font: key, size: size * f, x: ox + gr.offset() * f, y: (gr.baseline() - lm.block_min_coord) * f, color, glyphs, text: text.to_string() });
            }
            out.push((lm.block_min_coord, lm.block_max_coord, draws));
        }
        out
    }
    /// The wide table a block is in: (its left edge, its scale); (0, 1) elsewhere.
    fn scale_of(&self, id: BlockId) -> (f32, f32) { let mut up = Some(id); while let Some(i) = up { if let Some(w) = self.wide.get(&i) { return *w } up = self.doc.get(i).and_then(|b| b.parent); } (0.0, 1.0) }
    fn unit_of(&self, id: BlockId) -> Option<usize> { let mut up = self.doc.get(id).and_then(|b| b.parent); while let Some(i) = up { if let Some(u) = self.row_unit.get(&i) { return Some(*u) } up = self.doc.get(i).and_then(|b| b.parent); } None }

    fn text_block(&mut self, id: BlockId) {
        let doc = self.doc; let Some(b) = doc.get(id) else { return }; let (Some(bx), Some(r)) = (self.lay.get(id), self.lay.rect(doc, id)) else { return };
        let Part::Text { layout, dx, dy } = &bx.part else { return };
        let (x0, f) = self.scale_of(id); let sx = |x: f32| x0 + (x - x0) * f; let unit = self.unit_of(id); let st = self.lay.style.clone();
        let (rx0, ry0, rx1, ry1) = (r.x0 as f32, r.y0 as f32, r.x1 as f32, r.y1 as f32); let (tx, ty) = (rx0 + dx, ry0 + dy);
        let typed = if b.kind == Kind::Paragraph { mn_edit::typed::typed_of(doc, id) } else { None };
        let boxed = matches!(&typed, Some(x) if x.kind == ColumnKind::Check);
        let lines = self.lines(layout, sx(tx), f, &b.text); if lines.is_empty() { return }
        let first = self.atoms.len();
        for (top, bottom, draws) in lines { self.atoms.push(Atom { anchor: ty + top, off: 0.0, h: (bottom - top) * f, unit, draws: if boxed { vec![] } else { draws } }); }
        let tops: Vec<(f32, f32)> = self.atoms[first..].iter().map(|a| (a.anchor, a.anchor + a.h / f)).collect();
        // the atom a height of the block falls in, and that height from the atom's top (scaled)
        let place = |y: f32| -> (usize, f32) { let i = tops.iter().position(|(t, bt)| y >= *t && y < *bt).unwrap_or(if y < tops[0].0 { 0 } else { tops.len() - 1 }); (first + i, (y - tops[i].0) * f) };
        let (mut under, mut over): (Vec<(usize, Draw)>, Vec<(usize, Draw)>) = (vec![], vec![]);
        match &b.kind {
            Kind::Code { .. } => self.spans.push(Span { x0: sx(rx0), x1: sx(rx1), y0: ry0, y1: ry1, fill: Some(GROUND), stroke: None }),
            Kind::Quote => self.spans.push(Span { x0: sx(rx0 + 4.0), x1: sx(rx0 + 7.0), y0: ry0, y1: ry1, fill: Some(RULE), stroke: None }),
            Kind::Bullet { depth } => { let (i, y) = place(ty + st.body * st.line_height * 0.5); under.push((i, Draw::Disc { cx: sx(tx - 11.0), cy: y, r: 2.2 * f, color: DIM, hollow: depth % 2 == 1 })); }
            Kind::Numbered { depth } => { let n = number(doc, id, *depth); let g = self.fonts.layout(&format!("{n}."), Family::Sans, st.body, 400.0, Color::from_rgb8(DIM[0], DIM[1], DIM[2])); let w = g.width();
                if let Some((_, _, d)) = self.lines(&g, sx(tx - 6.0 - w), f, "").into_iter().next() { for x in d { under.push((first, x)); } } }
            Kind::Task { state, .. } => check(&mut under, first, sx(tx - 20.0), 3.0 * f, f, *state),
            _ => {}
        }
        if boxed { let state = match mn_edit::typed::chosen(&b.text, &b.spans).as_deref() { Some("1") => 1, Some("2") => 2, _ => 0 }; check(&mut under, first, sx(tx), 3.0 * f, f, state); }
        let listed = |sel: &str| match &typed { Some(mn_edit::typed::Typed { kind: ColumnKind::Choice { options }, .. }) => options.iter().find(|o| o.id == sel).and_then(|o| colour(&o.color)), _ => None };
        for sp in b.spans.as_slice().iter().filter(|_| !boxed) {
            for q in self.lay.range_rects(doc, id, sp.start, sp.end) { let (qx0, qx1, qy0, qy1) = (sx(q.x0 as f32), sx(q.x1 as f32), q.y0 as f32, q.y1 as f32); let (i, y0) = place((qy0 + qy1) / 2.0); let hh = (qy1 - qy0) * f; let y0 = y0 - hh / 2.0;
                match &sp.mark {
                    Mark::Highlight(c) => under.push((i, Draw::Fill { x0: qx0, y0, x1: qx1, y1: y0 + hh, color: colour(c).map(|c| tint(c, 0.4)).unwrap_or([0xff, 0xf2, 0xa8]), radius: 0.0 })),
                    Mark::Choice(p) => { let (sel, own) = chip_of(p); let c = listed(&sel).or(colour(&own)).map(|c| tint(c, 0.3)).unwrap_or([0xef, 0xef, 0xef]); under.push((i, Draw::Fill { x0: qx0 - 5.0 * f, y0, x1: qx1 + 5.0 * f, y1: y0 + hh, color: c, radius: 9.0 * f })); }
                    Mark::Code => under.push((i, Draw::Fill { x0: qx0 - 2.0 * f, y0, x1: qx1 + 2.0 * f, y1: y0 + hh, color: [0xef, 0xef, 0xef], radius: 3.0 * f })),
                    m => { let k = m.kind(); if matches!(k, MarkKind::Underline | MarkKind::Link | MarkKind::Strike) { let y = if k == MarkKind::Strike { y0 + hh * 0.56 } else { y0 + hh - 3.0 * f };
                        over.push((i, Draw::Line { x0: qx0, y0: y, x1: qx1, y1: y, color: if k == MarkKind::Link { rgb_of(st.link) } else { INK }, width: 0.8 * f })); }
                        if let Mark::Link(u) = m { over.push((i, Draw::Link { x0: qx0, y0, x1: qx1, y1: y0 + hh, url: u.clone() })); } }
                }
            }
        }
        for (i, d) in under.into_iter().rev() { self.atoms[i].draws.insert(0, d); } for (i, d) in over { self.atoms[i].draws.push(d); }
    }
    /// A caption's lines as atoms under `anchor`, from `off` down; the new `off`.
    fn caption(&mut self, text: &str, family: Family, size: f32, weight: f32, color: Rgb, x: f32, width: f32, f: f32, anchor: f32, mut off: f32, unit: Option<usize>) -> f32 {
        if text.is_empty() { return off }
        let l = self.fonts.layout_width(text, family, size, weight, Color::from_rgb8(color[0], color[1], color[2]), Some(width / f));
        for (top, bottom, draws) in self.lines(&l, x, f, text) { let h = (bottom - top) * f; self.atoms.push(Atom { anchor, off, h, unit, draws }); off += h; }
        off
    }
    /// `boxed`: a hairline round it (a page of a PDF is white on white paper).
    fn picture(&mut self, s: &Shot, x: f32, width: f32, tallest: f32, anchor: f32, off: f32, unit: Option<usize>, boxed: bool) -> f32 {
        let Some(p) = s.pic.as_ref().filter(|p| p.w > 0 && p.h > 0) else { return off };
        let img = match p.ext { "png" => Image::from_png(p.bytes.clone().into(), true), "jpg" => Image::from_jpeg(p.bytes.clone().into(), true), _ => return off }; let Ok(img) = img else { return off };
        let (mut w, mut h) = (width, width * p.h as f32 / p.w as f32); if h > tallest { w *= tallest / h; h = tallest; }
        let mut draws = vec![Draw::Picture { img, x, y: 0.0, w, h }]; if boxed { draws.push(Draw::Outline { x0: x, y0: 0.0, x1: x + w, y1: h, color: RULE, radius: 0.0, width: 0.7 }); }
        self.atoms.push(Atom { anchor, off, h, unit, draws }); off + h
    }
    /// A media block, laid out here in its place.
    fn media(&mut self, id: BlockId, it: &Item, opts: Opts, page_h: f32) {
        let Some(r) = self.lay.rect(self.doc, id) else { return }; let (x0, f) = self.scale_of(id); let row = self.unit_of(id); let boxed = it.what != What::Picture;
        // outside a table, each picture stays with what is written under it
        let mut unit = row; let (mut from, mut at) = (0.0f32, self.atoms.len());
        macro_rules! group { ($me:expr, $off:expr) => { if row.is_none() { let u = $me.units.len(); $me.units.push((r.y0 as f32, from, r.y0 as f32, $off)); for a in &mut $me.atoms[at..] { a.unit = Some(u); } } from = $off; at = $me.atoms.len(); let _ = (from, at); } }
        let (x, w, top) = (x0 + (r.x0 as f32 - x0) * f, r.width() as f32 * f, r.y0 as f32); let tallest = page_h * 0.92; let gap = 4.0 * f; let mut off = 0.0;
        let first = it.shots.first();
        // a picture is as wide as the note shows it; a page of a PDF and a clip's frame fill their place
        if let Some(s) = first { off = self.picture(s, x, w, tallest, top, off, unit, boxed); }
        if it.what != What::Picture || it.problem.is_some() || first.is_none_or(|s| s.pic.is_none()) {
            off += gap; off = self.caption(&it.name, Family::Sans, 12.5, 700.0, INK, x, w, f, top, off, unit);
            if opts.paths { off = self.caption(&it.path, Family::Mono, 10.5, 400.0, DIM, x, w, f, top, off, unit); }
            off = self.caption(&it.meta, Family::Mono, 10.5, 400.0, DIM, x, w, f, top, off, unit);
            if let Some(p) = &it.problem { off = self.caption(p, Family::Mono, 10.5, 400.0, [0xc9, 0x25, 0x2d], x, w, f, top, off, unit); }
            off += 2.0 * gap;
        }
        group!(self, off);
        for s in it.shots.iter().skip(1) { off += 3.0 * gap; from = off; off = self.picture(s, x, w, tallest, top, off, unit, boxed); off += gap; off = self.caption(&format!("{} \u{2014} {}", s.head, s.sub), Family::Mono, 10.5, 400.0, DIM, x, w, f, top, off, unit); group!(self, off); }
        for s in &it.notes { off += 4.0 * gap; from = off;
            off = self.caption(&format!("{}   {}{}", s.head, s.sub, if s.done == Some(true) { "   \u{2713} Addressed" } else { "" }), Family::Mono, 11.0, 700.0, INK, x, w, f, top, off, unit); off += gap;
            off = self.picture(s, x, w * 0.66, tallest, top, off, unit, false); off += gap;
            off = self.caption(s.text.trim_end(), Family::Sans, self.lay.style.body, 400.0, INK, x, w, f, top, off, unit); group!(self, off); }
        unit = None; let _ = unit;
        // what is under it moves by the difference - but beside other things (in a column, a cell) nothing is pulled UP: the
        // lane beside it may be the taller one
        let delta = off - r.height() as f32 * f; let top_level = self.doc.get(id).is_some_and(|b| b.parent.is_none());
        if top_level || delta > 0.0 { self.inserts.push((r.y1 as f32, delta)); }
    }
    /// A height of the laid-out note as it stands on paper (before pages).
    fn map(&self, y: f32) -> f32 { y + self.inserts.iter().filter(|(after, _)| *after <= y + 0.01).map(|(_, d)| *d).sum::<f32>() - self.squeezes.iter().map(|(a, b, f)| (1.0 - f) * (y - a).clamp(0.0, b - a)).sum::<f32>() }
}
/// A task's (or a check cell's) box at `x`, `y` from the atom's top: to do, doing, done.
fn check(out: &mut Vec<(usize, Draw)>, atom: usize, x: f32, y: f32, f: f32, state: u8) {
    let (x1, y1) = (x + 14.0 * f, y + 14.0 * f);
    match state { 0 => out.push((atom, Draw::Outline { x0: x, y0: y, x1, y1, color: DIM, radius: 3.0 * f, width: 1.2 * f })),
        1 => { out.push((atom, Draw::Fill { x0: x + 3.0 * f, y0: (y + y1) / 2.0 - f, x1: x1 - 3.0 * f, y1: (y + y1) / 2.0 + f, color: ACCENT, radius: 0.0 })); out.push((atom, Draw::Outline { x0: x, y0: y, x1, y1, color: ACCENT, radius: 3.0 * f, width: 1.2 * f })); }
        _ => { out.push((atom, Draw::Fill { x0: x, y0: y, x1, y1, color: ACCENT, radius: 3.0 * f })); out.push((atom, Draw::Tick { pts: [(x + 3.0 * f, (y + y1) / 2.0), (x + 6.0 * f, y1 - 3.5 * f), (x1 - 3.0 * f, y + 3.5 * f)], width: 1.6 * f })); } }
}
/// The number a numbered item shows (the editor's rule: it counts the numbered items above it at its depth).
fn number(doc: &Doc, id: BlockId, depth: u8) -> usize {
    let Some(b) = doc.get(id) else { return 1 }; let sibs = doc.children(b.parent); let at = sibs.iter().position(|x| *x == id).unwrap_or(0); let mut n = 1;
    for s in sibs[..at].iter().rev() { match doc.get(*s).map(|x| &x.kind) { Some(Kind::Numbered { depth: d }) if *d == depth => n += 1, Some(Kind::Numbered { depth: d } | Kind::Bullet { depth: d } | Kind::Task { depth: d, .. }) if *d > depth => {}, _ => break } }
    n
}
fn rounded(x0: f32, y0: f32, x1: f32, y1: f32, r: f32) -> Option<krilla::geom::Path> {
    let mut pb = PathBuilder::new(); let r = r.min((x1 - x0) / 2.0).min((y1 - y0) / 2.0).max(0.0);
    if r < 0.05 { pb.move_to(x0, y0); pb.line_to(x1, y0); pb.line_to(x1, y1); pb.line_to(x0, y1); pb.close(); return pb.finish() }
    let c = r * 0.5523;
    pb.move_to(x0 + r, y0); pb.line_to(x1 - r, y0); pb.cubic_to(x1 - r + c, y0, x1, y0 + r - c, x1, y0 + r); pb.line_to(x1, y1 - r); pb.cubic_to(x1, y1 - r + c, x1 - r + c, y1, x1 - r, y1);
    pb.line_to(x0 + r, y1); pb.cubic_to(x0 + r - c, y1, x0, y1 - r + c, x0, y1 - r); pb.line_to(x0, y0 + r); pb.cubic_to(x0, y0 + r - c, x0 + r - c, y0, x0 + r, y0); pb.close(); pb.finish()
}
fn fill_of(c: Rgb) -> Fill { Fill { paint: rgb::Color::new(c[0], c[1], c[2]).into(), opacity: NormalizedF32::ONE, rule: FillRule::NonZero } }
fn stroke_of(c: Rgb, width: f32) -> Stroke { Stroke { paint: rgb::Color::new(c[0], c[1], c[2]).into(), width, line_cap: LineCap::Round, line_join: LineJoin::Round, ..Default::default() } }

/// The note as a PDF. `style`: the editor's (its sizes and spacing; the colours are paper's). `metric`: A4, else Letter.
pub fn pdf(doc: &Doc, title: &str, style: &Style, opts: Opts, metric: bool, src: &mut dyn Source) -> Result<Vec<u8>, String> {
    let (pw, ph) = if metric { (595.28f32, 841.89f32) } else { (612.0, 792.0) }; let m = 54.0f32; let k = (pw - 2.0 * m) / LAY_W; let page_h = (ph - 2.0 * m) / k;
    let style = Style { text: Color::from_rgb8(INK[0], INK[1], INK[2]), link: Color::from_rgb8(0x05, 0x63, 0xc1), syntax: mn_layout::palette(&mn_layout::syntax::LIGHT), ..style.clone() };
    let mut fonts = Fonts::new(); let lay = NoteLayout::build(doc, &mut fonts, &style, LAY_W, 1.0);
    let mut p = Paper { doc, lay: &lay, fonts, faces: HashMap::new(), atoms: vec![], spans: vec![], inserts: vec![], squeezes: vec![], units: vec![], row_unit: HashMap::new(), wide: HashMap::new() };
    // a table wider than the page, at the top of the note: drawn smaller (inside columns it is left as it is)
    for id in doc.children(None) { if let (Some((full, vis, _)), Some(r)) = (lay.table_scroll(*id), lay.rect(doc, *id)) { if full > vis + 0.5 { let f = (vis / full).max(0.25); p.wide.insert(*id, (r.x0 as f32, f)); p.squeezes.push((r.y0 as f32, r.y1 as f32, f)); } } }
    // the note's name, quietly, over the first page
    if !title.is_empty() { let end = p.caption(title, Family::Mono, 11.0, 400.0, DIM, 0.0, LAY_W, 1.0, -34.0, 0.0, None); p.atoms.push(Atom { anchor: -34.0, off: end + 6.0, h: 1.0, unit: None, draws: vec![Draw::Line { x0: 0.0, y0: 0.5, x1: LAY_W, y1: 0.5, color: RULE, width: 0.7 }] }); }
    for b in doc.walk() {
        let Some(r) = lay.rect(doc, b.id) else { continue }; let (x0, f) = p.scale_of(b.id); let sx = |x: f64| x0 + (x as f32 - x0) * f;
        match &b.kind {
            Kind::Divider => { let unit = p.unit_of(b.id); p.atoms.push(Atom { anchor: r.y0 as f32, off: 0.0, h: r.height() as f32 * f, unit, draws: vec![Draw::Line { x0: sx(r.x0), y0: r.height() as f32 * f / 2.0, x1: sx(r.x1), y1: r.height() as f32 * f / 2.0, color: RULE, width: 0.8 }] }); }
            Kind::Media { descriptor } => { let it = src.item(descriptor, Mode::Paper, opts); p.media(b.id, &it, opts, page_h); }
            Kind::Table { header_rows, .. } => {
                for (i, row) in doc.children(Some(b.id)).iter().enumerate() { let Some(rr) = lay.rect(doc, *row) else { continue };
                    p.row_unit.insert(*row, p.units.len()); p.units.push((rr.y0 as f32, 0.0, rr.y1 as f32, 0.0));
                    let row_bg = match doc.get(*row).map(|x| &x.kind) { Some(Kind::Row { bg, .. }) => colour(bg).map(|c| tint(c, 0.3)), _ => None };
                    if let Some(c) = row_bg.or((i < *header_rows as usize).then_some(GROUND)) { p.spans.push(Span { x0: sx(rr.x0), x1: sx(rr.x1), y0: rr.y0 as f32, y1: rr.y1 as f32, fill: Some(c), stroke: None }); }
                    for cell in doc.children(Some(*row)) { let Some(cr) = lay.rect(doc, *cell) else { continue };
                        let bg = match doc.get(*cell).map(|x| &x.kind) { Some(Kind::Cell { bg, .. }) => colour(bg).map(|c| tint(c, 0.3)), _ => None };
                        p.spans.push(Span { x0: sx(cr.x0), x1: sx(cr.x1), y0: cr.y0 as f32, y1: cr.y1 as f32, fill: bg, stroke: Some(RULE) }); }
                }
            }
            k if k.is_text() => p.text_block(b.id),
            _ => {}
        }
    }
    // where everything stands on paper, then the pages: an atom that would cross a page's end starts the next page; a
    // row that would, and fits a page, starts it too
    let mut placed: Vec<(f32, f32, usize)> = p.atoms.iter().enumerate().map(|(i, a)| { let t = p.map(a.anchor) + a.off; (t, t + a.h, i) }).collect();
    placed.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    let units: Vec<(f32, f32)> = p.units.iter().map(|(a, ao, b, bo)| (p.map(*a) + ao, p.map(*b) + bo)).collect();
    let mut starts: Vec<f32> = vec![]; let mut seen: HashSet<usize> = HashSet::new(); let mut page_of = vec![0usize; p.atoms.len()];
    for (top, bottom, i) in &placed { let unit = p.atoms[*i].unit;
        match starts.last().copied() { None => starts.push(top.min(0.0)),
            Some(s) => { if bottom - s > page_h && *top > s + 0.5 { starts.push(*top); }
                else if let Some(u) = unit.filter(|u| !seen.contains(u)) { let (ut, ub) = units[u]; if ub - s > page_h && ub - ut <= page_h && ut > s + 0.5 { starts.push(ut.min(*top)); } } } }
        if let Some(u) = unit { seen.insert(u); } page_of[*i] = starts.len() - 1; }
    if starts.is_empty() { starts.push(0.0); }
    let mut out = Document::new(); out.set_metadata(krilla::metadata::Metadata::new().title(title.to_string()).creator("minNotes".into()));
    for (pi, start) in starts.iter().enumerate() {
        let end = starts.get(pi + 1).copied().unwrap_or(f32::INFINITY);
        let mut page = out.start_page_with(PageSettings::new(Size::from_wh(pw, ph).ok_or("page")?)); let mut links: Vec<(KRect, String)> = vec![];
        { let mut s = page.surface(); let px = |x: f32| m + x * k; let py = |y: f32| m + (y - start) * k;
            // what runs down the note, cut at this page: grounds first, then the grid
            let cut: Vec<(&Span, f32, f32, bool, bool)> = p.spans.iter().filter_map(|sp| { let (a, b) = (p.map(sp.y0), p.map(sp.y1)); (b > *start && a < end).then(|| (sp, a.max(*start), b.min(end), a >= *start, b <= end)) }).collect();
            for (sp, a, b, _, _) in &cut { if let (Some(c), Some(path)) = (sp.fill, rounded(px(sp.x0), py(*a), px(sp.x1), py(*b), 0.0)) { s.set_stroke(None); s.set_fill(Some(fill_of(c))); s.draw_path(&path); } }
            for (sp, a, b, top, bottom) in &cut { let Some(c) = sp.stroke else { continue }; let mut pb = PathBuilder::new(); let (x0, x1, y0, y1) = (px(sp.x0), px(sp.x1), py(*a), py(*b));
                pb.move_to(x0, y0); pb.line_to(x0, y1); pb.move_to(x1, y0); pb.line_to(x1, y1); if *top { pb.move_to(x0, y0); pb.line_to(x1, y0); } if *bottom { pb.move_to(x0, y1); pb.line_to(x1, y1); }
                if let Some(path) = pb.finish() { s.set_fill(None); s.set_stroke(Some(stroke_of(c, 0.5))); s.draw_path(&path); } }
            s.set_stroke(None);
            for (top, _, i) in placed.iter().filter(|(_, _, i)| page_of[*i] == pi) { let a = &p.atoms[*i]; let ay = |y: f32| py(top + y);
                for d in &a.draws { match d {
                    Draw::Glyphs { font, size, x, y, color, glyphs, text } => { s.set_stroke(None); s.set_fill(Some(fill_of(*color))); s.draw_glyphs(Point::from_xy(px(*x), ay(*y)), glyphs, p.faces[font].clone(), text, size * k, false); }
                    Draw::Fill { x0, y0, x1, y1, color, radius } => if let Some(path) = rounded(px(*x0), ay(*y0), px(*x1), ay(*y1), radius * k) { s.set_stroke(None); s.set_fill(Some(fill_of(*color))); s.draw_path(&path); },
                    Draw::Outline { x0, y0, x1, y1, color, radius, width } => if let Some(path) = rounded(px(*x0), ay(*y0), px(*x1), ay(*y1), radius * k) { s.set_fill(None); s.set_stroke(Some(stroke_of(*color, width * k))); s.draw_path(&path); s.set_stroke(None); },
                    Draw::Line { x0, y0, x1, y1, color, width } => { let mut pb = PathBuilder::new(); pb.move_to(px(*x0), ay(*y0)); pb.line_to(px(*x1), ay(*y1)); if let Some(path) = pb.finish() { s.set_fill(None); s.set_stroke(Some(stroke_of(*color, (width * k).max(0.35)))); s.draw_path(&path); s.set_stroke(None); } }
                    Draw::Disc { cx, cy, r, color, hollow } => { if let Some(path) = rounded(px(*cx) - r * k, ay(*cy) - r * k, px(*cx) + r * k, ay(*cy) + r * k, r * k) { if *hollow { s.set_fill(None); s.set_stroke(Some(stroke_of(*color, 0.8 * k))); } else { s.set_stroke(None); s.set_fill(Some(fill_of(*color))); } s.draw_path(&path); s.set_stroke(None); } }
                    Draw::Tick { pts, width } => { let mut pb = PathBuilder::new(); pb.move_to(px(pts[0].0), ay(pts[0].1)); pb.line_to(px(pts[1].0), ay(pts[1].1)); pb.line_to(px(pts[2].0), ay(pts[2].1)); if let Some(path) = pb.finish() { s.set_fill(None); s.set_stroke(Some(stroke_of([255, 255, 255], width * k))); s.draw_path(&path); s.set_stroke(None); } }
                    Draw::Picture { img, x, y, w, h } => if let Some(size) = Size::from_wh(w * k, h * k) { s.push_transform(&Transform::from_translate(px(*x), ay(*y))); s.draw_image(img.clone(), size); s.pop(); },
                    Draw::Link { x0, y0, x1, y1, url } => if let Some(r) = KRect::from_ltrb(px(*x0), ay(*y0), px(*x1), ay(*y1)) { links.push((r, url.clone())); },
                } }
            }
            s.finish(); }
        for (r, url) in links { use krilla::{action::{Action, LinkAction}, annotation::{Annotation, LinkAnnotation, Target}}; page.add_annotation(Annotation::new_link(LinkAnnotation::new(r, Target::Action(Action::Link(LinkAction::new(url)))), None)); }
        page.finish();
    }
    out.finish().map_err(|e| format!("{e:?}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Fake;
    impl Source for Fake { fn item(&mut self, _: &str, mode: Mode, _: Opts) -> Item { assert_eq!(mode, Mode::Paper);
        Item { what: What::File, name: "brief.txt".into(), path: "/vol/brief.txt".into(), meta: "2 KB".into(), dw: None, problem: None, shots: vec![], notes: vec![] } } }
    #[test]
    fn a_long_note_is_cut_between_lines_onto_pages_of_text() {
        let mut d = Doc::new(); let mut i = 0;
        let mut put = |d: &mut Doc, k: Kind, t: String| { let id = BlockId::new(); d.insert(None, i, id, k, t).unwrap(); i += 1; id };
        put(&mut d, Kind::Heading { level: 1 }, "Plan".into());
        for n in 0..60 { put(&mut d, Kind::Paragraph, format!("Paragraph {n}: the grade on the wide shot holds up well against the reference, and the titles sit inside the safe area on every format except the vertical cut.")); }
        put(&mut d, Kind::Task { depth: 0, state: 2 }, "done".into()); put(&mut d, Kind::Numbered { depth: 0 }, "first".into()); put(&mut d, Kind::Code { lang: String::new() }, "let a = 1;".into()); put(&mut d, Kind::Media { descriptor: "{\"kind\":\"file\",\"name\":\"brief.txt\"}".into() }, String::new());
        let t = std::time::Instant::now(); let b = pdf(&d, "My note", &Style::default(), Opts::default(), false, &mut Fake).unwrap(); let ms = t.elapsed().as_millis();
        assert!(b.starts_with(b"%PDF-") && b.len() > 10_000, "{} bytes", b.len()); assert!(ms < 2000, "{ms} ms");
        if let Some(p) = std::env::var_os("MN_EXPORT_SAMPLE") { std::fs::write(p, &b).unwrap(); }
    }
}
