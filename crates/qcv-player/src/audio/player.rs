//! `AudioPlayer` (QCView `audio/audio_player.cpp`): one decoder + one device; the render callback drains the ring
//! through the drain stage at the servo's ratio; `update()` measures drift between the master clock and the playout
//! position reconstructed from REAL frames consumed minus the device latency and trims the ratio ±0.2 % (opening to
//! ±1.5 %) — hard re-seeks only as escape tiers (a soft one when drift leaves the 100-ms band for 4 ticks past a 1-s
//! cooldown, an immediate one above 1 s); the post-seek hold freezes audio until the picture lands and re-aligns by
//! dropping ring frames (≤ 400 ms forward) or re-seeking; pause fades out and stops the device 30 ms later.
//! Tempo = the decoders' SoundTouch stage; the player scales its consumption-based position by the tempo (QCView).
use super::decoder::AudioDecoder;
use super::multi_decoder::{audio_stream_count, MultiStreamAudioDecoder};
use super::source::AudioSourceCtl;
use super::device::{default_device, AudioDevice, NullDevice, RenderFn};
use super::drain::{AudioDrainStage, Request};
use super::routing::RoutingMode;
use super::servo::AudioSyncServo;
use super::shuttle::ShuttleAudioEngine;
use crate::transport::AudioHooks;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering::*};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const SERVO_BAND_SECONDS: f64 = 0.100;
const SEEK_COOLDOWN_SECONDS: f64 = 1.0;
const DISCONTINUITY_SECONDS: f64 = 1.0;
const OUT_OF_BAND_TICKS_TO_SEEK: i32 = 4;
const SKIP_MAX_SECONDS: f64 = 0.400;
const DEADBAND_SECONDS: f64 = 0.004;
const DEFERRED_STOP_MS: u64 = 30;
/// QCView stops the output unit 30 ms after a pause and starts it on play. On this Mac `AudioOutputUnitStart` / `Stop`
/// block the calling (UI) thread for ~120 ms each (the user's attended walks showed a 128-ms present stall and a burst
/// of dropped frames at every pause / play, 2026-11-07). The unit therefore stays running once started and renders
/// silence while paused (the drain's hold path); the deferred stop remains behind this switch.
const STOP_DEVICE_WHEN_IDLE: bool = false;

/// What the device callback reads (lock-free) and the UI thread writes.
struct CbShared {
    playing: AtomicBool, volume: AtomicU32, muted: AtomicBool, /// dual view's side gain (row 211: a side's mute, or its gap) — under the master volume / mute
    side_gain: AtomicU32,
    servo_ratio: AtomicU32, consumed: AtomicU64, req: Request,
    source: Mutex<Option<Arc<dyn AudioSourceCtl>>>,
    drain: Mutex<(AudioDrainStage, Vec<f32>)>,
    shuttle: ShuttleAudioEngine,
    /// a live source (row 228): it plays as it arrives; `playdown` = QCView's Listen (`audio/livePlaydown`, default off) —
    /// off, the drain still runs (meters, taps) and the output is zeroed
    live: AtomicBool, playdown: AtomicBool, /// the outputs' tap (row 227 step 3): the mix before mute / volume / Listen, ~1 s of stereo
    tap_on: AtomicBool, tap: super::ring::AudioRing,
    callbacks: AtomicU64, contended: AtomicU64,
    /// Callbacks whose output block had signal (peak > −60 dBFS) — the bench's proof that audio actually flowed.
    audible: AtomicU64,
}

#[derive(Default, Debug, Clone)]
pub struct AudioStats { pub drift_ms: Vec<f32>, pub reseeks: u32, pub holds: u32, pub skips: u32, pub underruns: u64, pub fades: u64, pub starved: u64, pub callbacks: u64, pub contended: u64, pub audible: u64 }

