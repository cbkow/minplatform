//! Collect Media and the package (canon rows 332, 333): the files a collection refers to are copied to one place - beside
//! the collection (Collect Media), or into a folder that holds a copy of the collection with everything it shows (the
//! package) - and the references are pointed at the copies. This file is the work without the window: what is to be
//! copied ([`plan`]), the copying itself off the window's thread ([`Run`]), and the references rewritten
//! ([`rewrite`]). The Qt app's rules are kept where they were sound (`PackageExporter.cpp`): one copy per source
//! however often it is used, the readable file name with `-2`, `-3` on a clash (compared without case), a clip's
//! notes folder travelling with the clip, folders and web addresses left alone. What is new: nothing is rolled back
//! because one file failed, a file already at the destination is not copied again, and every copy is checked.
use crate::media::Places;
use mn_doc::BlockId;
use std::{collections::{HashMap, HashSet, VecDeque}, io::{Read, Write}, path::{Path, PathBuf}, sync::{atomic::{AtomicBool, AtomicU64, AtomicU8, Ordering::Relaxed}, Arc, Mutex}, time::{Duration, Instant, SystemTime}};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum What { Collect, Package }

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Kind { Video, Pdf, Picture, Audio, File }
impl Kind {
    pub const ALL: [Kind; 5] = [Kind::Video, Kind::Pdf, Kind::Picture, Kind::Audio, Kind::File];
    pub fn label(self) -> &'static str { match self { Kind::Video => "Videos", Kind::Pdf => "PDFs", Kind::Picture => "Pictures", Kind::Audio => "Audio", Kind::File => "Files" } }
    pub fn icon(self) -> &'static str { match self { Kind::Video => "film-strip", Kind::Pdf => "file-pdf", Kind::Picture => "image", Kind::Audio => "file-audio", Kind::File => "file" } }
}

/// One file the collection refers to.
#[derive(Clone, Debug)]
pub struct Asset {
    pub kind: Kind, pub name: String,
    /// every way the collection names it (a record's `src`, as JSON text): what is rewritten
    pub srcs: Vec<String>,
    /// where it is on this machine (None: its place cannot be worked out - a volume this machine has no path for)
    pub path: Option<PathBuf>, pub size: u64, pub mtime: Option<SystemTime>,
    /// it is kept with the collection already (a package copies these too; Collect Media has nothing to do with them)
    pub inside: bool,
    /// why it cannot be copied as things stand
    pub problem: Option<String>,
    /// its name at the destination
    pub dest: String,
    /// a clip's notes folder (QCView's sidecar), which travels with it
    pub notes: Option<PathBuf>,
    /// chosen to be copied
    pub on: bool,
    /// the destination has it already (the same name, size and time): not copied again
    pub there: bool,
    /// how many blocks and canvas cards show it
    pub uses: usize,
}

fn usable(src: &serde_json::Value) -> bool { match src { serde_json::Value::Object(_) => true, serde_json::Value::String(s) => !s.is_empty() && !crate::media::remote(s), _ => false } }
fn leaf(src: &serde_json::Value) -> String {
    match src { serde_json::Value::String(s) => s.rsplit(['/', '\\']).next().unwrap_or(s).to_string(), serde_json::Value::Object(o) => o.get("rel").and_then(|r| r.as_array()).and_then(|a| a.last()).and_then(|l| l.as_str()).unwrap_or("").to_string(), _ => String::new() }
}
const AUDIO: [&str; 9] = ["wav", "mp3", "aif", "aiff", "flac", "m4a", "ogg", "aac", "opus"];

/// The files a media record names: its own (`src`) and, for a picture made of another file (a Photoshop file, an SVG),
/// that original too. A folder, a web address and a record that names nothing give none.
pub fn sources(descriptor: &str) -> Vec<(serde_json::Value, Kind, String)> {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(descriptor) else { return vec![] };
    let named = v.get("name").and_then(|n| n.as_str()).filter(|n| !n.is_empty()).map(str::to_string);
    let kind = match v.get("kind").and_then(|k| k.as_str()) {
        None => Kind::Picture, Some("video") => Kind::Video, Some("pdf") => Kind::Pdf, Some("audio") => Kind::Audio,
        Some("file") => { if v.get("dir").and_then(|d| d.as_bool()) == Some(true) { return vec![] }
            let ext = v.get("ext").and_then(|e| e.as_str()).unwrap_or("").to_ascii_lowercase(); if AUDIO.contains(&ext.as_str()) { Kind::Audio } else { Kind::File } }
        Some(_) => return vec![],
    };
    let mut out = vec![]; let original = v.get("preview").and_then(|p| p.get("src")).filter(|s| usable(s));
    if let Some(src) = v.get("src").filter(|s| usable(s)) { out.push((src.clone(), kind, if original.is_some() { leaf(src) } else { named.clone().unwrap_or_else(|| leaf(src)) })); }
    if let Some(src) = original { out.push((src.clone(), Kind::File, named.unwrap_or_else(|| leaf(src)))); }
    out
}

