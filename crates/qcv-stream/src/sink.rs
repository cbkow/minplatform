//! The stream's SESSION (QCView `src/output/stream_sink.cpp`): the renderer hands NV12 surfaces in; a paced clock at a fixed
//! rate encodes the newest (or repeats the last while the viewport is paused); the audio tap becomes AAC with silence where
//! the tap is idle; one thread muxes both to the wire and reconnects when it drops.
//!
//! Threads: the render side (`pool().acquire`, `submit` — from the GPU's work-done callback); the audio device's
//! (`push_audio`); PACED (the clock, the video session, repeat-last); AUDIO (1024-sample frames, silence when behind);
//! the encoder's callback (enqueues only); MUX (every connect and every write — it may block on the network).
//!
//! Differences from QCView, ruled (canon row 239): the mux thread owns the muxer and makes the first connect too (QCView's
//! paced thread opened it while the mux thread tested it); timestamps are exact at fractional rates; a wrong URL scheme is
//! refused at start; `stop` never waits on the network (a reaper joins the threads); the state is data, not an English
//! string, and never carries the URL (QCView's state line and log showed an RTMP key).
use crate::{aac::Aac, mux::{self, MuxConfig, Muxer}, pts90k, EncodedUnit, EncoderSpec, Profile};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering::*};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

