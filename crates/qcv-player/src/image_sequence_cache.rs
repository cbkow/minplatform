//! `ImageSequenceCache` — ported from `QCView-Player/src/decode/image_sequence_cache.{h,cpp}` (itself old QCView's
//! `DirectEXRCache`, ~2,249 lines): the read-ahead cache EVERY image-sequence format runs through (EXR / TIFF / PNG /
//! JPEG — the loader is `min_image::load`).
//!
//! Two threads, QCView's shape:
//! - the CACHE thread ticks every 10 ms (or when woken): evicts frames outside [playhead − behind, playhead + ahead]
//!   (modulo distance when looping, or inside a loop range), then queues — the playhead's frame FIRST, then forward
//!   read-ahead, then read-behind (only at stride 1) — at most 32 requests outstanding, batches of max(4, ahead / 2);
//! - the IO thread starts one load per frame (QCView's `std::async`, one thread per task) while fewer than
//!   `threads` are in flight (the seek target may take one more, and decodes with 8 internal threads — the latency
//!   tier), and files each result: a stale generation is dropped; a missing file is a GAP sentinel, a failed decode
//!   a BROKEN one, and both join the failed set (not retried until the next user seek — QCView's "amnesty").
//!
//! The display side pulls: [`ImageSequenceCache::frame`] peeks (never touches the LRU — shown frames age out),
//! [`ImageSequenceCache::closest_frame`] is the last-good fallback. A user seek (a jump beyond −2 / +20, or forced)
//! bumps the generation, clears the queue and the failed set, and queues the target at the FRONT. A loop seam is not
//! a seek. The GPU push sink fires for every fresh frame and on every playhead move onto a cached frame.
//!
//! Divergences, each for a reason:
//! - completions WAKE the IO thread; QCView polled its futures on a 10-ms timeout, so a seek target sat decoded for
//!   up to 10 ms before it was filed;
//! - ONE broken sentinel per cache, shared: QCView built a full-size RGBA8 image per broken frame (33 MB at 4K) and
//!   costed it at 4 bytes — a run of broken frames could hold gigabytes the budget never saw;
//! - closing never waits for a decode in flight (QCView waited on every future — up to a DWAB multilayer frame,
//!   ~900 ms, on the closing thread); the loads finish into the dropped generation;
//! - QCView's overrun flags (`m_overrunMode`, `m_lastGoodFrame`) were written and never read — not carried.
use crate::simple_lru::SimpleLru;
use min_image::{Image, Sentinel};
use std::{collections::{HashMap, HashSet, VecDeque}, sync::{atomic::{AtomicBool, AtomicI32, AtomicU64, Ordering::*}, Arc, Condvar, Mutex}, thread, time::Duration};

/// QCView's `ImageSeqStreamingConfig` — ~3 s ahead, ~0.5 s behind at 24 fps, 16 IO threads (validated 1–32 in the
/// old app; slow multilayer EXRs need them).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StreamingConfig { pub read_ahead: i32, pub read_behind: i32, pub threads: i32 }
impl Default for StreamingConfig { fn default() -> Self { StreamingConfig { read_ahead: 72, read_behind: 12, threads: 16 } } }
impl StreamingConfig {
    /// The Settings panel's `performance/imageSeqThreads` + `performance/imageSeqReadAhead`, QCView's clamps;
    /// read-behind derives from read-ahead at 6 : 1.
    pub fn from_settings(threads: i32, read_ahead: i32) -> Self {
        let read_ahead = read_ahead.clamp(24, 240);
        StreamingConfig { threads: threads.clamp(1, 64), read_ahead, read_behind: (read_ahead / 6).max(12).clamp(0, 120) }
    }
}

const CACHE_TICK: Duration = Duration::from_millis(10);
const IO_WAIT: Duration = Duration::from_millis(10);
const BIG_JUMP_FORWARD: i32 = 20;
const BIG_JUMP_BACKWARD: i32 = -2;
const LOOP_WRAP_SLACK: i32 = 2;
const MAX_CONCURRENT_REQS: usize = 32;
const EST_FRAME_BYTES_4K: usize = 64 * 1024 * 1024;
const SLACK_FRAMES: usize = 50;
const EXPRESS_DECODE_THREADS: usize = 8;

