//! The output device behind one trait (canon row 139: direct CoreAudio + WASAPI, NOT cpal — the servo wants the
//! device's real playout latency). The render callback is a boxed closure filling interleaved stereo f32 frames;
//! `NullDevice` paces that closure at wall time for headless runs and tests.
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering::*};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub type RenderFn = Box<dyn FnMut(&mut [f32]) + Send>;

pub trait AudioDevice: Send {
    fn start(&mut self);
    fn stop(&mut self);
    fn is_running(&self) -> bool;
    fn sample_rate(&self) -> i32;
    fn buffer_frame_count(&self) -> u32;
    /// The device's playout latency in seconds (unit latency + one buffer) — the servo subtracts it.
    fn buffer_latency_seconds(&self) -> f64;
    fn frames_rendered(&self) -> u64;
}

/// A clock without a speaker: calls the render closure every buffer at wall pace (headless benches, CI).
pub struct NullDevice { sample_rate: i32, buffer_frames: u32, should_play: Arc<AtomicBool>, render: Arc<Mutex<Option<RenderFn>>>, thread: Option<std::thread::JoinHandle<()>>, quit: Arc<AtomicBool>, rendered: Arc<AtomicU64> }
impl NullDevice {
    pub fn new(render: RenderFn, sample_rate: i32, buffer_ms: u32) -> NullDevice {
        let buffer_frames = (sample_rate as u32 * buffer_ms / 1000).max(1);
        let d = NullDevice { sample_rate, buffer_frames, should_play: Arc::new(AtomicBool::new(false)), render: Arc::new(Mutex::new(Some(render))), thread: None, quit: Arc::new(AtomicBool::new(false)), rendered: Arc::new(AtomicU64::new(0)) };
        d
    }
}
impl AudioDevice for NullDevice {
    fn start(&mut self) {
        self.should_play.store(true, Release);
        if self.thread.is_some() { return }
        let (play, render, quit, rendered) = (self.should_play.clone(), self.render.clone(), self.quit.clone(), self.rendered.clone());
        let (frames, rate) = (self.buffer_frames as usize, self.sample_rate as f64);
        self.thread = Some(std::thread::Builder::new().name("qcv-audio-null".into()).spawn(move || {
            let period = Duration::from_secs_f64(frames as f64 / rate);
            let mut buf = vec![0.0f32; frames * 2]; let mut next = Instant::now();
            while !quit.load(Relaxed) {
                next += period; let now = Instant::now(); if next > now { std::thread::sleep(next - now); } else if now - next > period * 4 { next = now; }
                if play.load(Acquire) { if let Some(r) = render.lock().unwrap().as_mut() { r(&mut buf); rendered.fetch_add(frames as u64, Relaxed); } }
            }
        }).expect("null audio device"));
    }
    fn stop(&mut self) { self.should_play.store(false, Release); }
    fn is_running(&self) -> bool { self.should_play.load(Acquire) }
    fn sample_rate(&self) -> i32 { self.sample_rate }
    fn buffer_frame_count(&self) -> u32 { self.buffer_frames }
    fn buffer_latency_seconds(&self) -> f64 { self.buffer_frames as f64 / self.sample_rate as f64 }
    fn frames_rendered(&self) -> u64 { self.rendered.load(Relaxed) }
}
impl Drop for NullDevice { fn drop(&mut self) { self.quit.store(true, Relaxed); if let Some(t) = self.thread.take() { let _ = t.join(); } } }

/// The platform's default output: CoreAudio's default-output AudioUnit on macOS (QCView `CoreAudioDevice`); WASAPI on
/// Windows arrives with the rhombus pass.
pub fn default_device(render: RenderFn, sample_rate: i32, buffer_ms: u32) -> Result<Box<dyn AudioDevice>, crate::Error> {
    #[cfg(target_os = "macos")] { return super::coreaudio::CoreAudioDevice::new(render, sample_rate, buffer_ms).map(|d| Box::new(d) as Box<dyn AudioDevice>) }
    #[allow(unreachable_code)] { let _ = (render, sample_rate, buffer_ms); Err(crate::Error::Open("no audio device layer on this platform yet (WASAPI: rhombus pass)".into())) }
}