fn near(a: Option<SystemTime>, b: Option<SystemTime>) -> bool { match (a, b) { (Some(a), Some(b)) => a.duration_since(b).or_else(|_| b.duration_since(a)).is_ok_and(|d| d <= Duration::from_secs(2)), _ => false } }
/// `name`, or `name-2`, `name-3`… when it is taken (compared without case, as most disks do).
pub fn unique(name: &str, taken: &mut HashSet<String>) -> String {
    let (stem, ext) = match name.rfind('.') { Some(i) if i > 0 => (&name[..i], &name[i..]), _ => (name, "") };
    let mut n = 1; loop { let c = if n == 1 { name.to_string() } else { format!("{stem}-{n}{ext}") }; if taken.insert(c.to_lowercase()) { return c } n += 1; }
}
/// The names a folder holds, without case.
fn names_in(dir: &Path) -> HashSet<String> { std::fs::read_dir(dir).map(|l| l.flatten().map(|e| e.file_name().to_string_lossy().to_lowercase()).collect()).unwrap_or_default() }

/// What is known of the file is read from the disk and its name at the destination is settled. Run again after the
/// file was located somewhere else.
pub fn settle(a: &mut Asset, dest: &Path, taken: &mut HashSet<String>) {
    a.there = false; a.notes = None; a.on = false;
    let Some(p) = a.path.clone() else { return };
    a.problem = match std::fs::metadata(&p) {
        Ok(m) if m.is_file() => { a.size = m.len(); a.mtime = m.modified().ok(); None }
        Ok(_) => Some("It is not a file".into()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Some("Missing \u{2014} moved, renamed, or on a drive that is not connected".into()),
        Err(e) => Some(format!("It cannot be read ({e})")),
    };
    if a.problem.is_some() { return }
    a.on = true;
    if a.kind == Kind::Video { let d = qcv_notes::io::notes_dir(&p); if d.is_dir() { a.notes = Some(d); } }
    // a file that IS at the destination (named by its whole path): the reference is all there is to change
    if p.parent() == Some(dest) { a.dest = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(); a.there = true; return }
    let want = if a.inside { a.dest.clone() } else { p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| a.name.clone()) };
    let same = std::fs::metadata(dest.join(&want)).is_ok_and(|m| m.is_file() && m.len() == a.size && near(m.modified().ok(), a.mtime));
    if a.inside || same { a.there = same; taken.insert(want.to_lowercase()); a.dest = want; return }
    a.dest = unique(&want, taken);
}

/// The files the records name, one entry per file however often it is used, in the order they are listed: by kind,
/// then by name. Collect Media leaves out what is with the collection already.
pub fn plan(what: What, descriptors: &[String], places: &Places, dest: &Path) -> Vec<Asset> {
    let (mut out, mut by): (Vec<Asset>, HashMap<String, usize>) = (vec![], HashMap::new());
    for d in descriptors { for (src, kind, name) in sources(d) {
        let inside = src.as_str().is_some_and(|s| s.starts_with(".minnotes/"));
        if what == What::Collect && inside { continue }
        let text = src.to_string();
        let (path, problem) = match places.locate(&src) { Ok(p) => (Some(p), None), Err(why) => (None, Some(why)) };
        let key = path.as_ref().map(|p| p.to_string_lossy().into_owned()).unwrap_or_else(|| format!("\0{text}"));
        if let Some(&i) = by.get(&key) { let a = &mut out[i]; a.uses += 1; if !a.srcs.contains(&text) { a.srcs.push(text); } continue }
        by.insert(key, out.len());
        let dest = if inside { src.as_str().map(|s| s[10..].to_string()).unwrap_or_default() } else { String::new() };
        out.push(Asset { kind, name, srcs: vec![text], path, size: 0, mtime: None, inside, problem, dest, notes: None, on: false, there: false, uses: 1 });
    } }
    // (a name in the hidden folder of before row 333, beside the destination, is taken too: a record's `.minnotes/x` is
    // looked for in the visible folder first, so a new file of that name there would stand in front of the old one)
    let mut taken = names_in(dest); if let Some(b) = dest.parent() { taken.extend(names_in(&b.join(crate::media::OLD_SIDE))); } for a in out.iter().filter(|a| a.inside) { taken.insert(a.dest.to_lowercase()); }
    for a in out.iter_mut() { settle(a, dest, &mut taken); }
    out.sort_by(|a, b| (a.kind, a.name.to_lowercase()).cmp(&(b.kind, b.name.to_lowercase())));
    out
}

