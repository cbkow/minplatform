//! `TimelineThumbnailCache` — ported from `QCView-Player/src/timeline/timeline_thumbnail_cache.{h,cpp}` with its video
//! loader `decode/video_image_loader.cpp`: the hover thumbnails over the timeline. Keys = (source path, source frame);
//! thumbs fit 320 × 180; an LRU of 100 GPU textures; three worker threads idle, ONE while playing (the others park on a
//! condvar and re-queue their task at the front); misses queue at the FRONT (the hover key is the priority), prefetch
//! (`precache_range`, stride 6) at the back; a miss answers with the nearest cached frame of the same source, else the
//! nearest numbered sibling file (image sequences); workers produce pixels + a texture, the main thread adopts ≤ 64 per
//! tick (`process_pending_uploads`, QCView's 33 ms drain timer) and evicts past the budget; `clear` bumps a generation
//! so in-flight results are dropped. The loader is a trait: video = QCView's `VideoImageLoader` (an own software session,
//! `thread_count` 1 + `FLAG2_FAST`, seek BACKWARD + flush, the frame NEAREST the target pts at (frame + 0.5) / fps,
//! `SWS_POINT` to RGBA8 fitting the max side, the legal-range expansion for RGB sources); stills (EXR / TIFF / PNG / JPEG,
//! the `?layer=` key suffix) arrive with `min-image` (2c). Textures are the caller's: workers hand back pixels, the main
//! thread's adopt closure makes the texture and returns its handle, `on_evict` releases it (QCView's 256 MB pools).
//!
//! Three measured additions for MOUSE hover (2026-11-07, the user's point: most hovering is far from the playhead):
//! the hover miss is ONE latest-wins slot (QCView queued every miss at the front and never dropped one — a two-second
//! sweep over a 4K H.264 timeline queued a hundred decodes the workers ground through for minutes); `precache_strip` on
//! open gives the whole file a thumbnail every `frames / 400` frames (floor 6) so every hover shows the nearest one
//! through QCView's own fallback while the exact one loads (the budget is 500 entries = 115 MB at 320 × 180, inside
//! QCView's 256 MB pool; QCView's 100 never held a strip); the video loader runs slice threads for intra codecs and frame
//! threads at half the cores for inter ones (QCView: one thread; 4K H.264 misses were 0.6 s). `offer` lets the scrub
//! decoder's GOP cache answer first (`ScrubDecoder::peek_thumbnail`).
use crate::ffmpeg::{OpenOptions, Read, RecvRaw, Session};
use ffmpeg_sys_next as ff;
use std::{collections::{BTreeMap, HashMap, HashSet, VecDeque}, sync::{atomic::{AtomicBool, AtomicU64, Ordering}, Arc, Condvar, Mutex}, thread};

pub const MAX_WORKER_THREADS: usize = 3;
pub const PLAYBACK_WORKER_THREADS: usize = 1;
pub const MAX_UPLOADS_PER_FRAME: usize = 64;
/// QCView's drain timer (33 ms, ~30 Hz) and the panel's hover request throttle (16 ms).
pub const UPLOAD_DRAIN_MS: u64 = 33;
pub const HOVER_REQUEST_THROTTLE_MS: u64 = 16;
pub const PREFETCH_STRIDE: i32 = 6;
/// `SWS_POINT` (swscale.h): nearest-neighbour — fast for thumbs
const SWS_POINT: i32 = 0x10;

#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ThumbKey { pub source_path: String, pub source_frame: i32 }
#[derive(Clone, Copy, Debug)]
pub struct ThumbConfig { pub width: u32, pub height: u32, pub cache_size: usize, pub enabled: bool }
impl Default for ThumbConfig { fn default() -> Self { ThumbConfig { width: 320, height: 180, cache_size: 500, enabled: true } } }
/// QCView's budget, kept for the comparison runs.
pub const QCVIEW_CACHE_SIZE: usize = 100;
/// The strip: about this many thumbnails across the file, never denser than `PREFETCH_STRIDE`.
pub const STRIP_TARGET: i64 = 400;
/// RGBA8, tightly packed.
/// A thumbnail's samples, interleaved RGB at the source's precision (canon row 216: the chain needs the file's own values —
/// 8-bit RGBA with a curve baked in banded log and lost linear shadows).
#[derive(Clone, Debug, PartialEq)]
pub enum ThumbSamples {
    /// the source's code values (R'G'B', full range), scaled to 16 bits
    Code16(Vec<u16>),
    /// linear light, IEEE half bits (EXR)
    Half(Vec<u16>),
}
#[derive(Clone, Debug, PartialEq)]
pub struct ThumbPixels { pub width: u32, pub height: u32, pub samples: ThumbSamples }
impl ThumbPixels {
    /// 8-bit code values as RGBA (opaque) — 8-bit sources exactly.
    pub fn from_rgba8(width: u32, height: u32, rgba: &[u8]) -> ThumbPixels {
        ThumbPixels { width, height, samples: ThumbSamples::Code16(rgba.chunks(4).flat_map(|c| [c[0] as u16 * 257, c[1] as u16 * 257, c[2] as u16 * 257]).collect()) }
    }
    /// The picture without the look stage: code values as stored; half floats through the sRGB curve (the old rule, so a
    /// fallback reads like the viewport's bypass).
    pub fn rgba8(&self) -> Vec<u8> {
        match &self.samples {
            ThumbSamples::Code16(v) => v.chunks(3).flat_map(|c| [(c[0] >> 8) as u8, (c[1] >> 8) as u8, (c[2] >> 8) as u8, 255]).collect(),
            ThumbSamples::Half(v) => v.chunks(3).flat_map(|c| { let e = |b: u16| { let v = half::f16::from_bits(b).to_f32(); let v = if v.is_nan() { 0.0 } else { v.clamp(0.0, 1.0) }; ((if v <= 0.003_130_8 { v * 12.92 } else { 1.055 * v.powf(1.0 / 2.4) - 0.055 }) * 255.0 + 0.5) as u8 }; [e(c[0]), e(c[1]), e(c[2]), 255] }).collect(),
        }
    }
}
#[derive(Clone, Copy, Debug, Default)]
pub struct ThumbEntry { pub texture: u64, pub width: u32, pub height: u32 }
#[derive(Clone, Copy, Debug, Default)]
pub struct ThumbStats { pub total_cached: usize, pub cache_hits: u64, pub cache_misses: u64, pub pending_requests: usize }

