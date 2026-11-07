//! `VideoDecoder` — ported from `QCView-Player/src/decode/video_decoder.{h,cpp}`: a background thread reads packets,
//! decodes, and PUBLISHES frames; the render side takes them with `fetch_latest`. Seeks go through an atomic slot
//! (latest wins; the decode thread runs each seek to completion — aborting mid-decode breaks fast-seek on B-frame media),
//! pause blocks the decode thread on the same condvar, EOF auto-pauses ("play stops at the end", Premiere / Resolve)
//! and waits for a seek. The scrub decoder publishes into the same slot (`publish_external`) so the renderer has ONE
//! fetch path. `FrameIndex` (tier 2 scan started at open), `ReadAhead` (attached at open, read position reported per
//! frame), the Range pill, the decode-health surface and the playback speed are carried as in QCView.
//!
//! The one DIVERGENCE, as a policy: `Policy::Paced` is QCView's (the decode thread sleeps until `baseline + (pts −
//! baseline_pts) / speed` and publishes into a latest-wins slot of one; the renderer draws on `frame_available`).
//! `Policy::Clocked` holds a short queue of frames decoded AHEAD (bounded — the decode thread blocks when it is full,
//! which is the memory discipline of QCView's slot) and the consumer picks, each refresh, the newest frame at or before
//! its rational clock (`fetch_due`). S2 measured the clocked design at exact 3:2 cadence with 0 drops against P0's
//! 17.4–20.5 ms p95 for the paced one; both stay, and `apps/qcv` runs clocked.
use crate::{decoder_cleanup, ffmpeg::{av_err, OpenOptions, Read, Recv, Session, SessionInfo}, frame_handle::FrameHandle, frame_index::FrameIndex, read_ahead::{self, ReadAhead}, rgb_range::RangeOverride, Error};
use ffmpeg_sys_next as ff;
use std::{collections::VecDeque, sync::{atomic::{AtomicBool, AtomicI32, AtomicU32, AtomicU64, AtomicU8, Ordering}, Arc, Condvar, Mutex}, thread, time::{Duration, Instant}};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State { Idle = 0, Opening = 1, Decoding = 2, EndOfStream = 3, Errored = 4 }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Policy { /// QCView's: the decoder paces itself, a latest-wins slot of one
    Paced, /// the platform's: `lookahead` frames decoded ahead, the consumer's clock picks per refresh
    Clocked { lookahead: usize } }

struct Shared {
    queue: Mutex<VecDeque<FrameHandle>>, cv: Condvar, policy: Policy,
    pending_seek: AtomicI32, is_playing: AtomicBool, stop: AtomicBool, state: AtomicU8,
    published_seq: AtomicU64, last_fetched_seq: AtomicU64, seeks_performed: AtomicU32, current_frame: AtomicI32,
    decode_errors: AtomicU32, last_error: Mutex<String>, speed_bits: AtomicU64, pace_dirty: AtomicBool, range_override: AtomicU8,
    read_ahead: AtomicU64,
}
impl Shared {
    fn set_state(&self, s: State) { self.state.store(s as u8, Ordering::Release); }
    fn record_error(&self, msg: String) { self.decode_errors.fetch_add(1, Ordering::AcqRel); log::warn!("VideoDecoder: {msg}"); *self.last_error.lock().unwrap() = msg; }
    fn speed(&self) -> f64 { f64::from_bits(self.speed_bits.load(Ordering::Acquire)) }
    fn range_override(&self) -> RangeOverride { match self.range_override.load(Ordering::Acquire) { 1 => RangeOverride::Full, 2 => RangeOverride::Limited, _ => RangeOverride::Auto } }
}

/// The streaming decoder's slot as seen by an external publisher (the scrub decoder): QCView's `publishExternalFrame`.
#[derive(Clone)]
pub struct ExternalSink(Arc<Shared>);
impl ExternalSink {
    pub fn publish(&self, mut handle: FrameHandle) {
        handle.frame_no = -1;   // due now, whatever the clock says
        let mut q = self.0.queue.lock().unwrap(); q.clear(); q.push_back(handle); drop(q);
        self.0.published_seq.fetch_add(1, Ordering::AcqRel); self.0.pace_dirty.store(true, Ordering::Release); self.0.cv.notify_all();
    }
}

pub struct VideoDecoder {
    shared: Arc<Shared>, thread: Option<thread::JoinHandle<()>>,
    pub info: Option<SessionInfo>, index: Option<Arc<FrameIndex>>, pub source_path: String,
}

