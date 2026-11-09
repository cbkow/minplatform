//! `LiveAudioSource` (QCView `audio/live_audio_source.{h,cpp}`, canon row 228): the QCAA segment beside a QCBridge host's
//! frame ring, read on its own thread every 2 ms (250 ms while the segment is absent or its producer dead), the host's
//! planes folded to stereo with the clip's routing mode, queued in an `AudioRing` the render callback drains like any
//! other source. No timeline: seek is a no-op, duration 0, and the player keeps its servo off it.
//!
//! The jitter buffer is PRIMED: silence until `buffer ms + max(0, A/V offset)` is queued (at a session start and after
//! any underrun), then it drains from there; when the queue creeps two packets past the target, one incoming packet is
//! dropped (clock drift is ppm). Playdown (whether the viewer hears it) is the player's: it drains and zeroes.
//!
//! Fix 7 (row 228; QCView only logged it): a host whose sequence is not at the device's 48 kHz is RESAMPLED here
//! (QCView's own Catmull-Rom `FractionalResampler` at src/48000), instead of played at the wrong speed and pitch.
use super::drain::AudioSource;
use super::resampler::FractionalResampler;
use super::ring::AudioRing;
use super::routing::{routing_matrix, RoutingMode};
use super::source::AudioSourceCtl;
use qcv_ring::audio::{self as qa, Next, State};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering::*};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

const WAIT_POLL_MS: u64 = 250;
const PACKET_POLL_MS: u64 = 2;
const IDLE_POLLS_BEFORE_METER_CLEAR: u32 = (100 / PACKET_POLL_MS) as u32;
/// The device's rate (AudioPlayer opens CoreAudio / WASAPI at 48 kHz).
pub const DEVICE_RATE: u32 = 48000;
/// About a second of stereo at 48 kHz — far more than any depth target.
const RING_SAMPLES: usize = DEVICE_RATE as usize * 2;

static BUFFER_MS: AtomicU32 = AtomicU32::new(60);
static SYNC_OFFSET_MS: AtomicU32 = AtomicU32::new(0);   // i32 bits
static SCRUB_MUTE: AtomicBool = AtomicBool::new(false);

/// QCView's process-wide knobs (WindowManager pushes them): `audio/liveBufferMs` [20, 200], `audio/liveSyncOffsetMs/<host>`
/// [−200, 200] (its positive part adds depth; the negative part holds video), scrub mute.
pub fn set_global_buffer_ms(ms: i32) { BUFFER_MS.store(ms.clamp(20, 200) as u32, Release); }
pub fn set_global_sync_offset_ms(ms: i32) { SYNC_OFFSET_MS.store(ms.clamp(-200, 200) as u32, Release); }
pub fn set_global_scrub_mute(m: bool) { SCRUB_MUTE.store(m, Release); }
/// The live A/V offset's negative part, the video hold in ms (the frame reader's).
pub fn video_hold_ms() -> i32 { (-global_sync_offset_ms()).max(0) }
pub fn global_sync_offset_ms() -> i32 { SYNC_OFFSET_MS.load(Acquire) as i32 }
fn depth_ms() -> i32 { BUFFER_MS.load(Acquire) as i32 + global_sync_offset_ms().max(0) }

struct Shared {
    ring: AudioRing,
    primed: AtomicBool, target_samples: AtomicUsize, reprime: AtomicBool,
    underruns: AtomicU64, primes: AtomicU64,
    stop: AtomicBool, running: AtomicBool, has_audio: AtomicBool,
    channels: AtomicU32, sample_rate: AtomicU32, routing: AtomicU32,
    peaks: [AtomicU32; 16],
    sleep: (Mutex<()>, Condvar),
}

pub struct LiveAudioSource { sh: Arc<Shared>, thread: Mutex<Option<std::thread::JoinHandle<()>>>, segment: String }