/// A per-worker loader for one source (QCView keeps one loader per worker per path).
pub trait ThumbLoader: Send { fn load_thumbnail(&mut self, source_frame: i32, max_size: u32) -> Option<ThumbPixels>; }
/// Makes loaders; the cache calls it on a worker thread the first time a worker meets a source.
pub type LoaderFactory = Arc<dyn Fn(&str) -> Option<Box<dyn ThumbLoader>> + Send + Sync>;

struct Request { key: ThumbKey, _high_priority: bool }
struct Queue { requests: VecDeque<Request>, queued: HashSet<ThumbKey>, in_progress: HashSet<ThumbKey>, current: HashMap<usize, ThumbKey>, /// the hover's latest-wins slot, served before the queue
    hover: Option<ThumbKey> }
struct PendingUpload { key: ThumbKey, pixels: ThumbPixels }
struct Shared {
    queue: Mutex<Queue>, queue_cv: Condvar, worker_cv: Condvar, running: AtomicBool, is_playing: AtomicBool, clear_generation: AtomicU64,
    uploads: Mutex<VecDeque<PendingUpload>>, factory: LoaderFactory, max_thumb: u32,
}
impl Shared { fn should_worker_run(&self, id: usize) -> bool { id < PLAYBACK_WORKER_THREADS || !self.is_playing.load(Ordering::Acquire) } }

pub struct TimelineThumbnailCache {
    config: ThumbConfig, shared: Arc<Shared>, workers: Vec<thread::JoinHandle<()>>,
    cache: BTreeMap<ThumbKey, ThumbEntry>, lru: VecDeque<ThumbKey>, hits: u64, misses: u64, clear_pending: bool,
    on_evict: Box<dyn FnMut(u64)>,
}