/// Called on the IO thread for every freshly filed frame, and on playhead moves onto a cached one. Must be cheap.
pub type PushSink = Arc<dyn Fn(i32, Arc<Image>) + Send + Sync>;
/// Called when a frame leaves the cache (window slid, budget, seek, layer change).
pub type EvictSink = Arc<dyn Fn(i32) + Send + Sync>;

#[derive(thiserror::Error, Debug)]
pub enum Error { #[error("no loader for '{0}'")] Format(String), #[error("first frame: {0}")] Probe(String) }

struct Io { requests: VecDeque<i32>, in_flight: HashMap<i32, u64>, done: Vec<(i32, u64, Option<Image>)> }

struct Shared {
    directory: String, pattern: String, start_frame: i32, frame_count: i32, fps: f64, width: u32, height: u32,
    is_exr: bool, layer: Mutex<String>, config: StreamingConfig,
    lru: SimpleLru<i32, Arc<Image>>, broken: Arc<Image>, gap: Arc<Image>,
    io: Mutex<Io>, io_cv: Condvar, sleep: Mutex<()>, cache_cv: Condvar,
    running: AtomicBool, playhead: AtomicI32, last_seek: AtomicI32, generation: AtomicU64,
    stride: AtomicI32, looping: AtomicBool, has_range: AtomicBool, loop_in: AtomicI32, loop_out: AtomicI32,
    failed: Mutex<HashSet<i32>>,
    ahead: AtomicI32, behind: AtomicI32, size: AtomicI32, ahead_cov: AtomicI32, behind_cov: AtomicI32,
    push: Mutex<Option<PushSink>>, evicted: Mutex<Option<EvictSink>>,
}

pub struct ImageSequenceCache { sh: Arc<Shared>, threads: Vec<thread::JoinHandle<()>> }

/// QCView's `snprintf(pattern, fileFrame)` for the patterns sequence detection writes: `%d`, `%0Nd`, `%Nd`, `%%`.
pub fn format_frame(pattern: &str, n: i32) -> String {
    let mut out = String::with_capacity(pattern.len() + 8);
    let mut it = pattern.chars().peekable();
    while let Some(c) = it.next() {
        if c != '%' { out.push(c); continue }
        if it.peek() == Some(&'%') { it.next(); out.push('%'); continue }
        let mut spec = String::new();
        while let Some(&d) = it.peek() { if d.is_ascii_digit() { spec.push(d); it.next(); } else { break } }
        if it.peek() == Some(&'d') {
            it.next();
            let zero = spec.starts_with('0'); let width: usize = spec.parse().unwrap_or(0);
            out.push_str(&if zero { format!("{n:0width$}") } else { format!("{n:width$}") });
        } else { out.push('%'); out.push_str(&spec) }
    }
    out
}

impl ImageSequenceCache {
    /// QCView's constructor + `initialize`: probes frame 0's size (EXR through the layer), sizes the byte budget and
    /// starts both threads. Frame numbers below are 0-based offsets from `start_frame`.
    pub fn open(directory: &str, pattern: &str, start_frame: i32, end_frame: i32, fps: f64, layer: &str, config: StreamingConfig) -> Result<Self, Error> {
        let first = Self::path_of(directory, pattern, start_frame);
        let fmt = min_image::Format::of(&first).ok_or_else(|| Error::Format(pattern.into()))?;
        let (width, height) = std::panic::catch_unwind(|| min_image::dimensions(&first, layer)).map_err(|_| Error::Probe(format!("{first}: the probe panicked")))?.map_err(|e| Error::Probe(e.to_string()))?;
        let frame_count = end_frame - start_frame + 1;
        let per_frame = width as usize * height as usize * 8;
        let window = (config.read_ahead + config.read_behind) as usize + SLACK_FRAMES;
        let budget = if per_frame > 0 { per_frame * window } else { EST_FRAME_BYTES_4K * window };
        let sh = Arc::new(Shared {
            directory: directory.into(), pattern: pattern.into(), start_frame, frame_count, fps, width, height,
            is_exr: fmt == min_image::Format::Exr, layer: Mutex::new(layer.into()), config,
            lru: SimpleLru::new(budget), broken: Arc::new(Image::sentinel(Sentinel::Broken, width, height)), gap: Arc::new(Image::sentinel(Sentinel::Gap, 1, 1)),
            io: Mutex::new(Io { requests: VecDeque::new(), in_flight: HashMap::new(), done: vec![] }), io_cv: Condvar::new(), sleep: Mutex::new(()), cache_cv: Condvar::new(),
            running: AtomicBool::new(true), playhead: AtomicI32::new(0), last_seek: AtomicI32::new(0), generation: AtomicU64::new(0),
            stride: AtomicI32::new(1), looping: AtomicBool::new(true), has_range: AtomicBool::new(false), loop_in: AtomicI32::new(-1), loop_out: AtomicI32::new(-1),
            failed: Mutex::new(HashSet::new()),
            ahead: AtomicI32::new(0), behind: AtomicI32::new(0), size: AtomicI32::new(0), ahead_cov: AtomicI32::new(0), behind_cov: AtomicI32::new(0),
            push: Mutex::new(None), evicted: Mutex::new(None),
        });
        let (a, b) = (sh.clone(), sh.clone());
        let threads = vec![
            thread::Builder::new().name("qcv-seq-cache".into()).spawn(move || a.cache_loop()).expect("spawn the sequence cache thread"),
            thread::Builder::new().name("qcv-seq-io".into()).spawn(move || b.io_loop()).expect("spawn the sequence IO thread"),
        ];
        Ok(ImageSequenceCache { sh, threads })
    }
    fn path_of(directory: &str, pattern: &str, file_frame: i32) -> String { std::path::Path::new(directory).join(format_frame(pattern, file_frame)).to_string_lossy().into_owned() }
    /// The file behind 0-based frame `n`.
    pub fn frame_path(&self, n: i32) -> String { self.sh.frame_path(n) }