pub struct AudioPlayer {
    device: Box<dyn AudioDevice>,
    cb: Arc<CbShared>,
    decoder: Option<Arc<dyn AudioSourceCtl>>,
    routing: RoutingMode,
    anchor_src_sec: f64,
    servo: AudioSyncServo,
    out_of_band_ticks: i32,
    pause_gen: u32, stop_due: Option<(u32, Instant)>,
    last_servo_update: Option<Instant>,
    sync_offset_ms: i32,
    scrub_muted: bool,
    tempo: f64,
    servo_log: u32,
    stats: AudioStats,
    trace: u32, released_at: Option<Instant>,
}

fn soft_limit(x: f32) -> f32 {
    const T: f32 = 0.8;
    if x > T { let e = x - T; T + (1.0 - T) * (e / (1.0 + e)) } else if x < -T { let e = -x - T; -T - (1.0 - T) * (e / (1.0 + e)) } else { x }
}

impl AudioPlayer {
    /// The platform's default output at 48 kHz / 10-ms buffers (QCView `initialize`).
    pub fn new() -> Result<AudioPlayer, crate::Error> { Self::with_device(|r| default_device(r, 48000, 10)) }
    /// A silent, wall-paced device (headless benches).
    pub fn headless() -> Result<AudioPlayer, crate::Error> { Self::with_device(|r| Ok(Box::new(NullDevice::new(r, 48000, 10)) as Box<dyn AudioDevice>)) }
    fn with_device(make: impl FnOnce(RenderFn) -> Result<Box<dyn AudioDevice>, crate::Error>) -> Result<AudioPlayer, crate::Error> {
        let cb = Arc::new(CbShared { playing: AtomicBool::new(false), volume: AtomicU32::new(1.0f32.to_bits()), muted: AtomicBool::new(false), side_gain: AtomicU32::new(1.0f32.to_bits()), servo_ratio: AtomicU32::new(1.0f32.to_bits()), consumed: AtomicU64::new(0), req: Request::default(), source: Mutex::new(None), drain: Mutex::new((AudioDrainStage::new(), vec![])), shuttle: ShuttleAudioEngine::new(), live: AtomicBool::new(false), playdown: AtomicBool::new(false), tap_on: AtomicBool::new(false), tap: super::ring::AudioRing::new(48000 * 2), callbacks: AtomicU64::new(0), contended: AtomicU64::new(0), audible: AtomicU64::new(0) });
        let c = cb.clone();
        let render: RenderFn = Box::new(move |out: &mut [f32]| Self::process_audio(&c, out));
        let device = make(render)?;
        // the scratch sized once from the device's real buffer count (+16; WASAPI can ask for the whole buffer in one callback)
        let frames = device.buffer_frame_count() as usize + 16;
        cb.drain.lock().unwrap().1 = vec![0.0; frames * 2 * 2];
        Ok(AudioPlayer { device, cb, decoder: None, routing: RoutingMode::Auto, scrub_muted: false, anchor_src_sec: 0.0, servo: AudioSyncServo::new(), out_of_band_ticks: 0, pause_gen: 0, stop_due: None, last_servo_update: None, sync_offset_ms: 0, tempo: 1.0, servo_log: 0, stats: AudioStats::default(), trace: if std::env::var_os("QCV_AUDIO_TRACE").is_some() { 60 } else { 0 }, released_at: None })
    }
    /// The render callback (QCView `processAudio`): the drain at the servo ratio, then mute / volume / the soft limiter.
    fn process_audio(c: &CbShared, out: &mut [f32]) {
        c.callbacks.fetch_add(1, Relaxed);
        let frames = out.len() / 2;
        if c.shuttle.active() {
            // the grain ring INSTEAD of the main decoder (whose decode thread idles on back-pressure, ready for the commit seek)
            c.shuttle.read(out);
            if c.tap_on.load(Relaxed) { c.tap.write(out); }
            if c.muted.load(Relaxed) { out.fill(0.0); return }
            let vol = f32::from_bits(c.volume.load(Relaxed)) * f32::from_bits(c.side_gain.load(Relaxed)); for v in out.iter_mut() { *v *= vol; }
            return
        }
        let Ok(src) = c.source.try_lock() else { c.contended.fetch_add(1, Relaxed); out.fill(0.0); return };
        let Some(dec) = src.as_ref() else { out.fill(0.0); return };
        let Ok(mut dr) = c.drain.try_lock() else { c.contended.fetch_add(1, Relaxed); out.fill(0.0); return };
        let (drain, scratch) = &mut *dr;
        let ratio = f32::from_bits(c.servo_ratio.load(Relaxed)) as f64;
        let extra_hold = !c.playing.load(Acquire);
        drain.process(&**dec, out, frames, ratio, scratch, &c.consumed, &c.req, extra_hold);
        if c.tap_on.load(Relaxed) { c.tap.write(out); }   // QCView: the outputs hear the mix before Listen, mute and volume
        if c.muted.load(Relaxed) || (c.live.load(Relaxed) && !c.playdown.load(Relaxed)) { out.fill(0.0); return }
        let vol = f32::from_bits(c.volume.load(Relaxed)) * f32::from_bits(c.side_gain.load(Relaxed));
        let mut peak = 0.0f32;
        for v in out.iter_mut() { *v = soft_limit(*v * vol); peak = peak.max(v.abs()); }
        if peak > 0.001 { c.audible.fetch_add(1, Relaxed); }
    }
    fn consumed_sec(&self) -> f64 { self.cb.consumed.load(Relaxed) as f64 / self.device.sample_rate() as f64 * self.tempo }
    pub fn set_routing_mode(&mut self, mode: RoutingMode) { self.routing = mode; if let Some(d) = &self.decoder { d.set_routing_mode(mode); } }
    pub fn peak_levels(&self) -> [f32; 16] { self.decoder.as_ref().map(|d| d.peak_levels()).unwrap_or([0.0; 16]) }
    pub fn duration(&self) -> f64 { self.decoder.as_ref().map(|d| d.duration()).unwrap_or(0.0) }
    pub fn set_volume(&mut self, v: f32) { self.cb.volume.store(v.clamp(0.0, 1.0).to_bits(), Relaxed); }
    pub fn set_muted(&mut self, m: bool) { self.cb.muted.store(m, Relaxed); }
    pub fn set_sync_offset_ms(&mut self, ms: i32) { self.sync_offset_ms = ms; }
    /// QCView `AudioPlayer::open` on a live URL: the QCAA segment's reader; the device starts before any audio has
    /// arrived (the jitter buffer primes), no servo, no seek.
    pub fn open_live(&mut self, segment: &str) -> Arc<super::live::LiveAudioSource> {
        AudioHooks::close(self);
        let live = Arc::new(super::live::LiveAudioSource::open(segment, self.routing));
        let d: Arc<dyn AudioSourceCtl> = live.clone();
        d.start();
        *self.cb.source.lock().unwrap() = Some(d.clone()); self.decoder = Some(d);
        self.cb.live.store(true, Release); self.cb.playing.store(true, Release); self.cb.req.restart.store(true, Release);
        self.device.start();
        log::info!("AudioPlayer: live source playing, device buffer latency {:.1} ms", self.device.buffer_latency_seconds() * 1000.0);
        live
    }
    /// The outputs' tap on / off (off empties it).
    pub fn set_output_tap(&mut self, on: bool) { if !on { self.cb.tap.discard_all(); } self.cb.tap_on.store(on, Release); }
    /// What the tap gathered since the last call, appended to `out`.
    pub fn drain_tap(&mut self, out: &mut Vec<f32>) { let n = self.cb.tap.available_read(); if n == 0 { return } let o = out.len(); out.resize(o + n, 0.0); let got = self.cb.tap.read(&mut out[o..]); out.truncate(o + got); }
    /// QCView's Listen toggle for a live source (`setLivePlaydown`).
    pub fn set_live_playdown(&mut self, on: bool) { self.cb.playdown.store(on, Release); }
    pub fn live_playdown(&self) -> bool { self.cb.playdown.load(Acquire) }
    pub fn source_is_live(&self) -> bool { self.cb.live.load(Acquire) }
    pub fn device_latency_ms(&self) -> f64 { self.device.buffer_latency_seconds() * 1000.0 }
    pub fn playout_position(&self) -> f64 { self.anchor_src_sec + self.consumed_sec() - self.device.buffer_latency_seconds() * f32::from_bits(self.cb.servo_ratio.load(Relaxed)) as f64 * self.tempo }
}