/// A record with its references pointed at the copies (`map`: a `src` as JSON text -> the new `src`). None: nothing
/// in it changes.
pub fn rewrite(descriptor: &str, map: &HashMap<String, String>) -> Option<String> {
    let mut v: serde_json::Value = serde_json::from_str(descriptor).ok()?; let o = v.as_object_mut()?; let mut hit = false;
    if let Some(n) = o.get("src").and_then(|s| map.get(&s.to_string())) { o.insert("src".into(), serde_json::Value::String(n.clone())); hit = true; }
    if let Some(p) = o.get_mut("preview").and_then(|p| p.as_object_mut()) { if let Some(n) = p.get("src").and_then(|s| map.get(&s.to_string())) { p.insert("src".into(), serde_json::Value::String(n.clone())); hit = true; } }
    hit.then(|| v.to_string())
}
/// The changes [`rewrite`] makes in a note.
pub fn rewritten(doc: &mn_doc::Doc, map: &HashMap<String, String>) -> Vec<(BlockId, mn_doc::Kind)> {
    doc.walk().iter().filter_map(|b| match &b.kind { mn_doc::Kind::Media { descriptor } => rewrite(descriptor, map).map(|d| (b.id, mn_doc::Kind::Media { descriptor: d })), _ => None }).collect()
}

/// Every note and canvas of a collection, folders walked.
pub fn entries(col: &mn_store::Collection) -> Vec<mn_store::Entry> {
    fn walk(c: &mn_store::Collection, parent: Option<BlockId>, out: &mut Vec<mn_store::Entry>) { for e in c.list(parent).unwrap_or_default() { let id = e.id; if e.kind != mn_store::DocKind::Folder { out.push(e); } walk(c, Some(id), out); } }
    let mut out = vec![]; walk(col, None, &mut out); out
}
/// Every media record of a collection: its notes' blocks and its canvases' cards.
pub fn descriptors(col: &mn_store::Collection) -> Vec<String> {
    let mut out = vec![];
    for e in entries(col) { match e.kind {
        mn_store::DocKind::Note => if let Ok(doc) = col.load_note(e.id) { out.extend(doc.walk().iter().filter_map(|b| match &b.kind { mn_doc::Kind::Media { descriptor } => Some(descriptor.clone()), _ => None })); },
        mn_store::DocKind::Canvas => if let Ok(c) = col.load_canvas(e.id) { out.extend(c.elements.into_iter().filter(|x| x.kind == "media").map(|x| x.record)); },
        mn_store::DocKind::Folder => {} } }
    out
}
/// The references of a whole collection are pointed at the copies, but for the notes and canvases in `skip` (those
/// the window holds open, which it changes itself). How many records changed.
pub fn rewrite_collection(col: &mut mn_store::Collection, map: &HashMap<String, String>, skip: &HashSet<BlockId>) -> Result<usize, String> {
    let mut n = 0;
    for e in entries(col) { if skip.contains(&e.id) { continue } match e.kind {
        mn_store::DocKind::Note => { let mut doc = col.load_note(e.id).map_err(|x| x.to_string())?; let ch = rewritten(&doc, map); if ch.is_empty() { continue }
            n += ch.len(); for (id, k) in ch { doc.set_kind(id, k).map_err(|x| format!("{x:?}"))?; } col.save_note(e.id, &doc).map_err(|x| x.to_string())?; }
        mn_store::DocKind::Canvas => { let mut c = col.load_canvas(e.id).map_err(|x| x.to_string())?; let mut hit = 0;
            for x in c.elements.iter_mut().filter(|x| x.kind == "media") { if let Some(r) = rewrite(&x.record, map) { x.record = r; hit += 1; } }
            if hit > 0 { n += hit; col.save_canvas(e.id, &c).map_err(|x| x.to_string())?; } }
        mn_store::DocKind::Folder => {} } }
    Ok(n)
}

