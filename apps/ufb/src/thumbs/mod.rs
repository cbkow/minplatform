//! Thumbnails and the icons of single files (canon row 456; spike S18). Nothing of a file's CONTENTS is read in the
//! application: every picture is made in a helper copy of this executable (`helper.rs`) and comes back over a pipe.
//!
//! Who draws (row 456, point 1): our own decoders FIRST for the production formats the family reads - a frame looks
//! the same here as in the family's other applications - and the system's own thumbnail for everything else, and for
//! a file our decoder refused. A file with no picture keeps its type's icon, and is not asked about again while it is
//! unchanged ("cannot preview" is remembered by path, size and date).
//!
//! What is in view is done first and ONLY that is asked for: each frame the views say what they show ([`Thumbs::want`]);
//! a request that still waits when its entry has left the view is called off ([`Thumbs::end_frame`]).
//!
//! This slice keeps the pictures in memory only (a cap, the least recently shown dropped first). The cache on disk
//! (row 456, point 4), video, sequences and Radiance HDR through our own decoders are later slices.
pub mod helper;
pub mod pool;
pub mod proto;
#[cfg(target_os = "macos")] #[path = "sys_mac.rs"] pub mod sys;
#[cfg(windows)] #[path = "sys_win.rs"] pub mod sys;
#[cfg(not(any(target_os = "macos", windows)))]
pub mod sys {
    use super::helper::Img; use std::time::Duration;
    pub fn init() {}
    pub fn thumbnail(_path: &str, _px: u32, _soft: Duration) -> Result<Img, (u8, i64)> { Err((super::proto::FAILED, 0)) }
    pub fn icon(_path: &str, _px: u32, _soft: Duration) -> Result<Img, (u8, i64)> { Err((super::proto::FAILED, 0)) }
}

use pool::{Job, Pool};
use proto::*;
use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

/// What is asked of a file.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Kind {
    /// Its picture: our decoder where we have one, else (or when ours refuses) the system's thumbnail.
    Picture,
    /// The icon of this very file (an application's own, a shortcut's): its type's icon would say too little.
    Icon,
}

/// What an entry is asked for, if anything. The grid shows pictures; the list only the icons that belong to one file
/// (thumbnails in the list's rows: not ruled). An empty file has no picture.
pub fn plan(ext: &str, name: &str, dir: bool, size: u64, grid: bool) -> Option<Kind> {
    if dir { return (cfg!(target_os = "macos") && name.len() > 4 && name.get(name.len() - 4..).is_some_and(|e| e.eq_ignore_ascii_case(".app"))).then_some(Kind::Icon) }
    if cfg!(windows) && matches!(ext, "exe" | "lnk" | "url" | "scr") { return Some(Kind::Icon) }
    (grid && size > 0).then_some(Kind::Picture)
}
/// The pixel size a picture is asked at: one of two, so that the grid's size slider does not ask anew at every step.
pub fn picture_px(tile_px: f64) -> u32 { if tile_px <= 256.5 { 256 } else { 512 } }

/// The memory the pictures may take; over it, those shown longest ago go first.
const CAP: usize = 256 << 20;
/// How long a helper without work is kept (S18: fresh 2 MiB, after work 20 - 25; a new one costs milliseconds).
const IDLE: Duration = Duration::from_secs(30);
/// The bounds (ms): how long the helper waits for the system, and when the helper itself is ended.
const DECODE_HARD: u32 = 20_000; const SYS_SOFT: u32 = 8_000; const SYS_HARD: u32 = 12_000;

enum Slot { Waiting { id: u64, seen: u64 }, Ready { img: peniko::ImageBrush, bytes: usize, used: u64 } }
struct Ask { key: u64, path: String, px: u32, op: u8 }

pub struct Thumbs {
    /// our decoders: several helpers; the system's thumbnails and icons: apart, so that a plug-in of the system's
    /// that hangs never holds a decoder
    decode: Pool, system: Pool,
    slots: HashMap<u64, Slot>, asked: HashMap<u64, Ask>, none: HashSet<u64>,
    frame: u64, bytes: usize, next: u64,
    /// counted for the scripted looks: pictures made, entries that have none
    pub made: u64, pub failed: u64,
}