impl TimelineThumbnailCache {
    /// `factory` makes the loaders (`VideoThumbLoader::factory()` for video); `on_evict` releases a texture handle.
    pub fn new(config: ThumbConfig, factory: LoaderFactory, on_evict: Box<dyn FnMut(u64)>) -> TimelineThumbnailCache {
        let shared = Arc::new(Shared { queue: Mutex::new(Queue { requests: VecDeque::new(), queued: HashSet::new(), in_progress: HashSet::new(), current: HashMap::new(), hover: None }), queue_cv: Condvar::new(), worker_cv: Condvar::new(), running: AtomicBool::new(true), is_playing: AtomicBool::new(false), clear_generation: AtomicU64::new(0), uploads: Mutex::new(VecDeque::new()), factory, max_thumb: config.width.max(config.height) });
        let workers = (0..MAX_WORKER_THREADS).map(|id| { let sh = shared.clone(); thread::Builder::new().name(format!("qcv-thumb-{id}")).spawn(move || worker(sh, id)).expect("thumbnail worker") }).collect();
        log::info!("TimelineThumbnailCache: initialized workers={MAX_WORKER_THREADS}");
        TimelineThumbnailCache { config, shared, workers, cache: BTreeMap::new(), lru: VecDeque::new(), hits: 0, misses: 0, clear_pending: false, on_evict }
    }
    /// A cached texture for (path, frame), queueing a miss at the front; with `allow_fallback` the nearest cached frame of the
    /// source, else the nearest numbered sibling. 0 = nothing yet.
    pub fn get_thumbnail(&mut self, source_path: &str, source_frame: i32, allow_fallback: bool) -> ThumbEntry {
        if !self.config.enabled || self.clear_pending { return ThumbEntry::default() }
        let key = ThumbKey { source_path: source_path.into(), source_frame };
        if let Some(e) = self.cache.get(&key).copied() { self.hits += 1; self.touch(&key); return e }
        self.misses += 1;
        { let mut q = self.shared.queue.lock().unwrap(); if !q.in_progress.contains(&key) && !q.queued.contains(&key) { q.hover = Some(key.clone()); self.shared.queue_cv.notify_one(); } }   // latest wins: a newer hover replaces this one before a worker starts it
        if allow_fallback {
            if let Some(near) = self.find_nearest_cached_frame(source_path, source_frame) { if let Some(e) = self.cache.get(&ThumbKey { source_path: source_path.into(), source_frame: near }) { return *e } }
            if let Some(sib) = self.find_nearest_sequence_sibling(source_path) { if let Some(e) = self.cache.get(&sib) { return *e } }
        }
        ThumbEntry::default()
    }
    /// Prefetch `start..=end` every `step` frames at the back of the queue; `priority_frame` (if inside) at the front.
    pub fn precache_range(&mut self, source_path: &str, start_frame: i32, end_frame: i32, priority_frame: i32, step: i32) {
        if !self.config.enabled { return }
        let step = step.max(1);
        let mut q = self.shared.queue.lock().unwrap();
        if priority_frame >= start_frame && priority_frame <= end_frame { let k = ThumbKey { source_path: source_path.into(), source_frame: priority_frame }; if !q.in_progress.contains(&k) && !q.queued.contains(&k) { q.requests.push_front(Request { key: k.clone(), _high_priority: true }); q.queued.insert(k); } }
        let mut f = start_frame;
        while f <= end_frame {
            if f != priority_frame { let k = ThumbKey { source_path: source_path.into(), source_frame: f }; if !self.cache.contains_key(&k) && !q.in_progress.contains(&k) && !q.queued.contains(&k) { q.requests.push_back(Request { key: k.clone(), _high_priority: false }); q.queued.insert(k); } }
            f += step;
        }
        self.shared.queue_cv.notify_one();
    }
    pub fn cancel_pending_requests(&mut self) { let mut q = self.shared.queue.lock().unwrap(); q.requests.clear(); q.queued.clear(); q.hover = None; }
    /// The strip: the whole file at a stride that scales with its length (`frames / STRIP_TARGET`, floor `PREFETCH_STRIDE`),
    /// the playhead's frame first.
    pub fn precache_strip(&mut self, source_path: &str, frames: i32, playhead: i32) { let stride = ((frames as i64 / STRIP_TARGET) as i32).max(PREFETCH_STRIDE); self.precache_range(source_path, 0, (frames - 1).max(0), playhead, stride); }
    /// Pixels made elsewhere (the scrub decoder's GOP cache): they land on the next drain like a worker's.
    pub fn offer(&mut self, key: ThumbKey, pixels: ThumbPixels) { if self.cache.contains_key(&key) { return } { let mut q = self.shared.queue.lock().unwrap(); if q.hover.as_ref() == Some(&key) { q.hover = None; } } self.shared.uploads.lock().unwrap().push_back(PendingUpload { key, pixels }); }
    /// How far the nearest cached frame of this source is from `source_frame` (the fallback's distance); None = none cached.
    pub fn nearest_cached_distance(&self, source_path: &str, source_frame: i32) -> Option<i32> { self.find_nearest_cached_frame(source_path, source_frame).map(|n| (n - source_frame).abs()) }
    pub fn contains(&self, source_path: &str, source_frame: i32) -> bool { self.cache.contains_key(&ThumbKey { source_path: source_path.into(), source_frame }) }
    /// The main thread's per-tick adopt: ≤ 64 finished thumbs become textures through `adopt(key, pixels) -> handle`; the LRU
    /// evicts past the budget. Returns how many landed.
    pub fn process_pending_uploads(&mut self, mut adopt: impl FnMut(&ThumbKey, &ThumbPixels) -> u64) -> usize {
        self.clear_pending = false;
        let mut uploads = { let mut u = self.shared.uploads.lock().unwrap(); std::mem::take(&mut *u) };
        let mut n = 0;
        while let Some(up) = uploads.pop_front() {
            if n >= MAX_UPLOADS_PER_FRAME { uploads.push_front(up); let mut u = self.shared.uploads.lock().unwrap(); while let Some(x) = uploads.pop_front() { u.push_back(x); } break }
            let tex = adopt(&up.key, &up.pixels);
            if tex != 0 {
                while self.cache.len() >= self.config.cache_size { self.evict_lru(); }
                if let Some(old) = self.cache.insert(up.key.clone(), ThumbEntry { texture: tex, width: up.pixels.width, height: up.pixels.height }) { (self.on_evict)(old.texture); self.lru.retain(|k| *k != up.key); }
                self.lru.push_front(up.key);
            }
            n += 1;
        }
        n
    }
    /// Drops everything, including results still in flight (the generation counter).
    pub fn clear(&mut self) {
        log::info!("TimelineThumbnailCache: clear");
        self.shared.clear_generation.fetch_add(1, Ordering::Release); self.clear_pending = true;
        { let mut q = self.shared.queue.lock().unwrap(); q.requests.clear(); q.queued.clear(); q.in_progress.clear(); }
        self.shared.uploads.lock().unwrap().clear();
        for (_, e) in std::mem::take(&mut self.cache) { if e.texture != 0 { (self.on_evict)(e.texture); } }
        self.lru.clear(); self.hits = 0; self.misses = 0;
    }
    /// Playing → one worker (the others park, re-queueing their task at the front); paused → all three.
    pub fn notify_playback_state(&mut self, playing: bool) {
        let was = self.shared.is_playing.swap(playing, Ordering::AcqRel);
        if playing != was { self.shared.queue_cv.notify_all(); self.shared.worker_cv.notify_all(); log::info!("TimelineThumbnailCache: {} → {} worker(s)", if playing { "playback" } else { "paused" }, if playing { PLAYBACK_WORKER_THREADS } else { MAX_WORKER_THREADS }); }
    }
    pub fn is_enabled(&self) -> bool { self.config.enabled }
    pub fn stats(&self) -> ThumbStats { let q = self.shared.queue.lock().unwrap(); ThumbStats { total_cached: self.cache.len(), cache_hits: self.hits, cache_misses: self.misses, pending_requests: q.requests.len() + q.in_progress.len() + q.hover.is_some() as usize } }
    pub fn shutdown(&mut self) {
        self.shared.running.store(false, Ordering::Release); self.shared.queue_cv.notify_all(); self.shared.worker_cv.notify_all();
        for w in self.workers.drain(..) { w.join().ok(); }
        for (_, e) in std::mem::take(&mut self.cache) { if e.texture != 0 { (self.on_evict)(e.texture); } }
        self.lru.clear();
    }
    fn touch(&mut self, key: &ThumbKey) { self.lru.retain(|k| k != key); self.lru.push_front(key.clone()); }
    fn evict_lru(&mut self) { let Some(k) = self.lru.pop_back() else { return }; if let Some(e) = self.cache.remove(&k) { if e.texture != 0 { (self.on_evict)(e.texture); } } }
    fn find_nearest_cached_frame(&self, path: &str, target: i32) -> Option<i32> { self.cache.keys().filter(|k| k.source_path == path).map(|k| k.source_frame).min_by_key(|f| (f - target).abs()) }
    fn find_nearest_sequence_sibling(&self, target_path: &str) -> Option<ThumbKey> {
        let (prefix, number, suffix) = parse_sequence_parts(target_path)?;
        let mut best: Option<(i64, &ThumbKey)> = None;
        for k in self.cache.keys() {
            let p = k.source_path.as_str();
            if p == target_path || !p.starts_with(&prefix) || !p.ends_with(&suffix) || p.len() <= prefix.len() + suffix.len() { continue }
            let digits = &p[prefix.len()..p.len() - suffix.len()];
            let Ok(n) = digits.parse::<i64>() else { continue };
            if !digits.bytes().all(|b| b.is_ascii_digit()) { continue }
            let d = (n - number).abs();
            if best.map(|(bd, _)| d < bd).unwrap_or(true) { best = Some((d, k)); }
        }
        best.map(|(_, k)| k.clone())
    }
}
impl Drop for TimelineThumbnailCache { fn drop(&mut self) { self.shutdown(); } }

