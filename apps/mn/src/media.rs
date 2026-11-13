//! Pictures on the page (canon row 310, the media step's first part): a picture block's file is found, decoded and
//! reduced OFF the main thread, kept as a texture-ready picture under a memory budget, and handed to the painter.
//!
//! The block's descriptor is the Qt app's, unchanged (`docs/phase3/step-media-spec.md` §1): `src` is a file in the
//! collection's `.minnotes/` folder, a `{vol, rel}` reference under a path mapping, an absolute path, or a web address
//! (not fetched here). A descriptor without `kind` is a picture; the other kinds are later steps.
//!
//! A picture is decoded at the size it is SHOWN at (physical pixels), and again when it is shown larger - never the
//! whole file kept for a small box. What `min-image` reads is read (EXR, DPX, PNG, TIFF, JPEG, TGA, BMP); a JPEG's
//! EXIF orientation is applied. Colour: integer formats are taken as sRGB; EXR (linear light) gets the sRGB curve.
use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{mpsc, Arc, Condvar, Mutex};
use std::time::Instant;

/// The longest side a picture is decoded to: the drawing library keeps every picture of a frame in one 8192 px atlas.
const LONGEST: u32 = 4096;
/// The fade a picture arrives with.
pub const FADE_MS: u128 = 140;
/// Decoded pictures kept (bytes) unless `MN_MEDIA_BUDGET_MB` says otherwise: large media is the normal case here.
const BUDGET: usize = 4 << 30;

/// What the painter gets for a picture block.
pub enum View<'a> {
    /// Not a picture this step draws (a video, a PDF, a file chip): the caller's placeholder stays.
    Other,
    Loading,
    /// The picture and how far its fade has come (0..1).
    Ready { brush: &'a peniko::ImageBrush, w: u32, h: u32, fade: f32, /// a preview's format, shown small on it
        pill: Option<String> },
    /// The file is not there: its name, and where it was looked for (or why it could not be looked for).
    Missing { name: String, looked: String },
    /// The file is there and could not be read.
    Failed { name: String, why: String },
}

struct Pic { brush: peniko::ImageBrush, w: u32, h: u32, bytes: usize, arrived: Instant }
enum State { Loading, Ready(Pic), Missing(String), Failed(String) }
struct Entry { state: State, name: String, /// the width asked of the decoder last (0: nothing asked)
    asked: u32, /// the file has no more pixels than the picture held
    whole: bool, used: u64 }

struct Job { key: String, path: PathBuf, w: u32, h: u32, /// a preview's original (row 313): where it is, and its size and time when the preview was made
    orig: Option<(PathBuf, u64, f64)>, /// a PDF's page (row 315): the file is a PDF and this page of it is wanted
    page: Option<usize>, /// a video's frame (row 316): the file is a video and a still of this frame is wanted
    still: Option<i32> }
enum Outcome { Pic { rgba: Vec<u8>, w: u32, h: u32, whole: bool }, /// a video's frame as the decoder gives it: its colour is finished on the window's thread
    Still(qcv_player::thumbnail_cache::ThumbPixels, min_present::ColorMeta), Missing, Failed(String) }
struct Queue { jobs: Mutex<VecDeque<Job>>, more: Condvar }

/// Where files are: the collection's folder, this session's waiting place for added files, the path mappings.
#[derive(Clone, Default)]
pub struct Places { base: Option<PathBuf>, staged: Option<PathBuf>, mappings: Vec<min_paths::map::Mapping> }

/// What came back from file work asked of the helpers.
/// The folder beside a collection's document that holds the files kept with it (row 333): visible, so that whoever
/// moves the document sees what belongs to it. A record still names such a file `.minnotes/<name>` - that prefix is the
/// MARK of a file kept with the collection, not a place on the disk.
pub const SIDE: &str = "minNotes Media";
/// Where such files were kept before row 333 (hidden); still read, never written.
pub const OLD_SIDE: &str = ".minnotes";

pub enum Mail {
    /// Dropped / chosen files as block records, in order; what could not be taken, in words.
    Added { token: u64, records: Vec<String>, refused: Vec<String> },
    /// A picture named by an address (pasted web content, row 314) was fetched and is kept with the collection: every
    /// block whose record still names `src` takes these fields in its place.
    Fetched { src: String, record: String },
    /// Pictures pointed at their files again: each block with its new record.
    Relinked { records: Vec<(mn_doc::BlockId, String)>, problem: Option<String> },
}

/// What is known of a file chip's file (row 320): where it is (None: its place cannot be worked out), whether it is there, its size, a folder.
#[derive(Clone, Debug, Default)]
pub struct FileState { pub path: Option<PathBuf>, pub found: bool, pub size: Option<u64>, pub dir: bool }

pub struct Media {
    /// file chips' files, looked at again every few seconds
    files: HashMap<String, (std::time::Instant, FileState)>,
    entries: HashMap<String, Entry>, queue: Arc<Queue>, done: mpsc::Receiver<(String, u32, Outcome)>,
    places: Places, budget: usize, total: usize, frame: u64,
    mail_tx: mpsc::Sender<Mail>, mail: mpsc::Receiver<Mail>, wake: Arc<dyn Fn() + Send + Sync>,
    /// clips' notes (QCView's sidecar), by the clip's file: what was read, and when it was last asked for
    notes: HashMap<PathBuf, (Arc<Vec<qcv_notes::Note>>, Instant, bool)>, notes_tx: mpsc::Sender<(PathBuf, Vec<qcv_notes::Note>)>, notes_rx: mpsc::Receiver<(PathBuf, Vec<qcv_notes::Note>)>,
    /// addresses being fetched or fetched (their record), failures as they come back, and those to be told again
    fetching: HashMap<String, Option<String>>, fetch_tx: mpsc::Sender<(String, Result<String, String>)>, fetched: mpsc::Receiver<(String, Result<String, String>)>, again: std::collections::HashSet<String>, slots: Arc<(Mutex<usize>, Condvar)>,
}

