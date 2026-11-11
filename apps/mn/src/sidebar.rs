//! The sidebar: the collection's tree (canon rows 271, 290) in a rail on the left, as QCView's rails. A click opens a
//! note or folds a folder; a double click renames; the window creates and deletes. Colours are placeholders.
use std::{collections::HashSet, time::Instant};
use kurbo::{Affine, Rect, RoundedRect};
use min_ui::{fonts::draw_layout, rail::{Rail, Side}, Button, Event, Family, FieldResponse, Fonts, KeyEvent, TextField};
use mn_doc::BlockId;
use mn_store::{Collection, DocKind};
use peniko::{Color, Fill};
use vello::Scene;

const ROW: f64 = 28.0;
const INDENT: f64 = 16.0;
const BG: Color = Color::from_rgb8(0x25, 0x25, 0x25);
const HOVER: Color = Color::from_rgb8(0x30, 0x30, 0x30);
const SELECTED: Color = Color::from_rgb8(0x1F, 0x4E, 0x8C);
const TEXT: Color = Color::from_rgb8(0xE0, 0xE0, 0xE0);
const DIM: Color = Color::from_rgb8(0x8A, 0x8A, 0x8A);

pub struct Row { pub id: BlockId, pub parent: Option<BlockId>, pub depth: usize, pub kind: DocKind, pub title: String, pub named: bool, pub has_children: bool }
pub enum Act { None, Redraw, Open(BlockId), Renamed(BlockId, String) }

pub struct Sidebar {
    pub rail: Rail, pub rows: Vec<Row>, folded: HashSet<BlockId>, pub selected: Option<BlockId>,
    /// keys go to the tree (Enter renames, Delete deletes) rather than to the note
    pub focused: bool,
    hover: Option<usize>, rename: Option<(BlockId, TextField)>, last_down: Option<(Instant, BlockId)>, scale: f64,
}

