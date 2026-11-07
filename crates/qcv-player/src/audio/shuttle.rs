//! `ShuttleAudioEngine` (QCView `audio/shuttle_audio_engine.cpp`), verbatim: grain scheduler + varispeed player for the
//! FF / RW hold gesture. A grain thread follows the integrated position, decoding 80-ms grains FORWARD from a
//! synchronous `AudioChunkReader`, varispeed-resampling them by the signed speed (pitch follows speed below 1×, natural
//! pitch at ≥ 1× — the cursor snap-chases the faster transport, FCP-skim style, never a squeal) into a small ring the
//! device callback drains. Reverse grains are reversed one by one with 5-ms edge fades; the cursor free-runs and snaps
//! only when off by more than max(0.25 s, 0.25 × speed); a stationary target (drag-scrub holds) goes silent after 120 ms.
use super::chunk_reader::AudioChunkReader;
use super::resampler::FractionalResampler;
use super::ring::AudioRing;
use super::routing::RoutingMode;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU64, Ordering::*};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const OUT_RATE: f64 = 48000.0;
const GRAIN_OUT_FRAMES: usize = 3840;      // 80 ms per grain
const EDGE_FADE_FRAMES: usize = 240;       // 5 ms grain-edge fade
const RING_SAMPLES: usize = 65536 / 4;     // ~170 ms
const MAX_BUFFERED_SAMPLES: usize = 15360 / 4;
const FLUSH_FADE_FRAMES: usize = 128;
const MIN_SPEED: f64 = 0.05;
const PITCH_CAP: f64 = 1.0;                // natural pitch is the ceiling
const HOLD_SPEED_THRESHOLD: f64 = 0.05;
const HOLD_TIMEOUT_MS: i64 = 120;
const TARGET_MOVE_EPSILON: f64 = 0.004;

struct Shared {
    running: AtomicBool, active: AtomicBool, in_gap: AtomicBool, global_mute: AtomicBool,
    target: AtomicU64, speed: AtomicU64,            // f64 bits
    last_move_ms: AtomicI64, flush_ring: AtomicBool,
    req: Mutex<(String, RoutingMode)>, req_gen: AtomicU64,
    ring: AudioRing,
}
fn f2b(v: f64) -> u64 { v.to_bits() } fn b2f(b: u64) -> f64 { f64::from_bits(b) }

pub struct ShuttleAudioEngine { sh: Arc<Shared>, thread: Mutex<Option<std::thread::JoinHandle<()>>>, t0: Instant, post_flush_fade: Mutex<usize> }

