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

pub const ZOOM: (f64, f64) = (0.05, 4.0);
const HANDLE: f64 = 9.0;
/// Under this zoom a card is its box and its name only.
const FAR: f64 = 0.12;
fn c(k: min_theme::Color) -> Color { min_ui::paint::color(k) }

enum Act { Pan { from: (f64, f64), cam: (f64, f64) }, Move { from: Point, orig: Vec<(BlockId, f64, f64)>, moved: bool }, Resize { id: BlockId, left: bool, top: bool, orig: Rect, keep: bool, moved: bool }, Marquee { from: Point, to: Point, add: bool } }

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
}

/// A media record's kind (`file`, `pdf`, `video`; None: a picture).
fn kind_of(e: &Element) -> Option<String> { if e.kind != "media" { return None } serde_json::from_str::<serde_json::Value>(&e.record).ok().and_then(|v| v.get("kind").and_then(|x| x.as_str()).map(str::to_string)) }
fn name_of(e: &Element) -> String { if e.kind == "text" { e.record.lines().next().unwrap_or("").to_string() } else { serde_json::from_str::<serde_json::Value>(&e.record).ok().and_then(|v| v.get("name").and_then(|n| n.as_str()).map(str::to_string)).unwrap_or_default() } }
fn small(icon: &'static str, tip: &str) -> ActionButton { let mut b = ActionButton::new(icon, tip).size_s(); b.icon_size = Some(t::ICON_SIZE_SMALL); b }

impl Canvas {
    pub fn new(id: BlockId, c: mn_store::Canvas) -> Canvas {
        let mut pc = ActionButton::new("", "Zoom to 100 %").size_s(); pc.label = Some("100 %".into());
        Canvas { id, els: c.elements, sel: vec![], cam: c.camera.unwrap_or((0.0, 0.0, 1.0)), fit: c.camera.is_none(), dirty: false, rect: Rect::ZERO, scale: 1.0, act: None, undo: vec![], redo: vec![], edit: None, back: None,
            space: false, hand: false, last_click: None, fit_h: Default::default(), entered: None, want_note: None, opened: None, zoom_out: small("minus", "Zoom out"), zoom_pc: pc, zoom_in: small("plus", "Zoom in"), zoom_fit: small("arrows-in-simple", "Fit everything"), b_hand: small("hand", "Pan (hold Space)") }
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
    fn after_history(&mut self) { self.dirty = true; let ids: Vec<BlockId> = self.els.iter().map(|e| e.id).collect(); self.sel.retain(|s| ids.contains(s)); }
    fn top_z(&self) -> i64 { self.els.iter().map(|e| e.z).max().unwrap_or(0) }
    fn get(&self, id: BlockId) -> Option<&Element> { self.els.iter().find(|e| e.id == id) }
    /// The element in front at a window point.
    fn at(&self, x: f64, y: f64) -> Option<BlockId> { let mut v: Vec<&Element> = self.els.iter().collect(); v.sort_by_key(|e| e.z); v.into_iter().rev().find(|e| self.on_screen(e).contains(Point::new(x, y))).map(|e| e.id) }
    /// Something is put on the canvas with its centre at a window point (None: the view's centre); it is selected.
    pub fn add(&mut self, kind: &str, record: String, w: f64, h: f64, at: Option<(f64, f64)>, nth: usize) -> BlockId {
        let m = self.rect.center(); let p = at.map(|(x, y)| self.to_world(x, y)).unwrap_or(self.to_world(m.x, m.y)); let off = nth as f64 * 24.0;
        let id = BlockId::new(); let z = self.top_z() + 1;
        self.els.push(Element { id, kind: kind.to_string(), x: (p.x - w / 2.0 + off).round(), y: (p.y - h / 2.0 + off).round(), w, h, z, parent: None, record }); if nth == 0 { self.sel.clear(); } self.sel.push(id); id
    }
    /// Media records (as a note's blocks have them) are put on the canvas side by side about a window point (None:
    /// the view's centre), one step of the history; they are selected.
    pub fn add_media(&mut self, records: Vec<String>, at: Option<(f64, f64)>) {
        if records.is_empty() { return } self.edit_end(true); self.step();
        let sized: Vec<(String, f64, f64)> = records.into_iter().map(|r| { let v: serde_json::Value = serde_json::from_str(&r).unwrap_or_default();
            let (w, h) = match (v.get("w").and_then(|x| x.as_f64()), v.get("h").and_then(|x| x.as_f64())) { (Some(w), Some(h)) if w > 0.0 && h > 0.0 => { let f = (480.0 / w).min(480.0 / h).min(1.0); ((w * f).round(), (h * f).round()) } _ => (280.0, 56.0) }; (r, w, h) }).collect();
        let m = self.rect.center(); let p = at.map(|(x, y)| self.to_world(x, y)).unwrap_or(self.to_world(m.x, m.y)); let gap = 24.0;
        let total: f64 = sized.iter().map(|x| x.1).sum::<f64>() + gap * (sized.len() - 1) as f64; let mut x = p.x - total / 2.0; self.sel.clear();
        for (r, w, h) in sized { let id = BlockId::new(); let z = self.top_z() + 1; self.els.push(Element { id, kind: "media".into(), x: x.round(), y: (p.y - h / 2.0).round(), w, h, z, parent: None, record: r }); self.sel.push(id); x += w + gap; }
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
    /// The one selected card, when it is of this kind of media (`file`, `pdf`, `video`).
    pub fn selected_media(&self, kind: &str) -> Option<&Element> { let [id] = self.sel[..] else { return None }; self.get(id).filter(|e| kind_of(e).as_deref() == Some(kind)) }
    /// Text from the clipboard becomes a text card at the view's centre.
    pub fn add_text(&mut self, text: &str) { self.edit_end(true); self.step(); let lines = text.lines().count().max(1) as f64; self.add("text", text.trim_end().to_string(), 280.0, (lines * 22.0 + 24.0).clamp(60.0, 480.0), None, 0); }
    /// A right press: the card under the pointer is selected, unless it is one of those selected already; none under it: nothing is.
    pub fn pick_at(&mut self, x: f64, y: f64) { self.edit_end(true); match self.at(x, y) { Some(id) => if !self.sel.contains(&id) { self.sel = vec![id]; }, None => self.sel.clear() } }
    pub fn select_all(&mut self) { self.sel = self.els.iter().map(|e| e.id).collect(); }
    pub fn delete(&mut self) { if self.sel.is_empty() { return } self.step(); let s = std::mem::take(&mut self.sel); self.els.retain(|e| !s.contains(&e.id)); }
    pub fn nudge(&mut self, dx: f64, dy: f64) { if self.sel.is_empty() { return } self.step(); for e in self.els.iter_mut().filter(|e| self.sel.contains(&e.id)) { e.x += dx; e.y += dy; } }
    /// The selected elements, for the clipboard.
    pub fn copy(&self) -> Vec<Element> { self.els.iter().filter(|e| self.sel.contains(&e.id)).cloned().collect() }
    /// Elements come in (a paste, a duplicate) a little down and to the right of where they were, in front, selected.
    pub fn paste(&mut self, clip: &[Element], shift: f64) {
        if clip.is_empty() { return } self.edit_end(true); self.step(); let mut z = self.top_z(); self.sel.clear(); let mut v: Vec<&Element> = clip.iter().collect(); v.sort_by_key(|e| e.z);
        for e in v { z += 1; let id = BlockId::new(); self.els.push(Element { id, x: e.x + shift, y: e.y + shift, z, parent: None, ..e.clone() }); self.sel.push(id); }
    }
    /// The selection goes in front of everything (or behind it).
    pub fn restack(&mut self, front: bool) {
        if self.sel.is_empty() { return } self.step(); let mut order: Vec<BlockId> = { let mut v: Vec<&Element> = self.els.iter().collect(); v.sort_by_key(|e| e.z); v.iter().map(|e| e.id).collect() };
        let (mut picked, rest): (Vec<BlockId>, Vec<BlockId>) = order.drain(..).partition(|id| self.sel.contains(id)); let mut all = if front { let mut r = rest; r.append(&mut picked); r } else { picked.extend(rest); picked };
        for (i, id) in all.drain(..).enumerate() { if let Some(e) = self.els.iter_mut().find(|e| e.id == id) { e.z = i as i64 + 1; } }
    }

    // -- a text card's words

    fn edit_begin(&mut self, id: BlockId, fonts: &mut Fonts) {
        self.edit_end(true); let Some(e) = self.get(id).filter(|e| e.kind == "text") else { return };
        let mut f = TextArea::new(Family::Sans, 14.0); f.set_text(&e.record); f.focused = true; let _ = fonts; self.edit = Some((id, f)); self.sel = vec![id];
    }
    /// The words being typed are kept (`keep`) or given up; a card left empty goes.
    pub fn edit_end(&mut self, keep: bool) {
        let Some((id, f)) = self.edit.take() else { return }; let text = f.text().trim_end().to_string(); let was = self.get(id).map(|e| e.record.clone()).unwrap_or_default();
        let text = if keep { text } else { was.clone() };
        if text.trim().is_empty() { if !was.trim().is_empty() { self.step(); } else { self.dirty = true; } self.els.retain(|e| e.id != id); self.sel.retain(|s| *s != id); }
        else if text != was { self.step(); if let Some(e) = self.els.iter_mut().find(|e| e.id == id) { e.record = text; } }
    }

    // -- the pointer and the keys

    fn handles(&self) -> Option<(BlockId, [(Rect, bool, bool); 4])> {
        let [id] = self.sel[..] else { return None }; let r = self.on_screen(self.get(id)?); let h = HANDLE * self.scale;
        let at = |x: f64, y: f64| Rect::from_center_size((x, y), (h, h));
        Some((id, [(at(r.x0, r.y0), true, true), (at(r.x1, r.y0), false, true), (at(r.x0, r.y1), true, false), (at(r.x1, r.y1), false, false)]))
    }
    fn buttons(&mut self) -> [&mut ActionButton; 5] { [&mut self.b_hand, &mut self.zoom_out, &mut self.zoom_pc, &mut self.zoom_in, &mut self.zoom_fit] }
    /// What the pointer would do at a window point: 0 nothing special, 1 pan, 2 a corner (nwse), 3 a corner (nesw), 4 a control.
    pub fn cursor(&mut self, x: f64, y: f64) -> u8 {
        if self.buttons().iter().any(|b| b.hit(x, y)) { return 4 } if matches!(self.act, Some(Act::Pan { .. })) || self.space || self.hand { return 1 }
        if let Some(Act::Resize { left, top, .. }) = &self.act { return if left == top { 2 } else { 3 } }
        if self.edit.is_none() { if let Some((_, hs)) = self.handles() { if let Some((_, l, tp)) = hs.iter().find(|(r, ..)| r.inflate(3.0, 3.0).contains(Point::new(x, y))) { return if l == tp { 2 } else { 3 } } } }
        0
    }
    /// A pointer event in the canvas's view. `middle`: the middle button. True: draw again.
    pub fn pointer(&mut self, e: &Event, middle: bool, shift: bool, fonts: &mut Fonts) -> bool {
        let now = Instant::now(); let m = self.rect.center();
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
                if let Some((id, hs)) = self.handles() { if let Some((_, left, top)) = hs.iter().find(|(r, ..)| r.inflate(3.0, 3.0).contains(Point::new(x, y))) { let el = self.get(id).unwrap();
                    self.act = Some(Act::Resize { id, left: *left, top: *top, orig: Rect::new(el.x, el.y, el.x + el.w, el.y + el.h), keep: el.kind == "media" && el.h > 60.0, moved: false }); return true } }
                let hit = self.at(x, y);
                let twice = self.last_click.is_some_and(|(t0, h)| h == hit && t0.elapsed().as_millis() < 420); self.last_click = if twice { None } else { Some((now, hit)) };
                match hit {
                    Some(id) if twice => { match self.get(id).map(|e| e.kind.clone()).as_deref() { Some("text") => self.edit_begin(id, fonts), Some("note") => { self.sel = vec![id]; self.entered = Some(id); } Some("link") => self.opened = Some(id), _ if self.get(id).and_then(kind_of).as_deref() == Some("file") => self.opened = Some(id), _ => self.open(id) } }
                    Some(id) => { if shift { if let Some(i) = self.sel.iter().position(|s| *s == id) { self.sel.remove(i); } else { self.sel.push(id); } } else if !self.sel.contains(&id) { self.sel = vec![id]; }
                        self.act = Some(Act::Move { from: w, orig: self.els.iter().filter(|e| self.sel.contains(&e.id)).map(|e| (e.id, e.x, e.y)).collect(), moved: false }); }
                    // twice on the empty sheet: a text card there, ready to be typed in
                    None if twice => { self.want_note = Some(w); }
                    None => { if !shift { self.sel.clear(); } self.act = Some(Act::Marquee { from: w, to: w, add: shift }); }
                }
                true }
            Event::Move { x, y } => { let w = self.to_world(x, y); let k = self.k();
                match self.act.take() {
                    Some(Act::Pan { from, cam }) => { self.cam.0 = cam.0 - (x - from.0) / k; self.cam.1 = cam.1 - (y - from.1) / k; self.fit = false; self.act = Some(Act::Pan { from, cam }); true }
                    Some(Act::Move { from, orig, moved }) => { let d = w - from; let go = moved || d.hypot() * k > 3.0 * self.scale;
                        if go { if !moved { self.step(); } for (id, ox, oy) in &orig { if let Some(e) = self.els.iter_mut().find(|e| e.id == *id) { e.x = (ox + d.x).round(); e.y = (oy + d.y).round(); } } }
                        self.act = Some(Act::Move { from, orig, moved: go }); go }
                    Some(Act::Resize { id, left, top, orig, keep, moved }) => { if !moved { self.step(); }
                        let (mut x0, mut y0, mut x1, mut y1) = (orig.x0, orig.y0, orig.x1, orig.y1); if left { x0 = w.x.min(x1 - 40.0); } else { x1 = w.x.max(x0 + 40.0); } if top { y0 = w.y.min(y1 - 24.0); } else { y1 = w.y.max(y0 + 24.0); }
                        // a picture keeps its shape: the wider of the two pulls decides
                        if keep { let a = orig.width() / orig.height(); let (nw, nh) = (x1 - x0, y1 - y0); let (nw, nh) = if nw / a >= nh { (nw, nw / a) } else { (nh * a, nh) }; if left { x0 = x1 - nw; } else { x1 = x0 + nw; } if top { y0 = y1 - nh; } else { y1 = y0 + nh; } }
                        if let Some(e) = self.els.iter_mut().find(|e| e.id == id) { e.x = x0.round(); e.y = y0.round(); e.w = (x1 - x0).round(); e.h = (y1 - y0).round(); }
                        self.act = Some(Act::Resize { id, left, top, orig, keep, moved: true }); true }
                    Some(Act::Marquee { from, add, .. }) => { self.act = Some(Act::Marquee { from, to: w, add }); true }
                    None => false,
                } }
            Event::Up { .. } => { match self.act.take() {
                    Some(Act::Marquee { from, to, add }) => { let r = Rect::from_points(from, to); if r.width() * self.k() > 3.0 || r.height() * self.k() > 3.0 { let hit: Vec<BlockId> = self.els.iter().filter(|e| r.intersect(Rect::new(e.x, e.y, e.x + e.w, e.y + e.h)).area() > 0.0).map(|e| e.id).collect(); if !add { self.sel.clear(); } for id in hit { if !self.sel.contains(&id) { self.sel.push(id); } } } true }
                    Some(_) => true, None => false } }
            _ => false,
        }
    }
    /// A card is looked at larger: the camera goes to it (Escape goes back to where it was).
    pub fn open(&mut self, id: BlockId) { let Some(r) = self.bounds(Some(&[id])) else { return }; if self.back.is_none() { self.back = Some(self.cam); } let fit = self.fit; self.show(r.inflate(24.0, 24.0)); self.cam.2 = self.cam.2.max(ZOOM.0); self.fit = false; let _ = fit; self.sel = vec![id]; }
    /// Escape: the words being typed are kept and the field closes; else the camera goes back; else nothing stays selected.
    pub fn escape(&mut self) { if self.edit.is_some() { self.edit_end(true); } else if let Some(c) = self.back.take() { self.cam = c; } else { self.sel.clear(); } self.act = None; }
    /// Return: the one selected card is opened (a text card: typed in).
    pub fn enter(&mut self, fonts: &mut Fonts) { if let [id] = self.sel[..] { match self.get(id).map(|e| e.kind.clone()).as_deref() { Some("text") => self.edit_begin(id, fonts), Some("note") => self.entered = Some(id), Some("link") => self.opened = Some(id), Some(_) if self.get(id).and_then(kind_of).as_deref() == Some("file") => self.opened = Some(id), Some(_) => self.open(id), None => {} } } }
    /// A key for the text card being typed in. True: it was the card's.
    pub fn key(&mut self, k: &KeyEvent, fonts: &mut Fonts) -> bool {
        let Some((_, f)) = self.edit.as_mut() else { return false };
        if f.key(k, fonts) == FieldResponse::Cancelled { self.edit_end(true); } true
    }
    pub fn ime(&mut self, e: &min_ui::ImeEvent, fonts: &mut Fonts) -> bool { match self.edit.as_mut() { Some((_, f)) => { f.ime(e, fonts); true } None => false } }

    // -- the picture

    /// Painted into `rect` (window px). True: something is still arriving or moving - draw again.
    /// `live`: the note card whose text the window itself draws (it is being edited); `note`: draws a note card's
    /// content into its box (the box, the units' size on screen, far out) and says how tall the content is (units) and whether to draw again.
    pub fn paint(&mut self, scene: &mut Scene, fonts: &mut Fonts, media: &mut Media, rect: Rect, s: f64, live: Option<BlockId>, note: &mut dyn FnMut(&mut Scene, &mut Fonts, &mut Media, &Element, Rect, f64, bool) -> (f64, bool)) -> bool {
        let first = self.rect.width() < 1.0; self.rect = rect; self.scale = s; if self.fit && (first || self.act.is_none()) && self.edit.is_none() { self.fit_all(); }
        let now = Instant::now(); let mut again = false; let k = self.k();
        scene.push_clip_layer(Fill::NonZero, Affine::IDENTITY, &rect);
        let (panel, line, text, dim, accent) = (c(t::BACKGROUND_LAYER_2_COLOR), c(t::DIVIDER_COLOR), c(t::NEUTRAL_CONTENT_COLOR_DEFAULT), c(t::NEUTRAL_SUBDUED_CONTENT_COLOR_DEFAULT), c(t::ACCENT_BACKGROUND_COLOR_DEFAULT));
        let mut order: Vec<usize> = (0..self.els.len()).collect(); order.sort_by_key(|i| self.els[*i].z); let mut fits: Vec<(BlockId, f64)> = vec![];
        for i in order { let e = &self.els[i]; let r = self.on_screen(e); if r.intersect(rect).area() <= 0.0 { continue }
            let far = self.cam.2 < FAR || r.width() < 28.0 * s; let round = RoundedRect::from_rect(r, (6.0 * k).min(8.0 * s));
            let words = |scene: &mut Scene, fonts: &mut Fonts, head: &str, more: &str| { scene.fill(Fill::NonZero, Affine::IDENTITY, panel, None, &round); if far { return }
                let a = fonts.layout_fit(head, Family::Sans, (13.0 * k).clamp(6.0, 40.0) as f32, 400.0, text, (r.width() - 20.0 * k).max(8.0)); draw_layout(scene, &a, Affine::translate((r.x0 + 10.0 * k, r.y0 + 8.0 * k)));
                if !more.is_empty() && r.height() > 44.0 * k { let b = fonts.layout_fit(more, Family::Sans, (11.0 * k).clamp(5.0, 34.0) as f32, 400.0, dim, (r.width() - 20.0 * k).max(8.0)); draw_layout(scene, &b, Affine::translate((r.x0 + 10.0 * k, r.y0 + 10.0 * k + a.height() as f64))); } };
            match e.kind.as_str() {
                "link" => { scene.fill(Fill::NonZero, Affine::IDENTITY, panel, None, &round); if far { continue } let link = c(t::ACCENT_CONTENT_COLOR_DEFAULT); let mut x = r.x0 + 12.0 * k;
                    if let Some(g) = fonts.icon("link", (18.0 * k) as f32, link) { draw_layout(scene, &g, Affine::translate((x, r.center().y - g.height() as f64 / 2.0))); x += 26.0 * k; }
                    let l = fonts.layout_fit(&e.record, Family::Sans, (13.0 * k).clamp(5.0, 44.0) as f32, 400.0, link, (r.x1 - x - 10.0 * k).max(8.0)); draw_layout(scene, &l, Affine::translate((x, r.center().y - l.height() as f64 / 2.0))); }
                "note" => { scene.fill(Fill::NonZero, Affine::IDENTITY, panel, None, &round); if live == Some(e.id) { continue }
                    let (h, more) = note(scene, fonts, media, e, r, k, far); again |= more; if h > 0.0 { fits.push((e.id, h)); } }
                "text" => { scene.fill(Fill::NonZero, Affine::IDENTITY, panel, None, &round);
                    if self.edit.as_ref().is_some_and(|(id, _)| *id == e.id) || far { continue }
                    scene.push_clip_layer(Fill::NonZero, Affine::IDENTITY, &r);
                    let l = fonts.layout_width(&e.record, Family::Sans, (14.0 * k) as f32, 400.0, text, Some(((r.width() - 20.0 * k).max(8.0)) as f32)); draw_layout(scene, &l, Affine::translate((r.x0 + 10.0 * k, r.y0 + 8.0 * k)));
                    scene.pop_layer(); }
                _ => { let name = name_of(e); let kind = serde_json::from_str::<serde_json::Value>(&e.record).ok().and_then(|v| v.get("kind").and_then(|x| x.as_str()).map(str::to_string));
                    if far { scene.fill(Fill::NonZero, Affine::IDENTITY, panel, None, &round); continue }
                    match media.view(&e.record, r.width(), r.height()) {
                        Pic::Ready { brush, fade, .. } => { if fade < 1.0 { again = true; scene.fill(Fill::NonZero, Affine::IDENTITY, panel, None, &r); } scene.push_clip_layer(Fill::NonZero, Affine::IDENTITY, &round); min_ui::paint::image(scene, brush, r); scene.pop_layer();
                            if kind.as_deref() == Some("video") { if let Some(g) = fonts.icon("play", (22.0 * s) as f32, Color::WHITE) { let m = Rect::from_center_size(r.center(), (40.0 * s, 40.0 * s)); scene.fill(Fill::NonZero, Affine::IDENTITY, Color::from_rgba8(0, 0, 0, 0x90), None, &RoundedRect::from_rect(m, 20.0 * s)); draw_layout(scene, &g, Affine::translate((m.center().x - g.width() as f64 / 2.0, m.center().y - g.height() as f64 / 2.0))); } } }
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
        for (id, h) in fits { self.fit_h.insert(id, h); }
        // the selection: an outline a card, the corners of the one card; the marquee
        for id in &self.sel { if let Some(e) = self.get(*id) { scene.stroke(&Stroke::new(1.5 * s), Affine::IDENTITY, accent, None, &self.on_screen(e).inflate(1.0 * s, 1.0 * s)); } }
        if self.edit.is_none() { if let Some((_, hs)) = self.handles() { for (h, ..) in hs { scene.fill(Fill::NonZero, Affine::IDENTITY, c(t::BACKGROUND_BASE_COLOR), None, &h); scene.stroke(&Stroke::new(1.5 * s), Affine::IDENTITY, accent, None, &h); } } }
        if let Some(Act::Marquee { from, to, .. }) = &self.act { let r = Rect::from_points(self.to_screen(*from), self.to_screen(*to)); scene.fill(Fill::NonZero, Affine::IDENTITY, accent.with_alpha(0.12), None, &r); scene.stroke(&Stroke::new(s.max(1.0)), Affine::IDENTITY, accent, None, &r); }
        // the words being typed, at the card's place and at the zoom's size
        let place = self.edit.as_ref().and_then(|(id, _)| self.get(*id)).map(|e| self.on_screen(e));
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