impl Media {
    /// `wake` is called from a worker when a picture is ready (it must make the window poll and redraw).
    pub fn new(settings: &Path, wake: impl Fn() + Send + Sync + 'static) -> Media {
        let queue = Arc::new(Queue { jobs: Mutex::new(VecDeque::new()), more: Condvar::new() });
        let (tx, done) = mpsc::channel(); let wake: Arc<dyn Fn() + Send + Sync> = Arc::new(wake); let (mail_tx, mail) = mpsc::channel(); let (ftx, frx) = mpsc::channel(); let (ntx, nrx) = mpsc::channel();
        std::thread::spawn(trim_cache);
        let n = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(2).clamp(1, 4);
        for i in 0..n {
            let (q, tx, wake) = (queue.clone(), tx.clone(), wake.clone());
            let _ = std::thread::Builder::new().name(format!("mn-media-{i}")).spawn(move || loop {
                // the newest request first: it is what is on screen now
                let job = { let mut j = q.jobs.lock().unwrap(); loop { if let Some(x) = j.pop_back() { break x } j = q.more.wait(j).unwrap(); } };
                // a preview answers twice: the stored picture at once, then (when it is worth it) the original, sharper
                let mut send = |out: Outcome| { let ok = tx.send((job.key.clone(), job.w, out)).is_ok(); wake(); ok };
                let ok = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| work(&job, &mut send))).unwrap_or_else(|_| send(Outcome::Failed("the decoder stopped".into())));
                if !ok { return }
            });
        }
        let mappings = std::fs::read_to_string(settings.join("mappings.json")).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
        let budget = std::env::var("MN_MEDIA_BUDGET_MB").ok().and_then(|v| v.parse::<usize>().ok()).map(|mb| mb << 20).unwrap_or(BUDGET);
        Media { files: HashMap::new(), entries: HashMap::new(), queue, done, places: Places { base: None, staged: None, mappings }, budget, total: 0, frame: 0, mail_tx, mail, wake, notes: HashMap::new(), notes_tx: ntx, notes_rx: nrx, fetching: HashMap::new(), fetch_tx: ftx, fetched: frx, again: Default::default(), slots: Arc::new((Mutex::new(0), Condvar::new())) }
    }
    /// The folder the collection's file is in (`.minnotes/` is beside it) and where this session keeps added files
    /// until a save. A change of folder forgets every picture.
    pub fn set_base(&mut self, base: Option<&Path>, staged: Option<PathBuf>) {
        self.places.staged = staged;
        if self.places.base.as_deref() == base { return }
        self.places.base = base.map(|p| p.to_path_buf()); self.entries.clear(); self.total = 0; self.queue.jobs.lock().unwrap().clear();
    }
    /// Files to become blocks (a drop, the toolbar's button): looked at, copied or referenced, off this thread; the
    /// records come back as [`Mail::Added`] with `token`.
    pub fn add(&self, token: u64, paths: Vec<PathBuf>) {
        let (places, tx, wake) = (self.places.clone(), self.mail_tx.clone(), self.wake.clone());
        let _ = std::thread::Builder::new().name("mn-media-add".into()).spawn(move || {
            let (mut records, mut refused) = (vec![], vec![]);
            for p in paths { match places.record(&p) { Ok(r) => records.push(r), Err(why) => refused.push(format!("{}: {why}", p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default())) } }
            let _ = tx.send(Mail::Added { token, records, refused }); wake();
        });
    }
    /// A picture that came as pixels (pasted from the clipboard): kept as a PNG with the collection, like a dropped
    /// picture that is copied; its record has no name (as the Qt app's).
    pub fn add_pixels(&self, token: u64, rgba: Vec<u8>, w: u32, h: u32) {
        let (places, tx, wake) = (self.places.clone(), self.mail_tx.clone(), self.wake.clone());
        let _ = std::thread::Builder::new().name("mn-media-add".into()).spawn(move || {
            let made = min_image::write::png_rgba8_bytes(&rgba, w, h).and_then(|png| places.keep_bytes("pasted", "png", &png)).map(|src| serde_json::json!({ "src": src, "w": w, "h": h }).to_string());
            let _ = tx.send(match made { Ok(r) => Mail::Added { token, records: vec![r], refused: vec![] }, Err(why) => Mail::Added { token, records: vec![], refused: vec![format!("the pasted picture: {why}")] } }); wake();
        });
    }
    /// A picture that was not found is pointed at `file`; the note's other pictures that are not found either and
    /// have a file of their name in the same folder follow. `blocks`: the note's media blocks with their records.
    pub fn relink(&self, blocks: Vec<(mn_doc::BlockId, String)>, chosen: mn_doc::BlockId, file: PathBuf) {
        let (places, tx, wake) = (self.places.clone(), self.mail_tx.clone(), self.wake.clone());
        let _ = std::thread::Builder::new().name("mn-media-relink".into()).spawn(move || {
            let folder = file.parent().map(|p| p.to_path_buf()).unwrap_or_default(); let (mut records, mut problem) = (vec![], None);
            for (id, rec) in blocks {
                let Ok(serde_json::Value::Object(mut v)) = serde_json::from_str::<serde_json::Value>(&rec) else { continue };
                if v.contains_key("kind") { continue }
                let to = if id == chosen { file.clone() } else {
                    if v.get("src").is_some_and(|s| places.locate(s).is_ok_and(|p| p.is_file())) { continue }
                    let name = v.get("name").and_then(|n| n.as_str()).map(str::to_string).or_else(|| v.get("src").and_then(|s| s.as_str()).and_then(|s| s.rsplit(['/', '\\']).next().map(str::to_string))).unwrap_or_default();
                    let c = folder.join(&name); if name.is_empty() || !c.is_file() { continue } c };
                match if let Some(f) = preview_format(&to) { places.preview(&to, f) } else { places.picture(&to) } {
                    Ok(serde_json::Value::Object(new)) => { for k in ["src", "w", "h", "name", "preview"] { if let Some(x) = new.get(k) { v.insert(k.to_string(), x.clone()); } } if !new.contains_key("preview") { v.remove("preview"); } records.push((id, serde_json::Value::Object(v).to_string())); }
                    Ok(_) => {}
                    Err(why) => if id == chosen { problem = Some(why) },
                }
            }
            let _ = tx.send(Mail::Relinked { records, problem }); wake();
        });
    }
    /// A video's notes (QCView's `.qcview` sidecar beside the clip; rows 215, 316), read off this thread and read again
    /// every two seconds while they are asked for - another application's edits show without anything being reopened.
    /// Empty until the first reading lands (and for a clip without notes).
    pub fn notes(&mut self, descriptor: &str) -> Arc<Vec<qcv_notes::Note>> {
        let Some(path) = self.locate(descriptor) else { return Arc::new(vec![]) };
        let now = Instant::now();
        let e = self.notes.entry(path.clone()).or_insert_with(|| (Arc::new(vec![]), now - std::time::Duration::from_secs(60), false));
        if !e.2 && now.duration_since(e.1) > std::time::Duration::from_secs(2) { e.1 = now; e.2 = true;
            let (tx, wake) = (self.notes_tx.clone(), self.wake.clone());
            let _ = std::thread::Builder::new().name("mn-media-notes".into()).spawn(move || { let n = qcv_notes::io::load(&path).unwrap_or_default(); let _ = tx.send((path, n)); wake(); }); }
        e.0.clone()
    }
    /// A clip's notes as this window just changed them (it wrote them itself: no need to wait for the next reading).
    pub fn set_notes(&mut self, media: &Path, notes: Vec<qcv_notes::Note>) { let now = Instant::now(); self.notes.insert(media.to_path_buf(), (Arc::new(notes), now, false)); }
    /// A picture file was written again: what is held of it is dropped (it is read anew when next shown).
    pub fn forget_file(&mut self, path: &Path) {
        let prefix = format!("file:{}#", path.display());
        let gone: Vec<String> = self.entries.keys().filter(|k| k.starts_with(&prefix)).cloned().collect();
        for k in gone { if let Some(e) = self.entries.remove(&k) { if let State::Ready(p) = e.state { self.total -= p.bytes; } } }
    }
    /// Takes the notes that were read. True: some changed - draw again.
    pub fn poll_notes(&mut self) -> bool {
        let mut any = false;
        while let Ok((path, n)) = self.notes_rx.try_recv() { if let Some(e) = self.notes.get_mut(&path) { e.2 = false; if *e.0 != n { e.0 = Arc::new(n); any = true; } } }
        any
    }
    /// A picture file as it is (a note's small picture in the sidecar), for a box of `w` x `h` physical pixels: asked for
    /// when it is not held. `stamp` tells one state of the file from another (it is rewritten when its note is drawn on).
    pub fn file_picture(&mut self, path: &Path, stamp: u64, w: f64, h: f64) -> Option<&peniko::ImageBrush> {
        let key = format!("file:{}#{stamp}", path.display()); let (w, h) = ((w.ceil() as u32).max(1), (h.ceil() as u32).max(1));
        if !self.entries.contains_key(&key) { self.ask(&key, path.to_path_buf(), w, h, None, None, None); self.entries.insert(key.clone(), Entry { state: State::Loading, name: String::new(), asked: w, whole: false, used: self.frame }); }
        let e = self.entries.get_mut(&key)?; e.used = self.frame;
        match &e.state { State::Ready(p) => Some(&p.brush), _ => None }
    }
    /// Where a block's file is on this machine (None: its record names none that can be looked for).
    /// Where files are, for work off this thread.
    pub fn places(&self) -> Places { self.places.clone() }
    pub fn locate(&self, descriptor: &str) -> Option<PathBuf> { let v: serde_json::Value = serde_json::from_str(descriptor).ok()?; self.places.locate(v.get("src")?).ok() }
    /// The characters of the page a PDF block shows, once its page has been drawn (None: not yet, or none to read).
    pub fn page_text(&self, descriptor: &str) -> Option<Arc<crate::pdf::PageText>> {
        let v: serde_json::Value = serde_json::from_str(descriptor).ok()?;
        if v.get("kind").and_then(|k| k.as_str()) != Some("pdf") { return None }
        TEXTS.lock().unwrap().get(&format!("{}#{}", v.get("src")?, v.get("page").and_then(|p| p.as_u64()).unwrap_or(0))).cloned()
    }
    /// What the helpers sent back.
    pub fn mail(&mut self) -> Vec<Mail> {
        let mut out: Vec<Mail> = self.mail.try_iter().collect();
        for (src, got) in self.fetched.try_iter().collect::<Vec<_>>() { match got {
            Ok(record) => { self.fetching.insert(src.clone(), Some(record.clone())); out.push(Mail::Fetched { src, record }); }
            Err(why) => { if let Some(e) = self.entries.get_mut(&serde_json::Value::String(src.clone()).to_string()) { e.state = State::Failed(why); } }
        } }
        // a record that names an address already fetched (a redo, another note): told again
        for src in std::mem::take(&mut self.again) { if let Some(Some(record)) = self.fetching.get(&src) { out.push(Mail::Fetched { src, record: record.clone() }); } }
        out
    }
    /// A picture named by an address is fetched (three at a time), kept with the collection, and reported as
    /// [`Mail::Fetched`]; asked once per address.
    fn fetch(&mut self, src: &str) {
        match self.fetching.get(src) { Some(Some(_)) => { self.again.insert(src.to_string()); return } Some(None) => return, None => {} }
        self.fetching.insert(src.to_string(), None);
        let (places, tx, wake, slots, src) = (self.places.clone(), self.fetch_tx.clone(), self.wake.clone(), self.slots.clone(), src.to_string());
        let _ = std::thread::Builder::new().name("mn-media-fetch".into()).spawn(move || {
            { let mut n = slots.0.lock().unwrap(); while *n >= 3 { n = slots.1.wait(n).unwrap(); } *n += 1; }
            let got = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| fetched(&places, &src))).unwrap_or_else(|_| Err("the fetch stopped".into()));
            { *slots.0.lock().unwrap() -= 1; slots.1.notify_one(); }
            let _ = tx.send((src, got)); wake();
        });
    }
    /// A file chip's file: looked for on disk at most every four seconds.
    pub fn file_state(&mut self, descriptor: &str) -> FileState {
        let v: serde_json::Value = serde_json::from_str(descriptor).unwrap_or_default(); let Some(src) = v.get("src") else { return FileState::default() }; let key = src.to_string();
        if let Some((t, st)) = self.files.get(&key) { if t.elapsed().as_secs() < 4 { return st.clone() } }
        let path = self.places.locate(src).ok(); let meta = path.as_ref().and_then(|p| std::fs::metadata(p).ok());
        let st = FileState { found: meta.is_some(), size: meta.as_ref().filter(|m| m.is_file()).map(|m| m.len()), dir: meta.as_ref().map(|m| m.is_dir()).unwrap_or(v.get("dir").and_then(|d| d.as_bool()).unwrap_or(false)), path };
        self.files.insert(key, (std::time::Instant::now(), st.clone())); st
    }
    /// The record of a file chip that points at `path` (a chip located again).
    pub fn file_record(&self, path: &Path) -> String {
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let mut v = serde_json::json!({ "kind": "file", "src": self.places.reference(path), "name": name, "ext": path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default() });
        if path.is_dir() { v["dir"] = serde_json::json!(true); } v.to_string()
    }
    /// A picture's record names a file that is not there (or cannot be looked for).
    pub fn is_missing(&self, descriptor: &str) -> bool {
        let v: serde_json::Value = serde_json::from_str(descriptor).unwrap_or_default();
        v.get("kind").is_none() && v.get("src").is_some_and(|s| matches!(self.entries.get(&s.to_string()).map(|e| &e.state), Some(State::Missing(_))))
    }
    /// A frame begins (what is shown in it is kept when the budget is met).
    pub fn begin_frame(&mut self) { self.frame += 1; }
    /// Takes what the workers finished. True: something arrived, draw again.
    pub fn poll(&mut self, finish: &mut dyn FnMut(&qcv_player::thumbnail_cache::ThumbPixels, &min_present::ColorMeta) -> (Vec<u8>, u32, u32)) -> bool {
        let mut any = false;
        while let Ok((key, w, out)) = self.done.try_recv() {
            // a video's still: its colour is finished here (the GPU's pipeline), then it is a picture like any other
            let out = match out { Outcome::Still(px, meta) => { let (rgba, w, h) = finish(&px, &meta); Outcome::Pic { rgba, w, h, whole: false } } other => other };
            let Some(e) = self.entries.get_mut(&key) else { continue };   // forgotten meanwhile
            if e.asked != w { continue }                                   // a larger one was asked since
            any = true;
            if let State::Ready(p) = &e.state { self.total -= p.bytes; }
            match out {
                Outcome::Pic { rgba, w, h, whole } => {
                    // a picture replacing a smaller one of itself does not fade again
                    let arrived = match &e.state { State::Ready(p) => p.arrived, _ => Instant::now() };
                    let bytes = rgba.len(); self.total += bytes; e.whole = whole;
                    e.state = State::Ready(Pic { brush: min_ui::paint::image_rgba8(w, h, rgba), w, h, bytes, arrived });
                }
                Outcome::Still(..) => {}
                Outcome::Missing => e.state = State::Missing(String::new()),
                Outcome::Failed(why) => e.state = State::Failed(why),
            }
        }
        if any { self.trim(); }
        any
    }
    /// Over the budget: the pictures seen longest ago go first; what this frame shows stays.
    fn trim(&mut self) {
        if self.total <= self.budget { return }
        let mut old: Vec<(u64, String, usize)> = self.entries.iter().filter_map(|(k, e)| match &e.state { State::Ready(p) if e.used < self.frame => Some((e.used, k.clone(), p.bytes)), _ => None }).collect();
        old.sort();
        for (_, k, bytes) in old { if self.total <= self.budget { break } self.entries.remove(&k); self.total -= bytes; }
    }
    /// The picture of a block for a box of `w` x `h` physical pixels; asks for it (or for a larger one) when needed.
    pub fn view(&mut self, descriptor: &str, w: f64, h: f64) -> View<'_> {
        let v: serde_json::Value = serde_json::from_str(descriptor).unwrap_or_default();
        // a PDF's page is a picture of its own: one per page of the file
        let page = (v.get("kind").and_then(|k| k.as_str()) == Some("pdf")).then(|| v.get("page").and_then(|p| p.as_u64()).unwrap_or(0) as usize);
        // a video at rest is a still of the frame it stands on (its first, until it has been played)
        let still = (v.get("kind").and_then(|k| k.as_str()) == Some("video")).then(|| v.get("frame").and_then(|p| p.as_i64()).unwrap_or(0) as i32);
        if v.get("kind").is_some() && page.is_none() && still.is_none() { return View::Other }
        let Some(src) = v.get("src") else { return View::Other };
        let key = match (page, still) { (Some(p), _) => format!("{src}#{p}"), (_, Some(f)) => format!("{src}@{f}"), _ => src.to_string() };
        // a preview (row 313): `src` is the picture made of the original, `preview` says where the original is
        let pv = v.get("preview");
        let orig = pv.and_then(|p| Some((self.places.locate(p.get("src")?).ok()?, p.get("size").and_then(|x| x.as_f64()).unwrap_or(0.0) as u64, p.get("mtime").and_then(|x| x.as_f64()).unwrap_or(0.0))));
        let pill = pv.and_then(|p| p.get("format")).and_then(|f| f.as_str()).map(str::to_string);
        let (w, h) = { let (w, h) = (w.ceil().max(1.0), h.ceil().max(1.0)); let k = (LONGEST as f64 / w.max(h)).min(1.0); (((w * k) as u32).max(1), ((h * k) as u32).max(1)) };
        // named by an address: shown as loading while it is fetched; the record is replaced when it arrives
        if let Some(addr) = src.as_str().filter(|s| remote(s) && page.is_none() && still.is_none()) {
            if self.places.staged.is_none() { return View::Missing { name: String::new(), looked: "on the web; a collection with a file is needed to keep it".into() } }
            let addr = addr.to_string(); self.fetch(&addr);
            let e = self.entries.entry(key).or_insert(Entry { state: State::Loading, name: short(&addr), asked: 0, whole: false, used: self.frame }); e.used = self.frame;
            return match &e.state { State::Failed(why) => View::Failed { name: e.name.clone(), why: why.clone() }, _ => View::Loading }
        }
        if !self.entries.contains_key(&key) {
            let name = v.get("name").and_then(|n| n.as_str()).map(str::to_string);
            let (state, name, asked) = match self.places.locate(src) {
                Ok(path) => { let name = name.unwrap_or_else(|| path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()); self.ask(&key, path, w, h, orig.clone(), page, still); (State::Loading, name, w) }
                Err(why) => (State::Missing(why), name.unwrap_or_default(), 0),
            };
            self.entries.insert(key.clone(), Entry { state, name, asked, whole: false, used: self.frame });
        }
        // shown larger than what is held (a wider window, a denser screen): the same file again, larger
        let again = { let e = &self.entries[&key]; matches!(&e.state, State::Ready(_)) && !e.whole && w > e.asked + e.asked / 8 };
        if again { if let Ok(path) = self.places.locate(src) { self.ask(&key, path, w, h, orig, page, still); self.entries.get_mut(&key).unwrap().asked = w; } }
        let looked = match &self.entries[&key].state { State::Missing(why) if why.is_empty() => self.places.locate(src).map(|p| p.to_string_lossy().into_owned()).unwrap_or_default(), State::Missing(why) => why.clone(), _ => String::new() };
        let e = self.entries.get_mut(&key).unwrap(); e.used = self.frame;
        match &e.state {
            State::Loading => View::Loading,
            State::Ready(p) => View::Ready { brush: &p.brush, w: p.w, h: p.h, fade: (p.arrived.elapsed().as_millis() as f32 / FADE_MS as f32).min(1.0), pill },
            State::Missing(_) => View::Missing { name: e.name.clone(), looked },
            State::Failed(why) => View::Failed { name: e.name.clone(), why: why.clone() },
        }
    }
    fn ask(&self, key: &str, path: PathBuf, w: u32, h: u32, orig: Option<(PathBuf, u64, f64)>, page: Option<usize>, still: Option<i32>) {
        let mut j = self.queue.jobs.lock().unwrap(); j.retain(|x| x.key != key); j.push_back(Job { key: key.to_string(), path, w, h, orig, page, still }); self.queue.more.notify_one();
    }
    /// Bytes of decoded pictures held, and how many (for a scripted look).
    pub fn held(&self) -> (usize, usize) { (self.total, self.entries.values().filter(|e| matches!(e.state, State::Ready(_))).count()) }
}