impl Default for ShuttleAudioEngine { fn default() -> Self { Self::new() } }
impl ShuttleAudioEngine {
    pub fn new() -> ShuttleAudioEngine {
        ShuttleAudioEngine { sh: Arc::new(Shared { running: AtomicBool::new(false), active: AtomicBool::new(false), in_gap: AtomicBool::new(false), global_mute: AtomicBool::new(false), target: AtomicU64::new(0), speed: AtomicU64::new(f2b(1.0)), last_move_ms: AtomicI64::new(0), flush_ring: AtomicBool::new(false), req: Mutex::new((String::new(), RoutingMode::Auto)), req_gen: AtomicU64::new(0), ring: AudioRing::new(RING_SAMPLES) }), thread: Mutex::new(None), t0: Instant::now(), post_flush_fade: Mutex::new(0) }
    }
    fn now_ms(&self) -> i64 { self.t0.elapsed().as_millis() as i64 }
    pub fn active(&self) -> bool { self.sh.active.load(Relaxed) }
    pub fn set_global_mute(&self, m: bool) { self.sh.global_mute.store(m, Relaxed); }
    pub fn set_in_gap(&self, g: bool) { self.sh.in_gap.store(g, Relaxed); }
    /// Start (or re-target) the grain thread following `path` at `start_src_sec` and `signed_speed`.
    pub fn begin(&self, path: &str, start_src_sec: f64, signed_speed: f64, routing: RoutingMode) {
        *self.sh.req.lock().unwrap() = (path.to_string(), routing);
        self.sh.req_gen.fetch_add(1, Release);
        self.sh.target.store(f2b(start_src_sec), Relaxed);
        let prev = b2f(self.sh.speed.swap(f2b(signed_speed), Relaxed));
        if prev * signed_speed < 0.0 { self.sh.flush_ring.store(true, Release); }
        self.sh.in_gap.store(false, Relaxed);
        self.sh.last_move_ms.store(self.now_ms(), Relaxed);
        if !self.sh.running.load(Acquire) {
            self.sh.ring.clear(); self.sh.flush_ring.store(false, Relaxed); *self.post_flush_fade.lock().unwrap() = 0;
            self.sh.running.store(true, Release); self.sh.active.store(true, Release);
            let sh = self.sh.clone(); let t0 = self.t0;
            *self.thread.lock().unwrap() = Some(std::thread::Builder::new().name("qcv-audio-shuttle".into()).spawn(move || grain_thread(sh, t0)).expect("shuttle thread"));
        }
    }
    /// Per-gesture-tick update; a path change hands the new file to the grain thread (the reader LRU keeps the old one warm).
    pub fn update_target(&self, path: &str, src_sec: f64, signed_speed: f64) {
        if !self.sh.running.load(Acquire) { return }
        let prev_t = b2f(self.sh.target.swap(f2b(src_sec), Relaxed));
        if (src_sec - prev_t).abs() > TARGET_MOVE_EPSILON { self.sh.last_move_ms.store(self.now_ms(), Relaxed); }
        let prev = b2f(self.sh.speed.swap(f2b(signed_speed), Relaxed));
        if prev * signed_speed < 0.0 { self.sh.flush_ring.store(true, Release); }
        let changed = { let mut r = self.sh.req.lock().unwrap(); if r.0 != path { r.0 = path.to_string(); true } else { false } };
        if changed { self.sh.req_gen.fetch_add(1, Release); }
    }
    /// Stop + join the grain thread, clear the ring.
    pub fn end(&self) {
        if self.sh.running.swap(false, AcqRel) { if let Some(t) = self.thread.lock().unwrap().take() { let _ = t.join(); } }
        self.sh.active.store(false, Release); self.sh.ring.clear();
    }
    /// Device callback: drain up to `out.len() / 2` frames; the rest is silence. Returns the frames delivered.
    pub fn read(&self, out: &mut [f32]) -> usize {
        if !self.sh.active.load(Relaxed) { out.fill(0.0); return 0 }
        let mut fade_left = self.post_flush_fade.lock().unwrap();
        if self.sh.flush_ring.swap(false, AcqRel) { self.sh.ring.discard_all(); *fade_left = FLUSH_FADE_FRAMES; }
        let n = self.sh.ring.read(out); let frames = n / 2;
        if *fade_left > 0 && frames > 0 {
            let k = (*fade_left).min(frames);
            for i in 0..k { let g = (FLUSH_FADE_FRAMES - *fade_left + i) as f32 / FLUSH_FADE_FRAMES as f32; out[i * 2] *= g; out[i * 2 + 1] *= g; }
            *fade_left -= k;
        }
        if n < out.len() { out[n..].fill(0.0); }
        frames
    }
}
impl Drop for ShuttleAudioEngine { fn drop(&mut self) { self.end(); } }

struct ReaderSlot { path: String, routing: RoutingMode, reader: Option<AudioChunkReader>, last_use: u64 }