    /// The cached frame, or None (the caller falls through to [`Self::closest_frame`]). Sentinels come back too.
    pub fn frame(&self, n: i32) -> Option<Arc<Image>> { self.sh.lru.peek(&n) }
    /// The nearest cached frame by absolute distance (ties → the lower frame, QCView's `std::map` order).
    pub fn closest_frame(&self, n: i32) -> Option<(i32, Arc<Image>)> {
        let mut keys = self.sh.lru.keys(); keys.sort_unstable();
        let best = keys.iter().copied().min_by_key(|k| (k - n).abs())?;
        self.sh.lru.peek(&best).map(|i| (best, i))
    }
    pub fn update_playhead(&self, n: i32, force_seek: bool) { self.sh.update_playhead(n, force_seek) }
    pub fn playhead(&self) -> i32 { self.sh.playhead.load(Acquire) }

    pub fn width(&self) -> u32 { self.sh.width }
    pub fn height(&self) -> u32 { self.sh.height }
    pub fn frame_count(&self) -> i32 { self.sh.frame_count }
    pub fn fps(&self) -> f64 { self.sh.fps }
    pub fn config(&self) -> StreamingConfig { self.sh.config }
    pub fn buffered_ahead(&self) -> i32 { self.sh.ahead.load(Relaxed) }
    pub fn buffered_behind(&self) -> i32 { self.sh.behind.load(Relaxed) }
    pub fn buffer_size(&self) -> i32 { self.sh.size.load(Relaxed) }
    /// The frame DISTANCE the window covers (≠ the count at stride > 1) — what the cache band draws.
    pub fn buffered_ahead_coverage(&self) -> i32 { self.sh.ahead_cov.load(Relaxed) }
    pub fn buffered_behind_coverage(&self) -> i32 { self.sh.behind_cov.load(Relaxed) }
    /// Bumps on every user seek / layer change — the renderer drops textures of older generations.
    pub fn generation(&self) -> u64 { self.sh.generation.load(Acquire) }
    pub fn bytes(&self) -> usize { self.sh.lru.current_bytes() }

