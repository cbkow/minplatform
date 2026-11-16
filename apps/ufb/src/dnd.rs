//! Drag and drop of files, the application's side (canon rows 449, 455 c, 472; the layer is `min-dnd`, from spike
//! S16). A drag over a window is answered HERE, at once, from what the window last drew: the places a drop can go
//! ([`Zone`]: a folder's row or tile, a pane, a sidebar row, a segment of the path) are written by the window every
//! frame, and the layer's callback - which the system calls outside the application's own loop - only reads them,
//! says which operation a drop would be, and leaves the drop itself as a note for the loop ([`Shared::drops`]).
//!
//! What a drop does (row 455 c: the system's own keys decide; 1.4.2 asked every time): on one volume a MOVE, to
//! another volume a COPY - unless the keys leave only one of them (macOS Option: copy, Command: move; Windows Ctrl:
//! copy, Shift: move). A drop into the folder the items are in, or into one of the items, is refused. The file
//! operation itself is the engine's (`ops`): the same question about a taken name, the same undo, the same belt.
use kurbo::Rect;
use min_dnd::{DragEvent, Notice, Phase, COPY, MOVE};
use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;

/// A place a drop can go: where it is in the window (physical px) and the folder a drop there goes INTO.
#[derive(Clone, Debug, PartialEq)]
pub struct Zone { pub rect: Rect, pub folder: PathBuf }
/// A drop to carry out: these into that folder.
#[derive(Clone, Debug, PartialEq)]
pub struct Dropped { pub paths: Vec<PathBuf>, pub dest: PathBuf, pub mv: bool }

#[derive(Default)]
pub struct Shared {
    /// where a drop can go, as the window last drew itself: the first that holds the pointer counts
    pub zones: Vec<Zone>,
    /// the place under a drag that would take it, to be shown
    pub hover: Option<Rect>,
    /// drops to carry out; a drag of our own has ended (its sources may be gone: the panes read again)
    pub drops: Vec<Dropped>, pub ended: bool,
    /// where promised files of the last drop go once they have arrived
    promised_into: Option<PathBuf>,
    /// the volume of a folder asked about before (the system is asked once a folder)
    volumes: HashMap<PathBuf, Option<u64>>,
}
pub type Handle = Rc<RefCell<Shared>>;

/// Where promised files (a mail attachment, a browser's picture) are received before they go to their folder: a
/// folder of our own beside the thumbnails' cache. The engine may take files out of it.
pub fn staging() -> PathBuf { crate::thumbs::cache_dir().unwrap_or_else(std::env::temp_dir).join("incoming") }

/// The volume a path is on, as a number (Unix: the device; Windows: its drive or share, hashed).
fn volume(p: &Path) -> Option<u64> {
    #[cfg(unix)] { use std::os::unix::fs::MetadataExt; std::fs::metadata(p).ok().map(|m| m.dev()) }
    #[cfg(not(unix))] { use std::hash::{Hash, Hasher}; let s = p.to_string_lossy().replace('/', "\\").to_lowercase(); let s = s.strip_prefix("\\\\?\\").unwrap_or(&s).to_string();
        let root: String = if let Some(rest) = s.strip_prefix("\\\\") { rest.split('\\').take(2).collect::<Vec<_>>().join("\\") } else { s.chars().take(2).collect() };
        let mut h = std::collections::hash_map::DefaultHasher::new(); root.hash(&mut h); Some(h.finish()) }
}

/// The operation a drop of `sources` into `dest` would be: [`MOVE`], [`COPY`] or 0 (refused). `allowed`: what the
/// source offers under the keys held. `same_volume`: source and destination are on one volume. `may_take`: the
/// sources may be taken away (the belt); `may_put`: things may be put into `dest`.
pub fn choose(allowed: u32, sources: &[PathBuf], dest: &Path, same_volume: bool, may_put: bool, may_take: bool) -> u32 {
    if !may_put || sources.is_empty() { return 0 }
    // into one of the items themselves (or below it): never
    if sources.iter().any(|s| dest.starts_with(s)) { return 0 }
    let want = if same_volume { MOVE } else { COPY };
    let mut op = if allowed & want != 0 { want } else if allowed & COPY != 0 { COPY } else if allowed & MOVE != 0 { MOVE } else { 0 };
    if op == MOVE && !may_take { op = if allowed & COPY != 0 { COPY } else { 0 }; }
    // a move into the folder the items are already in is nothing
    if op == MOVE && sources.iter().all(|s| s.parent() == Some(dest)) { return 0 }
    op
}

