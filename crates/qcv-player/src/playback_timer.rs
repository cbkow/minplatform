//! `PlaybackTimer` — ported from `QCView-Player/src/timeline/playback_timer.{h,cpp}` (Phase 7.2.b): the single source of
//! truth for playback time across video, audio, scrubbing and drift correction. Two clock policies, both first-class:
//! `WallClock` — `update()` advances position by (now − last) × speed; if the decoder can't keep up, frames skip but the
//! real-time cadence is preserved. `FrameLocked` — `advance_by_frame()` advances exactly 1/fps per call regardless of the
//! wall clock; if the decoder stalls, playback slows rather than skipping (image sequences, and video review where every
//! frame must be seen). Loop ranges stay off the timer's ownership (Guide 02 D7): the Transport pushes [in, out + 1) and
//! the timer only wraps inside it. Signals became return values + `take_frame_crossed()`.
use std::time::Instant;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClockPolicy { WallClock, FrameLocked }

pub struct PlaybackTimer {
    position: f64, duration: f64, frame_rate: f64, playback_speed: f64, playing: bool, policy: ClockPolicy,
    last_update: Option<Instant>, first_update: bool, looping: bool, loop_in: f64, loop_out: f64,
    crossed: Option<i32>, /// set by `update` when a loop wrap moved the position backwards (the Transport re-seats the decoder)
    pub wrapped: bool,
}
impl Default for PlaybackTimer { fn default() -> Self { PlaybackTimer { position: 0.0, duration: 0.0, frame_rate: 24.0, playback_speed: 1.0, playing: false, policy: ClockPolicy::WallClock, last_update: None, first_update: true, looping: false, loop_in: 0.0, loop_out: 0.0, crossed: None, wrapped: false } } }

