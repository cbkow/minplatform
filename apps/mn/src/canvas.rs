//! A canvas (canon row 322, stage 1 of the step; `docs/phase3/step-canvas-ink-spec.md` has the Qt app's): a document
//! of the collection that is a boundless sheet. Things are put on it - pictures, videos, PDFs and files (a note's
//! media records, drawn by the same pipeline) and text cards - and selected, moved, resized, stacked, copied and
//! removed; the sheet is panned and zoomed about the pointer. Its history is its own (row 275: one per document):
//! each step keeps the elements as they were.
//!
//! Units: an element's box is in the canvas's own units - a logical pixel at 100 %. On the screen a unit is
//! `zoom x scale` physical pixels; the camera is the unit at the view's centre.
use kurbo::{Affine, Point, Rect, RoundedRect, Stroke, Vec2};
use min_theme::tokens as t;
use min_ui::{action_button::{ActionButton, Response}, fonts::draw_layout, text_area::TextArea, Event, Family, FieldResponse, Fonts, KeyEvent};
use mn_doc::BlockId;
use mn_store::Element;
use peniko::{Color, Fill};
use std::time::Instant;
use vello::Scene;
use crate::media::{Media, View as Pic};
use crate::ink;
use qcv_notes::{ink::Smoother, strokes::{self, Stroke as Shape, Tool as Kind}};

pub const ZOOM: (f64, f64) = (0.05, 4.0);
const HANDLE: f64 = 9.0;
/// A frame's room round its members, and its title's strip (the canvas's units).
const FRAME_PAD: f64 = 24.0;
const FRAME_TITLE: f64 = 28.0;
/// Under this zoom a card is its box and its name only.
const FAR: f64 = 0.12;
fn c(k: min_theme::Color) -> Color { min_ui::paint::color(k) }

pub use crate::ink::Ink;

enum Act { /// a stroke being drawn: for the pen the smoother (fed the sheet's units / 1000 - QCView's units on a picture 1000 wide)
    /// and, after each sample, how many points it had made and the pen's pressure then; `pressed`: a pen that tells its pressure draws
    Draw { tool: Ink, from: Point, to: Point, sm: Option<Smoother>, marks: Vec<(usize, f64)>, pressed: bool, card: Option<BlockId> }, /// the eraser being rubbed (`stepped`: it has taken something - the history has its step)
    Rub { last: Point, stepped: bool, card: Option<BlockId> }, /// a frame being made with the frame tool
    Frame { from: Point, to: Point },
    Pan { from: (f64, f64), cam: (f64, f64) }, Move { from: Point, orig: Vec<(BlockId, f64, f64)>, moved: bool }, Resize { id: BlockId, left: bool, top: bool, orig: Rect, keep: bool, moved: bool }, Marquee { from: Point, to: Point, add: bool } }

pub struct Canvas {
    pub id: BlockId, pub els: Vec<Element>, pub sel: Vec<BlockId>,
    /// the unit at the view's centre, and the zoom; `fit`: the camera still follows the content (nobody moved it)
    cam: (f64, f64, f64), fit: bool,
    /// something changed since it was last written to the collection
    pub dirty: bool, pub rect: Rect, scale: f64,
    act: Option<Act>, undo: Vec<Vec<Element>>, redo: Vec<Vec<Element>>,
    /// a text card being typed in; the camera before a card was opened larger (Escape goes back to it)
    edit: Option<(BlockId, TextArea)>, back: Option<(f64, f64, f64)>,
    /// Space is held (a drag pans), the hand tool is on
    pub space: bool, pub hand: bool, last_click: Option<(Instant, Option<BlockId>)>,
    /// a note card's content height (the canvas's units), as last drawn: the card is no taller than what it holds
    fit_h: std::collections::HashMap<BlockId, f64>,
    /// asked for by a gesture, for the window to do: a note card is to be entered (its text edited in place); a note is to be made at this point of the sheet
    pub entered: Option<BlockId>, pub want_note: Option<Point>, /// a file's or a link's card is to be opened by the system
    pub opened: Option<BlockId>,
    zoom_out: ActionButton, zoom_pc: ActionButton, zoom_in: ActionButton, zoom_fit: ActionButton, b_hand: ActionButton,
    /// the armed drawing tool; the pen's colour and width (the sheet's units); the pen's pressure at the pointer event being
    /// handled (None: a mouse, or a pen that tells none)
    pub tool: Option<Ink>, pub pen: [f32; 4], pub pen_w: f64, pub pressure: Option<f64>,
    /// a picture's or a PDF's card that was entered (pressed twice): what is drawn inside it goes ON it - into its own record, as in a note (row 325)
    pub inked: Option<BlockId>,
    /// the frame tool is armed (row 327); Ctrl is held (a drag does not snap); Shift, as the pointer's last event had it;
    /// the lines a drag snapped to (true: upright, at this x); the frame the dragged cards would join
    pub framing: bool, pub ctrl: bool, shift: bool, guides: Vec<(bool, f64)>, drop_at: Option<BlockId>,
    hover: Option<(f64, f64)>, t0: Instant, /// what each stroke paints as (by its box's size and its record's length; a restyle and the history drop theirs)
    drawn: std::collections::HashMap<BlockId, ((u64, u64, usize), ink::Drawn)>,
}