/// The room left where `dir` is (or will be), in bytes.
#[cfg(unix)]
pub fn free_space(dir: &Path) -> Option<u64> {
    use std::os::unix::ffi::OsStrExt;
    let mut d = dir; while !d.exists() { d = d.parent()?; }
    let c = std::ffi::CString::new(d.as_os_str().as_bytes()).ok()?;
    unsafe { let mut s: libc::statvfs = std::mem::zeroed(); (libc::statvfs(c.as_ptr(), &mut s) == 0).then(|| s.f_bavail as u64 * s.f_frsize as u64) }
}
#[cfg(not(unix))]
pub fn free_space(_dir: &Path) -> Option<u64> { None }

// ── the copying

pub const QUEUED: u8 = 0; pub const COPYING: u8 = 1; pub const VERIFYING: u8 = 2; pub const DONE: u8 = 3;
/// at the destination already
pub const THERE: u8 = 4; pub const FAILED: u8 = 5;
/// not to be copied (not chosen, or it cannot be)
pub const OFF: u8 = 6;
/// a file from this size on is copied alone, one after another; smaller ones go beside it on a second thread
pub const LARGE: u64 = 64 << 20;

pub struct Job { pub from: PathBuf, pub to: PathBuf, pub size: u64, pub notes: Option<(PathBuf, PathBuf)> }
pub struct Slot { pub state: AtomicU8, pub done: AtomicU64, pub why: Mutex<String> }
enum Fail { Stopped, Why(String, bool) }

/// A copy under way: one slot per asset of the plan, read by the window while two threads fill them.
pub struct Run {
    pub jobs: Vec<Option<Job>>, pub slots: Vec<Slot>, queues: [Mutex<VecDeque<usize>>; 2], live: [AtomicBool; 2],
    /// asked to stop: the file being written is dropped, nothing more is begun
    pub stop: AtomicBool,
    /// the destination ran out of room: nothing more is begun
    pub full: AtomicBool,
    /// every copy is read back and compared
    pub check: bool,
    /// what this run wrote (files, notes folders), for a stop that takes it all back
    made: Mutex<Vec<PathBuf>>,
    /// bytes written by this run
    pub copied: AtomicU64, wake: Box<dyn Fn() + Send + Sync>,
}

fn full(e: &std::io::Error) -> bool { e.kind() == std::io::ErrorKind::StorageFull || e.kind() == std::io::ErrorKind::QuotaExceeded }
fn fail(what: &str, e: std::io::Error) -> Fail { Fail::Why(if full(&e) { "The destination is full".to_string() } else { format!("{what} ({e})") }, full(&e)) }
/// Reads until `buf` is full or the file ends (so a hash over the blocks does not depend on how the reads fell).
fn fill(f: &mut std::fs::File, buf: &mut [u8]) -> std::io::Result<usize> { let mut n = 0; while n < buf.len() { match f.read(&mut buf[n..]) { Ok(0) => break, Ok(k) => n += k, Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {} Err(e) => return Err(e) } } Ok(n) }
/// A quick 64-bit mix of a block into a running value - it tells a copy that reads back differently, nothing more.
fn mix(mut h: u64, b: &[u8]) -> u64 {
    let mut c = b.chunks_exact(8); for w in &mut c { h = (h ^ u64::from_le_bytes(w.try_into().unwrap())).wrapping_mul(0x9e37_79b9_7f4a_7c15).rotate_left(29); }
    for x in c.remainder() { h = (h ^ *x as u64).wrapping_mul(0x0000_0100_0000_01b3); } h
}
fn copy_dir(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for e in std::fs::read_dir(from)? { let e = e?; let (p, t) = (e.path(), to.join(e.file_name())); if e.file_type()?.is_dir() { copy_dir(&p, &t)?; } else { std::fs::copy(&p, &t)?; } }
    Ok(())
}

