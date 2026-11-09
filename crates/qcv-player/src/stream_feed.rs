//! `StreamFeed` — QCView's `LiveStreamDecoder` (`src/decode/live_stream_decoder.{h,cpp}`, canon row 227 step 2a): a live
//! network stream (SRT above all — the Blender bridge's replica calls a QCView listener; any `scheme://` FFmpeg opens) on
//! its own thread, presented latest-wins, never paced by PTS.
//!
//! The worker (QCView's loop): the first attempt runs CONNECTING; every later one RECONNECTING after a backoff of 250 ms
//! doubling to 3 s, reset after any session that delivered frames; EOF and errors both reconnect. Inside a session the
//! receive phases: DRAIN TO EDGE (video packets whose read returned in < 4 ms are backlog: dropped until the first read that
//! blocked), WAIT FOR A KEYFRAME, then STREAMING — three fast reads in a row mean a backlog is draining: non-reference frames
//! are skipped and a frame is shown at most every 250 ms (the rest counted as conflated); a blocking read ends it.
//!
//! Fixes over QCView (row 227 2a): the worker always publishes, so a reconnect during Review flips back to LIVE (QCView's
//! publish-off path returned before the Live transition); the last frame is kept so leaving Review shows the newest frame
//! even when the sender sends nothing new (QCView had no republish for network streams); the hardware-decode setting is
//! honoured on macOS too (QCView tried VideoToolbox regardless).
use crate::host_feed::{Facts, Status};
use min_ffmpeg::ffmpeg::{LiveRead, Recv};
use min_ffmpeg::{HwDevice, OpenOptions, Session};
use min_present::Frame;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicU8, Ordering::*};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

/// QCView `statusDetail` (context `qcv::LiveStreamDecoder`): an SRT LISTENER with no sender yet.
pub const WAITING_SENDER_PORT: &str = "Waiting for a sender on port %1";
pub const WAITING_SENDER: &str = "Waiting for a sender";
const BACKOFF_MIN: Duration = Duration::from_millis(250);
const BACKOFF_MAX: Duration = Duration::from_millis(3000);
const BURST_FAST_READS: u32 = 3;
const DRAIN_KEEPALIVE: Duration = Duration::from_millis(250);

#[derive(Default, Clone)]
struct Info { width: u32, height: u32, codec: String, pix_fmt: String, fps: Option<f64>, tags: (String, String, String) }

struct Shared {
    stop: Arc<AtomicBool>, status: AtomicU8, frames: AtomicU64, bytes: AtomicU64, drops: AtomicU32, conflated: AtomicU64,
    info: Mutex<Info>, slot: Mutex<Option<Frame>>, live_since: Mutex<Option<Instant>>, sleep: (Mutex<()>, Condvar),
}
impl Shared {
    fn set_status(&self, s: Status) { self.status.store(s as u8, Release); if s != Status::Live { *self.live_since.lock().unwrap() = None; } }
    fn status(&self) -> Status { match self.status.load(Acquire) { 1 => Status::Live, 2 => Status::Paused, 3 => Status::Reconnecting, _ => Status::Connecting } }
    fn sleep(&self, d: Duration) -> bool { let g = self.sleep.0.lock().unwrap(); let _ = self.sleep.1.wait_timeout_while(g, d, |_| !self.stop.load(Acquire)); !self.stop.load(Acquire) }
}

pub struct StreamFeed {
    sh: Arc<Shared>, thread: Option<std::thread::JoinHandle<()>>, last: Option<Arc<Frame>>, republish: bool,
    /// an SRT listener's port (`mode=listener`): the waiting detail names it
    listener: Option<Option<u16>>,
}

/// The URL with its passphrase hidden (QCView put it in logs, the Inspector and Recents — row 227 2a).
pub fn redact(url: &str) -> String {
    let Some(q) = url.find('?') else { return url.to_string() };
    let parts: Vec<String> = url[q + 1..].split('&').map(|kv| if kv.to_ascii_lowercase().starts_with("passphrase=") { "passphrase=•••".to_string() } else { kv.to_string() }).collect();
    format!("{}?{}", &url[..q], parts.join("&"))
}

