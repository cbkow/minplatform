//! minNotes — the layout of a note (Phase 3 step 2). Given a [`mn_doc::Doc`], a width and a [`Style`], every block
//! gets a box: text leaves are shaped by parley, Columns put their columns side by side, a Table sizes its columns and
//! rows. The result answers the questions an editor and the desktop text element (row 281) ask: where is this caret,
//! which rectangles cover this range, what is under this point, how tall is the note.
//!
//! No drawing and no window: the layout is data, and it is tested without a GPU.
//!
//! Coordinates are physical pixels, y down, the origin at the note's top-left; a box's `x`/`y` are relative to its
//! PARENT's box, so a block that grows moves its later siblings and resizes its ancestors, nothing else.
use std::{borrow::Cow, collections::HashMap};
use kurbo::Rect;
use min_ui::fonts::{Family, Fonts};
use mn_doc::{Block, BlockId, ColumnSpec, Doc, FlatText, Kind, Mark};
use parley::{Affinity, Alignment, AlignmentOptions, Cursor, FontFamily, FontFamilyName, FontStyle, FontWeight, Layout, LineHeight, Selection, StyleProperty};
use peniko::{Brush, Color};

/// The metrics, in LOGICAL pixels (multiplied by the scale at layout). The defaults are the Qt app's numbers
/// (`docs/phase3/step2-layout-spec.md` §1–§9) — a starting point, not a ruling: how the page looks is settled when it
/// can be seen. The Qt app pads every block 6 px above and below with no margins between; here that is a gap of 12.
#[derive(Clone, Debug, PartialEq)]
pub struct Style {
    pub body: f32,
    /// 500 in the Qt app; the bundled Geist has 400 and 700 only, so this shapes as 400 until a Medium face is added
    pub body_weight: f32,
    /// line height as a multiple of the font size
    pub line_height: f32,
    /// font sizes of heading levels 1…6
    pub heading: [f32; 6],
    pub heading_weight: f32,
    pub code: f32,
    /// vertical gap between two blocks
    pub block_gap: f32,
    /// the gap between two list items of the same list
    pub list_gap: f32,
    /// extra space above a heading that is not the first block
    pub heading_above: f32,
    /// indent per list depth, and the room the marker takes left of the text
    pub list_indent: f32,
    pub marker: f32,
    /// the room left of a NUMBERED item's text (the number is right-aligned in it)
    pub marker_numbered: f32,
    /// a quote's bar + gap, left of its text
    pub quote_inset: f32,
    /// padding inside a code block, all sides
    pub code_pad: f32,
    /// the height of a divider's box (its line is drawn in the middle)
    pub divider: f32,
    pub column_gutter: f32,
    /// padding inside a table cell, all sides
    pub cell_pad: f32,
    /// an automatic table column: its widest line + padding, within these; `table_col_default` when nothing measures
    pub table_col_min: f32,
    pub table_col_max: f32,
    pub table_col_default: f32,
    /// what an automatic column adds to its widest line
    pub table_col_extra: f32,
    /// the box of a media item that has no picture size (a file, audio, a bookmark)
    pub media_chip: f32,
    pub text: Color,
    pub link: Color,
}
impl Default for Style {
    fn default() -> Self {
        Style { body: 14.0, body_weight: 500.0, line_height: 1.35, heading: [33.0, 28.0, 24.0, 20.0, 18.0, 16.0], heading_weight: 300.0, code: 13.0, block_gap: 12.0, list_gap: 12.0, heading_above: 0.0, list_indent: 24.0, marker: 22.0, marker_numbered: 34.0,
            quote_inset: 22.0, code_pad: 12.0, divider: 30.0, column_gutter: 24.0, cell_pad: 8.0, table_col_min: 48.0, table_col_max: 360.0, table_col_default: 160.0, table_col_extra: 22.0, media_chip: 56.0,
            text: Color::from_rgb8(0xEB, 0xEB, 0xEB), link: Color::from_rgb8(0x4D, 0xA3, 0xFF) }
    }
}

/// What a box holds.
pub enum Part {
    /// Shaped text; its origin inside the box is (`dx`, `dy`) (the room left of it is the marker's, the quote bar's…).
    Text { layout: Layout<Brush>, dx: f32, dy: f32 },
    Media,
    Divider,
    Container,
}
pub struct Box { pub x: f32, pub y: f32, pub w: f32, pub h: f32, pub part: Part }

pub struct NoteLayout { boxes: HashMap<BlockId, Box>, pub width: f32, pub height: f32, pub scale: f32, pub style: Style }