impl LiveAudioSource {
    /// Watches `segment` (the host's audio ring name, `qcv_ring::url::Host::audio_ring_name`).
    pub fn open(segment: &str, routing: RoutingMode) -> LiveAudioSource {
        let sh = Arc::new(Shared {
            ring: AudioRing::new(RING_SAMPLES), primed: AtomicBool::new(false), target_samples: AtomicUsize::new(60 * DEVICE_RATE as usize / 1000 * 2), reprime: AtomicBool::new(false),
            underruns: AtomicU64::new(0), primes: AtomicU64::new(0), stop: AtomicBool::new(false), running: AtomicBool::new(false), has_audio: AtomicBool::new(false),
            channels: AtomicU32::new(0), sample_rate: AtomicU32::new(0), routing: AtomicU32::new(routing as u32), peaks: Default::default(), sleep: (Mutex::new(()), Condvar::new()),
        });
        let (s2, name) = (sh.clone(), segment.to_string());
        let t = std::thread::Builder::new().name("qcv-live-audio".into()).spawn(move || reader_loop(&s2, &name)).expect("live audio thread");
        log::info!("LiveAudioSource: watching {segment}");
        LiveAudioSource { sh, thread: Mutex::new(Some(t)), segment: segment.to_string() }
    }
    pub fn sample_rate(&self) -> u32 { self.sh.sample_rate.load(Acquire) }
    pub fn has_audio(&self) -> bool { self.sh.has_audio.load(Acquire) }
    pub fn underruns(&self) -> u64 { self.sh.underruns.load(Relaxed) }
    /// The queued depth in ms (the Inspector's Audio card).
    pub fn queued_ms(&self) -> f64 { self.sh.ring.fresh_samples() as f64 / 2.0 / DEVICE_RATE as f64 * 1000.0 }
    pub fn close(&self) {
        let Some(t) = self.thread.lock().unwrap().take() else { return };
        self.sh.stop.store(true, Release);
        { let _g = self.sh.sleep.0.lock().unwrap(); self.sh.sleep.1.notify_all(); }
        let _ = t.join();
        self.sh.running.store(false, Release); self.sh.has_audio.store(false, Release);
        for p in &self.sh.peaks { p.store(0, Relaxed); }
        log::info!("LiveAudioSource: closed ({})", self.segment);
    }
}
impl Drop for LiveAudioSource { fn drop(&mut self) { self.close(); } }

impl AudioSource for LiveAudioSource {
    fn read(&self, out: &mut [f32]) -> usize {
        let sh = &self.sh;
        if sh.reprime.swap(false, AcqRel) { sh.primed.store(false, Relaxed); }
        if !sh.primed.load(Relaxed) {
            // silence until the jitter buffer holds the target; the drain fades back in when real frames resume
            if sh.ring.fresh_samples() < sh.target_samples.load(Acquire) { out.fill(0.0); return 0 }
            sh.primed.store(true, Relaxed); sh.primes.fetch_add(1, Relaxed);
        }
        let got = sh.ring.read(out);
        if got < out.len() {
            out[got..].fill(0.0);
            // an underrun: prime again rather than limp along one packet at a time
            sh.primed.store(false, Relaxed); sh.underruns.fetch_add(1, Relaxed);
        }
        got / 2
    }
    fn seek_pending(&self) -> bool { false }
    fn flush_generation(&self) -> u32 { self.sh.ring.stale_generation() }
    fn stale_frames(&self) -> usize { self.sh.ring.stale_samples() / 2 }
    fn discard_stale(&self) { self.sh.ring.discard_stale() }
}
impl AudioSourceCtl for LiveAudioSource {
    fn start(&self) { self.sh.running.store(true, Release); }
    fn stop(&self) { self.sh.running.store(false, Release); }
    fn seek(&self, _: f64) {}
    fn duration(&self) -> f64 { 0.0 }
    fn seconds_since_last_seek(&self) -> f64 { 1e9 }
    fn set_routing_mode(&self, m: RoutingMode) { self.sh.routing.store(m as u32, Release); }
    fn routing_mode(&self) -> RoutingMode { match self.sh.routing.load(Acquire) { 1 => RoutingMode::Downmix5_1, 2 => RoutingMode::Stereo7_8, _ => RoutingMode::Auto } }
    fn set_tempo(&self, _: f64) {}
    fn tempo(&self) -> f64 { 1.0 }
    fn source_channels(&self) -> usize { self.sh.channels.load(Acquire) as usize }
    fn layout_name(&self) -> String { match self.sh.channels.load(Acquire) { 1 => "mono", 2 => "stereo", 6 => "5.1", 8 => "7.1 / 5.1+stereo", _ => "" }.into() }
    fn peak_levels(&self) -> [f32; 16] { std::array::from_fn(|i| f32::from_bits(self.sh.peaks[i].load(Relaxed))) }
    fn is_live(&self) -> bool { true }
}

