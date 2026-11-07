//! `ScrubDecoder` — ported from `QCView-Player/src/decode/scrub_decoder.{h,cpp}` (Phase 1.8.5 + the decode-once-per-GOP
//! rework): a separate decoder for fast single-frame fetches during scrub gestures. Owns its own FFmpeg session + hwaccel
//! so it can seek freely without disturbing the streaming `VideoDecoder`'s read position or codec state; shares the
//! `FrameIndex`; publishes into the streaming decoder's slot (`publish_external`) so the renderer keeps ONE fetch path.
//!
//! The three-way decision per request: (1) cache hit — served without touching the decoder (backward scrub and revisits
//! are free); (2) forward-no-reseek — the decoder is positioned just behind the target within `FORWARD_REACH` frames
//! (inter codecs): keep decoding forward, no seek, no flush, caching every frame; (3) cold / backward / long jump — seek
//! to the keyframe at-or-before the target (an IDR, exactly what VideoToolbox needs after a flush) and decode forward.
//! Intra codecs (ProRes / DNxHD / MJPEG / raw) take the direct path on every platform: seek-exact + decode one, no cache.
//!
//! The hwaccel is gated on intra-only codecs: VideoToolbox's async B-frame reorder loses reference frames after
//! `avcodec_flush_buffers`, which scrub calls on every seek (50–200 ms per step rebuilding the session; software decode of
//! a keyframe + a short P/B chain is far faster on Apple silicon). `hw_inter_scrub` lifts the gate — QCView's
//! `performance/hwScrubInterCodecs`, default off there; the platform defaults it ON on Windows (row 155: S2 measured
//! D3D12VA scrub at 7.5 ms vs 42.8 software), where the cache reads pool frames back to planes.
//!
//! NO mid-decode preemption: a continuous 60 Hz drag fires a request every ~16 ms; abandoning each decode would freeze
//! the player until release. Each decode finishes and the worker takes the LATEST pending target next. The platform's one
//! measured addition (S2): while filling forward, a newer target AHEAD within reach RETARGETS the fill instead of
//! publishing the stale one first.
use crate::{ffmpeg::{HwFrame, OpenOptions, RecvRaw, Session}, frame_index::FrameIndex, hw::HwDevice, rgb_range::RangeOverride, scrub_frame_cache::{Kind, ScrubCacheEntry}, simple_lru::SimpleLru, video_decoder::ExternalSink, Error};
use ffmpeg_sys_next as ff;
use std::{sync::{atomic::{AtomicBool, AtomicI32, AtomicU8, AtomicU64, Ordering}, Arc, Condvar, Mutex}, thread, time::Instant};

/// ~1–2 GOPs; past this, seeking to the nearest keyframe is cheaper than decoding the whole gap.
pub const FORWARD_REACH: i32 = 90;

#[derive(Clone)]
pub struct ScrubOptions { pub hw: Option<HwDevice>, /// lift the intra-only hwaccel gate for inter codecs (QCView `performance/hwScrubInterCodecs`; the platform: on for Windows)
    pub hw_inter_scrub: bool, /// the GOP cache budget (QCView `performance/scrubCacheMB`, 512)
    pub budget_bytes: usize, pub range_override: RangeOverride, /// the scrub session's thread count; 0 = auto (QCView's choice) — a knob for measurement only
    pub threads: i32, /// keep per-request timing (the harness)
    pub stats: bool }
impl Default for ScrubOptions { fn default() -> Self { ScrubOptions { hw: None, hw_inter_scrub: cfg!(windows), budget_bytes: 512 << 20, range_override: RangeOverride::Auto, threads: 0, stats: false } } }

/// One served request, for the harness: when it was issued, when the frame reached the slot, how.
#[derive(Clone, Copy, Debug)]
pub struct Served { pub id: u64, pub target: i32, pub issued: Instant, pub done: Instant, pub hit: bool, pub seeked: bool, pub decoded: u32, pub retargeted: bool }

/// `pending` packs (request id << 32) | (target + 1); 0 = nothing pending — one atomic so the worker never pairs a target with another request's id.
struct Shared { pending: AtomicU64, stop: AtomicBool, cv: Condvar, lock: Mutex<()>, range_override: AtomicU8, cache_invalidate: AtomicBool, last_shown: AtomicI32, issued: Mutex<Vec<(u64, i32, Instant)>> }

