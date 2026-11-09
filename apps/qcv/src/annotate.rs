//! Annotations in the picture (canon rows 210 / 214 / 215; QCView `ViewportAnnotator` + WindowManager's annotation slice):
//! single view only; while the Notes panel is open a tool draws on the picture — Freehand (the row-214 smoother), Rectangle,
//! Oval, Arrow, Line — and the release commits the stroke to the note at the playhead's frame (the key: SMPTE non-drop from
//! frame 0), creating it when needed; the sidecar saves on every change (qcv-notes, byte-for-byte QCView). Eraser removes the
//! stroke under the pointer; Select picks one, drags it, Delete removes it. Notes show on their exact frame (row 215).
//! Fixed against QCView (row 215): undo / redo are PER CLIP (cleared on a load — QCView's survived a clip switch and could edit
//! another clip's note at the same timecode); redo restores the whole note (QCView dropped a new note's frame); hit tests use
//! the drawn line, not its bounding box; a click-only arrow / line is not saved; the selection outline is never part of a
//! thumbnail or a screenshot (it is drawn here, on screen only).
use kurbo::{Affine, Cap, Join, ParamCurveNearest, Point, Shape, Stroke as KStroke};
use qcv_notes::{ink, io, strokes::{self, Stroke}, Note, Tool as STool};
use std::path::{Path, PathBuf};
use vello::Scene;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tool { Select, Freehand, Rectangle, Oval, Arrow, Line, Eraser }
impl Tool { fn shape(self) -> Option<STool> { Some(match self { Tool::Freehand => STool::Freehand, Tool::Rectangle => STool::Rectangle, Tool::Oval => STool::Oval, Tool::Arrow => STool::Arrow, Tool::Line => STool::Line, _ => return None }) } }

/// One undo step: the note at `key` before and after (None = absent).
struct Step { key: String, before: Option<Note>, after: Option<Note> }

enum Live { Ink(ink::Smoother, bool), Shape { tool: STool, a: [f64; 2], b: [f64; 2] } }
struct Drag { id: String, from: [f64; 2], last: [f64; 2], moved: bool, before: Note }

/// A committed edit the app follows up: the note's key, and whether its clean thumbnail must be captured (a new note).
pub struct Committed { pub key: String, pub new_note: bool }

pub struct Annotations {
    pub active: bool, pub tool: Tool, pub color: [f32; 4], pub width: f32,
    pub media: Option<PathBuf>, pub notes: Vec<Note>, pub selected: Option<String>,
    live: Option<Live>, drag: Option<Drag>, undo: Vec<Step>, redo: Vec<Step>, t0: std::time::Instant,
}

impl Annotations {
    pub fn new() -> Annotations { Annotations { active: false, tool: Tool::Freehand, color: [1.0, 0.0, 0.0, 1.0], width: 6.0, media: None, notes: vec![], selected: None, live: None, drag: None, undo: vec![], redo: vec![], t0: std::time::Instant::now() } }
    /// A media file loaded (or none): its notes from the sidecar; the history starts over (per clip).
    pub fn load(&mut self, media: Option<&Path>) {
        self.media = media.map(Path::to_path_buf); self.live = None; self.drag = None; self.selected = None; self.undo.clear(); self.redo.clear();
        self.notes = match media { Some(m) => io::load(m).unwrap_or_else(|e| { log::warn!("notes: {e}"); vec![] }), None => vec![] };
    }
    pub fn save(&self) { if let Some(m) = &self.media { if let Err(e) = io::save(m, &self.notes) { log::warn!("notes save: {e}"); } } }
    pub fn note_at_frame(&self, frame: i32) -> Option<&Note> { self.notes.iter().find(|n| n.frame == frame as i64) }
    pub fn strokes_at(&self, frame: i32) -> Vec<Stroke> { self.note_at_frame(frame).map(|n| strokes::parse(n.annotation.as_ref())).unwrap_or_default() }
    pub fn drawing(&self) -> bool { self.live.is_some() || self.drag.is_some() }
    fn t(&self) -> f64 { self.t0.elapsed().as_secs_f64() }