impl VideoDecoder {
    pub fn new(policy: Policy) -> VideoDecoder {
        VideoDecoder { shared: Arc::new(Shared { queue: Mutex::new(VecDeque::new()), cv: Condvar::new(), policy, pending_seek: AtomicI32::new(-1), is_playing: AtomicBool::new(false), stop: AtomicBool::new(false), state: AtomicU8::new(0), published_seq: AtomicU64::new(0), last_fetched_seq: AtomicU64::new(0), seeks_performed: AtomicU32::new(0), current_frame: AtomicI32::new(-1), decode_errors: AtomicU32::new(0), last_error: Mutex::new(String::new()), speed_bits: AtomicU64::new(1.0f64.to_bits()), pace_dirty: AtomicBool::new(false), range_override: AtomicU8::new(0), read_ahead: AtomicU64::new(0) }), thread: None, info: None, index: None, source_path: String::new() }
    }
    /// Opens the file and starts the decode thread (playing: QCView's default once a file is open).
    pub fn open(&mut self, path: &str, opts: &OpenOptions) -> Result<(), Error> {
        self.close();
        self.shared.set_state(State::Opening);
        let mut session = match Session::open(path, opts) { Ok(s) => s, Err(e) => { self.shared.set_state(State::Errored); return Err(e) } };
        session.range_override = self.shared.range_override();
        let index = Arc::new(session.frame_index());
        if index.is_valid() {
            let spans = session.read_ahead_spans(&index);
            self.shared.read_ahead.store(ReadAhead::instance().attach(path, spans, index.fps()), Ordering::Release);
        }
        index.start_scan(path, session.stream_idx);
        self.info = Some(session.info.clone()); self.index = Some(index.clone()); self.source_path = path.to_string();
        let sh = self.shared.clone();
        sh.stop.store(false, Ordering::Release); sh.published_seq.store(0, Ordering::Release); sh.last_fetched_seq.store(0, Ordering::Relaxed); sh.pending_seek.store(-1, Ordering::Release); sh.current_frame.store(-1, Ordering::Release);
        sh.is_playing.store(true, Ordering::Release);
        self.shared.set_state(State::Decoding);
        self.thread = Some(thread::Builder::new().name("qcv-decode".into()).spawn(move || decode_loop(session, sh, index)).map_err(|e| Error::Open(e.to_string()))?);
        Ok(())
    }
    pub fn close(&mut self) {
        self.shared.stop.store(true, Ordering::Release); self.shared.cv.notify_all();
        if let Some(h) = self.thread.take() { h.join().ok(); }   // the Session drops on its own thread; the slow parts ride the cleanup queue
        let id = self.shared.read_ahead.swap(0, Ordering::AcqRel); ReadAhead::instance().detach(id);
        if let Some(i) = self.index.take() { decoder_cleanup::post(Box::new(move || drop(i))); }
        self.shared.queue.lock().unwrap().clear();
        self.info = None; self.source_path.clear();
        if self.state() != State::Errored { self.shared.set_state(State::Idle); }
    }
    pub fn state(&self) -> State { match self.shared.state.load(Ordering::Acquire) { 1 => State::Opening, 2 => State::Decoding, 3 => State::EndOfStream, 4 => State::Errored, _ => State::Idle } }
    pub fn frame_index(&self) -> Option<&Arc<FrameIndex>> { self.index.as_ref() }
    pub fn is_playing(&self) -> bool { self.shared.is_playing.load(Ordering::Acquire) }
    pub fn play(&self) { self.shared.is_playing.store(true, Ordering::Release); self.shared.pace_dirty.store(true, Ordering::Release); self.shared.cv.notify_all(); }
    pub fn pause(&self) { self.shared.is_playing.store(false, Ordering::Release); self.shared.cv.notify_all(); }
    pub fn toggle_playback(&self) { if self.is_playing() { self.pause() } else { self.play() } }
    /// Latest request wins; an in-flight seek's target is replaced for the NEXT seek, never aborted mid-decode.
    pub fn seek_to_frame(&self, frame_no: i32) { self.shared.pending_seek.store(frame_no.max(0), Ordering::Release); self.shared.cv.notify_all(); }
    pub fn seeks_performed(&self) -> u32 { self.shared.seeks_performed.load(Ordering::Acquire) }
    pub fn set_playback_speed(&self, speed: f64) { self.shared.speed_bits.store(speed.to_bits(), Ordering::Release); self.shared.pace_dirty.store(true, Ordering::Release); }
    pub fn playback_speed(&self) -> f64 { self.shared.speed() }
    pub fn current_frame(&self) -> i32 { self.shared.current_frame.load(Ordering::Acquire) }
    pub fn decode_error_count(&self) -> u32 { self.shared.decode_errors.load(Ordering::Acquire) }
    pub fn last_decode_error(&self) -> String { self.shared.last_error.lock().unwrap().clone() }
    /// True once any frame has been published since open (the playlist orchestrator's pre-warm check).
    pub fn is_ready(&self) -> bool { self.shared.published_seq.load(Ordering::Acquire) > 0 }
    /// The Range pill; takes effect on the next decoded frame (QCView re-decodes the current frame too — step 2's Transport).
    pub fn set_range_override(&self, r: RangeOverride) { self.shared.range_override.store(r as u8, Ordering::Release); }
    pub fn set_read_ahead_range(&self, first: i32, last: i32) { ReadAhead::instance().set_range(self.shared.read_ahead.load(Ordering::Acquire), first, last); }
    /// QCView's `fetchLatest`: if a frame newer than the last one consumed is available, MOVES it out. Older queued frames
    /// are dropped on the floor (the latest wins).
    pub fn fetch_latest(&self) -> Option<FrameHandle> {
        let latest = self.shared.published_seq.load(Ordering::Acquire);
        if latest == self.shared.last_fetched_seq.load(Ordering::Relaxed) { return None }
        let mut q = self.shared.queue.lock().unwrap();
        let h = q.pop_back(); q.clear();
        drop(q); self.shared.cv.notify_all();
        self.shared.last_fetched_seq.store(latest, Ordering::Relaxed);
        h
    }
    /// The clocked consumer: the newest queued frame at or before `frame` (older ones are dropped and counted); frames
    /// beyond `frame` wait for their refresh. External (scrub) publishes are always due.
    pub fn fetch_due(&self, frame: i32) -> (Option<FrameHandle>, u32) {
        let mut q = self.shared.queue.lock().unwrap();
        let mut taken: Option<FrameHandle> = None; let mut dropped = 0u32;
        while let Some(front) = q.front() {
            if front.frame_no > frame && front.frame_no >= 0 { break }
            let h = q.pop_front().unwrap();
            if taken.is_some() { dropped += 1; }
            taken = Some(h);
        }
        drop(q);
        if taken.is_some() { self.shared.cv.notify_all(); self.shared.last_fetched_seq.store(self.shared.published_seq.load(Ordering::Acquire), Ordering::Relaxed); }
        (taken, dropped)
    }
    /// External publish entry (the scrub decoder): bypasses pacing, replaces whatever is queued, marks the frame due now.
    pub fn publish_external(&self, handle: FrameHandle) { self.external_sink().publish(handle) }
    pub fn queued(&self) -> usize { self.shared.queue.lock().unwrap().len() }
    pub fn external_sink(&self) -> ExternalSink { ExternalSink(self.shared.clone()) }
}
impl Drop for VideoDecoder { fn drop(&mut self) { self.close(); } }