/// One number for (file, its size and date, the size asked, the kind): a changed file is another entry.
pub fn key(path: &Path, size: u64, mtime: i64, px: u32, kind: Kind) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    path.hash(&mut h); size.hash(&mut h); mtime.hash(&mut h); px.hash(&mut h); kind.hash(&mut h); h.finish()
}
/// How many helpers decode: half the machine's cores, at least 2, at most 8 (S18).
pub fn decoders(cores: usize) -> usize { (cores / 2).clamp(2, 8) }

impl Thumbs {
    /// No helper is started here: the first request starts one. `wake` is called when a picture has come.
    pub fn new(wake: Arc<dyn Fn() + Send + Sync>) -> Thumbs {
        let exe = std::env::current_exe().unwrap_or_default();
        let cores = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4);
        let sys_n = std::env::var("UFB_SYS_HELPERS").ok().and_then(|v| v.parse().ok()).unwrap_or(SYS_HELPERS);
        Thumbs { decode: Pool::new(exe.clone(), vec![], decoders(cores), IDLE, wake.clone()), system: Pool::new(exe, vec![], sys_n, IDLE, wake),
            slots: HashMap::new(), asked: HashMap::new(), none: HashSet::new(), frame: 1, bytes: 0, next: 1, made: 0, failed: 0 }
    }
    fn submit(&mut self, key: u64, path: String, px: u32, op: u8) -> u64 {
        let id = self.next; self.next += 1;
        let (soft, hard) = if op == OP_DECODE { (0, DECODE_HARD) } else { (SYS_SOFT, SYS_HARD) };
        let job = Job { req: Request { id, op, size: px as u16, soft_ms: soft, path: path.clone() }, hard_ms: hard };
        if op == OP_DECODE { self.decode.submit(job) } else { self.system.submit(job) }
        self.asked.insert(id, Ask { key, path, px, op }); id
    }
    /// An entry in view says what it wants; it is asked for when it is neither here, nor on its way, nor known to
    /// have no picture. Returns the number to look it up by.
    pub fn want(&mut self, path: &Path, ext: &str, size: u64, mtime: i64, px: u32, kind: Kind) -> u64 {
        let key = key(path, size, mtime, px, kind); let frame = self.frame;
        match self.slots.get_mut(&key) {
            Some(Slot::Waiting { seen, .. }) => *seen = frame,
            Some(Slot::Ready { used, .. }) => *used = frame,
            None => if !self.none.contains(&key) {
                let op = match kind { Kind::Icon => OP_ICON, Kind::Picture => if helper::ours(ext) { OP_DECODE } else { OP_THUMB } };
                let id = self.submit(key, path.to_string_lossy().into_owned(), px, op); self.slots.insert(key, Slot::Waiting { id, seen: frame });
            },
        }
        key
    }
    pub fn peek(&self, key: u64) -> Option<&peniko::ImageBrush> { match self.slots.get(&key)? { Slot::Ready { img, .. } => Some(img), _ => None } }
    /// Before the views of a frame say what they want.
    pub fn begin_frame(&mut self) { self.frame += 1; }
    /// After them: what still waits for an entry no view showed this frame is called off. (A request already with a
    /// helper is left to finish: its picture is kept, the entry may come back.)
    pub fn end_frame(&mut self) {
        let frame = self.frame;
        let stale: Vec<u64> = self.slots.values().filter_map(|s| match s { Slot::Waiting { id, seen } if *seen < frame => Some(*id), _ => None }).collect();
        if stale.is_empty() { return }
        let mut freed = self.decode.cancel(&stale); freed.extend(self.system.cancel(&stale));
        for id in freed { if let Some(a) = self.asked.remove(&id) { self.slots.remove(&a.key); } }
    }
    /// Take in what the helpers answered. True when something came.
    pub fn poll(&mut self) -> bool {
        let mut done = vec![];
        while let Ok(d) = self.decode.rx.try_recv() { done.push(d); }
        while let Ok(d) = self.system.rx.try_recv() { done.push(d); }
        let any = !done.is_empty();
        for d in done {
            let Some(a) = self.asked.remove(&d.id) else { continue };
            let seen = match self.slots.get(&a.key) { Some(Slot::Waiting { seen, .. }) => *seen, _ => self.frame };
            let (w, h) = (d.w as u32, d.h as u32);
            if d.status == OK && w > 0 && h > 0 && d.payload.len() == (w * h * 4) as usize {
                let bytes = d.payload.len(); self.bytes += bytes; self.made += 1;
                let img = peniko::ImageBrush::new(peniko::ImageData { data: peniko::Blob::new(Arc::new(d.payload)), format: peniko::ImageFormat::Rgba8, alpha_type: peniko::ImageAlphaType::AlphaPremultiplied, width: w, height: h });
                self.slots.insert(a.key, Slot::Ready { img, bytes, used: seen });
            } else if a.op == OP_DECODE && d.status != NO_HELPER {
                // our decoder refused the file, or the file ended its helper: the system may still have a picture
                let id = self.submit(a.key, a.path, a.px, OP_THUMB); self.slots.insert(a.key, Slot::Waiting { id, seen });
            } else { self.slots.remove(&a.key); self.none.insert(a.key); self.failed += 1; }
        }
        if self.bytes > CAP { self.evict(); }
        any
    }
    /// Over the cap: the pictures shown longest ago go, down to three quarters of it; what this frame shows stays.
    fn evict(&mut self) {
        let mut old: Vec<(u64, u64, usize)> = self.slots.iter().filter_map(|(k, s)| match s { Slot::Ready { bytes, used, .. } if *used < self.frame => Some((*used, *k, *bytes)), _ => None }).collect();
        old.sort_unstable();
        for (_, k, bytes) in old { if self.bytes <= CAP / 4 * 3 { break } self.slots.remove(&k); self.bytes -= bytes; }
    }
    /// Refresh: files that had no picture are asked about again.
    pub fn retry(&mut self) { self.none.clear(); }
    /// Requests not answered yet.
    pub fn pending(&self) -> usize { self.asked.len() }
}
/// Helpers for the system's thumbnails and icons.
const SYS_HELPERS: usize = 2;

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn what_an_entry_is_asked_for() {
        assert_eq!(plan("exr", "a.exr", false, 10, true), Some(Kind::Picture)); assert_eq!(plan("exr", "a.exr", false, 10, false), None, "the list shows the type's icon");
        assert_eq!(plan("exr", "a.exr", false, 0, true), None, "an empty file has no picture");
        assert_eq!(plan("/", "renders", true, 0, true), None);
        #[cfg(target_os = "macos")] { assert_eq!(plan("/", "Blender.app", true, 0, false), Some(Kind::Icon)); assert_eq!(plan("/", ".app", true, 0, true), None); }
        #[cfg(windows)] assert_eq!(plan("exe", "setup.exe", false, 10, false), Some(Kind::Icon));
        assert_eq!(picture_px(96.0), 256); assert_eq!(picture_px(256.0), 256); assert_eq!(picture_px(300.0), 512);
        assert_eq!(decoders(1), 2); assert_eq!(decoders(8), 4); assert_eq!(decoders(18), 8); assert_eq!(decoders(64), 8);
    }
    #[test]
    fn a_changed_file_is_another_entry() {
        let p = Path::new("/jobs/a.exr"); let k = key(p, 10, 100, 256, Kind::Picture);
        assert_eq!(k, key(p, 10, 100, 256, Kind::Picture));
        for other in [key(p, 11, 100, 256, Kind::Picture), key(p, 10, 101, 256, Kind::Picture), key(p, 10, 100, 512, Kind::Picture), key(p, 10, 100, 256, Kind::Icon), key(Path::new("/jobs/b.exr"), 10, 100, 256, Kind::Picture)] { assert_ne!(k, other); }
    }
}
