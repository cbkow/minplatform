//! The lightbox (canon rows 452, 473): one file, large, over the whole window. Step 4a: stills - the file decoded
//! WHOLE in a helper copy of the executable (`OP_FULL`: a damaged picture costs a helper, never the window), shown by
//! the presenter's own pipeline as QCView shows a still (half floats are linear light, 8 and 16 bits display-referred
//! sRGB; alpha over a checker), with zoom and pan. The window draws the lightbox's few controls over it.
//!
//! Only the newest request counts: stepping through a folder calls off what still waits, and an answer for a file
//! that is no longer the one shown is dropped.
use crate::thumbs::helper::Full;
use crate::thumbs::pool::{Job, Pool, HELPER_ARG};
use crate::thumbs::proto::{Request, MAX_FULL, OK, OP_DECODE, OP_FULL, OP_LAYERS};
use kurbo::Rect;
use min_present::{Chroma, ColorMeta, Frame, Matrix, Pixels, PlanarLayout, Plane, Primaries, Range, Transfer};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// The kinds the lightbox shows in this step (stills). Everything else opens in its default application on a double
/// click, and says "no preview yet" when stepped onto.
pub fn shows(ext: &str) -> bool {
    matches!(ext, "png" | "jpg" | "jpeg" | "tif" | "tiff" | "exr" | "dpx" | "tga" | "bmp" | "psd" | "psb" | "hdr" | "heic" | "heif" | "webp" | "gif" | "avif" | "cr2" | "cr3" | "nef" | "arw" | "dng" | "raf" | "jp2" | "ico" | "icns")
}

/// What is known of the picture shown.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Info { pub w: u32, pub h: u32, /// the file's own size, and by how much each side was divided to show it (1: whole)
    pub orig: (u32, u32), pub reduced: u32, pub alpha: bool, /// 8, 16, or 32 for half floats (said as "float")
    pub depth: u8 }
#[derive(Clone, Debug, PartialEq)]
pub enum State { Loading, Ready(Info), Failed(String), /// a kind this step does not show
    NoPreview, /// an EXR of several layers: its contact sheet (`View::layers`); one is chosen there
    Layers }

pub struct View {
    pub path: PathBuf, pub name: String, pub size: u64, pub kind: String, /// the pane it was opened from (its cursor follows the lightbox)
    pub pane: usize, pub state: State, pub since: Instant,
    /// an EXR's layers, each with its small picture when that has come; the layer shown large (none: the sheet, or a file of one layer)
    pub layers: Vec<(String, Option<peniko::ImageBrush>)>, pub layer: Option<String>,
    /// screen pixels a picture pixel (none: fitted to the window, never enlarged), and the picture's shift from the centre
    zoom: Option<f64>, pan: (f64, f64),
    /// a drag that pans: where it began, the shift then
    drag: Option<((f64, f64), (f64, f64))>,
}

pub struct Lightbox { pool: Pool, next: u64, waiting: Option<u64>, pub view: Option<View>, frame: Option<(Frame, bool)>,
    /// the question "which layers" that is out; the small pictures of the sheet that are out (request -> layer)
    asked_layers: Option<u64>, tiles: std::collections::HashMap<u64, usize> }

/// The decoder's planes as the presenter's frame (QCView's still pipeline: RGB at full range; 8 / 16-bit stills are
/// display-referred sRGB, half floats linear light).
fn frame_of(f: Full) -> Frame {
    let (bps, code_bits, float) = match f.sample { 0 => (1, 8, false), 1 => (2, 16, false), _ => (2, 16, true) };
    let meta = ColorMeta { matrix: Matrix::Identity, range: Range::Full, transfer: if float { Transfer::Linear } else { Transfer::Srgb }, primaries: Primaries::Bt709, bit_depth: if f.sample == 0 { 8 } else { 16 }, chroma: Chroma::Rgb, range_forced: false, rotation: 0, sar: (1, 1), mastering: None };
    let (w, h, pitch, alpha) = (f.w, f.h, f.pitch, f.alpha);
    Frame { width: w, height: h, pixels: Pixels::Planar { planes: f.planes.into_iter().map(|p| Plane::owned(p, w, h, pitch)).collect(), layout: PlanarLayout::Rgb { bytes_per_sample: bps, code_bits, float, alpha, legal_expand: false } }, meta }
}