/// What the layer tells of a drag over the window, answered from what the window last drew.
pub fn on_notice(shared: &Handle, n: Notice) -> u32 {
    match n {
        Notice::Drag(DragEvent { phase: Phase::Leave, .. }) => { shared.borrow_mut().hover = None; 0 }
        Notice::Drag(e) => {
            let mut s = shared.borrow_mut(); let p = kurbo::Point::new(e.x, e.y);
            let Some(zone) = s.zones.iter().find(|z| z.rect.contains(p)).cloned() else { s.hover = None; return 0 };
            let may_put = crate::ops::allowed_in(&zone.folder);
            let op = if let Some(first) = e.payload.paths.first() {
                let dv = *s.volumes.entry(zone.folder.clone()).or_insert_with(|| volume(&zone.folder)); let key = first.parent().unwrap_or(first).to_path_buf(); let sv = *s.volumes.entry(key.clone()).or_insert_with(|| volume(&key));
                let may_take = e.payload.paths.iter().all(|src| src.parent().is_some_and(crate::ops::allowed_in));
                choose(e.allowed, &e.payload.paths, &zone.folder, dv.is_some() && dv == sv, may_put, may_take)
            } else if !e.payload.promised.is_empty() && may_put && e.allowed & (COPY | MOVE) != 0 { COPY } else { 0 };
            s.hover = (op != 0 && e.phase != Phase::Drop).then_some(zone.rect);
            if e.phase == Phase::Drop && op != 0 {
                if !e.payload.paths.is_empty() { s.drops.push(Dropped { paths: e.payload.paths.clone(), dest: zone.folder.clone(), mv: op == MOVE }); }
                if !e.payload.promised.is_empty() { s.promised_into = Some(zone.folder); }
                if s.volumes.len() > 256 { s.volumes.clear(); }
            }
            op
        }
        // a promised file has arrived in the staging folder: it is moved to where it was dropped
        Notice::Promised { result, .. } => { let mut s = shared.borrow_mut(); if let (Ok(path), Some(dest)) = (result, s.promised_into.clone()) { s.drops.push(Dropped { paths: vec![path], dest, mv: true }); } 0 }
        Notice::Ended { .. } => { let mut s = shared.borrow_mut(); s.ended = true; s.hover = None; 0 }
    }
}

/// The picture under the pointer while files are dragged: a small dark card with one mark a file (at most ten).
/// (A snapshot of the rows themselves is a later nicety.)
pub fn picture(n: usize, s: f64) -> min_dnd::DragImage {
    let (w, h) = ((180.0 * s) as u32, (34.0 * s) as u32); let mut rgba = vec![0u8; (w * h * 4) as usize]; let u = s.round().max(1.0) as u32;
    for y in 0..h { for x in 0..w {
        let edge = x < u || y < u || x >= w - u || y >= h - u;
        let mark = { let k = (x.saturating_sub(10 * u)) / (14 * u); x >= 10 * u && (k as usize) < n.min(10) && (x - 10 * u) % (14 * u) < 10 * u && y >= 12 * u && y < 22 * u };
        let c: [u8; 4] = if edge { [70, 140, 255, 255] } else if mark { [240, 240, 240, 255] } else { [40, 42, 48, 215] };
        rgba[((y * w + x) * 4) as usize..][..4].copy_from_slice(&c);
    } }
    min_dnd::DragImage { w, h, rgba, hot: (8.0 * s, 8.0 * s), scale: s }
}

#[cfg(test)]
mod tests {
    use super::*;
    use min_dnd::LINK;
    fn p(s: &str) -> PathBuf { PathBuf::from(s) }
    #[test]
    fn the_keys_and_the_volume_decide_what_a_drop_is() {
        let srcs = [p("/jobs/a/plate.exr"), p("/jobs/a/shots")]; let all = COPY | MOVE | LINK;
        // nothing held: a move on one volume, a copy to another
        assert_eq!(choose(all, &srcs, Path::new("/jobs/b"), true, true, true), MOVE); assert_eq!(choose(all, &srcs, Path::new("/other/b"), false, true, true), COPY);
        // the keys leave one operation: that one
        assert_eq!(choose(COPY, &srcs, Path::new("/jobs/b"), true, true, true), COPY); assert_eq!(choose(MOVE, &srcs, Path::new("/other/b"), false, true, true), MOVE);
        // a link alone is not something a drop here does
        assert_eq!(choose(LINK, &srcs, Path::new("/jobs/b"), true, true, true), 0);
        // into the folder they are in: a move is nothing; a copy (the copy key) makes duplicates
        assert_eq!(choose(all, &srcs, Path::new("/jobs/a"), true, true, true), 0); assert_eq!(choose(COPY, &srcs, Path::new("/jobs/a"), true, true, true), COPY);
        // into one of the items, or below it: never
        assert_eq!(choose(all, &srcs, Path::new("/jobs/a/shots"), true, true, true), 0); assert_eq!(choose(COPY, &srcs, Path::new("/jobs/a/shots/010"), true, true, true), 0);
        // the belt: nothing is put where it may not be; what may not be taken away is copied, if a copy is on offer
        assert_eq!(choose(all, &srcs, Path::new("/jobs/b"), true, false, true), 0); assert_eq!(choose(all, &srcs, Path::new("/jobs/b"), true, true, false), COPY); assert_eq!(choose(MOVE, &srcs, Path::new("/jobs/b"), true, true, false), 0);
        assert_eq!(choose(all, &[], Path::new("/jobs/b"), true, true, true), 0);
    }
    #[test]
    fn the_picture_is_a_card_of_the_scale_asked() { let i = picture(3, 2.0); assert_eq!((i.w, i.h), (360, 68)); assert_eq!(i.rgba.len(), 360 * 68 * 4); assert_eq!(&i.rgba[..4], [70, 140, 255, 255]); }
}
