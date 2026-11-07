//! `ReadAhead` — ported from `QCView-Player/src/decode/read_ahead.{h,cpp}`: warm the bytes of the next few video frames
//! before the decoder asks for them.
//!
//! On a network volume (LucidLink, SMB shares) an uncached frame costs a round trip per read, and a decoder reads one
//! frame at a time: 8K ProRes arrived at about one frame a second, while the same file played in real time once cached.
//! This touches the next frames' exact byte ranges, from the container's index: one byte per volume cache page
//! (LucidLink's page is 1 MiB; `QCV_READAHEAD_PAGE_KB`). Nothing is held in the app: a table of frame offsets.
//! It runs for every video file (telling a network volume from a local one isn't reliable), kept cheap: one background
//! thread for the whole app at utility QoS / I/O priority, no locks shared with decode or render, asleep when idle.
//! Driven by READ position, not the playhead; a jump restarts the window. The window is media time with a byte cap
//! (`QCV_READAHEAD_SECONDS`, default 2; 0 = off; `QCV_READAHEAD_MB`, default 512); a loop range hydrates whole after
//! the window (`QCV_READAHEAD_LOOP_GB`, default 16).
use ffmpeg_sys_next as ff;
use std::{collections::HashMap, fs::File, io::{Read, Seek, SeekFrom}, sync::{atomic::{AtomicBool, AtomicI32, AtomicU32, Ordering}, Arc, Condvar, Mutex, OnceLock}, thread, time::Instant};

const GAP_FILL: i64 = 4 << 20;   // interleaved audio between video samples
const NO_GEN: u32 = u32::MAX;

/// One frame's bytes in the file: the sample plus interleaved data up to the next video sample, when that gap is small.
#[derive(Clone, Copy, Debug)] pub struct FrameSpan { pub pos: i64, pub len: i64 }
impl Default for FrameSpan { fn default() -> Self { FrameSpan { pos: -1, len: 0 } } }

pub type Id = u64;   // 0 = not attached

struct Client {
    #[allow(dead_code)] id: Id, path: String, name: String, spans: Vec<FrameSpan>, max_frames: i32,
    position: AtomicI32, window_gen: AtomicU32, detached: AtomicBool, range_first: AtomicI32, range_last: AtomicI32, range_gen: AtomicU32,
    w: Mutex<Worker>,   // worker-thread state
}
#[derive(Default)]
struct Worker {
    seen_gen: u32, frontier: i32, failed: bool, file: Option<File>, window_start: Option<Instant>, window_bytes: i64, last_page: i64, window_logged: bool, window_logs: u32, bytes: i64, last_log_bytes: i64, read_secs: f64, restarts: u32,
    range_seen_gen: u32, range_frontier: i32, range_end: i32, range_last_page: i64, range_bytes: i64, range_start: Option<Instant>, range_logged: bool,
}

struct Inner { seconds: f64, cap_bytes: i64, page_bytes: i64, loop_cap_bytes: i64, state: Mutex<State>, cv: Condvar }
struct State { clients: HashMap<Id, Arc<Client>>, next_id: Id, work_pending: bool, stop: bool }

pub struct ReadAhead { inner: Arc<Inner>, thread: Mutex<Option<thread::JoinHandle<()>>>, pub touched_pages: Arc<std::sync::atomic::AtomicU64> }
static INSTANCE: OnceLock<ReadAhead> = OnceLock::new();

fn env_f64(k: &str, d: f64) -> f64 { std::env::var(k).ok().and_then(|v| v.parse().ok()).map(|v: f64| v.max(0.0)).unwrap_or(d) }
fn env_i64(k: &str, d: i64) -> i64 { std::env::var(k).ok().and_then(|v| v.parse().ok()).filter(|v: &i64| *v > 0).unwrap_or(d) }