/// A media record's kind (`file`, `pdf`, `video`; None: a picture).
fn kind_of(e: &Element) -> Option<String> { if e.kind != "media" { return None } serde_json::from_str::<serde_json::Value>(&e.record).ok().and_then(|v| v.get("kind").and_then(|x| x.as_str()).map(str::to_string)) }
fn name_of(e: &Element) -> String { if e.kind == "text" { e.record.lines().next().unwrap_or("").to_string() } else { serde_json::from_str::<serde_json::Value>(&e.record).ok().and_then(|v| v.get("name").and_then(|n| n.as_str()).map(str::to_string)).unwrap_or_default() } }
fn small(icon: &'static str, tip: &str) -> ActionButton { let mut b = ActionButton::new(icon, tip).size_s(); b.icon_size = Some(t::ICON_SIZE_SMALL); b }

impl Canvas {
    pub fn new(id: BlockId, c: mn_store::Canvas) -> Canvas {
        let mut pc = ActionButton::new("", "Zoom to 100 %").size_s(); pc.label = Some("100 %".into());
        Canvas { id, els: c.elements, sel: vec![], cam: c.camera.unwrap_or((0.0, 0.0, 1.0)), fit: c.camera.is_none(), dirty: false, rect: Rect::ZERO, scale: 1.0, act: None, undo: vec![], redo: vec![], edit: None, back: None,
            space: false, hand: false, last_click: None, fit_h: Default::default(), entered: None, want_note: None, opened: None, zoom_out: small("minus", "Zoom out"), zoom_pc: pc, zoom_in: small("plus", "Zoom in"), zoom_fit: small("arrows-in-simple", "Fit everything"), b_hand: small("hand", "Pan (hold Space)"), tool: None, pen: [1.0, 0.0, 0.0, 1.0], pen_w: 6.0, pressure: None, inked: None, framing: false, ctrl: false, shift: false, guides: vec![], drop_at: None, hover: None, t0: Instant::now(), drawn: Default::default() }
    }
    /// As the collection keeps it.
    pub fn stored(&self) -> mn_store::Canvas { mn_store::Canvas { elements: self.els.clone(), camera: (!self.fit).then_some(self.cam) } }
    pub fn can_undo(&self) -> bool { !self.undo.is_empty() }
    pub fn can_redo(&self) -> bool { !self.redo.is_empty() }
    pub fn editing(&self) -> bool { self.edit.is_some() }
    /// The text card's field, while one is typed in (a paste, a copy).
    pub fn field(&mut self) -> Option<&mut TextArea> { self.edit.as_mut().map(|(_, f)| f) }
    #[cfg(test)] pub fn zoom(&self) -> f64 { self.cam.2 }

    // -- the camera

    fn k(&self) -> f64 { self.cam.2 * self.scale }
    fn to_screen(&self, p: Point) -> Point { let m = self.rect.center(); Point::new(m.x + (p.x - self.cam.0) * self.k(), m.y + (p.y - self.cam.1) * self.k()) }
    pub fn to_world(&self, x: f64, y: f64) -> Point { let m = self.rect.center(); Point::new(self.cam.0 + (x - m.x) / self.k(), self.cam.1 + (y - m.y) / self.k()) }
    fn on_screen(&self, e: &Element) -> Rect { let a = self.to_screen(Point::new(e.x, e.y)); let h = if e.kind == "note" { self.fit_h.get(&e.id).map(|f| f.min(e.h)).unwrap_or(e.h) } else { e.h }; Rect::new(a.x, a.y, a.x + e.w * self.k(), a.y + h * self.k()) }
    /// An element's box in the window.
    pub fn rect_of(&self, id: BlockId) -> Option<Rect> { self.get(id).map(|e| self.on_screen(e)) }
    /// A note card's content is this tall (the canvas's units).
    pub fn set_fit(&mut self, id: BlockId, h: f64) { self.fit_h.insert(id, h); }
    /// The camera goes to a card at 100 %: its top in view, centred across (a note's text is edited at its own size).
    pub fn focus(&mut self, id: BlockId) { let Some(e) = self.get(id).cloned() else { return }; let (vw, vh) = (self.rect.width() / self.scale, self.rect.height() / self.scale); if self.back.is_none() { self.back = Some(self.cam); }
        let h = self.fit_h.get(&id).map(|f| f.min(e.h)).unwrap_or(e.h); self.cam = (e.x + e.w / 2.0, if h + 64.0 <= vh { e.y + h / 2.0 } else { e.y - 32.0 + vh / 2.0 }, 1.0); let _ = vw; self.fit = false; self.sel = vec![id]; }
    /// A note of the collection is put on the canvas as a card, its centre at a window point (None: the view's centre).
    pub fn add_note(&mut self, note: BlockId, at: Option<(f64, f64)>) -> BlockId { self.edit_end(true); self.step(); let id = self.add("note", note.to_string(), 560.0, 640.0, at, 0); if let Some(e) = self.els.iter_mut().find(|e| e.id == id) { if at.is_some() { e.y += 320.0 - 40.0; } } id }
    fn bounds(&self, ids: Option<&[BlockId]>) -> Option<Rect> { self.els.iter().filter(|e| ids.is_none_or(|i| i.contains(&e.id))).map(|e| Rect::new(e.x, e.y, e.x + e.w, e.y + e.h)).reduce(|a, b| a.union(b)) }
    /// The camera shows `r` (the canvas's units) whole, with a margin.
    fn show(&mut self, r: Rect) { let (w, h) = (self.rect.width() / self.scale, self.rect.height() / self.scale); if w < 1.0 || h < 1.0 { return }
        let z = ((w - 96.0) / r.width().max(1.0)).min((h - 96.0) / r.height().max(1.0)).clamp(ZOOM.0, 1.0f64.max(ZOOM.0)); self.cam = (r.center().x, r.center().y, z); }
    pub fn fit_all(&mut self) { match self.bounds(None) { Some(r) => self.show(r), None => self.cam = (0.0, 0.0, 1.0) } self.fit = true; }
    /// The zoom is multiplied, the unit under a window point staying under it.
    pub fn zoom_by(&mut self, f: f64, at: (f64, f64)) { let w = self.to_world(at.0, at.1); self.cam.2 = (self.cam.2 * f).clamp(ZOOM.0, ZOOM.1); let n = self.to_world(at.0, at.1); self.cam.0 += w.x - n.x; self.cam.1 += w.y - n.y; self.fit = false; }
    pub fn zoom_to(&mut self, z: f64) { let m = self.rect.center(); self.zoom_by(z / self.cam.2, (m.x, m.y)); }
    pub fn pan_by(&mut self, dx: f64, dy: f64) { self.cam.0 -= dx / self.k(); self.cam.1 -= dy / self.k(); self.fit = false; }

    // -- what is on it

    fn step(&mut self) { self.undo.push(self.els.clone()); if self.undo.len() > 200 { self.undo.remove(0); } self.redo.clear(); self.dirty = true; }
    pub fn undo(&mut self) { self.edit_end(true); if let Some(p) = self.undo.pop() { self.redo.push(std::mem::replace(&mut self.els, p)); self.after_history(); } }
    pub fn redo(&mut self) { self.edit_end(true); if let Some(p) = self.redo.pop() { self.undo.push(std::mem::replace(&mut self.els, p)); self.after_history(); } }
    fn after_history(&mut self) { self.dirty = true; self.drawn.clear(); let ids: Vec<BlockId> = self.els.iter().map(|e| e.id).collect(); self.sel.retain(|s| ids.contains(s)); }
    fn top_z(&self) -> i64 { self.els.iter().map(|e| e.z).max().unwrap_or(0) }
    fn get(&self, id: BlockId) -> Option<&Element> { self.els.iter().find(|e| e.id == id) }
    #[cfg(test)] fn get_mut(&mut self, id: BlockId) -> &mut Element { self.els.iter_mut().find(|e| e.id == id).unwrap() }
    /// The element in front at a window point.
    fn at(&self, x: f64, y: f64) -> Option<BlockId> { let (w, slack) = (self.to_world(x, y), 4.0 / self.cam.2);
        // a stroke is hit on its drawn line, not anywhere in its box; a frame on its title or its edge
        self.order().into_iter().rev().map(|i| &self.els[i]).find(|e| match e.kind.as_str() { "frame" => self.on_frame(e, x, y), "ink" => self.on_screen(e).contains(Point::new(x, y)) && ink::read(e).is_some_and(|l| ink::hit(&l, w, slack)), _ => self.on_screen(e).contains(Point::new(x, y)) }).map(|e| e.id) }
    /// Something is put on the canvas with its centre at a window point (None: the view's centre); it is selected.
    pub fn add(&mut self, kind: &str, record: String, w: f64, h: f64, at: Option<(f64, f64)>, nth: usize) -> BlockId {
        let m = self.rect.center(); let p = at.map(|(x, y)| self.to_world(x, y)).unwrap_or(self.to_world(m.x, m.y)); let off = nth as f64 * 24.0;
        let id = BlockId::new(); let z = self.top_z() + 1;
        self.els.push(Element { id, kind: kind.to_string(), x: (p.x - w / 2.0 + off).round(), y: (p.y - h / 2.0 + off).round(), w, h, z, parent: None, record, locked: false }); if nth == 0 { self.sel.clear(); } self.sel.push(id); self.adopt(&[id]); id
    }
    /// Media records (as a note's blocks have them) are put on the canvas side by side about a window point (None:
    /// the view's centre), one step of the history; they are selected.
    pub fn add_media(&mut self, records: Vec<String>, at: Option<(f64, f64)>) {
        if records.is_empty() { return } self.edit_end(true); self.step();
        let sized: Vec<(String, f64, f64)> = records.into_iter().map(|r| { let v: serde_json::Value = serde_json::from_str(&r).unwrap_or_default();
            let (w, h) = match (v.get("w").and_then(|x| x.as_f64()), v.get("h").and_then(|x| x.as_f64())) { (Some(w), Some(h)) if w > 0.0 && h > 0.0 => { let f = (480.0 / w).min(480.0 / h).min(1.0); ((w * f).round(), (h * f).round()) } _ => (280.0, 56.0) }; (r, w, h) }).collect();
        let m = self.rect.center(); let p = at.map(|(x, y)| self.to_world(x, y)).unwrap_or(self.to_world(m.x, m.y)); let gap = 24.0;
        let total: f64 = sized.iter().map(|x| x.1).sum::<f64>() + gap * (sized.len() - 1) as f64; let mut x = p.x - total / 2.0; self.sel.clear();
        for (r, w, h) in sized { let id = BlockId::new(); let z = self.top_z() + 1; self.els.push(Element { id, kind: "media".into(), x: x.round(), y: (p.y - h / 2.0).round(), w, h, z, parent: None, record: r, locked: false }); self.sel.push(id); x += w + gap; }
        let put = self.sel.clone(); self.adopt(&put);
        if self.fit { self.fit_all(); }
    }
    /// An address becomes a link card at the view's centre.
    pub fn add_link(&mut self, url: &str) { self.edit_end(true); self.step(); self.add("link", url.to_string(), 340.0, 52.0, None, 0); }
    /// The one selected PDF card shows the page after (or before) the one it shows. True: it did.
    pub fn pdf_page(&mut self, forward: bool) -> bool {
        let [id] = self.sel[..] else { return false }; let Some(e) = self.get(id).filter(|e| kind_of(e).as_deref() == Some("pdf")) else { return false };
        let Ok(serde_json::Value::Object(mut v)) = serde_json::from_str::<serde_json::Value>(&e.record) else { return false };
        let (page, pages) = (v.get("page").and_then(|p| p.as_u64()).unwrap_or(0) as i64, v.get("pages").and_then(|p| p.as_u64()).unwrap_or(1).max(1) as i64);
        let to = (page + if forward { 1 } else { -1 }).clamp(0, pages - 1); if to == page { return true }
        v.insert("page".into(), serde_json::json!(to)); let rec = serde_json::Value::Object(v).to_string(); self.step(); if let Some(e) = self.els.iter_mut().find(|e| e.id == id) { e.record = rec; } true
    }
    // -- a video's card (row 326): it plays in the card, on the window's one player

    pub fn camera(&self) -> (f64, f64, f64) { self.cam }
    /// A video card's picture in the window and its record.
    pub fn video_el(&self, id: BlockId) -> Option<(Rect, String)> { let e = self.get(id).filter(|e| kind_of(e).as_deref() == Some("video"))?; Some((self.on_screen(e), e.record.clone())) }
    /// Where a video card can play: its picture's place, while it is in view and not seen from far out.
    pub fn video_place(&self, id: BlockId) -> Option<Rect> { let (r, _) = self.video_el(id)?; (self.cam.2 >= FAR && r.width() >= 28.0 * self.scale && r.intersect(self.rect).area() > 0.0).then_some(r) }
    /// The strip under a video card (play, the frame, the line to scrub on): shown while the card is the one selected.
    pub fn video_strip(&self, id: BlockId) -> Option<Rect> { let (r, _) = self.video_el(id)?; let s = self.scale; Some(Rect::new(r.x0, r.y1 + 6.0 * s, r.x1.max(r.x0 + 300.0 * s), r.y1 + 6.0 * s + mn_layout::VIDEO_STRIP as f64 * s)) }
    pub fn selected_video(&self) -> Option<BlockId> { if self.edit.is_some() { return None } self.selected_media("video").map(|e| e.id) }
    /// The frame a video card stands on goes into its record (not a step of the history).
    pub fn set_frame(&mut self, id: BlockId, frame: i32) {
        let Some(e) = self.els.iter_mut().find(|e| e.id == id) else { return }; let Ok(serde_json::Value::Object(mut v)) = serde_json::from_str::<serde_json::Value>(&e.record) else { return };
        if v.get("frame").and_then(|f| f.as_i64()).unwrap_or(0) == frame as i64 { return } v.insert("frame".into(), serde_json::json!(frame)); e.record = serde_json::Value::Object(v).to_string(); self.dirty = true;
    }
    /// The one selected card, when it is of this kind of media (`file`, `pdf`, `video`).
    pub fn selected_media(&self, kind: &str) -> Option<&Element> { let [id] = self.sel[..] else { return None }; self.get(id).filter(|e| kind_of(e).as_deref() == Some(kind)) }
    /// Text from the clipboard becomes a text card at the view's centre.
    pub fn add_text(&mut self, text: &str) { self.edit_end(true); self.step(); let lines = text.lines().count().max(1) as f64; self.add("text", text.trim_end().to_string(), 280.0, (lines * 22.0 + 24.0).clamp(60.0, 480.0), None, 0); }
    /// A right press: the card under the pointer is selected, unless it is one of those selected already; none under it: nothing is.
    pub fn pick_at(&mut self, x: f64, y: f64) { self.edit_end(true); match self.at(x, y) { Some(id) => if !self.sel.contains(&id) { self.sel = vec![id]; }, None => self.sel.clear() } }
    pub fn select_all(&mut self) { self.sel = self.els.iter().filter(|e| !e.locked).map(|e| e.id).collect(); }
    /// The selected cards go (a frame with what is in it); a locked one stays.
    pub fn delete(&mut self) { let gone: Vec<BlockId> = self.units().into_iter().flat_map(|u| u.0).collect(); if gone.is_empty() { return } self.step(); self.sel.retain(|s| !gone.contains(s)); self.els.retain(|e| !gone.contains(&e.id)); self.hug(); }
    pub fn nudge(&mut self, dx: f64, dy: f64) { let ids: Vec<BlockId> = self.units().into_iter().flat_map(|u| u.0).collect(); if ids.is_empty() { return } self.step(); self.shift_els(&ids, dx, dy); self.hug(); }
    /// The selected elements, for the clipboard (a frame with what is in it).
    pub fn copy(&self) -> Vec<Element> { self.els.iter().filter(|e| self.sel.contains(&e.id) || self.frame_of(e).is_some_and(|f| self.sel.contains(&f))).cloned().collect() }
    /// Elements come in (a paste, a duplicate) a little down and to the right of where they were, in front, selected.
    pub fn paste(&mut self, clip: &[Element], shift: f64) {
        if clip.is_empty() { return } self.edit_end(true); self.step(); let mut z = self.top_z(); self.sel.clear(); let mut v: Vec<&Element> = clip.iter().collect(); v.sort_by_key(|e| e.z);
        // a frame's members come in as the new frame's; a member copied without its frame stays in that frame while it is there
        let ids: std::collections::HashMap<BlockId, BlockId> = v.iter().map(|e| (e.id, BlockId::new())).collect();
        for e in v { z += 1; let parent = e.parent.and_then(|p| ids.get(&p).copied().or(Some(p).filter(|p| self.is_frame(*p)))); self.els.push(Element { id: ids[&e.id], x: e.x + shift, y: e.y + shift, z, parent, locked: false, ..e.clone() }); if !e.parent.is_some_and(|p| ids.contains_key(&p)) { self.sel.push(ids[&e.id]); } }
        self.hug();
    }
    /// The selection goes in front of everything (or behind it).
    pub fn restack(&mut self, front: bool) {
        if self.sel.is_empty() { return } self.step(); let mut order: Vec<BlockId> = { let mut v: Vec<&Element> = self.els.iter().collect(); v.sort_by_key(|e| e.z); v.iter().map(|e| e.id).collect() };
        let (mut picked, rest): (Vec<BlockId>, Vec<BlockId>) = order.drain(..).partition(|id| self.sel.contains(id)); let mut all = if front { let mut r = rest; r.append(&mut picked); r } else { picked.extend(rest); picked };
        for (i, id) in all.drain(..).enumerate() { if let Some(e) = self.els.iter_mut().find(|e| e.id == id) { e.z = i as i64 + 1; } }
    }

    // -- ink (row 324)

    fn secs(&self) -> f64 { self.t0.elapsed().as_secs_f64() }
    /// A tool is armed (the same again, or None: put down - the pointer selects).
    pub fn arm(&mut self, tool: Option<Ink>) { self.edit_end(true); self.act = None; self.framing = false; self.tool = if self.tool == tool { None } else { tool }; if self.tool.is_some() { self.sel.clear(); } }
    /// The eraser's reach on the sheet: half the pen's width, and never less than five points on the screen.
    fn rub_r(&self) -> f64 { (self.pen_w / 2.0).max(5.0 / self.cam.2) }
    /// The stroke a gesture makes, in the sheet's units (`pen`: the pen's line so far). None: too small to be one.
    fn line_of(&self, tool: Ink, from: Point, to: Point, pen: Option<Vec<[f64; 2]>>, marks: &[(usize, f64)], pressed: bool) -> Option<ink::Line> { ink::line_of(tool, from, to, pen, marks, pressed, self.pen, self.pen_w, 2.0) }
    /// A picture's or a PDF's card as a page to draw on: its record, the page shown, the page's size in its own units, how
    /// many of the sheet's units one of those is, and the card's corner.
    fn card_page(&self, id: BlockId) -> Option<(serde_json::Value, usize, (f64, f64), f64, Vec2)> {
        let e = self.get(id).filter(|e| e.kind == "media" && matches!(kind_of(e).as_deref(), None | Some("pdf")))?; let rec: serde_json::Value = serde_json::from_str(&e.record).ok()?;
        let size = ink::page_size(&rec, Rect::new(e.x, e.y, e.x + e.w, e.y + e.h)); Some((ink::page_no(&rec), size, e.w / size.0, Vec2::new(e.x, e.y))).map(|(p, s, k, o)| (rec, p, s, k, o))
    }
    /// The strokes on a card's page, in the sheet's units; and put back from them.
    fn card_lines(&self, id: BlockId) -> Vec<ink::Line> { self.card_page(id).map(|(rec, page, size, k, off)| ink::page_lines(&rec, page, size).iter().map(|l| ink::placed(l, k, off)).collect()).unwrap_or_default() }
    fn card_set(&mut self, id: BlockId, lines: &[ink::Line]) {
        let Some((_, page, size, k, off)) = self.card_page(id) else { return }; let own: Vec<ink::Line> = lines.iter().map(|l| ink::placed(l, 1.0 / k, -off / k)).collect();
        if let Some(e) = self.els.iter_mut().find(|e| e.id == id) { if let Some(r) = ink::page_set(&e.record, page, size, &own) { e.record = r; } }
    }
    fn ink_el(l: &ink::Line, z: i64, parent: Option<BlockId>) -> Element { let (b, record) = ink::write(l); Element { id: BlockId::new(), kind: "ink".into(), x: b.x0, y: b.y0, w: b.width(), h: b.height(), z, parent, record, locked: false } }
    /// A drawn stroke becomes an element in front of everything (one step); it is NOT selected - the next stroke follows it.
    pub fn ink_put(&mut self, l: ink::Line) { let l = ink::settled(l, ink::KEEP); self.step(); let z = self.top_z() + 1; self.els.push(Self::ink_el(&l, z, None)); }
    /// The eraser passes from `a` to `b` (the sheet's units): each freehand line loses what is under it and the rest stay as
    /// lines of their own, in the old one's place in the stack; a shape goes whole. True: something went.
    fn rub(&mut self, a: Point, b: Point, stepped: &mut bool, card: Option<BlockId>) -> bool {
        let r = self.rub_r();
        // on an entered card: its own strokes, not the sheet's
        if let Some(id) = card { let mut any = false; let mut out = vec![]; for l in self.card_lines(id) { match ink::rub(&l, a, b, r) { Some(left) => { any = true; out.extend(left); } None => out.push(l) } }
            if any { if !*stepped { self.step(); *stepped = true; } self.dirty = true; self.card_set(id, &out); } return any } let reach = Rect::from_points(a, b).inflate(r, r);
        let hits: Vec<(usize, Vec<ink::Line>)> = self.els.iter().enumerate().filter(|(_, e)| e.kind == "ink" && reach.intersect(Rect::new(e.x, e.y, e.x + e.w, e.y + e.h)).area() > 0.0).filter_map(|(i, e)| ink::rub(&ink::read(e)?, a, b, r).map(|left| (i, left))).collect();
        if hits.is_empty() { return false } if !*stepped { self.step(); *stepped = true; } self.dirty = true;
        for (i, left) in hits.into_iter().rev() { let old = self.els.remove(i); self.drawn.remove(&old.id); self.sel.retain(|s| *s != old.id); for (n, l) in left.iter().enumerate() { self.els.insert(i + n, Self::ink_el(l, old.z, old.parent)); } }
        true
    }
    /// The selected strokes take a colour and / or a width (one step). True: ink was selected.
    pub fn restyle(&mut self, color: Option<[f32; 4]>, width: Option<f64>) -> bool {
        let ids: Vec<BlockId> = self.els.iter().filter(|e| e.kind == "ink" && self.sel.contains(&e.id)).map(|e| e.id).collect(); if ids.is_empty() { return false } self.step();
        for e in self.els.iter_mut().filter(|e| ids.contains(&e.id)) { let Some(mut l) = ink::read(e) else { continue }; if let Some(c) = color { l.s.color = Shape::qcolor(c); } if let Some(w) = width { l.s.width = w; }
            let (b, record) = ink::write(&l); e.x = b.x0; e.y = b.y0; e.w = b.width(); e.h = b.height(); e.record = record; self.drawn.remove(&e.id); }
        true
    }
    /// For scripted checks (`MN_CANVAS=5`): one of each kind of stroke - a pen's line swelling with its pressure among them - and
    /// `more` pen lines scattered over the sheet (`MN_INK=<n>`: how the canvas draws with thousands of them).
    pub fn ink_demo(&mut self, more: usize) {
        let pen = (self.pen, self.pen_w); let put = |me: &mut Canvas, tool: Kind, color: [f32; 4], width: f64, points: Vec<[f64; 2]>, pressures: Option<Vec<f64>>| { me.ink_put(ink::Line { s: Shape { id: strokes::new_id(), tool, color: Shape::qcolor(color), width, filled: false, is_modeled: tool == Kind::Freehand, points }, pressures }); };
        let wave: Vec<[f64; 2]> = (0..=200).map(|i| { let x = i as f64 * 3.0; [x, 60.0 * (x / 70.0).sin()] }).collect(); let swell: Vec<f64> = (0..=200).map(|i| (i as f64 / 200.0 * std::f64::consts::PI).sin()).collect();
        put(self, Kind::Freehand, [0.92, 0.92, 0.92, 1.0], 14.0, wave.clone(), Some(swell)); put(self, Kind::Freehand, [0.9, 0.2, 0.2, 1.0], 6.0, wave.iter().map(|p| [p[0], p[1] + 120.0]).collect(), None);
        put(self, Kind::Line, [0.2, 0.5, 0.9, 1.0], 4.0, vec![[0.0, 260.0], [260.0, 330.0]], None); put(self, Kind::Arrow, [0.2, 0.7, 0.3, 1.0], 6.0, vec![[320.0, 330.0], [600.0, 250.0]], None);
        put(self, Kind::Rectangle, [0.95, 0.6, 0.1, 1.0], 3.0, vec![[0.0, 380.0], [240.0, 380.0], [240.0, 500.0], [0.0, 500.0]], None); put(self, Kind::Oval, [0.6, 0.3, 0.8, 1.0], 9.0, vec![[450.0, 440.0], [130.0, 60.0]], None);
        let mut seed = 0x2545F4914F6CDD1Du64; let mut rnd = move || { seed ^= seed << 13; seed ^= seed >> 7; seed ^= seed << 17; (seed >> 11) as f64 / (1u64 << 53) as f64 };
        for _ in 0..more { let (x, y, a, w) = (rnd() * 6000.0 - 2700.0, rnd() * 4000.0 + 600.0, rnd() * 6.28, 2.0 + rnd() * 8.0); let col = [rnd() as f32, rnd() as f32, rnd() as f32, 1.0];
            let pts: Vec<[f64; 2]> = (0..=120).map(|i| { let s = i as f64; [x + a.cos() * s * 1.5 + 14.0 * (s / 9.0 + a).sin(), y + a.sin() * s * 1.5 + 14.0 * (s / 7.0).cos()] }).collect();
            let z = (rnd() < 0.5).then(|| (0..=120).map(|i| (i as f64 / 120.0 * std::f64::consts::PI).sin()).collect()); put(self, Kind::Freehand, col, w, pts, z); }
        (self.pen, self.pen_w) = pen; self.undo.clear(); self.sel.clear();
    }
    /// For scripted checks (`MN_CANVAS=7`): two text cards in a tinted frame that hugs them, a locked link card, a frame with a
    /// size of its own that clips the stroke crossing its edge.
    pub fn frame_demo(&mut self) {
        let put = |me: &mut Canvas, kind: &str, rec: &str, x: f64, y: f64, w: f64, h: f64| { let id = BlockId::new(); let z = me.top_z() + 1; me.els.push(Element { id, kind: kind.into(), x, y, w, h, z, parent: None, record: rec.into(), locked: false }); id };
        let a = put(self, "link", "https://example.org/one", 0.0, 0.0, 300.0, 52.0); let b = put(self, "link", "https://example.org/two", 60.0, 110.0, 300.0, 52.0); self.sel = vec![a, b]; self.group(); self.frame_tint("teal");
        let l = put(self, "link", "https://example.org/locked", 0.0, 320.0, 300.0, 52.0); self.sel = vec![l]; self.lock(true);
        let f = put(self, "frame", r#"{"title":"Board 16 : 9","fixed":true,"aspect":1.7777}"#, 480.0, 0.0, 480.0, 270.0);
        self.ink_put(ink::Line { s: Shape { id: strokes::new_id(), tool: Kind::Line, color: Shape::qcolor([1.0, 0.4, 0.2, 1.0]), width: 10.0, filled: false, is_modeled: false, points: vec![[560.0, 120.0], [1100.0, 200.0]] }, pressures: None });
        if let Some(e) = self.els.last_mut() { e.parent = Some(f); } self.sel = vec![l]; self.undo.clear();
    }
    #[cfg(test)] fn inks(&self) -> Vec<ink::Line> { self.els.iter().filter_map(ink::read).collect() }

    // -- frames (row 327): a titled box that owns what is in it. It hugs its members until it is given a size - then it
    //    keeps that size and clips. A member's `parent` is its frame; no frame is in a frame.

    fn is_frame(&self, id: BlockId) -> bool { self.get(id).is_some_and(|e| e.kind == "frame") }
    fn frame_of(&self, e: &Element) -> Option<BlockId> { e.parent.filter(|p| self.is_frame(*p)) }
    fn kids(&self, frame: BlockId) -> Vec<BlockId> { self.els.iter().filter(|e| e.parent == Some(frame)).map(|e| e.id).collect() }
    fn frame_rec(e: &Element) -> serde_json::Map<String, serde_json::Value> { serde_json::from_str::<serde_json::Value>(&e.record).ok().and_then(|v| v.as_object().cloned()).unwrap_or_default() }
    fn frame_fixed(e: &Element) -> bool { e.kind == "frame" && Self::frame_rec(e).get("fixed").and_then(|f| f.as_bool()).unwrap_or(false) }
    fn frame_put(&mut self, id: BlockId, key: &str, v: serde_json::Value) { if let Some(e) = self.els.iter_mut().find(|e| e.id == id && e.kind == "frame") { let mut r = Self::frame_rec(e); if v.is_null() { r.remove(key); } else { r.insert(key.into(), v); } e.record = serde_json::Value::Object(r).to_string(); } }
    /// The elements from the back to the front: by their place in the stack, each frame with its members straight after it.
    /// The elements as an export writes them: from the back to the front, each frame with its members after it.
    pub fn ordered(&self) -> Vec<&Element> { self.order().into_iter().map(|i| &self.els[i]).collect() }
    fn order(&self) -> Vec<usize> {
        let frames: std::collections::HashSet<BlockId> = self.els.iter().filter(|e| e.kind == "frame").map(|e| e.id).collect();
        if frames.is_empty() { let mut v: Vec<usize> = (0..self.els.len()).collect(); v.sort_by_key(|i| self.els[*i].z); return v }
        let mut kids: std::collections::HashMap<BlockId, Vec<usize>> = Default::default(); let mut top = vec![];
        for (i, e) in self.els.iter().enumerate() { match e.parent.filter(|p| frames.contains(p) && e.kind != "frame") { Some(p) => kids.entry(p).or_default().push(i), None => top.push(i) } }
        top.sort_by_key(|i| self.els[*i].z); let mut out = Vec::with_capacity(self.els.len());
        for i in top { out.push(i); if let Some(mut k) = kids.remove(&self.els[i].id) { k.sort_by_key(|j| self.els[*j].z); out.extend(k); } }
        out
    }
    /// A window point is on a frame itself: its title's strip or its edge (inside it the sheet is the sheet's).
    fn on_frame(&self, e: &Element, x: f64, y: f64) -> bool { let r = self.on_screen(e); let p = Point::new(x, y); let m = 5.0 * self.scale;
        r.inflate(m, m).contains(p) && (y < r.y0 + FRAME_TITLE * self.k().max(self.scale * 0.6) || !r.inflate(-m, -m).contains(p)) }
    /// The frame in front at a point of the sheet (none of `not`).
    fn frame_at(&self, w: Point, not: &[BlockId]) -> Option<BlockId> { self.order().into_iter().rev().map(|i| &self.els[i]).find(|e| e.kind == "frame" && !not.contains(&e.id) && Rect::new(e.x, e.y, e.x + e.w, e.y + e.h).contains(w)).map(|e| e.id) }
    /// Frames that hug are drawn round their members again (an empty one keeps its box).
    fn hug(&mut self) {
        let frames: Vec<BlockId> = self.els.iter().filter(|e| e.kind == "frame" && !Self::frame_fixed(e)).map(|e| e.id).collect();
        for f in frames { let Some(u) = self.els.iter().filter(|e| e.parent == Some(f)).map(|e| Rect::new(e.x, e.y, e.x + e.w, e.y + e.h)).reduce(|a, b| a.union(b)) else { continue };
            let r = Rect::new(u.x0 - FRAME_PAD, u.y0 - FRAME_PAD - FRAME_TITLE, u.x1 + FRAME_PAD, u.y1 + FRAME_PAD);
            if let Some(e) = self.els.iter_mut().find(|e| e.id == f) { if (e.x, e.y, e.w, e.h) != (r.x0, r.y0, r.width(), r.height()) { e.x = r.x0; e.y = r.y0; e.w = r.width(); e.h = r.height(); self.dirty = true; } } }
    }
    fn frame_title(&self, base: &str) -> String { let n = self.els.iter().filter(|e| e.kind == "frame").count(); if n == 0 { base.to_string() } else { format!("{base} {}", n + 1) } }
    /// Cmd+G: a frame round the selection; with one frame among the selected, the others join it. True: something was done.
    pub fn group(&mut self) -> bool {
        self.edit_end(true); let frames: Vec<BlockId> = self.sel.iter().copied().filter(|id| self.is_frame(*id)).collect(); let rest: Vec<BlockId> = self.sel.iter().copied().filter(|id| !self.is_frame(*id) && self.get(*id).is_some_and(|e| !e.locked)).collect();
        if rest.is_empty() || frames.len() > 1 { return false } self.step();
        let f = match frames.first() { Some(f) => *f, None => { let id = BlockId::new(); let z = rest.iter().filter_map(|i| self.get(*i)).map(|e| e.z).min().unwrap_or(0);
            let title = self.frame_title("Frame"); self.els.push(Element { id, kind: "frame".into(), x: 0.0, y: 0.0, w: 320.0, h: 240.0, z, parent: None, record: serde_json::json!({ "title": title }).to_string(), locked: false }); id } };
        for e in self.els.iter_mut().filter(|e| rest.contains(&e.id)) { e.parent = Some(f); } self.hug(); self.sel = vec![f]; true
    }
    /// Shift+Cmd+G: the one selected frame goes, what was in it stays and is selected.
    pub fn ungroup(&mut self) -> bool {
        let [f] = self.sel[..] else { return false }; if !self.is_frame(f) { return false } self.edit_end(true); self.step(); let kids = self.kids(f); let z = self.get(f).map(|e| e.z).unwrap_or(0);
        for e in self.els.iter_mut().filter(|e| e.parent == Some(f)) { e.parent = None; e.z = e.z.max(z); } self.els.retain(|e| e.id != f); self.sel = kids; true
    }
    /// The one selected frame, for the menu: (it has a size of its own, its tint).
    pub fn selected_frame(&self) -> Option<(BlockId, bool, String)> { let [f] = self.sel[..] else { return None }; let e = self.get(f).filter(|e| e.kind == "frame")?; Some((f, Self::frame_fixed(e), Self::frame_rec(e).get("tint").and_then(|t| t.as_str()).unwrap_or("").to_string())) }
    /// The selected frame hugs its members again.
    pub fn frame_fit(&mut self) { if let Some((f, ..)) = self.selected_frame() { self.step(); self.frame_put(f, "fixed", serde_json::Value::Null); self.frame_put(f, "aspect", serde_json::Value::Null); self.hug(); } }
    /// The selected frame takes a shape (width : height) at its width, and keeps it: it has a size of its own and clips.
    pub fn frame_shape(&mut self, aspect: f64) { if let Some((f, ..)) = self.selected_frame() { self.step(); self.frame_put(f, "fixed", serde_json::json!(true)); self.frame_put(f, "aspect", serde_json::json!(aspect)); if let Some(e) = self.els.iter_mut().find(|e| e.id == f) { e.h = (e.w / aspect).max(120.0).round(); } } }
    /// The selected frame's tint: one of the palette's named colours, or "" for none.
    pub fn frame_tint(&mut self, tint: &str) { if let Some((f, ..)) = self.selected_frame() { self.step(); self.frame_put(f, "tint", if tint.is_empty() { serde_json::Value::Null } else { serde_json::json!(tint) }); } }
    /// The selected frame's title is typed.
    pub fn frame_rename(&mut self, fonts: &mut Fonts) { if let Some((f, ..)) = self.selected_frame() { self.edit_begin(f, fonts); } }
    /// New cards join the frame they were put in.
    fn adopt(&mut self, ids: &[BlockId]) { for id in ids { let Some(c) = self.get(*id).filter(|e| e.kind != "frame" && e.parent.is_none()).map(|e| Point::new(e.x + e.w / 2.0, e.y + e.h / 2.0)) else { continue }; if let Some(f) = self.frame_at(c, &[]) { if let Some(e) = self.els.iter_mut().find(|e| e.id == *id) { e.parent = Some(f); } } } }

    // -- the basics the Qt app lacked (row 327): lock, align and distribute, snapping

    pub fn any_locked(&self) -> bool { self.els.iter().any(|e| e.locked && self.sel.contains(&e.id)) }
    /// The selected cards are locked - not moved, resized or removed until unlocked - or unlocked.
    pub fn lock(&mut self, on: bool) { if self.sel.is_empty() { return } self.step(); for e in self.els.iter_mut().filter(|e| self.sel.contains(&e.id)) { e.locked = on; } }
    /// What a command moves as one: each selected card that is not locked and does not travel with a selected frame - a frame with its members.
    fn units(&self) -> Vec<(Vec<BlockId>, Rect)> {
        self.els.iter().filter(|e| self.sel.contains(&e.id) && !e.locked && !self.frame_of(e).is_some_and(|f| self.sel.contains(&f))).map(|e| { let mut ids = vec![e.id]; if e.kind == "frame" { ids.extend(self.kids(e.id)); } (ids, Rect::new(e.x, e.y, e.x + e.w, e.y + e.h)) }).collect()
    }
    fn shift_els(&mut self, ids: &[BlockId], dx: f64, dy: f64) { for e in self.els.iter_mut().filter(|e| ids.contains(&e.id)) { e.x = (e.x + dx).round(); e.y = (e.y + dy).round(); } }
    /// The selected cards are lined up: 0 their left edges, 1 their centres across, 2 right edges, 3 tops, 4 middles, 5 bottoms.
    pub fn align(&mut self, how: u8) {
        let u = self.units(); if u.len() < 2 { return } let all = u.iter().map(|x| x.1).reduce(|a, b| a.union(b)).unwrap(); self.step();
        for (ids, r) in u { let (dx, dy) = match how { 0 => (all.x0 - r.x0, 0.0), 1 => (all.center().x - r.center().x, 0.0), 2 => (all.x1 - r.x1, 0.0), 3 => (0.0, all.y0 - r.y0), 4 => (0.0, all.center().y - r.center().y), _ => (0.0, all.y1 - r.y1) }; self.shift_els(&ids, dx, dy); } self.hug();
    }
    /// The selected cards are spread with equal gaps between the first and the last, across or down.
    pub fn distribute(&mut self, across: bool) {
        let mut u = self.units(); if u.len() < 3 { return } let lo = |r: &Rect| if across { r.x0 } else { r.y0 }; let size = |r: &Rect| if across { r.width() } else { r.height() };
        u.sort_by(|a, b| (lo(&a.1) + size(&a.1) / 2.0).partial_cmp(&(lo(&b.1) + size(&b.1) / 2.0)).unwrap_or(std::cmp::Ordering::Equal));
        let (first, last) = (u[0].1, u[u.len() - 1].1); let span = lo(&last) + size(&last) - lo(&first); let gap = (span - u.iter().map(|x| size(&x.1)).sum::<f64>()) / (u.len() - 1) as f64; self.step();
        let mut at = lo(&first); for (ids, r) in u { let d = at - lo(&r); if across { self.shift_els(&ids, d, 0.0); } else { self.shift_els(&ids, 0.0, d); } at += size(&r) + gap; } self.hug();
    }
    /// The lines a dragged thing can snap to: the edges and the centres of the cards in view that stay where they are (strokes are not lines to snap to).
    fn snap_lines(&self, moving: &[BlockId]) -> (Vec<f64>, Vec<f64>) {
        let (mut xs, mut ys) = (vec![], vec![]); let view = Rect::from_points(self.to_world(self.rect.x0, self.rect.y0), self.to_world(self.rect.x1, self.rect.y1));
        for e in self.els.iter().filter(|e| e.kind != "ink" && !moving.contains(&e.id)) { let r = Rect::new(e.x, e.y, e.x + e.w, e.y + e.h); if r.intersect(view).area() <= 0.0 { continue } xs.extend([r.x0, r.center().x, r.x1]); ys.extend([r.y0, r.center().y, r.y1]); }
        (xs, ys)
    }
    /// The nearest of `lines` to one of `mine` within reach: (how far to go, the line).
    fn snap_to(mine: &[f64], lines: &[f64], reach: f64) -> Option<(f64, f64)> { let mut best: Option<(f64, f64)> = None; for m in mine { for l in lines { let d = l - m; if d.abs() <= reach && best.is_none_or(|b| d.abs() < b.0.abs()) { best = Some((d, *l)); } } } best }
    /// Where a shape's drag ends with Shift held: a line or an arrow on the nearest of the eight directions, a rectangle or an oval square.
    fn aim(&self, tool: Ink, from: Point, to: Point) -> Point {
        if !self.shift { return to } let d = to - from;
        match tool { Ink::Line | Ink::Arrow => { let a = (d.y.atan2(d.x) / std::f64::consts::FRAC_PI_4).round() * std::f64::consts::FRAC_PI_4; from + Vec2::new(a.cos(), a.sin()) * d.hypot() }
            Ink::Rect | Ink::Oval => { let m = d.x.abs().max(d.y.abs()); from + Vec2::new(m * d.x.signum(), m * d.y.signum()) } _ => to }
    }

    // -- a text card's words

    fn edit_begin(&mut self, id: BlockId, fonts: &mut Fonts) {
        self.edit_end(true); let Some(e) = self.get(id).filter(|e| e.kind == "text" || e.kind == "frame") else { return };
        let mut f = TextArea::new(Family::Sans, 14.0); f.set_text(&if e.kind == "frame" { Self::frame_rec(e).get("title").and_then(|t| t.as_str()).unwrap_or("").to_string() } else { e.record.clone() }); f.focused = true; let _ = fonts; self.edit = Some((id, f)); self.sel = vec![id];
    }
    /// The words being typed are kept (`keep`) or given up; a card left empty goes.
    pub fn edit_end(&mut self, keep: bool) {
        let Some((id, f)) = self.edit.take() else { return }; let text = f.text().trim_end().to_string();
        // a frame's title: an empty one is not taken
        if self.is_frame(id) { let title = text.lines().next().unwrap_or("").trim().to_string(); let was = self.get(id).map(|e| Self::frame_rec(e).get("title").and_then(|t| t.as_str()).unwrap_or("").to_string()).unwrap_or_default(); if keep && !title.is_empty() && title != was { self.step(); self.frame_put(id, "title", serde_json::json!(title)); } return }
        let was = self.get(id).map(|e| e.record.clone()).unwrap_or_default();
        let text = if keep { text } else { was.clone() };
        if text.trim().is_empty() { if !was.trim().is_empty() { self.step(); } else { self.dirty = true; } self.els.retain(|e| e.id != id); self.sel.retain(|s| *s != id); }
        else if text != was { self.step(); if let Some(e) = self.els.iter_mut().find(|e| e.id == id) { e.record = text; } }
    }

    // -- the pointer and the keys

    fn handles(&self) -> Option<(BlockId, [(Rect, bool, bool); 4])> {
        let [id] = self.sel[..] else { return None }; let r = self.on_screen(self.get(id).filter(|e| !e.locked)?); let h = HANDLE * self.scale;
        let at = |x: f64, y: f64| Rect::from_center_size((x, y), (h, h));
        Some((id, [(at(r.x0, r.y0), true, true), (at(r.x1, r.y0), false, true), (at(r.x0, r.y1), true, false), (at(r.x1, r.y1), false, false)]))
    }
    fn buttons(&mut self) -> [&mut ActionButton; 5] { [&mut self.b_hand, &mut self.zoom_out, &mut self.zoom_pc, &mut self.zoom_in, &mut self.zoom_fit] }
    /// What the pointer would do at a window point: 0 nothing special, 1 pan, 2 a corner (nwse), 3 a corner (nesw), 4 a control, 5 draw.
    pub fn cursor(&mut self, x: f64, y: f64) -> u8 {
        if self.buttons().iter().any(|b| b.hit(x, y)) { return 4 } if matches!(self.act, Some(Act::Pan { .. })) || self.space || self.hand { return 1 } if self.tool.is_some() || self.framing { return 5 }
        if let Some(Act::Resize { left, top, .. }) = &self.act { return if left == top { 2 } else { 3 } }
        if self.edit.is_none() { if let Some((_, hs)) = self.handles() { if let Some((_, l, tp)) = hs.iter().find(|(r, ..)| r.inflate(3.0, 3.0).contains(Point::new(x, y))) { return if l == tp { 2 } else { 3 } } } }
        0
    }
    /// A pointer event in the canvas's view. `middle`: the middle button. True: draw again.
    pub fn pointer(&mut self, e: &Event, middle: bool, shift: bool, fonts: &mut Fonts) -> bool {
        let now = Instant::now(); let m = self.rect.center(); self.hover = e.pos(); self.shift = shift;
        // the zoom's controls
        let mut did = None; for (i, b) in self.buttons().into_iter().enumerate() { if b.event(e, now) == Response::Clicked { did = Some(i); } }
        match did { Some(0) => { self.hand = !self.hand; return true } Some(1) => { self.zoom_by(1.0 / 2f64.sqrt(), (m.x, m.y)); return true } Some(2) => { self.zoom_to(1.0); return true } Some(3) => { self.zoom_by(2f64.sqrt(), (m.x, m.y)); return true } Some(4) => { self.fit_all(); return true } _ => {} }
        if let Some((x, y)) = e.pos() { if self.act.is_none() && self.buttons().iter().any(|b| b.hit(x, y)) { return true } }
        // a text card being typed in takes what is in it; a press outside keeps the words
        if let Some((id, f)) = self.edit.as_mut() { let inside = e.pos().is_some_and(|(x, y)| f.hit(x, y)); let id = *id;
            if inside || f.dragging() { f.pointer(e, fonts); return true } if matches!(e, Event::Down { .. }) { self.edit_end(true); let _ = id; } }
        match *e {
            Event::Down { x, y, .. } => { let w = self.to_world(x, y);
                if middle || self.space || self.hand { self.act = Some(Act::Pan { from: (x, y), cam: (self.cam.0, self.cam.1) }); return true }
                // the frame tool: a press and a drag make a frame of that size, a click an empty one
                if self.framing { self.fit = false; self.act = Some(Act::Frame { from: w, to: w }); return true }
                // an armed tool takes the press wherever it lands: nothing under it is selected or moved
                // a press outside the entered card leaves it
                let card = self.inked.filter(|id| self.get(*id).is_some_and(|e| self.on_screen(e).contains(Point::new(x, y)))); if card.is_none() { self.inked = None; }
                if let Some(tool) = self.tool { self.fit = false; self.sel.clear();
                    if tool == Ink::Eraser { let mut stepped = false; self.rub(w, w, &mut stepped, card); self.act = Some(Act::Rub { last: w, stepped, card }); }
                    else { let p = self.pressure; self.act = Some(Act::Draw { card, tool, from: w, to: w, sm: (tool == Ink::Pen).then(|| Smoother::begin([w.x / 1000.0, w.y / 1000.0], self.secs())), marks: vec![(1, p.unwrap_or(1.0))], pressed: p.is_some() }); }
                    return true }
                if let Some((id, hs)) = self.handles() { if let Some((_, left, top)) = hs.iter().find(|(r, ..)| r.inflate(3.0, 3.0).contains(Point::new(x, y))) { let el = self.get(id).unwrap();
                    self.act = Some(Act::Resize { id, left: *left, top: *top, orig: Rect::new(el.x, el.y, el.x + el.w, el.y + el.h), keep: (el.kind == "media" && el.h > 60.0) || Self::frame_rec(el).contains_key("aspect") && el.kind == "frame", moved: false }); return true } }
                let hit = self.at(x, y);
                let twice = self.last_click.is_some_and(|(t0, h)| h == hit && t0.elapsed().as_millis() < 420); self.last_click = if twice { None } else { Some((now, hit)) };
                match hit {
                    Some(id) if twice => { match self.get(id).map(|e| e.kind.clone()).as_deref() { Some("text") | Some("frame") => self.edit_begin(id, fonts), Some("ink") => {} Some("note") => { self.sel = vec![id]; self.entered = Some(id); } Some("link") => self.opened = Some(id), _ if self.get(id).and_then(kind_of).as_deref() == Some("file") => self.opened = Some(id), _ => { self.open(id); if self.card_page(id).is_some() || self.video_el(id).is_some() { self.inked = Some(id); } } } }
                    Some(id) => { if shift { if let Some(i) = self.sel.iter().position(|s| *s == id) { self.sel.remove(i); } else { self.sel.push(id); } } else if !self.sel.contains(&id) { self.sel = vec![id]; }
                        // what moves: the selected cards that are not locked, a frame with what is in it
                        let ids: Vec<BlockId> = self.units().into_iter().flat_map(|u| u.0).collect();
                        if self.get(id).is_some_and(|e| !e.locked) && !ids.is_empty() { self.act = Some(Act::Move { from: w, orig: self.els.iter().filter(|e| ids.contains(&e.id)).map(|e| (e.id, e.x, e.y)).collect(), moved: false }); } }
                    // twice on the empty sheet: a text card there, ready to be typed in
                    None if twice => { self.want_note = Some(w); }
                    None => { if !shift { self.sel.clear(); } self.act = Some(Act::Marquee { from: w, to: w, add: shift }); }
                }
                true }
            Event::Move { x, y } => { let w = self.to_world(x, y); let k = self.k();
                match self.act.take() {
                    Some(Act::Draw { tool, from, mut sm, mut marks, pressed, card, .. }) => { let p = self.pressure; let t = self.secs();
                        if let Some(s) = sm.as_mut() { s.push([w.x / 1000.0, w.y / 1000.0], t); marks.push((s.points().len(), p.unwrap_or(1.0))); }
                        let to = self.aim(tool, from, w); self.act = Some(Act::Draw { tool, from, to, sm, marks, pressed: pressed || p.is_some(), card }); true }
                    Some(Act::Frame { from, .. }) => { self.act = Some(Act::Frame { from, to: w }); true }
                    Some(Act::Rub { last, mut stepped, card }) => { self.rub(last, w, &mut stepped, card); self.act = Some(Act::Rub { last: w, stepped, card }); true }
                    Some(Act::Pan { from, cam }) => { self.cam.0 = cam.0 - (x - from.0) / k; self.cam.1 = cam.1 - (y - from.1) / k; self.fit = false; self.act = Some(Act::Pan { from, cam }); true }
                    Some(Act::Move { from, orig, moved }) => { let mut d = w - from; let go = moved || d.hypot() * k > 3.0 * self.scale;
                        // Shift keeps the move to one axis; the dragged cards' edges and centres snap to those of the others (not with Ctrl held)
                        if shift { if d.x.abs() >= d.y.abs() { d.y = 0.0; } else { d.x = 0.0; } }
                        self.guides.clear(); let ids: Vec<BlockId> = orig.iter().map(|o| o.0).collect();
                        if go && !self.ctrl { if let Some(b) = orig.iter().filter_map(|(id, ox, oy)| self.get(*id).map(|e| Rect::new(*ox, *oy, ox + e.w, oy + e.h))).reduce(|a, b| a.union(b)) { let (xs, ys) = self.snap_lines(&ids); let reach = 6.0 / self.cam.2;
                            if let Some((dx, l)) = Self::snap_to(&[b.x0 + d.x, b.center().x + d.x, b.x1 + d.x], &xs, reach).filter(|_| !(shift && d.x == 0.0)) { d.x += dx; self.guides.push((true, l)); }
                            if let Some((dy, l)) = Self::snap_to(&[b.y0 + d.y, b.center().y + d.y, b.y1 + d.y], &ys, reach).filter(|_| !(shift && d.y == 0.0)) { d.y += dy; self.guides.push((false, l)); } } }
                        if go { if !moved { self.step(); } for (id, ox, oy) in &orig { if let Some(e) = self.els.iter_mut().find(|e| e.id == *id) { e.x = (ox + d.x).round(); e.y = (oy + d.y).round(); } }
                            // the frame the cards would join if let go here (a frame being dragged joins none)
                            self.drop_at = if ids.iter().any(|i| self.get(*i).is_some_and(|e| e.kind != "frame" && !self.frame_of(e).is_some_and(|f| ids.contains(&f)))) { self.frame_at(w, &ids) } else { None }; }
                        self.act = Some(Act::Move { from, orig, moved: go }); go }
                    Some(Act::Resize { id, left, top, orig, keep, moved }) => { if !moved { self.step(); }
                        let (mut x0, mut y0, mut x1, mut y1) = (orig.x0, orig.y0, orig.x1, orig.y1); let (mw, mh) = if self.get(id).is_some_and(|e| e.kind == "ink") { (2.0, 2.0) } else { (40.0, 24.0) }; let keep = keep || shift; let mut w = w; self.guides.clear();
                        if !keep && !self.ctrl { let (xs, ys) = self.snap_lines(&[id]); let reach = 6.0 / self.cam.2; if let Some((dx, l)) = Self::snap_to(&[w.x], &xs, reach) { w.x += dx; self.guides.push((true, l)); } if let Some((dy, l)) = Self::snap_to(&[w.y], &ys, reach) { w.y += dy; self.guides.push((false, l)); } }
                        if left { x0 = w.x.min(x1 - mw); } else { x1 = w.x.max(x0 + mw); } if top { y0 = w.y.min(y1 - mh); } else { y1 = w.y.max(y0 + mh); }
                        // a picture keeps its shape: the wider of the two pulls decides
                        if keep { let a = orig.width() / orig.height(); let (nw, nh) = (x1 - x0, y1 - y0); let (nw, nh) = if nw / a >= nh { (nw, nw / a) } else { (nh * a, nh) }; if left { x0 = x1 - nw; } else { x1 = x0 + nw; } if top { y0 = y1 - nh; } else { y1 = y0 + nh; } }
                        if let Some(e) = self.els.iter_mut().find(|e| e.id == id) { e.x = x0.round(); e.y = y0.round(); e.w = (x1 - x0).round(); e.h = (y1 - y0).round(); }
                        // a frame given a size by hand keeps it
                        if !moved && self.is_frame(id) { self.frame_put(id, "fixed", serde_json::json!(true)); }
                        self.act = Some(Act::Resize { id, left, top, orig, keep, moved: true }); true }
                    Some(Act::Marquee { from, add, .. }) => { self.act = Some(Act::Marquee { from, to: w, add }); true }
                    // the eraser's ring follows the pointer
                    None => self.tool == Some(Ink::Eraser),
                } }
            Event::Up { .. } => { self.guides.clear(); let joins = self.drop_at.take(); let w = self.hover.map(|(x, y)| self.to_world(x, y));
                match self.act.take() {
                    // let go: the cards that were dragged join the frame under the pointer, or leave theirs over none (a member that
                    // travelled with its frame stays its member)
                    Some(Act::Move { orig, moved: true, .. }) => { let ids: Vec<BlockId> = orig.iter().map(|o| o.0).collect(); let to = w.and_then(|w| self.frame_at(w, &ids)).or(joins.filter(|_| w.is_none()));
                        for e in self.els.iter_mut().filter(|e| ids.contains(&e.id) && e.kind != "frame") { if !e.parent.is_some_and(|p| ids.contains(&p)) { e.parent = to; } } self.hug(); true }
                    Some(Act::Frame { from, to }) => { self.framing = false; self.step(); let r = Rect::from_points(from, to); let dragged = r.width() * self.k() > 8.0 * self.scale || r.height() * self.k() > 8.0 * self.scale;
                        let (r, rec) = if dragged { (Rect::new(r.x0, r.y0, r.x0 + r.width().max(160.0), r.y0 + r.height().max(120.0)), serde_json::json!({ "title": self.frame_title("Frame"), "fixed": true })) } else { (Rect::new(from.x - 160.0, from.y - 120.0, from.x + 160.0, from.y + 120.0), serde_json::json!({ "title": self.frame_title("Frame") })) };
                        let id = BlockId::new(); let z = self.top_z() + 1; self.els.push(Element { id, kind: "frame".into(), x: r.x0.round(), y: r.y0.round(), w: r.width().round(), h: r.height().round(), z, parent: None, record: rec.to_string(), locked: false }); self.sel = vec![id]; true }
                    Some(Act::Draw { tool, from, to, sm, marks, pressed, card }) => { let t = self.secs(); let pen = sm.map(|s| s.end([to.x / 1000.0, to.y / 1000.0], t).into_iter().map(|p| [p[0] * 1000.0, p[1] * 1000.0]).collect());
                        if let Some(l) = self.line_of(tool, from, to, pen, &marks, pressed) { match card { Some(id) => { let mut ls = self.card_lines(id); ls.push(ink::settled(l, ink::KEEP)); self.step(); self.card_set(id, &ls); } None => self.ink_put(l) } } true }
                    Some(Act::Marquee { from, to, add }) => { let r = Rect::from_points(from, to); if r.width() * self.k() > 3.0 || r.height() * self.k() > 3.0 { // (a locked card is passed over; a frame is taken when all of it is inside)
                        let hit: Vec<BlockId> = self.els.iter().filter(|e| !e.locked && { let b = Rect::new(e.x, e.y, e.x + e.w, e.y + e.h); if e.kind == "frame" { r.union(b) == r } else { r.intersect(b).area() > 0.0 } }).map(|e| e.id).collect(); if !add { self.sel.clear(); } for id in hit { if !self.sel.contains(&id) { self.sel.push(id); } } } true }
                    Some(_) => true, None => false } }
            _ => false,
        }
    }
    /// A card is looked at larger: the camera goes to it (Escape goes back to where it was).
    pub fn open(&mut self, id: BlockId) { let Some(r) = self.bounds(Some(&[id])) else { return }; if self.back.is_none() { self.back = Some(self.cam); } let fit = self.fit; self.show(r.inflate(24.0, 24.0)); self.cam.2 = self.cam.2.max(ZOOM.0); self.fit = false; let _ = fit; self.sel = vec![id]; }
    /// Escape: a stroke being drawn is given up; else the armed tool is put down; else the words being typed are kept and the field closes; else the camera goes back; else nothing stays selected.
    pub fn escape(&mut self) { if matches!(self.act, Some(Act::Draw { .. } | Act::Rub { .. } | Act::Frame { .. })) { self.act = None; return } if self.framing { self.framing = false; return } if self.edit.is_none() && self.tool.is_some() { self.tool = None; return } if self.inked.take().is_some() { return } if self.edit.is_some() { self.edit_end(true); } else if let Some(c) = self.back.take() { self.cam = c; } else { self.sel.clear(); } self.act = None; }
    /// Return: the one selected card is opened (a text card: typed in).
    pub fn enter(&mut self, fonts: &mut Fonts) { if let [id] = self.sel[..] { match self.get(id).map(|e| e.kind.clone()).as_deref() { Some("text") | Some("frame") => self.edit_begin(id, fonts), Some("ink") => {} Some("note") => self.entered = Some(id), Some("link") => self.opened = Some(id), Some(_) if self.get(id).and_then(kind_of).as_deref() == Some("file") => self.opened = Some(id), Some(_) => self.open(id), None => {} } } }
    /// A key for the text card being typed in. True: it was the card's.
    pub fn key(&mut self, k: &KeyEvent, fonts: &mut Fonts) -> bool {
        let Some(id) = self.edit.as_ref().map(|e| e.0) else { return false };
        if self.is_frame(id) && matches!(k.key, min_ui::Key::Named(min_ui::Named::Enter)) { self.edit_end(true); return true }
        let Some((_, f)) = self.edit.as_mut() else { return false };
        if f.key(k, fonts) == FieldResponse::Cancelled { self.edit_end(!self.is_frame(id)); } true
    }
    pub fn ime(&mut self, e: &min_ui::ImeEvent, fonts: &mut Fonts) -> bool { match self.edit.as_mut() { Some((_, f)) => { f.ime(e, fonts); true } None => false } }

    // -- the picture

    /// Painted into `rect` (window px). True: something is still arriving or moving - draw again.
    /// `live`: the note card whose text the window itself draws (it is being edited); `note`: draws a note card's
    /// content into its box (the box, the units' size on screen, far out) and says how tall the content is (units) and whether to draw again.
    /// `playing`: the video card the window's player shows UNDER this picture - an opening is cut for it (`cut`, also the
    /// opening for a video of the entered note card), so what lies behind it does not show and what lies in front covers it;
    /// `video`: draws what goes over a video card's picture (its frame's notes) and its strip (the box given when it is selected).
    #[allow(clippy::too_many_arguments)]
    pub fn paint(&mut self, scene: &mut Scene, fonts: &mut Fonts, media: &mut Media, rect: Rect, s: f64, live: Option<BlockId>, note: &mut dyn FnMut(&mut Scene, &mut Fonts, &mut Media, &Element, Rect, f64, bool) -> (f64, bool), playing: Option<BlockId>, cut: Option<Rect>, video: &mut dyn FnMut(&mut Scene, &mut Fonts, &mut Media, &Element, Rect, Option<Rect>)) -> bool {
        let open = |scene: &mut Scene, h: Rect| { scene.push_layer(Fill::NonZero, peniko::BlendMode::new(peniko::Mix::Normal, peniko::Compose::DestOut), 1.0, Affine::IDENTITY, &h); scene.fill(Fill::NonZero, Affine::IDENTITY, Color::BLACK, None, &h); scene.pop_layer(); };
        let first = self.rect.width() < 1.0; self.rect = rect; self.scale = s; if self.act.is_none() { self.hug(); } if self.fit && (first || self.act.is_none()) && self.edit.is_none() { self.fit_all(); }
        let now = Instant::now(); let mut again = false; let k = self.k();
        scene.push_clip_layer(Fill::NonZero, Affine::IDENTITY, &rect);
        let (panel, line, text, dim, accent) = (c(t::BACKGROUND_LAYER_2_COLOR), c(t::DIVIDER_COLOR), c(t::NEUTRAL_CONTENT_COLOR_DEFAULT), c(t::NEUTRAL_SUBDUED_CONTENT_COLOR_DEFAULT), c(t::ACCENT_BACKGROUND_COLOR_DEFAULT));
        let order = self.order(); let mut fits: Vec<(BlockId, f64)> = vec![];
        // what is in a frame with a size of its own is clipped to it (not the frame of the playing video: its opening must reach the sheet's own layer)
        let fixed: std::collections::HashMap<BlockId, Rect> = self.els.iter().filter(|e| Self::frame_fixed(e)).map(|e| (e.id, self.on_screen(e))).collect();
        let free = playing.and_then(|p| self.get(p)).and_then(|e| e.parent).or(live.and_then(|p| self.get(p)).and_then(|e| e.parent)); let mut clip: Option<BlockId> = None;
        for i in order { let e = &self.els[i];
            let want = e.parent.filter(|p| fixed.contains_key(p) && Some(*p) != free && e.kind != "frame");
            if want != clip { if clip.is_some() { scene.pop_layer(); } if let Some(f) = want { scene.push_clip_layer(Fill::NonZero, Affine::IDENTITY, &fixed[&f]); } clip = want; }
            let r = self.on_screen(e); if r.intersect(rect).area() <= 0.0 { continue }
            let far = self.cam.2 < FAR || r.width() < 28.0 * s; let round = RoundedRect::from_rect(r, (6.0 * k).min(8.0 * s));
            let words = |scene: &mut Scene, fonts: &mut Fonts, head: &str, more: &str| { scene.fill(Fill::NonZero, Affine::IDENTITY, panel, None, &round); if far { return }
                let a = fonts.layout_fit(head, Family::Sans, (13.0 * k).clamp(6.0, 40.0) as f32, 400.0, text, (r.width() - 20.0 * k).max(8.0)); draw_layout(scene, &a, Affine::translate((r.x0 + 10.0 * k, r.y0 + 8.0 * k)));
                if !more.is_empty() && r.height() > 44.0 * k { let b = fonts.layout_fit(more, Family::Sans, (11.0 * k).clamp(5.0, 34.0) as f32, 400.0, dim, (r.width() - 20.0 * k).max(8.0)); draw_layout(scene, &b, Affine::translate((r.x0 + 10.0 * k, r.y0 + 10.0 * k + a.height() as f64))); } };
            match e.kind.as_str() {
                "frame" => { let rec = Self::frame_rec(e); let tint = rec.get("tint").and_then(|x| x.as_str()).and_then(mn_layout::colour); let (ink_c, fill_a, edge_a, strip_a) = match tint { Some(c) => (c, 0.10, 0.5, 0.12), None => (dim, 0.10, 0.45, 0.14) };
                    scene.fill(Fill::NonZero, Affine::IDENTITY, ink_c.with_alpha(fill_a), None, &round); let th = (FRAME_TITLE * k).min(r.height());
                    if !far { scene.push_clip_layer(Fill::NonZero, Affine::IDENTITY, &round); scene.fill(Fill::NonZero, Affine::IDENTITY, ink_c.with_alpha(strip_a), None, &Rect::new(r.x0, r.y0, r.x1, r.y0 + th)); scene.pop_layer();
                        if !self.edit.as_ref().is_some_and(|(id, _)| *id == e.id) { let title = rec.get("title").and_then(|x| x.as_str()).unwrap_or(""); let l = fonts.layout_fit(title, Family::Sans, (13.0 * k).clamp(7.0, 40.0) as f32, 600.0, if tint.is_some() { ink_c } else { text }, (r.width() - 20.0 * k).max(8.0)); draw_layout(scene, &l, Affine::translate((r.x0 + 10.0 * k, r.y0 + (th - l.height() as f64) / 2.0))); } }
                    scene.stroke(&Stroke::new(if self.drop_at == Some(e.id) { 2.5 * s } else { s.max(1.0) }), Affine::IDENTITY, if self.drop_at == Some(e.id) { accent } else { ink_c.with_alpha(edge_a) }, None, &round); }
                "ink" => { let key = (e.w.to_bits(), e.h.to_bits(), e.record.len());
                    if self.drawn.get(&e.id).is_none_or(|d| d.0 != key) { if let Some(l) = ink::read(e) { self.drawn.insert(e.id, (key, ink::drawn(&l, Point::new(e.x, e.y)))); } }
                    if let Some((_, d)) = self.drawn.get(&e.id) { d.paint(scene, Affine::translate((r.x0, r.y0)) * Affine::scale(k)); } }
                "link" => { scene.fill(Fill::NonZero, Affine::IDENTITY, panel, None, &round); if far { continue } let link = c(t::ACCENT_CONTENT_COLOR_DEFAULT); let mut x = r.x0 + 12.0 * k;
                    if let Some(g) = fonts.icon("link", (18.0 * k) as f32, link) { draw_layout(scene, &g, Affine::translate((x, r.center().y - g.height() as f64 / 2.0))); x += 26.0 * k; }
                    let l = fonts.layout_fit(&e.record, Family::Sans, (13.0 * k).clamp(5.0, 44.0) as f32, 400.0, link, (r.x1 - x - 10.0 * k).max(8.0)); draw_layout(scene, &l, Affine::translate((x, r.center().y - l.height() as f64 / 2.0))); }
                "note" => { scene.fill(Fill::NonZero, Affine::IDENTITY, panel, None, &round); if live == Some(e.id) { if let Some(h) = cut { open(scene, h.intersect(r)); } continue }
                    let (h, more) = note(scene, fonts, media, e, r, k, far); again |= more; if h > 0.0 { fits.push((e.id, h)); } }
                "text" => { scene.fill(Fill::NonZero, Affine::IDENTITY, panel, None, &round);
                    if self.edit.as_ref().is_some_and(|(id, _)| *id == e.id) || far { continue }
                    scene.push_clip_layer(Fill::NonZero, Affine::IDENTITY, &r);
                    let l = fonts.layout_width(&e.record, Family::Sans, (14.0 * k) as f32, 400.0, text, Some(((r.width() - 20.0 * k).max(8.0)) as f32)); draw_layout(scene, &l, Affine::translate((r.x0 + 10.0 * k, r.y0 + 8.0 * k)));
                    scene.pop_layer(); }
                _ => { let name = name_of(e); let kind = serde_json::from_str::<serde_json::Value>(&e.record).ok().and_then(|v| v.get("kind").and_then(|x| x.as_str()).map(str::to_string));
                    if far { scene.fill(Fill::NonZero, Affine::IDENTITY, panel, None, &round); continue }
                    let strip = (kind.as_deref() == Some("video") && self.sel.len() == 1 && self.sel[0] == e.id).then(|| self.video_strip(e.id)).flatten();
                    // the playing video: the presenter's picture shows through an opening; its notes and its strip go over it
                    if playing == Some(e.id) { if let Some(h) = cut { open(scene, h.intersect(r)); } else { scene.fill(Fill::NonZero, Affine::IDENTITY, panel, None, &r); } video(scene, fonts, media, e, r, strip); again = true; continue }
                    match media.view(&e.record, r.width(), r.height()) {
                        Pic::Ready { brush, fade, .. } => { if fade < 1.0 { again = true; scene.fill(Fill::NonZero, Affine::IDENTITY, panel, None, &r); } scene.push_clip_layer(Fill::NonZero, Affine::IDENTITY, &round); min_ui::paint::image(scene, brush, r); scene.pop_layer();
                            // what was drawn ON the picture or the page (its own record's ink, as in a note)
                            if matches!(kind.as_deref(), None | Some("pdf")) && e.record.contains("\"ink\"") { if let Ok(rec) = serde_json::from_str::<serde_json::Value>(&e.record) { ink::page_paint(scene, e.id, &e.record, &rec, r); } }
                            if kind.as_deref() == Some("video") { if let Some(g) = fonts.icon("play", (22.0 * s) as f32, Color::WHITE) { let m = Rect::from_center_size(r.center(), (40.0 * s, 40.0 * s)); if strip.is_none() { scene.fill(Fill::NonZero, Affine::IDENTITY, Color::from_rgba8(0, 0, 0, 0x90), None, &RoundedRect::from_rect(m, 20.0 * s)); draw_layout(scene, &g, Affine::translate((m.center().x - g.width() as f64 / 2.0, m.center().y - g.height() as f64 / 2.0))); } } video(scene, fonts, media, e, r, strip); } }
                        Pic::Loading => { again = true; scene.fill(Fill::NonZero, Affine::IDENTITY, panel, None, &round); }
                        Pic::Missing { name, looked } => words(scene, fonts, &format!("Not found \u{2014} {name}"), &looked),
                        Pic::Failed { name, why } => words(scene, fonts, &format!("Cannot be shown \u{2014} {name}"), &why),
                        // a file: the chip a note has - an icon, the name, the size or that it is missing
                        Pic::Other if kind.as_deref() == Some("file") => { let st = media.file_state(&e.record); scene.fill(Fill::NonZero, Affine::IDENTITY, panel, None, &round); let tone = if st.found { text } else { dim };
                            if let Some(g) = fonts.icon(if st.dir { "folder-simple" } else { "file" }, (22.0 * k) as f32, tone) { draw_layout(scene, &g, Affine::translate((r.x0 + 12.0 * k, r.center().y - g.height() as f64 / 2.0))); }
                            let tx = r.x0 + 44.0 * k; let a = fonts.layout_fit(&name, Family::Sans, (14.0 * k).clamp(5.0, 48.0) as f32, 400.0, tone, (r.x1 - tx - 10.0 * k).max(8.0)); draw_layout(scene, &a, Affine::translate((tx, r.y0 + 8.0 * k)));
                            let under = if !st.found { "Missing".to_string() } else if st.dir { "Folder".to_string() } else { st.size.map(|n| if n < 1048576 { format!("{:.0} KB", n as f64 / 1024.0) } else { format!("{:.1} MB", n as f64 / 1048576.0) }).unwrap_or_default() };
                            let b = fonts.layout_fit(&under, Family::Sans, (12.0 * k).clamp(5.0, 40.0) as f32, 400.0, if st.found { dim } else { c(t::NOTICE_VISUAL_COLOR) }, (r.x1 - tx - 10.0 * k).max(8.0)); draw_layout(scene, &b, Affine::translate((tx, r.y0 + 28.0 * k))); }
                        Pic::Other => words(scene, fonts, &name, &kind.unwrap_or_default().to_uppercase()),
                    } }
            }
        }
        if clip.is_some() { scene.pop_layer(); }
        for (id, h) in fits { self.fit_h.insert(id, h); }
        // the selection: an outline a card, the corners of the one card; the marquee
        for id in &self.sel { if let Some(e) = self.get(*id) { scene.stroke(&Stroke::new(1.5 * s), Affine::IDENTITY, accent, None, &self.on_screen(e).inflate(1.0 * s, 1.0 * s)); } }
        if self.edit.is_none() { if let Some((_, hs)) = self.handles() { for (h, ..) in hs { scene.fill(Fill::NonZero, Affine::IDENTITY, c(t::BACKGROUND_BASE_COLOR), None, &h); scene.stroke(&Stroke::new(1.5 * s), Affine::IDENTITY, accent, None, &h); } } }
        // a locked card says so when it is selected; the lines a drag snapped to; the frame being made
        for e in self.els.iter().filter(|e| e.locked && self.sel.contains(&e.id)) { let r = self.on_screen(e); if let Some(g) = fonts.icon("lock-simple", (13.0 * s) as f32, accent) { let b = Rect::new(r.x1 - 22.0 * s, r.y0 + 4.0 * s, r.x1 - 4.0 * s, r.y0 + 22.0 * s); scene.fill(Fill::NonZero, Affine::IDENTITY, c(t::BACKGROUND_BASE_COLOR).with_alpha(0.85), None, &RoundedRect::from_rect(b, 4.0 * s)); draw_layout(scene, &g, Affine::translate((b.center().x - g.width() as f64 / 2.0, b.center().y - g.height() as f64 / 2.0))); } }
        for (upright, at) in &self.guides { let p = self.to_screen(Point::new(*at, *at)); let l = if *upright { Rect::new(p.x - s / 2.0, rect.y0, p.x + s / 2.0, rect.y1) } else { Rect::new(rect.x0, p.y - s / 2.0, rect.x1, p.y + s / 2.0) }; scene.fill(Fill::NonZero, Affine::IDENTITY, accent.with_alpha(0.7), None, &l); }
        if let Some(Act::Frame { from, to }) = &self.act { let r = Rect::from_points(self.to_screen(*from), self.to_screen(*to)); scene.stroke(&Stroke::new(1.5 * s), Affine::IDENTITY, accent, None, &r); }
        // the card entered to be drawn on: a heavier outline
        if let Some(r) = self.inked.and_then(|id| self.get(id)).map(|e| self.on_screen(e)) { scene.stroke(&Stroke::new(3.0 * s), Affine::IDENTITY, accent, None, &r.inflate(2.0 * s, 2.0 * s)); }
        if let Some(Act::Marquee { from, to, .. }) = &self.act { let r = Rect::from_points(self.to_screen(*from), self.to_screen(*to)); scene.fill(Fill::NonZero, Affine::IDENTITY, accent.with_alpha(0.12), None, &r); scene.stroke(&Stroke::new(s.max(1.0)), Affine::IDENTITY, accent, None, &r); }
        // the stroke being drawn, as it will be kept; the eraser's ring
        if let Some(Act::Draw { tool, from, to, sm, marks, pressed, .. }) = &self.act { let pen = sm.as_ref().map(|s| s.points().iter().map(|p| [p[0] * 1000.0, p[1] * 1000.0]).collect());
            if let Some(l) = self.line_of(*tool, *from, *to, pen, marks, *pressed) { let o = self.to_screen(Point::ORIGIN); ink::drawn(&l, Point::ORIGIN).paint(scene, Affine::translate((o.x, o.y)) * Affine::scale(k)); } }
        if self.tool == Some(Ink::Eraser) && !self.space && !self.hand { if let Some((x, y)) = self.hover.filter(|(x, y)| rect.contains(Point::new(*x, *y))) { let ring = kurbo::Circle::new((x, y), self.rub_r() * k);
            scene.stroke(&Stroke::new(3.0 * s), Affine::IDENTITY, c(t::BACKGROUND_BASE_COLOR).with_alpha(0.6), None, &ring); scene.stroke(&Stroke::new(1.2 * s), Affine::IDENTITY, text, None, &ring); } }
        // the words being typed, at the card's place and at the zoom's size
        let place = self.edit.as_ref().and_then(|(id, _)| self.get(*id)).map(|e| { let r = self.on_screen(e); if e.kind == "frame" { Rect::new(r.x0, r.y0 - 2.0 * k, r.x1, r.y0 + FRAME_TITLE * k + 6.0 * k) } else { r } });
        if let (Some((_, f)), Some(r)) = (self.edit.as_mut(), place) { f.layout(fonts, r, k); f.paint(scene, fonts, k); again |= f.animating(now); }
        if self.els.is_empty() && self.edit.is_none() { let l = fonts.layout("Drop pictures, videos, PDFs or files here \u{2014} or double-click to type", Family::Sans, (14.0 * s) as f32, 400.0, dim); draw_layout(scene, &l, Affine::translate((rect.center().x - l.width() as f64 / 2.0, rect.center().y - l.height() as f64 / 2.0))); }
        scene.pop_layer();
        // the zoom's controls, at the view's bottom right (they are there without modifiers or a pinch: a remote desktop passes neither)
        let pc = format!("{} %", (self.cam.2 * 100.0).round() as i64); self.zoom_pc.label = Some(pc); self.b_hand.selected = self.hand;
        let b = t::ACTION_BUTTON_HEIGHT_S as f64 * s; let (pw, _) = self.zoom_pc.measure(fonts, s); let widths = [b, b, pw.max(56.0 * s), b, b]; let total: f64 = widths.iter().sum::<f64>() + 4.0 * 2.0 * s;
        let bar = Rect::new(rect.x1 - 12.0 * s - total - 8.0 * s, rect.y1 - 12.0 * s - b - 8.0 * s, rect.x1 - 12.0 * s, rect.y1 - 12.0 * s);
        min_ui::paint::rounded(scene, bar, 8.0 * s, t::POPOVER_BORDER_COLOR); scene.fill(Fill::NonZero, Affine::IDENTITY, c(t::POPOVER_BACKGROUND_COLOR), None, &RoundedRect::from_rect(bar.inset(-s), 7.0 * s));
        let mut x = bar.x0 + 4.0 * s; let _ = (line, Vec2::ZERO);
        for (btn, w) in self.buttons().into_iter().zip(widths) { btn.rect = Rect::new(x, bar.y0 + 4.0 * s, x + w, bar.y0 + 4.0 * s + b); x += w + 2.0 * s; btn.paint(scene, fonts, s, now); again |= btn.animating(now); }
        again
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn press(c: &mut Canvas, f: &mut Fonts, pts: &[(f64, f64)]) { let b = min_ui::Button::Primary; c.pointer(&Event::Down { x: pts[0].0, y: pts[0].1, button: b }, false, false, f); for p in &pts[1..] { c.pointer(&Event::Move { x: p.0, y: p.1 }, false, false, f); } let l = pts[pts.len() - 1]; c.pointer(&Event::Up { x: l.0, y: l.1, button: b }, false, false, f); }
    #[test]
    fn ink_is_drawn_picked_on_its_line_rubbed_out_restyled_and_brought_back() {
        let mut c = sheet(); let mut f = Fonts::new(); c.cam = (500.0, 400.0, 1.0);   // a point of the window is the same point of the sheet
        // a line, an arrow, a rectangle, an oval: each one element, none selected after it; a press that goes nowhere draws nothing
        c.arm(Some(Ink::Line)); press(&mut c, &mut f, &[(100.0, 100.0), (300.0, 300.0)]); press(&mut c, &mut f, &[(50.0, 50.0), (50.5, 50.0)]);
        c.arm(Some(Ink::Rect)); press(&mut c, &mut f, &[(400.0, 100.0), (600.0, 200.0)]); c.arm(Some(Ink::Oval)); press(&mut c, &mut f, &[(400.0, 300.0), (600.0, 400.0)]);
        assert_eq!(c.els.len(), 3); assert!(c.sel.is_empty() && c.els.iter().all(|e| e.kind == "ink")); let l = c.inks(); let o = &l[2].s.points; assert!((o[0][0] - 500.0).abs() < 0.01 && (o[0][1] - 350.0).abs() < 0.01 && (o[1][0] - 100.0).abs() < 0.01 && (o[1][1] - 50.0).abs() < 0.01, "{o:?}"); assert_eq!(l[0].s.width, 6.0);
        // the pen: a line through the samples, its pressure kept when the pen tells one, ending on the pointer
        c.arm(Some(Ink::Pen)); c.pressure = Some(0.2); c.pointer(&Event::Down { x: 100.0, y: 600.0, button: min_ui::Button::Primary }, false, false, &mut f);
        for i in 1..=40 { std::thread::sleep(std::time::Duration::from_millis(2)); c.pressure = Some(0.2 + 0.02 * i as f64); c.pointer(&Event::Move { x: 100.0 + 10.0 * i as f64, y: 600.0 }, false, false, &mut f); }
        c.pointer(&Event::Up { x: 500.0, y: 600.0, button: min_ui::Button::Primary }, false, false, &mut f); c.pressure = None;
        let pen = c.inks().pop().unwrap(); let (a, z) = (pen.s.points.clone(), pen.pressures.clone().unwrap()); assert!(a.len() >= 2 && a.len() == z.len() && (a[0][0] - 100.0).abs() < 0.01 && (a[a.len() - 1][0] - 500.0).abs() < 2.0, "{a:?}"); assert!(z[0] < 0.3 && z[z.len() - 1] > 0.9, "{z:?}");
        // the pointer picks a stroke on its line - the diagonal's box alone is not the line
        c.arm(None); assert!(c.tool.is_none()); assert_eq!(c.at(200.0, 201.0), Some(c.els[0].id)); assert_eq!(c.at(280.0, 120.0), None); assert_eq!(c.at(500.0, 150.0), None, "inside the rectangle's outline");
        // restyled while selected: one step, the box follows the width
        c.sel = vec![c.els[0].id]; let w0 = c.els[0].w; assert!(c.restyle(Some([0.0, 0.0, 1.0, 1.0]), Some(22.0))); let l = &c.inks()[0]; assert_eq!((l.s.width, l.s.color[2]), (22.0, 1.0)); assert_eq!(c.els[0].w, w0 + 16.0); assert!((l.s.points[1][0] - 300.0).abs() < 0.01); c.undo(); assert_eq!(c.inks()[0].s.width, 6.0);
        // the eraser rubbed across the pen's line: two lines where one was, one step; across the rectangle's edge: it goes whole
        c.sel.clear(); c.arm(Some(Ink::Eraser)); let n = c.undo.len(); press(&mut c, &mut f, &[(300.0, 560.0), (300.0, 600.0), (300.0, 640.0)]); assert_eq!(c.els.len(), 5); assert_eq!(c.undo.len(), n + 1);
        let parts = c.inks(); assert!(parts[3].s.points.last().unwrap()[0] < 296.0 && parts[4].s.points[0][0] > 304.0 && parts[3].pressures.is_some());
        press(&mut c, &mut f, &[(380.0, 150.0), (420.0, 150.0)]); assert_eq!(c.els.len(), 4); c.undo(); c.undo(); assert_eq!(c.els.len(), 4); assert_eq!(c.inks()[3].s.points.len(), a.len());
        // a picture's card, entered: what is drawn inside it goes into ITS record (in its own pixels) and no element is made; the eraser takes it from there
        c.arm(None); c.add_media(vec![r#"{"src":"a.png","w":1600,"h":800,"name":"a.png"}"#.into()], Some((700.0, 650.0))); let card = *c.sel.last().unwrap(); let n = c.els.len(); let e = c.get(card).unwrap().clone(); assert_eq!((e.w, e.h), (480.0, 240.0));
        c.inked = Some(card); c.arm(Some(Ink::Line)); c.inked = Some(card); press(&mut c, &mut f, &[(e.x + 48.0, e.y + 24.0), (e.x + 240.0, e.y + 120.0)]); assert_eq!(c.els.len(), n); assert_eq!(c.inked, Some(card));
        let rec: serde_json::Value = serde_json::from_str(&c.get(card).unwrap().record).unwrap(); let sh = &rec["ink"]["0"]["shapes"][0]; assert_eq!(sh["points"], serde_json::json!([[0.1, 0.1], [0.5, 0.5]])); assert_eq!(sh["stroke_width"], 20.0, "6 on the sheet, in the picture's own pixels");
        c.arm(Some(Ink::Eraser)); c.inked = Some(card); press(&mut c, &mut f, &[(e.x + 140.0, e.y + 60.0), (e.x + 150.0, e.y + 80.0)]); assert!(!c.get(card).unwrap().record.contains("\"ink\"")); c.undo(); assert!(c.get(card).unwrap().record.contains("\"ink\""));
        // a press outside the card leaves it: the next line is the sheet's
        c.arm(Some(Ink::Line)); c.inked = Some(card); press(&mut c, &mut f, &[(20.0, 700.0), (60.0, 760.0)]); assert_eq!(c.els.len(), n + 1); assert!(c.inked.is_none()); c.undo();
        // Escape gives up the stroke being drawn, then puts the tool down
        c.arm(Some(Ink::Pen)); c.pointer(&Event::Down { x: 10.0, y: 10.0, button: min_ui::Button::Primary }, false, false, &mut f); c.escape(); assert!(c.act.is_none() && c.tool == Some(Ink::Pen)); c.escape(); assert!(c.tool.is_none()); assert_eq!(c.els.len(), 5);
    }
    #[test]
    fn frames_own_their_cards_and_the_basics_line_them_up() {
        let mut c = sheet(); let mut f = Fonts::new(); c.cam = (500.0, 400.0, 1.0); let pic = |n: &str| format!(r#"{{"src":"{n}.png","w":100,"h":100,"name":"{n}"}}"#);
        c.add_media(vec![pic("a")], Some((150.0, 150.0))); c.add_media(vec![pic("b")], Some((350.0, 170.0))); c.add_media(vec![pic("c")], Some((800.0, 600.0))); let (a, b, d) = (c.els[0].id, c.els[1].id, c.els[2].id);
        // Cmd+G: a frame round the two, hugging them (24 all round, 28 more above); it is what is selected
        c.sel = vec![a, b]; assert!(c.group()); let fr = c.sel[0]; assert!(c.is_frame(fr)); let e = c.get(fr).unwrap().clone(); assert_eq!((e.x, e.y, e.w, e.h), (76.0, 48.0, 348.0, 196.0)); assert_eq!(c.kids(fr), vec![a, b]);
        // the frame is taken by its title, not by its inside; it moves with what is in it
        assert_eq!(c.at(120.0, 60.0), Some(fr)); assert_eq!(c.at(250.0, 200.0), None); assert_eq!(c.at(150.0, 150.0), Some(a));
        c.ctrl = true; press(&mut c, &mut f, &[(120.0, 60.0), (140.0, 90.0), (220.0, 160.0)]); assert_eq!((c.get(a).unwrap().x, c.get(fr).unwrap().x), (200.0, 176.0));
        // a member dragged out over the bare sheet leaves the frame, which hugs what is left; dragged back in over it, it joins again
        press(&mut c, &mut f, &[(250.0, 250.0), (400.0, 500.0), (600.0, 700.0)]); assert_eq!(c.get(a).unwrap().parent, None); assert_eq!(c.get(fr).unwrap().w, 148.0);
        let fb = c.get(fr).unwrap().clone(); c.last_click = None; press(&mut c, &mut f, &[(600.0, 700.0), (500.0, 500.0), (fb.x + 30.0, fb.y + 60.0)]); assert_eq!(c.get(a).unwrap().parent, Some(fr));
        // copied with its frame: a new frame with new members; removed with it; brought back
        c.sel = vec![fr]; let clip = c.copy(); assert_eq!(clip.len(), 3); c.paste(&clip, 400.0); assert_eq!(c.els.len(), 7); let f2 = c.sel[0]; assert!(c.is_frame(f2) && c.kids(f2).len() == 2 && c.sel.len() == 1); c.delete(); assert_eq!(c.els.len(), 4); c.undo(); c.undo(); assert_eq!(c.els.len(), 4);
        // given a shape it keeps it and no longer hugs; Fit to Contents hugs again; Shift+Cmd+G lets its members go
        c.sel = vec![fr]; c.frame_shape(16.0 / 9.0); let e = c.get(fr).unwrap().clone(); assert!(e.h == (e.w * 9.0 / 16.0).max(120.0).round() && c.selected_frame().unwrap().1); c.hug(); assert_eq!(c.get(fr).unwrap().h, e.h); c.frame_fit(); assert!(!c.selected_frame().unwrap().1);
        c.frame_tint("teal"); assert_eq!(c.selected_frame().unwrap().2, "teal"); assert!(c.ungroup()); assert_eq!(c.sel.len(), 2); assert!(c.els.iter().all(|e| e.kind != "frame" && e.parent.is_none()));
        // aligned by their left edges, then by their tops; three spread evenly; a locked card stays where it is and is not removed
        c.sel = vec![a, b, d]; c.align(0); let x = c.get(a).unwrap().x.min(c.get(b).unwrap().x); assert!(c.els.iter().all(|e| e.x == x)); c.undo();
        c.get_mut(a).x = 0.0; c.get_mut(b).x = 130.0; c.get_mut(d).x = 600.0; c.sel = vec![a, b, d]; c.distribute(true); assert_eq!(c.get(b).unwrap().x, 300.0);
        c.sel = vec![a]; c.lock(true); c.sel = vec![a, b]; c.align(3); c.nudge(50.0, 0.0); assert_eq!(c.get(a).unwrap().x, 0.0); c.delete(); assert!(c.get(a).is_some() && c.get(b).is_none()); c.select_all(); assert!(!c.sel.contains(&a));
        // a dragged card snaps its edge to another's and shows the line; Shift keeps a line to the eight directions
        c.ctrl = false; let dx = c.get(d).unwrap().clone(); c.cam = (dx.x + 50.0, dx.y + 50.0, 1.0); c.add_media(vec![pic("e")], Some((200.0, 200.0))); let e5 = *c.sel.last().unwrap(); let before = c.get(e5).unwrap().clone();
        let b = min_ui::Button::Primary; let at = c.to_screen(Point::new(before.x + 50.0, before.y + 50.0)); let goal = dx.x - before.x + 4.0;   // 4 short of lining the left edges up
        c.pointer(&Event::Down { x: at.x, y: at.y, button: b }, false, false, &mut f); c.pointer(&Event::Move { x: at.x + goal / 2.0, y: at.y + 300.0 }, false, false, &mut f); c.pointer(&Event::Move { x: at.x + goal, y: at.y + 300.0 }, false, false, &mut f);
        assert_eq!(c.get(e5).unwrap().x, dx.x, "snapped"); assert!(c.guides.iter().any(|g| g.0 && g.1 == dx.x)); c.pointer(&Event::Up { x: at.x + goal, y: at.y + 300.0, button: b }, false, false, &mut f); assert!(c.guides.is_empty());
        c.shift = true; let p = c.aim(Ink::Line, Point::new(0.0, 0.0), Point::new(100.0, 8.0)); assert!(p.y.abs() < 1e-9 && (p.x - 100.32).abs() < 0.1); let q = c.aim(Ink::Rect, Point::new(0.0, 0.0), Point::new(100.0, -40.0)); assert_eq!((q.x, q.y), (100.0, -100.0));
        // the frame tool: a click an empty frame that hugs, a drag one with that size
        c.shift = false; c.framing = true; press(&mut c, &mut f, &[(100.0, 100.0), (100.0, 100.0)]); assert!(!c.framing && !c.selected_frame().unwrap().1); c.framing = true; press(&mut c, &mut f, &[(600.0, 100.0), (900.0, 400.0)]); let (_, fixed, _) = c.selected_frame().unwrap(); assert!(fixed);
    }
    fn sheet() -> Canvas { let mut c = Canvas::new(BlockId::new(), mn_store::Canvas::default()); c.rect = Rect::new(0.0, 0.0, 1000.0, 800.0); c.scale = 1.0; c.fit = false; c }
    #[test]
    fn things_are_put_on_moved_stacked_copied_and_brought_back() {
        let mut c = sheet(); c.add_media(vec![r#"{"src":"a.png","w":960,"h":480,"name":"a.png"}"#.into(), r#"{"kind":"file","src":"b.zip","name":"b.zip"}"#.into()], Some((500.0, 400.0)));
        assert_eq!(c.els.len(), 2); assert_eq!((c.els[0].w, c.els[0].h), (480.0, 240.0)); assert_eq!((c.els[1].w, c.els[1].h), (280.0, 56.0)); assert_eq!(c.sel.len(), 2); assert!(c.dirty && c.can_undo());
        // the camera: a point of the sheet stays under the pointer through a zoom; the zoom keeps to its limits
        let w = c.to_world(700.0, 300.0); c.zoom_by(2.0, (700.0, 300.0)); let n = c.to_world(700.0, 300.0); assert!((w.x - n.x).abs() < 1e-9 && (w.y - n.y).abs() < 1e-9); assert_eq!(c.zoom(), 2.0);
        c.zoom_by(100.0, (0.0, 0.0)); assert_eq!(c.zoom(), ZOOM.1); c.zoom_to(1.0); assert_eq!(c.zoom(), 1.0);
        // the one in front is the one hit; behind and in front change that
        // (the two are put one over the other for this)
        c.els[1].x = c.els[0].x + 20.0; c.els[1].y = c.els[0].y + 20.0; c.undo = vec![];
        let (a, b) = (c.els[0].id, c.els[1].id); let p = c.to_screen(Point::new(c.els[1].x + 5.0, c.els[1].y + 5.0)); assert_eq!(c.at(p.x, p.y), Some(b));
        c.sel = vec![b]; c.restack(false); assert_eq!(c.at(p.x, p.y), Some(a)); c.undo(); assert_eq!(c.at(p.x, p.y), Some(b));
        // nudged, copied in (a little further on, selected), removed, brought back
        c.sel = vec![a]; let x = c.els[0].x; c.nudge(10.0, 0.0); assert_eq!(c.els[0].x, x + 10.0);
        let clip = c.copy(); c.paste(&clip, 24.0); assert_eq!(c.els.len(), 3); assert_eq!(c.els[2].x, x + 34.0); assert_eq!(c.sel, vec![c.els[2].id]);
        c.select_all(); c.delete(); assert!(c.els.is_empty()); c.undo(); assert_eq!(c.els.len(), 3); c.undo(); c.undo(); assert_eq!(c.els[0].x, x); c.redo(); assert_eq!(c.els[0].x, x + 10.0);
        // as it is kept: the elements, and the camera once someone moved it; a camera nobody moved follows the content
        assert_eq!(c.stored().elements.len(), 2); assert!(c.stored().camera.is_some());
        let mut f = sheet(); f.fit = true; f.add_media(vec![r#"{"src":"a.png","w":4000,"h":400,"name":"a"}"#.into(); 3], None); assert!(f.stored().camera.is_none());
        let b = f.bounds(None).unwrap(); let (l, r) = (f.to_screen(Point::new(b.x0, b.y0)), f.to_screen(Point::new(b.x1, b.y1))); assert!(l.x >= 0.0 && r.x <= 1000.0 && l.y >= 0.0 && r.y <= 800.0, "all of it is in view: {l:?} {r:?}");
    }
}
