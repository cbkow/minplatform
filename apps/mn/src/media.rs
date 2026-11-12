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
    Ready { brush: &'a peniko::ImageBrush, w: u32, h: u32, fade: f32 },
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

struct Job { key: String, path: PathBuf, w: u32, h: u32 }
enum Outcome { Pic { rgba: Vec<u8>, w: u32, h: u32, whole: bool }, Missing, Failed(String) }
struct Queue { jobs: Mutex<VecDeque<Job>>, more: Condvar }

/// Where files are: the collection's folder, this session's waiting place for added files, the path mappings.
#[derive(Clone, Default)]
pub struct Places { base: Option<PathBuf>, staged: Option<PathBuf>, mappings: Vec<min_paths::map::Mapping> }

/// What came back from file work asked of the helpers.
pub enum Mail {
    /// Dropped / chosen files as block records, in order; what could not be taken, in words.
    Added { token: u64, records: Vec<String>, refused: Vec<String> },
    /// Pictures pointed at their files again: each block with its new record.
    Relinked { records: Vec<(mn_doc::BlockId, String)>, problem: Option<String> },
}

pub struct Media {
    entries: HashMap<String, Entry>, queue: Arc<Queue>, done: mpsc::Receiver<(String, u32, Outcome)>,
    places: Places, budget: usize, total: usize, frame: u64,
    mail_tx: mpsc::Sender<Mail>, mail: mpsc::Receiver<Mail>, wake: Arc<dyn Fn() + Send + Sync>,
}

impl Media {
    /// `wake` is called from a worker when a picture is ready (it must make the window poll and redraw).
    pub fn new(settings: &Path, wake: impl Fn() + Send + Sync + 'static) -> Media {
        let queue = Arc::new(Queue { jobs: Mutex::new(VecDeque::new()), more: Condvar::new() });
        let (tx, done) = mpsc::channel(); let wake: Arc<dyn Fn() + Send + Sync> = Arc::new(wake); let (mail_tx, mail) = mpsc::channel();
        let n = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(2).clamp(1, 4);
        for i in 0..n {
            let (q, tx, wake) = (queue.clone(), tx.clone(), wake.clone());
            let _ = std::thread::Builder::new().name(format!("mn-media-{i}")).spawn(move || loop {
                // the newest request first: it is what is on screen now
                let job = { let mut j = q.jobs.lock().unwrap(); loop { if let Some(x) = j.pop_back() { break x } j = q.more.wait(j).unwrap(); } };
                let out = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| decode(&job.path, job.w, job.h))).unwrap_or_else(|_| Outcome::Failed("the decoder stopped".into()));
                if tx.send((job.key, job.w, out)).is_err() { return }
                wake();
            });
        }
        let mappings = std::fs::read_to_string(settings.join("mappings.json")).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
        let budget = std::env::var("MN_MEDIA_BUDGET_MB").ok().and_then(|v| v.parse::<usize>().ok()).map(|mb| mb << 20).unwrap_or(BUDGET);
        Media { entries: HashMap::new(), queue, done, places: Places { base: None, staged: None, mappings }, budget, total: 0, frame: 0, mail_tx, mail, wake }
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
                match places.picture(&to) {
                    Ok(serde_json::Value::Object(new)) => { for k in ["src", "w", "h", "name"] { if let Some(x) = new.get(k) { v.insert(k.to_string(), x.clone()); } } v.remove("preview"); records.push((id, serde_json::Value::Object(v).to_string())); }
                    Ok(_) => {}
                    Err(why) => if id == chosen { problem = Some(why) },
                }
            }
            let _ = tx.send(Mail::Relinked { records, problem }); wake();
        });
    }
    /// What the helpers sent back.
    pub fn mail(&mut self) -> Vec<Mail> { self.mail.try_iter().collect() }
    /// A picture's record names a file that is not there (or cannot be looked for).
    pub fn is_missing(&self, descriptor: &str) -> bool {
        let v: serde_json::Value = serde_json::from_str(descriptor).unwrap_or_default();
        v.get("kind").is_none() && v.get("src").is_some_and(|s| matches!(self.entries.get(&s.to_string()).map(|e| &e.state), Some(State::Missing(_))))
    }
    /// A frame begins (what is shown in it is kept when the budget is met).
    pub fn begin_frame(&mut self) { self.frame += 1; }
    /// Takes what the workers finished. True: something arrived, draw again.
    pub fn poll(&mut self) -> bool {
        let mut any = false;
        while let Ok((key, w, out)) = self.done.try_recv() {
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
        if v.get("kind").is_some() { return View::Other }
        let Some(src) = v.get("src") else { return View::Other };
        let key = src.to_string();
        let (w, h) = { let (w, h) = (w.ceil().max(1.0), h.ceil().max(1.0)); let k = (LONGEST as f64 / w.max(h)).min(1.0); (((w * k) as u32).max(1), ((h * k) as u32).max(1)) };
        if !self.entries.contains_key(&key) {
            let name = v.get("name").and_then(|n| n.as_str()).map(str::to_string);
            let (state, name, asked) = match self.places.locate(src) {
                Ok(path) => { let name = name.unwrap_or_else(|| path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()); self.ask(&key, path, w, h); (State::Loading, name, w) }
                Err(why) => (State::Missing(why), name.unwrap_or_default(), 0),
            };
            self.entries.insert(key.clone(), Entry { state, name, asked, whole: false, used: self.frame });
        }
        // shown larger than what is held (a wider window, a denser screen): the same file again, larger
        let again = { let e = &self.entries[&key]; matches!(&e.state, State::Ready(_)) && !e.whole && w > e.asked + e.asked / 8 };
        if again { if let Ok(path) = self.places.locate(src) { self.ask(&key, path, w, h); self.entries.get_mut(&key).unwrap().asked = w; } }
        let looked = match &self.entries[&key].state { State::Missing(why) if why.is_empty() => self.places.locate(src).map(|p| p.to_string_lossy().into_owned()).unwrap_or_default(), State::Missing(why) => why.clone(), _ => String::new() };
        let e = self.entries.get_mut(&key).unwrap(); e.used = self.frame;
        match &e.state {
            State::Loading => View::Loading,
            State::Ready(p) => View::Ready { brush: &p.brush, w: p.w, h: p.h, fade: (p.arrived.elapsed().as_millis() as f32 / FADE_MS as f32).min(1.0) },
            State::Missing(_) => View::Missing { name: e.name.clone(), looked },
            State::Failed(why) => View::Failed { name: e.name.clone(), why: why.clone() },
        }
    }
    fn ask(&self, key: &str, path: PathBuf, w: u32, h: u32) {
        let mut j = self.queue.jobs.lock().unwrap(); j.retain(|x| x.key != key); j.push_back(Job { key: key.to_string(), path, w, h }); self.queue.more.notify_one();
    }
    /// Bytes of decoded pictures held, and how many (for a scripted look).
    pub fn held(&self) -> (usize, usize) { (self.total, self.entries.values().filter(|e| matches!(e.state, State::Ready(_))).count()) }
}