/// "…/name0042.exr?layer=x" → ("…/name", 42, ".exr?layer=x"); None without trailing digits in the stem.
fn parse_sequence_parts(path: &str) -> Option<(String, i64, String)> {
    let (file, key_suffix) = match path.find("?layer=") { Some(q) => (&path[..q], &path[q..]), None => (path, "") };
    let dot = file.rfind('.')?;
    let (stem, ext) = (&file[..dot], &file[dot..]);
    let digits_start = stem.bytes().rposition(|b| !b.is_ascii_digit()).map(|p| p + 1).unwrap_or(0);
    if digits_start == stem.len() { return None }
    let number = stem[digits_start..].parse::<i64>().ok()?;
    Some((stem[..digits_start].to_string(), number, format!("{ext}{key_suffix}")))
}

fn worker(sh: Arc<Shared>, id: usize) {
    // The strip precache after open runs 3 workers × the loaders' FFmpeg threads: at default priority it starved the UI
    // thread (130-ms present stalls, dropped frames during the user's first audio walk, 2026-11-07). Utility QoS, as
    // `read_ahead` does: the thumbnails yield to the picture and the device callback.
    #[cfg(target_os = "macos")]
    unsafe { unsafe extern "C" { fn pthread_set_qos_class_self_np(qos: u32, relative_priority: i32) -> i32; } pthread_set_qos_class_self_np(0x09, 0); }
    let mut loaders: HashMap<String, Box<dyn ThumbLoader>> = HashMap::new();
    while sh.running.load(Ordering::Acquire) {
        let req = {
            let mut q = sh.queue.lock().unwrap();
            q = sh.queue_cv.wait_while(q, |q| sh.running.load(Ordering::Acquire) && ((q.requests.is_empty() && q.hover.is_none()) || !sh.should_worker_run(id))).unwrap();
            if !sh.running.load(Ordering::Acquire) { break }
            if !sh.should_worker_run(id) {
                // park for playback; our in-flight task (if any) goes back to the front
                if let Some(k) = q.current.remove(&id) { q.in_progress.remove(&k); q.requests.push_front(Request { key: k, _high_priority: true }); }
                let _q = sh.worker_cv.wait_while(q, |_| sh.running.load(Ordering::Acquire) && !sh.should_worker_run(id)).unwrap();
                continue;
            }
            let r = match q.hover.take() { Some(k) => Request { key: k, _high_priority: true }, None => match q.requests.pop_front() { Some(r) => r, None => continue } };
            q.queued.remove(&r.key); q.in_progress.insert(r.key.clone()); q.current.insert(id, r.key.clone());
            r
        };
        let gen_before = sh.clear_generation.load(Ordering::Acquire);
        let pixels = {
            let path = req.key.source_path.clone();
            let loader = match loaders.get_mut(&path) { Some(l) => Some(l), None => { if let Some(l) = (sh.factory)(&path) { loaders.insert(path.clone(), l); loaders.get_mut(&path) } else { None } } };
            loader.and_then(|l| l.load_thumbnail(req.key.source_frame, sh.max_thumb))
        };
        if sh.clear_generation.load(Ordering::Acquire) != gen_before { let mut q = sh.queue.lock().unwrap(); q.in_progress.remove(&req.key); q.current.remove(&id); continue }
        if let Some(px) = pixels { if sh.running.load(Ordering::Acquire) { sh.uploads.lock().unwrap().push_back(PendingUpload { key: req.key.clone(), pixels: px }); } }
        let mut q = sh.queue.lock().unwrap(); q.in_progress.remove(&req.key); q.current.remove(&id);
    }
}