impl Places {
    /// The places of a collection in `base` with nothing waiting and no mapped volumes.
    #[cfg(test)] pub fn at(base: Option<PathBuf>) -> Places { Places { base, staged: None, mappings: vec![] } }
    /// Where a descriptor's `src` is on this machine. Err: why it cannot be looked for.
    pub fn locate(&self, src: &serde_json::Value) -> Result<PathBuf, String> {
        use min_paths::map::{resolve_ref, Ref};
        let os = min_paths::Os::current();
        let r = match src {
            serde_json::Value::Object(o) => Ref::Vol { vol: o.get("vol").and_then(|v| v.as_str()).unwrap_or("").to_string(), rel: o.get("rel").and_then(|r| r.as_array()).map(|a| a.iter().filter_map(|s| s.as_str().map(str::to_string)).collect()).unwrap_or_default() },
            serde_json::Value::String(s) if s.is_empty() => return Err("the block names no file".into()),
            serde_json::Value::String(s) if s.starts_with("http://") || s.starts_with("https://") => return Err(format!("on the web, not fetched: {s}")),
            serde_json::Value::String(s) if s.starts_with(".minnotes/") => return match (&self.staged, &self.base) {
                // added since the last save: still on this machine
                (Some(st), _) if st.join(&s[10..]).is_file() => Ok(st.join(&s[10..])),
                // beside the document: the visible folder, else the hidden one of before row 333
                (_, Some(b)) => { let (new, old) = (b.join(SIDE).join(&s[10..]), b.join(s)); Ok(if !new.is_file() && old.is_file() { old } else { new }) } _ => Err("the collection has no folder yet".into()) },
            serde_json::Value::String(s) => Ref::Abs(s.clone()),
            _ => return Err("the block names no file".into()),
        };
        match (&r, resolve_ref(&r, os, &self.mappings)) {
            (_, Some(p)) if !p.is_empty() => Ok(PathBuf::from(p)),
            (Ref::Vol { vol, rel }, _) => Err(format!("on a volume this machine has no path for ({vol}): {}", rel.join("/"))),
            (Ref::Abs(s), _) => Ok(PathBuf::from(s)),
        }
    }
    /// How `path` is named in a record when the file stays where it is: under a mapped volume, or as it is.
    fn reference(&self, path: &Path) -> serde_json::Value {
        use min_paths::map::{to_ref, Ref};
        match to_ref(&path.to_string_lossy(), min_paths::Os::current(), &self.mappings) { Ref::Vol { vol, rel } => serde_json::json!({ "vol": vol, "rel": rel }), Ref::Abs(s) => serde_json::Value::String(s) }
    }
    /// The Qt app's rule (spec §3): a picture on a network share stays there and is referenced; any other is COPIED
    /// into the collection - here into the session's waiting place, to go beside the document at the next save. The
    /// copy is named after the file with eight digits of its content's hash, so the same picture is kept once.
    fn keep(&self, path: &Path) -> Result<serde_json::Value, String> {
        use std::io::{Read, Write};
        let p = path.to_string_lossy();
        if min_paths::classify::is_network_source(&p) && !min_paths::classify::is_volatile_source(&p, &[]) { return Ok(self.reference(path)) }
        let dir = self.staged.as_ref().ok_or("the collection has no file yet: save it first")?;
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        let tmp = dir.join(format!(".adding-{}-{:?}.tmp", std::process::id(), std::thread::current().id()).replace(['(', ')'], ""));
        let mut hash = 0xcbf2_9ce4_8422_2325u64;
        let done = (|| -> std::io::Result<()> {
            let (mut from, mut to) = (std::fs::File::open(path)?, std::fs::File::create(&tmp)?); let mut buf = vec![0u8; 1 << 20];
            loop { let n = from.read(&mut buf)?; if n == 0 { break } for b in &buf[..n] { hash = (hash ^ *b as u64).wrapping_mul(0x0000_0100_0000_01b3); } to.write_all(&buf[..n])?; }
            to.sync_all()
        })();
        if let Err(e) = done { let _ = std::fs::remove_file(&tmp); return Err(e.to_string()) }
        let stem: String = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default().chars().map(|c| if c.is_alphanumeric() || matches!(c, '-' | '_' | '.') { c } else { '-' }).take(60).collect();
        let ext = path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_else(|| "img".into());
        self.settle(dir, &tmp, &stem, &ext, hash)
    }
    /// A file written under a temporary name in the waiting place takes its lasting name; when the same content is
    /// there already (in this session, or beside the document) the new copy is dropped.
    fn settle(&self, dir: &Path, tmp: &Path, stem: &str, ext: &str, hash: u64) -> Result<serde_json::Value, String> {
        let name = format!("{}-{:08x}.{ext}", if stem.is_empty() { "picture" } else { stem }, (hash >> 32) as u32 ^ hash as u32);
        let at = dir.join(&name);
        let there = at.is_file() || self.base.as_ref().is_some_and(|b| b.join(SIDE).join(&name).is_file() || b.join(OLD_SIDE).join(&name).is_file());
        if there { let _ = std::fs::remove_file(tmp); } else { std::fs::rename(tmp, &at).map_err(|e| e.to_string())?; }
        Ok(serde_json::Value::String(format!(".minnotes/{name}")))
    }
    /// Bytes made here (a preview's PNG, a pasted picture) are kept in the waiting place under a name of their hash.
    fn keep_bytes(&self, stem: &str, ext: &str, bytes: &[u8]) -> Result<serde_json::Value, String> {
        let dir = self.staged.as_ref().ok_or("the collection has no file yet: save it first")?;
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        let tmp = dir.join(format!(".adding-{}-{:?}.tmp", std::process::id(), std::thread::current().id()).replace(['(', ')'], ""));
        std::fs::write(&tmp, bytes).map_err(|e| e.to_string())?;
        self.settle(dir, &tmp, stem, ext, fnv(bytes))
    }
    /// A file whose picture is MADE of it (a Photoshop file, an SVG, a camera's HEIC or RAW; row 313), as the Qt app's
    /// preview record: `src` = a PNG of it (its longer side at most 2048) kept with the collection, so the picture
    /// shows wherever the collection is opened; `preview` = the original, always referenced where it is, with its
    /// size and time. An SVG's `w` / `h` are its own nominal size; the others' are the PNG's.
    fn preview(&self, path: &Path, format: &str) -> Result<serde_json::Value, String> {
        let r = render(path, STORED)?;
        let png = min_image::write::png_rgba8_bytes(&r.rgba, r.w, r.h)?;
        let stem: String = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default().chars().map(|c| if c.is_alphanumeric() || matches!(c, '-' | '_' | '.') { c } else { '-' }).take(60).collect();
        let src = self.keep_bytes(&stem, "png", &png)?;
        let m = std::fs::metadata(path).map_err(|e| e.to_string())?;
        let (w, h) = r.nominal.unwrap_or((r.w, r.h));
        Ok(serde_json::json!({ "src": src, "w": w, "h": h, "name": path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
            "preview": { "src": self.reference(path), "format": format, "mtime": mtime_ms(&m), "size": m.len() as f64 } }))
    }
    /// As [`Self::picture`], for a file that will not stay (a fetched one): always copied.
    fn picture_copied(&self, path: &Path) -> Result<serde_json::Value, String> {
        let (w, h) = size_of(path)?; let (w, h) = if orientation(path) >= 5 { (h, w) } else { (w, h) };
        let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
        let stem: String = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default().chars().map(|c| if c.is_alphanumeric() || matches!(c, '-' | '_' | '.') { c } else { '-' }).take(60).collect();
        let ext = path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_else(|| "img".into());
        Ok(serde_json::json!({ "src": self.keep_bytes(&stem, &ext, &bytes)?, "w": w, "h": h, "name": path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default() }))
    }
    /// A picture file as a block record: its size as it is SEEN (a JPEG lying on its side is measured upright).
    fn picture(&self, path: &Path) -> Result<serde_json::Value, String> {
        let p = path.to_string_lossy();
        let (w, h) = size_of(path)?; let _ = &p;
        let (w, h) = if orientation(path) >= 5 { (h, w) } else { (w, h) };
        Ok(serde_json::json!({ "src": self.keep(path)?, "w": w, "h": h, "name": path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default() }))
    }
    /// Any dropped file as a block record: a picture `min-image` reads is a picture; anything else is a file chip,
    /// referenced where it is (videos, PDFs and the preview formats get records of their own with their steps).
    fn record(&self, path: &Path) -> Result<String, String> {
        // a folder: a chip that points at it (never copied)
        if path.is_dir() { return Ok(serde_json::json!({ "kind": "file", "dir": true, "src": self.reference(path), "name": path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(), "ext": "" }).to_string()) }
        if !path.is_file() { return Err("it is not there".into()) }
        if readable(path) { return self.picture(path).map(|v| v.to_string()) }
        if let Some(format) = preview_format(path) { return self.preview(path, format).map(|v| v.to_string()) }
        // a video: the Qt app's record - referenced where it is, its coded size, its length, frames and rate
        if crate::video::is_video(path) {
            let p = crate::video::probe(path)?;
            return Ok(serde_json::json!({ "kind": "video", "src": self.reference(path), "w": p.w, "h": p.h, "durMs": p.dur_ms, "frames": p.frames, "fps": p.fps, "name": path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default() }).to_string())
        }
        // a PDF: the Qt app's record - referenced where it is, its first page's size in points, its pages
        if path.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case("pdf")) {
            let (pages, w, h) = crate::pdf::info(path)?;
            return Ok(serde_json::json!({ "kind": "pdf", "src": self.reference(path), "w": w.round() as u32, "h": h.round() as u32, "pages": pages, "name": path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default() }).to_string())
        }
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        Ok(serde_json::json!({ "kind": "file", "src": self.reference(path), "name": name, "ext": path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default() }).to_string())
    }
}