pub struct ScrubDecoder {
    shared: Arc<Shared>, thread: Option<thread::JoinHandle<()>>, next_id: u64,
    pub served: Option<crossbeam_channel::Receiver<Served>>, pub intra_direct: bool, pub hw_active: bool,
}

impl ScrubDecoder {
    /// Open the file on the calling thread (errors come back) and start the worker. `sink` is the streaming decoder's slot.
    pub fn open(path: &str, opts: &ScrubOptions, index: Arc<FrameIndex>, sink: ExternalSink) -> Result<ScrubDecoder, Error> {
        // hwaccel: intra-only, or inter when the gate is lifted; the session's thread policy gives intra codecs slice threads
        let probe_intra = { let cpath = std::ffi::CString::new(path).map_err(|_| Error::Open("path".into()))?; unsafe {
            let mut fmt: *mut ff::AVFormatContext = std::ptr::null_mut();
            if ff::avformat_open_input(&mut fmt, cpath.as_ptr(), std::ptr::null_mut(), std::ptr::null_mut()) < 0 { return Err(Error::Open("avformat_open_input".into())) }
            let mut intra = false;
            if ff::avformat_find_stream_info(fmt, std::ptr::null_mut()) >= 0 { for i in 0..(*fmt).nb_streams { let cp = (**(*fmt).streams.add(i as usize)).codecpar; if (*cp).codec_type == ff::AVMediaType::AVMEDIA_TYPE_VIDEO { let d = ff::avcodec_descriptor_get((*cp).codec_id); intra = !d.is_null() && ((*d).props & ff::AV_CODEC_PROP_INTRA_ONLY) != 0; break } } }
            ff::avformat_close_input(&mut fmt); intra } };
        let hw_eligible = probe_intra || opts.hw_inter_scrub;
        let session_opts = OpenOptions { threads: opts.threads, hw: if hw_eligible { opts.hw.clone() } else { None }, seq: None, fast: false };   // NOT the user's thread count: scrub runs beside the main decoder during seek bursts
        let mut session = Session::open(path, &session_opts)?;
        session.range_override = opts.range_override;
        let hw_active = !session.info.hw_accel.is_empty();
        if hw_active && !probe_intra { log::info!("ScrubDecoder: hardware scrub enabled for an inter codec (hw_inter_scrub)"); }
        let shared = Arc::new(Shared { pending: AtomicU64::new(0), stop: AtomicBool::new(false), cv: Condvar::new(), lock: Mutex::new(()), range_override: AtomicU8::new(opts.range_override as u8), cache_invalidate: AtomicBool::new(false), last_shown: AtomicI32::new(-1), issued: Mutex::new(vec![]) });
        let (stx, srx) = if opts.stats { let (a, b) = crossbeam_channel::unbounded(); (Some(a), Some(b)) } else { (None, None) };
        let (sh, budget, intra_direct) = (shared.clone(), opts.budget_bytes, probe_intra);
        let thread = thread::Builder::new().name("qcv-scrub".into()).spawn(move || worker(session, sh, index, sink, budget, intra_direct, stx)).map_err(|e| Error::Open(e.to_string()))?;
        Ok(ScrubDecoder { shared, thread: Some(thread), next_id: 0, served: srx, intra_direct, hw_active })
    }
    /// Latest target wins; cheap at slider-drag rate. Returns the request id (the harness matches `Served` on it).
    pub fn request_frame(&mut self, frame_no: i32) -> u64 {
        self.next_id += 1;
        if self.served.is_some() { let mut v = self.shared.issued.lock().unwrap(); if v.len() > 4096 { v.drain(..2048); } v.push((self.next_id, frame_no.max(0), Instant::now())); }
        self.shared.pending.store(pack(self.next_id, frame_no.max(0)), Ordering::Release);
        { let _g = self.shared.lock.lock().unwrap(); } self.shared.cv.notify_one();
        self.next_id
    }
    pub fn last_shown(&self) -> i32 { self.shared.last_shown.load(Ordering::Acquire) }
    /// The Range pill: CPU entries baked the old range into their levels and native ones snapshot it — the worker drops
    /// the cache before its next decode, and the held frame is re-requested so a paused scrub updates live.
    pub fn set_range_override(&mut self, r: RangeOverride) {
        let prev = self.shared.range_override.swap(r as u8, Ordering::AcqRel);
        if prev != r as u8 { self.shared.cache_invalidate.store(true, Ordering::Release); let held = self.last_shown(); if held >= 0 { self.request_frame(held); } }
    }
    pub fn close(&mut self) {
        self.shared.stop.store(true, Ordering::Release); { let _g = self.shared.lock.lock().unwrap(); } self.shared.cv.notify_all();
        if let Some(h) = self.thread.take() { h.join().ok(); }   // the cache clears and the session drops on the worker, before the contexts go
    }
}
impl Drop for ScrubDecoder { fn drop(&mut self) { self.close(); } }