/// QCView's `VideoImageLoader`: an own software session (one thread, `FLAG2_FAST`), seek-exact to the frame nearest
/// (frame + 0.5) / fps (+ the stream's start), nearest-neighbour scale to RGBA8 fitting `max_size`.
pub struct VideoThumbLoader { session: Session, fps: f64, frames: i32, sws: *mut ff::SwsContext, sws_key: (i32, i32, i32, i32, i32), range: crate::rgb_range::RangeOverride }
unsafe impl Send for VideoThumbLoader {}
impl VideoThumbLoader {
    pub fn open(path: &str) -> Option<VideoThumbLoader> { Self::open_with(path, crate::rgb_range::RangeOverride::Auto) }
    /// The clip's Range override applies to its thumbnails as to its picture (the 2026-11-09 owed item).
    pub fn open_with(path: &str, range: crate::rgb_range::RangeOverride) -> Option<VideoThumbLoader> {
        // slice threads for intra codecs, frame threads at half the cores for inter ones (QCView: one thread; see the module doc)
        let session = Session::open(path, &OpenOptions { threads: crate::thread_policy::dual_side_thread_count(), hw: None, hw_intra: None, seq: None, fast: true }).ok()?;
        let fps = session.info.rate().as_f64(); let frames = session.info.frame_count;
        Some(VideoThumbLoader { session, fps, frames, sws: std::ptr::null_mut(), sws_key: (0, 0, 0, 0, 0), range })
    }
    pub fn factory() -> LoaderFactory { Arc::new(|key: &str| { let (path, r) = split_range_key(key); VideoThumbLoader::open_with(path, r).map(|l| Box::new(l) as Box<dyn ThumbLoader>) }) }
    /// QCView `getOrCreateLoader`: a still's key (an image-sequence frame's own file, `?layer=` for an EXR part) goes to the
    /// image loaders, anything else to the video loader.
    pub fn any_factory() -> LoaderFactory {
        Arc::new(|key: &str| {
            let (file, layer) = key.split_once("?layer=").unwrap_or((key, ""));
            if min_image::Format::of(file).is_some() { return Some(Box::new(ImageThumbLoader { path: file.to_string(), layer: layer.to_string() }) as Box<dyn ThumbLoader>) }
            let (path, r) = split_range_key(key);
            VideoThumbLoader::open_with(path, r).map(|l| Box::new(l) as Box<dyn ThumbLoader>)
        })
    }
}
/// A video's cache key with its Range override (`?range=full` / `?range=limited`; Auto = the bare path), so thumbnails
/// made under one override never answer for another — the `?layer=` pattern.
pub fn range_key(path: &str, r: crate::rgb_range::RangeOverride) -> String {
    match r { crate::rgb_range::RangeOverride::Full => format!("{path}?range=full"), crate::rgb_range::RangeOverride::Limited => format!("{path}?range=limited"), _ => path.to_string() }
}
pub fn split_range_key(key: &str) -> (&str, crate::rgb_range::RangeOverride) {
    match key.rsplit_once("?range=") { Some((p, "full")) => (p, crate::rgb_range::RangeOverride::Full), Some((p, "limited")) => (p, crate::rgb_range::RangeOverride::Limited), _ => (key, crate::rgb_range::RangeOverride::Auto) }
}

