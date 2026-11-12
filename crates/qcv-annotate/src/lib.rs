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
struct Drag { id: String, last: [f64; 2], moved: bool, before: Note, /// a corner handle's scale: (corner 0 tl · 1 tr · 2 bl · 3 br, the points' bounds and the points at the press)
    scale: Option<(usize, [f64; 4], Vec<[f64; 2]>)> }

/// A committed edit the app follows up: the note's key, and whether its clean thumbnail must be captured (a new note).
pub struct Committed { pub key: String, pub new_note: bool }

/// The note a drawing / Add note targets — QCView `currentNoteKey` + `noteIsCurrent`. A file's: the playhead's SMPTE key,
/// matched by FRAME. A stream's (plan B1/B2): the note on the Review frame, or the stamp minted at the freeze, matched by KEY
/// (every stream note has frame 0).
#[derive(Clone, Debug, PartialEq)]
pub struct Cur { pub key: String, pub secs: f64, pub frame: i64, pub stream: Option<StreamTag> }
/// A stream note's fields (QCView `setNoteStreamFields`): ISO capture time with the UTC offset, the host's timecode (Premiere), the item's name.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct StreamTag { pub captured_at: String, pub host_timecode: String, pub name: String }
impl Cur {
    pub fn file(frame: i32, fps: f64) -> Cur { let (key, secs) = qcv_notes::note_key(frame, fps); Cur { key, secs, frame: frame as i64, stream: None } }
    pub fn matches(&self, n: &Note) -> bool { if self.stream.is_some() { n.timecode == self.key } else { n.frame == self.frame } }
    fn new_note(&self) -> Note {
        let t = self.stream.clone().unwrap_or_default();
        Note { timecode: self.key.clone(), timestamp_seconds: self.secs, frame: self.frame, image: format!("images/{}", io::image_filename(&self.key)), captured_at: t.captured_at, host_timecode: t.host_timecode, stream: t.name, ..Default::default() }
    }
}

pub struct Annotations {
    pub active: bool, pub tool: Tool, pub color: [f32; 4], pub width: f32,
    pub media: Option<PathBuf>, /// a live item's notes (plan B1): its folder under the project's `.qcview/streams` + the header
    pub stream: Option<(PathBuf, serde_json::Map<String, serde_json::Value>)>,
    pub notes: Vec<Note>, pub selected: Option<String>,
    live: Option<Live>, drag: Option<Drag>, undo: Vec<Step>, redo: Vec<Step>, t0: std::time::Instant,
    /// minNotes' `VideoNotesModel` rules (canon §4.5): keys deleted here since the last save (a merge never brings them
    /// back), the sidecar's signature after our own write (its echo is no reload), the last poll
    tombstones: std::collections::HashSet<String>, disk_sig: Option<(std::time::SystemTime, u64)>, last_poll: std::time::Instant,
}