/// `#rgb`, `#rrggbb` or `#rrggbbaa` (the colours the file stores).
pub fn hex(s: &str) -> Option<Color> {
    let h = s.strip_prefix('#')?; let n = u32::from_str_radix(h, 16).ok()?;
    match h.len() { 3 => Some(Color::from_rgb8((((n >> 8) & 15) * 17) as u8, (((n >> 4) & 15) * 17) as u8, ((n & 15) * 17) as u8)), 6 => Some(Color::from_rgb8((n >> 16) as u8, (n >> 8) as u8, n as u8)), 8 => Some(Color::from_rgba8((n >> 24) as u8, (n >> 16) as u8, (n >> 8) as u8, n as u8)), _ => None }
}

/// The named colours (canon row 301): a note stores the NAME, the theme says what it looks like. These are the dark
/// theme's, taken from the Qt app's presets so that its users find their colours again.
pub const NAMED: [(&str, &str); 8] = [("red", "#FF5768"), ("orange", "#FFA23A"), ("yellow", "#FFEC59"), ("green", "#86E36B"), ("teal", "#00CDAC"), ("blue", "#00A5E3"), ("purple", "#9D8DF1"), ("pink", "#FF60A8")];
/// A stored colour of either spelling: a name of [`NAMED`] or a hex.
pub fn colour(s: &str) -> Option<Color> { NAMED.iter().find(|(n, _)| *n == s).and_then(|(_, h)| hex(h)).or_else(|| hex(s)) }
/// A colour BEHIND text (a highlight, a fill) is drawn as a tint — this much of it over the page — so that the text
/// on it is drawn exactly as it would be without it.
pub const TINT: f32 = 0.35;
pub fn tint(s: &str) -> Option<Color> { colour(s).map(|c| c.with_alpha(TINT)) }

fn media_size(descriptor: &str) -> Option<(f32, f32, Option<f32>)> {
    let v: serde_json::Value = serde_json::from_str(descriptor).ok()?;
    let (w, h) = (v.get("w")?.as_f64()? as f32, v.get("h")?.as_f64()? as f32);
    (w > 0.0 && h > 0.0).then_some((w, h, v.get("dw").and_then(|d| d.as_f64()).map(|d| d as f32)))
}

impl NoteLayout {
    /// Lays the whole note out at `width` physical pixels.
    pub fn build(doc: &Doc, fonts: &mut Fonts, style: &Style, width: f32, scale: f32) -> NoteLayout {
        let mut l = NoteLayout { boxes: HashMap::with_capacity(doc.len()), width, height: 0.0, scale, style: style.clone() };
        for id in doc.children(None) { l.assign(doc, fonts, *id, 0.0, width); }
        l.height = l.stack(doc, None);
        l
    }
    fn px(&self, v: f32) -> f32 { v * self.scale }

    /// Top-down: gives every block its x and width, shapes the leaves (which fixes their height), and stacks each
    /// container once its children are known.
    fn assign(&mut self, doc: &Doc, fonts: &mut Fonts, id: BlockId, x: f32, w: f32) {
        let Some(b) = doc.get(id) else { return };
        let part = match &b.kind {
            Kind::Columns => {
                let cols = doc.children(Some(id)); let n = cols.len().max(1) as f32;
                let free = (w - self.px(self.style.column_gutter) * (n - 1.0)).max(n);
                let shares: Vec<f32> = cols.iter().map(|c| match doc.get(*c).map(|c| &c.kind) { Some(Kind::Column { width }) if *width > 0.0 => *width, _ => 1.0 / n }).collect();
                let total: f32 = shares.iter().sum::<f32>().max(f32::EPSILON); let mut cx = 0.0;
                for (c, share) in cols.iter().zip(shares) { let cw = free * share / total; self.assign(doc, fonts, *c, cx, cw); cx += cw + self.px(self.style.column_gutter); }
                Part::Container
            }
            Kind::Column { .. } => { for c in doc.children(Some(id)) { self.assign(doc, fonts, *c, 0.0, w); } Part::Container }
            Kind::Table { columns, .. } => {
                let widths = self.column_widths(doc, fonts, id, columns);
                for row in doc.children(Some(id)) {
                    let mut cx = 0.0;
                    for (i, cell) in doc.children(Some(*row)).iter().enumerate() {
                        let cw = widths.get(i).copied().unwrap_or(self.px(self.style.table_col_default));
                        let pad = self.px(self.style.cell_pad);
                        for c in doc.children(Some(*cell)) { self.assign(doc, fonts, *c, pad, (cw - 2.0 * pad).max(1.0)); }
                        self.boxes.insert(*cell, Box { x: cx, y: 0.0, w: cw, h: 0.0, part: Part::Container }); self.stack(doc, Some(*cell));
                        cx += cw;
                    }
                    self.boxes.insert(*row, Box { x: 0.0, y: 0.0, w: cx, h: 0.0, part: Part::Container }); self.stack(doc, Some(*row));
                }
                self.boxes.insert(id, Box { x, y: 0.0, w: widths.iter().sum(), h: 0.0, part: Part::Container }); self.stack(doc, Some(id));
                return
            }
            Kind::Row { .. } | Kind::Cell { .. } => return,   // placed by their table
            Kind::Divider => { self.boxes.insert(id, Box { x, y: 0.0, w, h: self.px(self.style.divider), part: Part::Divider }); return }
            Kind::Media { descriptor } => {
                let (bw, bh) = match media_size(descriptor) { Some((mw, mh, dw)) => { let shown = dw.map(|d| self.px(d)).unwrap_or(self.px(mw)).min(w).max(1.0); (shown, shown * mh / mw) } None => (w, self.px(self.style.media_chip)) };
                self.boxes.insert(id, Box { x, y: 0.0, w: bw, h: bh, part: Part::Media }); return
            }
            _ => { let (layout, dx, dy, h) = self.shape(fonts, b, Some(w)); self.boxes.insert(id, Box { x, y: 0.0, w, h, part: Part::Text { layout, dx, dy } }); return }
        };
        self.boxes.insert(id, Box { x, y: 0.0, w, h: 0.0, part }); self.stack(doc, Some(id));
    }