impl PlaybackTimer {
    pub fn new() -> PlaybackTimer { PlaybackTimer::default() }
    /// Wall-clock advance; call from the render loop. Returns true if the position changed.
    pub fn update(&mut self, now: Instant) -> bool {
        self.wrapped = false;
        if !self.playing { self.last_update = Some(now); self.first_update = false; return false }   // keeps the delta fresh for the resume
        if self.policy == ClockPolicy::FrameLocked { self.advance_by_frame(); return true }           // the caller is the metronome
        if self.first_update { self.last_update = Some(now); self.first_update = false; return false }
        let dt = self.last_update.map(|t| (now - t).as_secs_f64()).unwrap_or(0.0);
        self.last_update = Some(now);
        if dt <= 0.0 { return false }
        let old_frame = self.current_frame(); let old_pos = self.position;
        self.position += dt * self.playback_speed;
        // Loop wrap: inside [in, out) when a range is set (ignores duration so wraps past the clip end preview), else on the
        // clip duration; fmod keeps the sub-frame remainder so the seam stays smooth.
        if self.looping && self.loop_out > self.loop_in {
            let span = self.loop_out - self.loop_in;
            if self.position >= self.loop_out { self.position = self.loop_in + (self.position - self.loop_in) % span; self.wrapped = true; }
            else if self.position < self.loop_in { self.position = self.loop_out - (self.loop_in - self.position) % span; self.wrapped = true; }
        } else if self.looping && self.duration > 0.0 {
            if self.position >= self.duration { self.position %= self.duration; self.wrapped = true; }
            else if self.position < 0.0 { self.position = self.position % self.duration + self.duration; self.wrapped = true; }
        } else { self.clamp_position(); }
        let _ = old_pos;
        self.notify_frame_crossed(old_frame);
        true
    }
    /// Frame-locked advance: exactly 1/fps regardless of the wall clock.
    pub fn advance_by_frame(&mut self) {
        if self.frame_rate <= 0.0 { return }
        let old = self.current_frame();
        self.position += 1.0 / self.frame_rate;
        self.clamp_position();
        self.notify_frame_crossed(old);
    }
    /// Divergence from QCView's PlaybackTimer (reason): it set `m_firstUpdate` on play and on seek so the NEXT update only
    /// re-based the delta — which drops one whole refresh of time from the clock at every play / seek landing (measured
    /// 2026-11-07: the audio ran ~50 ms ahead of the picture from the first frame until the servo ground it out). The
    /// base is taken NOW instead: no tick is skipped and no stale delta can land either.
    pub fn play(&mut self) { if self.playing { return } self.playing = true; self.last_update = Some(Instant::now()); self.first_update = false; }
    pub fn pause(&mut self) { self.playing = false; }
    pub fn toggle_play_pause(&mut self) { if self.playing { self.pause() } else { self.play() } }
    /// Absolute seek in seconds; pauses the integrator for one tick so no stale delta lands after it.
    pub fn seek(&mut self, seconds: f64) {
        let old = self.current_frame();
        let mut s = seconds.max(0.0);
        if self.duration > 0.0 && s > self.duration { s = self.duration; }
        // re-base BEFORE the unchanged-position return: the seek gate skips `update` while a landing is in flight, so a
        // seek to the position already held (the open at 0; the landing confirming an immediate seek) would otherwise leave
        // the old base and the first update after the gate would integrate the whole landing time at once (measured
        // 2026-11-07: the clock 20–40 ms ahead of the audio after every seek)
        self.last_update = Some(Instant::now()); self.first_update = false;   // see `play`
        if (self.position - s).abs() < 1e-12 { return }
        self.position = s;
        self.notify_frame_crossed(old);
    }
    pub fn seek_relative(&mut self, delta: f64) { self.seek(self.position + delta) }
    pub fn seek_to_start(&mut self) { self.seek(0.0) }
    pub fn seek_to_end(&mut self) { self.seek(self.duration) }
    /// Frame-precise stepping: pause, then seek by exactly n / fps (drift-free: through `seek`, never accumulated).
    pub fn step_forward(&mut self, frames: i32) { if self.frame_rate <= 0.0 || frames == 0 { return } self.pause(); self.seek(self.position + frames as f64 / self.frame_rate); }
    pub fn step_backward(&mut self, frames: i32) { self.step_forward(-frames) }
    pub fn set_duration(&mut self, seconds: f64) { self.duration = seconds.max(0.0); if self.position > self.duration { self.position = self.duration; } }
    pub fn set_frame_rate(&mut self, fps: f64) { if fps > 0.0 { self.frame_rate = fps; } }
    /// Clamped to 0.0625 .. 8 (the old app capped at 4×: beyond, the decoder can't keep up and frames just skip).
    pub fn set_playback_speed(&mut self, speed: f64) { let s = speed.clamp(0.0625, 8.0); if (self.playback_speed - s).abs() < 1e-12 { return } self.playback_speed = s; self.first_update = true; }
    pub fn set_clock_policy(&mut self, p: ClockPolicy) { if self.policy != p { self.policy = p; self.first_update = true; } }
    pub fn set_looping(&mut self, on: bool) { self.looping = on }
    pub fn looping(&self) -> bool { self.looping }
    /// Empty / inverted / negative-in clears the range.
    pub fn set_loop_range(&mut self, in_sec: f64, out_sec: f64) { if in_sec < 0.0 || out_sec <= in_sec { self.loop_in = 0.0; self.loop_out = 0.0; } else { self.loop_in = in_sec; self.loop_out = out_sec; } }
    pub fn has_loop_range(&self) -> bool { self.loop_out > self.loop_in && self.loop_in >= 0.0 }
    pub fn position(&self) -> f64 { self.position }
    pub fn duration(&self) -> f64 { self.duration }
    pub fn frame_rate(&self) -> f64 { self.frame_rate }
    pub fn playback_speed(&self) -> f64 { self.playback_speed }
    pub fn is_playing(&self) -> bool { self.playing }
    pub fn clock_policy(&self) -> ClockPolicy { self.policy }
    /// Rounded to nearest: keeps the displayed frame stable around half-frame boundaries.
    pub fn current_frame(&self) -> i32 { if self.frame_rate <= 0.0 { 0 } else { (self.position * self.frame_rate).round() as i32 } }
    pub fn total_frames(&self) -> i32 { if self.frame_rate <= 0.0 || self.duration <= 0.0 { 0 } else { (self.duration * self.frame_rate).round() as i32 } }
    /// The `frameAdvanced` signal: the frame crossed since the last take, if any.
    pub fn take_frame_crossed(&mut self) -> Option<i32> { self.crossed.take() }
    fn clamp_position(&mut self) { if self.position < 0.0 { self.position = 0.0; } if self.duration > 0.0 && self.position > self.duration { self.position = self.duration; } }
    fn notify_frame_crossed(&mut self, old: i32) { let n = self.current_frame(); if n != old { self.crossed = Some(n); } }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    #[test] fn wall_clock_advances_and_wraps_in_range() {
        let mut t = PlaybackTimer::new(); t.set_frame_rate(24.0); t.set_duration(10.0); t.set_looping(true); t.set_loop_range(1.0, 2.0);
        let t0 = Instant::now(); t.seek(1.5); t.play(); t.update(t0);
        assert!(t.update(t0 + Duration::from_millis(400)) && !t.wrapped); assert!((t.position() - 1.9).abs() < 1e-6);
        assert!(t.update(t0 + Duration::from_millis(600)) && t.wrapped); assert!((t.position() - 1.1).abs() < 1e-6);   // 2.1 wraps to 1.1
        assert_eq!(t.current_frame(), 26);
    }
    #[test] fn steps_pause_and_speed_clamps() {
        let mut t = PlaybackTimer::new(); t.set_frame_rate(24.0); t.set_duration(1.0); t.play();
        t.step_forward(3); assert!(!t.is_playing()); assert_eq!(t.current_frame(), 3); assert_eq!(t.take_frame_crossed(), Some(3));
        t.set_playback_speed(100.0); assert_eq!(t.playback_speed(), 8.0);
        t.seek_to_end(); assert_eq!(t.current_frame(), 24); assert_eq!(t.total_frames(), 24);
    }
}