impl Places {
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
                (_, Some(b)) => Ok(b.join(s)), _ => Err("the collection has no folder yet".into()) },
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
        let name = format!("{}-{:08x}.{ext}", if stem.is_empty() { "picture" } else { &stem }, (hash >> 32) as u32 ^ hash as u32);
        let at = dir.join(&name);
        // the same content under the same name is there already (in this session, or beside the document)
        let there = at.is_file() || self.base.as_ref().is_some_and(|b| b.join(".minnotes").join(&name).is_file());
        if there { let _ = std::fs::remove_file(&tmp); } else { std::fs::rename(&tmp, &at).map_err(|e| e.to_string())?; }
        Ok(serde_json::Value::String(format!(".minnotes/{name}")))
    }
    /// A picture file as a block record: its size as it is SEEN (a JPEG lying on its side is measured upright).
    fn picture(&self, path: &Path) -> Result<serde_json::Value, String> {
        let p = path.to_string_lossy();
        let (w, h) = min_image::dimensions(&p, "").map_err(|e| e.to_string())?;
        let (w, h) = if orientation(path) >= 5 { (h, w) } else { (w, h) };
        Ok(serde_json::json!({ "src": self.keep(path)?, "w": w, "h": h, "name": path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default() }))
    }
    /// Any dropped file as a block record: a picture `min-image` reads is a picture; anything else is a file chip,
    /// referenced where it is (videos, PDFs and the preview formats get records of their own with their steps).
    fn record(&self, path: &Path) -> Result<String, String> {
        if path.is_dir() { return Err("folders are not taken yet".into()) }
        if !path.is_file() { return Err("it is not there".into()) }
        if min_image::Format::of(&path.to_string_lossy()).is_some() { return self.picture(path).map(|v| v.to_string()) }
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        Ok(serde_json::json!({ "kind": "file", "src": self.reference(path), "name": name, "ext": path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default() }).to_string())
    }
}

/// A save's first half (runs on the save's helper thread): the files added in this session go beside the document,
/// into `side` (its `.minnotes/`), each written under a temporary name and renamed; one already there with the same
/// size is left. `also`: another folder of such files to carry along (Save As to a new place).
pub fn publish(staged: &Path, side: &Path, also: Option<&Path>) -> std::io::Result<()> {
    for from in [Some(staged), also].into_iter().flatten() {
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
    }
    Ok(())
}

// ── the worker's side

fn decode(path: &Path, want_w: u32, want_h: u32) -> Outcome {
    if !path.is_file() { return Outcome::Missing }
    let p = path.to_string_lossy();
    if min_image::Format::of(&p).is_none() { return Outcome::Failed(format!("{} files are not read yet", path.extension().map(|e| e.to_string_lossy().to_uppercase()).unwrap_or_else(|| "these".into()))) }
    let turn = orientation(path);
    // the box is the picture as it is seen: a picture lying on its side in the file is asked for the other way round
    let (bw, bh) = if turn >= 5 { (want_h, want_w) } else { (want_w, want_h) };
    let full = match min_image::dimensions(&p, "") { Ok(d) => d, Err(e) => return Outcome::Failed(e.to_string()) };
    let reduce = full.0 > bw + bw / 2 && full.1 > bh + bh / 2;
    let img = if reduce { min_image::load_thumbnail(&p, "", bw.max(bh) * 2) } else { min_image::load(&p, "", 2) };
    let img = match img { Ok(i) => i, Err(e) => return Outcome::Failed(e.to_string()) };
    let (mut rgba, mut w, mut h) = rgba8(&img);
    drop(img);
    if w > bw && h > bh { let (tw, th) = fit(w, h, bw, bh); rgba = shrink(&rgba, w, h, tw, th); w = tw; h = th; }
    let whole = w >= full.0 && h >= full.1;
    if turn > 1 { (rgba, w, h) = oriented(&rgba, w, h, turn); }
    Outcome::Pic { rgba, w, h, whole }
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
    fn linear_light_gets_the_srgb_curve_and_halves_decode() {
        assert_eq!(half(0x3c00), 1.0); assert_eq!(half(0x0000), 0.0); assert_eq!(half(0x3800), 0.5); assert_eq!(half(0xc000), -2.0);
        assert_eq!(srgb(0.0), 0); assert_eq!(srgb(1.0), 255); assert_eq!(srgb(0.18), 118); assert_eq!(srgb(7.0), 255);
    }
}