impl AudioHooks for AudioPlayer {
    fn open(&mut self, path: &str) -> bool {
        self.close();
        // QCView's dispatch: ≥ 2 audio streams → the multi-stream decoder (one codec each + the merge / pan graph)
        let n = audio_stream_count(path);
        log::info!("audio: {n} audio stream(s) in the file → {}", if n >= 2 { "the multi-stream decoder" } else { "the single-stream decoder" });
        let opened: Result<Option<Arc<dyn AudioSourceCtl>>, crate::Error> = if n >= 2 {
            MultiStreamAudioDecoder::open(path, self.routing).map(|o| o.map(|d| Arc::new(d) as Arc<dyn AudioSourceCtl>))
        } else {
            AudioDecoder::open(path, self.routing).map(|o| o.map(|d| Arc::new(d) as Arc<dyn AudioSourceCtl>))
        };
        match opened {
            Ok(Some(d)) => { d.start(); d.set_tempo(self.tempo); *self.cb.source.lock().unwrap() = Some(d.clone()); self.decoder = Some(d); if !STOP_DEVICE_WHEN_IDLE { self.device.start(); } true }
            Ok(None) => false,
            Err(e) => { log::warn!("audio open: {e}"); false }
        }
    }
    fn close(&mut self) {
        self.pause();
        *self.cb.source.lock().unwrap() = None;
        if let Some(d) = self.decoder.take() { d.stop(); }
        self.cb.live.store(false, Release);
        self.cb.consumed.store(0, Relaxed);
    }
    fn has_audio(&self) -> bool { self.decoder.is_some() }
    fn play(&mut self) {
        if self.decoder.is_none() { return }
        if self.cb.playing.swap(true, AcqRel) { return }
        self.pause_gen = self.pause_gen.wrapping_add(1); self.stop_due = None;   // cancels a deferred stop
        self.cb.req.restart.store(true, Release);
        self.device.start();
    }
    fn pause(&mut self) {
        if !self.cb.playing.swap(false, AcqRel) { return }
        self.cb.req.hold.store(false, Release); self.cb.req.skip_frames.store(0, Release);
        self.pause_gen = self.pause_gen.wrapping_add(1);
        if STOP_DEVICE_WHEN_IDLE { self.stop_due = Some((self.pause_gen, Instant::now() + Duration::from_millis(DEFERRED_STOP_MS))); }
    }
    fn tick(&mut self, now: Instant) {
        if let Some((g, due)) = self.stop_due { if now >= due { self.stop_due = None; if g == self.pause_gen && !self.cb.playing.load(Acquire) { self.device.stop(); } } }
    }
    fn seek(&mut self, seconds: f64) {
        let Some(d) = &self.decoder else { return };
        let off = self.sync_offset_ms as f64 / 1000.0;
        d.seek(seconds - off);
        self.anchor_src_sec = seconds - off;
        self.cb.consumed.store(0, Relaxed);
        self.cb.req.skip_frames.store(0, Release);
        self.servo.reset(); self.cb.servo_ratio.store(1.0f32.to_bits(), Relaxed);
        self.last_servo_update = None; self.out_of_band_ticks = 0;
    }
    fn set_hold(&mut self, on: bool) { self.cb.req.hold.store(on, Release); if on { self.last_servo_update = None; self.stats.holds += 1; } }
    fn release_hold(&mut self, master_seconds: f64) {
        if !self.cb.req.hold.load(Acquire) { return }
        let Some(_) = &self.decoder else { self.cb.req.hold.store(false, Release); return };
        let off = self.sync_offset_ms as f64 / 1000.0;
        // Divergence from QCView's `releaseHold` (reason): it compares the master to anchor + consumed WITHOUT the
        // device latency, so audio resumes one playout latency behind the picture and the servo grinds that out at
        // ±0.2 % — a +20 ms step on CoreAudio (10-ms buffer + unit latency) that sat at the quiet band's edge in the
        // attended walks. Skipping the latency as well lands the first audible sample on the picture.
        let latency = self.device.buffer_latency_seconds() * self.tempo;
        let delta = (master_seconds - off) - (self.anchor_src_sec + self.consumed_sec()) + latency;
        if delta > DEADBAND_SECONDS && delta <= SKIP_MAX_SECONDS {
            self.cb.req.skip_frames.store((delta / self.tempo * self.device.sample_rate() as f64).round() as u32, Release);
            self.stats.skips += 1;
            log::info!("AudioPlayer: hold released, skipping {:+.0} ms to the landed frame (device latency {:.1} ms, master {:.3}, anchor {:.3}, consumed {:.3})", delta * 1000.0, latency * 1000.0, master_seconds, self.anchor_src_sec, self.consumed_sec());
        } else if delta < -DEADBAND_SECONDS || delta > SKIP_MAX_SECONDS {
            log::info!("AudioPlayer: hold released, re-seeking ({:+.0} ms)", delta * 1000.0);
            self.seek(master_seconds); self.stats.reseeks += 1;
        } else { log::debug!("AudioPlayer: hold released in place ({:+.1} ms)", delta * 1000.0); }
        self.cb.req.hold.store(false, Release); self.last_servo_update = None; self.released_at = Some(Instant::now());
    }
    fn update(&mut self, video_position_seconds: f64) {
        let Some(d) = &self.decoder else { return };
        if d.is_live() { return }   // a live source plays what arrives; there is no position to servo
        if !self.cb.playing.load(Acquire) || self.cb.req.hold.load(Acquire) { return }
        if d.seek_pending() { self.last_servo_update = None; return }
        let off = self.sync_offset_ms as f64 / 1000.0;
        let target = video_position_seconds - off;
        let dur = d.duration();
        if dur > 0.0 && target > dur - 0.050 { self.cb.servo_ratio.store(1.0f32.to_bits(), Relaxed); self.last_servo_update = None; return }
        let audio_src_pos = self.playout_position();
        let drift = target - audio_src_pos;
        self.stats.drift_ms.push((drift * 1000.0) as f32);
        if self.trace > 0 { self.trace -= 1; log::info!("servo: master {:.4} playout {:.4} drift {:+.1} ms  anchor {:.4} consumed {:.4} latency {:.4} ratio {:.5}  since release {:.4} s  callbacks {}", target, audio_src_pos, drift * 1000.0, self.anchor_src_sec, self.consumed_sec(), self.device.buffer_latency_seconds(), f32::from_bits(self.cb.servo_ratio.load(Relaxed)), self.released_at.map(|t| t.elapsed().as_secs_f64()).unwrap_or(-1.0), self.cb.callbacks.load(Relaxed)); }
        if drift.abs() > SERVO_BAND_SECONDS {
            let discontinuity = drift.abs() > DISCONTINUITY_SECONDS;
            self.out_of_band_ticks += 1;
            let sustained = self.out_of_band_ticks >= OUT_OF_BAND_TICKS_TO_SEEK;
            if discontinuity || (sustained && d.seconds_since_last_seek() > SEEK_COOLDOWN_SECONDS) {
                log::info!("AudioPlayer: drift {:+.0} ms — re-seeking audio to {:.2}s{}", drift * 1000.0, video_position_seconds, if discontinuity { " (discontinuity, cooldown bypassed)" } else { "" });
                self.seek(video_position_seconds); self.stats.reseeks += 1;
            }
            return
        }
        self.out_of_band_ticks = 0;
        let now = Instant::now();
        let dt = self.last_servo_update.map(|t| now.duration_since(t).as_secs_f64()).unwrap_or(0.0);
        self.last_servo_update = Some(now);
        let ratio = self.servo.update(drift, dt);
        self.cb.servo_ratio.store((ratio as f32).to_bits(), Relaxed);
        self.servo_log += 1; if self.servo_log >= 300 { self.servo_log = 0; log::debug!("AudioPlayer: servo drift {:+.1} ms  ratio {:.5}", drift * 1000.0, ratio); }
    }
    fn set_tempo(&mut self, speed: f64) {
        // QCView `setPlaybackTempo`: the decoders' WSOLA stage; the owner re-anchors with a seek right after (Transport does)
        let t = speed.clamp(0.25, 4.0);
        self.tempo = t;
        if let Some(d) = &self.decoder { d.set_tempo(t); }
    }
    fn set_muted(&mut self, m: bool) { self.cb.muted.store(m, Relaxed); }
    fn set_output_tap(&mut self, on: bool) { AudioPlayer::set_output_tap(self, on) }
    fn drain_tap(&mut self, out: &mut Vec<f32>) { AudioPlayer::drain_tap(self, out) }
    fn set_routing_mode(&mut self, m: RoutingMode) { AudioPlayer::set_routing_mode(self, m); }
    fn routing_mode(&self) -> RoutingMode { self.routing }
    fn peak_levels(&self) -> [f32; 16] { AudioPlayer::peak_levels(self) }
    fn set_sync_offset_ms(&mut self, ms: i32) { self.sync_offset_ms = ms; }
    fn set_scrub_muted(&mut self, m: bool) { self.scrub_muted = m; if m { self.cb.shuttle.end(); } }
    fn begin_shuttle(&mut self, path: &str, source_sec: f64, signed_speed: f64) {
        if self.scrub_muted { return }   // QCView `audio/scrubMuted`: silence while shuttling / scrubbing
        self.pause_gen = self.pause_gen.wrapping_add(1); self.stop_due = None;   // the device stays up for the grains
        self.device.start();
        self.cb.shuttle.begin(path, source_sec, signed_speed, self.routing);
    }
    fn shuttle_target(&mut self, path: &str, source_sec: f64, signed_speed: f64) { if self.scrub_muted { return } self.cb.shuttle.update_target(path, source_sec, signed_speed); }
    fn end_shuttle(&mut self) {
        self.cb.shuttle.end();
        if STOP_DEVICE_WHEN_IDLE && !self.cb.playing.load(Acquire) { self.pause_gen = self.pause_gen.wrapping_add(1); self.stop_due = Some((self.pause_gen, Instant::now() + Duration::from_millis(DEFERRED_STOP_MS))); }
    }
    fn shuttle_active(&self) -> bool { self.cb.shuttle.active() }
    fn set_volume(&mut self, v: f32) { self.cb.volume.store(v.clamp(0.0, 1.0).to_bits(), Relaxed); }
    fn set_side_gain(&mut self, g: f32) { self.cb.side_gain.store(g.clamp(0.0, 1.0).to_bits(), Relaxed); }
    fn toggle_mute(&mut self) { let m = !self.cb.muted.load(Relaxed); self.cb.muted.store(m, Relaxed); }
    fn volume_step(&mut self, delta: f32) { let v = (f32::from_bits(self.cb.volume.load(Relaxed)) + delta).clamp(0.0, 1.0); self.cb.volume.store(v.to_bits(), Relaxed); }
    fn is_muted(&self) -> bool { self.cb.muted.load(Relaxed) }
    fn device_latency_ms(&self) -> f64 { self.device.buffer_latency_seconds() * 1000.0 }
    fn source_channels(&self) -> usize { self.decoder.as_ref().map(|d| d.source_channels()).unwrap_or(0) }
    fn layout_name(&self) -> String { self.decoder.as_ref().map(|d| d.layout_name()).unwrap_or_default() }
    fn volume(&self) -> f32 { f32::from_bits(self.cb.volume.load(Relaxed)) }
    fn stats(&mut self) -> Option<AudioStats> {
        let (u, f, st) = { let dr = self.cb.drain.lock().unwrap(); (dr.0.underruns, dr.0.fades, dr.0.starved) };
        self.stats.underruns = u; self.stats.fades = f; self.stats.starved = st;
        self.stats.callbacks = self.cb.callbacks.load(Relaxed); self.stats.contended = self.cb.contended.load(Relaxed); self.stats.audible = self.cb.audible.load(Relaxed);
        Some(self.stats.clone())
    }
    fn take_drift(&mut self) -> Vec<f32> { std::mem::take(&mut self.stats.drift_ms) }
}