    // ---- pointer (normalized picture coordinates; outside 0..1 allowed) ----
    pub fn press(&mut self, p: [f64; 2], frame: i32, geo: &Geo) {
        match self.tool {
            Tool::Freehand => self.live = Some(Live::Ink(ink::Smoother::begin(p, self.t()), inside(p))),
            Tool::Eraser => {}
            Tool::Select => {
                self.selected = self.hit(p, frame, geo);
                if let (Some(id), Some(n)) = (self.selected.clone(), self.note_at_frame(frame).cloned()) { self.drag = Some(Drag { id, from: p, last: p, moved: false, before: n }); }
            }
            t => if let Some(tool) = t.shape() { self.live = Some(Live::Shape { tool, a: p, b: p }); },
        }
    }
    /// Returns a committed edit (the eraser commits as it passes over strokes).
    pub fn motion(&mut self, p: [f64; 2], frame: i32, fps: f64, geo: &Geo, pressed: bool) -> Option<Committed> {
        let t = self.t();
        match &mut self.live {
            Some(Live::Ink(s, touched)) => { s.push(p, t); *touched |= inside(p); return None }
            Some(Live::Shape { b, .. }) => { *b = p; return None }
            None => {}
        }
        if let Some(d) = &mut self.drag {
            let delta = [p[0] - d.last[0], p[1] - d.last[1]]; d.last = p; d.moved = true;
            let id = d.id.clone();
            if let Some(n) = self.notes.iter_mut().find(|n| n.frame == frame as i64) {
                let mut ss = strokes::parse(n.annotation.as_ref());
                if let Some(s) = ss.iter_mut().find(|s| s.id == id) {
                    // QCView: the move is clamped so the stroke's visible bounds stay on the picture
                    let (mut x0, mut y0, mut x1, mut y1) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
                    for q in s.points.iter() { x0 = x0.min(q[0]); y0 = y0.min(q[1]); x1 = x1.max(q[0]); y1 = y1.max(q[1]); }
                    if s.tool == STool::Oval { let c = s.points[0]; let r = s.points.get(1).copied().unwrap_or([0.0, 0.0]); x0 = c[0] - r[0]; x1 = c[0] + r[0]; y0 = c[1] - r[1]; y1 = c[1] + r[1]; }
                    let dx = delta[0].clamp((-x0).min(0.0), (1.0 - x1).max(0.0)); let dy = delta[1].clamp((-y0).min(0.0), (1.0 - y1).max(0.0));   // bounds always span 0: a stroke already partly off the picture can still move back
                    let n_pts = if s.tool == STool::Oval { 1 } else { s.points.len() };
                    for q in s.points.iter_mut().take(n_pts) { q[0] += dx; q[1] += dy; }
                }
                n.annotation = Some(strokes::to_data(&ss));
            }
            return None
        }
        if pressed && self.tool == Tool::Eraser { return self.erase(p, frame, fps, geo) }
        None
    }
    pub fn release(&mut self, p: [f64; 2], frame: i32, fps: f64, geo: &Geo) -> Option<Committed> {
        if let Some(d) = self.drag.take() {
            if !d.moved { return None }
            let key = d.before.timecode.clone(); let after = self.notes.iter().find(|n| n.timecode == key).cloned();
            self.push_step(Step { key: key.clone(), before: Some(d.before), after }); self.save();
            return Some(Committed { key, new_note: false })
        }
        let live = self.live.take()?;
        let (tool, points) = match live {
            Live::Ink(s, touched) => { let pts = s.end(p, self.t()); if !touched && !inside(p) { return None } (STool::Freehand, pts) }
            Live::Shape { tool, a, b: _ } => {
                let b = p;
                let pts = match tool {
                    STool::Rectangle => { let (x0, y0, x1, y1) = (a[0].min(b[0]), a[1].min(b[1]), a[0].max(b[0]), a[1].max(b[1])); vec![[x0, y0], [x1, y0], [x1, y1], [x0, y1]] }
                    STool::Oval => vec![[(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0], [(b[0] - a[0]).abs() / 2.0, (b[1] - a[1]).abs() / 2.0]],
                    _ => vec![a, b],
                };
                // a click without a drag makes nothing (QCView saved an invisible one-point arrow / line); a shape off the picture drops
                let span = ((b[0] - a[0]) * geo.rect[2]).hypot((b[1] - a[1]) * geo.rect[3]);
                if span < 3.0 { return None }
                let (x0, y0, x1, y1) = (a[0].min(b[0]), a[1].min(b[1]), a[0].max(b[0]), a[1].max(b[1]));
                if x1 < 0.0 || y1 < 0.0 || x0 > 1.0 || y0 > 1.0 { return None }
                (tool, pts)
            }
        };
        let stroke = Stroke { id: strokes::new_id(), tool, color: Stroke::qcolor(self.color), width: self.width as f64, filled: false, is_modeled: tool == STool::Freehand, points };
        Some(self.add_stroke(stroke, frame, fps))
    }
    /// The stroke joins the note at the playhead (created if absent: QCView's `onStrokeFinalized`).
    fn add_stroke(&mut self, s: Stroke, frame: i32, fps: f64) -> Committed {
        let (key, secs) = qcv_notes::note_key(frame, fps);
        let before = self.notes.iter().find(|n| n.timecode == key).cloned();
        let new_note = before.is_none();
        let mut n = before.clone().unwrap_or_else(|| Note { timecode: key.clone(), timestamp_seconds: secs, frame: frame as i64, image: format!("images/{}", io::image_filename(&key)), ..Default::default() });
        let mut ss = strokes::parse(n.annotation.as_ref()); ss.push(s); n.annotation = Some(strokes::to_data(&ss));
        self.put(n.clone());
        self.push_step(Step { key: key.clone(), before, after: Some(n) }); self.save();
        Committed { key, new_note }
    }
    fn put(&mut self, n: Note) { match self.notes.iter_mut().find(|x| x.timecode == n.timecode) { Some(x) => *x = n, None => { self.notes.push(n); self.notes.sort_by(|a, b| a.timecode.cmp(&b.timecode)); } } }
    fn remove_note(&mut self, key: &str) { self.notes.retain(|n| n.timecode != key); }
    fn push_step(&mut self, s: Step) { self.undo.push(s); self.redo.clear(); }
    fn erase(&mut self, p: [f64; 2], frame: i32, _fps: f64, geo: &Geo) -> Option<Committed> {
        let id = self.hit(p, frame, geo)?;
        let before = self.note_at_frame(frame)?.clone();
        let mut n = before.clone(); let mut ss = strokes::parse(n.annotation.as_ref()); ss.retain(|s| s.id != id); n.annotation = Some(strokes::to_data(&ss));
        let key = n.timecode.clone(); self.put(n.clone());
        self.push_step(Step { key: key.clone(), before: Some(before), after: Some(n) }); self.save();
        Some(Committed { key, new_note: false })
    }
    /// Delete / Backspace with Select: the selected stroke goes.
    pub fn delete_selected(&mut self, frame: i32) -> Option<Committed> {
        let id = self.selected.take()?;
        let before = self.note_at_frame(frame)?.clone();
        let mut n = before.clone(); let mut ss = strokes::parse(n.annotation.as_ref()); ss.retain(|s| s.id != id); n.annotation = Some(strokes::to_data(&ss));
        let key = n.timecode.clone(); self.put(n.clone());
        self.push_step(Step { key: key.clone(), before: Some(before), after: Some(n) }); self.save();
        Some(Committed { key, new_note: false })
    }
    /// QCView: Escape cancels the stroke in progress, else clears the selection. True when it did something.
    pub fn escape(&mut self) -> bool { if self.live.take().is_some() { return true } self.selected.take().is_some() }
    pub fn undo(&mut self) -> Option<Committed> { let s = self.undo.pop()?; let key = s.key.clone(); match &s.before { Some(n) => self.put(n.clone()), None => self.remove_note(&key) } self.redo.push(s); self.save(); self.selected = None; Some(Committed { key, new_note: false }) }
    pub fn redo(&mut self) -> Option<Committed> { let s = self.redo.pop()?; let key = s.key.clone(); match &s.after { Some(n) => self.put(n.clone()), None => self.remove_note(&key) } self.undo.push(s); self.save(); self.selected = None; Some(Committed { key, new_note: false }) }

    /// The stroke under `p` — the drawn line within its half-width + 4 px, topmost first.
    fn hit(&self, p: [f64; 2], frame: i32, geo: &Geo) -> Option<String> {
        let pt = geo.to_px(p);
        for s in self.strokes_at(frame).iter().rev() {
            let g = ink::geometry(s, geo.rect, geo.px_per_src);
            let reach = g.width / 2.0 + 4.0;
            let near = g.paths.iter().chain(g.head.iter()).any(|path| path.segments().any(|seg| seg.nearest(pt, 0.1).distance_sq <= reach * reach))
                || g.head.as_ref().is_some_and(|h| h.contains(pt));
            if near { return Some(s.id.clone()) }
        }
        None
    }

    /// Draw the frame's strokes, the live one and the selection outline.
    pub fn paint(&self, scene: &mut Scene, frame: i32, geo: &Geo) {
        for s in self.strokes_at(frame) { draw(scene, &s, geo); }
        match &self.live {
            Some(Live::Ink(sm, _)) => { let s = Stroke { id: String::new(), tool: STool::Freehand, color: Stroke::qcolor(self.color), width: self.width as f64, filled: false, is_modeled: true, points: sm.points().to_vec() }; draw(scene, &s, geo); }
            Some(Live::Shape { tool, a, b }) => {
                let pts = match tool { STool::Rectangle => vec![*a, [b[0], a[1]], *b, [a[0], b[1]]], STool::Oval => vec![[(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0], [(b[0] - a[0]).abs() / 2.0, (b[1] - a[1]).abs() / 2.0]], _ => vec![*a, *b] };
                draw(scene, &Stroke { id: String::new(), tool: *tool, color: Stroke::qcolor(self.color), width: self.width as f64, filled: false, is_modeled: false, points: pts }, geo);
            }
            None => {}
        }
        // the selection: QCView's #0189f1 outline + corner handles, here on screen only
        if let Some(id) = &self.selected { if let Some(s) = self.strokes_at(frame).into_iter().find(|s| &s.id == id) {
            let g = ink::geometry(&s, geo.rect, geo.px_per_src);
            let mut bb = g.paths.iter().chain(g.head.iter()).map(|p| p.bounding_box()).reduce(|a, b| a.union(b)).unwrap_or_default();
            bb = bb.inflate(g.width / 2.0 + 3.0, g.width / 2.0 + 3.0);
            let blue = peniko::Color::from_rgba8(0x01, 0x89, 0xf1, 0xff);
            scene.stroke(&KStroke::new(1.5), Affine::IDENTITY, blue, None, &bb);
            for c in [bb.origin(), Point::new(bb.x1, bb.y0), Point::new(bb.x1, bb.y1), Point::new(bb.x0, bb.y1)] { scene.fill(peniko::Fill::NonZero, Affine::IDENTITY, blue, None, &kurbo::Rect::from_center_size(c, (8.0, 8.0))); }
        } }
    }
}

/// Where the picture is: its displayed rect (window px, post-rotation) and target px per source px (the stroke widths' scale).
pub struct Geo { pub rect: [f64; 4], pub px_per_src: f64 }
impl Geo {
    pub fn to_norm(&self, x: f64, y: f64) -> [f64; 2] { [(x - self.rect[0]) / self.rect[2].max(1.0), (y - self.rect[1]) / self.rect[3].max(1.0)] }
    fn to_px(&self, p: [f64; 2]) -> Point { Point::new(self.rect[0] + p[0] * self.rect[2], self.rect[1] + p[1] * self.rect[3]) }
}
fn inside(p: [f64; 2]) -> bool { (0.0..=1.0).contains(&p[0]) && (0.0..=1.0).contains(&p[1]) }

/// One stroke into `scene`, clipped to the picture (QCView scissors to the image rect): round caps and joins, its sRGB colour.
pub fn draw(scene: &mut Scene, s: &Stroke, geo: &Geo) {
    let g = ink::geometry(s, geo.rect, geo.px_per_src);
    let col = peniko::Color::new([s.color[0] as f32, s.color[1] as f32, s.color[2] as f32, s.color[3] as f32]);
    let clip = kurbo::Rect::new(geo.rect[0], geo.rect[1], geo.rect[0] + geo.rect[2], geo.rect[1] + geo.rect[3]);
    scene.push_clip_layer(peniko::Fill::NonZero, Affine::IDENTITY, &clip);
    let st = KStroke::new(g.width).with_caps(Cap::Round).with_join(Join::Round);
    for p in &g.paths { scene.stroke(&st, Affine::IDENTITY, col, None, p); }
    if let Some(h) = &g.head { scene.fill(peniko::Fill::NonZero, Affine::IDENTITY, col, None, h); }
    scene.pop_layer();
}
