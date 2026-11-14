//! Notes written out as a WEB FOLDER (canon row 339): pages a browser opens from a shared drive or a web server - one
//! per note and per canvas, one stylesheet and one script for all of them, the pictures as files. It is what a whole
//! collection, or some of its notes, is exported as; one note alone is the same folder with one page.
//!
//! ```text
//! index.html            the one note; or, for several, the contents
//! notes/<name>.html     a page per note and per canvas (several only)
//! assets/site.css       the look (fonts in it), canvas.css, site.js (the list of notes, the search), data.js (what they show)
//! media/<hash>.<ext>    every picture, once however often it is shown
//! media/files/<name>    PDFs and attached files, when they are taken along
//! media/video/<hash>.mp4 a playable copy of each video (row 340), kept from one export to the next
//! ```
//!
//! Rules it keeps:
//! - It opens from disk by a double click: no page loads data at run time; the list and the search's text are a
//!   script (`data.js`) every page includes.
//! - A page's content goes through ONE step ([`page`]) on its way into its shell: with a password (row 342) it is
//!   sealed there, and nothing else of the layout changes.
//! - A page names its media by a path relative to itself; nothing else knows where media lives.
//! - Only a folder that is empty, or carries our marker, is written into; of an earlier export only what we wrote is
//!   removed.
//!
//! The pages' bodies are the HTML export's own (`html_blocks`, `canvas_body`): their pictures come embedded and are
//! moved out to files here ([`externalise`]).
use super::{canvas::{canvas_body, CANVAS_CSS}, esc, html_blocks, Col, Item, Mode, Opts, Source, Video, Walk, What, COPY_SCRIPT, CSS, LIGHTBOX, PRINT};
use mn_doc::{BlockId, Doc};
use std::{collections::{HashMap, HashSet}, path::{Path, PathBuf}, sync::{atomic::{AtomicBool, AtomicU64, AtomicU8, Ordering::Relaxed}, Mutex}};

pub const MARKER: &str = ".minnotes-export";
pub const QUEUED: u8 = 0; pub const WORKING: u8 = 1; pub const DONE: u8 = 2; /// a video: its copy of an earlier export is still good
pub const KEPT: u8 = 3; pub const FAILED: u8 = 4; /// a video that stays stills (why is said)
pub const SKIPPED: u8 = 5;

pub enum Kind { /// a folder, or an entry that is only on the way to an exported one
    Folder, Note(Doc), /// its elements from the back to the front, each frame's members after it
    Canvas(Vec<mn_store::Element>) }
pub struct Page { pub id: BlockId, pub parent: Option<BlockId>, pub title: String, pub kind: Kind }
pub struct Job {
    /// in the collection's order; every exported page's ancestors are among them
    pub pages: Vec<Page>, pub dest: PathBuf, pub opts: Opts,
    /// PDFs and attached files are copied in and linked
    pub files: bool,
    /// videos get a playable copy and a player (row 340)
    pub videos: bool,
    /// the pages' content is sealed with it (row 342); None or empty: no password
    pub password: Option<String>,
    /// the collection's name (the list's head, the contents page) and when this was made
    pub name: String, pub when: String,
    /// the notes canvases show as cards: their names and blocks
    pub cards: HashMap<BlockId, (String, Doc)>,
}
impl Job { pub fn exported(&self) -> Vec<usize> { (0..self.pages.len()).filter(|i| !matches!(self.pages[*i].kind, Kind::Folder)).collect() } }

/// What the window watches while a folder is written (one state per page of [`Job::pages`]).
pub struct Progress { pub states: Vec<AtomicU8>, pub stop: AtomicBool, pub over: AtomicBool, pub error: Mutex<Option<String>>, pub media: AtomicU64, pub bytes: AtomicU64, /// the videos, as they are met in the pages
    pub videos: Mutex<Vec<VideoRow>> }
#[derive(Clone, Debug)]
pub struct VideoRow { pub name: String, pub state: u8, pub done: f32, pub why: String }

/// A video as it can be made playable: its rate, its frames, whether its file plays in a browser as it is (it is
/// copied), and what tells one state of the file from another (its size and time: a copy made of the same is kept).
#[derive(Clone, Debug)]
pub struct Clip { pub fps: f64, pub frames: i64, pub copy: bool, pub key: String }
/// The application's side of videos: what a file is, and its playable copy made (H.264 / AAC in MP4).
pub trait Videos {
    /// Err: why this video stays stills.
    fn plan(&mut self, path: &Path) -> Result<Clip, String>;
    fn make(&mut self, path: &Path, clip: &Clip, out: &Path, stop: &AtomicBool, done: &dyn Fn(f32)) -> Result<(), String>;
}
/// What changes a playable copy when it changes (a copy of an earlier export made otherwise is made again).
const RECIPE: &str = "h264-1920-v2";
impl Progress {
    pub fn new(pages: usize) -> Progress { Progress { states: (0..pages).map(|_| AtomicU8::new(QUEUED)).collect(), stop: AtomicBool::new(false), over: AtomicBool::new(false), error: Mutex::new(None), media: AtomicU64::new(0), bytes: AtomicU64::new(0), videos: Mutex::new(vec![]) } }
    #[cfg(test)] pub fn count(&self, state: u8) -> usize { self.states.iter().filter(|s| s.load(Relaxed) == state).count() }
}

/// What a folder chosen as the destination is.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Dest { /// not there yet, or empty
    Free, /// an earlier web folder of ours: it is replaced
    Ours, /// something else: left alone
    Other }
pub fn dest_state(dir: &Path) -> Dest {
    if dir.is_file() { return Dest::Other }
    let Ok(list) = std::fs::read_dir(dir) else { return Dest::Free };
    let names: Vec<String> = list.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).filter(|n| n != ".DS_Store").collect();
    if names.is_empty() { Dest::Free } else if names.iter().any(|n| n == MARKER) { Dest::Ours } else { Dest::Other }
}
/// The destination made ready: what an earlier export of ours left is removed (its pages, its folders - nothing else),
/// the marker is written by the caller.
fn prepare(dir: &Path) -> Result<(), String> {
    match dest_state(dir) {
        Dest::Other => return Err("That folder holds other things: choose an empty folder, or an earlier web folder to replace".into()),
        Dest::Ours => { let gone = |p: PathBuf| if p.is_dir() { std::fs::remove_dir_all(&p) } else if p.exists() { std::fs::remove_file(&p) } else { Ok(()) }.map_err(|e| format!("The earlier export could not be replaced ({e})"));
            for d in ["assets", "notes", "index.html"] { gone(dir.join(d))?; }
            // (the videos' copies stay: one made of the same file the same way is not made again)
            if let Ok(list) = std::fs::read_dir(dir.join("media")) { for e in list.flatten() { if e.file_name() != "video" { gone(e.path())?; } } } }
        Dest::Free => {}
    }
    std::fs::create_dir_all(dir.join("assets")).map_err(|e| format!("The folder could not be made ({e})"))
}