impl Run {
    /// The chosen assets of a plan become jobs into `dest`; what is there already is marked so.
    pub fn new(assets: &[Asset], dest: &Path, check: bool, wake: impl Fn() + Send + Sync + 'static) -> Arc<Run> {
        let (mut jobs, mut slots, mut big, mut small) = (vec![], vec![], VecDeque::new(), VecDeque::new());
        for (i, a) in assets.iter().enumerate() {
            let go = a.on && a.problem.is_none() && a.path.is_some();
            let to = dest.join(&a.dest);
            let notes = a.notes.as_ref().map(|n| (n.clone(), dest.join(".qcview").join(qcv_notes::io::sanitize_media_name(&a.dest))));
            // a clip that is there already may still lack its notes: they are small, and copied again
            let state = if !go { OFF } else if a.there { if let Some((from, to)) = &notes { if !to.exists() { let _ = copy_dir(from, to); } } THERE } else { if a.size >= LARGE { big.push_back(i) } else { small.push_back(i) } QUEUED };
            jobs.push(go.then(|| Job { from: a.path.clone().unwrap(), to, size: a.size, notes }));
            slots.push(Slot { state: AtomicU8::new(state), done: AtomicU64::new(0), why: Mutex::new(String::new()) });
        }
        Arc::new(Run { jobs, slots, queues: [Mutex::new(big), Mutex::new(small)], live: [AtomicBool::new(false), AtomicBool::new(false)], stop: AtomicBool::new(false), full: AtomicBool::new(false), check, made: Mutex::new(vec![]), copied: AtomicU64::new(0), wake: Box::new(wake) })
    }
    /// A thread per queue that has work and no thread.
    pub fn start(self: &Arc<Self>) {
        for q in 0..2 { if !self.queues[q].lock().unwrap().is_empty() && !self.live[q].swap(true, Relaxed) { let me = self.clone(); let _ = std::thread::Builder::new().name(format!("mn-gather-{q}")).spawn(move || me.work(q)); } }
    }
    fn work(&self, q: usize) {
        loop {
            loop { if self.stop.load(Relaxed) || self.full.load(Relaxed) { break } let Some(i) = self.queues[q].lock().unwrap().pop_front() else { break }; self.one(i); }
            self.live[q].store(false, Relaxed);
            // a retry may have been queued as the loop ended
            if self.stop.load(Relaxed) || self.full.load(Relaxed) || self.queues[q].lock().unwrap().is_empty() || self.live[q].swap(true, Relaxed) { break }
        }
        (self.wake)();
    }
    pub fn busy(&self) -> bool { self.live.iter().any(|l| l.load(Relaxed)) }
    pub fn state(&self, i: usize) -> u8 { self.slots.get(i).map(|s| s.state.load(Relaxed)).unwrap_or(OFF) }
    pub fn count(&self, state: u8) -> usize { self.slots.iter().filter(|s| s.state.load(Relaxed) == state).count() }
    /// A failed file is tried again.
    pub fn retry(self: &Arc<Self>, i: usize) {
        let Some(Some(j)) = self.jobs.get(i) else { return }; if self.state(i) != FAILED { return }
        self.slots[i].state.store(QUEUED, Relaxed); self.slots[i].done.store(0, Relaxed); self.full.store(false, Relaxed); self.stop.store(false, Relaxed);
        self.queues[if j.size >= LARGE { 0 } else { 1 }].lock().unwrap().push_back(i); self.start();
    }
    /// Everything this run wrote is removed (after a stop); the files it copied count as not copied.
    pub fn remove_made(&self) {
        for p in self.made.lock().unwrap().drain(..) { if p.is_dir() { let _ = std::fs::remove_dir_all(&p); } else { let _ = std::fs::remove_file(&p); } }
        for s in &self.slots { if s.state.load(Relaxed) == DONE { s.state.store(QUEUED, Relaxed); s.done.store(0, Relaxed); } }
    }
    pub fn made_any(&self) -> bool { !self.made.lock().unwrap().is_empty() }
    fn one(&self, i: usize) {
        let (Some(job), slot) = (self.jobs[i].as_ref(), &self.slots[i]) else { return };
        slot.done.store(0, Relaxed); slot.state.store(COPYING, Relaxed); (self.wake)();
        match self.copy(job, slot) {
            Ok(()) => slot.state.store(DONE, Relaxed),
            Err(Fail::Stopped) => { slot.done.store(0, Relaxed); slot.state.store(QUEUED, Relaxed); }
            Err(Fail::Why(why, no_room)) => { *slot.why.lock().unwrap() = why; slot.done.store(0, Relaxed); slot.state.store(FAILED, Relaxed); if no_room { self.full.store(true, Relaxed); } }
        }
        (self.wake)();
    }
    /// One file: written under a temporary name, checked, given the original's time, then given its name.
    fn copy(&self, job: &Job, slot: &Slot) -> Result<(), Fail> {
        if let Some(d) = job.to.parent() { std::fs::create_dir_all(d).map_err(|e| fail("The destination folder could not be made", e))?; }
        let name = job.to.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let tmp = job.to.with_file_name(format!(".{name}.{}.part", std::process::id()));
        if let Err(e) = self.write(job, slot, &tmp) { let _ = std::fs::remove_file(&tmp); return Err(e) }
        if let Err(e) = std::fs::rename(&tmp, &job.to) { let _ = std::fs::remove_file(&tmp); return Err(fail("The copy could not be given its name", e)) }
        self.made.lock().unwrap().push(job.to.clone());
        if let Some((from, to)) = &job.notes { self.made.lock().unwrap().push(to.clone()); copy_dir(from, to).map_err(|e| fail("The file was copied, but its notes were not", e))?; }
        Ok(())
    }
    fn write(&self, job: &Job, slot: &Slot, tmp: &Path) -> Result<(), Fail> {
        let mut from = std::fs::File::open(&job.from).map_err(|e| fail("It cannot be read", e))?;
        let mut to = std::fs::File::create(tmp).map_err(|e| fail("The destination cannot be written to", e))?;
        let mut buf = vec![0u8; 4 << 20]; let (mut all, mut h, mut told) = (0u64, 0u64, Instant::now());
        loop {
            if self.stop.load(Relaxed) { return Err(Fail::Stopped) }
            let n = fill(&mut from, &mut buf).map_err(|e| fail("Reading it failed", e))?; if n == 0 { break }
            if self.check { h = mix(h, &buf[..n]); }
            to.write_all(&buf[..n]).map_err(|e| fail("Writing the copy failed", e))?;
            all += n as u64; slot.done.store(all, Relaxed); self.copied.fetch_add(n as u64, Relaxed);
            if told.elapsed() > Duration::from_millis(100) { told = Instant::now(); (self.wake)(); }
            if n < buf.len() { break }
        }
        to.sync_all().map_err(|e| fail("Writing the copy failed", e))?;
        slot.state.store(VERIFYING, Relaxed); (self.wake)();
        let (src, got) = (std::fs::metadata(&job.from).map_err(|e| fail("It cannot be read", e))?, to.metadata().map_err(|e| fail("The copy cannot be read", e))?.len());
        if got != all || src.len() != all { return Err(Fail::Why("The copy is not the size of the file \u{2014} the file may have changed while it was copied".into(), false)) }
        if self.check {
            let mut back = std::fs::File::open(tmp).map_err(|e| fail("The copy cannot be read", e))?; let mut h2 = 0u64;
            loop { if self.stop.load(Relaxed) { return Err(Fail::Stopped) } let n = fill(&mut back, &mut buf).map_err(|e| fail("The copy cannot be read", e))?; if n == 0 { break } h2 = mix(h2, &buf[..n]); if n < buf.len() { break } }
            if h2 != h { return Err(Fail::Why("The copy does not read back the same as the file".into(), false)) }
        }
        // the original's time: what tells a later run that this file is here already
        if let Ok(t) = src.modified() { let _ = to.set_modified(t); }
        Ok(())
    }
}

