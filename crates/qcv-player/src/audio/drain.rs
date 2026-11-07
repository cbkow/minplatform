//! `AudioDrainStage` (QCView `audio/audio_drain_stage.h`), verbatim: the render-callback-side drain of one source
//! through the servo resampler, with every discontinuity softened by a 5-ms fade — a seek fades the pre-seek tail out
//! and the post-seek audio in (the ring marks the boundary instead of clearing), an underrun fades out and back in, a
//! hold freezes without consuming, a skip drops ring frames to re-align without a decoder seek (counted). Only REAL
//! post-seek frames count as consumption. Callback-only; `Request` is what other threads touch.
use super::resampler::FractionalResampler;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering::*};

/// What the drain reads from (the decoder's consumer side).
pub trait AudioSource: Send + Sync {
    /// Fill `out` (stereo frames × 2) from the ring, padding silence; returns the REAL frames read.
    fn read(&self, out: &mut [f32]) -> usize;
    fn seek_pending(&self) -> bool;
    fn flush_generation(&self) -> u32;
    fn stale_frames(&self) -> usize;
    fn discard_stale(&self);
}

/// Cross-thread knobs (UI writes, callback reads).
pub struct Request { pub hold: AtomicBool, pub skip_frames: AtomicU32, pub restart: AtomicBool }
impl Default for Request { fn default() -> Self { Request { hold: AtomicBool::new(false), skip_frames: AtomicU32::new(0), restart: AtomicBool::new(true) } } }

/// 5 ms at 48 kHz: long enough to kill the click of a step, short enough to be a non-event.
pub const FADE_FRAMES: usize = 240;

pub struct AudioDrainStage { resampler: FractionalResampler, last_gen: u32, gen_valid: bool, live: bool, fade_in: usize, pub underruns: u64, pub fades: u64 }

