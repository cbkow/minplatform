//! `FrameIndex` — ported from `QCView-Player/src/decode/frame_index.{h,cpp}` (Phase 1.8.3 formula + 1.8.7 async scan).
//! First-class PTS ↔ frame-number mapping for a single video stream; queryable from any thread.
//!
//! Tier 1 — formula: from `time_base` + `frame_rate` at open. Exact for constant-rate intra codecs; approximate for
//! inter or variable-rate footage. Always available.
//! Tier 2 — async scan: `start_scan` walks every video packet on a background thread (its own `AVFormatContext`) and
//! builds an exact (pts, is_keyframe) table sorted by display order. While building, lookups use the formula.
use ffmpeg_sys_next as ff;
use std::{ffi::CString, sync::{atomic::{AtomicBool, AtomicI32, AtomicU8, Ordering}, Arc, Mutex}, thread};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScanState { NotStarted = 0, Building = 1, Ready = 2, Failed = 3 }

#[derive(Clone, Copy)] struct Entry { pts: i64, keyframe: bool }
struct Shared { entries: Mutex<Vec<Entry>>, state: AtomicU8, frames_built: AtomicI32, stop: AtomicBool }

pub struct FrameIndex {
    tb: ff::AVRational, fr: ff::AVRational, total: i32, intra_only: bool,
    shared: Arc<Shared>, thread: Mutex<Option<thread::JoinHandle<()>>>,
}

impl FrameIndex {
    pub fn new(tb_num: i32, tb_den: i32, fr_num: i32, fr_den: i32, total_frames: i32, intra_only: bool) -> FrameIndex {
        FrameIndex { tb: ff::AVRational { num: tb_num, den: tb_den }, fr: ff::AVRational { num: fr_num, den: fr_den }, total: total_frames, intra_only,
            shared: Arc::new(Shared { entries: Mutex::new(vec![]), state: AtomicU8::new(0), frames_built: AtomicI32::new(0), stop: AtomicBool::new(false) }), thread: Mutex::new(None) }
    }
    pub fn is_valid(&self) -> bool { self.tb.den > 0 && self.fr.num > 0 && self.fr.den > 0 }
    fn ready(&self) -> bool { self.shared.state.load(Ordering::Acquire) == ScanState::Ready as u8 }
    /// When the scan is Ready the exact table answers; otherwise the formula. Callers don't branch.
    pub fn pts_for_frame(&self, frame_no: i32) -> i64 {
        if !self.is_valid() { return 0 }
        if self.ready() { let e = self.shared.entries.lock().unwrap(); if frame_no >= 0 && (frame_no as usize) < e.len() { return e[frame_no as usize].pts } }
        unsafe { ff::av_rescale_q(frame_no as i64, ff::AVRational { num: self.fr.den, den: self.fr.num }, self.tb) }
    }
    pub fn frame_for_pts(&self, pts: i64) -> i32 {
        if !self.is_valid() { return 0 }
        if self.ready() {
            let e = self.shared.entries.lock().unwrap();
            // nearest pts at-or-before
            let i = e.partition_point(|x| x.pts < pts);
            if i == e.len() { return e.len() as i32 - 1 }
            if e[i].pts == pts { return i as i32 }
            return if i == 0 { 0 } else { i as i32 - 1 };
        }
        unsafe { ff::av_rescale_q(pts, self.tb, ff::AVRational { num: self.fr.den, den: self.fr.num }) as i32 }
    }
    pub fn frame_for_time(&self, seconds: f64) -> i32 { if !self.is_valid() || seconds <= 0.0 { 0 } else { (seconds * self.fps() + 0.5) as i32 } }
    pub fn total_frames(&self) -> i32 { self.total }
    pub fn is_intra_only(&self) -> bool { self.intra_only }
    pub fn time_base(&self) -> (i32, i32) { (self.tb.num, self.tb.den) }
    pub fn frame_rate(&self) -> (i32, i32) { (self.fr.num, self.fr.den) }
    pub fn fps(&self) -> f64 { if self.fr.den == 0 { 0.0 } else { self.fr.num as f64 / self.fr.den as f64 } }
    /// Tier 2: spawn the packet scan. Idempotent — a running scan is cancelled first.
    pub fn start_scan(&self, path: &str, video_stream_idx: i32) {
        self.cancel_scan();
        self.shared.stop.store(false, Ordering::Release); self.shared.frames_built.store(0, Ordering::Release);
        let sh = self.shared.clone(); let path = path.to_string(); let total = self.total;
        let h = thread::Builder::new().name("frame-index-scan".into()).spawn(move || run_scan(&sh, &path, video_stream_idx, total)).ok();
        *self.thread.lock().unwrap() = h;
    }
    pub fn cancel_scan(&self) {
        self.shared.stop.store(true, Ordering::Release);
        if let Some(h) = self.thread.lock().unwrap().take() { h.join().ok(); }
        self.shared.stop.store(false, Ordering::Release);
    }
    pub fn scan_state(&self) -> ScanState { match self.shared.state.load(Ordering::Acquire) { 1 => ScanState::Building, 2 => ScanState::Ready, 3 => ScanState::Failed, _ => ScanState::NotStarted } }
    pub fn scan_frames_built(&self) -> i32 { self.shared.frames_built.load(Ordering::Acquire) }
    pub fn has_exact_mapping(&self) -> bool { self.ready() }
    /// The PTS of the keyframe at-or-before `target_frame` (direct keyframe seeks); -1 without a complete scan.
    pub fn keyframe_pts_before(&self, target_frame: i32) -> i64 {
        if !self.is_valid() || !self.ready() { return -1 }
        let e = self.shared.entries.lock().unwrap();
        if e.is_empty() { return -1 }
        let idx = (target_frame.max(0) as usize).min(e.len() - 1);
        (0..=idx).rev().find(|&i| e[i].keyframe).map(|i| e[i].pts).unwrap_or(-1)
    }
    /// Scan-table keyframe count (tests).
    pub fn keyframe_count(&self) -> usize { self.shared.entries.lock().unwrap().iter().filter(|e| e.keyframe).count() }
}
impl Drop for FrameIndex { fn drop(&mut self) { self.cancel_scan(); } }