/// A save's first half (runs on the save's helper thread): the files added in this session go beside the document,
/// into `side` (its `minNotes Media/`), each written under a temporary name and renamed; one already there with the same
/// size is left. `also`: another folder of such files to carry along (Save As to a new place).
pub fn publish(staged: &Path, side: &Path, also: &[PathBuf]) -> std::io::Result<()> {
    for from in std::iter::once(staged).chain(also.iter().map(|p| p.as_path())) {
        let Ok(list) = std::fs::read_dir(from) else { continue };
        for e in list.flatten() {
            let (p, name) = (e.path(), e.file_name()); let Ok(m) = e.metadata() else { continue };
            if !m.is_file() || name.to_string_lossy().ends_with(".tmp") { continue }
            let to = side.join(&name);
            if std::fs::metadata(&to).is_ok_and(|t| t.len() == m.len()) { continue }
            std::fs::create_dir_all(side)?;
            let tmp = side.join(format!(".{}.{}.tmp", name.to_string_lossy(), std::process::id()));
            if let Err(e) = std::fs::copy(&p, &tmp).and_then(|_| std::fs::rename(&tmp, &to)) { let _ = std::fs::remove_file(&tmp); return Err(e) }
        }
        // collected clips' notes (QCView's sidecar folders) go along with a Save As
        if from != staged { let q = from.join(".qcview"); if q.is_dir() { carry(&q, &side.join(".qcview"))?; } }
    }
    Ok(())
}

/// A folder's files copied into another, folders walked; a file there already with the same size is left.
fn carry(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for e in std::fs::read_dir(from)?.flatten() { let (p, t) = (e.path(), to.join(e.file_name())); let Ok(m) = e.metadata() else { continue };
        if m.is_dir() { carry(&p, &t)?; } else if !std::fs::metadata(&t).is_ok_and(|x| x.len() == m.len()) { std::fs::copy(&p, &t)?; } }
    Ok(())
}

// ── the worker's side

