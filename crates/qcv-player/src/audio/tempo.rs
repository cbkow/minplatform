//! `TempoStage` (QCView `audio/tempo_stage.cpp`): SoundTouch WSOLA for constant-pitch review speeds (0.25–4×), a
//! producer-side stage on the decode thread between decoded PCM and the ring write — WSOLA wants 60–100 ms windows and
//! allocates, so it is never on the device callback. Exactly 1.0 = bypass and the 1× path stays byte-identical; a
//! tempo change applies lazily at the next `put` (clearing WSOLA state) and the owner re-anchors with a seek anyway.
//! The `soundtouch` crate compiles SoundTouch 2.3.2 in (LGPL-2.1; QCView links 2.4.1). The knobs (`TempoControl`) are
//! shared atomics any thread may set; the stretcher itself is confined to the decode thread.
use soundtouch::SoundTouch;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering::*};
use std::sync::Arc;

#[derive(Default)]
pub struct TempoControl { tempo: AtomicU64, bypassed: AtomicBool, dirty: AtomicBool }
impl TempoControl {
    pub fn new() -> TempoControl { TempoControl { tempo: AtomicU64::new(1.0f64.to_bits()), bypassed: AtomicBool::new(true), dirty: AtomicBool::new(false) } }
    /// Any thread. Clamped to [0.25, 4.0]; exactly 1.0 → bypass.
    pub fn set_tempo(&self, tempo: f64) {
        let t = tempo.clamp(0.25, 4.0);
        if f64::from_bits(self.tempo.swap(t.to_bits(), AcqRel)) == t { return }
        self.bypassed.store(t == 1.0, Release);
        self.dirty.store(true, Release);
    }
    pub fn tempo(&self) -> f64 { f64::from_bits(self.tempo.load(Relaxed)) }
    pub fn bypassed(&self) -> bool { self.bypassed.load(Acquire) }
}

pub struct TempoStage { st: SoundTouch, pub ctl: Arc<TempoControl> }
unsafe impl Send for TempoStage {}

impl Default for TempoStage { fn default() -> Self { Self::new() } }
impl TempoStage {
    pub fn new() -> TempoStage {
        let mut st = SoundTouch::new();
        st.set_sample_rate(48000).set_channels(2).set_tempo(1.0);
        TempoStage { st, ctl: Arc::new(TempoControl::new()) }
    }
    pub fn bypassed(&self) -> bool { self.ctl.bypassed() }
    /// Decode thread only: applies a pending tempo change, then feeds `frames` interleaved stereo frames.
    pub fn put(&mut self, input: &[f32], frames: usize) {
        if self.ctl.dirty.swap(false, AcqRel) { self.st.clear(); let t = self.ctl.tempo(); self.st.set_tempo(t); }
        if frames == 0 { return }
        self.st.put_samples(&input[..frames * 2], frames);
    }
    /// Drain up to `max_frames` stretched frames; 0 when the stretcher needs more input.
    pub fn receive(&mut self, out: &mut [f32], max_frames: usize) -> usize { if max_frames == 0 { return 0 } self.st.receive_samples(out, max_frames) }
    /// Discard buffered input / output (on seek).
    pub fn flush(&mut self) { self.st.clear(); }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn two_x_halves_the_output_and_one_x_bypasses() {
        let mut t = TempoStage::new();
        assert!(t.bypassed());
        t.ctl.set_tempo(2.0); assert!(!t.bypassed());
        let n = 96000; let input: Vec<f32> = (0..n).flat_map(|i| { let v = (i as f32 * 440.0 * std::f32::consts::TAU / 48000.0).sin() * 0.5; [v, v] }).collect();
        t.put(&input, n);
        let mut out = vec![0.0f32; 8192 * 2]; let mut total = 0;
        loop { let got = t.receive(&mut out, 8192); if got == 0 { break } total += got; }
        assert!(total > 40000 && total < 50000, "≈ half the frames out: {total}");
        t.ctl.set_tempo(1.0); assert!(t.bypassed());
    }
}