impl Drop for VideoThumbLoader { fn drop(&mut self) { unsafe { if !self.sws.is_null() { ff::sws_freeContext(self.sws); } } } }
impl ThumbLoader for VideoThumbLoader {
    fn load_thumbnail(&mut self, source_frame: i32, max_size: u32) -> Option<ThumbPixels> {
        if self.fps <= 0.0 { return None }
        let f = source_frame.clamp(0, (self.frames - 1).max(0));
        let mut timestamp = (f as f64 + 0.5) / self.fps;
        let start_us = self.session.pts_to_us(self.session.start_pts());
        if start_us > 0 { timestamp += start_us as f64 / 1e6; }
        let target_pts = self.session.us_to_pts((timestamp * 1e6) as i64);
        if self.session.seek(target_pts, ff::AVSEEK_FLAG_BACKWARD) < 0 { return None }
        // decode forward, keeping the frame NEAREST the target; stop once a frame past it has been seen
        let (mut best, mut best_diff, mut passed) = (std::ptr::null_mut::<ff::AVFrame>(), i64::MAX, false);
        let mut eof = false;
        'outer: while !eof {
            match self.session.read_and_send() { Read::Sent => {}, Read::Eof => eof = true, Read::Error(_) => break }
            loop {
                match self.session.receive_raw() {
                    RecvRaw::Again => break, RecvRaw::Eof | RecvRaw::Error(_) => { eof = true; break }
                    RecvRaw::Frame(fr) => unsafe {
                        let pts = if (*fr).pts != ff::AV_NOPTS_VALUE { (*fr).pts } else { (*fr).best_effort_timestamp };
                        let diff = (pts - target_pts).abs();
                        if diff < best_diff { best_diff = diff; if !best.is_null() { ff::av_frame_free(&mut best); } best = ff::av_frame_clone(fr); }
                        if pts > target_pts { passed = true; }
                        if passed && !best.is_null() { self.session.release_raw(); break 'outer }
                    }
                }
            }
        }
        if best.is_null() { return None }
        let out = unsafe { let mut c = VideoThumbLoaderSws { sws: self.sws, sws_key: self.sws_key, range: self.range }; let o = c.convert(best, max_size); self.sws = c.sws; self.sws_key = c.sws_key; o };
        unsafe { ff::av_frame_free(&mut best); }
        out
    }
}
/// An RGBA8 thumbnail from any planar AVFrame with a one-shot swscale context (the scrub cache's peek path).
pub unsafe fn rgba_thumb_from_avframe(fr: *mut ff::AVFrame, max_size: u32, range: crate::rgb_range::RangeOverride) -> Option<ThumbPixels> { unsafe {
    let mut tmp = VideoThumbLoaderSws { sws: std::ptr::null_mut(), sws_key: (0, 0, 0, 0, 0), range };
    let out = tmp.convert(fr, max_size);
    if !tmp.sws.is_null() { ff::sws_freeContext(tmp.sws); }
    out
} }
struct VideoThumbLoaderSws { sws: *mut ff::SwsContext, sws_key: (i32, i32, i32, i32, i32), range: crate::rgb_range::RangeOverride }
impl VideoThumbLoaderSws {
    unsafe fn convert(&mut self, fr: *mut ff::AVFrame, max_size: u32) -> Option<ThumbPixels> { unsafe {
        let (w, h) = ((*fr).width, (*fr).height);
        if w <= 0 || h <= 0 { return None }
        let (ow, oh) = if max_size > 0 && w.max(h) as u32 > max_size { let s = max_size as f32 / w.max(h) as f32; ((w as f32 * s) as i32, (h as f32 * s) as i32) } else { (w, h) };
        let (ow, oh) = (ow.max(1), oh.max(1));
        let key = (w, h, (*fr).format, ow, oh);
        if self.sws.is_null() || self.sws_key != key {
            if !self.sws.is_null() { ff::sws_freeContext(self.sws); }
            self.sws = ff::sws_getContext(w, h, std::mem::transmute::<i32, ff::AVPixelFormat>((*fr).format), ow, oh, ff::AVPixelFormat::AV_PIX_FMT_RGB48LE, SWS_POINT, std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null());
            if self.sws.is_null() { return None }
            // the screen's matrix + range rules (yuv_planar: tagged, else BT.709 at every size; the range tag, untagged →
            // limited) — swscale's own default was BT.601 with the range taken from the pixel format (canon row 216)
            let desc = ff::av_pix_fmt_desc_get(std::mem::transmute::<i32, ff::AVPixelFormat>((*fr).format));
            let is_rgb = !desc.is_null() && ((*desc).flags & ff::AV_PIX_FMT_FLAG_RGB as u64) != 0;
            if !is_rgb {
                use crate::yuv_planar::{effective_matrix, effective_source_range};
                let cs = match effective_matrix((*fr).colorspace) { min_present::Matrix::Bt601 => ff::SWS_CS_ITU601, min_present::Matrix::Smpte240m => ff::SWS_CS_SMPTE240M, min_present::Matrix::Fcc => ff::SWS_CS_FCC, min_present::Matrix::Bt2020 => ff::SWS_CS_BT2020, _ => ff::SWS_CS_ITU709 };
                let full = effective_source_range((*fr).color_range, false, self.range) == min_present::Range::Full;
                ff::sws_setColorspaceDetails(self.sws, ff::sws_getCoefficients(cs), full as i32, ff::sws_getCoefficients(ff::SWS_CS_DEFAULT), 1, 0, 1 << 16, 1 << 16);
            }
            self.sws_key = key;
        }
        let mut rgb = vec![0u16; (ow * oh * 3) as usize];
        let dst = [rgb.as_mut_ptr() as *mut u8, std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut()];
        let dst_stride = [ow * 6, 0, 0, 0];
        let src: [*const u8; 4] = [(*fr).data[0], (*fr).data[1], (*fr).data[2], (*fr).data[3]];
        let src_stride = [(*fr).linesize[0], (*fr).linesize[1], (*fr).linesize[2], (*fr).linesize[3]];
        ff::sws_scale(self.sws, src.as_ptr(), src_stride.as_ptr(), 0, h, dst.as_ptr(), dst_stride.as_ptr());
        // RGB sources tagged limited get the legal expansion swscale skips (rgb_range.h)
        for v in rgb.iter_mut() { *v = u16::from_le(*v); }
        // RGB sources tagged limited: rgb_range's 16–235 rule, at 16 bits
        if crate::rgb_range::rgb_frame_needs_legal_expansion(fr, self.range) { for v in rgb.iter_mut() { *v = (((*v as i64 - 4096) * 65535 + 28032) / 56064).clamp(0, 65535) as u16; } }
        Some(ThumbPixels { width: ow as u32, height: oh as u32, samples: ThumbSamples::Code16(rgb) })
    } }
}