impl Sidebar {
    pub fn new() -> Sidebar { let mut rail = Rail::new(Side::Left, 240.0, false); rail.settle(); rail.shortcut = if cfg!(target_os = "macos") { "⌘1" } else { "Ctrl+1" }.into();
        Sidebar { rail, rows: vec![], folded: HashSet::new(), selected: None, focused: false, hover: None, rename: None, last_down: None, scale: 1.0 } }
    /// Reads the tree again (after a create, a rename, a delete, a merge).
    pub fn set_tree(&mut self, col: &Collection) {
        fn walk(col: &Collection, parent: Option<BlockId>, depth: usize, folded: &HashSet<BlockId>, out: &mut Vec<Row>) {
            for e in col.list(parent).unwrap_or_default() {
                let kids = col.list(Some(e.id)).map(|k| !k.is_empty()).unwrap_or(false);
                // a note without a name of its own is called by its first line (row 290)
                let (title, named) = if !e.title.is_empty() { (e.title.clone(), true) } else { let t = if e.kind == DocKind::Note { col.first_line(e.id).unwrap_or_default() } else { String::new() }; (if t.is_empty() { "Untitled".to_string() } else { t }, false) };
                out.push(Row { id: e.id, parent, depth, kind: e.kind, title, named, has_children: kids });
                if !folded.contains(&e.id) { walk(col, Some(e.id), depth + 1, folded, out); }
            }
        }
        let mut rows = vec![]; walk(col, None, 0, &self.folded, &mut rows); self.rows = rows;
    }
    pub fn row(&self, id: BlockId) -> Option<&Row> { self.rows.iter().find(|r| r.id == id) }
    /// Places the rail; returns the width it takes of the window, physical px.
    pub fn layout(&mut self, height: f64, scale: f64, now: Instant) -> f64 {
        self.scale = scale; self.rail.layout(0.0, 0.0, height, scale, now);
        self.rail.content_height = self.rows.len() as f64 * ROW * scale;
        self.rail.visible_width(now, scale)
    }
    fn row_rect(&self, i: usize) -> Rect { let c = self.rail.content_rect(); let y = c.y0 + i as f64 * ROW * self.scale - self.rail.scroll; Rect::new(c.x0, y, c.x1, y + ROW * self.scale) }
    fn row_at(&self, x: f64, y: f64) -> Option<usize> { let c = self.rail.content_rect(); if !c.contains(kurbo::Point::new(x, y)) { return None } let i = ((y - c.y0 + self.rail.scroll) / (ROW * self.scale)).floor(); (i >= 0.0 && (i as usize) < self.rows.len()).then_some(i as usize) }
    pub fn renaming(&self) -> bool { self.rename.is_some() }
    pub fn start_rename(&mut self, id: BlockId, fonts: &mut Fonts) {
        let Some(r) = self.row(id) else { return }; let text = if r.named { r.title.clone() } else { String::new() };
        let mut f = TextField::new(Family::Sans, 13.0); f.placeholder = r.title.clone(); f.set_text_end(&text, fonts); f.select_all(fonts); f.focused = true;
        self.rename = Some((id, f)); self.selected = Some(id); self.focused = true;
    }
    /// A pointer event. `true` in the second place: the event was the sidebar's (the note must not act on it).
    pub fn event(&mut self, e: &Event, fonts: &mut Fonts, now: Instant) -> (Act, bool) {
        let r = self.rail.event(e, now);
        if r.consumed || self.rail.is_dragging() { return (Act::Redraw, true) }
        let Some((x, y)) = e.pos() else { let had = self.hover.take().is_some(); return (if had || r.dirty { Act::Redraw } else { Act::None }, false) };
        let inside = self.rail.hit(x, y);
        if let Some((id, f)) = self.rename.as_mut() {
            if matches!(e, Event::Down { .. }) && !f.hit(x, y) { let (id, text) = (*id, f.text().trim().to_string()); self.rename = None; return (Act::Renamed(id, text), inside) }
            if f.pointer(e, fonts) { return (Act::Redraw, true) }
        }
        if !inside { let had = self.hover.take().is_some(); return (if had || r.dirty { Act::Redraw } else { Act::None }, false) }
        let at = self.row_at(x, y);
        match e {
            Event::Move { .. } => { let changed = self.hover != at; self.hover = at; (if changed || r.dirty { Act::Redraw } else { Act::None }, true) }
            Event::Down { button: Button::Primary, .. } => {
                self.focused = true;
                let Some(i) = at else { return (Act::Redraw, true) };
                let (id, kind, kids) = (self.rows[i].id, self.rows[i].kind, self.rows[i].has_children);
                let double = self.last_down.is_some_and(|(t, d)| d == id && t.elapsed().as_millis() < 400);
                self.last_down = Some((now, id)); self.selected = Some(id);
                if double { self.start_rename(id, fonts); return (Act::Redraw, true) }
                match kind { DocKind::Note => (Act::Open(id), true), DocKind::Folder => { if kids { if !self.folded.remove(&id) { self.folded.insert(id); } } (Act::Open(id), true) } DocKind::Canvas => (Act::Redraw, true) }
            }
            _ => (Act::None, true),
        }
    }
    /// A key while a name is being edited. None: the key was not the sidebar's.
    pub fn key(&mut self, k: &KeyEvent, fonts: &mut Fonts) -> Option<Act> {
        let (id, f) = self.rename.as_mut()?;
        Some(match f.key(k, fonts) { FieldResponse::Accepted => { let (id, text) = (*id, f.text().trim().to_string()); self.rename = None; Act::Renamed(id, text) } FieldResponse::Cancelled => { self.rename = None; Act::Redraw } _ => Act::Redraw })
    }
    pub fn ime(&mut self, e: &min_ui::ImeEvent, fonts: &mut Fonts) -> bool { match self.rename.as_mut() { Some((_, f)) => { f.ime(e, fonts); true } None => false } }
    pub fn paint(&mut self, scene: &mut Scene, fonts: &mut Fonts, current: Option<BlockId>, now: Instant) {
        if self.rail.rect.width() < 1.0 { self.rail.paint_foreground(scene, fonts, self.scale, now); return }
        let s = self.scale;
        scene.fill(Fill::NonZero, Affine::IDENTITY, BG, None, &self.rail.rect);
        self.rail.paint_background(scene, now);
        self.rail.push_clip(scene);
        for i in 0..self.rows.len() {
            let r = self.row_rect(i); if r.y1 < self.rail.rect.y0 || r.y0 > self.rail.rect.y1 { continue }
            let row = &self.rows[i];
            let bg = if self.selected == Some(row.id) && self.focused { Some(SELECTED) } else if current == Some(row.id) { Some(HOVER) } else if self.hover == Some(i) { Some(HOVER) } else { None };
            if let Some(c) = bg { scene.fill(Fill::NonZero, Affine::IDENTITY, c, None, &RoundedRect::from_rect(r.inset((-2.0 * s, -1.0 * s)), 5.0 * s)); }
            let x = r.x0 + (8.0 + INDENT * row.depth as f64) * s;
            // a folder's fold mark; a note's dot; a canvas's square
            let (cx, cy) = (x + 5.0 * s, r.center().y);
            match row.kind {
                DocKind::Folder => { let open = !self.folded.contains(&row.id); let p = if open { [(cx - 4.0 * s, cy - 2.0 * s), (cx + 4.0 * s, cy - 2.0 * s), (cx, cy + 3.0 * s)] } else { [(cx - 2.0 * s, cy - 4.0 * s), (cx + 3.0 * s, cy), (cx - 2.0 * s, cy + 4.0 * s)] };
                    let mut path = kurbo::BezPath::new(); path.move_to(p[0]); path.line_to(p[1]); path.line_to(p[2]); path.close_path(); scene.fill(Fill::NonZero, Affine::IDENTITY, DIM, None, &path); }
                DocKind::Note => scene.fill(Fill::NonZero, Affine::IDENTITY, if current == Some(row.id) { TEXT } else { DIM }, None, &kurbo::Circle::new((cx, cy), 2.0 * s)),
                DocKind::Canvas => scene.stroke(&kurbo::Stroke::new(1.2 * s), Affine::IDENTITY, DIM, None, &Rect::new(cx - 3.5 * s, cy - 3.5 * s, cx + 3.5 * s, cy + 3.5 * s)),
            }
            let tx = x + 18.0 * s;
            if self.rename.as_ref().is_some_and(|(id, _)| *id == row.id) { continue }
            let color = if row.kind == DocKind::Canvas || !row.named && row.title == "Untitled" { DIM } else { TEXT };
            let l = fonts.layout_fit(&row.title, Family::Sans, (13.0 * s) as f32, if current == Some(row.id) { 700.0 } else { 400.0 }, color, (r.x1 - tx - 6.0 * s).max(10.0));
            draw_layout(scene, &l, Affine::translate((tx, r.y0 + (r.height() - l.height() as f64) / 2.0)));
        }
        if let Some((id, f)) = self.rename.as_mut() { if let Some(i) = self.rows.iter().position(|r| r.id == *id) {
            let r = { let c = self.rail.content_rect(); let y = c.y0 + i as f64 * ROW * s - self.rail.scroll; Rect::new(c.x0, y, c.x1, y + ROW * s) };
            let x = r.x0 + (8.0 + INDENT * self.rows[i].depth as f64 + 14.0) * s;
            f.layout(fonts, Rect::new(x, r.y0 + 2.0 * s, r.x1 - 2.0 * s, r.y1 - 2.0 * s), s); f.paint(scene, fonts, s);
        } }
        self.rail.pop_clip(scene);
        self.rail.paint_foreground(scene, fonts, s, now);
    }
}