/// A page's file name from its title: letters, digits and dashes, one of a kind.
fn slug(title: &str, taken: &mut HashSet<String>) -> String {
    let mut s = String::new(); for c in title.trim().chars() { if c.is_ascii_alphanumeric() { s.push(c.to_ascii_lowercase()); } else if !s.ends_with('-') && !s.is_empty() { s.push('-'); } if s.len() >= 60 { break } }
    let s = s.trim_end_matches('-'); let s = if s.is_empty() { "page" } else { s };
    let mut name = s.to_string(); let mut n = 2; while !taken.insert(name.clone()) { name = format!("{s}-{n}"); n += 1; }
    name
}
fn url(name: &str) -> String { name.bytes().map(|b| if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') { (b as char).to_string() } else { format!("%{b:02X}") }).collect() }
/// A note's words, for the search.
fn words(doc: &Doc) -> String { let mut s = String::new(); for b in doc.walk() { if b.kind.is_text() && !b.text.trim().is_empty() { if !s.is_empty() { s.push('\n'); } s.push_str(b.text.trim()); } } s }

/// The pictures a body carries in itself (`src="data:…;base64,…"`) written to `media/` - each once - and named there.
fn externalise(html: &str, dest: &Path, pre: &str, seen: &mut HashSet<String>, lock: Option<&Sealer>, p: &Progress) -> Result<String, String> {
    use base64::Engine; use std::hash::{Hash, Hasher};
    let mut out = String::with_capacity(html.len() / 4 + 1024); let mut rest = html;
    while let Some(i) = rest.find("src=\"data:") {
        out.push_str(&rest[..i + 5]); let after = &rest[i + 5..]; let end = after.find('"').unwrap_or(after.len()); let uri = &after[..end];
        match uri.split_once(";base64,") {
            Some((head, data)) => {
                let ext = match &head[5..] { "image/png" => "png", "image/jpeg" => "jpg", "image/gif" => "gif", "image/webp" => "webp", "image/svg+xml" => "svg", _ => "bin" };
                let mut h = std::collections::hash_map::DefaultHasher::new(); data.hash(&mut h); let name = format!("{:016x}-{:x}.{ext}", h.finish(), data.len()); let name = match lock { Some(l) => format!("{}.{ext}", l.name(&format!("m:{name}"))), None => name };
                if seen.insert(name.clone()) {
                    let bytes = base64::engine::general_purpose::STANDARD.decode(data).map_err(|e| format!("A picture could not be written ({e})"))?;
                    std::fs::create_dir_all(dest.join("media")).and_then(|_| std::fs::write(dest.join("media").join(&name), &bytes)).map_err(|e| format!("A picture could not be written ({e})"))?;
                    p.media.fetch_add(1, Relaxed); p.bytes.fetch_add(bytes.len() as u64, Relaxed);
                }
                out.push_str(pre); out.push_str("media/"); out.push_str(&name);
            }
            None => out.push_str(uri),
        }
        rest = &after[end..];
    }
    out.push_str(rest); Ok(out)
}

/// The application's source, with PDFs and attached files taken along: each copied to `media/files/` once and linked.
struct Linked<'a, 'v> { inner: &'a mut dyn Source, dest: &'a Path, pre: &'a str, on: bool, done: &'a mut HashMap<String, Option<String>>, taken: &'a mut HashSet<String>, p: &'a Progress, lock: Option<&'a Sealer>,
    /// the videos' side, and what is to be made: (the file, what it is, the name that says what it was made of, its name in `media/video/`, its row)
    videos: Option<&'a mut (dyn Videos + 'v)>, plans: &'a mut HashMap<String, Option<(Clip, String)>>, queue: &'a mut Vec<(PathBuf, Clip, String, String, usize)> }
impl Source for Linked<'_, '_> {
    fn item(&mut self, descriptor: &str, mode: Mode, opts: Opts) -> Item {
        let mut it = self.inner.item(descriptor, mode, opts);
        if let (What::Video, Some(v), false) = (it.what, self.videos.as_deref_mut(), it.path.is_empty()) {
            let from = PathBuf::from(&it.path);
            if !self.plans.contains_key(&it.path) {
                let plan = if from.is_file() { v.plan(&from) } else { Err("The file was not found".into()) };
                let mut rows = self.p.videos.lock().unwrap();
                let got = match plan {
                    Ok(clip) => { use std::hash::{Hash, Hasher}; let mut h = std::collections::hash_map::DefaultHasher::new(); (&it.path, &clip.key, RECIPE, clip.copy).hash(&mut h); let plain = format!("{:016x}.mp4", h.finish()); let name = match self.lock { Some(l) => format!("{}.mp4", l.name(&format!("v:{plain}"))), None => plain.clone() };
                        rows.push(VideoRow { name: it.name.clone(), state: QUEUED, done: 0.0, why: String::new() }); self.queue.push((from, clip.clone(), plain, name.clone(), rows.len() - 1)); Some((clip, name)) }
                    Err(why) => { rows.push(VideoRow { name: it.name.clone(), state: SKIPPED, done: 0.0, why }); None } };
                self.plans.insert(it.path.clone(), got);
            }
            if let Some(Some((clip, name))) = self.plans.get(&it.path) { it.video = Some(Video { src: format!("{}media/video/{name}", self.pre), fps: clip.fps, frames: clip.frames }); }
            return it
        }
        if !self.on || !matches!(it.what, What::Pdf | What::File) || it.path.is_empty() { return it }
        let from = PathBuf::from(&it.path);
        let rel = match self.done.get(&it.path) { Some(r) => r.clone(), None => {
            // (with a password each file has a folder of its own under a name nobody can work out; the file keeps its name in it)
            let r = from.is_file().then(|| { let (name, sub) = match self.lock { Some(l) => (it.name.clone(), Some(l.name(&format!("f:{}", it.path)))), None => (crate::gather::unique(&it.name, self.taken), None) };
                let dir = match &sub { Some(d) => self.dest.join("media").join("files").join(d), None => self.dest.join("media").join("files") };
                std::fs::create_dir_all(&dir).and_then(|_| { if sub.is_some() { std::fs::write(dir.join("index.html"), "<!DOCTYPE html><title></title>\n")?; } std::fs::copy(&from, dir.join(&name)) }).ok().map(|n| { self.p.media.fetch_add(1, Relaxed); self.p.bytes.fetch_add(n, Relaxed); match sub { Some(d) => format!("{d}/{}", url(&name)), None => url(&name) } }) }).flatten();
            self.done.insert(it.path.clone(), r.clone()); r } };
        if let Some(name) = rel { it.href = Some(format!("{}media/files/{name}", self.pre)); }
        it
    }
}

/// A password's work (row 342): the key made of it, every page's content and the list sealed with that key, and the
/// names media goes under - which only the sealed pages know. AES-256-GCM; the key is PBKDF2-HMAC-SHA-256 of the
/// password, [`ROUNDS`] rounds, over a salt that stays with the folder (so that the same password names media the
/// same way at the next export, and a video made once is kept).
pub struct Sealer { key: [u8; 32], names: ring::hmac::Key, salt: [u8; 16] }
pub const ROUNDS: u32 = 600_000;
fn hex(b: &[u8]) -> String { b.iter().map(|x| format!("{x:02x}")).collect() }
fn unhex(s: &str) -> Option<Vec<u8>> { if s.len() % 2 != 0 { return None } (0..s.len() / 2).map(|i| u8::from_str_radix(s.get(i * 2..i * 2 + 2)?, 16).ok()).collect() }
impl Sealer {
    pub fn new(password: &str, salt: Option<[u8; 16]>) -> Result<Sealer, String> {
        use ring::rand::SecureRandom;
        let salt = match salt { Some(s) => s, None => { let mut s = [0u8; 16]; ring::rand::SystemRandom::new().fill(&mut s).map_err(|_| "No random numbers could be had")?; s } };
        let mut key = [0u8; 32]; ring::pbkdf2::derive(ring::pbkdf2::PBKDF2_HMAC_SHA256, std::num::NonZeroU32::new(ROUNDS).unwrap(), &salt, password.as_bytes(), &mut key);
        let names = ring::hmac::Key::new(ring::hmac::HMAC_SHA256, ring::hmac::sign(&ring::hmac::Key::new(ring::hmac::HMAC_SHA256, &key), b"names").as_ref());
        Ok(Sealer { key, names, salt })
    }
    /// `nonce | ciphertext | tag`, in base64.
    pub fn seal(&self, plain: &[u8]) -> Result<String, String> {
        use base64::Engine; use ring::{aead, rand::SecureRandom};
        let mut iv = [0u8; 12]; ring::rand::SystemRandom::new().fill(&mut iv).map_err(|_| "No random numbers could be had")?;
        let k = aead::LessSafeKey::new(aead::UnboundKey::new(&aead::AES_256_GCM, &self.key).map_err(|_| "The key could not be made")?);
        let mut data = plain.to_vec(); k.seal_in_place_append_tag(aead::Nonce::assume_unique_for_key(iv), aead::Aad::empty(), &mut data).map_err(|_| "A page could not be sealed")?;
        let mut all = iv.to_vec(); all.extend(data); Ok(base64::engine::general_purpose::STANDARD.encode(all))
    }
    /// A name nobody can work out without the password (32 hex digits), for what `what` names.
    pub fn name(&self, what: &str) -> String { hex(&ring::hmac::sign(&self.names, what.as_bytes()).as_ref()[..16]) }
}
/// What the marker file remembers of the export before: the salt, and each video's copy by what it was made of.
fn marker_read(dir: &Path) -> (Option<[u8; 16]>, HashMap<String, String>) {
    let text = std::fs::read_to_string(dir.join(MARKER)).unwrap_or_default(); let (mut salt, mut videos) = (None, HashMap::new());
    for l in text.lines() { let mut w = l.split(' '); match (w.next(), w.next(), w.next()) { (Some("salt"), Some(h), _) => salt = unhex(h).and_then(|b| b.try_into().ok()), (Some("video"), Some(a), Some(b)) => { videos.insert(a.to_string(), b.to_string()); } _ => {} } }
    (salt, videos)
}
fn marker_write(dir: &Path, salt: Option<[u8; 16]>, videos: &HashMap<String, String>) -> std::io::Result<()> {
    let mut t = String::from("A web folder written by minNotes. Exporting to this folder again replaces index.html, assets, notes and media.\n");
    if let Some(s) = salt { t += &format!("salt {}\n", hex(&s)); } for (a, b) in videos { t += &format!("video {a} {b}\n"); }
    std::fs::write(dir.join(MARKER), t)
}

fn head(title: &str, pre: &str, canvas: bool, nav: bool) -> String {
    format!("<!DOCTYPE html>\n<html lang=\"en\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\"><meta name=\"generator\" content=\"minNotes\">\n<title>{}</title>\n<link rel=\"stylesheet\" href=\"{pre}assets/site.css\">{}\n{}</head>",
        esc(title), if canvas { format!("<link rel=\"stylesheet\" href=\"{pre}assets/canvas.css\">") } else { String::new() },
        if nav { "<script>(function(){var w=innerWidth>900,v=null;try{v=localStorage.getItem('mn.nav')}catch(e){}if(v==='1'||(v!=='0'&&w))document.documentElement.classList.add('nav')})()</script>\n" } else { "" })
}
/// A whole page: its shell around its content. `scripts`: the folder's scripts it needs, from the folder's top
/// (`assets/video.js`, `assets/site.js`). This is the ONE step content passes on its way out: with a password the
/// shell holds a password box and the content sealed - its title, its HTML and its scripts' names - which `lock.js`
/// opens; without one the content stands in the page as it is.
#[allow(clippy::too_many_arguments)]
fn page(title: &str, pre: &str, canvas: bool, nav: bool, id: &str, body: &str, scripts: &[&str], lock: Option<&Sealer>) -> Result<String, String> {
    let class = if canvas { " cv" } else { "" };
    Ok(match lock {
        None => { let mut tags: String = scripts.iter().filter(|s| **s != "assets/site.js").map(|s| format!("<script src=\"{pre}{s}\"></script>\n")).collect();
            if nav { tags += &format!("<script src=\"{pre}assets/data.js\"></script><script src=\"{pre}assets/site.js\"></script>\n"); }
            format!("{}<body class=\"site{class}\" data-id=\"{id}\" data-root=\"{pre}\">\n{body}{tags}</body></html>\n", head(title, pre, canvas, nav)) }
        Some(l) => { let sealed = l.seal(serde_json::json!({ "t": title, "h": body, "s": scripts }).to_string().as_bytes())?;
            format!("{}<body class=\"site locked{class}\" data-id=\"{id}\" data-root=\"{pre}\">\n{LOCK_FORM}<script type=\"application/octet-stream\" id=\"mnc\">{sealed}</script>\n<script src=\"{pre}assets/data.js\"></script><script src=\"{pre}assets/lock.js\"></script>\n</body></html>\n", head("Protected notes", pre, canvas, nav)) }
    })
}

/// Writes the folder. `src`: what media looks like (the application's). Stopped between pages by `p.stop`; what was
/// written by then stays, under the marker, to be replaced by the next export there.
pub fn write(job: &Job, src: &mut dyn Source, mut videos: Option<&mut dyn Videos>, fonts: &super::Fonts, p: &Progress, wake: &dyn Fn()) -> Result<(), String> {
    let io = |what: &str, e: std::io::Error| format!("{what} could not be written ({e})");
    let dest = job.dest.as_path(); let (old_salt, old_videos) = marker_read(dest); prepare(dest)?;
    let lock = match job.password.as_deref().filter(|w| !w.is_empty()) { Some(w) => Some(Sealer::new(w, old_salt)?), None => None }; let lock = lock.as_ref();
    marker_write(dest, lock.map(|l| l.salt), &old_videos).map_err(|e| io("The folder", e))?;
    let pages = job.exported(); let several = pages.len() != 1;
    // the look, once
    let mut css = qcv_notes::export::font_faces(fonts); css += qcv_notes::export::TOKENS; css += CSS; css += &super::syntax_css(&mn_layout::syntax::DARK); css += SITE_CSS;
    css += &format!("@media print{{\n{}\n{PRINT}{}{SITE_PRINT}}}\n", qcv_notes::export::PRINT_TOKENS, super::syntax_css(&mn_layout::syntax::LIGHT));
    std::fs::write(dest.join("assets/site.css"), css).map_err(|e| io("The stylesheet", e))?;
    if job.pages.iter().any(|x| matches!(x.kind, Kind::Canvas(_))) { std::fs::write(dest.join("assets/canvas.css"), format!("{CANVAS_CSS}{CANVAS_SITE_CSS}")).map_err(|e| io("The stylesheet", e))?; }
    // where each page goes (with a password: under a name that says nothing of its title)
    let mut taken = HashSet::new(); let mut href: HashMap<BlockId, String> = HashMap::new();
    for i in &pages { let pg = &job.pages[*i]; href.insert(pg.id, if !several { "index.html".into() } else { match lock { Some(l) => format!("notes/{}.html", l.name(&format!("page:{}", pg.id))), None => format!("notes/{}.html", slug(&pg.title, &mut taken)) } }); }
    if several { std::fs::create_dir_all(dest.join("notes")).map_err(|e| io("The folder", e))?; }
    let pre = if several { "../" } else { "" };
    let (mut seen, mut copied, mut names, mut plans, mut queue) = (HashSet::new(), HashMap::new(), HashSet::new(), HashMap::new(), vec![]); let mut players = false; let mut text: HashMap<String, String> = HashMap::new();
    for i in &pages {
        if p.stop.load(Relaxed) { return Err("Stopped".into()) }
        p.states[*i].store(WORKING, Relaxed); wake();
        let pg = &job.pages[*i]; let title = if pg.title.trim().is_empty() { "Untitled" } else { pg.title.as_str() };
        let mut linked = Linked { inner: &mut *src, dest, pre, on: job.files, done: &mut copied, taken: &mut names, p, lock, videos: if job.videos { videos.as_deref_mut().map(|v| v as &mut dyn Videos) } else { None }, plans: &mut plans, queue: &mut queue };
        let (canvas, body) = match &pg.kind {
            Kind::Note(doc) => { let w = Walk::new(doc.walk(), job.opts); let mut b = String::new(); html_blocks(&w, doc.children(None), &Col::default(), &mut linked, &mut b);
                text.insert(pg.id.to_string(), words(doc));
                let copies = b.contains("<button class=\"copy\"");
                (false, format!("<main>\n<header class=\"doc\"><div class=\"titles\"><div class=\"name\">{}</div><div class=\"meta\"><span>Exported {}</span></div></div></header>\n<article>\n{b}</article>\n</main>\n{LIGHTBOX}{}", esc(title), esc(&job.when), if copies { COPY_SCRIPT } else { "" })) }
            Kind::Canvas(els) => { let refs: Vec<&mn_store::Element> = els.iter().collect(); let mut note = |id: BlockId| job.cards.get(&id).cloned();
                text.insert(pg.id.to_string(), els.iter().filter(|e| e.kind == "text").map(|e| e.record.trim()).filter(|t| !t.is_empty()).collect::<Vec<_>>().join("\n"));
                (true, canvas_body(&refs, title, &job.when, job.opts, &mut linked, &mut note)) }
            Kind::Folder => continue,
        };
        let plays = body.contains(" data-video=\""); players |= plays;
        let body = externalise(&body, dest, pre, &mut seen, lock, p)?;
        let mut scripts = vec![]; if plays { scripts.push("assets/video.js"); } if several { scripts.push("assets/site.js"); }
        std::fs::write(dest.join(&href[&pg.id]), page(title, pre, canvas, several, &pg.id.to_string(), &body, &scripts, lock)?).map_err(|e| io(&format!("\u{201c}{title}\u{201d}"), e))?;
        p.states[*i].store(DONE, Relaxed); wake();
    }
    // what every page's list and search show - sealed with a password, with what a page needs to ask for it
    let list: Vec<serde_json::Value> = job.pages.iter().map(|x| serde_json::json!({ "id": x.id.to_string(), "parent": x.parent.map(|p| p.to_string()), "title": if x.title.trim().is_empty() { "Untitled" } else { x.title.as_str() },
        "kind": match x.kind { Kind::Folder => "folder", Kind::Note(_) => "note", Kind::Canvas(_) => "canvas" }, "href": href.get(&x.id) })).collect();
    let data = serde_json::json!({ "title": job.name, "pages": list, "text": text }).to_string();
    match lock {
        Some(l) => { use base64::Engine; let sealed = if several { format!("window.MNC=\"{}\";", l.seal(data.as_bytes())?) } else { String::new() };
            std::fs::write(dest.join("assets/data.js"), format!("window.MNP={{\"s\":\"{}\",\"n\":{ROUNDS}}};{sealed}\n", base64::engine::general_purpose::STANDARD.encode(l.salt))).and_then(|_| std::fs::write(dest.join("assets/lock.js"), LOCK_JS)).map_err(|e| io("The lock", e))?; }
        None if several => std::fs::write(dest.join("assets/data.js"), format!("window.MN={data};\n")).map_err(|e| io("The list of notes", e))?,
        None => {}
    }
    if several {
        std::fs::write(dest.join("assets/site.js"), SITE_JS).map_err(|e| io("The list of notes", e))?;
        fn toc(job: &Job, href: &HashMap<BlockId, String>, parent: Option<BlockId>, out: &mut String) {
            let kids: Vec<&Page> = job.pages.iter().filter(|x| x.parent == parent).collect(); if kids.is_empty() { return }
            out.push_str("<ul>");
            for k in kids { let t = if k.title.trim().is_empty() { "Untitled" } else { k.title.as_str() };
                match href.get(&k.id) { Some(h) => out.push_str(&format!("<li><a href=\"{h}\">{}</a>", esc(t))), None => out.push_str(&format!("<li><span class=\"f\">{}</span>", esc(t))) }
                toc(job, href, Some(k.id), out); out.push_str("</li>"); }
            out.push_str("</ul>");
        }
        let mut list = String::new(); let ids: HashSet<BlockId> = job.pages.iter().map(|x| x.id).collect();
        // (an entry whose parent is not among the pages stands at the top)
        let tops: Vec<Option<BlockId>> = { let mut v = vec![None]; for x in &job.pages { if let Some(p) = x.parent { if !ids.contains(&p) && !v.contains(&Some(p)) { v.push(Some(p)); } } } v };
        for t in tops { toc(job, &href, t, &mut list); }
        let n = pages.len();
        let body = format!("<main>\n<header class=\"doc\"><div class=\"titles\"><div class=\"name\">{}</div><div class=\"meta\"><span>{n} page{} \u{b7} Exported {}</span></div></div></header>\n<article class=\"toc\">\n<h1>{}</h1>\n{list}\n</article>\n</main>\n", esc(&job.name), if n == 1 { "" } else { "s" }, esc(&job.when), esc(&job.name));
        std::fs::write(dest.join("index.html"), page(&job.name, "", false, true, "", &body, &["assets/site.js"], lock)?).map_err(|e| io("The contents", e))?;
    }
    // the videos, one at a time, after the pages: a page whose video is not there (stopped, failed) shows its stills
    if players { std::fs::write(dest.join("assets/video.js"), VIDEO_JS).map_err(|e| io("The player", e))?; }
    let dir = dest.join("media").join("video");
    // a copy made before, under another name (the password changed, was set or was taken away): it takes its new name
    for (_, _, plain, name, _) in &queue { if let Some(old) = old_videos.get(plain).filter(|o| *o != name) { if dir.join(old).is_file() && !dir.join(name).exists() { let _ = std::fs::rename(dir.join(old), dir.join(name)); } } }
    let wanted: HashSet<&str> = queue.iter().map(|q| q.3.as_str()).collect();
    if let Ok(list) = std::fs::read_dir(&dir) { for e in list.flatten() { if !wanted.contains(e.file_name().to_string_lossy().as_ref()) && e.file_name() != "index.html" { let _ = std::fs::remove_file(e.path()); } } }
    marker_write(dest, lock.map(|l| l.salt), &queue.iter().map(|q| (q.2.clone(), q.3.clone())).collect()).map_err(|e| io("The folder", e))?;
    // with a password, a server that lists a folder's files is given an empty page instead: media's names stay unknown
    if lock.is_some() { for d in ["assets", "notes", "media", "media/files", "media/video"] { if dest.join(d).is_dir() { std::fs::write(dest.join(d).join("index.html"), "<!DOCTYPE html><title></title>\n").map_err(|e| io("The folder", e))?; } } }
    if let Some(v) = videos {
        for (from, clip, _, name, row) in &queue {
            let set = |state: u8, done: f32, why: &str| { if let Some(r) = p.videos.lock().unwrap().get_mut(*row) { r.state = state; r.done = done; r.why = why.to_string(); } wake(); };
            if p.stop.load(Relaxed) { return Err("Stopped".into()) }
            let out = dir.join(name);
            if out.is_file() { set(KEPT, 1.0, ""); continue }
            set(WORKING, 0.0, ""); std::fs::create_dir_all(&dir).map_err(|e| io("The folder", e))?;
            if lock.is_some() { let _ = std::fs::write(dir.join("index.html"), "<!DOCTYPE html><title></title>\n"); }
            let part = dir.join(format!("{name}.part"));
            let r = v.make(from, clip, &part, &p.stop, &|d| { if let Some(r) = p.videos.lock().unwrap().get_mut(*row) { r.done = d.clamp(0.0, 1.0); } wake(); }).and_then(|_| std::fs::rename(&part, &out).map_err(|e| format!("The video could not be written ({e})")));
            match r { Ok(()) => { p.bytes.fetch_add(std::fs::metadata(&out).map(|m| m.len()).unwrap_or(0), Relaxed); set(DONE, 1.0, "") }
                Err(e) => { let _ = std::fs::remove_file(&part); if p.stop.load(Relaxed) { set(QUEUED, 0.0, ""); return Err("Stopped".into()) } set(FAILED, 0.0, &e) } }
        }
    }
    Ok(())
}

const SITE_CSS: &str = r#"
#nav{position:fixed;inset:0 auto 0 0;width:280px;display:none;flex-direction:column;background:var(--l1);border-right:1px solid var(--g300);z-index:40;font:13px/20px var(--sans)}
html.nav #nav{display:flex}
html.nav body.site{padding-left:280px}
#navb{position:fixed;left:8px;top:8px;z-index:41;width:30px;height:30px;display:flex;align-items:center;justify-content:center;padding:0;border:1px solid var(--g300);border-radius:var(--rs);background:var(--l1);color:var(--g600);cursor:pointer}
#navb:hover{color:var(--g900);background:var(--l2)}
html.nav #navb{left:242px;border-color:transparent;background:none}
#nav .top{padding:12px 46px 8px 16px;font:500 13px/22px var(--sans);color:var(--g900);overflow:hidden;text-overflow:ellipsis;white-space:nowrap}
#nav .top a{color:inherit;text-decoration:none}
#nav input{margin:0 12px 8px;padding:6px 10px;border:1px solid var(--g300);border-radius:var(--rs);background:var(--base);color:var(--g900);font:13px/18px var(--sans);outline:none;-webkit-appearance:none}
#nav input:focus{border-color:var(--accent-hi)}
#nav .list{flex:1;overflow:auto;padding:0 8px 16px}
#nav .list a,#nav .fold{display:block;padding:4px 8px;border-radius:var(--rs);color:var(--g700);text-decoration:none;cursor:pointer;white-space:nowrap;overflow:hidden;text-overflow:ellipsis}
#nav .list a:hover,#nav .fold:hover{background:var(--l2);color:var(--g900)}
#nav .list a.cur{background:var(--l2);color:var(--g900)}
#nav .fold{color:var(--g600);font-weight:500;user-select:none}
#nav .fold::before{content:"\25BE";display:inline-block;width:14px;font-size:10px}
#nav .shut>.fold::before{content:"\25B8"}
#nav .kids{margin-left:14px}
#nav .shut>.kids{display:none}
#nav .list a.hit{white-space:normal}
#nav .hit small{display:block;color:var(--g500);font-size:12px;line-height:17px;max-height:34px;overflow:hidden}
#nav .none{padding:8px;color:var(--g500)}
.toc ul{list-style:none;margin:0;padding:0 0 0 20px}.toc>ul{padding:0}
.toc li{margin:0;padding:3px 0}.toc .f{color:var(--g600);font-weight:500}
@media (max-width:900px){html.nav body.site{padding-left:0}html.nav #nav{box-shadow:0 0 48px rgba(0,0,0,.6)}}
.vp{position:relative;outline:none;background:#000;line-height:0}
.vp .stage{position:relative;width:100%;background:#000;cursor:pointer}
.vp video{display:block;width:100%;height:100%}
.vp .over{position:absolute;inset:0;pointer-events:none}
.vp .over .ink{position:absolute;inset:0;width:100%;height:100%;overflow:visible}
.vp .bar{display:flex;align-items:center;gap:2px;height:36px;padding:0 6px;background:var(--l2);border-bottom:1px solid var(--g300);line-height:18px;user-select:none}
.vp .bar button,.vp .cap button{flex:none;display:flex;align-items:center;justify-content:center;width:28px;height:28px;padding:0;border:0;border-radius:var(--rs);background:none;color:var(--g700);cursor:pointer}
.vp .bar button:hover,.vp .cap button:hover{color:var(--g900);background:var(--l1)}
.vp .bar button.off{color:var(--g500)}
.vp .time{flex:none;min-width:150px;padding:0 8px;font:12px/18px var(--mono);color:var(--g700);white-space:pre}
.vp .track{position:relative;flex:1;height:28px;margin:0 8px;cursor:pointer;touch-action:none}
.vp .track::before{content:"";position:absolute;left:0;right:0;top:12px;height:4px;border-radius:2px;background:var(--g300)}
.vp .fill{position:absolute;left:0;top:12px;height:4px;border-radius:2px;background:var(--accent-hi)}
.vp .knob{position:absolute;top:8px;width:12px;height:12px;margin-left:-6px;border-radius:50%;background:var(--g900)}
.vp .mark{position:absolute;top:6px;width:3px;height:16px;margin-left:-1px;border-radius:1px;background:#b48ef0}
.vp .mark.done{background:var(--g500)}
.vp .cap{display:flex;align-items:center;gap:8px;min-height:44px;padding:6px;background:var(--l1);border-bottom:1px solid var(--g300);font:14px/20px var(--sans);user-select:none}
.vp .cap .tc{flex:none;font:12px/20px var(--mono);color:var(--g600)}
.vp .cap .tx{flex:1;min-width:0;color:var(--g900);white-space:pre-wrap;overflow-wrap:anywhere;display:-webkit-box;-webkit-line-clamp:2;-webkit-box-orient:vertical;overflow:hidden;cursor:pointer;user-select:text}
.vp .cap.open .tx{display:block}
.vp .cap .none{color:var(--g500);font-style:italic}
.vp .cap.dim .tx,.vp .cap.dim .tc{color:var(--g500)}.vp .cap.dim .state{opacity:.5}
.vp .cap .state{flex:none}
.vp .stills{display:flex;gap:6px;padding:8px;overflow-x:auto;background:var(--l1);border-bottom:1px solid var(--g300)}
.vp .stills img{flex:none;height:44px;width:auto;border-radius:3px;cursor:pointer;opacity:.75;outline:2px solid transparent}
.vp .stills img:hover,.vp .stills img.on{opacity:1}.vp .stills img.on{outline-color:#b48ef0}
.vp .stills img.done{filter:grayscale(.6)}
figure[data-video].live+table.vnotes{display:none}
.vp:fullscreen{display:flex;flex-direction:column;justify-content:center;background:#000}
.vp:fullscreen .stage{width:min(100vw,calc((100vh - 82px)*var(--a,1.7778)));margin:0 auto}
.vp:fullscreen .stills{display:none}
body.locked{min-height:100vh}html.nav body.locked{padding-left:0}
#lock{position:fixed;inset:0;display:flex;align-items:center;justify-content:center;padding:24px;z-index:60}
#lock .lockbox{width:320px;max-width:100%;padding:24px;background:var(--l1);border:1px solid var(--g300);border-radius:var(--r);display:flex;flex-direction:column;gap:12px}
#lock .t{font:500 15px/22px var(--sans);color:var(--g900)}
#lock input{padding:8px 10px;border:1px solid var(--g300);border-radius:var(--rs);background:var(--base);color:var(--g900);font:14px/20px var(--sans);outline:none}
#lock input:focus{border-color:var(--accent-hi)}
#lock button{padding:8px 14px;border:0;border-radius:var(--rs);background:var(--accent-hi);color:#fff;font:500 14px/20px var(--sans);cursor:pointer}
#lock .msg{min-height:18px;font:12px/18px var(--sans);color:var(--g600)}
.card .vp .stills{padding:6px;border-bottom:0}.card .vp .stills img{height:36px}.card .vp .cap{min-height:0;padding:2px 6px;font-size:12px;line-height:18px}.card .vp .cap .tx{-webkit-line-clamp:1}.card .vp .cap button{width:22px;height:22px}
"#;
const SITE_PRINT: &str = "#nav,#navb,.vp{display:none!important}figure[data-video] .thumb{display:block!important}figure[data-video].live+table.vnotes{display:table!important}html.nav body.site{padding-left:0}\n";
/// A canvas's page beside the list: its bar and its view start where the list ends.
const CANVAS_SITE_CSS: &str = "body.site.cv{padding-left:0!important}body.site.cv #bar{padding-left:48px}html.nav body.site.cv #bar,html.nav body.site.cv #view{left:280px}html.nav body.site.cv #bar{padding-left:14px}\n@media (max-width:900px){html.nav body.site.cv #bar,html.nav body.site.cv #view{left:0}}\n.card .vp{flex:1;min-height:0;display:flex;flex-direction:column}.card .vp .stage{flex:1;min-height:0;aspect-ratio:auto!important;container-type:size}.card .vp video{object-fit:contain}.card .vp .bar{flex:none;border-bottom:0}.card .vp .time{min-width:0}\n.card .vp .over{inset:auto;left:50%;top:50%;transform:translate(-50%,-50%);width:min(100cqw,calc(100cqh*var(--a,1.7778)));aspect-ratio:var(--a,1.7778)}\n";

/// The list of notes and the search, built on every page from `data.js`.
const SITE_JS: &str = r#"(function(){
var D=window.MN;if(!D)return;
var B=document.body,H=document.documentElement,root=B.getAttribute('data-root')||'',cur=B.getAttribute('data-id')||'';
function el(t,c,x){var e=document.createElement(t);if(c)e.className=c;if(x!=null)e.textContent=x;return e}
function mem(k,v){try{if(v===undefined)return localStorage.getItem(k);localStorage.setItem(k,v)}catch(e){}return null}
var nav=el('nav');nav.id='nav';nav.setAttribute('aria-label','Notes');
var top=el('div','top'),home=el('a','',D.title);home.href=root+'index.html';home.title=D.title;top.appendChild(home);
var q=el('input');q.type='search';q.placeholder='Search';q.setAttribute('aria-label','Search the notes');
var list=el('div','list');nav.appendChild(top);nav.appendChild(q);nav.appendChild(list);
var b=el('button');b.id='navb';b.type='button';b.title='Show or hide the list of notes';b.setAttribute('aria-label',b.title);
b.innerHTML='<svg viewBox="0 0 24 24" width="16" height="16" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linejoin="round"><rect x="3" y="4.5" width="18" height="15" rx="2.5"/><path d="M9 4.5v15"/></svg>';
B.appendChild(nav);B.appendChild(b);
var by={},ids={};D.pages.forEach(function(p){ids[p.id]=1});
D.pages.forEach(function(p){var k=p.parent&&ids[p.parent]?p.parent:'';(by[k]=by[k]||[]).push(p)});
function walk(pid,into){(by[pid]||[]).forEach(function(p){
  var kids=by[p.id]&&by[p.id].length;
  if(!p.href){var w=el('div',mem('mn.f.'+p.id)==='1'?'shut':''),f=el('div','fold',p.title),k=el('div','kids');
    f.onclick=function(){w.classList.toggle('shut');mem('mn.f.'+p.id,w.classList.contains('shut')?'1':'0')};
    w.appendChild(f);w.appendChild(k);into.appendChild(w);walk(p.id,k);return}
  var a=el('a',p.id===cur?'cur':'',p.title);a.href=root+p.href;a.title=p.title;into.appendChild(a);
  if(kids){var k2=el('div','kids');into.appendChild(k2);walk(p.id,k2)}
})}
function tree(){list.textContent='';walk('',list)}
function search(s){s=s.trim().toLowerCase();if(!s){tree();return}
  list.textContent='';var n=0;
  D.pages.forEach(function(p){if(!p.href)return;var t=D.text[p.id]||'',i=t.toLowerCase().indexOf(s),inTitle=p.title.toLowerCase().indexOf(s)>=0;if(i<0&&!inTitle)return;n++;
    var a=el('a','hit',p.title);a.href=root+p.href;
    if(i>=0){var from=Math.max(0,i-30);a.appendChild(el('small','',(from?'…':'')+t.substr(from,120).replace(/\s+/g,' ')))}
    list.appendChild(a)});
  if(!n)list.appendChild(el('div','none','Nothing found'))}
q.addEventListener('input',function(){search(q.value)});
q.addEventListener('keydown',function(e){e.stopPropagation();if(e.key==='Escape'){q.value='';search('');q.blur()}if(e.key==='Enter'){var a=list.querySelector('a');if(a)location.href=a.href}});
b.addEventListener('click',function(){var on=!H.classList.contains('nav');H.classList.toggle('nav',on);mem('mn.nav',on?'1':'0');window.dispatchEvent(new Event('resize'))});
tree();var c=list.querySelector('.cur');if(c&&c.scrollIntoView)c.scrollIntoView({block:'nearest'});
document.addEventListener('keydown',function(e){if(e.key==='/'&&!e.metaKey&&!e.ctrlKey&&!/^(input|textarea)$/i.test(e.target.tagName||'')&&H.classList.contains('nav')){e.preventDefault();q.focus()}});
})();
"#;

/// The password box of a sealed page.
const LOCK_FORM: &str = "<form id=\"lock\"><div class=\"lockbox\"><div class=\"t\">These notes are protected</div><input type=\"password\" id=\"lockpw\" placeholder=\"Password\" autocomplete=\"current-password\" aria-label=\"Password\"><button type=\"submit\">Open</button><div class=\"msg\" id=\"lockmsg\" role=\"status\"></div></div></form>\n";
/// A sealed page opened (row 342): the key is made of the password (PBKDF2-HMAC-SHA-256 over the folder's salt), the
/// page's content and the list are opened with it (AES-256-GCM: a wrong password is known by the seal not matching),
/// the content is put into the page and its scripts are run. The key is kept for the tab's session, so the next page
/// does not ask. The browser's own crypto does the work where a page may use it (https, a file from disk); over plain
/// http it may not, and the same is done here in script - slower, a few seconds for the key.
const LOCK_JS: &str = r#"(function(){
var P=window.MNP,B=document.body,root=B.getAttribute('data-root')||'',box=document.getElementById('mnc'),form=document.getElementById('lock'),pw=document.getElementById('lockpw'),msg=document.getElementById('lockmsg');
if(!P||!box)return;
function b64(s){var b=atob(s),u=new Uint8Array(b.length),i;for(i=0;i<b.length;i++)u[i]=b.charCodeAt(i);return u}
function tob64(u){var s='',i;for(i=0;i<u.length;i++)s+=String.fromCharCode(u[i]);return btoa(s)}
function utf8(s){if(window.TextEncoder)return new TextEncoder().encode(s);var e=unescape(encodeURIComponent(s)),u=new Uint8Array(e.length),i;for(i=0;i<e.length;i++)u[i]=e.charCodeAt(i);return u}
function text(u){if(window.TextDecoder)return new TextDecoder().decode(u);var s='',i;for(i=0;i<u.length;i+=8192)s+=String.fromCharCode.apply(null,u.subarray(i,i+8192));return decodeURIComponent(escape(s))}
var fast=!!(window.crypto&&crypto.subtle)&&!window.MN_SLOW;
// -- the same in script, for a page that may not use the browser's crypto --
function consts(h){var a=new Int32Array(h.length/8),i;for(i=0;i<a.length;i++)a[i]=parseInt(h.substr(i*8,8),16)|0;return a}
var K=consts('428a2f9871374491b5c0fbcfe9b5dba53956c25b59f111f1923f82a4ab1c5ed5d807aa9812835b01243185be550c7dc372be5d7480deb1fe9bdc06a7c19bf174e49b69c1efbe47860fc19dc6240ca1cc2de92c6f4a7484aa5cb0a9dc76f988da983e5152a831c66db00327c8bf597fc7c6e00bf3d5a7914706ca635114292967'+
 '27b70a852e1b21384d2c6dfc53380d13650a7354766a0abb81c2c92e92722c85a2bfe8a1a81a664bc24b8b70c76c51a3d192e819d6990624f40e3585106aa07019a4c1161e376c082748774c34b0bcb5391c0cb34ed8aa4a5b9cca4f682e6ff3748f82ee78a5636f84c878148cc7020890befffaa4506cebbef9a3f7c67178f2'),
 IV=consts('6a09e667bb67ae853c6ef372a54ff53a510e527f9b05688c1f83d9ab5be0cd19');
function comp(H,w,M,o){var i,x,y,a=H[0],b=H[1],c=H[2],d=H[3],e=H[4],f=H[5],g=H[6],h=H[7],t1,t2;
 for(i=0;i<16;i++)w[i]=M[o+i];
 for(i=16;i<64;i++){x=w[i-15];y=w[i-2];w[i]=(w[i-16]+((x>>>7|x<<25)^(x>>>18|x<<14)^(x>>>3))+w[i-7]+((y>>>17|y<<15)^(y>>>19|y<<13)^(y>>>10)))|0}
 for(i=0;i<64;i++){t1=(h+((e>>>6|e<<26)^(e>>>11|e<<21)^(e>>>25|e<<7))+((e&f)^(~e&g))+K[i]+w[i])|0;t2=(((a>>>2|a<<30)^(a>>>13|a<<19)^(a>>>22|a<<10))+((a&b)^(a&c)^(b&c)))|0;h=g;g=f;f=e;e=(d+t1)|0;d=c;c=b;b=a;a=(t1+t2)|0}
 H[0]=(H[0]+a)|0;H[1]=(H[1]+b)|0;H[2]=(H[2]+c)|0;H[3]=(H[3]+d)|0;H[4]=(H[4]+e)|0;H[5]=(H[5]+f)|0;H[6]=(H[6]+g)|0;H[7]=(H[7]+h)|0}
function bytesOf(W){var u=new Uint8Array(W.length*4),i;for(i=0;i<u.length;i++)u[i]=(W[i>>2]>>>(24-(i&3)*8))&255;return u}
function sha256(m){var l=m.length,n=((l+8)>>6)+1,M=new Int32Array(n*16),H=new Int32Array(IV),w=new Int32Array(64),i;
 for(i=0;i<l;i++)M[i>>2]|=m[i]<<(24-(i&3)*8);M[l>>2]|=0x80<<(24-(l&3)*8);M[n*16-1]=l*8;for(i=0;i<n;i++)comp(H,w,M,i*16);return H}
// PBKDF2-HMAC-SHA-256, one block of output (32 bytes), a salt of 16 bytes
function pbkdf2(pass,salt,rounds){var k=pass.length>64?bytesOf(sha256(pass)):pass,ip=new Int32Array(16),op=new Int32Array(16),w=new Int32Array(64),SI=new Int32Array(IV),SO=new Int32Array(IV),i,b,r;
 for(i=0;i<64;i++){b=i<k.length?k[i]:0;ip[i>>2]|=(b^0x36)<<(24-(i&3)*8);op[i>>2]|=(b^0x5c)<<(24-(i&3)*8)}
 comp(SI,w,ip,0);comp(SO,w,op,0);
 var M=new Int32Array(16),H=new Int32Array(8),G=new Int32Array(8),T=new Int32Array(8);
 function outer(){for(var j=0;j<8;j++)M[j]=H[j];M[8]=0x80000000|0;for(j=9;j<15;j++)M[j]=0;M[15]=768;G.set(SO);comp(G,w,M,0)}
 for(i=0;i<16;i++)M[i>>2]|=salt[i]<<(24-(i&3)*8);M[4]=1;M[5]=0x80000000|0;M[15]=672;H.set(SI);comp(H,w,M,0);outer();T.set(G);
 for(r=1;r<rounds;r++){for(i=0;i<8;i++)M[i]=G[i];M[8]=0x80000000|0;for(i=9;i<15;i++)M[i]=0;M[15]=768;H.set(SI);comp(H,w,M,0);outer();for(i=0;i<8;i++)T[i]^=G[i]}
 return bytesOf(T)}
// AES-256
var S=new Uint8Array(256);(function(){var p=1,q=1,x;do{p=(p^(p<<1)^(p&0x80?0x1b:0))&255;q^=q<<1;q&=255;q^=q<<2;q&=255;q^=q<<4;q&=255;if(q&0x80)q^=0x09;x=q^((q<<1|q>>7)&255)^((q<<2|q>>6)&255)^((q<<3|q>>5)&255)^((q<<4|q>>4)&255);S[p]=x^0x63}while(p!==1);S[0]=0x63})();
function xt(a){return((a<<1)^(a&0x80?0x1b:0))&255}
function sub(t){return(S[t>>>24]<<24)|(S[(t>>>16)&255]<<16)|(S[(t>>>8)&255]<<8)|S[t&255]}
function expand(k){var W=new Int32Array(60),i,t,rc=1;for(i=0;i<8;i++)W[i]=(k[4*i]<<24)|(k[4*i+1]<<16)|(k[4*i+2]<<8)|k[4*i+3];
 for(i=8;i<60;i++){t=W[i-1];if(i%8===0){t=(t<<8)|(t>>>24);t=sub(t)^(rc<<24);rc=xt(rc)}else if(i%8===4)t=sub(t);W[i]=W[i-8]^t}return W}
var st=new Uint8Array(16),tm=new Uint8Array(16);
function enc(W,inp,out){var i,r,c,a0,a1,a2,a3,x;
 for(i=0;i<16;i++)st[i]=inp[i]^((W[i>>2]>>>(24-(i&3)*8))&255);
 for(r=1;r<=14;r++){for(i=0;i<16;i++)tm[i]=S[st[(i+((i&3)<<2))&15]];
  if(r<14){for(c=0;c<16;c+=4){a0=tm[c];a1=tm[c+1];a2=tm[c+2];a3=tm[c+3];x=a0^a1^a2^a3;st[c]=a0^x^xt(a0^a1);st[c+1]=a1^x^xt(a1^a2);st[c+2]=a2^x^xt(a2^a3);st[c+3]=a3^x^xt(a3^a0)}}else st.set(tm);
  for(i=0;i<16;i++)st[i]^=(W[r*4+(i>>2)]>>>(24-(i&3)*8))&255}
 out.set(st)}
// GCM: the seal checked, then the text
function gmul(X,Hh){var z0=0,z1=0,z2=0,z3=0,v0=Hh[0],v1=Hh[1],v2=Hh[2],v3=Hh[3],i,j,x,l;
 for(i=0;i<4;i++){x=X[i];for(j=31;j>=0;j--){if((x>>>j)&1){z0^=v0;z1^=v1;z2^=v2;z3^=v3}l=v3&1;v3=(v3>>>1)|(v2<<31);v2=(v2>>>1)|(v1<<31);v1=(v1>>>1)|(v0<<31);v0=v0>>>1;if(l)v0^=0xe1000000}}
 X[0]=z0;X[1]=z1;X[2]=z2;X[3]=z3}
function gcmOpen(key,d){if(d.length<28)return null;var W=expand(key),ct=d.subarray(12,d.length-16),tag=d.subarray(d.length-16),n=ct.length,i,j,bad=0;
 var Hb=new Uint8Array(16),c=new Uint8Array(16),ks=new Uint8Array(16);enc(W,c,Hb);
 var Hw=new Int32Array(4),X=new Int32Array(4);for(i=0;i<16;i++)Hw[i>>2]|=Hb[i]<<(24-(i&3)*8);
 for(i=0;i<n;i+=16){for(j=0;j<16&&i+j<n;j++)X[j>>2]^=ct[i+j]<<(24-(j&3)*8);gmul(X,Hw)}
 X[2]^=n>>>29;X[3]^=(n<<3)|0;gmul(X,Hw);
 c.set(d.subarray(0,12));c[12]=0;c[13]=0;c[14]=0;c[15]=1;enc(W,c,ks);
 for(i=0;i<16;i++)bad|=tag[i]^ks[i]^((X[i>>2]>>>(24-(i&3)*8))&255);
 if(bad)return null;
 var out=new Uint8Array(n),ctr=2;
 for(i=0;i<n;i+=16){c[12]=ctr>>>24;c[13]=(ctr>>>16)&255;c[14]=(ctr>>>8)&255;c[15]=ctr&255;ctr++;enc(W,c,ks);for(j=0;j<16&&i+j<n;j++)out[i+j]=ct[i+j]^ks[j]}
 return out}
// -- the page --
function derive(pass){var p=utf8(pass),salt=b64(P.s);
 if(fast)return crypto.subtle.importKey('raw',p,'PBKDF2',false,['deriveBits']).then(function(k){return crypto.subtle.deriveBits({name:'PBKDF2',salt:salt,iterations:P.n,hash:'SHA-256'},k,256)}).then(function(b){return new Uint8Array(b)});
 return new Promise(function(ok){setTimeout(function(){ok(pbkdf2(p,salt,P.n))},30)})}
function open(key,d){
 if(fast)return crypto.subtle.importKey('raw',key,'AES-GCM',false,['decrypt']).then(function(k){return crypto.subtle.decrypt({name:'AES-GCM',iv:d.subarray(0,12)},k,d.subarray(12))}).then(function(b){return new Uint8Array(b)});
 return new Promise(function(ok,no){var r=gcmOpen(key,d);if(r)ok(r);else no(new Error('sealed'))})}
function run(key){
 var jobs=[open(key,b64(box.textContent.replace(/\s+/g,'')))];if(window.MNC)jobs.push(open(key,b64(window.MNC)));
 return Promise.all(jobs).then(function(r){var pg=JSON.parse(text(r[0]));if(r[1])window.MN=JSON.parse(text(r[1]));
  try{sessionStorage.setItem('mn.k.'+P.s,tob64(key))}catch(e){}
  document.title=pg.t;if(form&&form.parentNode)form.parentNode.removeChild(form);B.classList.remove('locked');
  var h=document.createElement('div'),olds=[],n;h.innerHTML=pg.h;
  while(h.firstChild){n=h.firstChild;B.insertBefore(n,box);if(n.nodeType===1){if(n.tagName==='SCRIPT')olds.push(n);else Array.prototype.push.apply(olds,n.querySelectorAll('script'))}}
  olds.forEach(function(o){var s=document.createElement('script');s.text=o.textContent;o.parentNode.replaceChild(s,o)});
  (function next(i){if(i>=pg.s.length)return;var s=document.createElement('script');s.src=root+pg.s[i];s.onload=s.onerror=function(){next(i+1)};B.appendChild(s)})(0)})}
function ask(m){if(!form)return;form.style.display='';msg.textContent=m||'';pw.disabled=false;pw.focus();if(pw.select)pw.select()}
var saved=null;try{saved=sessionStorage.getItem('mn.k.'+P.s)}catch(e){}
if(form)form.addEventListener('submit',function(e){e.preventDefault();var v=pw.value;if(!v)return;pw.disabled=true;msg.textContent=fast?'Opening…':'Opening… this takes a few seconds here';
 derive(v).then(run).catch(function(){ask('That is not the password')})});
if(saved){if(form)form.style.display='none';run(b64(saved)).catch(function(){try{sessionStorage.removeItem('mn.k.'+P.s)}catch(e){}ask('')})}else ask('');
})();
"#;

/// A video's player (row 340): the browser's own video element with our controls - play, a line to scrub with the
/// notes marked on it, the counter, a frame back and forward, the drawings' switch, sound, the whole screen - the
/// notes' stills under it, and a note's drawings laid over the picture while the video stands on its frame. A page
/// whose video cannot be loaded keeps its poster and stills.
const VIDEO_JS: &str = r#"(function(){
var H=document.documentElement;
function el(t,c,x){var e=document.createElement(t);if(c)e.className=c;if(x!=null)e.textContent=x;return e}
function p2(n){return(n<10?'0':'')+n}
function tc(f,fps){var r=Math.max(1,Math.round(fps)),s=Math.floor(f/r);return p2(Math.floor(s/3600))+':'+p2(Math.floor(s/60)%60)+':'+p2(s%60)+':'+p2(f%r)}
function svg(d){return '<svg viewBox="0 0 24 24" width="18" height="18" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round">'+d+'</svg>'}
var I={play:svg('<path d="M8 5.5v13l10.5-6.5z" fill="currentColor"/>'),pause:svg('<path d="M8 5.5v13M16 5.5v13" stroke-width="2.6"/>'),prev:svg('<path d="M14.5 6.5 8 12l6.5 5.5M6 6.5v11"/>'),next:svg('<path d="M9.5 6.5 16 12l-6.5 5.5M18 6.5v11"/>'),
 back:svg('<path d="M14.5 6 8.5 12l6 6"/>'),on:svg('<path d="M9.5 6l6 6-6 6"/>'),
 ink:svg('<path d="M4 20l1-4L16.5 4.5a2.1 2.1 0 0 1 3 3L8 19z"/>'),vol:svg('<path d="M4 9.5h3.5L12 6v12l-4.5-3.5H4zM15.5 9a4 4 0 0 1 0 6M18 6.5a7.5 7.5 0 0 1 0 11"/>'),mute:svg('<path d="M4 9.5h3.5L12 6v12l-4.5-3.5H4zM16 9.5l5 5M21 9.5l-5 5"/>'),full:svg('<path d="M4 9V4h5M20 9V4h-5M4 15v5h5M20 15v5h-5"/>')};
function btn(icon,title,fn){var b=el('button');b.type='button';b.title=title;b.setAttribute('aria-label',title);b.innerHTML=icon;b.addEventListener('click',function(e){e.stopPropagation();fn()});return b}
Array.prototype.forEach.call(document.querySelectorAll('figure[data-video]'),function(fig){
  var fps=parseFloat(fig.getAttribute('data-fps'))||24,frames=Math.max(1,parseInt(fig.getAttribute('data-frames'),10)||1);
  var poster=fig.querySelector('.thumb'),pimg=poster&&poster.querySelector('img');
  var table=fig.nextElementSibling;if(!(table&&table.matches&&table.matches('table.vnotes')))table=null;
  var notes=[];if(table)Array.prototype.forEach.call(table.querySelectorAll('tr[data-frame]'),function(tr){var th=tr.querySelector('.thumb');var tx=tr.querySelector('td.tx .text'),tm=tr.querySelector('td.tc .main');notes.push({f:parseInt(tr.getAttribute('data-frame'),10),tr:tr,img:th&&th.querySelector('img'),ink:th&&th.querySelector('svg.ink'),done:tr.classList.contains('done'),text:tx&&!tx.querySelector('.none')?tx.textContent:'',tc:tm?tm.textContent:''})});
  notes.sort(function(a,b){return a.f-b.f});
  var vp=el('div','vp'),stage=el('div','stage'),v=document.createElement('video'),over=el('div','over');
  v.preload='metadata';v.playsInline=true;v.setAttribute('playsinline','');
  if(pimg){v.poster=pimg.src;var a=(parseFloat(pimg.getAttribute('width'))||16)/(parseFloat(pimg.getAttribute('height'))||9);stage.style.aspectRatio=String(a);vp.style.setProperty('--a',String(a))}
  stage.appendChild(v);stage.appendChild(over);
  var bar=el('div','bar'),time=el('span','time'),track=el('div','track'),fill=el('div','fill'),knob=el('div','knob');track.appendChild(fill);
  notes.forEach(function(n){n.mark=el('div','mark'+(n.done?' done':''));n.mark.style.left=(frames>1?Math.min(1,n.f/(frames-1))*100:0)+'%';track.appendChild(n.mark)});track.appendChild(knob);
  var shown=null,ready=false,capN=null,capOn=null;
  var cap=el('div','cap'),cTc=el('span','tc'),cTx=el('div','tx'),cSt=el('span','state');
  function cur(){return Math.min(frames-1,Math.max(0,Math.floor(v.currentTime*fps+1e-3)))}
  function seek(f){f=Math.min(frames-1,Math.max(0,Math.round(f)));v.currentTime=(f+0.5)/fps;paint(f)}
  function paint(at){var f=at==null?cur():at,pc=(frames>1?f/(frames-1)*100:0)+'%';
    time.textContent=tc(f,fps)+'  '+f;fill.style.width=pc;knob.style.left=pc;
    var n=null;notes.forEach(function(x){var on=x.f===f;if(on)n=x;x.tr.classList.toggle('on',on);if(x.still)x.still.classList.toggle('on',on)});
    if(n!==shown){shown=n;over.textContent='';if(n&&n.ink)over.appendChild(n.ink.cloneNode(true))}
    // the caption: the note the video stands on, else - dimmed - the last one before here
    var c=n;if(!c)for(var i=0;i<notes.length;i++){if(notes[i].f<=f)c=notes[i]}
    if(c!==capN||!!n!==capOn){if(c!==capN)cap.classList.remove('open');capN=c;capOn=!!n;cap.classList.toggle('dim',!n);cTc.textContent=c?c.tc:'';cTx.textContent='';if(c){if(c.text)cTx.textContent=c.text;else cTx.appendChild(el('span','none','No text'))}
      cSt.style.display=c?'':'none';cSt.className='state'+(c&&c.done?' done':'');cSt.textContent=c?(c.done?'Addressed':'Open'):''}
    bPlay.innerHTML=v.paused?I.play:I.pause;bMute.innerHTML=v.muted?I.mute:I.vol;bInk.classList.toggle('off',H.classList.contains('noink'))}
  function toggle(){if(v.paused){var q=v.play();if(q&&q.catch)q.catch(function(){})}else v.pause()}
  function step(d){v.pause();seek(cur()+d)}
  function hop(d){var f=cur(),t=null;if(d>0){for(var i=0;i<notes.length;i++)if(notes[i].f>f){t=notes[i];break}}else{for(var j=notes.length-1;j>=0;j--)if(notes[j].f<f){t=notes[j];break}}if(t){v.pause();seek(t.f)}}
  function ink(){var off=!H.classList.contains('noink');H.classList.toggle('noink',off);try{localStorage.setItem('mn.noink',off?'1':'0')}catch(e){}paint()}
  function full(){if(document.fullscreenElement)document.exitFullscreen();else if(vp.requestFullscreen)vp.requestFullscreen();else if(v.webkitEnterFullscreen)v.webkitEnterFullscreen()}
  var bPlay=btn(I.play,'Play or pause (Space)',toggle),bPrev=btn(I.prev,'A frame back (←)',function(){step(-1)}),bNext=btn(I.next,'A frame on (→)',function(){step(1)}),
      bInk=btn(I.ink,'Show or hide the drawings (A)',ink),bMute=btn(I.vol,'Sound (M)',function(){v.muted=!v.muted;paint()}),bFull=btn(I.full,'The whole screen (F)',full);
  [bPlay,bPrev,bNext,time,track,bInk,bMute,bFull].forEach(function(x){bar.appendChild(x)});
  vp.appendChild(stage);vp.appendChild(bar);
  if(notes.length){cap.appendChild(btn(I.back,'The note before ([)',function(){hop(-1)}));cap.appendChild(cTc);cap.appendChild(cTx);cap.appendChild(cSt);cap.appendChild(btn(I.on,'The next note (])',function(){hop(1)}));
    cTx.addEventListener('click',function(e){e.stopPropagation();cap.classList.toggle('open')});vp.appendChild(cap)}
  if(notes.length){var row=el('div','stills');notes.forEach(function(n){if(!n.img)return;var s=el('img',n.done?'done':'');s.src=n.img.src;s.alt='';s.title=(n.tc||tc(n.f,fps))+(n.text?' \u2014 '+n.text:'');s.addEventListener('click',function(e){e.stopPropagation();v.pause();seek(n.f)});n.still=s;row.appendChild(s)});if(row.firstChild)vp.appendChild(row)}
  vp.tabIndex=0;
  var drag=false;function at(e){var r=track.getBoundingClientRect();return Math.max(0,Math.min(1,(e.clientX-r.left)/Math.max(1,r.width)))*(frames-1)}
  track.addEventListener('pointerdown',function(e){e.stopPropagation();e.preventDefault();drag=true;try{track.setPointerCapture(e.pointerId)}catch(x){}v.pause();seek(at(e))});
  track.addEventListener('pointermove',function(e){if(drag)seek(at(e))});
  track.addEventListener('pointerup',function(){drag=false});track.addEventListener('pointercancel',function(){drag=false});
  // (on a canvas the page pans with a drag: not one that starts on the player)
  ['pointerdown','mousedown','wheel','dblclick'].forEach(function(t){[bar,cap].forEach(function(x){x.addEventListener(t,function(e){e.stopPropagation()})})});
  stage.addEventListener('pointerdown',function(e){e.stopPropagation()});
  stage.addEventListener('click',function(e){e.stopPropagation();vp.focus();toggle()});
  vp.addEventListener('keydown',function(e){var k=e.key;if(e.metaKey||e.ctrlKey||e.altKey)return;
    if(k===' '||k==='k'){toggle()}else if(k==='ArrowLeft'||k===','){step(-1)}else if(k==='ArrowRight'||k==='.'){step(1)}else if(k==='a'||k==='A'){ink()}else if(k==='m'||k==='M'){v.muted=!v.muted;paint()}else if(k==='f'||k==='F'){full()}else if(k==='['){hop(-1)}else if(k===']'){hop(1)}else return;
    e.preventDefault();e.stopPropagation()});
  ['play','pause','seeked','timeupdate','loadedmetadata','volumechange','ended'].forEach(function(t){v.addEventListener(t,function(){paint()})});
  (function loop(){if(!v.paused&&!v.ended)paint();if(v.requestVideoFrameCallback)v.requestVideoFrameCallback(loop);else requestAnimationFrame(loop)})();
  // the notes' table: a row goes to its frame (its picture no longer opens alone)
  notes.forEach(function(n){n.tr.addEventListener('click',function(e){if(!ready)return;if(e.target.closest&&e.target.closest('a,button'))return;e.preventDefault();e.stopPropagation();v.pause();seek(n.f);if(vp.scrollIntoView)vp.scrollIntoView({block:'nearest'})},true)});
  function live(){if(ready)return;ready=true;if(poster)poster.style.display='none';fig.insertBefore(vp,fig.firstChild);fig.classList.add('live');paint(0)}
  v.addEventListener('loadedmetadata',live);
  v.addEventListener('error',function(){if(ready){ready=false;fig.classList.remove('live');if(vp.parentNode)vp.parentNode.removeChild(vp);if(poster)poster.style.display=''}});
  v.src=fig.getAttribute('data-video');
});
})();
"#;

#[cfg(test)]
mod tests {
    use super::*;
    use mn_doc::Kind as K;
    struct Fake;
    impl Source for Fake { fn item(&mut self, d: &str, _: Mode, _: Opts) -> Item {
        let file = d.contains("\"pdf\"");
        Item { what: if file { What::Pdf } else { What::Picture }, name: if file { "plan one.pdf".into() } else { "shot.png".into() }, path: if file { std::env::temp_dir().join("mn-site-test-plan.pdf").to_string_lossy().into_owned() } else { String::new() }, meta: String::new(), dw: None, problem: None,
            shots: if file { vec![] } else { vec![super::super::Shot { pic: Some(super::super::Pic { bytes: vec![1, 2, 3], mime: "image/png", ext: "png", w: 4, h: 2 }), head: "shot.png".into(), ..Default::default() }] }, notes: vec![], href: None, video: None } } }
    fn note(blocks: &[(K, &str)]) -> Doc { let mut d = Doc::new(); for (i, (k, t)) in blocks.iter().enumerate() { d.insert(None, i, BlockId::new(), k.clone(), *t).unwrap(); } d }
    fn dir(name: &str) -> PathBuf { let d = std::env::temp_dir().join(format!("mn-site-{name}-{}", std::process::id())); let _ = std::fs::remove_dir_all(&d); d }

    #[test]
    fn several_notes_become_pages_with_one_look_a_list_and_each_picture_once() {
        std::fs::write(std::env::temp_dir().join("mn-site-test-plan.pdf"), b"%PDF").unwrap();
        let (f, a, b) = (BlockId::new(), BlockId::new(), BlockId::new()); let pic = K::Media { descriptor: "{\"src\":\"a.png\"}".into() };
        let job = Job { dest: dir("several"), opts: Opts::default(), files: true, videos: true, password: None, name: "Show & Tell".into(), when: "now".into(), cards: HashMap::new(), pages: vec![
            Page { id: f, parent: None, title: "Shots".into(), kind: Kind::Folder },
            Page { id: a, parent: Some(f), title: "Shot 010 / notes".into(), kind: Kind::Note(note(&[(K::Paragraph, "the quick brown fox"), (pic.clone(), ""), (K::Media { descriptor: "{\"kind\":\"pdf\"}".into() }, "")])) },
            Page { id: b, parent: Some(f), title: "Shot 010 / notes".into(), kind: Kind::Note(note(&[(K::Code { lang: "rs".into() }, "let a = 1;"), (pic, "")])) }] };
        let p = Progress::new(job.pages.len()); write(&job, &mut Fake, None, &vec![], &p, &|| {}).unwrap();
        let read = |n: &str| std::fs::read_to_string(job.dest.join(n)).unwrap();
        let (one, two, index, data) = (read("notes/shot-010-notes.html"), read("notes/shot-010-notes-2.html"), read("index.html"), read("assets/data.js"));
        assert!(one.contains("href=\"../assets/site.css\"") && one.contains("the quick brown fox") && !one.contains("base64") && one.contains("src=\"../media/") && one.contains("<a class=\"name\" href=\"../media/files/plan%20one.pdf\">"));
        assert!(two.contains("class=\"copy\"") && two.contains("navigator.clipboard") && two.contains("data-root=\"../\""));
        assert!(index.contains("Show &amp; Tell") && index.contains("<span class=\"f\">Shots</span><ul><li><a href=\"notes/shot-010-notes.html\">") && index.contains("src=\"assets/site.js\""));
        assert!(data.starts_with("window.MN={") && data.contains("the quick brown fox") && data.contains("\"href\":\"notes/shot-010-notes-2.html\"") && data.contains("\"kind\":\"folder\""));
        assert_eq!(std::fs::read_dir(job.dest.join("media")).unwrap().flatten().filter(|e| e.path().is_file()).count(), 1, "one picture, shown twice");
        assert_eq!((std::fs::read(job.dest.join("media/files/plan one.pdf")).unwrap(), p.count(DONE), p.media.load(Relaxed)), (b"%PDF".to_vec(), 2, 2));
        let _ = std::fs::remove_dir_all(&job.dest);
    }
    #[test]
    fn one_note_is_the_index_and_only_our_own_folder_is_replaced() {
        let a = BlockId::new(); let d = dir("one");
        let job = Job { dest: d.clone(), opts: Opts::default(), files: false, videos: false, password: None, name: "C".into(), when: "now".into(), cards: HashMap::new(), pages: vec![Page { id: a, parent: None, title: "Only".into(), kind: Kind::Note(note(&[(K::Paragraph, "alone")])) }] };
        write(&job, &mut Fake, None, &vec![], &Progress::new(1), &|| {}).unwrap();
        let index = std::fs::read_to_string(d.join("index.html")).unwrap();
        assert!(index.contains("alone") && index.contains("href=\"assets/site.css\"") && !index.contains("site.js") && !d.join("notes").exists() && !d.join("assets/data.js").exists());
        // again over itself: replaced; a file of the user's beside our own stays
        std::fs::write(d.join("mine.txt"), "keep").unwrap(); std::fs::create_dir_all(d.join("notes")).unwrap(); std::fs::write(d.join("notes/old.html"), "old").unwrap();
        assert_eq!(dest_state(&d), Dest::Ours); write(&job, &mut Fake, None, &vec![], &Progress::new(1), &|| {}).unwrap();
        assert!(d.join("mine.txt").exists() && !d.join("notes").exists());
        // a folder of other things is refused, and left as it was
        std::fs::remove_file(d.join(MARKER)).unwrap(); assert_eq!(dest_state(&d), Dest::Other);
        assert!(write(&job, &mut Fake, None, &vec![], &Progress::new(1), &|| {}).is_err() && d.join("mine.txt").exists() && d.join("index.html").exists());
        let _ = std::fs::remove_dir_all(&d);
    }
    #[test]
    fn a_video_gets_a_playable_copy_made_once_and_one_that_cannot_be_made_stays_stills() {
        struct Vids; impl Source for Vids { fn item(&mut self, d: &str, _: Mode, _: Opts) -> Item {
            let mut note = super::super::Shot { head: "00:00:01:00".into(), sub: "frame 24".into(), frame: Some(24), done: Some(false), ..Default::default() }; note.text = "dark".into();
            Item { what: What::Video, name: if d.contains("hdr") { "hdr.mov".into() } else { "clip.mov".into() }, path: std::env::temp_dir().join(if d.contains("hdr") { "mn-site-test-hdr.mov" } else { "mn-site-test-clip.mov" }).to_string_lossy().into_owned(), meta: String::new(), dw: None, problem: None, shots: vec![], notes: vec![note], href: None, video: None } } }
        struct Maker(usize); impl Videos for Maker {
            fn plan(&mut self, p: &Path) -> Result<Clip, String> { if p.to_string_lossy().contains("hdr") { Err("An HDR picture".into()) } else { Ok(Clip { fps: 25.0, frames: 150, copy: false, key: "k1".into() }) } }
            fn make(&mut self, _: &Path, _: &Clip, out: &Path, _: &AtomicBool, done: &dyn Fn(f32)) -> Result<(), String> { self.0 += 1; done(0.5); std::fs::write(out, b"mp4").map_err(|e| e.to_string()) } }
        for n in ["mn-site-test-hdr.mov", "mn-site-test-clip.mov"] { std::fs::write(std::env::temp_dir().join(n), b"x").unwrap(); }
        let d = dir("video"); let vid = |s: &str| K::Media { descriptor: format!("{{\"kind\":\"video\",\"src\":\"{s}\"}}") };
        let job = Job { dest: d.clone(), opts: Opts::default(), files: false, videos: true, password: None, name: "C".into(), when: "now".into(), cards: HashMap::new(), pages: vec![Page { id: BlockId::new(), parent: None, title: "Only".into(), kind: Kind::Note(note(&[(vid("clip"), ""), (vid("hdr"), ""), (vid("clip"), "")])) }] };
        let mut m = Maker(0); let p = Progress::new(1); write(&job, &mut Vids, Some(&mut m), &vec![], &p, &|| {}).unwrap();
        let page = std::fs::read_to_string(d.join("index.html")).unwrap(); let rows = p.videos.lock().unwrap().clone();
        assert_eq!((m.0, rows.len(), rows[0].state, rows[1].state, rows[1].why.as_str()), (1, 2, DONE, SKIPPED, "An HDR picture"));
        assert!(page.matches(" data-video=\"media/video/").count() == 2 && page.contains("data-fps=\"25\" data-frames=\"150\"") && page.contains("<tr class=\"open\" data-frame=\"24\">") && page.contains("src=\"assets/video.js\"") && d.join("assets/video.js").exists());
        let made: Vec<_> = std::fs::read_dir(d.join("media/video")).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect(); assert!(made.len() == 1 && made[0].ends_with(".mp4"));
        // again: the copy is kept, not made again; without videos it goes, and the page shows stills
        let p = Progress::new(1); write(&job, &mut Vids, Some(&mut m), &vec![], &p, &|| {}).unwrap(); assert_eq!((m.0, p.videos.lock().unwrap()[0].state), (1, KEPT));
        let job = Job { videos: false, ..job }; write(&job, &mut Vids, Some(&mut m), &vec![], &Progress::new(1), &|| {}).unwrap();
        assert!(!std::fs::read_to_string(d.join("index.html")).unwrap().contains("data-video") && std::fs::read_dir(d.join("media/video")).map(|l| l.count()).unwrap_or(0) == 0);
        let _ = std::fs::remove_dir_all(&d);
    }
    /// What a page's lock opens with the password: its sealed content.
    fn opened(page: &str, password: &str, dir: &Path) -> Option<serde_json::Value> {
        use base64::Engine; use ring::aead;
        let (salt, _) = marker_read(dir); let l = Sealer::new(password, salt).unwrap();
        let sealed = page.split("id=\"mnc\">").nth(1)?.split("</script>").next()?; let mut all = base64::engine::general_purpose::STANDARD.decode(sealed).ok()?;
        let iv: [u8; 12] = all[..12].try_into().ok()?; let k = aead::LessSafeKey::new(aead::UnboundKey::new(&aead::AES_256_GCM, &l.key).ok()?);
        let plain = k.open_in_place(aead::Nonce::assume_unique_for_key(iv), aead::Aad::empty(), &mut all[12..]).ok()?; serde_json::from_slice(plain).ok()
    }
    #[test]
    fn with_a_password_nothing_readable_is_written_but_media_and_the_same_password_keeps_what_was_made() {
        struct Vids; impl Source for Vids { fn item(&mut self, d: &str, _: Mode, _: Opts) -> Item {
            if d.contains("video") { Item { what: What::Video, name: "secret clip.mov".into(), path: std::env::temp_dir().join("mn-site-test-clip.mov").to_string_lossy().into_owned(), meta: String::new(), dw: None, problem: None, shots: vec![], notes: vec![], href: None, video: None } }
            else { Item { what: What::Picture, name: "shot.png".into(), path: String::new(), meta: String::new(), dw: None, problem: None, shots: vec![super::super::Shot { pic: Some(super::super::Pic { bytes: vec![1, 2, 3], mime: "image/png", ext: "png", w: 4, h: 2 }), ..Default::default() }], notes: vec![], href: None, video: None } } } }
        struct Maker(usize); impl Videos for Maker {
            fn plan(&mut self, _: &Path) -> Result<Clip, String> { Ok(Clip { fps: 25.0, frames: 150, copy: false, key: "k1".into() }) }
            fn make(&mut self, _: &Path, _: &Clip, out: &Path, _: &AtomicBool, _: &dyn Fn(f32)) -> Result<(), String> { self.0 += 1; std::fs::write(out, b"mp4").map_err(|e| e.to_string()) } }
        std::fs::write(std::env::temp_dir().join("mn-site-test-clip.mov"), b"x").unwrap();
        let d = dir("sealed"); let (a, b) = (BlockId::new(), BlockId::new());
        let pages = || vec![Page { id: a, parent: None, title: "Budget overrun".into(), kind: Kind::Note(note(&[(K::Paragraph, "the client is unhappy"), (K::Media { descriptor: "{\"src\":\"a.png\"}".into() }, ""), (K::Media { descriptor: "{\"kind\":\"video\"}".into() }, "")])) },
            Page { id: b, parent: None, title: "Second".into(), kind: Kind::Note(note(&[(K::Paragraph, "more")])) }];
        let job = |pw: Option<&str>| Job { dest: d.clone(), opts: Opts::default(), files: false, videos: true, password: pw.map(str::to_string), name: "Top Secret".into(), when: "now".into(), cards: HashMap::new(), pages: pages() };
        let mut m = Maker(0); write(&job(Some("pw one")), &mut Vids, Some(&mut m), &vec![], &Progress::new(2), &|| {}).unwrap();
        // nothing that can be read says what the notes are: not a file's name, not a file's text (the stylesheet and the scripts are everyone's)
        let mut files = vec![]; fn all(p: &Path, out: &mut Vec<PathBuf>) { for e in std::fs::read_dir(p).unwrap().flatten() { if e.path().is_dir() { all(&e.path(), out) } else { out.push(e.path()) } } } all(&d, &mut files);
        for f in &files { let name = f.strip_prefix(&d).unwrap().to_string_lossy().to_lowercase(); assert!(!name.contains("budget") && !name.contains("secret") && !name.contains("second"), "{name}");
            let t = String::from_utf8_lossy(&std::fs::read(f).unwrap()).to_lowercase(); for w in ["budget", "unhappy", "top secret", "secret clip"] { assert!(!t.contains(w), "{name} says {w}"); } }
        let index = std::fs::read_to_string(d.join("index.html")).unwrap(); assert!(index.contains("<title>Protected notes</title>") && index.contains("id=\"lock\"") && index.contains("assets/lock.js"));
        let note_page = files.iter().find(|f| f.parent().is_some_and(|p| p.ends_with("notes")) && f.file_name().is_some_and(|n| n != "index.html")).map(|f| std::fs::read_to_string(f).unwrap()).unwrap();
        // the password opens them; another does not
        let got = [&index, &note_page].iter().filter_map(|pg| opened(pg, "pw one", &d)).collect::<Vec<_>>(); assert_eq!(got.len(), 2);
        assert!(got.iter().any(|v| v["t"] == "Budget overrun" && v["h"].as_str().unwrap().contains("the client is unhappy") && v["h"].as_str().unwrap().contains("../media/video/") && v["s"][0] == "assets/video.js") || got.iter().any(|v| v["t"] == "Second"));
        assert!(opened(&index, "pw two", &d).is_none());
        for sub in ["media", "media/video", "notes", "assets"] { assert!(d.join(sub).join("index.html").exists(), "{sub} can be listed"); }
        let video = |d: &Path| std::fs::read_dir(d.join("media/video")).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).filter(|n| n.ends_with(".mp4")).collect::<Vec<_>>();
        let first = video(&d); assert_eq!(first.len(), 1);
        // the same password again: the same names, nothing made again; another password: other names, still nothing made again; none: plain
        write(&job(Some("pw one")), &mut Vids, Some(&mut m), &vec![], &Progress::new(2), &|| {}).unwrap(); assert_eq!((m.0, video(&d)), (1, first.clone()));
        write(&job(Some("pw two")), &mut Vids, Some(&mut m), &vec![], &Progress::new(2), &|| {}).unwrap(); let second = video(&d); assert!(m.0 == 1 && second.len() == 1 && second != first);
        assert!(opened(&std::fs::read_to_string(d.join("index.html")).unwrap(), "pw two", &d).is_some());
        write(&job(None), &mut Vids, Some(&mut m), &vec![], &Progress::new(2), &|| {}).unwrap(); assert!(m.0 == 1 && video(&d).len() == 1 && d.join("notes/budget-overrun.html").exists() && !d.join("assets/lock.js").exists() && marker_read(&d).0.is_none());
        let _ = std::fs::remove_dir_all(&d);
    }
    #[test]
    fn a_stop_ends_the_export_between_pages() {
        let d = dir("stop"); let pages = (0..3).map(|i| Page { id: BlockId::new(), parent: None, title: format!("n{i}"), kind: Kind::Note(note(&[(K::Paragraph, "x")])) }).collect();
        let job = Job { dest: d.clone(), opts: Opts::default(), files: false, videos: false, password: None, name: "C".into(), when: "now".into(), cards: HashMap::new(), pages };
        let p = Progress::new(3); p.stop.store(true, Relaxed);
        assert_eq!(write(&job, &mut Fake, None, &vec![], &p, &|| {}), Err("Stopped".into())); assert_eq!((p.count(DONE), dest_state(&d)), (0, Dest::Ours));
        let _ = std::fs::remove_dir_all(&d);
    }
}
