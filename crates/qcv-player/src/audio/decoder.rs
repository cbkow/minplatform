//! `AudioDecoder` (QCView `audio/audio_decoder.cpp`): one audio stream decoded on its own thread into the stale-marking
//! ring (≈ 500 ms at 48 kHz stereo f32), back-pressure at 100 ms of fresh audio, a seek that marks the ring stale,
//! flushes the codec and trims the first post-seek frame to the target sample, EOF idling on the seek condvar, per-source-
//! channel peaks of the last decoded frame for the meters. The consumer side (`AudioSource`) is the device callback's.
use super::drain::AudioSource;
use super::ffmpeg_audio::{AudioCtx, OUT_CHANNELS, OUT_RATE};
use super::ring::AudioRing;
use super::routing::RoutingMode;
use super::source::AudioSourceCtl;
use super::tempo::{TempoControl, TempoStage};
use crate::Error;
use ffmpeg_sys_next as ff;
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU32, AtomicU64, Ordering::*};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

const RING_SAMPLES: usize = 192000 / 4;         // QCView kRingBytes
const BACK_PRESSURE_SAMPLES: usize = 38400 / 4; // QCView kBackPressureBytes (100 ms)

struct Shared {
    ring: AudioRing,
    running: AtomicBool,
    seek: Mutex<Option<f64>>, seek_cv: Condvar, seek_requested: AtomicBool,
    eof: AtomicBool,
    decode_pos: AtomicU64, read_pos: AtomicU64, last_seek: AtomicU64,   // f64 bits
    peaks: [AtomicU32; 16],
    routing: AtomicI32, routing_dirty: AtomicBool,
}
fn f2b(v: f64) -> u64 { v.to_bits() } fn b2f(b: u64) -> f64 { f64::from_bits(b) }

pub struct AudioDecoder {
    sh: Arc<Shared>,
    thread: Mutex<Option<JoinHandle<()>>>,
    ctx: Mutex<Option<(AudioCtx, TempoStage)>>,
    tempo: Arc<TempoControl>,
    pub duration: f64, pub src_channels: usize, pub src_rate: i32, pub layout_name: String,
    t0: Instant,
}

impl AudioDecoder {
    /// Open the file's best audio stream; `Ok(None)` when it has none.
    pub fn open(path: &str, routing: RoutingMode) -> Result<Option<AudioDecoder>, Error> {
        let Some(ctx) = AudioCtx::open(path, routing)? else { return Ok(None) };
        let sh = Arc::new(Shared { ring: AudioRing::new(RING_SAMPLES), running: AtomicBool::new(false), seek: Mutex::new(None), seek_cv: Condvar::new(), seek_requested: AtomicBool::new(false), eof: AtomicBool::new(false), decode_pos: AtomicU64::new(0), read_pos: AtomicU64::new(0), last_seek: AtomicU64::new(0), peaks: Default::default(), routing: AtomicI32::new(routing as i32), routing_dirty: AtomicBool::new(false) });
        let stage = TempoStage::new(); let tempo = stage.ctl.clone();
        Ok(Some(AudioDecoder { sh, thread: Mutex::new(None), duration: ctx.duration, src_channels: ctx.src_channels, src_rate: ctx.src_rate, layout_name: ctx.layout_name.clone(), ctx: Mutex::new(Some((ctx, stage))), tempo, t0: Instant::now() }))
    }
    /// Start the decode thread (QCView `start`).
    pub fn start(&self) {
        let Some((ctx, stage)) = self.ctx.lock().unwrap().take() else { return };
        self.sh.running.store(true, Release);
        let sh = self.sh.clone(); let t0 = self.t0;
        *self.thread.lock().unwrap() = Some(std::thread::Builder::new().name("qcv-audio-decode".into()).spawn(move || decode_thread(ctx, stage, sh, t0)).expect("audio decode thread"));
    }
    pub fn stop(&self) {
        self.sh.running.store(false, Release); self.sh.seek_cv.notify_all();
        if let Some(t) = self.thread.lock().unwrap().take() { let _ = t.join(); }
    }
    pub fn is_running(&self) -> bool { self.sh.running.load(Acquire) }
    pub fn eof(&self) -> bool { self.sh.eof.load(Relaxed) }
    pub fn read_position(&self) -> f64 { b2f(self.sh.read_pos.load(Relaxed)) }
}
impl Drop for AudioDecoder { fn drop(&mut self) { self.stop(); } }
impl AudioSourceCtl for AudioDecoder {
    fn start(&self) { AudioDecoder::start(self) }
    fn stop(&self) { AudioDecoder::stop(self) }
    fn seek(&self, sec: f64) { *self.sh.seek.lock().unwrap() = Some(sec); self.sh.seek_requested.store(true, Release); self.sh.seek_cv.notify_one(); }
    fn duration(&self) -> f64 { self.duration }
    fn seconds_since_last_seek(&self) -> f64 { let t = b2f(self.sh.last_seek.load(Relaxed)); if t <= 0.0 { f64::MAX } else { self.t0.elapsed().as_secs_f64() - t } }
    fn set_routing_mode(&self, mode: RoutingMode) { if self.sh.routing.swap(mode as i32, AcqRel) != mode as i32 { self.sh.routing_dirty.store(true, Release); } }
    fn routing_mode(&self) -> RoutingMode { match self.sh.routing.load(Relaxed) { 1 => RoutingMode::Downmix5_1, 2 => RoutingMode::Stereo7_8, _ => RoutingMode::Auto } }
    fn set_tempo(&self, tempo: f64) { self.tempo.set_tempo(tempo) }
    fn tempo(&self) -> f64 { self.tempo.tempo() }
    fn source_channels(&self) -> usize { self.src_channels }
    fn layout_name(&self) -> String { self.layout_name.clone() }
    fn peak_levels(&self) -> [f32; 16] { let mut o = [0.0; 16]; for (i, p) in self.sh.peaks.iter().enumerate() { o[i] = f32::from_bits(p.load(Relaxed)); } o }
}