fn grain_thread(sh: Arc<Shared>, t0: Instant) {
    let mut local_gen = 0u64;
    let mut slots: [ReaderSlot; 2] = [ReaderSlot { path: String::new(), routing: RoutingMode::Auto, reader: None, last_use: 0 }, ReaderSlot { path: String::new(), routing: RoutingMode::Auto, reader: None, last_use: 0 }];
    let mut use_counter = 0u64;
    let mut reader_idx: Option<usize> = None;
    let mut resampler = FractionalResampler::new();
    let mut src_buf: Vec<f32> = vec![]; let mut out_buf = vec![0.0f32; GRAIN_OUT_FRAMES * 2];
    let mut cursor = b2f(sh.target.load(Relaxed));
    let mut need_fade_in = true; let mut prev_dir = 0i32;
    let now_ms = |t0: Instant| t0.elapsed().as_millis() as i64;
    while sh.running.load(Relaxed) {
        let g = sh.req_gen.load(Acquire);
        if g != local_gen {
            local_gen = g;
            let (path, routing) = sh.req.lock().unwrap().clone();
            reader_idx = None;
            if !path.is_empty() {
                let mut found = slots.iter().position(|s| s.path == path && s.routing == routing && s.reader.is_some());
                if found.is_none() {
                    let i = if slots[0].last_use <= slots[1].last_use { 0 } else { 1 };
                    slots[i].reader = None; slots[i].path.clear();
                    match AudioChunkReader::open(&path, routing) { Ok(Some(r)) => { slots[i].reader = Some(r); slots[i].path = path.clone(); slots[i].routing = routing; found = Some(i); } Ok(None) => {} Err(e) => log::warn!("shuttle audio: {e}") }
                }
                if let Some(i) = found { use_counter += 1; slots[i].last_use = use_counter; reader_idx = Some(i); }
            }
            cursor = b2f(sh.target.load(Relaxed)); resampler.reset(); need_fade_in = true;
        }
        let speed = b2f(sh.speed.load(Relaxed));
        let held = speed.abs() < HOLD_SPEED_THRESHOLD || now_ms(t0) - sh.last_move_ms.load(Relaxed) > HOLD_TIMEOUT_MS;
        let Some(ri) = reader_idx else { cursor = b2f(sh.target.load(Relaxed)); need_fade_in = true; std::thread::sleep(Duration::from_millis(5)); continue };
        if sh.global_mute.load(Relaxed) || sh.in_gap.load(Relaxed) || held { cursor = b2f(sh.target.load(Relaxed)); need_fade_in = true; std::thread::sleep(Duration::from_millis(5)); continue }
        if sh.ring.available_read() > MAX_BUFFERED_SAMPLES { std::thread::sleep(Duration::from_millis(2)); continue }
        let aspeed = speed.abs().clamp(MIN_SPEED, PITCH_CAP);
        let dir: i32 = if speed < 0.0 { -1 } else { 1 };
        if prev_dir != 0 && dir != prev_dir { cursor = b2f(sh.target.load(Relaxed)); resampler.reset(); need_fade_in = true; }
        prev_dir = dir;
        let target = b2f(sh.target.load(Relaxed));
        let snap = 0.25f64.max(0.25 * aspeed);
        if (cursor - target).abs() > snap { cursor = target; resampler.reset(); need_fade_in = true; }
        let src_frames = resampler.source_frames_needed(GRAIN_OUT_FRAMES, aspeed);
        let span = src_frames as f64 / OUT_RATE;
        let grain_start = (if dir > 0 { cursor } else { cursor - span }).max(0.0);
        if src_buf.len() < src_frames * 2 { src_buf.resize(src_frames * 2, 0.0); }
        let reader = slots[ri].reader.as_mut().unwrap();
        let got = reader.read_at(grain_start, &mut src_buf, src_frames);
        cursor += dir as f64 * span;
        let dur = reader.duration();
        if cursor < 0.0 { cursor = 0.0; } if dur > 0.0 && cursor > dur { cursor = dur; }
        if got == 0 { need_fade_in = true; std::thread::sleep(Duration::from_millis(5)); continue }
        if got < src_frames { src_buf[got * 2..src_frames * 2].fill(0.0); }
        let reversed = dir < 0;
        if reversed { let (mut i, mut j) = (0usize, src_frames - 1); while i < j { src_buf.swap(i * 2, j * 2); src_buf.swap(i * 2 + 1, j * 2 + 1); i += 1; j -= 1; } resampler.reset(); }
        resampler.process(&src_buf, src_frames, &mut out_buf, GRAIN_OUT_FRAMES, aspeed);
        let fade_n = EDGE_FADE_FRAMES.min(GRAIN_OUT_FRAMES / 2);
        if need_fade_in || reversed { for i in 0..fade_n { let g = i as f32 / fade_n as f32; out_buf[i * 2] *= g; out_buf[i * 2 + 1] *= g; } need_fade_in = false; }
        if reversed { for i in 0..fade_n { let g = i as f32 / fade_n as f32; let k = GRAIN_OUT_FRAMES - 1 - i; out_buf[k * 2] *= g; out_buf[k * 2 + 1] *= g; } }
        let want = GRAIN_OUT_FRAMES * 2; let mut total = 0;
        while total < want && sh.running.load(Relaxed) {
            let live = b2f(sh.speed.load(Relaxed)); if (if live < 0.0 { -1 } else { 1 }) != dir { break }
            total += sh.ring.write(&out_buf[total..]);
            if total < want { std::thread::sleep(Duration::from_millis(2)); }
        }
    }
}