fn pack(id: u64, target: i32) -> u64 { ((id & 0xffff_ffff) << 32) | (target as u64 + 1) }
fn unpack(v: u64) -> Option<(u64, i32)> { if v == 0 { None } else { Some((v >> 32, (v & 0xffff_ffff) as i32 - 1)) } }

fn worker(mut s: Session, sh: Arc<Shared>, index: Arc<FrameIndex>, sink: ExternalSink, budget: usize, intra_direct: bool, stats: Option<crossbeam_channel::Sender<Served>>) {
    let cache: SimpleLru<i32, Arc<ScrubCacheEntry>> = SimpleLru::new(budget);
    let mut pos: i32 = -1; let mut positioned = false;
    while !sh.stop.load(Ordering::Acquire) {
        let Some((id, target)) = unpack(sh.pending.swap(0, Ordering::AcqRel)) else {
            let g = sh.lock.lock().unwrap();
            let _g = sh.cv.wait_while(g, |_| !sh.stop.load(Ordering::Acquire) && sh.pending.load(Ordering::Acquire) == 0).unwrap();
            continue;
        };
        if target == sh.last_shown.load(Ordering::Acquire) { continue }   // already showing it (Guide 13 §I.7)
        if sh.cache_invalidate.swap(false, Ordering::AcqRel) { cache.clear(); positioned = false; pos = -1; }
        s.range_override = match sh.range_override.load(Ordering::Acquire) { 1 => RangeOverride::Full, 2 => RangeOverride::Limited, _ => RangeOverride::Auto };
        let issued = if stats.is_some() { let mut v = sh.issued.lock().unwrap(); let t = v.iter().find(|(i, _, _)| *i == id).map(|(_, _, t)| *t); v.retain(|(i, _, _)| *i > id); t } else { None };
        let mut stat = Served { id, target, issued: issued.unwrap_or_else(Instant::now), done: Instant::now(), hit: false, seeked: false, decoded: 0, retargeted: false };
        // 1. cache hit — `get` touches: the frames around the playhead stay hot
        if let Some(hit) = cache.get(&target) {
            publish(&mut s, &sink, &index, &hit); sh.last_shown.store(target, Ordering::Release);
            stat.hit = true; stat.done = Instant::now(); if let Some(t) = &stats { t.send(stat).ok(); }
            continue;
        }
        let mut target = target;
        let target_pts = index.pts_for_frame(target);
        // 2. forward-no-reseek (inter codecs) / 3. keyframe seek + flush
        let can_forward = !intra_direct && positioned && target > pos && (target - pos) <= FORWARD_REACH;
        if !can_forward {
            if s.seek(target_pts, ff::AVSEEK_FLAG_BACKWARD) < 0 { log::warn!("ScrubDecoder: seek failed for frame {target}"); continue }
            positioned = false; pos = -1; stat.seeked = true;
        }
        // decode forward, caching every frame, until the target lands
        let mut target_pts = target_pts;
        let mut eof_sent = false; let mut done = false;
        let (mut t_read, mut t_recv, mut t_entry, mut t_pub) = (0f32, 0f32, 0f32, 0f32);
        'fill: while !sh.stop.load(Ordering::Acquire) && !done {
            if !eof_sent { let t = Instant::now(); let r = s.read_and_send(); t_read += t.elapsed().as_secs_f32() * 1000.0; match r { crate::ffmpeg::Read::Sent => {}, crate::ffmpeg::Read::Eof => eof_sent = true, crate::ffmpeg::Read::Error(_) => { positioned = false; break } } }
            loop {
                let t = Instant::now(); let r = s.receive_raw(); t_recv += t.elapsed().as_secs_f32() * 1000.0;
                match r {
                    RecvRaw::Again => break,
                    RecvRaw::Eof | RecvRaw::Error(_) => { positioned = false; break 'fill }
                    RecvRaw::Frame(f) => {
                        stat.decoded += 1;
                        let (fpts, w, h) = unsafe { ((if (*f).best_effort_timestamp != ff::AV_NOPTS_VALUE { (*f).best_effort_timestamp } else { (*f).pts }), (*f).width as u32, (*f).height as u32) };
                        // S2's addition: a newer target AHEAD within reach retargets the fill (inter only); otherwise finish, latest next
                        if !intra_direct { let raw = sh.pending.load(Ordering::Acquire); if let Some((_, np)) = unpack(raw) { if np > target && np - target <= FORWARD_REACH && sh.pending.compare_exchange(raw, 0, Ordering::AcqRel, Ordering::Acquire).is_ok() { target = np; target_pts = index.pts_for_frame(np); stat.target = np; stat.retargeted = true; } } }
                        let is_target = fpts >= target_pts;
                        let frame_no = if is_target { target } else { index.frame_for_pts(fpts) };   // the sought frame is keyed exactly (immune to rounding)
                        let t = Instant::now(); let entry = make_entry(&mut s, f, fpts, w, h);
                        s.release_raw(); t_entry += t.elapsed().as_secs_f32() * 1000.0;
                        pos = frame_no; positioned = true;
                        if let Some(e) = entry {
                            if !intra_direct { cache.add(frame_no, e.clone(), e.bytes()); }
                            if is_target { let t = Instant::now(); publish(&mut s, &sink, &index, &e); t_pub += t.elapsed().as_secs_f32() * 1000.0; sh.last_shown.store(target, Ordering::Release); done = true; break }
                        } else if is_target { done = true; break }
                    }
                }
            }
            if eof_sent && !done { positioned = false; break }
        }
        stat.done = Instant::now(); if let Some(t) = &stats { if std::env::var_os("QCV_SCRUB_ROWS").is_some() { log::info!("fill {target}: read {t_read:.1} recv {t_recv:.1} entry {t_entry:.1} publish {t_pub:.1} ms, decoded {}", stat.decoded); } t.send(stat).ok(); }
    }
    log::info!("ScrubDecoder: cache at close {} entries, {:.0} MB of {:.0}", cache.count(), cache.current_bytes() as f64 / 1048576.0, cache.max_bytes() as f64 / 1048576.0);
    cache.clear();   // before the session's contexts go (the entries hold refs into them)
}