/// A `src` that is an address to fetch from, not a file of this machine or the collection.
pub fn remote(s: &str) -> bool { s.starts_with("http://") || s.starts_with("https://") || s.starts_with("data:image/") }
/// An address in few words (for a state shown in the box).
fn short(addr: &str) -> String { if addr.starts_with("data:") { "an embedded picture".into() } else { addr.split(['?', '#']).next().unwrap_or(addr).chars().take(80).collect() } }
/// What a picture's first bytes say it is.
fn sniff(b: &[u8]) -> Option<&'static str> {
    let text = || { let head = String::from_utf8_lossy(&b[..b.len().min(512)]).to_ascii_lowercase(); head.contains("<svg") };
    Some(if b.starts_with(b"\x89PNG") { "png" } else if b.starts_with(&[0xff, 0xd8]) { "jpg" } else if b.starts_with(b"GIF8") { "gif" } else if b.len() > 12 && &b[..4] == b"RIFF" && &b[8..12] == b"WEBP" { "webp" }
        else if b.starts_with(b"BM") { "bmp" } else if b.starts_with(b"II*\0") || b.starts_with(b"MM\0*") { "tif" } else if b.starts_with(b"8BPS") { "psd" }
        else if b.len() > 12 && &b[4..8] == b"ftyp" { if &b[8..12] == b"avif" { "avif" } else { "heic" } } else if text() { "svg" } else { return None })
}
/// The picture an address names, kept with the collection as a dropped file of it would be: its record.
fn fetched(places: &Places, addr: &str) -> Result<String, String> {
    const MOST: u64 = 64 << 20;
    let (bytes, name): (Vec<u8>, String) = if let Some(rest) = addr.strip_prefix("data:") {
        let (head, data) = rest.split_once(',').ok_or("the embedded picture is not complete")?;
        let bytes = if head.ends_with(";base64") { use base64::Engine; base64::engine::general_purpose::STANDARD.decode(data.trim().replace(['\n', '\r', ' '], "")).map_err(|e| e.to_string())? } else { min_paths::url::percent_decode(data).into_bytes() };
        (bytes, "pasted".into())
    } else {
        let agent: ureq::Agent = ureq::Agent::config_builder().timeout_global(Some(std::time::Duration::from_secs(30))).build().into();
        let mut resp = agent.get(addr).header("User-Agent", "minNotes").header("Accept", "image/*,*/*;q=0.5").call().map_err(|e| e.to_string())?;
        let bytes = resp.body_mut().with_config().limit(MOST).read_to_vec().map_err(|e| e.to_string())?;
        let last = addr.split(['?', '#']).next().unwrap_or("").rsplit('/').next().unwrap_or("");
        (bytes, min_paths::url::percent_decode(last))
    };
    let ext = sniff(&bytes).ok_or("what came back is not a picture")?;
    if ext == "avif" { return Err("AVIF pictures are not read yet".into()) }
    let stem: String = std::path::Path::new(&name).file_stem().map(|s| s.to_string_lossy().into_owned()).filter(|s| !s.is_empty()).unwrap_or_else(|| "picture".into()).chars().filter(|c| !matches!(c, '/' | '\\' | ':')).take(60).collect();
    // through a file of its own, so that it is measured, copied (or previewed) exactly as a dropped file is
    static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let dir = cache_dir().join(format!("fetch-{}-{}", std::process::id(), N.fetch_add(1, std::sync::atomic::Ordering::Relaxed)));
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let tmp = dir.join(format!("{stem}.{ext}"));
    let rec = std::fs::write(&tmp, &bytes).map_err(|e| e.to_string()).and_then(|_| if readable(&tmp) { places.picture_copied(&tmp) } else if let Some(f) = preview_format(&tmp) { places.preview(&tmp, f) } else { Err(format!("{} pictures are not read yet", ext.to_uppercase())) });
    let _ = std::fs::remove_dir_all(&dir);
    // a preview of a fetched file has no original that stays: it is a plain picture
    rec.map(|mut v| { if let Some(o) = v.as_object_mut() { o.remove("preview"); } v.to_string() })
}

/// PDF pages' characters, by the page's key (read on the helpers, looked up by the window).
static TEXTS: std::sync::LazyLock<Mutex<HashMap<String, Arc<crate::pdf::PageText>>>> = std::sync::LazyLock::new(|| Mutex::new(HashMap::new()));
/// The longer side of the picture kept with the collection for a preview (the Qt app's figure).
const STORED: u32 = 2048;
/// Rendered originals kept on this machine (bytes) unless `MN_PREVIEW_CACHE_MB` says otherwise.
const CACHE: u64 = 5 << 30;