impl AudioSource for AudioDecoder {
    fn read(&self, out: &mut [f32]) -> usize {
        let n = self.sh.ring.read(out);
        if n < out.len() { out[n..].fill(0.0); }
        let frames = n / OUT_CHANNELS;
        let p = b2f(self.sh.read_pos.load(Relaxed)) + frames as f64 / OUT_RATE as f64;
        self.sh.read_pos.store(f2b(p), Relaxed);
        frames
    }
    fn seek_pending(&self) -> bool { self.sh.seek_requested.load(Acquire) }
    fn flush_generation(&self) -> u32 { self.sh.ring.stale_generation() }
    fn stale_frames(&self) -> usize { self.sh.ring.stale_samples() / OUT_CHANNELS }
    fn discard_stale(&self) { self.sh.ring.discard_stale() }
}

/// The decode thread (QCView `decodeThreadFn` + `decodeNextPacket` + `flushAndSeek` + `writeAllToRing`).
fn decode_thread(mut ctx: AudioCtx, mut tempo: TempoStage, sh: Arc<Shared>, t0: Instant) {
    let mut trim_pending = false; let mut trim_target = 0.0f64;
    let mut peaks = [0.0f32; 16]; let mut tempo_buf = vec![0.0f32; 4096 * 2];
    while sh.running.load(Acquire) {
        if sh.routing_dirty.swap(false, AcqRel) {
            ctx.routing = match sh.routing.load(Relaxed) { 1 => RoutingMode::Downmix5_1, 2 => RoutingMode::Stereo7_8, _ => RoutingMode::Auto };
            if !ctx.setup_swr() { log::warn!("audio: resampler re-init failed after a routing change; silent until the next open"); }
        }
        {
            let mut g = sh.seek.lock().unwrap();
            if let Some(target) = g.take() {
                // flushAndSeek: mark the ring stale (the drain fades the tail), flush, seek backward, trim to the target
                sh.ring.mark_stale();
                ctx.seek(target); tempo.flush();
                sh.eof.store(false, Relaxed);
                sh.decode_pos.store(f2b(target), Relaxed); sh.read_pos.store(f2b(target), Relaxed);
                trim_pending = true; trim_target = target.max(0.0);
                sh.last_seek.store(f2b(t0.elapsed().as_secs_f64()), Relaxed);
                sh.seek_requested.store(false, Release);
            }
        }
        if sh.ring.fresh_samples() > BACK_PRESSURE_SAMPLES { std::thread::sleep(Duration::from_millis(5)); continue }
        if sh.eof.load(Relaxed) {
            let g = sh.seek.lock().unwrap();
            let _ = sh.seek_cv.wait_timeout_while(g, Duration::from_millis(50), |_| sh.running.load(Relaxed) && !sh.seek_requested.load(Relaxed)).unwrap();
            continue
        }
        if !decode_next_packet(&mut ctx, &mut tempo, &mut tempo_buf, &sh, &mut trim_pending, trim_target, &mut peaks) { std::thread::sleep(Duration::from_millis(10)); }
    }
}