    pub fn stride(&self) -> i32 { self.sh.stride.load(Relaxed) }
    /// 1–4: cache every Nth frame (on the absolute grid); read-behind is off above 1.
    pub fn set_stride(&self, s: i32) { let s = s.clamp(1, 4); if self.sh.stride.swap(s, AcqRel) != s { self.sh.cache_cv.notify_one() } }
    pub fn looping(&self) -> bool { self.sh.looping.load(Relaxed) }
    /// Wrap-around caching (ON by default, QCView's `is_looping_ = true`).
    pub fn set_looping(&self, on: bool) { self.sh.looping.store(on, Release) }
    /// Restrict caching to [in, out] with wrap inside it; an invalid range clears it.
    pub fn set_loop_range(&self, inp: i32, out: i32) {
        if inp < 0 || out <= inp || out >= self.sh.frame_count { return self.clear_loop_range() }
        self.sh.loop_in.store(inp, Release); self.sh.loop_out.store(out, Release); self.sh.has_range.store(true, Release);
        self.sh.cache_cv.notify_one();
    }
    pub fn clear_loop_range(&self) {
        if !self.sh.has_range.swap(false, AcqRel) { return }
        self.sh.loop_in.store(-1, Release); self.sh.loop_out.store(-1, Release); self.sh.cache_cv.notify_one();
    }
    pub fn loop_range(&self) -> Option<(i32, i32)> { self.sh.has_range.load(Acquire).then(|| (self.sh.loop_in.load(Acquire), self.sh.loop_out.load(Acquire))) }

    pub fn layer(&self) -> String { self.sh.layer.lock().unwrap().clone() }
    /// EXR only: a new layer drops the cache (QCView's re-initialise approximation) and refills at the playhead.
    pub fn set_layer(&self, layer: &str) {
        let sh = &self.sh;
        if !sh.is_exr { return }
        { let mut l = sh.layer.lock().unwrap(); if *l == layer { return } *l = layer.into(); }
        sh.generation.fetch_add(1, AcqRel);
        let sink = sh.evicted.lock().unwrap().clone();
        if let Some(s) = &sink { for k in sh.lru.keys() { s(k) } }
        sh.lru.clear();
        sh.failed.lock().unwrap().clear();
        sh.io.lock().unwrap().requests.clear();
        for a in [&sh.ahead, &sh.behind, &sh.size, &sh.ahead_cov, &sh.behind_cov] { a.store(0, Relaxed) }
        sh.cache_cv.notify_one();
    }
    /// Seed the failed set with the frames sequence detection found missing — never even tried; the band shows them
    /// before any IO.
    pub fn set_known_missing(&self, frames: &[i32]) { self.sh.failed.lock().unwrap().extend(frames.iter().copied()) }
    pub fn failed_frames(&self) -> Vec<i32> { let mut v: Vec<i32> = self.sh.failed.lock().unwrap().iter().copied().collect(); v.sort_unstable(); v }
    pub fn failed_count(&self) -> usize { self.sh.failed.lock().unwrap().len() }

    pub fn set_push_sink(&self, s: Option<PushSink>) { *self.sh.push.lock().unwrap() = s }
    pub fn set_evict_sink(&self, s: Option<EvictSink>) { *self.sh.evicted.lock().unwrap() = s }
}

impl Drop for ImageSequenceCache {
    fn drop(&mut self) {
        self.sh.running.store(false, Release);
        self.sh.generation.fetch_add(1, AcqRel);   // loads still running finish into a dead generation
        self.sh.cache_cv.notify_all(); self.sh.io_cv.notify_all();
        for t in self.threads.drain(..) { let _ = t.join(); }
        self.sh.lru.clear();
    }
}

impl Shared {
    fn frame_path(&self, n: i32) -> String { ImageSequenceCache::path_of(&self.directory, &self.pattern, n + self.start_frame) }
    fn is_failed(&self, n: i32) -> bool { self.failed.lock().unwrap().contains(&n) }
    fn push_sink(&self) -> Option<PushSink> { self.push.lock().unwrap().clone() }