impl StreamFeed {
    /// `hw` = the platform decoder device when the hardware-decode setting is on (None = software).
    pub fn open(url: &str, hw: Option<HwDevice>) -> StreamFeed {
        let stop = Arc::new(AtomicBool::new(false));
        let sh = Arc::new(Shared { stop: stop.clone(), status: AtomicU8::new(Status::Connecting as u8), frames: AtomicU64::new(0), bytes: AtomicU64::new(0), drops: AtomicU32::new(0), conflated: AtomicU64::new(0), info: Mutex::new(Info::default()), slot: Mutex::new(None), live_since: Mutex::new(None), sleep: (Mutex::new(()), Condvar::new()) });
        let listener = url.contains("mode=listener").then(|| url.split("://").nth(1).and_then(|r| r.split(['/', '?']).next()).and_then(|hp| hp.rsplit(':').next()).and_then(|p| p.parse().ok()));
        let (s2, u) = (sh.clone(), url.to_string());
        let thread = std::thread::Builder::new().name("qcv-live-stream".into()).spawn(move || worker(&s2, &u, hw)).ok();
        log::info!("LiveStreamDecoder: opening {}", redact(url));
        StreamFeed { sh, thread, last: None, republish: false, listener }
    }
    /// Per tick: the newest decoded frame, if one landed (or the last one again after `republish`).
    pub fn tick(&mut self) -> Option<Arc<Frame>> {
        if let Some(f) = self.sh.slot.lock().unwrap().take() { let a = Arc::new(f); self.last = Some(a.clone()); self.republish = false; return Some(a) }
        if std::mem::take(&mut self.republish) { return self.last.clone() }
        None
    }
    /// Leaving Review: the newest frame shows again even if the sender sends nothing new.
    pub fn republish(&mut self) { self.republish = true; }
    pub fn facts(&self) -> Facts {
        let st = self.sh.status(); let i = self.sh.info.lock().unwrap().clone();
        let detail = match (st, self.listener) { (Status::Connecting | Status::Reconnecting, Some(Some(p))) => Some((WAITING_SENDER_PORT, p.to_string())), (Status::Connecting | Status::Reconnecting, Some(None)) => Some((WAITING_SENDER, String::new())), _ => None };
        Facts { status: st, detail, width: i.width, height: i.height, frames: self.sh.frames.load(Relaxed), restarts: self.sh.drops.load(Relaxed), non_finite: false, has_frame: self.last.is_some(), stream: true, bytes: self.sh.bytes.load(Relaxed), codec: i.codec, pix_fmt: i.pix_fmt }
    }
    pub fn status(&self) -> Status { self.sh.status() }
    pub fn fps(&self) -> Option<f64> { self.sh.info.lock().unwrap().fps }
    /// The stream's colour tags (transfer, primaries, matrix by FFmpeg's names; "" unknown) — the item's live video tags.
    pub fn tags(&self) -> (String, String, String) { self.sh.info.lock().unwrap().tags.clone() }
    pub fn live_seconds(&self) -> f64 { self.sh.live_since.lock().unwrap().map(|t| t.elapsed().as_secs_f64()).unwrap_or(0.0) }
    pub fn conflated(&self) -> u64 { self.sh.conflated.load(Relaxed) }
}
impl Drop for StreamFeed {
    fn drop(&mut self) {
        self.sh.stop.store(true, Release);
        { let _g = self.sh.sleep.0.lock().unwrap(); self.sh.sleep.1.notify_all(); }
        if let Some(t) = self.thread.take() { let _ = t.join(); }
        log::info!("LiveStreamDecoder: closed ({} frames, {} drops, {} conflated)", self.sh.frames.load(Relaxed), self.sh.drops.load(Relaxed), self.sh.conflated.load(Relaxed));
    }
}