/// The references to change once the copying is over: every way a copied (or already present) file was named, to its
/// place with the collection.
pub fn map(assets: &[Asset], run: &Run) -> HashMap<String, String> {
    let mut m = HashMap::new();
    for (i, a) in assets.iter().enumerate() { if matches!(run.state(i), DONE | THERE) { let to = format!(".minnotes/{}", a.dest); for s in &a.srcs { if *s != serde_json::Value::String(to.clone()).to_string() { m.insert(s.clone(), to.clone()); } } } }
    m
}

/// A size in words.
pub fn bytes(n: u64) -> String {
    let f = n as f64;
    if n >= 1 << 40 { format!("{:.2} TB", f / (1u64 << 40) as f64) } else if n >= 1 << 30 { format!("{:.1} GB", f / (1u64 << 30) as f64) } else if n >= 1 << 20 { format!("{:.0} MB", f / (1u64 << 20) as f64) } else if n >= 1 << 10 { format!("{:.0} KB", f / 1024.0) } else { format!("{n} bytes") }
}
/// A length of time in words, roughly.
pub fn span(secs: f64) -> String {
    let s = secs.max(0.0) as u64;
    if s < 60 { "less than a minute".into() } else if s < 3600 { format!("about {} min", (s + 30) / 60) } else { format!("about {} h {} min", s / 3600, (s % 3600) / 60) }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn tmp(name: &str) -> PathBuf { let d = std::env::temp_dir().join(format!("mn-gather-{name}-{}", std::process::id())); let _ = std::fs::remove_dir_all(&d); std::fs::create_dir_all(&d).unwrap(); d }
    fn wait(run: &Arc<Run>) { let t = Instant::now(); while run.busy() && t.elapsed() < Duration::from_secs(20) { std::thread::sleep(Duration::from_millis(5)); } assert!(!run.busy()); }
    fn rec(v: serde_json::Value) -> String { v.to_string() }

    #[test]
    fn a_plan_lists_each_file_once_names_clashes_apart_and_marks_what_is_missing() {
        let d = tmp("plan"); let (a, b, dest) = (d.join("a"), d.join("b"), d.join("col/minNotes Media"));
        for x in [&a, &b, &dest] { std::fs::create_dir_all(x).unwrap(); }
        std::fs::write(a.join("shot.png"), b"one").unwrap(); std::fs::write(b.join("Shot.png"), b"other").unwrap(); std::fs::write(a.join("clip.mov"), vec![7u8; 4096]).unwrap();
        std::fs::create_dir_all(a.join(".qcview/clip.mov")).unwrap(); std::fs::write(a.join(".qcview/clip.mov/notes.json"), b"{}").unwrap();
        std::fs::write(dest.join("kept-1234.png"), b"kept").unwrap();
        let p = |x: &Path| x.to_string_lossy().into_owned();
        let recs = vec![
            rec(serde_json::json!({ "src": p(&a.join("shot.png")), "name": "shot.png" })), rec(serde_json::json!({ "src": p(&a.join("shot.png")), "name": "shot.png" })),
            rec(serde_json::json!({ "src": p(&b.join("Shot.png")), "name": "Shot.png" })),
            rec(serde_json::json!({ "kind": "video", "src": p(&a.join("clip.mov")), "name": "clip.mov" })),
            rec(serde_json::json!({ "kind": "pdf", "src": p(&a.join("gone.pdf")), "name": "gone.pdf" })),
            rec(serde_json::json!({ "src": ".minnotes/kept-1234.png", "name": "kept.png" })),
            rec(serde_json::json!({ "kind": "file", "dir": true, "src": p(&a), "name": "a" })), rec(serde_json::json!({ "src": "https://example.com/x.png" })),
            rec(serde_json::json!({ "src": ".minnotes/kept-1234.png", "name": "art.psd", "preview": { "src": p(&a.join("art.psd")), "format": "psd" } })),
        ];
        let places = Places::at(Some(d.join("col")));
        let plan_c = plan(What::Collect, &recs, &places, &dest);
        let names: Vec<(&str, &str, usize, bool)> = plan_c.iter().map(|a| (a.name.as_str(), a.dest.as_str(), a.uses, a.problem.is_some())).collect();
        assert_eq!(names, [("clip.mov", "clip.mov", 1, false), ("gone.pdf", "", 1, true), ("shot.png", "shot.png", 2, false), ("Shot.png", "Shot-2.png", 1, false), ("art.psd", "", 1, true)]);
        assert!(plan_c[0].notes.is_some() && plan_c[0].on && !plan_c[1].on);
        // a package takes what is with the collection too, under its own name
        let pk = d.join("pkg/minNotes Media"); let plan_p = plan(What::Package, &recs, &places, &pk);
        let kept = plan_p.iter().find(|a| a.inside).unwrap(); assert_eq!((kept.dest.as_str(), kept.uses, kept.on), ("kept-1234.png", 2, true));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn files_are_copied_checked_and_not_copied_twice_and_a_failure_stops_nothing_else() {
        let d = tmp("run"); let (src, dest) = (d.join("src"), d.join("col/minNotes Media")); std::fs::create_dir_all(&src).unwrap();
        let big: Vec<u8> = (0..(5usize << 20) + 13).map(|i| (i * 31 % 251) as u8).collect();
        std::fs::write(src.join("big.mov"), &big).unwrap(); std::fs::write(src.join("small.png"), b"small").unwrap();
        std::fs::create_dir_all(src.join(".qcview/big.mov/images")).unwrap(); std::fs::write(src.join(".qcview/big.mov/notes.json"), b"{\"n\":1}").unwrap(); std::fs::write(src.join(".qcview/big.mov/images/a.png"), b"p").unwrap();
        let p = |x: &Path| x.to_string_lossy().into_owned();
        let recs = vec![rec(serde_json::json!({ "kind": "video", "src": p(&src.join("big.mov")), "name": "big.mov" })), rec(serde_json::json!({ "src": p(&src.join("small.png")), "name": "small.png" })), rec(serde_json::json!({ "kind": "file", "src": p(&src.join("later.txt")), "name": "later.txt", "ext": "txt" }))];
        let places = Places::default();
        let mut assets = plan(What::Collect, &recs, &places, &dest);
        // one file goes missing between the look and the copy
        std::fs::write(src.join("later.txt"), b"x").unwrap(); let mut taken = HashSet::new(); for a in assets.iter_mut() { settle(a, &dest, &mut taken); } std::fs::remove_file(src.join("later.txt")).unwrap();
        let run = Run::new(&assets, &dest, true, || {}); run.start(); wait(&run);
        let at = |n: &str| assets.iter().position(|a| a.name == n).unwrap();
        assert_eq!((run.state(at("big.mov")), run.state(at("small.png")), run.state(at("later.txt"))), (DONE, DONE, FAILED));
        assert_eq!(std::fs::read(dest.join("big.mov")).unwrap(), big); assert_eq!(std::fs::read(dest.join(".qcview/big.mov/images/a.png")).unwrap(), b"p");
        assert!(names_in(&dest).iter().all(|n| !n.ends_with(".part")));
        let m = map(&assets, &run); assert_eq!(m.len(), 2);
        let new = rewrite(&recs[0], &m).unwrap(); assert!(new.contains("\"src\":\".minnotes/big.mov\"") && new.contains("\"kind\":\"video\"")); assert!(rewrite(&recs[2], &m).is_none());
        // the file comes back: a retry copies it
        std::fs::write(src.join("later.txt"), b"x").unwrap(); run.retry(at("later.txt")); wait(&run); assert_eq!(run.state(at("later.txt")), DONE);
        // a second look finds everything there: nothing is copied again
        let again = plan(What::Collect, &recs, &places, &dest); assert!(again.iter().all(|a| a.there), "{again:?}");
        let run2 = Run::new(&again, &dest, false, || {}); run2.start(); wait(&run2); assert_eq!((run2.count(THERE), run2.copied.load(Relaxed)), (3, 0));
        // a stop takes back what the run made
        run.remove_made(); assert!(!dest.join("big.mov").exists() && !dest.join(".qcview/big.mov").exists());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_collection_copy_has_its_references_rewritten_in_notes_and_canvases() {
        let d = tmp("col"); let file = d.join("c.mnd");
        let mut col = mn_store::Collection::open(&file).unwrap();
        let note = col.create(None, mn_store::DocKind::Note, "n").unwrap(); let mut doc = mn_doc::Doc::new();
        let id = BlockId::new(); doc.insert(None, 0, id, mn_doc::Kind::Media { descriptor: rec(serde_json::json!({ "src": "/x/a.png", "name": "a.png" })) }, "").unwrap(); col.save_note(note, &doc).unwrap();
        let cv = col.create(None, mn_store::DocKind::Canvas, "c").unwrap();
        col.save_canvas(cv, &mn_store::Canvas { elements: vec![mn_store::Element { id: BlockId::new(), kind: "media".into(), x: 0.0, y: 0.0, w: 10.0, h: 10.0, z: 1, parent: None, record: rec(serde_json::json!({ "kind": "video", "src": "/x/b.mov" })), locked: false }], camera: None }).unwrap();
        assert_eq!(descriptors(&col).len(), 2);
        let m: HashMap<String, String> = [("\"/x/a.png\"".to_string(), ".minnotes/a.png".to_string()), ("\"/x/b.mov\"".to_string(), ".minnotes/b.mov".to_string())].into();
        assert_eq!(rewrite_collection(&mut col, &m, &HashSet::new()).unwrap(), 2);
        let all = descriptors(&col); assert!(all.iter().any(|r| r.contains(".minnotes/a.png")) && all.iter().any(|r| r.contains(".minnotes/b.mov")), "{all:?}");
        assert_eq!(rewrite_collection(&mut col, &m, &HashSet::new()).unwrap(), 0);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn sizes_and_times_are_said_in_words() { assert_eq!((bytes(5 << 30).as_str(), bytes(1536).as_str(), span(20.0).as_str(), span(200.0).as_str(), span(4000.0).as_str()), ("5.0 GB", "2 KB", "less than a minute", "about 3 min", "about 1 h 6 min")); }
}