fn run_scan(sh: &Shared, path: &str, stream_idx: i32, total: i32) {
    sh.state.store(ScanState::Building as u8, Ordering::Release);
    let fail = || sh.state.store(ScanState::Failed as u8, Ordering::Release);
    let Ok(cpath) = CString::new(path) else { fail(); return };
    unsafe {
        let mut fmt: *mut ff::AVFormatContext = std::ptr::null_mut();
        if ff::avformat_open_input(&mut fmt, cpath.as_ptr(), std::ptr::null_mut(), std::ptr::null_mut()) < 0 { fail(); return }
        if ff::avformat_find_stream_info(fmt, std::ptr::null_mut()) < 0 { ff::avformat_close_input(&mut fmt); fail(); return }
        let pkt = ff::av_packet_alloc();
        let mut built: Vec<Entry> = Vec::with_capacity(total.max(0) as usize);
        while !sh.stop.load(Ordering::Acquire) {
            let rc = ff::av_read_frame(fmt, pkt);
            if rc == ff::AVERROR_EOF { break }
            if rc < 0 { log::warn!("FrameIndex scan: av_read_frame error {rc}"); break }
            if (*pkt).stream_index == stream_idx {
                let pts = if (*pkt).pts != ff::AV_NOPTS_VALUE { (*pkt).pts } else { (*pkt).dts };
                if pts != ff::AV_NOPTS_VALUE { built.push(Entry { pts, keyframe: ((*pkt).flags & ff::AV_PKT_FLAG_KEY) != 0 }); sh.frames_built.store(built.len() as i32, Ordering::Release); }
            }
            ff::av_packet_unref(pkt);
        }
        let mut p = pkt; ff::av_packet_free(&mut p);
        ff::avformat_close_input(&mut fmt);
        if sh.stop.load(Ordering::Acquire) { fail(); return }
        built.sort_by_key(|e| e.pts);   // display order: packet order is decode order (B-frame reorder)
        *sh.entries.lock().unwrap() = built;
    }
    sh.state.store(ScanState::Ready as u8, Ordering::Release);
}
