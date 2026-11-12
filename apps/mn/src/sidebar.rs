//! The sidebar: the collection's tree (canon rows 271, 290, 294) in a rail on the left, as QCView's rails. A click on a
//! row SELECTS it (and opens a note); a folder's caret folds and unfolds it and does nothing else; a double click
//! renames; the window creates and deletes. Sizes, colours and icons are the theme's — the tokens QCView's project panel uses.
use std::{collections::HashSet, time::Instant};
use kurbo::{Affine, Rect};
use min_ui::{action_button::{ActionButton, Response}, action_group::ActionGroup, tooltip::Tooltips, fonts::draw_layout, paint, rail::{Rail, Side}, Button, Event, Family, FieldResponse, Fonts, KeyEvent, TextField};
use min_theme::tokens as t;
use mn_doc::BlockId;
use mn_store::{Collection, DocKind};
use vello::Scene;

/// A row's height and the step from one row to the next, logical px (QCView's project rows).
const ROW: f64 = t::COMPONENT_HEIGHT_75 as f64 + t::RAIL_ROW_GAP as f64;
const INDENT: f64 = t::ICON_SIZE_SMALL as f64;
/// The header row's height with the gap under it, logical px.
const HEAD: f64 = t::COMPONENT_HEIGHT_75 as f64 + t::SPACING_100 as f64;
/// The caret's slot at the start of a row: a press here folds or unfolds, anywhere else on the row selects.
const CARET: f64 = t::RAIL_SECTION_CHEVRON_SIZE as f64;
/// A line of a search result under its note's row.
const SNIP: f64 = 22.0;

pub struct Row { pub id: BlockId, pub parent: Option<BlockId>, pub depth: usize, pub kind: DocKind, pub title: String, pub named: bool, pub has_children: bool }
pub enum Act { None, Redraw, /// the header's buttons
    NewNote, NewFolder, NewCanvas, Open(BlockId), Renamed(BlockId, String), /// fold or unfold this folder
    Fold(BlockId), /// a search result was chosen: the note, and the occurrence to go to (the word, which of its occurrences)
    Found(BlockId, Option<(String, usize)>) }
/// The keys the tree takes while it has the focus.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Nav { Up, Down, Left, Right }

pub struct Sidebar {
    /// a note's row that dragged blocks would go to: outlined (row 307)
    pub drop_row: Option<BlockId>,
    pub rail: Rail, pub rows: Vec<Row>, folded: HashSet<BlockId>, pub selected: Option<BlockId>,
    /// keys go to the tree (Enter renames, Delete deletes) rather than to the note
    pub focused: bool,
    /// the header: a search field for the collection (shown, not built yet), new note, new folder
    head: ActionGroup, search: Rect, tooltips: Tooltips,
    /// the search field (row 321), and - while something is typed in it - what was found (in place of the tree); `scanning`: notes are still being read
    /// a note's row opens at the RELEASE, not the press (a canvas is open: the row may be dragged onto it); the row pressed
    pub defer_open: bool, pub pressed_row: Option<BlockId>,
    pub query: TextField, pub results: Option<Vec<crate::search::Found>>, pub scanning: bool, rhover: Option<(usize, Option<usize>)>,
    hover: Option<usize>, rename: Option<(BlockId, TextField)>, last_down: Option<(Instant, BlockId)>, scale: f64,
}