impl Lightbox {
    /// One helper of its own, started at the first picture and ended after a while without one.
    pub fn new(wake: Arc<dyn Fn() + Send + Sync>) -> Lightbox {
        let _ = HELPER_ARG;
        Lightbox { pool: Pool::new(std::env::current_exe().unwrap_or_default(), vec![], 1, Duration::from_secs(30), wake), next: 1, waiting: None, view: None, frame: None, asked_layers: None, tiles: Default::default() }
    }
    pub fn is_open(&self) -> bool { self.view.is_some() }
    pub fn loading(&self) -> bool { self.view.as_ref().is_some_and(|v| v.state == State::Loading) || !self.tiles.is_empty() }
    /// Show this file. `ext`: its extension in lower case.
    pub fn open(&mut self, path: PathBuf, name: String, size: u64, kind: String, ext: &str, pane: usize) {
        self.call_off(); self.frame = None;
        let state = if shows(ext) { State::Loading } else { State::NoPreview };
        let wanted = state == State::Loading; let p = path.to_string_lossy().into_owned();
        self.view = Some(View { path, name, size, kind, pane, state, since: Instant::now(), layers: vec![], layer: None, zoom: None, pan: (0.0, 0.0), drag: None });
        // an EXR is first asked which layers it has: several make a contact sheet, one is shown at once
        if wanted { if ext == "exr" { let id = self.id(); self.asked_layers = Some(id); self.pool.submit(Job::new(Request::new(id, OP_LAYERS, 0, 0, p), 20_000)); } else { self.ask_full(p); } }
    }
    fn id(&mut self) -> u64 { let id = self.next; self.next += 1; id }
    fn ask_full(&mut self, path: String) { let id = self.id(); self.waiting = Some(id); self.pool.submit(Job { req: Request::new(id, OP_FULL, 0, 0, path), hard_ms: 60_000, max: MAX_FULL }); }
    /// What still waits is called off (the file shown changes, or the lightbox closes).
    fn call_off(&mut self) { let mut ids: Vec<u64> = self.tiles.drain().map(|(id, _)| id).collect(); ids.extend(self.waiting.take()); ids.extend(self.asked_layers.take()); if !ids.is_empty() { self.pool.cancel(&ids); } }
    pub fn close(&mut self) { self.call_off(); self.view = None; self.frame = None; }
    /// A layer of the contact sheet is chosen: it is shown large.
    pub fn pick_layer(&mut self, i: usize) {
        let Some(v) = self.view.as_mut() else { return }; let Some((name, _)) = v.layers.get(i) else { return };
        let path = format!("{}\u{1}{name}", v.path.to_string_lossy()); v.layer = Some(name.clone()); v.state = State::Loading; v.since = Instant::now(); v.fit();
        if let Some(id) = self.waiting.take() { self.pool.cancel(&[id]); } self.frame = None; self.ask_full(path);
    }
    /// Back from one layer to the sheet. False when there is no sheet to go back to.
    pub fn back_to_layers(&mut self) -> bool {
        let Some(v) = self.view.as_mut() else { return false }; if v.layers.len() < 2 || v.layer.is_none() { return false }
        if let Some(id) = self.waiting.take() { self.pool.cancel(&[id]); } self.frame = None; v.layer = None; v.state = State::Layers; true
    }
    /// Take in what the helper answered. True when something came.
    pub fn poll(&mut self) -> bool {
        let mut any = false;
        while let Ok(d) = self.pool.rx.try_recv() {
            any = true;
            // a small picture of the contact sheet
            if let Some(i) = self.tiles.remove(&d.id) { let (w, h) = (d.w as u32, d.h as u32);
                if let (Some(v), true) = (self.view.as_mut(), d.status == OK && w > 0 && h > 0 && d.payload.len() == (w * h * 4) as usize) { if let Some(t) = v.layers.get_mut(i) {
                    t.1 = Some(peniko::ImageBrush::new(peniko::ImageData { data: peniko::Blob::new(Arc::new(d.payload)), format: peniko::ImageFormat::Rgba8, alpha_type: peniko::ImageAlphaType::AlphaPremultiplied, width: w, height: h })); } }
                continue }
            // which layers the file has
            if self.asked_layers == Some(d.id) { self.asked_layers = None;
                let names: Vec<String> = if d.status == OK { String::from_utf8_lossy(&d.payload).lines().filter(|l| !l.is_empty()).map(str::to_string).collect() } else { vec![] };
                let Some(v) = self.view.as_mut() else { continue }; let p = v.path.to_string_lossy().into_owned();
                if names.len() > 1 { v.state = State::Layers; v.layers = names.iter().map(|n| (n.clone(), None)).collect();
                    for (i, n) in names.iter().enumerate() { let id = self.next; self.next += 1; self.tiles.insert(id, i); self.pool.submit(Job::new(Request::new(id, OP_DECODE, 512, 0, format!("{p}\u{1}{n}")), 30_000)); } }
                else { self.ask_full(p); }
                continue }
            if self.waiting != Some(d.id) { continue } self.waiting = None;
            let Some(v) = self.view.as_mut() else { continue };
            match (d.status == OK).then(|| Full::unpack(d.w as u32, d.h as u32, 0, &d.payload)).flatten() {
                Some(f) => { let info = Info { w: f.w, h: f.h, orig: f.orig, reduced: (f.orig.0.max(f.orig.1)).div_ceil(f.w.max(f.h)).max(1), alpha: f.alpha, depth: match f.sample { 0 => 8, 1 => 16, _ => 32 } };
                    v.state = State::Ready(info); self.frame = Some((frame_of(f), info.alpha)); }
                None => v.state = State::Failed(match d.status { crate::thumbs::proto::CRASHED => "This file could not be read: it stopped its decoder.".into(), crate::thumbs::proto::HUNG => "This file took too long to read.".into(), _ => "This file could not be shown.".into() }),
            }
        }
        any
    }
    /// The picture that has come, to hand to the presenter (once), and whether it has transparency.
    pub fn take_frame(&mut self) -> Option<(Frame, bool)> { self.frame.take() }
}