/// Publish per policy. Paced: sleep to the frame's time, then latest-wins into a slot of one. Clocked: block while the
/// queue holds `lookahead` frames (seek / stop / pause wake it), then append. Returns false on stop.
fn publish(sh: &Shared, h: FrameHandle, pace: bool, baseline: &mut Option<(Instant, i64, (i32, i32))>, tb: (i32, i32)) -> bool {
    sh.current_frame.store(h.frame_no, Ordering::Release);
    match sh.policy {
        Policy::Paced => {
            if pace && sh.is_playing.load(Ordering::Acquire) {
                if sh.pace_dirty.swap(false, Ordering::AcqRel) { *baseline = None; }
                match baseline {
                    None => *baseline = Some((Instant::now(), h.pts, tb)),
                    Some((wall, pts0, tb)) => {
                        let secs_per_tick = tb.0 as f64 / tb.1 as f64; let speed = sh.speed();
                        let target = *wall + Duration::from_secs_f64(((h.pts - *pts0) as f64 * secs_per_tick / if speed > 0.0 { speed } else { 1.0 }).max(0.0));
                        let now = Instant::now(); if target > now { thread::sleep(target - now); }
                    }
                }
            }
            let mut q = sh.queue.lock().unwrap(); q.clear(); q.push_back(h);
        }
        Policy::Clocked { lookahead } => {
            let mut q = sh.queue.lock().unwrap();
            while q.len() >= lookahead.max(1) {
                if sh.stop.load(Ordering::Acquire) || sh.pending_seek.load(Ordering::Acquire) >= 0 { return !sh.stop.load(Ordering::Acquire) }
                q = sh.cv.wait_timeout(q, Duration::from_millis(50)).unwrap().0;
            }
            q.push_back(h);
        }
    }
    sh.published_seq.fetch_add(1, Ordering::AcqRel);
    true
}