    fn update_playhead(&self, n: i32, force_seek: bool) {
        let n = n.clamp(0, (self.frame_count - 1).max(0));
        let prev = self.playhead.swap(n, AcqRel);
        let delta = n - prev;
        let mut big = !(BIG_JUMP_BACKWARD..=BIG_JUMP_FORWARD).contains(&delta);
        // A loop seam (end → start or back) is not a seek: the in point is already in the wrapped window.
        if big && self.looping.load(Acquire) && self.frame_count > 0 {
            let last = self.frame_count - 1 - LOOP_WRAP_SLACK;
            if (prev >= last && n <= LOOP_WRAP_SLACK) || (prev <= LOOP_WRAP_SLACK && n >= last) { big = false }
        }
        if force_seek || big {
            self.generation.fetch_add(1, AcqRel);
            self.last_seek.store(n, Release);
            self.io.lock().unwrap().requests.clear();
            self.failed.lock().unwrap().clear();   // the amnesty: a seek retries frames a flaky share failed
            self.cache_cv.notify_one();
        }
        if delta != 0 || force_seek {
            let cached = self.lru.peek(&n);
            if let Some(img) = &cached && img.sentinel.is_none() && let Some(s) = self.push_sink() { s(n, img.clone()) }
            // The reactive priority request only on a seek — on every small scrub delta it queued stale frames faster
            // than the pool drained them (QCView's note).
            if cached.is_none() && (force_seek || big) {
                let mut io = self.io.lock().unwrap();
                if !io.in_flight.contains_key(&n) && !self.is_failed(n) {
                    io.requests.retain(|&f| f != n);
                    io.requests.push_front(n);
                    drop(io); self.io_cv.notify_one();
                }
            }
        }
    }

    fn fwd(&self, frame: i32, cur: i32) -> i32 { if self.looping.load(Relaxed) && self.frame_count > 0 { (frame - cur).rem_euclid(self.frame_count) } else { frame - cur } }
    fn bwd(&self, frame: i32, cur: i32) -> i32 { if self.looping.load(Relaxed) && self.frame_count > 0 { (cur - frame).rem_euclid(self.frame_count) } else { cur - frame } }

    fn evict_outside_window(&self, cur: i32) {
        let stride = self.stride.load(Relaxed).max(1);
        let read_behind = if stride > 1 { 0 } else { self.config.read_behind };
        let read_ahead = self.config.read_ahead;
        let looping = self.looping.load(Relaxed) && self.frame_count > 0;
        let (has_range, lin, lout) = (self.has_range.load(Acquire), self.loop_in.load(Acquire), self.loop_out.load(Acquire));
        let loop_size = if has_range && lout > lin { lout - lin + 1 } else { 0 };
        let sink = self.evicted.lock().unwrap().clone();
        let (mut ac, mut bc, mut acov, mut bcov) = (0, 0, 0, 0);
        for k in self.lru.keys() {
            let (evict, f, b);
            if loop_size > 0 {
                if k < lin || k > lout { evict = true; f = 0; b = 0 } else {
                    let cur_in = cur.clamp(lin, lout) - lin; let k_in = k - lin;
                    f = (k_in - cur_in).rem_euclid(loop_size); b = (cur_in - k_in).rem_euclid(loop_size);
                    evict = b > read_behind && f > read_ahead;
                }
            } else {
                f = self.fwd(k, cur); b = self.bwd(k, cur);
                evict = if looping { b > read_behind && f > read_ahead } else { f > read_ahead || b > read_behind };
            }
            if evict { self.lru.remove(&k); if let Some(s) = &sink { s(k) } continue }
            let is_ahead = if loop_size > 0 || looping { f <= b } else { k >= cur };
            if is_ahead { ac += 1; acov = acov.max(f) } else { bc += 1; bcov = bcov.max(b) }
        }
        self.ahead.store(ac, Relaxed); self.behind.store(bc, Relaxed); self.size.store(ac + bc, Relaxed);
        self.ahead_cov.store(acov, Relaxed); self.behind_cov.store(bcov, Relaxed);
    }