impl View {
    pub fn info(&self) -> Option<Info> { match self.state { State::Ready(i) => Some(i), _ => None } }
    /// Screen pixels a picture pixel, in `area`.
    pub fn scale(&self, area: Rect) -> f64 { let Some(i) = self.info() else { return 1.0 }; self.zoom.unwrap_or_else(|| (area.width() / i.w as f64).min(area.height() / i.h as f64).min(1.0)) }
    pub fn fitted(&self) -> bool { self.zoom.is_none() }
    /// Where the picture lies (it may reach past `area`): what the presenter is told.
    pub fn place(&self, area: Rect) -> Option<Rect> {
        let i = self.info()?; let z = self.scale(area); let (w, h) = (i.w as f64 * z, i.h as f64 * z); let c = area.center();
        Some(Rect::new(c.x - w / 2.0 + self.pan.0, c.y - h / 2.0 + self.pan.1, c.x + w / 2.0 + self.pan.0, c.y + h / 2.0 + self.pan.1))
    }
    /// The picture cannot be pushed out of sight: where it is larger than the window it may be moved until its edge
    /// meets the window's; where it is smaller it stays in the middle.
    fn clamp(&mut self, area: Rect) {
        let Some(i) = self.info() else { return }; let z = self.scale(area);
        let lim = |size: f64, room: f64| ((size - room) / 2.0).max(0.0);
        let (lx, ly) = (lim(i.w as f64 * z, area.width()), lim(i.h as f64 * z, area.height()));
        self.pan = (self.pan.0.clamp(-lx, lx), self.pan.1.clamp(-ly, ly));
    }
    /// Zoom by `factor` about the point `at` (the picture under the pointer stays under it). Between a tenth of the
    /// fitted size and 32 screen pixels a picture pixel.
    pub fn zoom_by(&mut self, factor: f64, at: (f64, f64), area: Rect) {
        let Some(i) = self.info() else { return }; let old = self.scale(area);
        let fit = (area.width() / i.w as f64).min(area.height() / i.h as f64).min(1.0);
        let new = (old * factor).clamp(fit * 0.1, 32.0); if (new - old).abs() < 1e-9 { return }
        let c = area.center(); let (dx, dy) = (at.0 - c.x - self.pan.0, at.1 - c.y - self.pan.1);
        self.pan = (self.pan.0 + dx * (1.0 - new / old), self.pan.1 + dy * (1.0 - new / old));
        self.zoom = Some(new); self.clamp(area);
    }
    /// Fitted to the window again.
    pub fn fit(&mut self) { self.zoom = None; self.pan = (0.0, 0.0); }
    /// One screen pixel a picture pixel, about `at`.
    pub fn actual(&mut self, at: (f64, f64), area: Rect) { let z = self.scale(area); if z > 0.0 { self.zoom_by(1.0 / z, at, area); self.zoom = Some(1.0); self.clamp(area); } }
    /// A double click: actual size where it is fitted, fitted where it is not.
    pub fn toggle(&mut self, at: (f64, f64), area: Rect) { if self.fitted() { self.actual(at, area) } else { self.fit() } }
    pub fn drag_begin(&mut self, at: (f64, f64)) { self.drag = Some((at, self.pan)); }
    pub fn drag_to(&mut self, at: (f64, f64), area: Rect) -> bool { let Some((from, pan)) = self.drag else { return false }; self.pan = (pan.0 + at.0 - from.0, pan.1 + at.1 - from.1); self.clamp(area); true }
    pub fn drag_end(&mut self) { self.drag = None; }
    pub fn dragging(&self) -> bool { self.drag.is_some() }
    /// The strip's words: the picture's size, its depth, how it is shown.
    pub fn facts(&self, area: Rect) -> String {
        let mut v = vec![self.kind.clone(), crate::format::size_text(self.size, crate::format::DECIMAL)];
        if let Some(l) = &self.layer { v.insert(0, format!("layer \u{201c}{l}\u{201d}")); } else if self.state == State::Layers { v.push(format!("{} layers", self.layers.len())); }
        if let Some(i) = self.info() {
            v.insert(0, format!("{} \u{d7} {}", crate::pane::group(i.orig.0 as usize), crate::pane::group(i.orig.1 as usize)));
            v.push(match i.depth { 8 => "8 bit".into(), 16 => "16 bit".into(), _ => "float".to_string() } + if i.alpha { ", alpha" } else { "" });
            v.push(format!("{:.0} %", self.scale(area) * 100.0 / i.reduced as f64));
            if i.reduced > 1 { v.push(format!("shown at 1/{} of its size", i.reduced)); }
        }
        v.join("  \u{b7}  ")
    }
}
/// The extension of a path, lower case ("" without one).
pub fn ext_of(path: &Path) -> String { path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default() }

