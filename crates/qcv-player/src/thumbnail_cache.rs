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
pub struct ThumbPixels { pub width: u32, pub height: u32, pub rgba: Vec<u8> }
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
pub struct VideoThumbLoader { session: Session, fps: f64, frames: i32, sws: *mut ff::SwsContext, sws_key: (i32, i32, i32, i32, i32) }
unsafe impl Send for VideoThumbLoader {}
impl VideoThumbLoader {
    pub fn open(path: &str) -> Option<VideoThumbLoader> {
        // slice threads for intra codecs, frame threads at half the cores for inter ones (QCView: one thread; see the module doc)
        let session = Session::open(path, &OpenOptions { threads: crate::thread_policy::dual_side_thread_count(), hw: None, hw_intra: None, seq: None, fast: true }).ok()?;
        let fps = session.info.rate().as_f64(); let frames = session.info.frame_count;
        Some(VideoThumbLoader { session, fps, frames, sws: std::ptr::null_mut(), sws_key: (0, 0, 0, 0, 0) })
    }
    pub fn factory() -> LoaderFactory { Arc::new(|path: &str| VideoThumbLoader::open(path).map(|l| Box::new(l) as Box<dyn ThumbLoader>)) }
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
        let out = unsafe { let mut c = VideoThumbLoaderSws { sws: self.sws, sws_key: self.sws_key }; let o = c.convert(best, max_size); self.sws = c.sws; self.sws_key = c.sws_key; o };
        unsafe { ff::av_frame_free(&mut best); }
        out
    }
}
/// An RGBA8 thumbnail from any planar AVFrame with a one-shot swscale context (the scrub cache's peek path).
pub unsafe fn rgba_thumb_from_avframe(fr: *mut ff::AVFrame, max_size: u32) -> Option<ThumbPixels> { unsafe {
    let mut tmp = VideoThumbLoaderSws { sws: std::ptr::null_mut(), sws_key: (0, 0, 0, 0, 0) };
    let out = tmp.convert(fr, max_size);
    if !tmp.sws.is_null() { ff::sws_freeContext(tmp.sws); }
    out
} }
struct VideoThumbLoaderSws { sws: *mut ff::SwsContext, sws_key: (i32, i32, i32, i32, i32) }
impl VideoThumbLoaderSws {
    unsafe fn convert(&mut self, fr: *mut ff::AVFrame, max_size: u32) -> Option<ThumbPixels> { unsafe {
        let (w, h) = ((*fr).width, (*fr).height);
        if w <= 0 || h <= 0 { return None }
        let (ow, oh) = if max_size > 0 && w.max(h) as u32 > max_size { let s = max_size as f32 / w.max(h) as f32; ((w as f32 * s) as i32, (h as f32 * s) as i32) } else { (w, h) };
        let (ow, oh) = (ow.max(1), oh.max(1));
        let key = (w, h, (*fr).format, ow, oh);
        if self.sws.is_null() || self.sws_key != key {
            if !self.sws.is_null() { ff::sws_freeContext(self.sws); }
            self.sws = ff::sws_getContext(w, h, std::mem::transmute::<i32, ff::AVPixelFormat>((*fr).format), ow, oh, ff::AVPixelFormat::AV_PIX_FMT_RGBA, SWS_POINT, std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null());
            if self.sws.is_null() { return None }
            self.sws_key = key;
        }
        let mut rgba = vec![0u8; (ow * oh * 4) as usize];
        let dst = [rgba.as_mut_ptr(), std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut()];
        let dst_stride = [ow * 4, 0, 0, 0];
        let src: [*const u8; 4] = [(*fr).data[0], (*fr).data[1], (*fr).data[2], (*fr).data[3]];
        let src_stride = [(*fr).linesize[0], (*fr).linesize[1], (*fr).linesize[2], (*fr).linesize[3]];
        ff::sws_scale(self.sws, src.as_ptr(), src_stride.as_ptr(), 0, h, dst.as_ptr(), dst_stride.as_ptr());
        // RGB sources tagged limited get the legal expansion swscale skips (rgb_range.h)
        if crate::rgb_range::rgb_frame_needs_legal_expansion(fr, crate::rgb_range::RangeOverride::Auto) { crate::rgb_range::expand_rgba8_legal_to_full(&mut rgba); }
        Some(ThumbPixels { width: ow as u32, height: oh as u32, rgba })
    } }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    #[test] fn sequence_parts() {
        assert_eq!(parse_sequence_parts("/s/shot_0042.exr?layer=diffuse"), Some(("/s/shot_".into(), 42, ".exr?layer=diffuse".into())));
        assert_eq!(parse_sequence_parts("/s/clip.mov"), None);
    }
    struct Fake; impl ThumbLoader for Fake { fn load_thumbnail(&mut self, f: i32, _m: u32) -> Option<ThumbPixels> { Some(ThumbPixels { width: 2, height: 1, rgba: vec![f as u8; 8] }) } }
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