impl ReadAhead {
    pub fn instance() -> &'static ReadAhead {
        INSTANCE.get_or_init(|| {
            let inner = Arc::new(Inner { seconds: env_f64("QCV_READAHEAD_SECONDS", 2.0), cap_bytes: env_i64("QCV_READAHEAD_MB", 512) << 20, page_bytes: env_i64("QCV_READAHEAD_PAGE_KB", 1024) << 10, loop_cap_bytes: env_i64("QCV_READAHEAD_LOOP_GB", 16) << 30,
                state: Mutex::new(State { clients: HashMap::new(), next_id: 1, work_pending: false, stop: false }), cv: Condvar::new() });
            let touched = Arc::new(std::sync::atomic::AtomicU64::new(0));
            let thread = if inner.seconds > 0.0 {
                log::info!("ReadAhead: {:.1} s window, {} MB cap, 1 byte per {} KB page", inner.seconds, inner.cap_bytes >> 20, inner.page_bytes >> 10);
                let i = inner.clone(); let t = touched.clone();
                thread::Builder::new().name("read-ahead".into()).spawn(move || run(&i, &t)).ok()
            } else { log::info!("ReadAhead: off (QCV_READAHEAD_SECONDS=0)"); None };
            ReadAhead { inner, thread: Mutex::new(thread), touched_pages: touched }
        })
    }
    /// The table of frame spans from the demuxer's index. `ts_to_frame` maps an index timestamp to the caller's frame
    /// number. Empty when the container has no usable index (MOV/MP4 list every sample; Matroska indexes cue points only).
    pub unsafe fn spans_from_index(fmt: *mut ff::AVFormatContext, stream_idx: i32, ts_to_frame: &dyn Fn(i64) -> i32, frame_count: i32) -> Vec<FrameSpan> {
        unsafe {
            if fmt.is_null() || stream_idx < 0 || stream_idx >= (*fmt).nb_streams as i32 || frame_count <= 0 { return vec![] }
            let st = *(*fmt).streams.add(stream_idx as usize);
            let n = ff::avformat_index_get_entries_count(st);
            if n <= 0 { return vec![] }
            struct E { pos: i64, end: i64, frame: i32 }
            let mut entries: Vec<E> = Vec::with_capacity(n as usize);
            for i in 0..n {
                let e = ff::avformat_index_get_entry(st, i);
                if e.is_null() || (*e).pos < 0 { continue }
                let size = (*e).size() as i64; if size <= 0 { continue }
                let f = ts_to_frame((*e).timestamp);
                if f < 0 || f >= frame_count { continue }
                entries.push(E { pos: (*e).pos, end: (*e).pos + size, frame: f });
            }
            if entries.is_empty() { return vec![] }
            entries.sort_by_key(|e| e.pos);
            let mut spans = vec![FrameSpan::default(); frame_count as usize];
            for k in 0..entries.len() {
                let mut end = entries[k].end;
                if k + 1 < entries.len() { let next = entries[k + 1].pos; if next > end && next - end <= GAP_FILL { end = next; } }
                let s = &mut spans[entries[k].frame as usize];
                if s.pos < 0 { s.pos = entries[k].pos; s.len = end - entries[k].pos; }
            }
            spans
        }
    }
    /// 0 (and nothing further) when read-ahead is off or there are no spans. Any thread.
    pub fn attach(&self, path: &str, spans: Vec<FrameSpan>, fps: f64) -> Id {
        if self.inner.seconds <= 0.0 || spans.is_empty() { return 0 }
        let max_frames = ((self.inner.seconds * if fps > 0.0 { fps } else { 24.0 }).ceil() as i32).max(1);
        let name = path.rsplit(['/', '\\']).next().unwrap_or(path).to_string();
        let mut st = self.inner.state.lock().unwrap();
        let id = st.next_id; st.next_id += 1;
        st.clients.insert(id, Arc::new(Client { id, path: path.to_string(), name, spans, max_frames, position: AtomicI32::new(0), window_gen: AtomicU32::new(0), detached: AtomicBool::new(false), range_first: AtomicI32::new(-1), range_last: AtomicI32::new(-1), range_gen: AtomicU32::new(0), w: Mutex::new(Worker { seen_gen: NO_GEN, range_seen_gen: NO_GEN, last_page: -1, range_last_page: -1, window_logged: true, range_logged: true, ..Default::default() }) }));
        st.work_pending = true; drop(st); self.inner.cv.notify_one();
        id
    }
    pub fn detach(&self, id: Id) {
        if id == 0 { return }
        let mut st = self.inner.state.lock().unwrap();
        if let Some(c) = st.clients.remove(&id) { c.detached.store(true, Ordering::Release); }
    }
    /// The first frame the caller has not read yet. Cheap; any thread.
    pub fn set_position(&self, id: Id, next_frame: i32) {
        if id == 0 { return }
        { let mut st = self.inner.state.lock().unwrap();
          let Some(c) = st.clients.get(&id).cloned() else { return };
          let prev = c.position.swap(next_frame, Ordering::AcqRel);
          if next_frame < prev || next_frame > prev + c.max_frames { c.window_gen.fetch_add(1, Ordering::AcqRel); }   // a jump restarts the window
          st.work_pending = true; }
        self.inner.cv.notify_one();
    }
    /// A loop range (inclusive frames) to hydrate whole after the window; first < 0 clears it.
    pub fn set_range(&self, id: Id, first: i32, last: i32) {
        if id == 0 { return }
        { let mut st = self.inner.state.lock().unwrap();
          let Some(c) = st.clients.get(&id).cloned() else { return };
          let none = first < 0 || last < first; let (f, l) = if none { (-1, -1) } else { (first, last) };
          if c.range_first.load(Ordering::Relaxed) == f && c.range_last.load(Ordering::Relaxed) == l { return }
          c.range_first.store(f, Ordering::Relaxed); c.range_last.store(l, Ordering::Relaxed); c.range_gen.fetch_add(1, Ordering::Release);
          st.work_pending = true; }
        self.inner.cv.notify_one();
    }
    pub fn enabled(&self) -> bool { self.inner.seconds > 0.0 }
}