    /// A table's column widths: a manual width, or the widest unwrapped line of the column's cells plus padding,
    /// within the style's bounds.
    fn column_widths(&mut self, doc: &Doc, fonts: &mut Fonts, table: BlockId, columns: &[ColumnSpec]) -> Vec<f32> {
        let rows = doc.children(Some(table)); let n = rows.iter().map(|r| doc.children(Some(*r)).len()).max().unwrap_or(0).max(columns.len());
        (0..n).map(|i| {
            if let Some(w) = columns.get(i).and_then(|c| c.width) { return self.px(w as f32) }
            let mut widest: f32 = 0.0;
            for row in rows { if let Some(cell) = doc.children(Some(*row)).get(i) { for c in doc.children(Some(*cell)) { if let Some(b) = doc.get(*c).filter(|b| b.kind.is_text() && !b.text.is_empty()) { let (l, dx, _, _) = self.shape(fonts, b, None); widest = widest.max(l.full_width() + dx); } } } }
            if widest == 0.0 { self.px(self.style.table_col_default) } else { (widest + self.px(self.style.table_col_extra)).round().clamp(self.px(self.style.table_col_min), self.px(self.style.table_col_max)) }
        }).collect()
    }

    /// Shapes a text block for `width` (None: unwrapped). Returns the layout, the text's origin inside the box and
    /// the box's height.
    fn shape(&self, fonts: &mut Fonts, b: &Block, width: Option<f32>) -> (Layout<Brush>, f32, f32, f32) {
        let st = &self.style;
        let (size, weight, family) = match &b.kind { Kind::Heading { level } => (st.heading[(*level as usize).clamp(1, 6) - 1], st.heading_weight, Family::Sans), Kind::Code { .. } => (st.code, 400.0, Family::Mono), _ => (st.body, st.body_weight, Family::Sans) };
        let (dx, pad) = match &b.kind {
            Kind::Bullet { depth } | Kind::Task { depth, .. } => (self.px(st.list_indent * *depth as f32 + st.marker), 0.0),
            Kind::Numbered { depth } => (self.px(st.list_indent * *depth as f32 + st.marker_numbered), 0.0),
            Kind::Quote => (self.px(st.quote_inset), 0.0),
            Kind::Code { .. } => (self.px(st.code_pad), self.px(st.code_pad)),
            _ => (0.0, 0.0),
        };
        let named = |f: Family| FontFamily::Single(FontFamilyName::Named(Cow::Owned(fonts.family_name(f).to_string())));
        let (sans_or_mono, mono) = (named(family), named(Family::Mono));
        let mut builder = fonts.layout_cx.ranged_builder(&mut fonts.font_cx, &b.text, 1.0, true);
        builder.push_default(StyleProperty::FontFamily(sans_or_mono));
        builder.push_default(StyleProperty::FontSize(self.px(size)));
        builder.push_default(StyleProperty::FontWeight(FontWeight::new(weight)));
        builder.push_default(StyleProperty::LineHeight(LineHeight::FontSizeRelative(st.line_height)));
        builder.push_default(StyleProperty::Brush(Brush::Solid(st.text)));
        for s in b.spans.as_slice() {
            let r = s.start.min(b.text.len())..s.end.min(b.text.len());
            match &s.mark {
                Mark::Bold => builder.push(StyleProperty::FontWeight(FontWeight::new(700.0)), r),
                Mark::Italic => builder.push(StyleProperty::FontStyle(FontStyle::Italic), r),
                Mark::Code => builder.push(StyleProperty::FontFamily(mono.clone()), r),
                Mark::Strike => builder.push(StyleProperty::Strikethrough(true), r),
                Mark::Underline => builder.push(StyleProperty::Underline(true), r),
                Mark::Link(_) => { builder.push(StyleProperty::Underline(true), r.clone()); builder.push(StyleProperty::Brush(Brush::Solid(st.link)), r) }
                Mark::Color(c) => { if let Some(c) = colour(c) { builder.push(StyleProperty::Brush(Brush::Solid(c)), r) } }
                // a highlight and a chip are drawn behind the text by the painter: they move nothing
                Mark::Highlight(_) | Mark::Choice(_) => {}
            }
        }
        let mut layout = builder.build(&b.text);
        layout.break_all_lines(width.map(|w| (w - dx - pad).max(1.0)));
        layout.align(Alignment::Start, AlignmentOptions::default());
        // an empty block is still one line tall
        let h = layout.height().max(self.px(size) * st.line_height) + 2.0 * pad;
        (layout, dx, pad, h)
    }

