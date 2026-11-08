//! What the shell's bands read and drive, over either transport (single `Transport` or `DualTransport`): the status
//! strip, the transport bar, the compact strip and the scrubber see one playhead whichever is loaded.
use crate::{dual::DualTransport, key_map::Action, transport::Transport};
use qcv_timeline::Rate;
use std::time::Instant;

/// The playhead's timecode description: (rate num, den), drop-frame, the embedded start timecode, the duration (s).
pub struct TimecodeInfo { pub frame_rate: (i32, i32), pub drop_frame: bool, pub source_timecode: Option<String>, pub duration_secs: f64 }

pub trait Playback {
    fn current_frame(&self) -> i32;
    fn frame_count(&self) -> i32;
    fn rate(&self) -> Rate;
    fn is_playing(&self) -> bool;
    fn is_scrubbing(&self) -> bool;
    fn loop_enabled(&self) -> bool;
    fn in_point(&self) -> i32;
    fn out_point(&self) -> i32;
    fn review_speed(&self) -> f64;
    fn has_audio(&self) -> bool;
    fn is_muted(&self) -> bool;
    fn volume(&self) -> f32;
    fn timecode_info(&self) -> TimecodeInfo;
    fn apply(&mut self, a: Action, now: Instant) -> bool;
    fn seek_to_frame(&mut self, f: i32);
    fn begin_scrub(&mut self);
    fn scrub_to_frame(&mut self, f: i32);
    fn end_scrub(&mut self);
    fn set_review_speed(&mut self, s: f64);
    fn play(&mut self);
    fn pause(&mut self);
}

impl Playback for Transport {
    fn current_frame(&self) -> i32 { Transport::current_frame(self) }
    fn frame_count(&self) -> i32 { Transport::frame_count(self) }
    fn rate(&self) -> Rate { Transport::rate(self) }
    fn is_playing(&self) -> bool { Transport::is_playing(self) }
    fn is_scrubbing(&self) -> bool { Transport::is_scrubbing(self) }
    fn loop_enabled(&self) -> bool { Transport::loop_enabled(self) }
    fn in_point(&self) -> i32 { Transport::in_point(self) }
    fn out_point(&self) -> i32 { Transport::out_point(self) }
    fn review_speed(&self) -> f64 { Transport::review_speed(self) }
    fn has_audio(&self) -> bool { self.audio.has_audio() }
    fn is_muted(&self) -> bool { self.audio.is_muted() }
    fn volume(&self) -> f32 { self.audio.volume() }
    fn timecode_info(&self) -> TimecodeInfo { let i = self.info(); TimecodeInfo { frame_rate: i.frame_rate, drop_frame: i.drop_frame, source_timecode: i.source_timecode.clone(), duration_secs: i.duration_secs } }
    fn apply(&mut self, a: Action, now: Instant) -> bool { Transport::apply(self, a, now) }
    fn seek_to_frame(&mut self, f: i32) { Transport::seek_to_frame(self, f) }
    fn begin_scrub(&mut self) { Transport::begin_scrub(self) }
    fn scrub_to_frame(&mut self, f: i32) { Transport::scrub_to_frame(self, f) }
    fn end_scrub(&mut self) { Transport::end_scrub(self) }
    fn set_review_speed(&mut self, s: f64) { Transport::set_review_speed(self, s) }
    fn play(&mut self) { Transport::play(self) }
    fn pause(&mut self) { Transport::pause(self) }
}

impl Playback for DualTransport {
    fn current_frame(&self) -> i32 { DualTransport::current_frame(self) }
    fn frame_count(&self) -> i32 { DualTransport::frame_count(self) }
    fn rate(&self) -> Rate { DualTransport::rate(self) }
    fn is_playing(&self) -> bool { DualTransport::is_playing(self) }
    fn is_scrubbing(&self) -> bool { DualTransport::is_scrubbing(self) }
    fn loop_enabled(&self) -> bool { DualTransport::loop_enabled(self) }
    fn in_point(&self) -> i32 { DualTransport::in_point(self) }
    fn out_point(&self) -> i32 { DualTransport::out_point(self) }
    fn review_speed(&self) -> f64 { DualTransport::review_speed(self) }
    fn has_audio(&self) -> bool { self.side(crate::dual::Side::A).is_some_and(|t| t.audio.has_audio()) }
    fn is_muted(&self) -> bool { self.side(crate::dual::Side::A).is_some_and(|t| t.audio.is_muted()) }
    fn volume(&self) -> f32 { self.side(crate::dual::Side::A).map(|t| t.audio.volume()).unwrap_or(1.0) }
    /// QCView's dual timecode: the master rate, non-drop, from zero
    fn timecode_info(&self) -> TimecodeInfo { let r = DualTransport::rate(self); TimecodeInfo { frame_rate: (r.num as i32, r.den as i32), drop_frame: false, source_timecode: None, duration_secs: self.frame_count() as f64 / r.as_f64().max(1e-9) } }
    fn apply(&mut self, a: Action, now: Instant) -> bool { DualTransport::apply(self, a, now) }
    fn seek_to_frame(&mut self, f: i32) { DualTransport::seek_to_frame(self, f) }
    fn begin_scrub(&mut self) { DualTransport::begin_scrub(self) }
    fn scrub_to_frame(&mut self, f: i32) { DualTransport::scrub_to_frame(self, f) }
    fn end_scrub(&mut self) { DualTransport::end_scrub(self) }
    fn set_review_speed(&mut self, s: f64) { DualTransport::set_review_speed(self, s) }
    fn play(&mut self) { DualTransport::play(self) }
    fn pause(&mut self) { DualTransport::pause(self) }
}