fn worker(sh: &Shared, url: &str, hw: Option<HwDevice>) {
    let mut backoff = BACKOFF_MIN; let mut first = true;
    while !sh.stop.load(Acquire) {
        if !first { sh.set_status(Status::Reconnecting); sh.drops.fetch_add(1, Relaxed); if !sh.sleep(backoff) { break } }
        first = false;
        let delivered = session(sh, url, hw.clone());
        backoff = if delivered { BACKOFF_MIN } else { (backoff * 2).min(BACKOFF_MAX) };
    }
}

#[derive(PartialEq)]
enum Phase { DrainToEdge, WaitKeyframe, Streaming }

/// One connection; true when it delivered frames.
fn session(sh: &Shared, url: &str, hw: Option<HwDevice>) -> bool {
    let opts = OpenOptions { threads: 0, hw, hw_intra: None, seq: None, fast: false, live: Some(sh.stop.clone()) };
    let mut s = match Session::open(url, &opts) { Ok(s) => s, Err(e) => { if !sh.stop.load(Acquire) { log::debug!("LiveStreamDecoder: open {}: {e}", redact(url)); } return false } };
    let i = &s.info;
    let fps = (i.frame_rate.1 > 0).then(|| i.frame_rate.0 as f64 / i.frame_rate.1 as f64).filter(|f| *f > 0.0 && *f < 1000.0);
    *sh.info.lock().unwrap() = Info { width: i.width, height: i.height, codec: i.codec_short.clone(), pix_fmt: i.pix_fmt.clone(), fps, tags: i.tags.clone() };
    log::info!("LiveStreamDecoder: {} {}×{} {} {} tags {:?}{}", redact(url), i.width, i.height, i.codec_short, i.pix_fmt, i.tags, if i.hw_accel.is_empty() { String::new() } else { format!(" ({})", i.hw_accel) });
    let index = s.frame_index();
    let (mut phase, mut fast, mut draining, mut last_pub, mut delivered) = (Phase::DrainToEdge, 0u32, false, None::<Instant>, false);
    while !sh.stop.load(Acquire) {
        match s.read_live() {
            LiveRead::Eof => { log::info!("LiveStreamDecoder: end of stream"); break }
            LiveRead::Error(e) => { if !sh.stop.load(Acquire) { log::info!("LiveStreamDecoder: read: {}", min_ffmpeg::ffmpeg::av_err(e)); } break }
            LiveRead::Other { bytes } => { sh.bytes.fetch_add(bytes as u64, Relaxed); }
            LiveRead::Video { key, bytes, blocked } => {
                sh.bytes.fetch_add(bytes as u64, Relaxed);
                if phase == Phase::DrainToEdge { if !blocked { s.drop_held(); continue } phase = Phase::WaitKeyframe; }
                if phase == Phase::WaitKeyframe { if !key { s.drop_held(); continue } phase = Phase::Streaming; }
                if blocked { fast = 0; if draining { draining = false; s.set_skip_nonref(false); } }
                else { fast += 1; if fast >= BURST_FAST_READS && !draining { draining = true; s.set_skip_nonref(true); } }
                s.send_held();
                loop {
                    match s.receive(&index) {
                        Recv::Frame(h) => {
                            let now = Instant::now();
                            if draining && last_pub.is_some_and(|t| now.duration_since(t) < DRAIN_KEEPALIVE) { sh.conflated.fetch_add(1, Relaxed); continue }
                            if !delivered { let pf = s.current_pix_fmt(); if !pf.is_empty() { sh.info.lock().unwrap().pix_fmt = pf; } }
                            last_pub = Some(now); delivered = true;
                            *sh.slot.lock().unwrap() = Some(h.frame);
                            sh.frames.fetch_add(1, Relaxed);
                            if sh.status() != Status::Live { sh.set_status(Status::Live); *sh.live_since.lock().unwrap() = Some(now); }
                        }
                        _ => break,
                    }
                }
            }
        }
    }
    delivered
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_passphrase_never_shows() {
        assert_eq!(super::redact("srt://0.0.0.0:9998?mode=listener&latency=120000&passphrase=abc123&pbkeylen=16"), "srt://0.0.0.0:9998?mode=listener&latency=120000&passphrase=•••&pbkeylen=16");
        assert_eq!(super::redact("rtmp://host/live"), "rtmp://host/live");
    }
}