#[cfg(target_os = "macos")]
pub use crate::vt::{Surface, SurfacePool};
#[cfg(target_os = "macos")]
use crate::vt::VtSession as Session;
#[cfg(not(target_os = "macos"))]
mod none {
    //! No hardware session yet on this OS (Windows: Media Foundation after the step 4d spike).
    pub struct Surface { pub y: wgpu::TextureView, pub cbcr: wgpu::TextureView }
    pub struct SurfacePool;
    impl SurfacePool { pub fn new(_: u32, _: u32) -> Result<SurfacePool, String> { Err("no hardware encoder on this platform yet".into()) } pub fn acquire(&self, _: &wgpu::Device) -> Option<Surface> { None } }
    pub struct Session;
    impl Session { pub const NAME: &'static str = ""; pub fn open(_: &crate::EncoderSpec, _: Box<dyn Fn(crate::EncodedUnit) + Send + Sync>) -> Result<Session, String> { Err("no hardware encoder on this platform yet".into()) } pub fn encode(&self, _: &Surface, _: i64, _: i64, _: bool) -> bool { false } }
}
#[cfg(not(target_os = "macos"))]
pub use none::{Surface, SurfacePool};
#[cfg(not(target_os = "macos"))]
use none::Session;

/// Does this OS have the hardware session (QCView `available`)?
pub fn available() -> bool { cfg!(target_os = "macos") }
pub fn encoder_name() -> &'static str { Session::NAME }

/// QCView `StreamConfig`. `fps_num` 0 = auto: the source's rate when the session starts (30 when it has none).
#[derive(Clone, Debug, PartialEq)]
pub struct StreamConfig { pub url: String, pub width: u32, pub height: u32, pub bitrate_kbps: u32, pub profile: Profile, pub srt_latency_ms: u32, pub stream_id: String, pub passphrase: String, pub fps_num: u32, pub fps_den: u32, /// 0 = video only
    pub audio_kbps: u32 }

/// QCView's key rule (output_bus.cpp 254-267): RTMP → the stream name appended to the URL (a trailing `/` dropped); SRT →
/// the stream id, unless the URL's query already names one. Returns (the URL to open, the SRT stream id).
pub fn apply_key(url: &str, key: &str) -> (String, String) {
    let (url, key) = (url.trim(), key.trim());
    match mux::scheme(url) {
        Some(mux::Scheme::Rtmp) if !key.is_empty() => (format!("{}/{key}", url.trim_end_matches('/')), String::new()),
        Some(mux::Scheme::Srt) if !key.is_empty() && !url.split_once('?').is_some_and(|(_, q)| q.split('&').any(|p| p.starts_with("streamid="))) => (url.to_string(), key.to_string()),
        _ => (url.to_string(), String::new()),
    }
}
/// A URL fit to show or log (ruling 5): the RTMP stream name and any `streamid` / `passphrase` value hidden.
pub fn redact(url: &str) -> String {
    let (base, query) = url.split_once('?').map_or((url, None), |(b, q)| (b, Some(q)));
    let mut out = match mux::scheme(base) {
        Some(mux::Scheme::Rtmp) => { let rest = base.split_once("://").map_or(base, |x| x.1); if rest.matches('/').count() >= 2 { let i = base.rfind('/').unwrap(); format!("{}/…", &base[..i]) } else { base.to_string() } }
        _ => base.to_string(),
    };
    if let Some(q) = query { out.push('?'); out.push_str(&q.split('&').map(|p| match p.split_once('=') { Some((k, _)) if k == "streamid" || k == "passphrase" => format!("{k}=…"), _ => p.to_string() }).collect::<Vec<_>>().join("&")); }
    out
}

/// What the session is doing (QCView's state strings, as data: the app words them).
#[derive(Clone, Debug, PartialEq)]
pub enum State {
    /// started; no picture has reached the session yet
    Waiting, OpeningEncoder,
    /// attempt `n` (1 = the first); `again` = after a drop
    Connecting { attempt: u32, again: bool },
    /// an attempt failed: the next one in `secs`
    Retry { error: String, secs: u32, again: bool },
    Streaming,
    /// the connection dropped: reconnect number `n`
    Reconnecting(u64),
    Failed(String), Stopped,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Status { pub state: State, pub running: bool, pub failed: bool, /// the session's fixed rate
    pub fps: f64, /// the elementary streams' measured rate
    pub kbps: f64, pub encoded: u64, pub repeated: u64, pub skipped: u64, pub dropped: u64, pub audio_silent: u64, pub mux_errors: u64, pub reconnects: u64 }

const QUEUE_CAP: usize = 96;            // video + audio units (QCView)
const AUDIO_RING: usize = 2 * 48000 * 2; // 2 s of interleaved stereo

struct Shared {
    cfg: StreamConfig, stop: Arc<AtomicBool>, done: AtomicBool, failed: AtomicBool,
    latest: Mutex<Option<Surface>>, latest_cv: Condvar,
    queue: Mutex<VecDeque<EncodedUnit>>, queue_cv: Condvar,
    audio: Mutex<VecDeque<f32>>, audio_extradata: Vec<u8>,
    state: Mutex<State>, source_fps: Mutex<(u32, u32)>, fps: Mutex<(u32, u32)>, clock: Mutex<Option<Instant>>, kbps: Mutex<f64>,
    encoder_open: AtomicBool, mux_ready: AtomicBool, force_key: AtomicBool,
    bytes: AtomicU64, encoded: AtomicU64, repeated: AtomicU64, skipped: AtomicU64, dropped: AtomicU64, audio_silent: AtomicU64, mux_errors: AtomicU64, reconnects: AtomicU64,
}
impl Shared {
    fn stopping(&self) -> bool { self.stop.load(Acquire) }
    fn set_state(&self, s: State) { let mut st = self.state.lock().unwrap(); if *st != s { log::info!("stream: {s:?}"); *st = s; } }
    fn fail(&self, e: String) { log::warn!("stream: failed: {e}"); self.failed.store(true, Release); self.set_state(State::Failed(e)); self.stop.store(true, Release); }
    /// From the encoder's callback and the audio thread: into the mux queue (nothing is kept while the wire is down).
    fn enqueue(&self, u: EncodedUnit) {
        if !self.mux_ready.load(Acquire) { return }
        let mut q = self.queue.lock().unwrap();
        q.push_back(u);
        if q.len() > QUEUE_CAP { q.pop_front(); self.dropped.fetch_add(1, Relaxed); }
        self.queue_cv.notify_one();
    }
    /// Sleep up to `d`, ending early on stop.
    fn sleep(&self, d: Duration) { let end = Instant::now() + d; while !self.stopping() { let now = Instant::now(); if now >= end { break } std::thread::sleep((end - now).min(Duration::from_millis(50))); } }
}

pub struct StreamSink { sh: Arc<Shared>, pool: Arc<SurfacePool>, threads: Vec<std::thread::JoinHandle<()>> }

impl StreamSink {
    /// Start a session (QCView `StreamSink::start` + the bus's checks; the strings are QCView's). `source_fps` = the rate
    /// "auto" takes.
    pub fn start(cfg: StreamConfig, source_fps: (u32, u32)) -> Result<StreamSink, String> {
        if cfg.url.trim().is_empty() { return Err("enter the server URL first".into()) }
        if mux::scheme(&cfg.url).is_none() { return Err(mux::scheme_error(&redact(&cfg.url))) }
        if !available() { return Err("no hardware encoder on this platform yet".into()) }
        let pool = Arc::new(SurfacePool::new(cfg.width, cfg.height)?);
        let aac = if cfg.audio_kbps > 0 { Some(Aac::open(cfg.audio_kbps)?) } else { None };
        let sh = Arc::new(Shared {
            audio_extradata: aac.as_ref().map(|a| a.extradata()).unwrap_or_default(), cfg, stop: Arc::new(AtomicBool::new(false)), done: AtomicBool::new(false), failed: AtomicBool::new(false),
            latest: Mutex::new(None), latest_cv: Condvar::new(), queue: Mutex::new(VecDeque::new()), queue_cv: Condvar::new(), audio: Mutex::new(VecDeque::new()),
            state: Mutex::new(State::Waiting), source_fps: Mutex::new(source_fps), fps: Mutex::new((0, 1)), clock: Mutex::new(None), kbps: Mutex::new(0.0),
            encoder_open: AtomicBool::new(false), mux_ready: AtomicBool::new(false), force_key: AtomicBool::new(false),
            bytes: AtomicU64::new(0), encoded: AtomicU64::new(0), repeated: AtomicU64::new(0), skipped: AtomicU64::new(0), dropped: AtomicU64::new(0), audio_silent: AtomicU64::new(0), mux_errors: AtomicU64::new(0), reconnects: AtomicU64::new(0),
        });
        log::info!("stream: start {} {}x{} {} kbps {:?} audio {} kbps", redact(&sh.cfg.url), sh.cfg.width, sh.cfg.height, sh.cfg.bitrate_kbps, sh.cfg.profile, sh.cfg.audio_kbps);
        let mut threads = vec![];
        let spawn = |name: &str, f: Box<dyn FnOnce() + Send>| std::thread::Builder::new().name(name.into()).spawn(f).map_err(|e| e.to_string());
        { let s = sh.clone(); threads.push(spawn("stream mux", Box::new(move || mux_thread(s)))?); }
        if let Some(aac) = aac { let s = sh.clone(); threads.push(spawn("stream audio", Box::new(move || audio_thread(s, aac)))?); }
        { let s = sh.clone(); threads.push(spawn("stream clock", Box::new(move || paced_thread(s)))?); }
        Ok(StreamSink { sh, pool, threads })
    }
    pub fn config(&self) -> &StreamConfig { &self.sh.cfg }
    /// The encoder's surfaces: `acquire` one, write it (the NV12 pass), `submit` it when the GPU is done.
    pub fn pool(&self) -> Arc<SurfacePool> { self.pool.clone() }
    /// A handle the GPU's work-done callback can submit through.
    pub fn submitter(&self) -> Submitter { Submitter(self.sh.clone()) }
    /// The source's rate, for a session that has not fixed its own yet.
    pub fn set_source_fps(&self, fps: (u32, u32)) { *self.sh.source_fps.lock().unwrap() = fps; }
    /// Interleaved stereo at 48 kHz from the tap; `muted` writes silence (the master mute or the feed mute).
    pub fn push_audio(&self, stereo: &[f32], muted: bool) {
        if self.sh.cfg.audio_kbps == 0 { return }
        let mut a = self.sh.audio.lock().unwrap();
        if muted { a.extend(std::iter::repeat_n(0.0, stereo.len())); } else { a.extend(stereo.iter().copied()); }
        let over = a.len().saturating_sub(AUDIO_RING);
        if over > 0 { a.drain(..over + over % 2); }
    }
    pub fn status(&self) -> Status {
        let s = &self.sh; let (n, d) = *s.fps.lock().unwrap();
        let (failed, done) = (s.failed.load(Acquire), s.done.load(Acquire));
        Status { state: s.state.lock().unwrap().clone(), running: !failed && !done && !s.stopping(), failed, fps: if n > 0 { n as f64 / d.max(1) as f64 } else { 0.0 }, kbps: *s.kbps.lock().unwrap(),
            encoded: s.encoded.load(Relaxed), repeated: s.repeated.load(Relaxed), skipped: s.skipped.load(Relaxed), dropped: s.dropped.load(Relaxed), audio_silent: s.audio_silent.load(Relaxed), mux_errors: s.mux_errors.load(Relaxed), reconnects: s.reconnects.load(Relaxed) }
    }
    /// Stop: returns at once — the flag ends every wait and every network call; a reaper joins the threads.
    pub fn stop(&mut self) {
        let sh = self.sh.clone();
        sh.stop.store(true, Release); sh.latest_cv.notify_all(); sh.queue_cv.notify_all();
        let (threads, pool) = (std::mem::take(&mut self.threads), self.pool.clone());
        if threads.is_empty() { return }   // already stopping
        let _ = std::thread::Builder::new().name("stream stop".into()).spawn(move || {
            for t in threads { let _ = t.join(); }
            *sh.latest.lock().unwrap() = None; sh.queue.lock().unwrap().clear();
            if !sh.failed.load(Acquire) { sh.set_state(State::Stopped); }
            sh.done.store(true, Release);
            drop(pool);
        });
    }
    /// Quitting: stop and wait (bounded) until the session's threads are gone — the process must not tear the network
    /// library down under a thread still inside it.
    pub fn stop_wait(&mut self, limit: Duration) {
        let sh = self.sh.clone(); self.stop();
        let end = Instant::now() + limit;
        while !sh.done.load(Acquire) && Instant::now() < end { std::thread::sleep(Duration::from_millis(5)); }
        if !sh.done.load(Acquire) { log::warn!("stream: the session did not end within {limit:?}"); }
    }
}
impl Drop for StreamSink { fn drop(&mut self) { self.stop(); } }

/// The render side's hand-over (QCView `submit`): the newest surface waits for the clock; one it replaces was never encoded.
#[derive(Clone)]
pub struct Submitter(Arc<Shared>);
impl Submitter {
    pub fn submit(&self, surface: Surface) {
        if self.0.stopping() { return }
        let mut l = self.0.latest.lock().unwrap();
        if l.replace(surface).is_some() { self.0.skipped.fetch_add(1, Relaxed); }
        self.0.latest_cv.notify_all();
    }
}

fn paced_thread(sh: Arc<Shared>) {
    // the first picture
    { let mut l = sh.latest.lock().unwrap(); while l.is_none() && !sh.stopping() { l = sh.latest_cv.wait_timeout(l, Duration::from_millis(50)).unwrap().0; } }
    if sh.stopping() { return }
    // the rate, fixed for the session: the choice, else the source's, else 30
    let (n, d) = if sh.cfg.fps_num > 0 { (sh.cfg.fps_num, sh.cfg.fps_den.max(1)) } else { let s = *sh.source_fps.lock().unwrap(); if s.0 > 0 && s.1 > 0 { s } else { (30, 1) } };
    *sh.fps.lock().unwrap() = (n, d);
    sh.set_state(State::OpeningEncoder);
    let spec = EncoderSpec { width: sh.cfg.width, height: sh.cfg.height, fps_num: n, fps_den: d, bitrate_kbps: sh.cfg.bitrate_kbps, gop_frames: (n as f64 / d as f64).round().max(1.0) as u32, profile: sh.cfg.profile };
    let session = { let s = sh.clone(); match Session::open(&spec, Box::new(move |u| { s.encoded.fetch_add(1, Relaxed); s.enqueue(u); })) { Ok(s) => s, Err(e) => { sh.fail(e); return } } };
    sh.encoder_open.store(true, Release);
    while !sh.mux_ready.load(Acquire) && !sh.stopping() { std::thread::sleep(Duration::from_millis(5)); }
    if sh.stopping() { return }
    // the clock starts once the wire is up; tick k is due at t0 + k periods
    let t0 = Instant::now(); *sh.clock.lock().unwrap() = Some(t0);
    let period = Duration::from_secs_f64(d as f64 / n as f64);
    let (mut tick, mut last, mut first) = (0u64, None::<Surface>, true);
    let (mut rate_t, mut rate_bytes) = (t0, 0u64);
    while !sh.stopping() {
        let due = t0 + period.mul_f64(tick as f64);
        while !sh.stopping() { let now = Instant::now(); if now >= due { break } std::thread::sleep((due - now).min(Duration::from_millis(20))); }
        if sh.stopping() { break }
        match sh.latest.lock().unwrap().take() { Some(s) => last = Some(s), None => if !first { sh.repeated.fetch_add(1, Relaxed); } }
        if let Some(s) = &last {
            let key = sh.force_key.swap(false, AcqRel) || first;
            session.encode(s, pts90k(tick, n, d), pts90k(tick + 1, n, d) - pts90k(tick, n, d), key);
            first = false;
        }
        tick += 1;
        // more than two periods late: skip ahead, never burst
        let now = Instant::now(); let next = t0 + period.mul_f64(tick as f64);
        if now > next + period * 2 { tick += ((now - next).as_secs_f64() / period.as_secs_f64()) as u64; }
        let dt = now.duration_since(rate_t).as_secs_f64();
        if dt >= 1.0 { let b = sh.bytes.load(Relaxed); *sh.kbps.lock().unwrap() = (b - rate_bytes) as f64 * 8.0 / 1000.0 / dt; rate_t = now; rate_bytes = b; }
    }
    drop(session);   // the frames in flight finish into the queue
}

fn audio_thread(sh: Arc<Shared>, mut aac: Aac) {
    let t0 = loop { if sh.stopping() { return } if let Some(t) = *sh.clock.lock().unwrap() { break t } std::thread::sleep(Duration::from_millis(5)); };
    sh.audio.lock().unwrap().clear();
    let (n, d) = *sh.fps.lock().unwrap();
    let offset = (48000.0 * d as f64 / n.max(1) as f64).round() as i64;   // one video period late (QCView: A/V within 6 ms)
    let (mut emitted, mut out) = (0i64, vec![]);
    let (mut l, mut r) = (vec![0f32; Aac::FRAME], vec![0f32; Aac::FRAME]);
    while !sh.stopping() {
        let got = {
            let mut a = sh.audio.lock().unwrap();
            let frames = a.len() / 2;
            if frames >= Aac::FRAME {
                if frames > 8 * Aac::FRAME { let cut = (frames - 7 * Aac::FRAME) * 2; a.drain(..cut); }   // the tap runs ahead of the clock: stay near it
                // A sample tapped NOW must be stamped now: what went out plus what waits may not run ahead of the clock by
                // more than two frames. It does after silence was filled in and the tap's audio then arrived late, in a burst
                // (playback and the stream starting together): the oldest of it goes, so the rest lands on time. Measured
                // without this: audio 67 ms late for the whole session (QCView has the same start).
                let frames = a.len() / 2;
                let ahead = emitted + frames as i64 - (t0.elapsed().as_secs_f64() * 48000.0) as i64;
                if ahead > 2 * Aac::FRAME as i64 { let cut = ((ahead as usize - Aac::FRAME).min(frames - Aac::FRAME)) * 2; a.drain(..cut); }
                for i in 0..Aac::FRAME { l[i] = a.pop_front().unwrap_or(0.0); r[i] = a.pop_front().unwrap_or(0.0); }
                true
            } else { false }
        };
        if !got {
            // the tap is idle: silence once the clock is 80 ms ahead of what went out
            let elapsed = (t0.elapsed().as_secs_f64() * 48000.0) as i64;
            if emitted + Aac::FRAME as i64 <= elapsed - 3840 { l.fill(0.0); r.fill(0.0); sh.audio_silent.fetch_add(1, Relaxed); } else { std::thread::sleep(Duration::from_millis(3)); continue }
        }
        aac.encode(&l, &r, emitted + offset, &mut out);
        emitted += Aac::FRAME as i64;
        for u in out.drain(..) { sh.enqueue(u); }
    }
}

/// QCView `connectWithBackoff`: 1, 2, 4, 8, then 10 s between attempts, until it connects or the session stops.
fn connect(sh: &Shared, m: &mut Muxer, cfg: &MuxConfig, again: bool) -> bool {
    let mut attempt = 1u32;
    loop {
        if sh.stopping() { return false }
        sh.set_state(State::Connecting { attempt, again });
        match m.open(cfg) {
            Ok(()) => return true,
            Err(e) => {
                if sh.stopping() { return false }
                let secs = [1, 2, 4, 8, 10][(attempt as usize - 1).min(4)];
                sh.set_state(State::Retry { error: e, secs, again });
                sh.sleep(Duration::from_secs(secs as u64));
            }
        }
        attempt += 1;
    }
}

fn mux_thread(sh: Arc<Shared>) {
    while !sh.encoder_open.load(Acquire) && !sh.stopping() { std::thread::sleep(Duration::from_millis(5)); }
    if sh.stopping() { return }
    let (n, d) = *sh.fps.lock().unwrap();
    let cfg = MuxConfig { url: sh.cfg.url.clone(), width: sh.cfg.width, height: sh.cfg.height, fps_num: n, fps_den: d, video_kbps: sh.cfg.bitrate_kbps, audio_kbps: sh.cfg.audio_kbps, audio_extradata: sh.audio_extradata.clone(), srt_latency_ms: sh.cfg.srt_latency_ms, stream_id: sh.cfg.stream_id.clone(), passphrase: sh.cfg.passphrase.clone() };
    let mut m = Muxer::new(sh.stop.clone());
    if !connect(&sh, &mut m, &cfg, false) { return }
    sh.mux_ready.store(true, Release); sh.set_state(State::Streaming);
    let mut wait_key = false;
    loop {
        let unit = { let mut q = sh.queue.lock().unwrap(); loop { if sh.stopping() { break None } if let Some(u) = q.pop_front() { break Some(u) } q = sh.queue_cv.wait_timeout(q, Duration::from_millis(50)).unwrap().0; } };
        let Some(u) = unit else { break };
        // after a reconnect the receiver resumes at a keyframe
        if wait_key { if u.audio || !u.keyframe { continue } wait_key = false; }
        match m.write(&u) {
            Ok(()) => sh.bytes.store(m.bytes, Relaxed),
            Err(e) => {
                if sh.stopping() { break }
                sh.mux_errors.fetch_add(1, Relaxed); log::warn!("stream: {e}");
                m.close(); sh.queue.lock().unwrap().clear();
                sh.set_state(State::Reconnecting(sh.reconnects.load(Relaxed) + 1));
                sh.sleep(Duration::from_secs(1));
                if !connect(&sh, &mut m, &cfg, true) { break }
                sh.reconnects.fetch_add(1, Relaxed);
                wait_key = true; sh.force_key.store(true, Release);
                sh.queue.lock().unwrap().clear();
                sh.set_state(State::Streaming);
            }
        }
    }
    m.close();
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn the_key_goes_where_qcview_put_it() {
        assert_eq!(apply_key("rtmp://host/app/", "abc"), ("rtmp://host/app/abc".into(), String::new()));
        assert_eq!(apply_key(" srt://host:9000 ", " abc "), ("srt://host:9000".into(), "abc".into()));
        assert_eq!(apply_key("srt://host:9000?streamid=x", "abc"), ("srt://host:9000?streamid=x".into(), String::new()), "the URL's own stream id wins");
        assert_eq!(apply_key("srt://host:9000", ""), ("srt://host:9000".into(), String::new()));
    }
    #[test]
    fn a_shown_url_hides_the_key() {
        assert_eq!(redact("rtmp://host/app/secret"), "rtmp://host/app/…");
        assert_eq!(redact("rtmp://host/app"), "rtmp://host/app");
        assert_eq!(redact("srt://host:9000?streamid=secret&latency=120&passphrase=pw"), "srt://host:9000?streamid=…&latency=120&passphrase=…");
        assert_eq!(redact("srt://host:9000"), "srt://host:9000");
    }
    #[test]
    fn a_bad_url_is_refused_at_start() {
        let cfg = |url: &str| StreamConfig { url: url.into(), width: 1920, height: 1080, bitrate_kbps: 6000, profile: Profile::Baseline, srt_latency_ms: 120, stream_id: String::new(), passphrase: String::new(), fps_num: 0, fps_den: 1, audio_kbps: 0 };
        assert_eq!(StreamSink::start(cfg(" "), (25, 1)).err().unwrap(), "enter the server URL first");
        assert_eq!(StreamSink::start(cfg("http://host"), (25, 1)).err().unwrap(), "the URL must start with srt://, rtmp:// or rtmps://: http://host");
    }
}