/// QCView's `decodeLoop`, with `performSeek` inline.
fn decode_loop(mut s: Session, sh: Arc<Shared>, index: Arc<FrameIndex>) {
    let tb = s.info.time_base;
    let mut baseline: Option<(Instant, i64, (i32, i32))> = None;
    let mut eof_sent = false; let mut at_eof = false;
    let read_ahead_id = sh.read_ahead.load(Ordering::Acquire);
    let mut stat_start = Instant::now(); let mut stat_frames = 0u32; let mut stat_read_ms = 0.0f64; let mut stat_max_read = 0.0f64;
    while !sh.stop.load(Ordering::Acquire) {
        s.range_override = sh.range_override();
        // ---- pending seek: latest request wins
        let target = sh.pending_seek.swap(-1, Ordering::AcqRel);
        if target >= 0 {
            baseline = None;
            perform_seek(&mut s, &sh, &index, target, read_ahead_id);
            sh.seeks_performed.fetch_add(1, Ordering::AcqRel);
            eof_sent = false; at_eof = false; sh.set_state(State::Decoding);
            continue;
        }
        // ---- paused: block until play(), a seek, or stop; the last published frame stays visible
        if !sh.is_playing.load(Ordering::Acquire) {
            let g = sh.queue.lock().unwrap();
            let _g = sh.cv.wait_while(g, |_| !sh.stop.load(Ordering::Acquire) && sh.pending_seek.load(Ordering::Acquire) < 0 && !sh.is_playing.load(Ordering::Acquire)).unwrap();
            baseline = None;
            continue;
        }
        // ---- drained: auto-pause and block until a seek arrives
        if at_eof {
            sh.is_playing.store(false, Ordering::Release);
            let g = sh.queue.lock().unwrap();
            let _g = sh.cv.wait_while(g, |_| !sh.stop.load(Ordering::Acquire) && sh.pending_seek.load(Ordering::Acquire) < 0).unwrap();
            continue;
        }
        if stat_start.elapsed().as_secs_f64() >= 2.0 {
            let secs = stat_start.elapsed().as_secs_f64();
            if stat_frames > 0 { log::debug!("VideoDecoder: playing {:.1} fps over {:.1} s; read waits {:.0} ms total ({:.0}%), slowest {:.0} ms", stat_frames as f64 / secs, secs, stat_read_ms, 100.0 * stat_read_ms / (secs * 1000.0), stat_max_read); }
            stat_start = Instant::now(); stat_frames = 0; stat_read_ms = 0.0; stat_max_read = 0.0;
        }
        // ---- normal decode: read a packet, send, drain
        if !eof_sent {
            let t = Instant::now();
            match s.read_and_send() {
                Read::Sent => {}
                Read::Eof => eof_sent = true,
                Read::Error(e) => { sh.record_error(format!("av_read_frame: {}", av_err(e))); sh.set_state(State::Errored); break }
            }
            let ms = t.elapsed().as_secs_f64() * 1000.0; stat_read_ms += ms; if ms > stat_max_read { stat_max_read = ms; }
        }
        loop {
            match s.receive(&index) {
                Recv::Again => break,
                Recv::Eof => { sh.set_state(State::EndOfStream); at_eof = true; break }
                Recv::Error(e) => { sh.record_error(format!("avcodec_receive_frame: {}", av_err(e))); sh.set_state(State::Errored); return }
                Recv::Frame(h) => {
                    stat_frames += 1;
                    read_ahead::ReadAhead::instance().set_position(read_ahead_id, h.frame_no + 1);
                    if !publish(&sh, h, true, &mut baseline, tb) { return }
                }
            }
        }
    }
}

/// QCView's `performSeek`: the keyframe at-or-before the target, decode forward, publish the first frame at/after it.
fn perform_seek(s: &mut Session, sh: &Shared, index: &FrameIndex, target_frame: i32, read_ahead_id: u64) {
    if !index.is_valid() { return }
    let target_pts = index.pts_for_frame(target_frame);
    if s.seek(target_pts, ff::AVSEEK_FLAG_BACKWARD) < 0 { sh.record_error(format!("av_seek_frame failed for frame {target_frame}")); return }
    { sh.queue.lock().unwrap().clear(); }
    read_ahead::ReadAhead::instance().set_position(read_ahead_id, target_frame);
    let mut eof = false;
    while !sh.stop.load(Ordering::Acquire) {
        if !eof { match s.read_and_send() { Read::Sent => {}, Read::Eof => eof = true, Read::Error(e) => { sh.record_error(format!("read_frame after seek: {}", av_err(e))); return } } }
        loop {
            match s.receive(index) {
                Recv::Again => break,
                Recv::Eof | Recv::Error(_) => return,   // EOF before reaching the target
                Recv::Frame(h) => {
                    if h.pts >= target_pts { let mut b = None; publish(sh, h, false, &mut b, s.info.time_base); return }
                    // a pre-target frame in an inter-frame chain — dropped silently
                }
            }
        }
        if eof { return }
    }
}