impl Annotations {
    pub fn new() -> Annotations { Annotations { active: false, tool: Tool::Freehand, color: [1.0, 0.0, 0.0, 1.0], width: 6.0, media: None, stream: None, notes: vec![], selected: None, live: None, drag: None, undo: vec![], redo: vec![], t0: std::time::Instant::now(), tombstones: Default::default(), disk_sig: None, last_poll: std::time::Instant::now() } }
    /// A media file loaded (or none): its notes from the sidecar; the history starts over (per clip).
    pub fn load(&mut self, media: Option<&Path>) {
        self.stream = None;
        self.media = media.map(Path::to_path_buf); self.live = None; self.drag = None; self.selected = None; self.undo.clear(); self.redo.clear();
        self.notes = match media { Some(m) => io::load(m).unwrap_or_else(|e| { log::warn!("notes: {e}"); vec![] }), None => vec![] };
        self.tombstones.clear(); self.disk_sig = self.sidecar_sig();
    }
    /// A live item's notes (QCView `loadNotesForStream`): the folder + header; the history starts over.
    pub fn load_stream(&mut self, dir: PathBuf, header: serde_json::Map<String, serde_json::Value>) {
        self.media = None; self.live = None; self.drag = None; self.selected = None; self.undo.clear(); self.redo.clear();
        self.notes = io::load_at(&dir).map(|(_, n)| n).unwrap_or_else(|e| { log::warn!("stream notes: {e}"); vec![] });
        self.stream = Some((dir, header));
        self.tombstones.clear(); self.disk_sig = self.sidecar_sig();
    }
    /// Where the notes live (the media's sidecar folder, or the stream's folder) — None = no notes target.
    pub fn dir(&self) -> Option<PathBuf> { self.stream.as_ref().map(|(d, _)| d.clone()).or_else(|| self.media.as_ref().map(|m| io::notes_dir(m))) }
    pub fn has_target(&self) -> bool { self.media.is_some() || self.stream.is_some() }
    fn load_disk(&self) -> Result<Vec<Note>, String> { match (&self.stream, &self.media) { (Some((d, _)), _) => io::load_at(d).map(|(_, n)| n), (None, Some(m)) => io::load(m), _ => Ok(vec![]) } }
    /// Merge-on-save (minNotes): notes another app wrote that we haven't seen join ours — for a key both hold, ours wins —
    /// unless we deleted that key since the last save. Then the deletes are durable and the write's signature is ours.
    pub fn save(&mut self) {
        if !self.has_target() { return }
        if let Ok(disk) = self.load_disk() {
            for d in disk { if !self.notes.iter().any(|n| n.timecode == d.timecode) && !self.tombstones.contains(&d.timecode) { self.notes.push(d); } }
            self.notes.sort_by(|a, b| a.timecode.cmp(&b.timecode));
        }
        let r = match (&self.stream, &self.media) { (Some((d, h)), _) => io::save_at(d, h, &self.notes), (None, Some(m)) => io::save(m, &self.notes), _ => Ok(()) };
        if let Err(e) = r { log::warn!("notes save: {e}"); return }
        self.tombstones.clear(); self.disk_sig = self.sidecar_sig();
    }
    fn sidecar_sig(&self) -> Option<(std::time::SystemTime, u64)> {
        let md = std::fs::metadata(io::notes_json(&self.dir()?)).ok()?;
        Some((md.modified().ok()?, md.len()))
    }
    /// minNotes' watcher reload: another app (minNotes, a second QCView) rewrote the sidecar → its notes, less our tombstones.
    /// A stat twice a second, not an OS watcher: review media lives on SMB / NFS mounts where those don't fire (canon §19).
    /// Never mid-gesture. True when the notes changed.
    pub fn poll_disk(&mut self, now: std::time::Instant) -> bool {
        if !self.has_target() || self.drawing() || now.duration_since(self.last_poll) < std::time::Duration::from_millis(500) { return false }
        self.last_poll = now;
        let sig = self.sidecar_sig();
        if sig == self.disk_sig { return false }
        self.disk_sig = sig;
        let Ok(mut loaded) = self.load_disk() else { return false };
        loaded.retain(|n| !self.tombstones.contains(&n.timecode));
        if loaded == self.notes { return false }
        log::info!("notes: {} reloaded from disk (changed by another app)", loaded.len());
        self.notes = loaded;
        if let Some(id) = &self.selected && !self.notes.iter().any(|n| strokes::parse(n.annotation.as_ref()).iter().any(|s| &s.id == id)) { self.selected = None; }
        true
    }
    pub fn note_at(&self, cur: &Cur) -> Option<&Note> { self.notes.iter().find(|n| cur.matches(n)) }
    pub fn strokes_at(&self, cur: &Cur) -> Vec<Stroke> { self.note_at(cur).map(|n| strokes::parse(n.annotation.as_ref())).unwrap_or_default() }
    pub fn drawing(&self) -> bool { self.live.is_some() || self.drag.is_some() }
    fn t(&self) -> f64 { self.t0.elapsed().as_secs_f64() }