fn lower_priority() {
    #[cfg(target_os = "macos")] unsafe {
        unsafe extern "C" { fn pthread_set_qos_class_self_np(qos: u32, relative_priority: i32) -> i32; fn setiopolicy_np(iotype: i32, scope: i32, policy: i32) -> i32; }
        pthread_set_qos_class_self_np(0x09, 0);   // QOS_CLASS_UTILITY
        setiopolicy_np(0, 1, 3);                  // IOPOL_TYPE_DISK, IOPOL_SCOPE_THREAD, IOPOL_UTILITY
    }
    #[cfg(windows)] unsafe {
        use windows_sys::Win32::System::Threading::{GetCurrentThread, SetThreadPriority, THREAD_MODE_BACKGROUND_BEGIN};
        SetThreadPriority(GetCurrentThread(), THREAD_MODE_BACKGROUND_BEGIN);
    }
}

fn run(inner: &Inner, touched: &std::sync::atomic::AtomicU64) {
    lower_priority();
    let mut st = inner.state.lock().unwrap();
    while !st.stop {
        st.work_pending = false;
        let snapshot: Vec<Arc<Client>> = st.clients.values().cloned().collect();
        drop(st);
        // most urgent first: the client whose next unwarmed frame is closest to where its decoder is reading
        let mut best: Option<Arc<Client>> = None; let mut best_lead = i32::MAX;
        for c in &snapshot { if !prepare(inner, c) { continue } let lead = c.w.lock().unwrap().frontier - c.position.load(Ordering::Acquire); if lead < best_lead { best_lead = lead; best = Some(c.clone()); } }
        let mut did = false;
        if let Some(c) = &best { warm_one(inner, c, touched); did = true; }
        else { for c in &snapshot { if prepare_range(inner, c) { warm_range_one(inner, c, touched); did = true; break } } }
        st = inner.state.lock().unwrap();
        if !did { st = inner.cv.wait_while(st, |s| !s.stop && !s.work_pending).unwrap(); }
    }
}
/// Worker only: applies a pending restart and says whether there is a frame to warm.
fn prepare(inner: &Inner, c: &Client) -> bool {
    let mut w = c.w.lock().unwrap();
    if w.failed || c.detached.load(Ordering::Acquire) { return false }
    let g = c.window_gen.load(Ordering::Acquire); let pos = c.position.load(Ordering::Acquire);
    if g != w.seen_gen { if w.seen_gen != NO_GEN { w.restarts += 1; } w.seen_gen = g; w.frontier = pos; w.window_start = Some(Instant::now()); w.window_bytes = 0; w.window_logged = false; w.last_page = -1; }
    if w.frontier < pos { w.frontier = pos; }
    let last = (pos + c.max_frames).min(c.spans.len() as i32);
    let (mut bytes, mut end) = (0i64, pos);
    while end < last { let len = c.spans[end as usize].len.max(0); if end > pos && bytes + len > inner.cap_bytes { break } bytes += len; end += 1; }
    if w.frontier < end { return true }
    if !w.window_logged && w.window_bytes > 0 && w.window_logs < 4 {
        w.window_logged = true; w.window_logs += 1;
        let s = w.window_start.map(|t| t.elapsed().as_secs_f64()).unwrap_or(0.0);
        log::info!("ReadAhead: {} — window warm, {:.1} MB in {:.2} s ({:.1} MB/s)", c.name, w.window_bytes as f64 / 1e6, s, if s > 0.0 { w.window_bytes as f64 / 1e6 / s } else { 0.0 });
    }
    w.window_logged = true;
    false
}
fn prepare_range(inner: &Inner, c: &Client) -> bool {
    let mut w = c.w.lock().unwrap();
    if w.failed || c.detached.load(Ordering::Acquire) { return false }
    let g = c.range_gen.load(Ordering::Acquire);
    if g != w.range_seen_gen {
        w.range_seen_gen = g;
        let (first, last, n) = (c.range_first.load(Ordering::Relaxed), c.range_last.load(Ordering::Relaxed), c.spans.len() as i32);
        if first < 0 || first >= n { w.range_end = 0 } else { w.range_frontier = first; w.range_end = last.min(n - 1) + 1; }
        w.range_last_page = -1; w.range_bytes = 0; w.range_start = Some(Instant::now()); w.range_logged = false;
    }
    if w.range_frontier < w.range_end && w.range_bytes < inner.loop_cap_bytes { return true }
    if !w.range_logged && w.range_bytes > 0 {
        w.range_logged = true;
        let s = w.range_start.map(|t| t.elapsed().as_secs_f64()).unwrap_or(0.0);
        if w.range_frontier < w.range_end { log::info!("ReadAhead: {} — loop range stopped at the {} GB cap, frame {}", c.name, inner.loop_cap_bytes >> 30, w.range_frontier); }
        else { log::info!("ReadAhead: {} — loop range {}-{} touched, {:.1} MB in {:.2} s", c.name, c.range_first.load(Ordering::Relaxed), c.range_last.load(Ordering::Relaxed), w.range_bytes as f64 / 1e6, s); }
    }
    w.range_logged = true;
    false
}
fn open_file(c: &Client, w: &mut Worker) -> bool {
    if w.file.is_some() { return true }
    match File::open(&c.path) {
        Ok(f) => {
            #[cfg(target_os = "macos")] unsafe { use std::os::unix::io::AsRawFd; libc::fcntl(f.as_raw_fd(), libc::F_NOCACHE, 1); }   // the volume's cache is the point, not ours
            w.file = Some(f); true
        }
        Err(e) => { log::warn!("ReadAhead: {} — cannot open ({e}); read-ahead off for it", c.name); w.failed = true; false }
    }
}
/// Touches one frame's span: one byte in every page it covers. False if abandoned partway (a jump, a range change, a detach).
fn touch_frame(inner: &Inner, c: &Client, w: &mut Worker, frame: i32, range_pass: bool, abandon: &dyn Fn() -> bool, touched: &std::sync::atomic::AtomicU64) -> bool {
    let span = c.spans[frame as usize];
    if span.pos < 0 || span.len <= 0 { return true }
    let (first, last) = (span.pos / inner.page_bytes, (span.pos + span.len - 1) / inner.page_bytes);
    for p in first..=last {
        let lp = if range_pass { w.range_last_page } else { w.last_page };
        if p == lp { continue }
        if c.detached.load(Ordering::Acquire) || abandon() { return false }
        let t = Instant::now();
        let f = w.file.as_mut().unwrap();
        let mut b = [0u8; 1];
        let ok = f.seek(SeekFrom::Start((p * inner.page_bytes) as u64)).is_ok() && f.read(&mut b).map(|n| n == 1).unwrap_or(false);
        w.read_secs += t.elapsed().as_secs_f64();
        if !ok { break }
        touched.fetch_add(1, Ordering::Relaxed);
        if range_pass { w.range_last_page = p } else { w.last_page = p }
    }
    w.bytes += span.len;
    true
}
fn warm_one(inner: &Inner, c: &Client, touched: &std::sync::atomic::AtomicU64) {
    let mut w = c.w.lock().unwrap();
    if !open_file(c, &mut w) { return }
    let g = w.seen_gen; let frontier = w.frontier;
    if !touch_frame(inner, c, &mut w, frontier, false, &|| c.window_gen.load(Ordering::Acquire) != g, touched) { return }
    w.window_bytes += c.spans[frontier as usize].len.max(0); w.frontier += 1;
    if w.bytes - w.last_log_bytes >= (512 << 20) { w.last_log_bytes = w.bytes; log::info!("ReadAhead: {} — so far {:.0} MB touched, {:.1} MB/s while touching, {} restarts", c.name, w.bytes as f64 / 1e6, if w.read_secs > 0.0 { w.bytes as f64 / 1e6 / w.read_secs } else { 0.0 }, w.restarts); }
}
fn warm_range_one(inner: &Inner, c: &Client, touched: &std::sync::atomic::AtomicU64) {
    let mut w = c.w.lock().unwrap();
    if !open_file(c, &mut w) { return }
    let (rg, wg, frontier) = (w.range_seen_gen, w.seen_gen, w.range_frontier);
    if !touch_frame(inner, c, &mut w, frontier, true, &|| c.range_gen.load(Ordering::Acquire) != rg || c.window_gen.load(Ordering::Acquire) != wg, touched) { return }
    w.range_bytes += c.spans[frontier as usize].len.max(0); w.range_frontier += 1;
}
impl Drop for ReadAhead { fn drop(&mut self) { { self.inner.state.lock().unwrap().stop = true; } self.inner.cv.notify_all(); if let Some(h) = self.thread.lock().unwrap().take() { h.join().ok(); } } }