/// QCView's still loaders' `loadThumbnail`: the frame's file through `min_image::load_thumbnail` (JPEG DCT scaling, PNG / DPX
/// row skipping; EXR / TIFF / TGA / BMP decode whole and are reduced here).
pub struct ImageThumbLoader { path: String, layer: String }
impl ThumbLoader for ImageThumbLoader {
    fn load_thumbnail(&mut self, _source_frame: i32, max_size: u32) -> Option<ThumbPixels> { image_thumb(&min_image::load_thumbnail(&self.path, &self.layer, max_size).ok()?, max_size) }
}

/// A decoded still reduced to a thumbnail: every Nth pixel, N = max(1, the long side / `max_size`) (QCView's EXR skip
/// rule), RGBA8. Half floats are linear light and get the sRGB curve, so the thumbnail reads like the viewport (QCView
/// shows linear values as code values — the banked colour question, canon §18.6). Sentinels have no thumbnail.
pub fn image_thumb(img: &min_image::Image, max_size: u32) -> Option<ThumbPixels> {
    if img.sentinel.is_some() || img.width == 0 || img.height == 0 || img.planes.len() < 3 { return None }
    let skip = (img.width.max(img.height) / max_size.max(1)).max(1);
    let (w, h) = ((img.width / skip).max(1), (img.height / skip).max(1));
    let bps = img.sample.bytes();
    // the planes are G, B, R (gbrp); the samples stay the file's: 8-bit → ×257, 16-bit as is, half floats as half bits
    let read = |plane: usize, x: u32, y: u32| -> u16 {
        let o = (y * img.pitch) as usize + x as usize * bps; let p = &img.planes[plane];
        match img.sample { min_image::Sample::U8 => p[o] as u16 * 257, _ => u16::from_ne_bytes([p[o], p[o + 1]]) }
    };
    let mut v = Vec::with_capacity((w * h * 3) as usize);
    for y in 0..h { for x in 0..w { let (sx, sy) = (x * skip, y * skip); v.extend_from_slice(&[read(2, sx, sy), read(0, sx, sy), read(1, sx, sy)]); } }
    Some(ThumbPixels { width: w, height: h, samples: if img.sample == min_image::Sample::F16 { ThumbSamples::Half(v) } else { ThumbSamples::Code16(v) } })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    /// `QCV_THUMB_BENCH=<dir>`: the hover thumbnail's cost per still in the folder (the ImageThumbLoader path).
    #[test] #[ignore] fn thumb_bench() {
        let Ok(dir) = std::env::var("QCV_THUMB_BENCH") else { return };
        let mut files: Vec<_> = std::fs::read_dir(dir).unwrap().filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| min_image::Format::of(&p.to_string_lossy()).is_some()).collect(); files.sort();
        for p in files {
            let p = p.to_string_lossy().into_owned(); let mut l = ImageThumbLoader { path: p.clone(), layer: String::new() };
            let _ = l.load_thumbnail(0, 320);   // warm the page cache
            let t = std::time::Instant::now(); let n = 5; for _ in 0..n { assert!(l.load_thumbnail(0, 320).is_some()); }
            eprintln!("{:>10}  {:6.1} ms per thumbnail", std::path::Path::new(&p).file_name().unwrap().to_string_lossy(), t.elapsed().as_secs_f64() * 1000.0 / n as f64);
        }
    }
    #[test] fn image_thumb_skips_and_encodes() {
        use min_image::{Image, Sample};
        // 640×360 half-float planes G, B, R: G = 0.5 linear everywhere, B = 0, R = 1.0; max 320 → every 2nd pixel
        let (w, h) = (640u32, 360u32); let pitch = min_image::pitch_for(w, Sample::F16);
        let plane = |v: f32| { let b = half::f16::from_f32(v).to_bits().to_ne_bytes(); let mut p = vec![0u8; (pitch * h) as usize]; for y in 0..h { for x in 0..w { let o = (y * pitch + x * 2) as usize; p[o] = b[0]; p[o + 1] = b[1]; } } p };
        let img = Image { width: w, height: h, sample: Sample::F16, planes: vec![plane(0.5), plane(0.0), plane(1.0)], pitch, alpha: false, sentinel: None };
        let t = image_thumb(&img, 320).unwrap();
        assert_eq!((t.width, t.height), (320, 180));
        assert_eq!(&t.rgba8()[..4], &[255, 188, 0, 255], "R 1.0, G 0.5 linear → sRGB 188, B 0, opaque");
    }
    #[test] fn sequence_parts() {
        assert_eq!(parse_sequence_parts("/s/shot_0042.exr?layer=diffuse"), Some(("/s/shot_".into(), 42, ".exr?layer=diffuse".into())));
        assert_eq!(parse_sequence_parts("/s/clip.mov"), None);
    }
    struct Fake; impl ThumbLoader for Fake { fn load_thumbnail(&mut self, f: i32, _m: u32) -> Option<ThumbPixels> { Some(ThumbPixels::from_rgba8(2, 1, &[f as u8; 8])) } }
    #[test] fn misses_queue_land_and_fall_back() {
        let factory: LoaderFactory = Arc::new(|_| Some(Box::new(Fake) as Box<dyn ThumbLoader>));
        let evicted = Arc::new(Mutex::new(vec![])); let ev = evicted.clone();
        let mut c = TimelineThumbnailCache::new(ThumbConfig { cache_size: 3, ..Default::default() }, factory, Box::new(move |t| ev.lock().unwrap().push(t)));
        assert_eq!(c.get_thumbnail("/v.mov", 10, true).texture, 0);
        let mut next = 100u64; let mut landed = 0;
        for _ in 0..200 { landed += c.process_pending_uploads(|_, _| { next += 1; next }); if landed > 0 { break } thread::sleep(Duration::from_millis(5)); }
        assert_eq!(landed, 1); assert_ne!(c.get_thumbnail("/v.mov", 10, true).texture, 0);
        assert_ne!(c.get_thumbnail("/v.mov", 12, true).texture, 0);   // a miss answered by the nearest cached (10) while 12 is queued
        c.precache_range("/v.mov", 0, 30, 12, 6);
        for _ in 0..200 { c.process_pending_uploads(|_, _| { next += 1; next }); if c.stats().pending_requests == 0 { break } thread::sleep(Duration::from_millis(5)); }
        assert_eq!(c.stats().total_cached, 3); assert!(!evicted.lock().unwrap().is_empty());   // the budget evicted
        c.clear(); assert_eq!(c.stats().total_cached, 0);
    }
}