/// QCView `rebuildFold`: 2 × N row-major; `routing_matrix`'s None layouts (1, 2, 6 in Auto) spelled out — there is no
/// swresample here: mono to both, stereo through, 5.1 by BS.775 (LFE dropped).
fn fold(mode: RoutingMode, ch: usize) -> Vec<f32> {
    let mut m = vec![0.0f32; 2 * ch];
    if ch == 0 { return m }
    if let Some(r) = routing_matrix(mode, ch) { for (d, s) in m.iter_mut().zip(r) { *d = s as f32; } return m }
    let r = ch;
    match ch { 1 => { m[0] = 1.0; m[r] = 1.0; } 6 => { m[0] = 1.0; m[2] = 0.707; m[4] = 0.707; m[r + 1] = 1.0; m[r + 2] = 0.707; m[r + 5] = 0.707; } _ => { m[0] = 1.0; m[r + 1] = 1.0; } }
    m
}

/// Fix 7: a host rate other than the device's, resampled on the reader thread. Folded stereo goes in; whole output
/// blocks come out as the source frames they need are on hand (the 2-frame look-ahead stays pending).
struct RateConv { rs: FractionalResampler, src_rate: u32, pending: Vec<f32>, out: Vec<f32> }
impl RateConv {
    fn new() -> RateConv { RateConv { rs: FractionalResampler::new(), src_rate: DEVICE_RATE, pending: vec![], out: vec![] } }
    fn reset(&mut self, src_rate: u32) { self.rs.reset(); self.src_rate = src_rate; self.pending.clear(); }
    /// The stereo to queue for `stereo` (interleaved) at `src_rate`.
    fn convert<'a>(&'a mut self, stereo: &'a [f32], src_rate: u32) -> &'a [f32] {
        if src_rate != self.src_rate { self.reset(src_rate); }
        if src_rate == DEVICE_RATE || src_rate == 0 { return stereo }
        self.pending.extend_from_slice(stereo);
        let ratio = src_rate as f64 / DEVICE_RATE as f64;
        let have = self.pending.len() / 2;
        let mut dst = ((have as f64) / ratio) as usize;
        while dst > 0 && self.rs.source_frames_needed(dst, ratio) > have { dst -= 1; }
        if dst == 0 { return &[] }
        self.out.resize(dst * 2, 0.0);
        let used = self.rs.process(&self.pending, have, &mut self.out, dst, ratio);
        self.pending.drain(..used.min(have) * 2);
        &self.out
    }
}

fn sleep(sh: &Shared, ms: u64) -> bool {
    let g = sh.sleep.0.lock().unwrap();
    let _ = sh.sleep.1.wait_timeout_while(g, Duration::from_millis(ms), |_| !sh.stop.load(Acquire));
    !sh.stop.load(Acquire)
}