    fn build_request_queue(&self, io: &mut Io, cur: i32) {
        let stride = self.stride.load(Relaxed).max(1);
        let pending = io.requests.len() + io.in_flight.len();
        if pending >= MAX_CONCURRENT_REQS { return }
        let mut budget = ((self.config.read_ahead / 2).max(4) as usize).min(MAX_CONCURRENT_REQS - pending);
        let failed = self.failed.lock().unwrap().clone();
        let already = |io: &Io, f: i32| self.lru.contains(&f) || io.in_flight.contains_key(&f) || io.requests.contains(&f) || failed.contains(&f);
        let n = self.frame_count;
        let looping = self.looping.load(Relaxed) && n > 0;
        let in_range = |f: i32| f >= 0 && f < n;
        let on_grid = |f: i32| stride <= 1 || f % stride == 0;
        let (has_range, lin, lout) = (self.has_range.load(Acquire), self.loop_in.load(Acquire), self.loop_out.load(Acquire));
        let use_range = has_range && lout > lin;
        let base = if use_range { cur.clamp(lin, lout) } else { cur };
        let size = if use_range { lout - lin + 1 } else { 0 };
        let wrap = |offset: i32| -> i32 {
            if use_range { return lin + (base - lin + offset).rem_euclid(size) }
            let raw = cur + offset;
            if looping { raw.rem_euclid(n) } else if in_range(raw) { raw } else { -1 }
        };
        if stride <= 1 && budget > 0 && in_range(base) && on_grid(base) && !already(io, base) { io.requests.push_back(base); budget -= 1; }
        let ahead_cap = if use_range { self.config.read_ahead.min(size - 1) } else { self.config.read_ahead };
        let behind_cap = if use_range { self.config.read_behind.min(size - 1) } else { self.config.read_behind };
        let mut off = 1;
        while off <= ahead_cap && budget > 0 {
            let f = wrap(off); if f < 0 { break }
            if on_grid(f) && !already(io, f) { io.requests.push_back(f); budget -= 1; }
            off += 1;
        }
        if stride <= 1 {
            let mut off = 1;
            while off <= behind_cap && budget > 0 {
                let f = wrap(-off); if f < 0 { break }
                if !already(io, f) { io.requests.push_back(f); budget -= 1; }
                off += 1;
            }
        }
    }

    fn cache_loop(&self) {
        while self.running.load(Acquire) {
            { let g = self.sleep.lock().unwrap(); let _ = self.cache_cv.wait_timeout(g, CACHE_TICK).unwrap(); }
            if !self.running.load(Acquire) { break }
            let cur = self.playhead.load(Acquire);
            self.evict_outside_window(cur);
            let queued = { let mut io = self.io.lock().unwrap(); let before = io.requests.len(); self.build_request_queue(&mut io, cur); io.requests.len() > before };
            if queued { self.io_cv.notify_one() }
        }
    }