#[cfg(test)]
mod colour_tests {
    use ffmpeg_sys_next as ff;
    /// A 16×16 yuv420p frame of one code value, tagged.
    unsafe fn frame(y: u8, cb: u8, cr: u8, cs: ff::AVColorSpace, range: ff::AVColorRange) -> *mut ff::AVFrame { unsafe {
        let f = ff::av_frame_alloc(); (*f).width = 16; (*f).height = 16; (*f).format = ff::AVPixelFormat::AV_PIX_FMT_YUV420P as i32;
        assert!(ff::av_frame_get_buffer(f, 0) >= 0);
        for (p, v, h) in [(0usize, y, 16), (1, cb, 8), (2, cr, 8)] { for row in 0..h { std::ptr::write_bytes((*f).data[p].add(row * (*f).linesize[p] as usize), v, if p == 0 { 16 } else { 8 }); } }
        (*f).colorspace = cs; (*f).color_range = range; f
    } }
    #[test]
    fn thumbnails_use_the_frames_matrix_and_range() { unsafe {
        // BT.709 limited red: Y 63, Cb 102, Cr 240 → (255, 0, 0); BT.601's table would give a pink-orange
        let mut f = frame(63, 102, 240, ff::AVColorSpace::AVCOL_SPC_BT709, ff::AVColorRange::AVCOL_RANGE_MPEG);
        let t = super::rgba_thumb_from_avframe(f, 0, crate::rgb_range::RangeOverride::Auto).unwrap();
        let q = t.rgba8(); let (r, g, b) = (q[0] as i32, q[1] as i32, q[2] as i32);
        assert!(r > 245 && g < 12 && b < 12, "709 red → ({r}, {g}, {b})");
        ff::av_frame_free(&mut f);
        // full range grey 128 stays 128 (limited would stretch it)
        let mut f = frame(128, 128, 128, ff::AVColorSpace::AVCOL_SPC_BT709, ff::AVColorRange::AVCOL_RANGE_JPEG);
        let t = super::rgba_thumb_from_avframe(f, 0, crate::rgb_range::RangeOverride::Auto).unwrap();
        assert!((t.rgba8()[0] as i32 - 128).abs() <= 2, "full-range grey → {}", t.rgba8()[0]);
        ff::av_frame_free(&mut f);
        // the clip's Range override wins over the tag (the picture's rule): limited-tagged Y 16 is black as tagged, 16 when forced Full
        let mut f = frame(16, 128, 128, ff::AVColorSpace::AVCOL_SPC_BT709, ff::AVColorRange::AVCOL_RANGE_MPEG);
        let auto = super::rgba_thumb_from_avframe(f, 0, crate::rgb_range::RangeOverride::Auto).unwrap().rgba8()[0] as i32;
        let full = super::rgba_thumb_from_avframe(f, 0, crate::rgb_range::RangeOverride::Full).unwrap().rgba8()[0] as i32;
        assert!(auto <= 2 && (full - 16).abs() <= 2, "Y 16: auto {auto}, forced full {full}");
        ff::av_frame_free(&mut f);
    } }
    #[test] fn range_keys_round_trip() {
        use crate::rgb_range::RangeOverride as R;
        for r in [R::Auto, R::Full, R::Limited] { let k = super::range_key("/m/a.mov", r); assert_eq!(super::split_range_key(&k), ("/m/a.mov", r)); }
        assert_eq!(super::range_key("/m/a.mov", R::Auto), "/m/a.mov");
    }
}