#[cfg(test)]
mod tests {
    use super::*;
    fn view(w: u32, h: u32) -> View { View { path: PathBuf::from("/a/plate.exr"), name: "plate.exr".into(), size: 12_400_000, kind: "OpenEXR image".into(), pane: 0, state: State::Ready(Info { w, h, orig: (w, h), reduced: 1, alpha: false, depth: 32 }), since: Instant::now(), layers: vec![], layer: None, zoom: None, pan: (0.0, 0.0), drag: None } }
    #[test]
    fn a_picture_is_fitted_never_enlarged_and_zooms_about_the_pointer() {
        let area = Rect::new(0.0, 0.0, 1000.0, 600.0);
        // larger than the window: fitted; smaller: its own size, in the middle
        let v = view(4000, 2000); assert_eq!(v.scale(area), 0.25); assert_eq!(v.place(area), Some(Rect::new(0.0, 50.0, 1000.0, 550.0)));
        let small = view(200, 100); assert_eq!(small.scale(area), 1.0); assert_eq!(small.place(area), Some(Rect::new(400.0, 250.0, 600.0, 350.0)));
        // zoom in about a point: the picture's pixel under it stays under it
        let mut v = view(4000, 2000); let at = (250.0, 175.0); let before = { let r = v.place(area).unwrap(); ((at.0 - r.x0) / v.scale(area), (at.1 - r.y0) / v.scale(area)) };
        v.zoom_by(2.0, at, area); assert_eq!(v.scale(area), 0.5); assert!(!v.fitted());
        let after = { let r = v.place(area).unwrap(); ((at.0 - r.x0) / v.scale(area), (at.1 - r.y0) / v.scale(area)) }; assert!((before.0 - after.0).abs() < 1e-6 && (before.1 - after.1).abs() < 1e-6, "{before:?} {after:?}");
        // it cannot be dragged out of sight: its edge stops at the window's
        v.drag_begin((500.0, 300.0)); v.drag_to((5000.0, 5000.0), area); let r = v.place(area).unwrap(); assert!((r.x0 - 0.0).abs() < 1e-6 && (r.y0 - 0.0).abs() < 1e-6, "{r:?}"); v.drag_end();
        // actual size, then fitted again by the same double click
        v.toggle((500.0, 300.0), area); assert!(v.fitted()); v.toggle((500.0, 300.0), area); assert_eq!(v.scale(area), 1.0); v.fit(); assert_eq!(v.place(area), Some(Rect::new(0.0, 50.0, 1000.0, 550.0)));
        // the limits
        for _ in 0..40 { v.zoom_by(2.0, at, area); } assert_eq!(v.scale(area), 32.0); for _ in 0..80 { v.zoom_by(0.5, at, area); } assert!((v.scale(area) - 0.025).abs() < 1e-9);
    }
    #[test]
    fn the_strip_says_what_the_picture_is() {
        let area = Rect::new(0.0, 0.0, 1000.0, 600.0); let mut v = view(4000, 2000);
        let mb = crate::format::size_text(12_400_000, crate::format::DECIMAL);
        assert_eq!(v.facts(area), format!("4,000 \u{d7} 2,000  \u{b7}  OpenEXR image  \u{b7}  {mb}  \u{b7}  float  \u{b7}  25 %"));
        v.state = State::Ready(Info { w: 8000, h: 4000, orig: (16000, 8000), reduced: 2, alpha: true, depth: 16 }); assert!(v.facts(area).ends_with("16 bit, alpha  \u{b7}  6 %  \u{b7}  shown at 1/2 of its size"), "{}", v.facts(area));
        v.state = State::NoPreview; assert_eq!(v.facts(area), format!("OpenEXR image  \u{b7}  {mb}"));
        assert!(shows("exr") && shows("psd") && shows("heic") && !shows("mov") && !shows("pdf") && !shows("")); assert_eq!(ext_of(Path::new("/a/Plate.EXR")), "exr");
    }
}