    fn io_loop(self: &Arc<Self>) {
        while self.running.load(Acquire) {
            let mut io = self.io.lock().unwrap();
            io = self.io_cv.wait_timeout_while(io, IO_WAIT, |io| self.running.load(Acquire) && io.requests.is_empty() && io.done.is_empty()).unwrap().0;
            if !self.running.load(Acquire) { break }
            // Start loads while fewer than `threads` are in flight (the seek target may take one more).
            let cap = self.config.threads.max(1) as usize;
            let seek = self.last_seek.load(Acquire);
            let generation = self.generation.load(Acquire);
            while let Some(&frame) = io.requests.front() {
                if io.in_flight.len() >= MAX_CONCURRENT_REQS { break }
                let express = frame == seek;
                if io.in_flight.len() >= if express { cap + 1 } else { cap } { break }
                io.requests.pop_front();
                if io.in_flight.contains_key(&frame) || self.lru.contains(&frame) { continue }
                io.in_flight.insert(frame, generation);
                let (sh, path, layer) = (self.clone(), self.frame_path(frame), self.layer.lock().unwrap().clone());
                let spawned = thread::Builder::new().name("qcv-seq-load".into()).spawn(move || {
                    let r = std::panic::catch_unwind(|| {
                        if std::fs::metadata(&path).is_err() { return Some(Image::sentinel(Sentinel::Gap, 1, 1)) }
                        min_image::load(&path, &layer, if express { EXPRESS_DECODE_THREADS } else { 0 }).map_err(|e| log::warn!("ImageSequenceCache: {e}")).ok()
                    }).unwrap_or(None);
                    sh.io.lock().unwrap().done.push((frame, generation, r));
                    sh.io_cv.notify_all();
                });
                if spawned.is_err() { io.in_flight.remove(&frame); break }
            }
            let done = std::mem::take(&mut io.done);
            for (f, _, _) in &done { io.in_flight.remove(f); }
            drop(io);
            let cur = self.generation.load(Acquire);
            for (frame, g, r) in done {
                if g != cur { continue }
                let img = match r {
                    None => { log::warn!("ImageSequenceCache: decode failed for frame {frame} — blacklisted until the next seek"); self.failed.lock().unwrap().insert(frame); self.broken.clone() }
                    Some(i) if i.sentinel.is_some() => { self.failed.lock().unwrap().insert(frame); self.gap.clone() }
                    Some(i) => Arc::new(i),
                };
                let sentinel = img.sentinel.is_some();
                let bytes = if sentinel { 4 } else { img.byte_size() };
                let pushed_out = self.lru.add_evicting(frame, img.clone(), bytes);
                if !pushed_out.is_empty() && let Some(s) = self.evicted.lock().unwrap().clone() { for k in pushed_out { s(k) } }
                if !sentinel && let Some(s) = self.push_sink() { s(frame, img) }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    #[test]
    fn printf_patterns() {
        assert_eq!(format_frame("shot.%04d.exr", 7), "shot.0007.exr");
        assert_eq!(format_frame("a_%d.png", 1001), "a_1001.png");
        assert_eq!(format_frame("b%6d.tif", 12), "b    12.tif");
        assert_eq!(format_frame("100%%_%03d.jpg", 5), "100%_005.jpg");
    }
    #[test]
    fn streaming_config_clamps_like_qcview() {
        assert_eq!(StreamingConfig::from_settings(16, 72), StreamingConfig { threads: 16, read_ahead: 72, read_behind: 12 });
        assert_eq!(StreamingConfig::from_settings(0, 600), StreamingConfig { threads: 1, read_ahead: 240, read_behind: 40 });
        assert_eq!(StreamingConfig::from_settings(99, 10), StreamingConfig { threads: 64, read_ahead: 24, read_behind: 12 });
    }

    /// A synthetic PNG sequence (FFmpeg; fixtures never travel): 1001–1040, frame 1010 missing, 1020 corrupt.
    fn sequence() -> Option<(tempdir::Dir, &'static str)> {
        let d = tempdir::Dir::new();
        let ok = std::process::Command::new("ffmpeg").args(["-hide_banner", "-loglevel", "error", "-y", "-f", "lavfi", "-i", "testsrc2=size=64x36:rate=24", "-frames:v", "40", "-start_number", "1001"])
            .arg(d.0.join("f_%04d.png")).status().map(|s| s.success()).unwrap_or(false);
        if !ok { return None }
        std::fs::remove_file(d.0.join("f_1010.png")).unwrap();
        std::fs::write(d.0.join("f_1020.png"), b"not a png").unwrap();
        Some((d, "f_%04d.png"))
    }
    mod tempdir { pub struct Dir(pub std::path::PathBuf); impl Dir { pub fn new() -> Dir { let p = std::env::temp_dir().join(format!("qcv-seq-{}-{:?}", std::process::id(), std::thread::current().id())); let _ = std::fs::remove_dir_all(&p); std::fs::create_dir_all(&p).unwrap(); Dir(p) } } impl Drop for Dir { fn drop(&mut self) { let _ = std::fs::remove_dir_all(&self.0); } } }
    fn wait(mut f: impl FnMut() -> bool) -> bool { let t = Instant::now(); while t.elapsed() < Duration::from_secs(5) { if f() { return true } thread::sleep(Duration::from_millis(5)) } false }

    #[test]
    fn fills_the_window_files_gaps_and_broken_frames_and_wraps_the_loop() {
        let Some((d, pat)) = sequence() else { eprintln!("no ffmpeg — skipped"); return };
        let c = ImageSequenceCache::open(&d.0.to_string_lossy(), pat, 1001, 1040, 24.0, "", StreamingConfig { read_ahead: 24, read_behind: 4, threads: 4 }).unwrap();
        assert_eq!((c.width(), c.height(), c.frame_count()), (64, 36, 40));
        let pushed = Arc::new(Mutex::new(vec![]));
        { let p = pushed.clone(); c.set_push_sink(Some(Arc::new(move |f, _| p.lock().unwrap().push(f)))); }
        c.update_playhead(0, true);
        // 0..=24 ahead + 36..=39 behind; on a 40-frame loop QCView counts 21..=24 as "behind" (nearer backwards)
    assert!(wait(|| c.buffer_size() == 29), "window filled: {} cached", c.buffer_size());
        let f0 = c.frame(0).unwrap();
        assert!(f0.sentinel.is_none() && f0.width == 64);
        assert_eq!(c.frame(9).unwrap().sentinel, Some(Sentinel::Gap), "frame 1010 is missing on disk");
        assert_eq!(c.frame(19).unwrap().sentinel, Some(Sentinel::Broken), "frame 1020 does not decode");
        assert_eq!(c.failed_frames(), vec![9, 19]);
        assert!(!pushed.lock().unwrap().contains(&9), "sentinels never reach the GPU sink");
        // read-behind wraps (looping on by default): frames 36..39 are behind frame 0
        assert!(wait(|| (36..40).all(|f| c.frame(f).is_some())), "the wrapped read-behind");
        // a user seek: the window follows, the failed set is cleared (amnesty), the old ahead frames go
        let g = c.generation();
        c.update_playhead(30, false);
        assert!(c.generation() > g);
        assert!(wait(|| c.frame(30).is_some() && c.frame(15).is_none()), "the window moved to 30");
        assert_eq!(c.closest_frame(30).unwrap().0, 30);
    }
    #[test]
    fn stride_caches_every_nth_frame_and_drops_read_behind() {
        let Some((d, pat)) = sequence() else { return };
        let c = ImageSequenceCache::open(&d.0.to_string_lossy(), pat, 1001, 1040, 24.0, "", StreamingConfig { read_ahead: 24, read_behind: 4, threads: 4 }).unwrap();
        c.set_looping(false);
        c.set_stride(3);
        c.update_playhead(0, true);
        assert!(wait(|| c.frame(24).is_some()));
        assert!(c.frame(1).is_none() && c.frame(2).is_none() && c.frame(3).is_some());
        assert_eq!(c.buffered_behind(), 0);
    }
    #[test]
    fn a_missing_first_frame_fails_the_open() {
        let d = tempdir::Dir::new();
        assert!(matches!(ImageSequenceCache::open(&d.0.to_string_lossy(), "x_%04d.exr", 1, 10, 24.0, "", StreamingConfig::default()), Err(Error::Probe(_))));
        assert!(matches!(ImageSequenceCache::open(&d.0.to_string_lossy(), "x_%04d.dpx", 1, 10, 24.0, "", StreamingConfig::default()), Err(Error::Format(_))));
    }
}

/// `QCV_SEQ_BENCH=<dir>` (frames `frame_%04d.exr` from 1): the seek target's latency and the window's fill rate.
/// `cargo test --release -p qcv-player seq_bench -- --ignored --nocapture`.
#[cfg(test)]
mod bench {
    use super::*;
    use std::time::Instant;
    #[test] #[ignore]
    fn seq_bench() {
        let Ok(dir) = std::env::var("QCV_SEQ_BENCH") else { return };
        let n = std::fs::read_dir(&dir).unwrap().filter(|e| e.as_ref().unwrap().path().extension().is_some_and(|x| x == "exr")).count() as i32;
        let threads: i32 = std::env::var("QCV_SEQ_THREADS").ok().and_then(|v| v.parse().ok()).unwrap_or(16);
        let t = Instant::now();
        let c = ImageSequenceCache::open(&dir, "frame_%04d.exr", 1, n, 24.0, "", StreamingConfig { threads, ..Default::default() }).unwrap();
        println!("open (probe) {:.1} ms; {}x{}, {n} frames, {threads} threads", t.elapsed().as_secs_f64() * 1e3, c.width(), c.height());
        let t = Instant::now();
        c.update_playhead(n / 2, true);
        while c.frame(n / 2).is_none() { thread::sleep(Duration::from_micros(200)) }
        println!("seek target cold {:.1} ms", t.elapsed().as_secs_f64() * 1e3);
        while c.buffer_size() < n.min(c.config().read_ahead + c.config().read_behind + 1) && t.elapsed() < Duration::from_secs(60) { thread::sleep(Duration::from_millis(2)) }
        let s = t.elapsed().as_secs_f64();
        println!("window of {} filled in {:.2} s = {:.0} fps, {:.0} MB cached", c.buffer_size(), s, c.buffer_size() as f64 / s, c.bytes() as f64 / 1e6);
        let t = Instant::now();
        c.update_playhead(3, true);
        while c.frame(3).is_none() { thread::sleep(Duration::from_micros(200)) }
        println!("seek to a cached frame {:.2} ms", t.elapsed().as_secs_f64() * 1e3);
    }
}