/// QCView's `makeCacheEntry`: VideoToolbox surfaces are held (CoreVideo pools grow); pool-based backends are read back
/// to planes; software frames are cloned (a refcount bump).
fn make_entry(s: &mut Session, f: *mut ff::AVFrame, pts: i64, w: u32, h: u32) -> Option<Arc<ScrubCacheEntry>> {
    unsafe {
        let fmt = std::mem::transmute::<i32, ff::AVPixelFormat>((*f).format);
        if fmt == crate::hw::HW_PIX_FMT && fmt != ff::AVPixelFormat::AV_PIX_FMT_NONE {
            #[cfg(target_os = "macos")]
            { let pix = (*f).data[3] as *mut std::ffi::c_void; if min_present::import::metal::zero_copy_supported(pix) { return Some(ScrubCacheEntry::native(HwFrame(ff::av_frame_clone(f)), w, h, pts)) } }
            // pool-based (D3D12VA) or an unbridged VT layout: read back
            let sw = s.hw_readback(f)?;
            let bytes = ff::av_image_get_buffer_size(std::mem::transmute::<i32, ff::AVPixelFormat>((*sw).format), (*sw).width, (*sw).height, 1);
            return Some(ScrubCacheEntry::planar(HwFrame(ff::av_frame_clone(sw)), pts, if bytes > 0 { bytes as usize } else { (w as usize) * (h as usize) * 2 }));
        }
        let bytes = ff::av_image_get_buffer_size(fmt, (*f).width, (*f).height, 1);
        Some(ScrubCacheEntry::planar(HwFrame(ff::av_frame_clone(f)), pts, if bytes > 0 { bytes as usize } else { (w as usize) * (h as usize) * 2 }))
    }
}

/// QCView's `publishEntry`: native entries re-ref and publish zero-copy; planar entries become planes at show time (the
/// deferred convert — one repack of the frame actually shown, never of the GOP fill).
fn publish(s: &mut Session, sink: &ExternalSink, index: &FrameIndex, e: &Arc<ScrubCacheEntry>) {
    // Native (VideoToolbox) entries re-ref the CVPixelBuffer inside `wrap`; Planar ones repack planes there — the deferred convert
    debug_assert!(matches!(e.kind(), Kind::Native | Kind::Planar));
    match unsafe { s.handle_from_avframe(e.avframe(), index) } { Ok(h) => sink.publish(h), Err(err) => log::warn!("ScrubDecoder: publish: {err}") }
}