fn write_all(sh: &Shared, floats: &[f32]) { let mut total = 0; while total < floats.len() && sh.running.load(Relaxed) && !sh.seek_requested.load(Relaxed) { total += sh.ring.write(&floats[total..]); if total < floats.len() { std::thread::sleep(Duration::from_millis(2)); } } }

fn decode_next_packet(ctx: &mut AudioCtx, tempo: &mut TempoStage, tempo_buf: &mut Vec<f32>, sh: &Shared, trim_pending: &mut bool, trim_target: f64, peaks: &mut [f32; 16]) -> bool {
    unsafe {
        if ctx.swr.is_null() { return false }
        let rc = ff::av_read_frame(ctx.fmt, ctx.pkt);
        if rc < 0 { if rc == ff::AVERROR_EOF { sh.eof.store(true, Relaxed); } return false }
        if (*ctx.pkt).stream_index != ctx.stream_idx { ff::av_packet_unref(ctx.pkt); return true }
        let rc = ff::avcodec_send_packet(ctx.cctx, ctx.pkt);
        ff::av_packet_unref(ctx.pkt);
        if rc < 0 { return false }
        loop {
            let rc = ff::avcodec_receive_frame(ctx.cctx, ctx.frame);
            if rc == ff::AVERROR(libc::EAGAIN) || rc == ff::AVERROR_EOF { break }
            if rc < 0 { return false }
            ctx.frame_peaks(peaks);
            for (i, p) in peaks.iter().enumerate() { sh.peaks[i].store(p.to_bits(), Relaxed); }
            let mut converted = ctx.convert();
            let mut offset_frames = 0usize;
            if converted > 0 && *trim_pending {
                match ctx.frame_start_sec() {
                    None => *trim_pending = false,
                    Some(start) => {
                        let dur = ctx.frame_samples() as f64 / ctx.src_rate as f64;
                        if start + dur <= trim_target { converted = 0; }   // wholly pre-target
                        else {
                            if start < trim_target { let skip = (((trim_target - start) * OUT_RATE as f64).round() as i32).clamp(0, converted); offset_frames = skip as usize; converted -= skip; }
                            *trim_pending = false;
                        }
                    }
                }
            }
            if converted > 0 {
                let samples = converted as usize * OUT_CHANNELS;
                let bytes = &ctx.swr_buf[offset_frames * OUT_CHANNELS * 4..(offset_frames * OUT_CHANNELS + samples) * 4];
                let floats: &[f32] = std::slice::from_raw_parts(bytes.as_ptr() as *const f32, samples);
                if !tempo.bypassed() {
                    // the review-speed path: stretch through WSOLA, drain whatever it has ready (early receives return 0 while it fills its window)
                    tempo.put(floats, converted as usize);
                    loop { if !sh.running.load(Relaxed) || sh.seek_requested.load(Relaxed) { break } let got = tempo.receive(tempo_buf, 4096); if got == 0 { break } write_all(sh, &tempo_buf[..got * 2]); }
                } else {
                    write_all(sh, floats);   // the 1× path — byte-identical to the pre-tempo code
                }
                let p = b2f(sh.decode_pos.load(Relaxed)) + converted as f64 / OUT_RATE as f64;
                sh.decode_pos.store(f2b(p), Relaxed);
            }
            ff::av_frame_unref(ctx.frame);
        }
        true
    }
}