fn fnv(bytes: &[u8]) -> u64 { let mut h = 0xcbf2_9ce4_8422_2325u64; for b in bytes { h = (h ^ *b as u64).wrapping_mul(0x0000_0100_0000_01b3); } h }
fn mtime_ms(m: &std::fs::Metadata) -> f64 { m.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_millis() as f64).unwrap_or(0.0) }
/// Where rendered originals are kept on this machine (scripted runs keep theirs beside their sessions).
fn cache_dir() -> PathBuf {
    if let Some(p) = std::env::var_os("MN_SESSIONS_DIR") { return PathBuf::from(p).join("preview-cache") }
    let base = if cfg!(target_os = "macos") { std::env::var_os("HOME").map(|h| PathBuf::from(h).join("Library/Caches")) } else if cfg!(windows) { std::env::var_os("LOCALAPPDATA").map(PathBuf::from) } else { std::env::var_os("XDG_CACHE_HOME").map(PathBuf::from).or(std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache"))) };
    base.unwrap_or_else(std::env::temp_dir).join("minNotes").join("previews")
}
/// Over the budget, the files used longest ago go (a file's time is set when it is used).
fn trim_cache() {
    let budget = std::env::var("MN_PREVIEW_CACHE_MB").ok().and_then(|v| v.parse::<u64>().ok()).map(|mb| mb << 20).unwrap_or(CACHE);
    let Ok(list) = std::fs::read_dir(cache_dir()) else { return };
    let mut files: Vec<(std::time::SystemTime, u64, PathBuf)> = list.flatten().filter_map(|e| { let m = e.metadata().ok()?; m.is_file().then(|| (m.modified().unwrap_or(std::time::UNIX_EPOCH), m.len(), e.path())) }).collect();
    let mut total: u64 = files.iter().map(|f| f.1).sum(); files.sort();
    for (_, len, p) in files { if total <= budget { break } if std::fs::remove_file(&p).is_ok() { total -= len; } }
}

/// The format of a file whose picture is made of it (None: not one of them).
pub fn preview_format(path: &Path) -> Option<&'static str> {
    Some(match path.extension()?.to_str()?.to_ascii_lowercase().as_str() {
        "psd" => "PSD", "psb" => "PSB", "svg" => "SVG", "ai" => "AI", "heic" | "heif" | "hif" => "HEIC",
        "dng" | "cr2" | "cr3" | "nef" | "arw" | "raf" | "orf" | "rw2" | "pef" | "srw" => "RAW", _ => return None })
}
pub struct Rendered { pub rgba: Vec<u8>, pub w: u32, pub h: u32, /// a vector file's own size
    pub nominal: Option<(u32, u32)> }
/// The picture of such a file, its longer side at most `max`.
fn render(path: &Path, max: u32) -> Result<Rendered, String> {
    match preview_format(path) {
        Some("PSD" | "PSB") => crate::psd::load(path, max).map(|f| Rendered { rgba: f.rgba, w: f.w, h: f.h, nominal: None }),
        Some("SVG") => svg(path, max),
        // an Illustrator file carries a PDF of itself: its first artboard, flat
        Some("AI") => { let (_, w, h) = crate::pdf::info(path)?; let width = if w >= h { max } else { ((max as f32 * w / h).round() as u32).max(1) }; crate::pdf::page(path, 0, width).map(|(rgba, w, h)| Rendered { rgba, w, h, nominal: None }) }
        Some(f) => system(path, max).map_err(|e| if e.is_empty() { format!("{f} files are not read on this system yet") } else { e }),
        None => Err("not a format a picture is made of".into()),
    }
}
/// An SVG drawn to pixels with its longer side `max` (it has no pixels of its own to run out of).
fn svg(path: &Path, max: u32) -> Result<Rendered, String> {
    use resvg::{tiny_skia, usvg};
    let data = std::fs::read(path).map_err(|e| e.to_string())?;
    let mut opt = usvg::Options::default(); opt.fontdb_mut().load_system_fonts();
    opt.resources_dir = path.parent().map(|p| p.to_path_buf());
    let tree = usvg::Tree::from_data(&data, &opt).map_err(|e| e.to_string())?;
    let (nw, nh) = (tree.size().width() as f64, tree.size().height() as f64); let long = nw.max(nh).max(1.0);
    let k = max as f64 / long;
    let (w, h) = (((nw * k).round() as u32).max(1), ((nh * k).round() as u32).max(1));
    let mut pm = tiny_skia::Pixmap::new(w, h).ok_or("the SVG is too large to draw")?;
    resvg::render(&tree, tiny_skia::Transform::from_scale((w as f64 / nw) as f32, (h as f64 / nh) as f32), &mut pm.as_mut());
    // the drawing's colour is multiplied by its alpha: divided out
    let mut rgba = pm.take(); for px in rgba.chunks_exact_mut(4) { let a = px[3] as u32; if a > 0 && a < 255 { for c in &mut px[..3] { *c = ((*c as u32 * 255 + a / 2) / a).min(255) as u8; } } }
    Ok(Rendered { rgba, w, h, nominal: Some(((nw.round() as u32).max(1), (nh.round() as u32).max(1))) })
}
/// The system's own decoders (HEIC, camera RAW). An empty error: this system has none wired yet.
#[cfg(not(target_os = "macos"))]
fn system(_path: &Path, _max: u32) -> Result<Rendered, String> { Err(String::new()) }
#[cfg(target_os = "macos")]
fn system(path: &Path, max: u32) -> Result<Rendered, String> { crate::imageio::load(path, max).map(|(rgba, w, h)| Rendered { rgba, w, h, nominal: None }) }

/// One request. A plain picture: decoded and sent. A preview: the stored picture is sent at once; then the ORIGINAL
/// is looked at - when it has changed since the preview was made, or the box is larger than the stored picture
/// covers, it is rendered again (once per machine and version of the file: the result is kept in the cache) and
/// sent in the stored picture's place. False: nobody listens any more.
fn work(job: &Job, send: &mut dyn FnMut(Outcome) -> bool) -> bool {
    // a video's still: the frame, reduced by the decoder to about the size asked
    if let Some(f) = job.still {
        if !job.path.is_file() { return send(Outcome::Missing) }
        return send(match crate::video::still(&job.path, f, job.w.max(job.h)) { Ok((px, meta)) => Outcome::Still(px, meta), Err(why) => Outcome::Failed(why) })
    }
    // a PDF's page: drawn at the width asked; its characters are read once and kept for selection
    if let Some(p) = job.page {
        if !job.path.is_file() { return send(Outcome::Missing) }
        let ok = send(match crate::pdf::page(&job.path, p, job.w) { Ok((rgba, w, h)) => Outcome::Pic { rgba, w, h, whole: false }, Err(why) => Outcome::Failed(why) });
        if ok && !TEXTS.lock().unwrap().contains_key(&job.key) { if let Ok(t) = crate::pdf::text(&job.path, p) { let mut all = TEXTS.lock().unwrap(); if all.len() > 64 { all.clear(); } all.insert(job.key.clone(), Arc::new(t)); } }
        return ok
    }
    let first = decode(&job.path, job.w, job.h);
    let Some((orig, size, mtime)) = &job.orig else { return send(first) };
    let (shown, covered) = match &first { Outcome::Pic { whole, .. } => (true, !*whole), _ => (false, false) };
    let Ok(m) = std::fs::metadata(orig) else { return send(first) };   // the original is away: the stored picture is what there is
    let changed = m.len() != *size || (mtime_ms(&m) - *mtime).abs() > 1500.0;
    if shown && covered && !changed { return send(first) }
    let failed = match first { Outcome::Pic { rgba, w, h, .. } => { if !send(Outcome::Pic { rgba, w, h, whole: false }) { return false } None } other => Some(other) };
    let long = if job.w.max(job.h) <= STORED { STORED } else { LONGEST };
    let at = cache_dir().join(format!("{:016x}-{long}.png", fnv(format!("{}|{}|{}", orig.display(), m.len(), mtime_ms(&m)).as_bytes())));
    let got = if at.is_file() { let _ = std::fs::File::options().write(true).open(&at).and_then(|f| f.set_modified(std::time::SystemTime::now())); min_image::load(&at.to_string_lossy(), "", 2).map(|i| rgba8(&i)).map_err(|e| e.to_string()) }
        else { render(orig, long).map(|r| { if let Ok(png) = min_image::write::png_rgba8_bytes(&r.rgba, r.w, r.h) { let _ = std::fs::create_dir_all(cache_dir()); let tmp = at.with_extension(format!("{}.tmp", std::process::id())); if std::fs::write(&tmp, png).and_then(|_| std::fs::rename(&tmp, &at)).is_err() { let _ = std::fs::remove_file(&tmp); } } (r.rgba, r.w, r.h) }) };
    match got {
        Ok((mut rgba, mut w, mut h)) => { let whole = long == LONGEST || w.max(h) < long; if w > job.w && h > job.h { let (tw, th) = fit(w, h, job.w, job.h); rgba = shrink(&rgba, w, h, tw, th); w = tw; h = th; } send(Outcome::Pic { rgba, w, h, whole }) }
        // the original would not render: the stored picture stays; with none, that is said
        Err(why) => match failed { Some(Outcome::Missing) | None => if shown { true } else { send(Outcome::Failed(why)) }, Some(other) => send(other) },
    }
}

fn decode(path: &Path, want_w: u32, want_h: u32) -> Outcome {
    if !path.is_file() { return Outcome::Missing }
    let p = path.to_string_lossy();
    if !readable(path) { return Outcome::Failed(format!("{} files are not read yet", path.extension().map(|e| e.to_string_lossy().to_uppercase()).unwrap_or_else(|| "these".into()))) }
    let turn = orientation(path);
    // the box is the picture as it is seen: a picture lying on its side in the file is asked for the other way round
    let (bw, bh) = if turn >= 5 { (want_h, want_w) } else { (want_w, want_h) };
    let full = match size_of(path) { Ok(d) => d, Err(e) => return Outcome::Failed(e) };
    let (mut rgba, mut w, mut h) = if still(path) {
        // a GIF or a WebP: its first frame
        match image::open(path) { Ok(i) => { let i = i.into_rgba8(); let (w, h) = i.dimensions(); (i.into_raw(), w, h) } Err(e) => return Outcome::Failed(e.to_string()) }
    } else {
        let reduce = full.0 > bw + bw / 2 && full.1 > bh + bh / 2;
        let img = if reduce { min_image::load_thumbnail(&p, "", bw.max(bh) * 2) } else { min_image::load(&p, "", 2) };
        match img { Ok(i) => rgba8(&i), Err(e) => return Outcome::Failed(e.to_string()) }
    };
    if w > bw && h > bh { let (tw, th) = fit(w, h, bw, bh); rgba = shrink(&rgba, w, h, tw, th); w = tw; h = th; }
    let whole = w >= full.0 && h >= full.1;
    if turn > 1 { (rgba, w, h) = oriented(&rgba, w, h, turn); }
    Outcome::Pic { rgba, w, h, whole }
}
/// A picture file's pixels for an export: straight RGBA, upright, its longer side at most `max`.
pub fn pixels(path: &Path, max: u32) -> Result<(Vec<u8>, u32, u32), String> {
    if preview_format(path).is_some() { return render(path, max).map(|r| (r.rgba, r.w, r.h)) }
    match decode(path, max, max) { Outcome::Pic { rgba, w, h, .. } => Ok((rgba, w, h)), Outcome::Missing => Err("The file was not found".into()), Outcome::Failed(e) => Err(e), Outcome::Still(..) => Err("not a picture".into()) }
}
/// A GIF or a WebP (read here, their first frame).
fn still(path: &Path) -> bool { matches!(path.extension().and_then(|e| e.to_str()).map(|e| e.to_ascii_lowercase()).as_deref(), Some("gif" | "webp")) }
/// A file that is shown as a picture today.
fn readable(path: &Path) -> bool { still(path) || min_image::Format::of(&path.to_string_lossy()).is_some() }
/// A picture file's size as its header gives it.
fn size_of(path: &Path) -> Result<(u32, u32), String> {
    if still(path) { image::image_dimensions(path).map_err(|e| e.to_string()) } else { min_image::dimensions(&path.to_string_lossy(), "").map_err(|e| e.to_string()) }
}
/// The size inside `bw` x `bh` that keeps the picture's shape (never larger than the picture).
fn fit(w: u32, h: u32, bw: u32, bh: u32) -> (u32, u32) {
    let k = (bw as f64 / w as f64).max(bh as f64 / h as f64).min(1.0);
    (((w as f64 * k).round() as u32).clamp(1, w), ((h as f64 * k).round() as u32).clamp(1, h))
}
fn half(b: u16) -> f32 {
    let (s, e, m) = ((b >> 15) & 1, (b >> 10) & 0x1f, (b & 0x3ff) as f32);
    let v = match e { 0 => m * 2f32.powi(-24), 31 => if m == 0.0 { f32::INFINITY } else { 0.0 }, _ => (1.0 + m / 1024.0) * 2f32.powi(e as i32 - 15) };
    if s == 1 { -v } else { v }
}
fn srgb(l: f32) -> u8 { let l = l.clamp(0.0, 1.0); let v = if l <= 0.003_130_8 { l * 12.92 } else { 1.055 * l.powf(1.0 / 2.4) - 0.055 }; (v * 255.0 + 0.5) as u8 }
/// A decoded picture's pixels for other parts of the app (a note's clean frame read back from its PNG).
pub fn rgba_of(img: &min_image::Image) -> (Vec<u8>, u32, u32) { rgba8(img) }
/// The decoder's planes (G, B, R [, A]) as straight-alpha RGBA, 8 bits.
fn rgba8(img: &min_image::Image) -> (Vec<u8>, u32, u32) {
    let (w, h, p) = (img.width as usize, img.height as usize, img.pitch as usize);
    let mut out = vec![255u8; w * h * 4];
    let a = img.alpha.then(|| &img.planes[3]);
    for y in 0..h {
        let (g, b, r) = (&img.planes[0][y * p..], &img.planes[1][y * p..], &img.planes[2][y * p..]);
        let row = &mut out[y * w * 4..(y + 1) * w * 4];
        match img.sample {
            min_image::Sample::U8 => for x in 0..w { row[x * 4] = r[x]; row[x * 4 + 1] = g[x]; row[x * 4 + 2] = b[x]; if let Some(a) = a { row[x * 4 + 3] = a[y * p + x]; } },
            min_image::Sample::U16 => for x in 0..w { row[x * 4] = r[x * 2 + 1]; row[x * 4 + 1] = g[x * 2 + 1]; row[x * 4 + 2] = b[x * 2 + 1]; if let Some(a) = a { row[x * 4 + 3] = a[y * p + x * 2 + 1]; } },
            // linear light, colour multiplied by its alpha (the format's rule): divided out, then the sRGB curve
            min_image::Sample::F16 => for x in 0..w {
                let f = |pl: &[u8]| half(u16::from_le_bytes([pl[x * 2], pl[x * 2 + 1]]));
                let al = a.map(|a| half(u16::from_le_bytes([a[y * p + x * 2], a[y * p + x * 2 + 1]])).clamp(0.0, 1.0)).unwrap_or(1.0);
                let un = |v: f32| if al > 0.0 && al < 1.0 { v / al } else { v };
                row[x * 4] = srgb(un(f(r))); row[x * 4 + 1] = srgb(un(f(g))); row[x * 4 + 2] = srgb(un(f(b))); row[x * 4 + 3] = (al * 255.0 + 0.5) as u8;
            },
        }
    }
    (out, w as u32, h as u32)
}
/// A smaller picture by averaging the pixels each new one covers (colour weighted by its alpha).
fn shrink(src: &[u8], w: u32, h: u32, tw: u32, th: u32) -> Vec<u8> {
    let (w, h, tw, th) = (w as usize, h as usize, tw as usize, th as usize);
    let mut out = vec![0u8; tw * th * 4];
    let xs: Vec<(usize, usize)> = (0..tw).map(|x| { let a = x * w / tw; (a, ((x + 1) * w / tw).max(a + 1).min(w)) }).collect();
    for y in 0..th {
        let (y0, y1) = { let a = y * h / th; (a, ((y + 1) * h / th).max(a + 1).min(h)) };
        for (x, (x0, x1)) in xs.iter().enumerate() {
            let (mut r, mut g, mut b, mut a) = (0u64, 0u64, 0u64, 0u64);
            for sy in y0..y1 { let row = &src[(sy * w + x0) * 4..(sy * w + x1) * 4]; for px in row.chunks_exact(4) { let al = px[3] as u64; r += px[0] as u64 * al; g += px[1] as u64 * al; b += px[2] as u64 * al; a += al; } }
            let n = ((y1 - y0) * (x1 - x0)) as u64; let o = &mut out[(y * tw + x) * 4..(y * tw + x) * 4 + 4];
            if a > 0 { o[0] = ((r + a / 2) / a) as u8; o[1] = ((g + a / 2) / a) as u8; o[2] = ((b + a / 2) / a) as u8; o[3] = ((a + n / 2) / n) as u8; }
        }
    }
    out
}
/// A JPEG's EXIF orientation (1..8; 1 when there is none, or the file is something else).
fn orientation(path: &Path) -> u8 {
    use std::io::Read;
    if !matches!(path.extension().and_then(|e| e.to_str()).map(|e| e.to_ascii_lowercase()).as_deref(), Some("jpg" | "jpeg")) { return 1 }
    let mut d = vec![0u8; 256 * 1024];
    let n = std::fs::File::open(path).and_then(|mut f| { let mut n = 0; loop { let k = f.read(&mut d[n..])?; if k == 0 { break } n += k; if n == d.len() { break } } Ok(n) }).unwrap_or(0);
    exif_orientation(&d[..n]).unwrap_or(1)
}
fn exif_orientation(d: &[u8]) -> Option<u8> {
    if d.get(..2)? != [0xff, 0xd8] { return None }
    let mut i = 2;
    loop {
        let (marker, len) = (*d.get(i + 1)?, u16::from_be_bytes([*d.get(i + 2)?, *d.get(i + 3)?]) as usize);
        if *d.get(i)? != 0xff || marker == 0xda || len < 2 { return None }
        let seg = d.get(i + 4..i + 2 + len)?;
        if marker == 0xe1 && seg.starts_with(b"Exif\0\0") {
            let t = &seg[6..]; let le = match t.get(..2)? { b"II" => true, b"MM" => false, _ => return None };
            let u16at = |o: usize| t.get(o..o + 2).map(|b| if le { u16::from_le_bytes([b[0], b[1]]) } else { u16::from_be_bytes([b[0], b[1]]) });
            let u32at = |o: usize| t.get(o..o + 4).map(|b| if le { u32::from_le_bytes([b[0], b[1], b[2], b[3]]) } else { u32::from_be_bytes([b[0], b[1], b[2], b[3]]) });
            let ifd = u32at(4)? as usize;
            for k in 0..u16at(ifd)? as usize { let e = ifd + 2 + k * 12; if u16at(e)? == 0x0112 { return u16at(e + 8).map(|v| v as u8).filter(|v| (1..=8).contains(v)) } }
            return None
        }
        i += 2 + len;
    }
}
/// The picture as its orientation says it is seen (EXIF 2..8).
fn oriented(src: &[u8], w: u32, h: u32, turn: u8) -> (Vec<u8>, u32, u32) {
    let (w, h) = (w as usize, h as usize);
    let (tw, th) = if turn >= 5 { (h, w) } else { (w, h) };
    let mut out = vec![0u8; tw * th * 4];
    for y in 0..th { for x in 0..tw {
        // the source pixel shown at (x, y)
        let (sx, sy) = match turn { 2 => (w - 1 - x, y), 3 => (w - 1 - x, h - 1 - y), 4 => (x, h - 1 - y), 5 => (y, x), 6 => (y, h - 1 - x), 7 => (w - 1 - y, h - 1 - x), 8 => (w - 1 - y, x), _ => (x, y) };
        out[(y * tw + x) * 4..(y * tw + x) * 4 + 4].copy_from_slice(&src[(sy * w + sx) * 4..(sy * w + sx) * 4 + 4]);
    } }
    (out, tw as u32, th as u32)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn a_picture_is_reduced_by_averaging_and_turned_as_its_orientation_says() {
        // 4 x 2: left half red, right half blue, the last pixel clear
        let mut src = vec![]; for _y in 0..2 { for x in 0..4 { src.extend_from_slice(if x < 2 { &[255, 0, 0, 255] } else { &[0, 0, 255, 255] }); } }
        src[7 * 4 + 3] = 0;
        let small = shrink(&src, 4, 2, 2, 1);
        assert_eq!(&small[..4], &[255, 0, 0, 255]);
        assert_eq!(&small[4..], &[0, 0, 255, 191], "a clear pixel thins the alpha, not the colour");
        assert_eq!(fit(4000, 3000, 800, 600), (800, 600)); assert_eq!(fit(100, 50, 800, 600), (100, 50));
        // 2 x 1 [a, b], orientation 6 (the camera was turned): seen as 1 x 2, a on top
        let (a, b) = ([1u8, 2, 3, 255], [4u8, 5, 6, 255]); let two = [a, b].concat();
        let (o, w, h) = oriented(&two, 2, 1, 6); assert_eq!((w, h), (1, 2)); assert_eq!(&o[..4], &a); assert_eq!(&o[4..], &b);
        let (o, _, _) = oriented(&two, 2, 1, 3); assert_eq!(&o[..4], &b);
        let (o, w, h) = oriented(&two, 2, 1, 8); assert_eq!((w, h), (1, 2)); assert_eq!(&o[..4], &b);
    }
    #[test]
    fn the_orientation_is_read_from_a_jpegs_exif_block() {
        let mut tiff = b"MM\0\x2a\0\0\0\x08\0\x01".to_vec(); tiff.extend_from_slice(&[0x01, 0x12, 0, 3, 0, 0, 0, 1, 0, 6, 0, 0, 0, 0, 0, 0]);
        let mut seg = b"Exif\0\0".to_vec(); seg.extend_from_slice(&tiff);
        let mut jpg = vec![0xff, 0xd8, 0xff, 0xe0, 0, 4, 0, 0, 0xff, 0xe1]; jpg.extend_from_slice(&((seg.len() + 2) as u16).to_be_bytes()); jpg.extend_from_slice(&seg);
        assert_eq!(exif_orientation(&jpg), Some(6));
        assert_eq!(exif_orientation(&[0xff, 0xd8, 0xff, 0xda, 0, 2]), None);
        assert_eq!(exif_orientation(b"not a jpeg"), None);
    }
    #[test]
    fn a_kept_file_is_looked_for_in_the_visible_folder_then_in_the_hidden_one_of_before_and_a_save_as_carries_both() {
        let d = std::env::temp_dir().join(format!("mn-side-{}", std::process::id())); let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("a/.minnotes")).unwrap(); std::fs::create_dir_all(d.join("a/minNotes Media/.qcview/c.mov")).unwrap(); std::fs::create_dir_all(d.join("staged")).unwrap();
        std::fs::write(d.join("a/.minnotes/old.png"), b"old").unwrap(); std::fs::write(d.join("a/minNotes Media/new.png"), b"new").unwrap(); std::fs::write(d.join("a/minNotes Media/.qcview/c.mov/notes.json"), b"{}").unwrap(); std::fs::write(d.join("staged/added.png"), b"added").unwrap();
        let places = Places::at(Some(d.join("a"))); let at = |n: &str| places.locate(&serde_json::json!(format!(".minnotes/{n}"))).unwrap();
        assert_eq!((at("old.png"), at("new.png"), at("none.png")), (d.join("a/.minnotes/old.png"), d.join("a/minNotes Media/new.png"), d.join("a/minNotes Media/none.png")));
        let side = d.join("b").join(SIDE); publish(&d.join("staged"), &side, &[d.join("a").join(OLD_SIDE), d.join("a").join(SIDE)]).unwrap();
        for f in ["old.png", "new.png", "added.png", ".qcview/c.mov/notes.json"] { assert!(side.join(f).is_file(), "{f}"); }
        let _ = std::fs::remove_dir_all(&d);
    }
    #[test]
    fn a_gif_gives_its_first_frame_and_an_unknown_format_says_so() {
        let dir = std::env::temp_dir().join(format!("mn-media-test-{}", std::process::id())); std::fs::create_dir_all(&dir).unwrap();
        // one red pixel
        let gif: [u8; 35] = [0x47, 0x49, 0x46, 0x38, 0x39, 0x61, 1, 0, 1, 0, 0x80, 0, 0, 0xff, 0, 0, 0, 0, 0, 0x2c, 0, 0, 0, 0, 1, 0, 1, 0, 0, 2, 2, 0x44, 1, 0, 0x3b];
        let (g, h) = (dir.join("dot.gif"), dir.join("photo.heic")); std::fs::write(&g, gif).unwrap(); std::fs::write(&h, b"not really").unwrap();
        assert!(readable(&g) && !readable(&h)); assert_eq!(size_of(&g), Ok((1, 1)));
        match decode(&g, 10, 10) { Outcome::Pic { rgba, w, h, whole } => { assert_eq!((w, h, whole), (1, 1, true)); assert_eq!(rgba, [255, 0, 0, 255]); } _ => panic!("the GIF was not read") }
        assert!(matches!(decode(&h, 10, 10), Outcome::Failed(why) if why == "HEIC files are not read yet"));
        assert!(matches!(decode(&dir.join("gone.png"), 10, 10), Outcome::Missing));
        let _ = std::fs::remove_dir_all(dir);
    }
    #[test]
    fn a_photoshop_file_and_an_svg_become_previews_kept_with_the_collection_and_the_original_is_rendered_again_when_it_changed() {
        let dir = std::env::temp_dir().join(format!("mn-preview-test-{}", std::process::id())); let _ = std::fs::remove_dir_all(&dir); std::fs::create_dir_all(&dir).unwrap();
        // SAFETY: the test's own process; the cache goes beside the test's files
        unsafe { std::env::set_var("MN_SESSIONS_DIR", dir.join("sessions")); }
        let places = Places { base: None, staged: Some(dir.join("media")), mappings: vec![] };
        let psd = dir.join("layout v4.psd"); std::fs::write(&psd, crate::psd::tests::file(false, true, 8, true)).unwrap();
        let rec: serde_json::Value = serde_json::from_str(&places.record(&psd).unwrap()).unwrap();
        assert_eq!((rec["w"].as_u64(), rec["h"].as_u64(), rec["name"].as_str(), rec["preview"]["format"].as_str()), (Some(4), Some(2), Some("layout v4.psd"), Some("PSD")));
        assert_eq!(rec["preview"]["src"].as_str(), psd.to_str()); assert!(rec["preview"]["size"].as_f64().unwrap() > 0.0);
        let stored = places.locate(&rec["src"]).unwrap(); assert!(stored.is_file() && stored.file_name().unwrap().to_string_lossy().starts_with("layout-v4-"));
        // shown: the stored picture is enough while the original is as it was
        let orig = |r: &serde_json::Value| Some((psd.clone(), r["preview"]["size"].as_f64().unwrap() as u64, r["preview"]["mtime"].as_f64().unwrap()));
        let run = |job: &Job| { let mut got = vec![]; work(job, &mut |o| { got.push(o); true }); got };
        let got = run(&Job { key: "k".into(), path: stored.clone(), w: 2, h: 1, orig: orig(&rec), page: None, still: None });
        assert!(matches!(got.as_slice(), [Outcome::Pic { w: 2, h: 1, .. }]));
        // the original changed (another size): the stored picture first, then the original's - and it is kept in the cache
        let mut other = crate::psd::tests::file(false, false, 8, true); let n = other.len(); other[n - 16..n - 8].fill(0); other.extend_from_slice(&[0; 2]); std::fs::write(&psd, &other).unwrap();
        let got = run(&Job { key: "k".into(), path: stored.clone(), w: 4, h: 2, orig: orig(&rec), page: None, still: None });
        assert_eq!(got.len(), 2, "the stored picture, then the original"); assert!(matches!(&got[1], Outcome::Pic { w: 4, h: 2, whole: true, .. }));
        assert_eq!(std::fs::read_dir(cache_dir()).unwrap().count(), 1);
        // an SVG: its own size in the record, a large drawing kept
        let svg = dir.join("mark.svg"); std::fs::write(&svg, r##"<svg xmlns="http://www.w3.org/2000/svg" width="20" height="10"><rect width="10" height="10" fill="#ff0000"/></svg>"##).unwrap();
        let rec: serde_json::Value = serde_json::from_str(&places.record(&svg).unwrap()).unwrap();
        assert_eq!((rec["w"].as_u64(), rec["h"].as_u64(), rec["preview"]["format"].as_str()), (Some(20), Some(10), Some("SVG")));
        let r = render(&svg, 200).unwrap(); assert_eq!((r.w, r.h), (200, 100)); assert_eq!(&r.rgba[..4], &[255, 0, 0, 255]); assert_eq!(r.rgba[(150 * 4) + 3], 0, "the right half is clear");
        // a file no decoder takes says so; the same goes into the waiting place only once
        assert!(places.record(&dir.join("nothing.psd")).is_err());
        let n = std::fs::read_dir(dir.join("media")).unwrap().count(); let _ = places.record(&svg).unwrap(); assert_eq!(std::fs::read_dir(dir.join("media")).unwrap().count(), n);
        unsafe { std::env::remove_var("MN_SESSIONS_DIR"); }
        let _ = std::fs::remove_dir_all(dir);
    }
    #[test]
    fn an_embedded_picture_is_kept_with_the_collection_and_what_is_not_a_picture_is_refused() {
        let dir = std::env::temp_dir().join(format!("mn-fetch-test-{}", std::process::id())); let _ = std::fs::remove_dir_all(&dir); std::fs::create_dir_all(&dir).unwrap();
        let places = Places { base: None, staged: Some(dir.join("media")), mappings: vec![] };
        // the one red pixel, as a browser embeds it
        let rec: serde_json::Value = serde_json::from_str(&fetched(&places, "data:image/gif;base64,R0lGODlhAQABAIAAAP8AAAAAACwAAAAAAQABAAACAkQBADs=").unwrap()).unwrap();
        assert_eq!((rec["w"].as_u64(), rec["h"].as_u64()), (Some(1), Some(1))); assert!(rec.get("preview").is_none());
        let kept = places.locate(&rec["src"]).unwrap(); assert!(kept.is_file() && kept.extension().unwrap() == "gif");
        assert!(matches!(decode(&kept, 4, 4), Outcome::Pic { w: 1, h: 1, .. }));
        assert!(fetched(&places, "data:image/png;base64,aGVsbG8=").is_err_and(|e| e.contains("not a picture")));
        assert!(remote("https://example.com/a.png") && remote("data:image/png;base64,xx") && !remote(".minnotes/a.png") && !remote("/Volumes/x/a.png"));
        assert_eq!(sniff(b"\x89PNG\r\n"), Some("png")); assert_eq!(sniff(b"<?xml version='1.0'?><svg xmlns='x'/>"), Some("svg")); assert_eq!(sniff(b"<html>"), None);
        let _ = std::fs::remove_dir_all(dir);
    }
    #[test]
    fn a_pdf_is_a_block_of_its_own_and_an_illustrator_file_a_flat_preview() {
        let dir = std::env::temp_dir().join(format!("mn-pdfrec-test-{}", std::process::id())); let _ = std::fs::remove_dir_all(&dir); std::fs::create_dir_all(&dir).unwrap();
        let places = Places { base: None, staged: Some(dir.join("media")), mappings: vec![] };
        let (pdf, ai) = (dir.join("deck.pdf"), dir.join("icon.ai")); std::fs::write(&pdf, crate::pdf::tests::sample()).unwrap(); std::fs::write(&ai, crate::pdf::tests::sample()).unwrap();
        let rec: serde_json::Value = serde_json::from_str(&places.record(&pdf).unwrap()).unwrap();
        assert_eq!((rec["kind"].as_str(), rec["pages"].as_u64(), rec["w"].as_u64(), rec["h"].as_u64(), rec["src"].as_str()), (Some("pdf"), Some(1), Some(200), Some(100), pdf.to_str()));
        assert!(!dir.join("media").exists(), "a PDF is referenced where it is: nothing is copied");
        // its page through the worker: drawn at the width asked, and its characters kept for selection
        let mut got = vec![]; work(&Job { key: "pdf-test-key#0".into(), path: pdf.clone(), w: 300, h: 150, orig: None, page: Some(0), still: None }, &mut |o| { got.push(o); true });
        assert!(matches!(got.as_slice(), [Outcome::Pic { w: 300, h: 150, whole: false, .. }]));
        assert_eq!(TEXTS.lock().unwrap().get("pdf-test-key#0").map(|t| t.words(0, 9)), Some("Hello page".to_string()));
        let rec: serde_json::Value = serde_json::from_str(&places.record(&ai).unwrap()).unwrap();
        assert_eq!((rec["preview"]["format"].as_str(), rec["w"].as_u64(), rec["h"].as_u64(), rec.get("kind")), (Some("AI"), Some(2048), Some(1024), None));
        assert!(places.locate(&rec["src"]).unwrap().is_file());
        let _ = std::fs::remove_dir_all(dir);
    }
    /// Run by hand with an address in `MN_TEST_URL` (`cargo test -p mn --release -- --ignored from_the_web --nocapture`).
    #[test]
    #[ignore]
    fn a_picture_is_fetched_from_the_web() {
        let Ok(url) = std::env::var("MN_TEST_URL") else { return };
        let dir = std::env::temp_dir().join(format!("mn-web-test-{}", std::process::id())); std::fs::create_dir_all(&dir).unwrap();
        let rec = fetched(&Places { base: None, staged: Some(dir.join("media")), mappings: vec![] }, &url); println!("{rec:?}");
        assert!(rec.is_ok()); let _ = std::fs::remove_dir_all(dir);
    }
    #[cfg(target_os = "macos")]
    #[test]
    fn the_system_decodes_a_heic() {
        // made by the system's own tool from a PNG written here; where the tool is missing the test has nothing to say
        let dir = std::env::temp_dir().join(format!("mn-heic-test-{}", std::process::id())); std::fs::create_dir_all(&dir).unwrap();
        let (png, heic) = (dir.join("a.png"), dir.join("a.heic")); let rgb: Vec<u8> = (0..64 * 32).flat_map(|i| if i % 64 < 32 { [220u8, 30, 30] } else { [30, 30, 220] }).collect();
        min_image::write::png8(&png.to_string_lossy(), &rgb, 64, 32).unwrap();
        let made = std::process::Command::new("sips").args(["-s", "format", "heic"]).arg(&png).arg("--out").arg(&heic).output().is_ok_and(|o| o.status.success()) && heic.is_file();
        if made { let r = render(&heic, 32).unwrap(); assert_eq!((r.w, r.h), (32, 16)); let (l, rt) = (&r.rgba[(8 * 32 + 4) * 4..][..3], &r.rgba[(8 * 32 + 28) * 4..][..3]); assert!(l[0] > 150 && l[2] < 100 && rt[2] > 150 && rt[0] < 100, "{l:?} {rt:?}"); }
        let _ = std::fs::remove_dir_all(dir);
    }
    #[test]
    fn linear_light_gets_the_srgb_curve_and_halves_decode() {
        assert_eq!(half(0x3c00), 1.0); assert_eq!(half(0x0000), 0.0); assert_eq!(half(0x3800), 0.5); assert_eq!(half(0xc000), -2.0);
        assert_eq!(srgb(0.0), 0); assert_eq!(srgb(1.0), 255); assert_eq!(srgb(0.18), 118); assert_eq!(srgb(7.0), 255);
    }
}