    // ---- pointer (normalized picture coordinates; outside 0..1 allowed) ----
    pub fn press(&mut self, p: [f64; 2], cur: &Cur, geo: &Geo) {
        match self.tool {
            Tool::Freehand => self.live = Some(Live::Ink(ink::Smoother::begin(p, self.t()), inside(p))),
            Tool::Eraser => {}
            Tool::Select => {
                // QCView `onSelectPointer`: a corner handle of the selection wins over re-hit-testing (a neighbour can't steal the grab)
                if let (Some(id), Some(n)) = (self.selected.clone(), self.note_at(cur).cloned()) && let Some(s) = strokes::parse(n.annotation.as_ref()).into_iter().find(|s| s.id == id) {
                    if let Some(c) = self.sel_box(&s, geo).and_then(|bb| corner_at(bb, geo.to_px(p))) {
                        self.drag = Some(Drag { id, last: p, moved: false, before: n, scale: Some((c, point_bounds(&s), s.points.clone())) });
                        return
                    }
                }
                self.selected = self.hit(p, cur, geo);
                if let (Some(id), Some(n)) = (self.selected.clone(), self.note_at(cur).cloned()) { self.drag = Some(Drag { id, last: p, moved: false, before: n, scale: None }); }
            }
            t => if let Some(tool) = t.shape() { self.live = Some(Live::Shape { tool, a: p, b: p }); },
        }
    }
    /// Returns a committed edit (the eraser commits as it passes over strokes).
    pub fn motion(&mut self, p: [f64; 2], cur: &Cur, geo: &Geo, pressed: bool) -> Option<Committed> {
        let t = self.t();
        match &mut self.live {
            Some(Live::Ink(s, touched)) => { s.push(p, t); *touched |= inside(p); return None }
            Some(Live::Shape { b, .. }) => { *b = p; return None }
            None => {}
        }
        if let Some(d) = &mut self.drag {
            let delta = [p[0] - d.last[0], p[1] - d.last[1]]; d.last = p; d.moved = true;
            let id = d.id.clone();
            if let Some((corner, b, orig)) = d.scale.clone() {
                if let Some(n) = self.notes.iter_mut().find(|n| cur.matches(n)) {
                    let mut ss = strokes::parse(n.annotation.as_ref());
                    if let Some(s) = ss.iter_mut().find(|s| s.id == id) && let Some(pts) = scale_points(s.tool, &orig, b, corner, p) { s.points = pts; }
                    n.annotation = Some(strokes::to_data(&ss));
                }
                return None
            }
            if let Some(n) = self.notes.iter_mut().find(|n| cur.matches(n)) {
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
        if pressed && self.tool == Tool::Eraser { return self.erase(p, cur, geo) }
        None
    }
    pub fn release(&mut self, p: [f64; 2], cur: &Cur, geo: &Geo) -> Option<Committed> {
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
        Some(self.add_stroke(stroke, cur))
    }
    /// The stroke joins the note at the playhead (created if absent: QCView's `onStrokeFinalized`).
    fn add_stroke(&mut self, s: Stroke, cur: &Cur) -> Committed {
        let key = cur.key.clone();
        let before = self.notes.iter().find(|n| n.timecode == key).cloned();
        let new_note = before.is_none();
        let mut n = before.clone().unwrap_or_else(|| cur.new_note());
        let mut ss = strokes::parse(n.annotation.as_ref()); ss.push(s); n.annotation = Some(strokes::to_data(&ss));
        self.put(n.clone());
        self.push_step(Step { key: key.clone(), before, after: Some(n) }); self.save();
        Committed { key, new_note }
    }
    fn put(&mut self, n: Note) { self.tombstones.remove(&n.timecode); match self.notes.iter_mut().find(|x| x.timecode == n.timecode) { Some(x) => *x = n, None => { self.notes.push(n); self.notes.sort_by(|a, b| a.timecode.cmp(&b.timecode)); } } }
    fn remove_note(&mut self, key: &str) { self.notes.retain(|n| n.timecode != key); self.tombstones.insert(key.to_string()); }
    fn push_step(&mut self, s: Step) { self.undo.push(s); self.redo.clear(); }
    fn erase(&mut self, p: [f64; 2], cur: &Cur, geo: &Geo) -> Option<Committed> {
        let id = self.hit(p, cur, geo)?;
        let before = self.note_at(cur)?.clone();
        let mut n = before.clone(); let mut ss = strokes::parse(n.annotation.as_ref()); ss.retain(|s| s.id != id); n.annotation = Some(strokes::to_data(&ss));
        let key = n.timecode.clone(); self.put(n.clone());
        self.push_step(Step { key: key.clone(), before: Some(before), after: Some(n) }); self.save();
        Some(Committed { key, new_note: false })
    }
    /// Delete / Backspace with Select: the selected stroke goes.
    pub fn delete_selected(&mut self, cur: &Cur) -> Option<Committed> {
        let id = self.selected.take()?;
        let before = self.note_at(cur)?.clone();
        let mut n = before.clone(); let mut ss = strokes::parse(n.annotation.as_ref()); ss.retain(|s| s.id != id); n.annotation = Some(strokes::to_data(&ss));
        let key = n.timecode.clone(); self.put(n.clone());
        self.push_step(Step { key: key.clone(), before: Some(before), after: Some(n) }); self.save();
        Some(Committed { key, new_note: false })
    }
    /// QCView's "Add note here": a note at the playhead without a drawing (its clean frame captured by the app).
    pub fn add_note(&mut self, cur: &Cur) -> Option<Committed> {
        let key = cur.key.clone();
        if self.notes.iter().any(|n| n.timecode == key) { return None }
        let n = cur.new_note();
        self.put(n.clone()); self.push_step(Step { key: key.clone(), before: None, after: Some(n) }); self.save();
        Some(Committed { key, new_note: true })
    }
    /// A note's text / addressed flag (saved; QCView keeps these out of the drawing history).
    pub fn set_text(&mut self, key: &str, text: &str) { if let Some(n) = self.notes.iter_mut().find(|n| n.timecode == key) { if n.text != text { n.text = text.to_string(); self.save(); } } }
    /// The clean frame's records (QCView updateNoteViewerTag / updateNoteOcioChain).
    pub fn set_tags(&mut self, key: &str, viewer: &str, ocio: &str) { if let Some(n) = self.notes.iter_mut().find(|n| n.timecode == key) { if n.viewer != viewer || n.ocio != ocio { n.viewer = viewer.to_string(); n.ocio = ocio.to_string(); self.save(); } } }
    pub fn set_addressed(&mut self, key: &str, on: bool) { if let Some(n) = self.notes.iter_mut().find(|n| n.timecode == key) { if n.addressed != on { n.addressed = on; self.save(); } } }
    /// QCView's "Delete note": the note and both its images (a user delete is the one thing that removes the PNGs).
    pub fn delete_note(&mut self, key: &str) {
        self.notes.retain(|n| n.timecode != key); self.tombstones.insert(key.to_string()); self.undo.retain(|s| s.key != key); self.redo.retain(|s| s.key != key); self.save();
        if let Some(d) = self.dir() { let dir = io::images_dir(&d); let _ = std::fs::remove_file(dir.join(io::image_filename(key))); let _ = std::fs::remove_file(dir.join(io::annotated_filename(key))); }
    }
    /// QCView: Escape cancels the stroke in progress, else clears the selection. True when it did something.
    pub fn escape(&mut self) -> bool { if self.live.take().is_some() { return true } self.selected.take().is_some() }
    pub fn undo(&mut self) -> Option<Committed> { let s = self.undo.pop()?; let key = s.key.clone(); match &s.before { Some(n) => self.put(n.clone()), None => self.remove_note(&key) } self.redo.push(s); self.save(); self.selected = None; Some(Committed { key, new_note: false }) }
    pub fn redo(&mut self) -> Option<Committed> { let s = self.redo.pop()?; let key = s.key.clone(); match &s.after { Some(n) => self.put(n.clone()), None => self.remove_note(&key) } self.undo.push(s); self.save(); self.selected = None; Some(Committed { key, new_note: false }) }

    /// The selection's outline on screen: the drawn stroke's bounds + its half-width + 3 px (the handles sit on its corners).
    fn sel_box(&self, s: &Stroke, geo: &Geo) -> Option<kurbo::Rect> {
        let g = ink::geometry(s, geo.rect, geo.px_per_src);
        let bb = g.paths.iter().chain(g.head.iter()).map(|p| p.bounding_box()).reduce(|a, b| a.union(b))?;
        Some(bb.inflate(g.width / 2.0 + 3.0, g.width / 2.0 + 3.0))
    }
    /// The stroke under `p` — the drawn line within its half-width + 4 px, topmost first.
    fn hit(&self, p: [f64; 2], cur: &Cur, geo: &Geo) -> Option<String> {
        let pt = geo.to_px(p);
        for s in self.strokes_at(cur).iter().rev() {
            let g = ink::geometry(s, geo.rect, geo.px_per_src);
            let reach = g.width / 2.0 + 4.0;
            let near = g.paths.iter().chain(g.head.iter()).any(|path| path.segments().any(|seg| seg.nearest(pt, 0.1).distance_sq <= reach * reach))
                || g.head.as_ref().is_some_and(|h| h.contains(pt));
            if near { return Some(s.id.clone()) }
        }
        None
    }

    /// Is there ink at `cur` — a stored stroke or one being drawn?
    pub fn has_ink(&self, cur: &Cur) -> bool { self.live.is_some() || !self.strokes_at(cur).is_empty() }
    /// A signature of the ink at `cur` (the outputs' change stamp: a new / moved / erased stroke, the live one growing).
    pub fn ink_sig(&self, cur: &Cur) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        cur.key.hash(&mut h);
        if let Some(a) = self.note_at(cur).and_then(|n| n.annotation.as_ref()) { a.to_string().hash(&mut h); }
        match &self.live {
            Some(Live::Ink(sm, _)) => { 1u8.hash(&mut h); let p = sm.points(); p.len().hash(&mut h); if let Some(l) = p.last() { format!("{l:?}").hash(&mut h); } }
            Some(Live::Shape { a, b, .. }) => { 2u8.hash(&mut h); for v in a.iter().chain(b) { v.to_bits().hash(&mut h); } }
            None => {}
        }
        self.color.map(f32::to_bits).hash(&mut h); self.width.to_bits().hash(&mut h);
        h.finish()
    }
    /// Draw the frame's strokes, the live one and the selection outline.
    pub fn paint(&self, scene: &mut Scene, cur: &Cur, geo: &Geo) {
        self.paint_ink(scene, cur, geo);
        // the selection: QCView's #0189f1 outline + corner handles, here on screen only
        if let Some(id) = &self.selected { if let Some(s) = self.strokes_at(cur).into_iter().find(|s| &s.id == id) {
            let Some(bb) = self.sel_box(&s, geo) else { return };
            let blue = peniko::Color::from_rgba8(0x01, 0x89, 0xf1, 0xff);
            scene.stroke(&KStroke::new(1.5), Affine::IDENTITY, blue, None, &bb);
            for c in [bb.origin(), Point::new(bb.x1, bb.y0), Point::new(bb.x1, bb.y1), Point::new(bb.x0, bb.y1)] { scene.fill(peniko::Fill::NonZero, Affine::IDENTITY, blue, None, &kurbo::Rect::from_center_size(c, (8.0, 8.0))); }
        } }
    }
    /// The ink alone — the frame's strokes and the live one — what an output carries (row 238).
    pub fn paint_ink(&self, scene: &mut Scene, cur: &Cur, geo: &Geo) {
        for s in self.strokes_at(cur) { draw(scene, &s, geo); }
        match &self.live {
            Some(Live::Ink(sm, _)) => { let s = Stroke { id: String::new(), tool: STool::Freehand, color: Stroke::qcolor(self.color), width: self.width as f64, filled: false, is_modeled: true, points: sm.points().to_vec() }; draw(scene, &s, geo); }
            Some(Live::Shape { tool, a, b }) => {
                let pts = match tool { STool::Rectangle => vec![*a, [b[0], a[1]], *b, [a[0], b[1]]], STool::Oval => vec![[(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0], [(b[0] - a[0]).abs() / 2.0, (b[1] - a[1]).abs() / 2.0]], _ => vec![*a, *b] };
                draw(scene, &Stroke { id: String::new(), tool: *tool, color: Stroke::qcolor(self.color), width: self.width as f64, filled: false, is_modeled: false, points: pts }, geo);
            }
            None => {}
        }
    }
}

/// Where the picture is: its displayed rect (window px, post-rotation) and target px per source px (the stroke widths' scale).
pub struct Geo { pub rect: [f64; 4], pub px_per_src: f64 }
impl Geo {
    pub fn to_norm(&self, x: f64, y: f64) -> [f64; 2] { [(x - self.rect[0]) / self.rect[2].max(1.0), (y - self.rect[1]) / self.rect[3].max(1.0)] }
    fn to_px(&self, p: [f64; 2]) -> Point { Point::new(self.rect[0] + p[0] * self.rect[2], self.rect[1] + p[1] * self.rect[3]) }
}
/// QCView `annotationHandleAt`: the corner within 9 px of the pointer (0 tl · 1 tr · 2 bl · 3 br), screen space.
fn corner_at(bb: kurbo::Rect, pt: Point) -> Option<usize> {
    [Point::new(bb.x0, bb.y0), Point::new(bb.x1, bb.y0), Point::new(bb.x0, bb.y1), Point::new(bb.x1, bb.y1)].iter().position(|c| c.distance(pt) <= 9.0)
}
/// A stroke's point bounds in picture coordinates (an oval = centre ± radii), clipped to the picture (QCView's visible bounds).
fn point_bounds(s: &Stroke) -> [f64; 4] {
    let (mut x0, mut y0, mut x1, mut y1) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
    if s.tool == STool::Oval && !s.points.is_empty() { let c = s.points[0]; let r = s.points.get(1).copied().unwrap_or([0.0, 0.0]); (x0, y0, x1, y1) = (c[0] - r[0], c[1] - r[1], c[0] + r[0], c[1] + r[1]); }
    else { for q in &s.points { x0 = x0.min(q[0]); y0 = y0.min(q[1]); x1 = x1.max(q[0]); y1 = y1.max(q[1]); } }
    [x0.clamp(0.0, 1.0), y0.clamp(0.0, 1.0), x1.clamp(0.0, 1.0), y1.clamp(0.0, 1.0)]
}
/// QCView `annotationResizeTo`: uniform, aspect-locked scale about the opposite corner, by the drag's dominant axis; at
/// least 0.03 of the picture; clamped so the scaled bounds stay inside it. None = nothing to scale (a flat bounds).
fn scale_points(tool: STool, orig: &[[f64; 2]], b: [f64; 4], corner: usize, p: [f64; 2]) -> Option<Vec<[f64; 2]>> {
    let [l, t, r, bt] = b;
    let (grab, pivot) = match corner { 0 => ([l, t], [r, bt]), 1 => ([r, t], [l, bt]), 2 => ([l, bt], [r, t]), _ => ([r, bt], [l, t]) };
    let (dxs, dys) = (grab[0] - pivot[0], grab[1] - pivot[1]);
    let (ow, oh) = (dxs.abs(), dys.abs());
    if ow <= 0.0 || oh <= 0.0 { return None }
    let mut k = ((p[0] - pivot[0]).abs() / ow).max((p[1] - pivot[1]).abs() / oh);
    k = k.max((0.03 / ow).max(0.03 / oh));
    let kx = if dxs > 0.0 { (1.0 - pivot[0]) / dxs } else if dxs < 0.0 { -pivot[0] / dxs } else { 1e9 };
    let ky = if dys > 0.0 { (1.0 - pivot[1]) / dys } else if dys < 0.0 { -pivot[1] / dys } else { 1e9 };
    k = k.min(kx.min(ky));
    if k <= 0.0 { return None }
    let at = |q: [f64; 2]| [pivot[0] + (q[0] - pivot[0]) * k, pivot[1] + (q[1] - pivot[1]) * k];
    Some(if tool == STool::Oval && orig.len() >= 2 { let mut v = orig.to_vec(); v[0] = at(orig[0]); v[1] = [orig[1][0] * k, orig[1][1] * k]; v } else { orig.iter().map(|q| at(*q)).collect() })
}
#[cfg(test)]
mod sidecar_tests {
    use super::*;
    /// minNotes' rules: another app's new note is picked up by the poll and survives our save; ours wins a shared key;
    /// a note we deleted is not merged back; our own save is no reload.
    #[test] fn reload_merge_and_tombstones() {
        let dir = std::env::temp_dir().join(format!("qcv-sidecar-{}", std::process::id())); let _ = std::fs::remove_dir_all(&dir); std::fs::create_dir_all(&dir).unwrap();
        let media = dir.join("clip.mov"); std::fs::write(&media, b"").unwrap();
        let note = |tc: &str, text: &str, f: i64| Note { timecode: tc.into(), text: text.into(), frame: f, ..Default::default() };
        io::save(&media, &[note("00:00:00:01", "a", 1), note("00:00:00:02", "b", 2)]).unwrap();
        let mut a = Annotations::new(); a.load(Some(&media));
        let later = |a: &mut Annotations| { a.last_poll -= std::time::Duration::from_secs(1); };
        // our own save: no reload
        a.set_text("00:00:00:01", "a2"); later(&mut a); assert!(!a.poll_disk(std::time::Instant::now()));
        // another app adds a note and edits ours (bumping mtime / length)
        std::thread::sleep(std::time::Duration::from_millis(20));
        io::save(&media, &[note("00:00:00:01", "theirs", 1), note("00:00:00:02", "b", 2), note("00:00:00:03", "c", 3)]).unwrap();
        later(&mut a); assert!(a.poll_disk(std::time::Instant::now()), "their write reloads");
        assert_eq!(a.notes.len(), 3);
        // we delete 2 while they (unseen) still hold it: the merge does not bring it back
        a.delete_note("00:00:00:02");
        let disk = io::load(&media).unwrap(); assert!(disk.iter().all(|n| n.timecode != "00:00:00:02"), "the delete is durable");
        // they add 4 we haven't polled; our next save merges it in, our text wins for 1
        std::thread::sleep(std::time::Duration::from_millis(20));
        let mut theirs = io::load(&media).unwrap(); theirs.push(note("00:00:00:04", "d", 4)); io::save(&media, &theirs).unwrap();
        a.set_text("00:00:00:01", "ours");
        let disk = io::load(&media).unwrap();
        assert!(disk.iter().any(|n| n.timecode == "00:00:00:04") && disk.iter().find(|n| n.timecode == "00:00:00:01").unwrap().text == "ours", "{disk:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[cfg(test)]
mod golden_tests {
    use super::*;
    /// A fixed sequence of edits, and the sidecar it leaves: written to `QCV_GOLDEN_OUT` when that is set, so that two
    /// builds of this module can be compared byte for byte (stroke ids apart: they are random). The move of this module
    /// into a crate of its own (2026-11-12) was checked with it.
    #[test] fn a_fixed_sequence_of_edits_leaves_the_same_sidecar() {
        let dir = std::env::temp_dir().join(format!("qcv-golden-{}", std::process::id())); let _ = std::fs::remove_dir_all(&dir); std::fs::create_dir_all(&dir).unwrap();
        let media = dir.join("clip.mov"); std::fs::write(&media, b"").unwrap();
        let geo = Geo { rect: [0.0, 0.0, 1920.0, 1080.0], px_per_src: 1.0 };
        let mut a = Annotations::new(); a.load(Some(&media)); a.active = true;
        let stroke = |a: &mut Annotations, tool: Tool, f: i32, pts: &[[f64; 2]]| { let cur = Cur::file(f, 24.0); std::thread::sleep(std::time::Duration::from_millis(3)); a.tool = tool; a.press(pts[0], &cur, &geo); for p in &pts[1..] { a.motion(*p, &cur, &geo, true); } a.release(*pts.last().unwrap(), &cur, &geo); };
        // the pointer is in the picture's own coordinates (0..1)
        a.add_note(&Cur::file(12, 24.0));
        stroke(&mut a, Tool::Rectangle, 12, &[[0.10, 0.14], [0.33, 0.44]]);
        a.color = [0.2, 0.6, 1.0, 1.0]; a.width = 9.0;
        stroke(&mut a, Tool::Arrow, 12, &[[0.50, 0.50], [0.78, 0.28]]);
        stroke(&mut a, Tool::Oval, 49, &[[0.21, 0.37], [0.36, 0.56]]);
        stroke(&mut a, Tool::Line, 49, &[[0.05, 0.83], [0.94, 0.88]]);
        stroke(&mut a, Tool::Freehand, 96, &[[0.16, 0.28], [0.17, 0.29], [0.19, 0.32], [0.22, 0.35], [0.26, 0.36], [0.31, 0.34]]);
        // three steps back (the freehand and its note, the line, the oval and its note), two forward (the oval, the line)
        a.undo(); a.undo(); a.undo(); a.redo(); a.redo();
        a.set_text("00:00:00:12", "first note\nsecond line"); a.set_addressed("00:00:02:01", true); a.set_text("00:00:02:01", "an oval \u{2014} \"quoted\" <tag> & more");
        a.add_note(&Cur::file(200, 24.0)); a.delete_note("00:00:08:08");
        // the eraser passes over the rectangle's left edge on frame 12
        { let cur = Cur::file(12, 24.0); a.tool = Tool::Eraser; a.press([0.10, 0.30], &cur, &geo); a.motion([0.10, 0.30], &cur, &geo, true); a.release([0.10, 0.30], &cur, &geo); }
        // (strokes are made a few milliseconds apart: an id is minted from the clock, as QCView's are)
        assert_eq!(a.notes.iter().map(|n| (n.timecode.as_str(), strokes::parse(n.annotation.as_ref()).len())).collect::<Vec<_>>(), [("00:00:00:12", 1), ("00:00:02:01", 2)]);
        a.save();
        let out = std::fs::read(io::notes_dir(&media).join("notes.json")).expect("the sidecar was written");
        assert!(out.len() > 900);
        if let Some(to) = std::env::var_os("QCV_GOLDEN_OUT") { std::fs::write(to, &out).unwrap(); }
        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[cfg(test)]
mod scale_tests {
    use super::*;
    #[test] fn corner_scale_is_qcviews() {
        let sq = vec![[0.4, 0.4], [0.6, 0.4], [0.6, 0.6], [0.4, 0.6]]; let b = [0.4, 0.4, 0.6, 0.6];
        // the bottom-right handle dragged to (0.8, 0.7): the dominant axis wins (×2), the top-left corner stays put
        let out = scale_points(STool::Rectangle, &sq, b, 3, [0.8, 0.7]).unwrap();
        assert!((out[0][0] - 0.4).abs() < 1e-9 && (out[2][0] - 0.8).abs() < 1e-9 && (out[2][1] - 0.8).abs() < 1e-9, "{out:?}");
        // dragged past the picture: clamped so the bounds stay inside
        let out = scale_points(STool::Rectangle, &sq, b, 3, [2.0, 2.0]).unwrap(); assert!((out[2][0] - 1.0).abs() < 1e-9 && out[2][1] <= 1.0 + 1e-9);
        // collapsed onto the pivot: the 0.03 floor
        let out = scale_points(STool::Rectangle, &sq, b, 3, [0.4, 0.4]).unwrap(); assert!((out[2][0] - 0.43).abs() < 1e-9, "{out:?}");
        // an oval: the centre moves about the pivot, the radii scale
        let ov = vec![[0.5, 0.5], [0.1, 0.1]]; let out = scale_points(STool::Oval, &ov, [0.4, 0.4, 0.6, 0.6], 0, [0.2, 0.2]).unwrap();
        assert!((out[1][0] - 0.2).abs() < 1e-9 && (out[0][0] - 0.4).abs() < 1e-9, "{out:?}");
    }
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