impl Default for AudioDrainStage { fn default() -> Self { Self::new() } }
impl AudioDrainStage {
    pub fn new() -> AudioDrainStage { AudioDrainStage { resampler: FractionalResampler::new(), last_gen: 0, gen_valid: false, live: false, fade_in: 0, underruns: 0, fades: 0 } }
    pub fn live(&self) -> bool { self.live }
    fn silence(out: &mut [f32]) { out.fill(0.0); }
    /// Fill `out` (`frame_count` stereo frames) from `dec` at `ratio` source frames per output frame; `scratch` holds
    /// ≥ its frames; real frames consumed are added to `consumed`. `extra_hold` = the owner's own hold (not playing).
    #[allow(clippy::too_many_arguments)]
    pub fn process(&mut self, dec: &dyn AudioSource, out: &mut [f32], frame_count: usize, ratio: f64, scratch: &mut [f32], consumed: &AtomicU64, req: &Request, extra_hold: bool) {
        if req.restart.swap(false, AcqRel) { self.live = false; self.gen_valid = false; self.fade_in = 0; self.resampler.reset(); }
        let generation = dec.flush_generation();
        if !self.gen_valid { self.gen_valid = true; self.last_gen = generation; dec.discard_stale(); }
        else if generation != self.last_gen {
            self.last_gen = generation;
            if self.live {
                let stale = dec.stale_frames();
                self.drain(dec, out, frame_count, ratio, scratch, None, stale, true);
                dec.discard_stale(); self.resampler.reset(); return
            }
            dec.discard_stale(); self.resampler.reset();
        }
        if dec.seek_pending() { if self.live { self.drain(dec, out, frame_count, ratio, scratch, None, usize::MAX, false); } else { Self::silence(out); } return }
        if extra_hold || req.hold.load(Acquire) { if self.live { self.drain(dec, out, frame_count, ratio, scratch, Some(consumed), usize::MAX, true); } else { Self::silence(out); } return }
        let mut skip = req.skip_frames.load(Acquire);
        if skip > 0 {
            if self.live { self.drain(dec, out, frame_count, ratio, scratch, Some(consumed), usize::MAX, true); return }
            let scratch_frames = scratch.len() / 2;
            while skip > 0 {
                let n = (skip as usize).min(scratch_frames);
                let got = dec.read(&mut scratch[..n * 2]);
                consumed.fetch_add(got as u64, Relaxed);
                skip -= got as u32;
                if got < n { break }
            }
            req.skip_frames.store(skip, Release);
            if skip > 0 { Self::silence(out); return }
            self.resampler.reset();
        }
        self.drain(dec, out, frame_count, ratio, scratch, Some(consumed), usize::MAX, false);
    }
    #[allow(clippy::too_many_arguments)]
    fn drain(&mut self, dec: &dyn AudioSource, out: &mut [f32], frame_count: usize, ratio: f64, scratch: &mut [f32], counted: Option<&AtomicU64>, max_src: usize, force_fade_out: bool) {
        let scratch_frames = scratch.len() / 2;
        let src_needed = self.resampler.source_frames_needed(frame_count, ratio);
        if src_needed > scratch_frames { Self::silence(out); self.live = false; return }   // a callback larger than the scratch: silence beats allocating on the RT thread
        let want = src_needed.min(max_src);
        let got = if want > 0 { dec.read(&mut scratch[..want * 2]) } else { 0 };
        if got < src_needed { scratch[got * 2..src_needed * 2].fill(0.0); }
        if let Some(c) = counted { c.fetch_add(got as u64, Relaxed); }
        if got == 0 { if self.live { self.underruns += 1; } Self::silence(out); self.live = false; return }
        if !self.live { self.fade_in = FADE_FRAMES; self.fades += 1; }
        let ends_silent = got < src_needed || force_fade_out;
        if ends_silent {
            let n = got.min(FADE_FRAMES);
            for i in 0..n { let g = (n - 1 - i) as f32 / n as f32; scratch[(got - n + i) * 2] *= g; scratch[(got - n + i) * 2 + 1] *= g; }
            self.fades += 1;
        }
        self.resampler.process(scratch, src_needed, out, frame_count, ratio);
        if self.fade_in > 0 {
            let n = self.fade_in.min(frame_count);
            for i in 0..n { let g = (FADE_FRAMES - self.fade_in + i) as f32 / FADE_FRAMES as f32; out[i * 2] *= g; out[i * 2 + 1] *= g; }
            self.fade_in -= n;
        }
        self.live = !ends_silent;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicBool as AB;
    struct Fake { ring: super::super::ring::AudioRing, pending: AB }
    impl AudioSource for Fake {
        fn read(&self, out: &mut [f32]) -> usize { let n = self.ring.read(out); out[n..].fill(0.0); n / 2 }
        fn seek_pending(&self) -> bool { self.pending.load(Relaxed) }
        fn flush_generation(&self) -> u32 { self.ring.stale_generation() }
        fn stale_frames(&self) -> usize { self.ring.stale_samples() / 2 }
        fn discard_stale(&self) { self.ring.discard_stale() }
    }
    #[test]
    fn fades_in_then_fades_the_pre_seek_tail_out_on_a_new_generation() {
        let f = Fake { ring: super::super::ring::AudioRing::new(48000), pending: AB::new(false) };
        f.ring.write(&vec![0.5f32; 9600]);
        let mut d = AudioDrainStage::new(); let req = Request::default(); let consumed = AtomicU64::new(0);
        let mut out = vec![0.0f32; 960]; let mut scratch = vec![0.0f32; 4096];
        d.process(&f, &mut out, 480, 1.0, &mut scratch, &consumed, &req, false);
        assert!(out[0].abs() < 0.01 && (out[959] - 0.5).abs() < 0.05, "fades in over 5 ms: {} … {}", out[0], out[959]);
        assert!(d.live() && consumed.load(Relaxed) == 480);
        f.ring.mark_stale();                                   // a seek landed: the rest is pre-seek audio
        d.process(&f, &mut out, 480, 1.0, &mut scratch, &consumed, &req, false);
        assert!(out[0].abs() > 0.3 && out[959].abs() < 0.02, "the tail fades out: {} … {}", out[0], out[959]);
        assert_eq!(consumed.load(Relaxed), 480, "the stale tail is not counted");
        assert_eq!(f.ring.available_read(), 0, "and discarded");
    }
}