    /// The gap above `b` when it follows `prev` in a list of blocks.
    fn gap(&self, prev: &Kind, b: &Kind) -> f32 {
        let list = |k: &Kind| matches!(k, Kind::Bullet { .. } | Kind::Numbered { .. } | Kind::Task { .. });
        let st = &self.style;
        self.px(if list(prev) && list(b) { st.list_gap } else if matches!(b, Kind::Heading { .. }) { st.block_gap + st.heading_above } else { st.block_gap })
    }

    /// Bottom-up, one level: places the children of `parent` from their current sizes, sets the parent's height and
    /// returns it. (None: the note; returns its height.)
    fn stack(&mut self, doc: &Doc, parent: Option<BlockId>) -> f32 {
        let kids = doc.children(parent);
        let kind = parent.and_then(|p| doc.get(p)).map(|b| b.kind.clone());
        let h = match kind {
            Some(Kind::Columns) | Some(Kind::Row { .. }) => {
                let h = kids.iter().filter_map(|k| self.boxes.get(k)).map(|b| b.h).fold(0.0, f32::max);
                if matches!(kind, Some(Kind::Row { .. })) { for k in kids { if let Some(b) = self.boxes.get_mut(k) { b.h = h; } } }   // every cell is as tall as its row
                for k in kids { if let Some(b) = self.boxes.get_mut(k) { b.y = 0.0; } }
                h
            }
            _ => {
                let pad = if matches!(kind, Some(Kind::Cell { .. })) { self.px(self.style.cell_pad) } else { 0.0 };
                let mut y = pad; let mut prev: Option<&Kind> = None;
                for k in kids {
                    let Some(b) = doc.get(*k) else { continue };
                    if let Some(p) = prev { if !matches!(b.kind, Kind::Row { .. }) { y += self.gap(p, &b.kind); } }   // a table's rows touch
                    if let Some(bx) = self.boxes.get_mut(k) { bx.y = y; y += bx.h; }
                    prev = Some(&b.kind);
                }
                y + pad
            }
        };
        if let Some(p) = parent { if let Some(b) = self.boxes.get_mut(&p) { b.h = h; } }
        h
    }

    /// A text block's content changed (not the tree): it is shaped again, and whatever its new height moves is moved —
    /// its later siblings and the heights of its ancestors.
    pub fn update_block(&mut self, doc: &Doc, fonts: &mut Fonts, id: BlockId) {
        let (Some(b), Some(w)) = (doc.get(id), self.boxes.get(&id).map(|b| b.w)) else { return };
        if !b.kind.is_text() { return }
        let (layout, dx, dy, h) = self.shape(fonts, b, Some(w));
        let bx = self.boxes.get_mut(&id).unwrap();
        let grew = (bx.h - h).abs() > 0.01; bx.h = h; bx.part = Part::Text { layout, dx, dy };
        if !grew { return }
        let mut up = b.parent;
        loop { let h = self.stack(doc, up); match up { None => { self.height = h; break } Some(p) => up = doc.get(p).and_then(|b| b.parent) } }
    }