impl Sidebar {
    pub fn new() -> Sidebar { let mut rail = Rail::new(Side::Left, 240.0, false); rail.settle(); rail.tab_centered = true; rail.header_extra = HEAD; rail.shortcut = if cfg!(target_os = "macos") { "⌘1" } else { "Ctrl+1" }.into();
        let key = |k: &str| if cfg!(target_os = "macos") { format!("{}\u{2318}N", k) } else { format!("Ctrl+{}N", if k.is_empty() { "" } else { "Shift+" }) };
        let head = ActionGroup::new(vec![ActionButton::new("note-pencil", &format!("New note ({})", key(""))), ActionButton::new("bounding-box", "New canvas"), ActionButton::new("folder-simple-plus", &format!("New folder ({})", key("\u{21e7}")))]).compact();
        let mut query = TextField::new(Family::Sans, t::RAIL_LABEL_SIZE); query.placeholder = "Search".into();
        Sidebar { defer_open: false, pressed_row: None, query, results: None, scanning: false, rhover: None, drop_row: None, head, search: Rect::ZERO, tooltips: Tooltips::default(), rail, rows: vec![], folded: HashSet::new(), selected: None, focused: false, hover: None, rename: None, last_down: None, scale: 1.0 } }
    /// Reads the tree again (after a create, a rename, a delete, a merge).
    pub fn set_tree(&mut self, col: &Collection) {
        fn walk(col: &Collection, parent: Option<BlockId>, depth: usize, folded: &HashSet<BlockId>, out: &mut Vec<Row>) {
            for e in col.list(parent).unwrap_or_default() {
                // only a folder shows what is under it (a canvas's own notes are the canvas's: they are not rows)
                let kids = e.kind == DocKind::Folder && col.list(Some(e.id)).map(|k| !k.is_empty()).unwrap_or(false);
                // a note without a name of its own is called by its first line (row 290)
                let (title, named) = if !e.title.is_empty() { (e.title.clone(), true) } else { let t = if e.kind == DocKind::Note { col.first_line(e.id).unwrap_or_default() } else { String::new() }; (if t.is_empty() { "Untitled".to_string() } else { t }, false) };
                out.push(Row { id: e.id, parent, depth, kind: e.kind, title, named, has_children: kids });
                if e.kind == DocKind::Folder && !folded.contains(&e.id) { walk(col, Some(e.id), depth + 1, folded, out); }
            }
        }
        let mut rows = vec![]; walk(col, None, 0, &self.folded, &mut rows); self.rows = rows;
    }
    pub fn toggle_fold(&mut self, id: BlockId) { if !self.folded.remove(&id) { self.folded.insert(id); } }
    /// An arrow key with the tree focused: Up / Down move the selection (a note is opened as it is reached), Left folds
    /// a folder or goes to the parent, Right unfolds.
    pub fn nav(&mut self, n: Nav) -> Act {
        let Some(i) = self.selected.and_then(|s| self.rows.iter().position(|r| r.id == s)) else { self.selected = self.rows.first().map(|r| r.id); return Act::Redraw };
        let go = |me: &mut Sidebar, j: usize| { let r = &me.rows[j]; me.selected = Some(r.id); if r.kind == DocKind::Note { Act::Open(r.id) } else { Act::Redraw } };
        match n {
            Nav::Up => if i > 0 { go(self, i - 1) } else { Act::None },
            Nav::Down => if i + 1 < self.rows.len() { go(self, i + 1) } else { Act::None },
            Nav::Left => { let r = &self.rows[i]; if r.kind == DocKind::Folder && r.has_children && !self.folded.contains(&r.id) { Act::Fold(r.id) } else if let Some(p) = r.parent.and_then(|p| self.rows.iter().position(|x| x.id == p)) { go(self, p) } else { Act::None } }
            Nav::Right => { let r = &self.rows[i]; if r.kind == DocKind::Folder && self.folded.contains(&r.id) { Act::Fold(r.id) } else { Act::None } }
        }
    }
    pub fn row(&self, id: BlockId) -> Option<&Row> { self.rows.iter().find(|r| r.id == id) }
    /// Places the rail; returns the width it takes of the window, physical px.
    pub fn layout(&mut self, top: f64, height: f64, scale: f64, now: Instant) -> f64 {
        self.scale = scale; self.rail.layout(0.0, top, height, scale, now);
        self.rail.content_height = match &self.results { Some(r) => r.iter().map(|f| ROW + f.snippets.len() as f64 * SNIP + t::SPACING_75 as f64).sum::<f64>().max(ROW) * scale, None => self.rows.len() as f64 * ROW * scale };
        self.head.rect = Rect::ZERO; self.search = Rect::ZERO;
        self.rail.visible_width(now, scale)
    }
    fn row_rect(&self, i: usize) -> Rect { let c = self.rail.content_rect(); let y = c.y0 + i as f64 * ROW * self.scale - self.rail.scroll; Rect::new(c.x0, y, c.x1, y + ROW * self.scale) }
    fn row_at(&self, x: f64, y: f64) -> Option<usize> { let c = self.rail.content_rect(); if !c.contains(kurbo::Point::new(x, y)) { return None } let i = ((y - c.y0 + self.rail.scroll) / (ROW * self.scale)).floor(); (i >= 0.0 && (i as usize) < self.rows.len()).then_some(i as usize) }
    /// The row under a window point (the context menu's target).
    pub fn id_at(&self, x: f64, y: f64) -> Option<BlockId> { self.row_at(x, y).map(|i| self.rows[i].id) }
    /// The pointer is over the name being edited (the I-beam's place in the tree).
    pub fn over_field(&self, x: f64, y: f64) -> bool { self.rename.as_ref().is_some_and(|(_, f)| f.hit(x, y)) }
    pub fn renaming(&self) -> bool { self.rename.is_some() }
    /// One of its fields has the keys (a name being edited, the search).
    pub fn typing(&self) -> bool { self.rename.is_some() || self.query.focused }
    /// The search field takes the keys.
    pub fn search_focus(&mut self, fonts: &mut Fonts) { self.query.focused = true; self.focused = false; self.query.select_all(fonts); }
    /// The rows of the results: (the row's box, the result, the snippet - None: the note's own row).
    fn result_rows(&self) -> Vec<(Rect, usize, Option<usize>)> {
        let Some(res) = &self.results else { return vec![] }; let c = self.rail.content_rect(); let s = self.scale; let mut y = c.y0 - self.rail.scroll; let mut out = vec![];
        for (i, f) in res.iter().enumerate() { out.push((Rect::new(c.x0, y, c.x1, y + ROW * s), i, None)); y += ROW * s;
            for k in 0..f.snippets.len() { out.push((Rect::new(c.x0, y, c.x1, y + SNIP * s), i, Some(k))); y += SNIP * s; } y += t::SPACING_75 as f64 * s; }
        out
    }
    pub fn start_rename(&mut self, id: BlockId, fonts: &mut Fonts) {
        let Some(r) = self.row(id) else { return }; let text = if r.named { r.title.clone() } else { String::new() };
        let mut f = TextField::new(Family::Sans, t::RAIL_LABEL_SIZE); f.placeholder = r.title.clone(); f.set_text_end(&text, fonts); f.select_all(fonts); f.focused = true;
        self.rename = Some((id, f)); self.selected = Some(id); self.focused = true;
    }
    /// A pointer event. `true` in the second place: the event was the sidebar's (the note must not act on it).
    pub fn event(&mut self, e: &Event, fonts: &mut Fonts, now: Instant) -> (Act, bool) {
        let r = self.rail.event(e, now);
        // the rail reports EVERY event inside it as consumed (its panel routes the rest): only its own parts — the
        // fold arrow and tab, the splitter, the scrollbar — are taken here; the rows get what is left
        let own = r.toggled || r.width_changed || self.rail.is_dragging() || self.rail.splitter_hovered() || self.rail.scrollbar_hovered() || e.pos().is_some_and(|(x, y)| self.rail.over_control(x, y, now));
        if own { return (Act::Redraw, true) }
        // the header's buttons
        if self.rail.rect.width() > 1.0 {
            let clicked = self.head.event(e, now).into_iter().find(|(_, r)| *r == Response::Clicked).map(|(i, _)| i);
            if matches!(e, Event::Down { .. }) { self.tooltips.pressed(now); }
            let over = self.head.buttons.iter().enumerate().find(|(_, b)| b.hovered()).map(|(i, b)| (i as u64, b.rect, b.tooltip.clone()))
;
            self.tooltips.hover(over.as_ref().map(|(k, r, s)| (*k, *r, s.as_str())), now);
            match clicked { Some(0) => return (Act::NewNote, true), Some(1) => return (Act::NewCanvas, true), Some(_) => return (Act::NewFolder, true), None => {} }
            // the search field: a press in it gives it the keys; a press anywhere else takes them away
            if let Event::Down { x, y, .. } = *e { if self.search.contains(kurbo::Point::new(x, y)) { self.query.focused = true; self.focused = false; self.query.pointer(e, fonts); return (Act::Redraw, true) } else if self.query.focused { self.query.focused = false; } }
            else if self.query.focused && self.query.pointer(e, fonts) { return (Act::Redraw, true) }
            if e.pos().is_some_and(|(x, y)| self.rail.hit(x, y) && y < self.rail.rect.y0 + self.rail.header_height()) { return (Act::Redraw, true) }
        }
        let Some((x, y)) = e.pos() else { let had = self.hover.take().is_some(); return (if had || r.dirty { Act::Redraw } else { Act::None }, false) };
        let inside = self.rail.hit(x, y);
        if let Some((id, f)) = self.rename.as_mut() {
            if matches!(e, Event::Down { .. }) && !f.hit(x, y) { let (id, text) = (*id, f.text().trim().to_string()); self.rename = None; return (Act::Renamed(id, text), inside) }
            if f.pointer(e, fonts) { return (Act::Redraw, true) }
        }
        if !inside { let had = self.hover.take().is_some() | self.rhover.take().is_some(); return (if had || r.dirty { Act::Redraw } else { Act::None }, false) }
        // what was found stands in the tree's place: a press on a note's row opens it, on a line of it goes to that occurrence
        if let Some(res) = &self.results {
            let at = if self.rail.content_rect().contains(kurbo::Point::new(x, y)) { self.result_rows().into_iter().find(|(q, ..)| q.contains(kurbo::Point::new(x, y))).map(|(_, i, k)| (i, k)) } else { None };
            return match e { Event::Move { .. } => { let ch = self.rhover != at; self.rhover = at; (if ch || r.dirty { Act::Redraw } else { Act::None }, true) }
                Event::Down { button: Button::Primary, .. } => match at.and_then(|(i, k)| res.get(i).map(|f| (f, k))) { Some((f, k)) => { let go = k.and_then(|k| f.snippets.get(k)).or(f.snippets.first()).map(|sn| (sn.term.clone(), sn.ord)); (Act::Found(f.id, go), true) } None => (Act::Redraw, true) },
                _ => (Act::None, true) } }
        let at = self.row_at(x, y);
        match e {
            Event::Move { .. } => { let changed = self.hover != at; self.hover = at; (if changed || r.dirty { Act::Redraw } else { Act::None }, true) }
            Event::Down { button: Button::Primary, .. } => {
                self.focused = true;
                let Some(i) = at else { return (Act::Redraw, true) };
                let (id, kind, depth) = (self.rows[i].id, self.rows[i].kind, self.rows[i].depth);
                // the caret folds and unfolds, and does nothing else: the selection stays where it was
                let caret_x0 = self.row_rect(i).x0 + INDENT * depth as f64 * self.scale;
                if kind == DocKind::Folder && x >= caret_x0 && x < caret_x0 + (CARET + t::SPACING_75 as f64) * self.scale { self.last_down = None; return (Act::Fold(id), true) }
                let double = self.last_down.is_some_and(|(t, d)| d == id && t.elapsed().as_millis() < 400);
                self.last_down = Some((now, id)); self.selected = Some(id);
                if double { self.start_rename(id, fonts); return (Act::Redraw, true) }
                // the row selects; a note is also opened by it
                match kind { DocKind::Note if self.defer_open => { self.pressed_row = Some(id); (Act::Redraw, true) } DocKind::Note | DocKind::Canvas => (Act::Open(id), true), _ => (Act::Redraw, true) }
            }
            // a deferred open: the release on the row that was pressed
            Event::Up { button: Button::Primary, .. } => match (self.pressed_row.take(), at.map(|i| self.rows[i].id)) { (Some(p), Some(id)) if p == id => (Act::Open(id), true), _ => (Act::None, true) },
            _ => (Act::None, true),
        }
    }
    /// A key while a name is being edited. None: the key was not the sidebar's.
    pub fn key(&mut self, k: &KeyEvent, fonts: &mut Fonts) -> Option<Act> {
        // the search field: Escape empties it and gives the keys back, Enter opens the first note found
        if self.query.focused && self.rename.is_none() { return Some(match self.query.key(k, fonts) {
            FieldResponse::Cancelled => { self.query.clear(); self.query.focused = false; Act::Redraw }
            FieldResponse::Accepted => match self.results.as_ref().and_then(|r| r.first()) { Some(f) => Act::Found(f.id, f.snippets.first().map(|sn| (sn.term.clone(), sn.ord))), None => Act::Redraw },
            _ => Act::Redraw }) }
        let (id, f) = self.rename.as_mut()?;
        Some(match f.key(k, fonts) { FieldResponse::Accepted => { let (id, text) = (*id, f.text().trim().to_string()); self.rename = None; Act::Renamed(id, text) } FieldResponse::Cancelled => { self.rename = None; Act::Redraw } _ => Act::Redraw })
    }
    pub fn ime(&mut self, e: &min_ui::ImeEvent, fonts: &mut Fonts) -> bool { match self.rename.as_mut() { Some((_, f)) => { f.ime(e, fonts); true } None if self.query.focused => { self.query.ime(e, fonts); true } None => false } }
    pub fn tick(&mut self, now: Instant) -> bool { self.tooltips.tick(now) || self.head.animating(now) }
    pub fn next_change(&self, now: Instant) -> Option<Instant> { self.tooltips.next_change(now) }
    pub fn paint_tooltip(&self, scene: &mut Scene, fonts: &mut Fonts, bounds: Rect, now: Instant) { self.tooltips.paint(scene, fonts, self.scale, bounds, now); }
    /// `changed`: the notes with something that is not in the document yet (a dot at the row's end).
    pub fn paint(&mut self, scene: &mut Scene, fonts: &mut Fonts, current: Option<BlockId>, changed: &HashSet<BlockId>, now: Instant) {
        if self.rail.rect.width() < 1.0 { self.rail.paint_foreground(scene, fonts, self.scale, now); return }
        let s = self.scale;
        self.rail.paint_background(scene, now);
        // the header: [ search … ] [new note][new folder], clipped to the rail as it slides
        {
            let (pad, hh) = (t::RAIL_PADDING as f64 * s, t::COMPONENT_HEIGHT_75 as f64 * s);
            let (x0, x1, y) = (self.rail.rect.x0 + pad, self.rail.rect.x1 - pad, self.rail.rect.y0 + pad);
            scene.push_clip_layer(peniko::Fill::NonZero, Affine::IDENTITY, &self.rail.rect);
            let w = self.head.layout(fonts, 0.0, 0.0, s); let bh = self.head.rect.height();
            self.head.layout(fonts, x1 - w, y + (hh - bh) / 2.0, s);
            self.search = Rect::new(x0, y, (x1 - w - t::SPACING_100 as f64 * s).max(x0), y + hh);
            let field = self.search; self.query.layout(fonts, field, s); self.query.paint(scene, fonts, s);
            self.head.paint(scene, fonts, s, now);
            scene.pop_layer();
        }
        self.rail.push_clip(scene);
        let icon = t::ICON_SIZE_SMALL as f64 * s;
        // what was found, in the tree's place (row 321): a row a note - its name and how often - and up to three lines of it, the words found in the stronger colour
        if let Some(res) = &self.results {
            let rows = self.result_rows(); let (dim, strong) = (paint::color(t::NEUTRAL_SUBDUED_CONTENT_COLOR_DEFAULT), paint::color(t::NEUTRAL_CONTENT_COLOR_DEFAULT));
            if res.is_empty() { let c = self.rail.content_rect(); let l = fonts.layout_fit(if self.scanning { "Searching\u{2026}" } else { "No results" }, Family::Sans, t::RAIL_LABEL_SIZE * s as f32, t::FONT_WEIGHT_REGULAR as f32, dim, c.width()); draw_layout(scene, &l, Affine::translate((c.x0 + t::SPACING_75 as f64 * s, c.y0 + (ROW * s - l.height() as f64) / 2.0))); }
            for (r, i, k) in rows { if r.y1 < self.rail.rect.y0 || r.y0 > self.rail.rect.y1 { continue } let f = &res[i]; let mid = |h: f64| r.y0 + (r.height() - h) / 2.0;
                if self.rhover == Some((i, k)) { paint::rounded(scene, r, t::CORNER_RADIUS_SMALL_DEFAULT as f64 * s, t::HOVER_BACKGROUND_COLOR); } else if k.is_none() && current == Some(f.id) { paint::rounded(scene, r, t::CORNER_RADIUS_SMALL_DEFAULT as f64 * s, t::RAIL_ROW_ACTIVE_FILL); }
                match k {
                    None => { let mut x = r.x0 + t::SPACING_75 as f64 * s; let label = if current == Some(f.id) { t::RAIL_ROW_ACTIVE_LABEL } else { t::RAIL_LABEL_COLOR };
                        if let Some(g) = fonts.icon("note", icon as f32, paint::color(label)) { draw_layout(scene, &g, Affine::translate((x, mid(g.height() as f64)))); x += icon + t::TEXT_TO_VISUAL_75 as f64 * s; }
                        let n = fonts.layout(&f.count.to_string(), Family::Sans, t::RAIL_LABEL_SIZE * s as f32, t::FONT_WEIGHT_REGULAR as f32, dim); let nx = r.x1 - t::SPACING_100 as f64 * s - n.width() as f64;
                        if f.count > 0 { draw_layout(scene, &n, Affine::translate((nx, mid(n.height() as f64)))); }
                        let l = fonts.layout_fit(&f.title, Family::Sans, t::RAIL_LABEL_SIZE * s as f32, t::FONT_WEIGHT_BOLD as f32, paint::color(label), (nx - x - t::SPACING_75 as f64 * s).max(10.0)); draw_layout(scene, &l, Affine::translate((x, mid(l.height() as f64)))); }
                    Some(k) => { let sn = &f.snippets[k]; let size = (t::RAIL_LABEL_SIZE - 1.0) * s as f32; let mut x = r.x0 + (t::SPACING_75 as f64 + t::ICON_SIZE_SMALL as f64 + t::TEXT_TO_VISUAL_75 as f64) * s;
                        scene.push_clip_layer(peniko::Fill::NonZero, Affine::IDENTITY, &Rect::new(r.x0, r.y0, r.x1 - t::SPACING_75 as f64 * s, r.y1));
                        for (piece, col, weight) in [(&sn.text[..sn.bold.0], dim, t::FONT_WEIGHT_REGULAR), (&sn.text[sn.bold.0..sn.bold.1], strong, t::FONT_WEIGHT_BOLD), (&sn.text[sn.bold.1..], dim, t::FONT_WEIGHT_REGULAR)] { if piece.is_empty() { continue }
                            let l = fonts.layout(piece, Family::Sans, size, weight as f32, col); draw_layout(scene, &l, Affine::translate((x, mid(l.height() as f64)))); x += l.width() as f64; }
                        scene.pop_layer(); }
                }
            }
            self.rail.pop_clip(scene); self.rail.paint_foreground(scene, fonts, s, now); return
        }
        for i in 0..self.rows.len() {
            let r = self.row_rect(i); if r.y1 < self.rail.rect.y0 || r.y0 > self.rail.rect.y1 { continue }
            let r = Rect::new(r.x0, r.y0, r.x1, r.y1 - t::RAIL_ROW_GAP as f64 * s);
            let row = &self.rows[i];
            // QCView's project rows: the open one filled, a selected one in the neutral selection, a hovered one lighter
            let (active, selected) = (current == Some(row.id), self.selected == Some(row.id) && self.focused);
            let bg = if active { Some(t::RAIL_ROW_ACTIVE_FILL) } else if selected { Some(t::NEUTRAL_SELECTED_BACKGROUND_COLOR_DEFAULT) } else if self.hover == Some(i) { Some(t::HOVER_BACKGROUND_COLOR) } else { None };
            if let Some(c) = bg { paint::rounded(scene, r, t::CORNER_RADIUS_SMALL_DEFAULT as f64 * s, c); }
            if self.drop_row == Some(row.id) { scene.stroke(&kurbo::Stroke::new(1.5 * s), Affine::IDENTITY, paint::color(t::ACCENT_BACKGROUND_COLOR_DEFAULT), None, &kurbo::RoundedRect::from_rect(r.inset(-0.75 * s), t::CORNER_RADIUS_SMALL_DEFAULT as f64 * s)); }
            let label = if !row.named && row.title == "Untitled" && row.kind != DocKind::Canvas { t::DISABLED_CONTENT_COLOR } else if active { t::RAIL_ROW_ACTIVE_LABEL } else { t::RAIL_LABEL_COLOR };
            let mut x = r.x0 + INDENT * row.depth as f64 * s;
            let mid = |h: f64| r.y0 + (r.height() - h) / 2.0;
            // the caret (folders with something in them), then the kind's icon, then the name
            if row.kind == DocKind::Folder && row.has_children { if let Some(g) = fonts.icon(if self.folded.contains(&row.id) { "caret-right" } else { "caret-down" }, (CARET * s) as f32, paint::color(t::RAIL_GROUP_HEADER_COLOR)) { draw_layout(scene, &g, Affine::translate((x, mid(g.height() as f64)))); } }
            x += (CARET + t::SPACING_75 as f64) * s;
            let slug = match row.kind { DocKind::Note => Some("note"), DocKind::Canvas => Some("bounding-box"), DocKind::Folder => None };
            if let Some(g) = slug.and_then(|g| fonts.icon(g, icon as f32, paint::color(label))) { draw_layout(scene, &g, Affine::translate((x, mid(g.height() as f64)))); x += icon + t::TEXT_TO_VISUAL_75 as f64 * s; }
            if self.rename.as_ref().is_some_and(|(id, _)| *id == row.id) { continue }
            // something of this note is not in the document yet
            let dot = 6.0 * s; let has_dot = changed.contains(&row.id);
            if has_dot { scene.fill(peniko::Fill::NonZero, Affine::IDENTITY, paint::color(label), None, &kurbo::Circle::new((r.x1 - t::SPACING_100 as f64 * s - dot / 2.0, r.y0 + r.height() / 2.0), dot / 2.0)); }
            let l = fonts.layout_fit(&row.title, Family::Sans, t::RAIL_LABEL_SIZE * s as f32, t::FONT_WEIGHT_REGULAR as f32, paint::color(label), (r.x1 - x - t::SPACING_75 as f64 * s - if has_dot { dot + t::SPACING_100 as f64 * s } else { 0.0 }).max(10.0));
            draw_layout(scene, &l, Affine::translate((x, mid(l.height() as f64))));
        }
        if let Some((id, f)) = self.rename.as_mut() { if let Some(i) = self.rows.iter().position(|r| r.id == *id) {
            let r = { let c = self.rail.content_rect(); let y = c.y0 + i as f64 * ROW * s - self.rail.scroll; Rect::new(c.x0, y, c.x1, y + ROW * s) };
            let x = r.x0 + (INDENT * self.rows[i].depth as f64 + CARET + t::SPACING_75 as f64 + if self.rows[i].kind == DocKind::Folder { 0.0 } else { t::ICON_SIZE_SMALL as f64 + t::TEXT_TO_VISUAL_75 as f64 }) * s;
            f.layout(fonts, Rect::new(x - t::SPACING_75 as f64 * s, r.y0, r.x1, r.y1 - t::RAIL_ROW_GAP as f64 * s), s); f.paint(scene, fonts, s);
        } }
        self.rail.pop_clip(scene);
        self.rail.paint_foreground(scene, fonts, s, now);
    }
}