fn reader_loop(sh: &Shared, name: &str) {
    let mut ring: Option<qa::AudioRing> = None;
    let (mut last, mut dropped, mut dropped_depth, mut packets, mut generation, mut silenced) = (0u64, 0u64, 0u64, 0u64, 0u64, 0u64);
    let mut said_silenced = false; let mut underruns_said = 0u64; let mut last_underrun_line: Option<Instant> = None;
    let mut idle_polls = 0u32; let mut logged_pid = 0u32; let mut said_rate = 0u32;
    let mut planes: Vec<f32> = vec![]; let mut stereo: Vec<f32> = vec![];
    let mut fold_m: Vec<f32> = vec![]; let mut fold_key = (usize::MAX, u32::MAX);
    let mut conv = RateConv::new();
    while !sh.stop.load(Acquire) {
        if ring.is_none() {
            match qa::AudioRing::open(name) {
                Err(_) => { sh.has_audio.store(false, Release); if !sleep(sh, WAIT_POLL_MS) { break } continue }
                Ok(r) => {
                    let h = r.header();
                    planes = vec![0.0; h.max_channels as usize * h.slot_frames as usize];
                    stereo = vec![0.0; h.slot_frames as usize * 2];
                    r.skip_to_latest(&mut last);   // live: history is not for playing
                    generation = 0;
                    if h.producer_pid != logged_pid { logged_pid = h.producer_pid; log::info!("LiveAudioSource: segment {name} open (producer pid {}, {} slots x {} frames)", h.producer_pid, h.slot_count, h.slot_frames); }
                    ring = Some(r);
                }
            }
        }
        let r = ring.as_ref().unwrap();
        if !r.producer_alive() {
            ring = None;
            // the frame ring's housekeeping: a dead producer's segment stays in the kernel until someone unlinks it
            if qcv_ring::mapping::unlink(name) { log::info!("LiveAudioSource: removed the segment a dead producer left ({name})"); }
            sh.has_audio.store(false, Release);
            if !sleep(sh, WAIT_POLL_MS) { break }
            continue
        }
        if r.state() == State::Retired { ring = None; continue }
        // a new session (every play, scrub and speed change on the host): its format; whatever was queued is stale
        if let Some(s) = r.read_session(&mut generation) {
            sh.channels.store(s.channels, Release); sh.sample_rate.store(s.sample_rate, Release); sh.has_audio.store(s.channels > 0, Release);
            sh.ring.mark_stale(); sh.reprime.store(true, Release); said_silenced = false;
            conv.reset(s.sample_rate);
        }
        let slot_frames = r.slot_frames() as usize;
        let per_packet = slot_frames * 2;
        let depth = depth_ms();
        let target = depth as usize * DEVICE_RATE as usize / 1000 * 2;
        sh.target_samples.store(target, Release);
        let mut any = false;
        for _ in 0..64 {   // bounded: never starve the sleep
            let mut d = qcv_ring::layout::AudioPacketDesc::default();
            match r.next_packet(&mut last, &mut d, &mut planes, &mut dropped) {
                Next::None => break,
                Next::Resynced => { sh.ring.mark_stale(); sh.reprime.store(true, Release); continue }
                Next::Packet => {}
            }
            any = true; packets += 1;
            let ch = d.channels as usize; let n = d.frames as usize;
            for c in 0..16 {
                let peak = if c < ch { planes[c * slot_frames..c * slot_frames + n].iter().fold(0.0f32, |m, v| m.max(v.abs())) } else { 0.0 };
                sh.peaks[c].store(peak.to_bits(), Relaxed);
            }
            if d.channels != sh.channels.load(Relaxed) { sh.channels.store(d.channels, Release); sh.has_audio.store(ch > 0, Release); }
            if d.sample_rate != sh.sample_rate.load(Relaxed) { sh.sample_rate.store(d.sample_rate, Release); }
            // depth control: past the target by two packets, this one is not queued
            if sh.ring.fresh_samples() > target + 2 * per_packet { dropped_depth += 1; continue }
            let key = (ch, sh.routing.load(Acquire));
            if key != fold_key { fold_key = key; fold_m = fold(match key.1 { 1 => RoutingMode::Downmix5_1, 2 => RoutingMode::Stereo7_8, _ => RoutingMode::Auto }, ch); }
            let silent = d.flags & qa::PACKET_SCRUBBING != 0 && SCRUB_MUTE.load(Acquire);
            if silent || ch == 0 {
                stereo[..n * 2].fill(0.0);
                if silent { silenced += 1; if !said_silenced { said_silenced = true; log::info!("LiveAudioSource: scrub packets silenced (Mute scrub audio is on)"); } }
            } else {
                for i in 0..n {
                    let (mut l, mut rr) = (0.0f32, 0.0f32);
                    for c in 0..ch { let v = planes[c * slot_frames + i]; l += fold_m[c] * v; rr += fold_m[ch + c] * v; }
                    stereo[i * 2] = l; stereo[i * 2 + 1] = rr;
                }
            }
            if d.sample_rate != DEVICE_RATE && d.sample_rate != said_rate { said_rate = d.sample_rate; log::info!("LiveAudioSource: the host sends {} Hz — resampled to the device's {DEVICE_RATE} Hz", d.sample_rate); }
            let out = conv.convert(&stereo[..n * 2], d.sample_rate);
            sh.ring.write(out);
        }
        if packets == 1 && any { log::info!("LiveAudioSource: LIVE — {} ch {} Hz, depth target {depth} ms", sh.channels.load(Relaxed), sh.sample_rate.load(Relaxed)); }
        if any && packets % 2000 == 0 {
            log::info!("LiveAudioSource: {packets} packets, resync-dropped {dropped}, depth-dropped {dropped_depth}, scrub-silenced {silenced}, underruns {}, primed {} times, queued {:.0} ms", sh.underruns.load(Relaxed), sh.primes.load(Relaxed), sh.ring.fresh_samples() as f64 / 2.0 / DEVICE_RATE as f64 * 1000.0);
        }
        let u = sh.underruns.load(Relaxed);
        if u != underruns_said && last_underrun_line.is_none_or(|t| t.elapsed() > Duration::from_secs(5)) {
            log::info!("LiveAudioSource: {u} underrun(s) so far (+{}); re-priming to {depth} ms", u - underruns_said);
            last_underrun_line = Some(Instant::now()); underruns_said = u;
        }
        // meters: the host stopped — the peaks are per packet and would hold the last level for ever
        if any { idle_polls = 0; } else if idle_polls < IDLE_POLLS_BEFORE_METER_CLEAR { idle_polls += 1; if idle_polls == IDLE_POLLS_BEFORE_METER_CLEAR { for p in &sh.peaks { p.store(0, Relaxed); } } }
        if !sleep(sh, PACKET_POLL_MS) { break }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn folds_as_qcview() {
        assert_eq!(fold(RoutingMode::Auto, 1), vec![1.0, 1.0]);
        assert_eq!(fold(RoutingMode::Auto, 2), vec![1.0, 0.0, 0.0, 1.0]);
        let m = fold(RoutingMode::Auto, 6); assert_eq!((m[0], m[2], m[4], m[7], m[8], m[11], m[3]), (1.0, 0.707, 0.707, 1.0, 0.707, 0.707, 0.0));
    }
    #[test]
    fn fix_7_a_44k1_host_comes_out_at_48k() {
        let mut c = RateConv::new();
        let mut produced = 0usize;
        let block: Vec<f32> = vec![0.25; 1024 * 2];
        for _ in 0..441 { produced += c.convert(&block, 44100).len() / 2; }   // 441 × 1024 frames = 10.24 s at 44.1 kHz
        let want = (441.0 * 1024.0 * 48000.0 / 44100.0) as usize;
        assert!(produced.abs_diff(want) <= 4, "{produced} vs {want}");
        assert_eq!(c.convert(&block, 48000).len(), 2048, "48 kHz passes straight through");
    }
    #[test]
    fn a_host_segment_primes_then_plays_folded_and_resampled() {
        let name = format!("/qcv-live-audio-test-{}", std::process::id());
        let mut prod = qa::AudioRing::create(&name, 44100, 128, 1024, 2).unwrap();
        prod.begin_session(qcv_ring::layout::AudioSession { sample_rate: 44100, channels: 2, speed: 1.0, ..Default::default() });
        set_global_buffer_ms(60);
        let src = LiveAudioSource::open(&name, RoutingMode::Auto);
        std::thread::sleep(Duration::from_millis(30));   // the reader finds the segment, skips to latest
        let l = vec![0.5f32; 1024]; let r = vec![-0.25f32; 1024];
        let mut out = vec![0.0f32; 480 * 2];
        assert_eq!(src.read(&mut out), 0, "nothing queued: silence while priming");
        for _ in 0..10 { prod.push(&[&l, &r], 44100, 1024, 0, 0); }   // ~232 ms at 44.1 kHz (the depth control keeps ~target + 2 packets)
        let t = Instant::now(); while src.queued_ms() < 80.0 && t.elapsed() < Duration::from_secs(2) { std::thread::sleep(Duration::from_millis(5)); }
        assert!(src.queued_ms() > 60.0, "queued {} ms", src.queued_ms());
        assert_eq!(src.read(&mut out), 480, "primed: plays");
        assert!((out[100] - 0.5).abs() < 1e-3 && (out[101] + 0.25).abs() < 1e-3, "stereo through: {} {}", out[100], out[101]);
        assert_eq!(src.sample_rate(), 44100);
        src.close(); let _ = qcv_ring::mapping::unlink(&name);
    }
}