    pub fn get(&self, id: BlockId) -> Option<&Box> { self.boxes.get(&id) }
    /// A block's box in the note's coordinates.
    pub fn rect(&self, doc: &Doc, id: BlockId) -> Option<Rect> {
        let b = self.boxes.get(&id)?; let (mut x, mut y) = (b.x, b.y); let mut up = doc.get(id)?.parent;
        while let Some(p) = up { let pb = self.boxes.get(&p)?; x += pb.x; y += pb.y; up = doc.get(p)?.parent; }
        Some(Rect::new(x as f64, y as f64, (x + b.w) as f64, (y + b.h) as f64))
    }
    fn text_origin(&self, doc: &Doc, id: BlockId) -> Option<(&Layout<Brush>, f64, f64)> {
        let r = self.rect(doc, id)?;
        match &self.boxes.get(&id)?.part { Part::Text { layout, dx, dy } => Some((layout, r.x0 + *dx as f64, r.y0 + *dy as f64)), _ => None }
    }
    /// The caret's rectangle at byte `at` of a block (`width` physical px wide). For a block without text: its left
    /// edge (0) or its right edge (anything else), as tall as the block.
    pub fn caret(&self, doc: &Doc, id: BlockId, at: usize, width: f32) -> Option<Rect> { self.caret_side(doc, id, at, width, false) }
    /// As [`NoteLayout::caret`]; `upstream` puts a caret that stands exactly at a soft wrap at the END of the upper line
    /// (where End leaves it) instead of the start of the lower one.
    pub fn caret_side(&self, doc: &Doc, id: BlockId, at: usize, width: f32, upstream: bool) -> Option<Rect> {
        match self.text_origin(doc, id) {
            Some((layout, ox, oy)) => { let g = Cursor::from_byte_index(layout, at, if upstream { Affinity::Upstream } else { Affinity::Downstream }).geometry(layout, width); Some(Rect::new(ox + g.x0, oy + g.y0, ox + g.x1, oy + g.y1)) }
            None => { let r = self.rect(doc, id)?; let x = if at == 0 { r.x0 } else { r.x1 }; Some(Rect::new(x, r.y0, x + width as f64, r.y1)) }
        }
    }
    /// The rectangles covering bytes [start, end) of a block, one per line; the whole box for a block without text.
    pub fn range_rects(&self, doc: &Doc, id: BlockId, start: usize, end: usize) -> Vec<Rect> {
        if end <= start { return vec![] }
        match self.text_origin(doc, id) {
            Some((layout, ox, oy)) => {
                let sel = Selection::new(Cursor::from_byte_index(layout, start, Affinity::Downstream), Cursor::from_byte_index(layout, end, Affinity::Upstream));
                sel.geometry(layout).into_iter().map(|(g, _)| Rect::new(ox + g.x0, oy + g.y0, ox + g.x1, oy + g.y1)).collect()
            }
            None => self.rect(doc, id).into_iter().collect(),
        }
    }
    /// The rectangles covering a UTF-16 range of the note's flat text — what the desktop text element answers a tool
    /// that asks where a range is. A line break between two blocks has no rectangle of its own.
    pub fn flat_rects(&self, doc: &Doc, flat: &FlatText, start16: usize, len16: usize) -> Vec<Rect> {
        let (end16, mut out) = (start16 + len16, vec![]);
        for l in flat.lines.iter().filter(|l| l.start16 < end16 && l.start16 + l.len16 > start16) {
            let (a, b) = (start16.max(l.start16) - l.start16, end16.min(l.start16 + l.len16) - l.start16);
            if l.is_text { let t = &flat.text[l.start8..l.start8 + l.len8]; out.extend(self.range_rects(doc, l.block, mn_doc::text::to8(t, a), mn_doc::text::to8(t, b))); }
            else { out.extend(self.rect(doc, l.block)); }
        }
        out
    }
    /// What is under a point of the note: the block and the byte offset in it nearest to the point (for a block
    /// without text, 0 = before it, 1 = after it). A point in a gap, beside a block or below the note finds the
    /// nearest block; None only for an empty note.
    pub fn hit(&self, doc: &Doc, x: f64, y: f64) -> Option<(BlockId, usize)> {
        let (mut parent, mut ox, mut oy) = (None, 0.0f64, 0.0f64);
        loop {
            let kids = doc.children(parent);
            let across = matches!(parent.and_then(|p| doc.get(p)).map(|b| &b.kind), Some(Kind::Columns) | Some(Kind::Row { .. }));
            let pick = if across { kids.iter().find(|k| self.boxes.get(k).is_some_and(|b| x < ox + (b.x + b.w) as f64)).or(kids.last()) }
                       else { kids.iter().find(|k| self.boxes.get(k).is_some_and(|b| y < oy + (b.y + b.h) as f64)).or(kids.last()) };
            let id = *pick?; let b = self.boxes.get(&id)?;
            let (bx, by) = (ox + b.x as f64, oy + b.y as f64);
            match &b.part {
                Part::Text { layout, dx, dy } => return Some((id, Cursor::from_point(layout, (x - bx - *dx as f64) as f32, (y - by - *dy as f64) as f32).index())),
                Part::Media | Part::Divider => return Some((id, if x < bx + b.w as f64 / 2.0 { 0 } else { 1 })),
                Part::Container => { if doc.children(Some(id)).is_empty() { return None } parent = Some(id); ox = bx; oy = by; }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mn_doc::{ColumnKind, Spans};
    const LONG: &str = "The grade on the wide shot holds up well against the reference, and the titles sit inside the safe area on every format we checked this morning before the review.";
    fn add(d: &mut Doc, parent: Option<BlockId>, kind: Kind, text: &str) -> BlockId { let id = BlockId::new(); let at = d.children(parent).len(); d.insert(parent, at, id, kind, text).unwrap(); id }
    struct Sample { doc: Doc, h: BlockId, p: BlockId, b0: BlockId, b1: BlockId, cols: BlockId, left: BlockId, right: BlockId, table: BlockId, cells: Vec<BlockId>, media: BlockId, div: BlockId, last: BlockId }
    fn sample() -> Sample {
        let mut d = Doc::new();
        let h = add(&mut d, None, Kind::Heading { level: 1 }, "Review notes");
        let p = add(&mut d, None, Kind::Paragraph, LONG);
        let (b0, b1) = (add(&mut d, None, Kind::Bullet { depth: 0 }, "first"), add(&mut d, None, Kind::Bullet { depth: 1 }, "nested"));
        let cols = add(&mut d, None, Kind::Columns, "");
        let (c1, c2) = (add(&mut d, Some(cols), Kind::Column { width: 0.3 }, ""), add(&mut d, Some(cols), Kind::Column { width: 0.7 }, ""));
        let (left, right) = (add(&mut d, Some(c1), Kind::Paragraph, LONG), add(&mut d, Some(c2), Kind::Paragraph, "short"));
        let table = add(&mut d, None, Kind::Table { header_rows: 1, columns: vec![ColumnSpec { width: Some(200), ..Default::default() }, ColumnSpec { kind: ColumnKind::Text, ..Default::default() }] }, "");
        let mut cells = vec![];
        for row in [["Item", "State"], ["a longer first cell that has to wrap inside two hundred pixels", "ok"]] { let r = add(&mut d, Some(table), Kind::row(), ""); for t in row { let c = add(&mut d, Some(r), Kind::cell(), ""); add(&mut d, Some(c), Kind::Paragraph, t); cells.push(c); } }
        let media = add(&mut d, None, Kind::Media { descriptor: r#"{"w":400,"h":300}"#.into() }, "");
        let div = add(&mut d, None, Kind::Divider, "");
        let last = add(&mut d, None, Kind::Paragraph, "");
        Sample { doc: d, h, p, b0, b1, cols, left, right, table, cells, media, div, last }
    }
    fn lay(d: &Doc, f: &mut Fonts, width: f32) -> NoteLayout { NoteLayout::build(d, f, &Style::default(), width, 1.0) }

    #[test]
    fn blocks_stack_in_order_without_overlap_and_fill_the_notes_height() {
        let s = sample(); let mut f = Fonts::new(); let l = lay(&s.doc, &mut f, 760.0);
        let top: Vec<Rect> = s.doc.children(None).iter().map(|id| l.rect(&s.doc, *id).unwrap()).collect();
        assert!(top.windows(2).all(|w| w[0].y1 <= w[1].y0), "{top:?}");
        assert!(top.iter().all(|r| r.height() > 0.0 && r.x0 >= 0.0));
        assert_eq!(top.last().unwrap().y1 as f32, l.height);
        // the heading is taller than a body line; the long paragraph wraps at 760 and wraps more at 300
        let line = l.rect(&s.doc, s.last).unwrap().height();
        assert!(l.rect(&s.doc, s.h).unwrap().height() > line && l.rect(&s.doc, s.p).unwrap().height() > line * 1.5);
        let narrow = lay(&s.doc, &mut f, 300.0);
        assert!(narrow.rect(&s.doc, s.p).unwrap().height() > l.rect(&s.doc, s.p).unwrap().height() && narrow.height > l.height);
        // a nested item's text starts further right
        assert!(l.caret(&s.doc, s.b1, 0, 1.0).unwrap().x0 > l.caret(&s.doc, s.b0, 0, 1.0).unwrap().x0 && l.caret(&s.doc, s.b0, 0, 1.0).unwrap().x0 > 0.0);
        // a picture keeps its shape; a divider and an empty paragraph have a height
        let m = l.rect(&s.doc, s.media).unwrap(); assert!((m.width() - 400.0).abs() < 0.01 && (m.height() - 300.0).abs() < 0.01);
        assert!(l.rect(&s.doc, s.div).unwrap().height() > 0.0 && line > 0.0);
        assert_eq!(lay(&s.doc, &mut f, 200.0).rect(&s.doc, s.media).unwrap().width(), 200.0);
    }
    #[test]
    fn columns_sit_side_by_side_and_a_table_row_is_as_tall_as_its_tallest_cell() {
        let s = sample(); let mut f = Fonts::new(); let l = lay(&s.doc, &mut f, 760.0);
        let (a, b, c) = (l.rect(&s.doc, s.left).unwrap(), l.rect(&s.doc, s.right).unwrap(), l.rect(&s.doc, s.cols).unwrap());
        assert_eq!(a.y0, b.y0);
        assert!(a.x1 <= b.x0 && (b.x0 - a.x1 - 24.0).abs() < 0.5, "{a:?} {b:?}");                    // the gutter
        assert!((a.width() / b.width() - 0.3 / 0.7).abs() < 0.01);                                   // the shares
        assert_eq!(c.height(), a.height().max(b.height()));
        let r: Vec<Rect> = s.cells.iter().map(|c| l.rect(&s.doc, *c).unwrap()).collect();
        assert_eq!((r[0].width(), r[2].width()), (200.0, 200.0));                                    // the manual column
        assert!(r[1].width() >= 48.0 && r[1].width() < 200.0);                                       // the automatic one fits its widest line
        assert_eq!((r[0].y0, r[0].y1, r[2].y1), (r[1].y0, r[1].y1, r[3].y1));                        // cells of a row share its height
        assert!(r[2].height() > r[0].height() && r[0].y1 == r[2].y0 && r[0].x1 == r[1].x0);          // the wrapped cell; rows and cells touch
        assert!((l.rect(&s.doc, s.table).unwrap().y1 - r[3].y1).abs() < 0.01);
    }
    #[test]
    fn a_caret_a_range_and_a_point_agree() {
        let s = sample(); let mut f = Fonts::new(); let l = lay(&s.doc, &mut f, 400.0);
        for (id, at) in [(s.h, 3), (s.p, 0), (s.p, 90), (s.p, LONG.len()), (s.left, 40), (s.b1, 2), (s.last, 0)] {
            let c = l.caret(&s.doc, id, at, 1.0).unwrap(); let r = l.rect(&s.doc, id).unwrap();
            assert!(c.y0 >= r.y0 - 1.5 && c.y1 <= r.y1 + 1.5 && c.x0 >= r.x0 - 0.5 && c.x0 <= r.x1 + 1.5, "{id} {at}: {c:?} in {r:?}");
            assert_eq!(l.hit(&s.doc, c.x0 + 0.2, (c.y0 + c.y1) / 2.0), Some((id, at)), "block {id} byte {at}");
        }
        // a range over a wrap is one rectangle per line, inside the block
        let rects = l.range_rects(&s.doc, s.p, 10, 120);
        assert!(rects.len() >= 2 && rects.windows(2).all(|w| w[0].y1 <= w[1].y0 + 1.5 && w[0].y0 < w[1].y0));
        assert!(l.range_rects(&s.doc, s.p, 5, 5).is_empty());
        // points outside any block find the nearest one
        assert_eq!(l.hit(&s.doc, 5.0, l.height as f64 + 500.0).map(|h| h.0), Some(s.last));
        assert_eq!(l.hit(&s.doc, -50.0, 2.0), Some((s.h, 0)));
        let m = l.rect(&s.doc, s.media).unwrap();
        assert_eq!((l.hit(&s.doc, m.x0 + 3.0, m.y0 + 3.0), l.hit(&s.doc, m.x1 - 3.0, m.y0 + 3.0)), (Some((s.media, 0)), Some((s.media, 1))));
        assert_eq!(l.hit(&s.doc, l.rect(&s.doc, s.right).unwrap().x0 + 2.0, l.rect(&s.doc, s.right).unwrap().y0 + 2.0), Some((s.right, 0)));
        assert_eq!(lay(&Doc::new(), &mut f, 400.0).hit(&Doc::new(), 0.0, 0.0), None);
    }
    /// What the desktop text element asks (row 281): a UTF-16 range of the whole note → rectangles.
    #[test]
    fn a_range_of_the_flat_text_becomes_rectangles_across_blocks() {
        let s = sample(); let mut f = Fonts::new(); let l = lay(&s.doc, &mut f, 760.0); let flat = FlatText::build(&s.doc);
        let word = flat.text.find("grade").unwrap();                                                // ASCII so far: bytes = units
        let r = l.flat_rects(&s.doc, &flat, word, 5);
        assert_eq!(r.len(), 1);
        let p = l.rect(&s.doc, s.p).unwrap(); assert!(r[0].y0 >= p.y0 - 1.5 && r[0].y1 <= p.y1 + 1.5 && r[0].width() > 20.0);
        let from_heading = l.flat_rects(&s.doc, &flat, 7, 20);                                      // "notes" + the line break + the paragraph's start
        assert_eq!(from_heading.len(), 2);
        assert!(from_heading[0].y1 <= l.rect(&s.doc, s.h).unwrap().y1 + 1.5 && from_heading[1].y0 >= p.y0 - 1.5);
        let whole = l.flat_rects(&s.doc, &flat, 0, flat.len16);
        assert!(whole.iter().any(|x| *x == l.rect(&s.doc, s.media).unwrap()));                      // a media block answers with its box
    }
    #[test]
    fn typing_reshapes_one_block_and_moves_what_follows() {
        let mut s = sample(); let mut f = Fonts::new(); let mut l = lay(&s.doc, &mut f, 400.0);
        let before = |l: &NoteLayout, d: &Doc, id| l.rect(d, id).unwrap();
        let (p0, after0, cols0, h0) = (before(&l, &s.doc, s.left), before(&l, &s.doc, s.table), before(&l, &s.doc, s.cols), l.height);
        // the left column's paragraph doubles: the Columns block grows, everything below moves down by the same amount
        let longer = format!("{LONG} {LONG}"); s.doc.set_text(s.left, longer).unwrap(); s.doc.set_spans(s.left, Spans::new()).unwrap();
        l.update_block(&s.doc, &mut f, s.left);
        let grown = before(&l, &s.doc, s.left).height() - p0.height();
        assert!(grown > 20.0);
        assert!(((before(&l, &s.doc, s.table).y0 - after0.y0) - grown).abs() < 0.01 && ((before(&l, &s.doc, s.cols).height() - cols0.height()) - grown).abs() < 0.01 && ((l.height - h0) as f64 - grown).abs() < 0.01);
        assert_eq!(before(&l, &s.doc, s.h), lay(&s.doc, &mut f, 400.0).rect(&s.doc, s.h).unwrap());
        // and the incremental result is the same as laying the note out afresh
        let fresh = lay(&s.doc, &mut f, 400.0);
        for b in s.doc.walk() { assert_eq!(l.rect(&s.doc, b.id), fresh.rect(&s.doc, b.id), "{:?}", b.kind); }
        // a change that keeps the height moves nothing
        s.doc.set_text(s.h, "Review note").unwrap(); l.update_block(&s.doc, &mut f, s.h);
        assert_eq!(l.height, fresh.height);
    }
    #[test]
    fn formatting_changes_the_shaping_where_it_should() {
        let mut d = Doc::new(); let (a, b) = (add(&mut d, None, Kind::Paragraph, "some bold words here"), add(&mut d, None, Kind::Paragraph, "some bold words here"));
        let mut sp = Spans::new(); sp.put(5, 9, Mark::Bold); sp.put(10, 15, Mark::Code); sp.put(0, 4, Mark::Color("#ff0000".into())); d.set_spans(b, sp).unwrap();
        let mut f = Fonts::new(); let l = lay(&d, &mut f, 760.0);
        let end = |id| l.caret(&d, id, 20, 1.0).unwrap().x0;
        assert!(end(b) > end(a));                                                                    // bold and mono are wider
        assert_eq!(l.caret(&d, a, 4, 1.0).unwrap().x0, l.caret(&d, b, 4, 1.0).unwrap().x0);         // a colour moves nothing
        assert_eq!(l.rect(&d, a).unwrap().height(), l.rect(&d, b).unwrap().height());
    }
    /// Not a pass / fail test: the cost at the size the spike S9 used (about 50,000 words in 950 paragraphs).
    /// `cargo test -p mn-layout --release -- --ignored --nocapture timing`
    #[test]
    #[ignore]
    fn timing_at_fifty_thousand_words() {
        let mut d = Doc::new(); let mut ids = vec![];
        for i in 0..950 { let words: Vec<&str> = LONG.split(' ').cycle().skip(i % 7).take(53).collect(); ids.push(add(&mut d, None, Kind::Paragraph, &words.join(" "))); }
        let mut f = Fonts::new();
        let t = std::time::Instant::now(); let mut l = lay(&d, &mut f, 760.0); let full = t.elapsed();
        let mut keys = vec![];
        for k in 0..200 { let id = ids[10]; let mut text = d.get(id).unwrap().text.clone(); text.push(if k % 6 == 5 { ' ' } else { 'x' }); d.set_text(id, text).unwrap(); let t = std::time::Instant::now(); l.update_block(&d, &mut f, id); keys.push(t.elapsed().as_secs_f64() * 1e6); }
        keys.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let flat = FlatText::build(&d); let t = std::time::Instant::now(); let mut n = 0; for i in (0..flat.len16 - 20).step_by(97) { n += l.flat_rects(&d, &flat, i, 12).len(); } let per = t.elapsed().as_secs_f64() * 1e6 / (flat.len16 / 97) as f64;
        println!("{} words: full layout {:.1} ms; one key near the top (reshape + restack) p50 {:.0} us p95 {:.0} us; a 12-unit range to rectangles {:.1} us ({n} rects)", flat.text.split_whitespace().count(), full.as_secs_f64() * 1e3, keys[100], keys[190], per);
    }
}
