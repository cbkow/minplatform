//! `Transport` — ported from `WindowManager`'s transport slice (`QCView-Player/src/window/window_manager.{h,cpp}`: play /
//! pause / togglePlayback with `m_userWantsPlayback`, seekToTime / seekToFrame (+AndResume) with the 50 ms playlist
//! debounce, stepFrames, seekToStart / End, the review speeds, loop + in / out with `pushInOutToTimer`, the fast-seek
//! shuttle (`startFastSeek` / `stopFastSeek` and its 33 ms tick), begin / endModal, the end-of-stream loop wrap and the
//! in / out snap) plus `TimelinePanel.qml`'s scrub gesture (pause, preview through the scrub decoder, the heavy seek on
//! release). Canon §8.5: ONE object owns the decoder, the scrub decoder, the audio hooks, the shuttle and the clock; the UI
//! binds to it, never to its parts; the constants (row 88) live here once.
//!
//! The one structural divergence, carried from step 1b: the streaming decoder runs `Policy::Clocked`, so the
//! `PlaybackTimer` LEADS video playback here (QCView's single-video mode lets the decoder pace itself and mirrors the
//! timer from `currentFrameChanged`; its shuttle / dual / image-sequence paths are timer-leads already). What that
//! changes: each refresh fetches the frame due at the timer's frame; a seek while playing holds the integrator until the
//! sought frame lands and re-seats the timer there (QCView's decoder re-baselines its pacing on that same frame); the
//! loop wrap is the timer's (the decoder is re-seated at the wrap frame, as QCView's EndOfStream handler seeks to 0 and
//! resumes); FrameLocked advances one frame per refresh only when the decoder has delivered it ("playback slows rather
//! than skipping").
use crate::{ffmpeg::{OpenOptions, SeqOptions, SessionInfo}, frame_handle::FrameHandle, frame_index::FrameIndex, hw::HwDevice, key_map::Action, playback_timer::{ClockPolicy, PlaybackTimer}, scrub_decoder::{ScrubDecoder, ScrubOptions}, source::Source, video_decoder::{Policy, State, VideoDecoder}, Error};
use qcv_timeline::Rate;
use std::{collections::BTreeMap, sync::Arc, time::{Duration, Instant}};

/// Fast-seek (FF / RW) — the old app's TimelinePlaybackController constants: 2× initial, 32× cap, doubles per second, 33 ms tick.
pub const FAST_SEEK_INITIAL_SPEED: f64 = 2.0;
pub const FAST_SEEK_MAX_SPEED: f64 = 32.0;
pub const FAST_SEEK_ACCELERATION: f64 = 2.0;
pub const FAST_SEEK_TICK_MS: u64 = 33;
/// R cycles this ring (nearest current → next); ⇧R snaps to 1×; the setter clamps 0.25 .. 4.
pub const REVIEW_SPEEDS: [f64; 6] = [0.5, 0.75, 1.0, 1.25, 1.5, 2.0];
/// A burst of playlist seeks collapses to one `apply_playlist_seek` for the latest target.
pub const PLAYLIST_SEEK_DEBOUNCE_MS: u64 = 50;
/// Frames decoded ahead of the clock (QCView's slot of one, widened for the clock; the presenter's ring holds 3 more).
pub const LOOKAHEAD: usize = 3;

/// The audio player's call sites, in QCView's shape; step 2b implements them. `NoAudio` is silence.
pub trait AudioHooks {
    fn has_audio(&self) -> bool { false }
    fn play(&mut self) {}
    fn pause(&mut self) {}
    fn seek(&mut self, _source_sec: f64) {}
    /// hold after play until the first picture lands; `release_hold` at the frame that landed
    fn set_hold(&mut self, _on: bool) {}
    fn release_hold(&mut self, _source_sec: f64) {}
    /// the drift-correction loop, fed the master clock while playing
    fn update(&mut self, _source_sec: f64) {}
    fn set_tempo(&mut self, _speed: f64) {}
    fn begin_shuttle(&mut self, _path: &str, _source_sec: f64, _signed_speed: f64) {}
    fn shuttle_target(&mut self, _path: &str, _source_sec: f64, _signed_speed: f64) {}
    fn end_shuttle(&mut self) {}
    fn shuttle_active(&self) -> bool { false }
    fn toggle_mute(&mut self) {}
    fn volume_step(&mut self, _delta: f32) {}
}
pub struct NoAudio;
impl AudioHooks for NoAudio {}

#[derive(Default, Debug)]
pub struct Stats {
    /// shown-frame lateness vs the clock, ms (negative = early)
    pub lateness_ms: Vec<f32>, /// refreshes per shown frame → count
    pub cadence: BTreeMap<i64, u32>, pub dropped: u32, pub late: u32, pub black: u32, pub shown: u32, pub decode_ms: Vec<f32>, pub loops: u32,
    /// seek issued → the sought frame landed (the stream's own answer), ms
    pub seek_land_ms: Vec<f32>,
}

pub struct TransportOptions { pub hw: Option<HwDevice>, pub threads: i32, /// None = no scrub decoder (tests)
    pub scrub: Option<ScrubOptions>, /// QCView: `playback/loopEnabled`, default ON ("press play, watch a cycle, scrub, watch again")
    pub loop_enabled: bool, pub clock_policy: ClockPolicy, /// one refresh in ms — the "late" threshold
    pub refresh_ms: f32 }
impl Default for TransportOptions { fn default() -> Self { TransportOptions { hw: None, threads: 0, scrub: Some(ScrubOptions::default()), loop_enabled: true, clock_policy: ClockPolicy::WallClock, refresh_ms: 1000.0 / 60.0 } } }

struct FastSeek { dir: i32, speed: f64, started: Instant, last: Instant, last_tick: Instant, position_sec: f64 }

pub struct Transport {
    pub decoder: VideoDecoder, pub scrub: Option<ScrubDecoder>, pub timer: PlaybackTimer, pub audio: Box<dyn AudioHooks>,
    info: SessionInfo, index: Arc<FrameIndex>, rate: Rate, frames: i32, fps: f64, path: String,
    review_speed: f64, loop_enabled: bool, in_point: i32, out_point: i32,
    /// user intent, captured BEFORE the decoder flips — the loop wrap reads it to decide on the auto-resume
    user_wants_playback: bool,
    fast: Option<FastSeek>, modal_active: bool, modal_was_playing: bool, scrubbing: Option<bool>,
    /// a seek in flight: (target, the decoder's seek counter when issued, issued at) — the integrator holds until it lands
    seek_gate: Option<(i32, u32, Instant)>,
    playlist_active: bool, pending_playlist_seek: Option<(f64, Instant)>, resume_after_playlist_seek: bool,
    pub stats: Stats, last_shown: Option<i64>, refreshes_on_current: i64, pub refresh_ms: f32, current: Option<FrameHandle>, first_landed: bool,
}

impl Transport {
    pub fn open(src: &Source, opts: TransportOptions) -> Result<Transport, Error> {
        let open = OpenOptions { threads: opts.threads, hw: opts.hw.clone(), seq: match src { Source::Sequence { rate, start, .. } => Some(SeqOptions { rate: *rate, start_number: *start }), _ => None } };
        let mut decoder = VideoDecoder::new(Policy::Clocked { lookahead: LOOKAHEAD });
        decoder.open(src.path(), &open)?;
        let info = decoder.info.clone().unwrap();
        let index = decoder.frame_index().cloned().ok_or_else(|| Error::Open("no frame index".into()))?;
        let rate = info.rate(); let fps = rate.as_f64(); let frames = info.frame_count as i32;
        let scrub = match (&opts.scrub, src) {
            (Some(so), Source::File(_)) => { let so = ScrubOptions { hw: opts.hw.clone(), ..so.clone() }; match ScrubDecoder::open(src.path(), &so, index.clone(), decoder.external_sink()) { Ok(s) => Some(s), Err(e) => { log::warn!("Transport: no scrub decoder: {e}"); None } } }
            _ => None,
        };
        let mut timer = PlaybackTimer::new();
        timer.set_frame_rate(fps); timer.set_duration(if fps > 0.0 { frames as f64 / fps } else { 0.0 }); timer.set_clock_policy(opts.clock_policy); timer.set_looping(opts.loop_enabled);
        let seeks = decoder.seeks_performed();
        Ok(Transport { decoder, scrub, timer, audio: Box::new(NoAudio), info, index, rate, frames, fps, path: src.path().to_string(),
            review_speed: 1.0, loop_enabled: opts.loop_enabled, in_point: -1, out_point: -1, user_wants_playback: false,
            fast: None, modal_active: false, modal_was_playing: false, scrubbing: None,
            seek_gate: Some((0, seeks, Instant::now())),   // the open's first frame re-seats the clock at frame 0 (paused at open)
            playlist_active: false, pending_playlist_seek: None, resume_after_playlist_seek: false,
            stats: Stats::default(), last_shown: None, refreshes_on_current: 0, refresh_ms: opts.refresh_ms, current: None, first_landed: false })
    }
    pub fn info(&self) -> &SessionInfo { &self.info }
    pub fn frame_index(&self) -> &Arc<FrameIndex> { &self.index }
    pub fn rate(&self) -> Rate { self.rate }
    pub fn frame_count(&self) -> i32 { self.frames }
    pub fn path(&self) -> &str { &self.path }
    pub fn current(&self) -> Option<&FrameHandle> { self.current.as_ref() }
    /// the first frame has landed (the clock is seated)
    pub fn started(&self) -> bool { self.first_landed }
    pub fn is_playing(&self) -> bool { self.timer.is_playing() }
    /// played through to the end, not looping (QCView: "play stops at the end")
    pub fn at_end(&self) -> bool { !self.loop_enabled && self.timer.duration() > 0.0 && self.timer.position() >= self.timer.duration() && !self.timer.is_playing() }
    pub fn current_frame(&self) -> i32 { self.timer.current_frame().clamp(0, (self.frames - 1).max(0)) }
    pub fn review_speed(&self) -> f64 { self.review_speed }
    pub fn loop_enabled(&self) -> bool { self.loop_enabled }
    pub fn in_point(&self) -> i32 { self.in_point }
    pub fn out_point(&self) -> i32 { self.out_point }
    pub fn has_in_out_range(&self) -> bool { self.in_point >= 0 && self.out_point > self.in_point }
    pub fn is_fast_seeking(&self) -> bool { self.fast.is_some() }
    pub fn modal_active(&self) -> bool { self.modal_active }
    pub fn seek_in_flight(&self) -> bool { self.seek_gate.is_some() }

    // ---- play / pause ---------------------------------------------------------------------------------------------
    pub fn play(&mut self) {
        self.user_wants_playback = true;
        if self.at_end() { self.issue_seek(0); }   // play from the end restarts (the timer clamps at duration)
        self.timer.play(); self.decoder.play();
        if self.audio.has_audio() { self.audio.seek(self.audio_position()); self.audio.set_hold(true); self.audio.play(); }
    }
    pub fn pause(&mut self) {
        self.user_wants_playback = false;
        self.timer.pause(); self.decoder.pause(); self.audio.pause();
    }
    pub fn toggle_playback(&mut self) { if self.timer.is_playing() { self.pause() } else { self.play() } }

    // ---- seeks ----------------------------------------------------------------------------------------------------
    /// The canonical seek. `seconds` is a TIMELINE position.
    pub fn seek_to_time(&mut self, seconds: f64) {
        let seconds = seconds.max(0.0);
        if self.playlist_active { self.pending_playlist_seek = Some((seconds, Instant::now())); return }   // coalesced; `tick` fires it
        if self.fps <= 0.0 { return }
        let target = ((seconds * self.fps).round() as i32).clamp(0, (self.frames - 1).max(0));
        self.issue_seek(target);
    }
    pub fn seek_to_frame(&mut self, frame: i32) { if self.fps > 0.0 { self.seek_to_time(frame as f64 / self.fps) } }
    /// Pauses the decoder NOW so it stops publishing pre-seek frames through the debounce; the commit resumes it.
    pub fn seek_to_frame_and_resume(&mut self, frame: i32, resume: bool) {
        if resume && self.playlist_active && self.decoder.is_playing() { self.decoder.pause(); }
        self.resume_after_playlist_seek = resume;
        self.seek_to_frame(frame);
    }
    pub fn seek_to_start(&mut self) { self.seek_to_frame(0) }
    pub fn seek_to_end(&mut self) { if self.frames > 0 { self.seek_to_frame(self.frames - 1) } }
    /// Pause first — stepping while playing is ambiguous (the wall clock would race the step); clamp to the media.
    pub fn step_frames(&mut self, delta: i32) {
        if delta == 0 { return }
        if self.is_playing() { self.pause(); }
        let target = (self.current_frame() + delta).clamp(0, (self.frames - 1).max(0));
        self.seek_to_frame(target);
    }
    fn issue_seek(&mut self, target: i32) {
        self.seek_gate = Some((target, self.decoder.seeks_performed(), Instant::now()));
        self.timer.seek(target as f64 / self.fps);   // immediate feedback for readouts; the landing re-seats it exactly
        self.decoder.seek_to_frame(target);
        if !self.is_playing() { self.audio.seek(self.audio_position()); }
    }
    fn apply_playlist_seek(&mut self, _pos: f64) {
        // the playlist's clip resolve + slot swap arrives with the timeline step (rows 85 / 157); single-media never gets here
        self.resume_after_playlist_seek = false;
    }

    // ---- the scrub gesture (TimelinePanel: press pauses, moves preview through the scrub decoder, release seeks) ----
    pub fn begin_scrub(&mut self) {
        if self.scrubbing.is_some() { return }
        let was_playing = self.is_playing();
        if was_playing { self.timer.pause(); self.decoder.pause(); self.audio.pause(); }
        self.scrubbing = Some(was_playing);
    }
    pub fn scrub_to_frame(&mut self, frame: i32) {
        let f = frame.clamp(0, (self.frames - 1).max(0));
        if let Some(s) = &mut self.scrub { s.request_frame(f); } else { self.decoder.seek_to_frame(f); }
        if self.fps > 0.0 { self.timer.seek(f as f64 / self.fps); }
        self.seek_gate = None;
    }
    /// The heavy decoder seek lands here; playback resumes if the press interrupted it.
    pub fn end_scrub(&mut self) {
        let Some(was_playing) = self.scrubbing.take() else { return };
        let f = self.current_frame();
        self.issue_seek(f);
        if was_playing { self.play(); }
    }
    pub fn is_scrubbing(&self) -> bool { self.scrubbing.is_some() }

    // ---- review speed ---------------------------------------------------------------------------------------------
    pub fn set_review_speed(&mut self, speed: f64) {
        let speed = speed.clamp(0.25, 4.0);
        if (self.review_speed - speed).abs() < 1e-9 { return }
        self.review_speed = speed;
        self.decoder.set_playback_speed(speed); self.timer.set_playback_speed(speed);
        if self.audio.has_audio() { self.audio.set_tempo(speed); self.audio.seek(self.audio_position()); }
        log::info!("Transport: review speed {speed:.2}x");
    }
    pub fn cycle_review_speed(&mut self) {
        let (mut nearest, mut best) = (0usize, f64::MAX);
        for (i, s) in REVIEW_SPEEDS.iter().enumerate() { let d = (s - self.review_speed).abs(); if d < best { best = d; nearest = i; } }
        self.set_review_speed(REVIEW_SPEEDS[(nearest + 1) % REVIEW_SPEEDS.len()]);
    }

    // ---- loop + in / out ------------------------------------------------------------------------------------------
    pub fn set_loop_enabled(&mut self, on: bool) {
        if self.loop_enabled == on { return }
        self.loop_enabled = on;
        self.timer.set_looping(if self.playlist_active { false } else { on });   // playlist wraps are app-level
        self.push_in_out_to_timer();
    }
    pub fn set_in_point_at_current(&mut self) {
        if self.frames <= 0 { return }
        let new_in = self.current_frame().clamp(0, self.frames - 1);
        if self.out_point >= 0 && new_in >= self.out_point { self.out_point = -1; }   // "I clears O when they'd cross"
        if new_in == self.in_point { return }
        self.in_point = new_in; self.push_in_out_to_timer();
        if self.has_in_out_range() && !self.loop_enabled { self.set_loop_enabled(true); }   // auto-enable once the range becomes valid
    }
    pub fn set_out_point_at_current(&mut self) {
        if self.frames <= 0 { return }
        let new_out = self.current_frame().clamp(0, self.frames - 1);
        if self.in_point >= 0 && new_out <= self.in_point { self.in_point = -1; }
        if new_out == self.out_point { return }
        self.out_point = new_out; self.push_in_out_to_timer();
        if self.has_in_out_range() && !self.loop_enabled { self.set_loop_enabled(true); }
    }
    pub fn clear_in_out_points(&mut self) { if self.in_point < 0 && self.out_point < 0 { return } self.in_point = -1; self.out_point = -1; self.push_in_out_to_timer(); }
    fn push_in_out_to_timer(&mut self) {
        if self.fps <= 0.0 || !self.has_in_out_range() { self.timer.set_loop_range(0.0, 0.0); }
        else { self.timer.set_loop_range(self.in_point as f64 / self.fps, (self.out_point + 1) as f64 / self.fps); }   // out is inclusive: +1 so the wrap fires after it shows
        // network read-ahead: loop on + a range → hydrate the whole range under review; otherwise the playhead window only
        let hydrate = self.loop_enabled && self.has_in_out_range() && !self.playlist_active;
        self.decoder.set_read_ahead_range(if hydrate { self.in_point } else { -1 }, if hydrate { self.out_point } else { -1 });
    }

    // ---- the shuttle (hold-to-fast-seek) ----------------------------------------------------------------------------
    pub fn start_fast_seek(&mut self, direction: i32, now: Instant) {
        if direction == 0 { return }
        let direction = direction.signum();
        if let Some(f) = &self.fast { if f.dir == direction { return } }   // re-press / repeat: no-op; opposite: restart snappy
        if self.review_speed != 1.0 { self.set_review_speed(1.0); }        // shuttle and review speed are mutually exclusive
        if self.is_playing() { self.timer.pause(); self.decoder.pause(); self.audio.pause(); }
        let position_sec = if self.fps > 0.0 { self.current_frame().max(0) as f64 / self.fps } else { 0.0 };
        self.seek_gate = None;   // the gesture drives the clock; no in-flight landing may drag it back
        let signed = direction as f64 * FAST_SEEK_INITIAL_SPEED;
        if self.audio.has_audio() { let p = self.path.clone(); self.audio.begin_shuttle(&p, position_sec, signed); }
        self.fast = Some(FastSeek { dir: direction, speed: FAST_SEEK_INITIAL_SPEED, started: now, last: now, last_tick: now, position_sec });
    }
    fn fast_seek_tick(&mut self, now: Instant) {
        let Some(f) = &mut self.fast else { return };
        let delta = (now - f.last).as_secs_f64(); let elapsed = (now - f.started).as_secs_f64();
        f.last = now; f.last_tick = now;
        f.speed = (FAST_SEEK_INITIAL_SPEED * FAST_SEEK_ACCELERATION.powf(elapsed)).min(FAST_SEEK_MAX_SPEED);   // geometric: doubles every second
        let signed = f.speed * f.dir as f64;
        f.position_sec += delta * signed;
        if self.fps <= 0.0 || self.frames <= 0 { return }
        let max_sec = (self.frames - 1) as f64 / self.fps;
        f.position_sec = f.position_sec.clamp(0.0, max_sec);   // hold at the edge until release
        let target = (f.position_sec * self.fps).round() as i32;
        let pos = f.position_sec;
        // timer-leads-decoder: the clock advances every tick, the decoder chases async
        self.timer.seek(pos);
        self.decoder.seek_to_frame(target);
        if self.audio.shuttle_active() { let p = self.path.clone(); self.audio.shuttle_target(&p, pos, signed); }
    }
    pub fn stop_fast_seek(&mut self) {
        let Some(f) = self.fast.take() else { return };
        if self.audio.shuttle_active() { self.audio.end_shuttle(); }   // before the final commit re-seats the normal pipeline
        if self.fps > 0.0 && self.frames > 0 { let target = ((f.position_sec * self.fps).round() as i32).clamp(0, self.frames - 1); self.seek_to_frame(target); }   // the final commit (idempotent if the last tick targeted it)
    }

    // ---- modals -----------------------------------------------------------------------------------------------------
    /// A modal pauses on the frame it freezes (the backdrop capture is the presenter's); `end_modal` resumes.
    pub fn begin_modal(&mut self) { if self.modal_active { return } self.modal_was_playing = self.is_playing(); if self.modal_was_playing { self.pause(); } self.modal_active = true; }
    pub fn end_modal(&mut self) { if !self.modal_active { return } self.modal_active = false; if self.modal_was_playing { self.modal_was_playing = false; self.play(); } }

    /// The key map's transport actions; returns false for the shell's (background, screenshots, fullscreen, annotations).
    pub fn apply(&mut self, a: Action, now: Instant) -> bool {
        match a {
            Action::ShuttleBegin(d) => self.start_fast_seek(d, now), Action::ShuttleEnd => self.stop_fast_seek(),
            Action::Step(n) => self.step_frames(n), Action::TogglePlayback => self.toggle_playback(),
            Action::SeekToStart => self.seek_to_start(), Action::SeekToEnd => self.seek_to_end(),
            Action::SetInPoint => self.set_in_point_at_current(), Action::SetOutPoint => self.set_out_point_at_current(), Action::ClearInOut => self.clear_in_out_points(),
            Action::ToggleLoop => { let on = !self.loop_enabled; self.set_loop_enabled(on) }
            Action::CycleReviewSpeed => self.cycle_review_speed(), Action::ResetReviewSpeed => self.set_review_speed(1.0),
            Action::ToggleMute => self.audio.toggle_mute(), Action::VolumeUp => self.audio.volume_step(0.05), Action::VolumeDown => self.audio.volume_step(-0.05),
            _ => return false,
        }
        true
    }

    // ---- the refresh ------------------------------------------------------------------------------------------------
    /// One refresh: the shuttle's 33 ms tick, the playlist debounce, the clock, then the frame due now (if a new one).
    pub fn tick(&mut self, now: Instant) -> Option<&FrameHandle> {
        if let Some(f) = &self.fast { if now - f.last_tick >= Duration::from_millis(FAST_SEEK_TICK_MS) { self.fast_seek_tick(now); } }
        if let Some((sec, at)) = self.pending_playlist_seek { if now - at >= Duration::from_millis(PLAYLIST_SEEK_DEBOUNCE_MS) { self.pending_playlist_seek = None; self.apply_playlist_seek(sec); } }
        // a seek in flight: the integrator holds; the sought frame re-seats the clock when it lands
        if let Some((target, seeks_before, issued)) = self.seek_gate {
            let performed = self.decoder.seeks_performed() != seeks_before || !self.first_landed;   // the open's first frame needs no seek
            if !performed { if self.decoder.front_is_external() { let h = self.decoder.fetch_front()?; return self.show(h, now) } return None }   // pre-seek frames: the seek clears them
            let Some(h) = self.decoder.fetch_front() else {
                // the landing was consumed by a scrub publish (the slot replaces): the picture is the same target's; seat the clock on the decoder
                if self.decoder.current_frame() >= target { self.seek_gate = None; self.stats.seek_land_ms.push((now - issued).as_secs_f32() * 1000.0); if self.fps > 0.0 { self.timer.seek(self.decoder.current_frame() as f64 / self.fps); } }
                return None
            };
            if h.frame_no < 0 { return self.show(h, now) }   // a scrub preview: shown, the gate stays
            self.seek_gate = None;   // post-seek, the oldest queued = the landing (at or past the target: keyframe-rounded landings included)
            self.stats.seek_land_ms.push((now - issued).as_secs_f32() * 1000.0);
            if self.fps > 0.0 { self.timer.seek(h.frame_no as f64 / self.fps); }
            if self.audio.has_audio() && self.is_playing() { let p = self.audio_position(); self.audio.release_hold(p); }
            return self.show(h, now);
        }
        if self.fast.is_some() || self.scrubbing.is_some() { let (h, _) = self.decoder.fetch_due(i32::MAX); return self.show(h?, now) }   // the gesture drives the clock; frames land whenever
        let fetch_frame = match self.timer.clock_policy() {
            ClockPolicy::WallClock => {
                self.timer.update(now);
                if self.timer.wrapped { self.on_loop_wrap(); return None }
                if self.timer.is_playing() && !self.timer.looping() && self.timer.duration() > 0.0 && self.timer.position() >= self.timer.duration() { self.timer.pause(); self.decoder.pause(); self.audio.pause(); }   // EndOfStream: play stops at the end
                self.current_frame()
            }
            ClockPolicy::FrameLocked => if self.timer.is_playing() { self.current_frame() + 1 } else { self.current_frame() },
        };
        let (h, dropped) = self.decoder.fetch_due(fetch_frame.min(self.frames - 1));
        self.stats.dropped += dropped;
        let h = h?;
        if self.timer.clock_policy() == ClockPolicy::FrameLocked && self.timer.is_playing() && h.frame_no >= 0 {
            self.timer.advance_by_frame();
            if !self.timer.looping() && self.timer.current_frame() >= self.frames { self.timer.pause(); self.decoder.pause(); }
        }
        if self.audio.has_audio() && self.is_playing() { let p = self.audio_position(); self.audio.update(p); }
        self.show(h, now)
    }
    /// The timer wrapped (full clip or [in, out]): re-seat the decoder at the wrap frame and keep going if the user wants playback.
    fn on_loop_wrap(&mut self) {
        self.stats.loops += 1;
        if self.frames <= 1 { return }   // a 1-frame source: nothing to loop
        let f = self.current_frame();
        self.seek_gate = Some((f, self.decoder.seeks_performed(), Instant::now()));
        self.decoder.seek_to_frame(f);
        if self.user_wants_playback && !self.decoder.is_playing() { self.decoder.play(); }
        self.last_shown = None;
    }
    fn audio_position(&self) -> f64 { self.timer.position() }   // playlist mode translates to the active clip's source time (the timeline step)
    fn show(&mut self, h: FrameHandle, now: Instant) -> Option<&FrameHandle> {
        let k = h.frame_no as i64;
        if k >= 0 {
            if let Some(last) = self.last_shown { if k > last + 1 { self.stats.dropped += (k - last - 1) as u32; } *self.stats.cadence.entry(self.refreshes_on_current).or_insert(0) += 1; }
            if self.timer.is_playing() && self.fps > 0.0 { let late = ((self.timer.position() - k as f64 / self.fps) / self.timer.playback_speed().max(1e-6) * 1000.0) as f32; self.stats.lateness_ms.push(late); if late > self.refresh_ms { self.stats.late += 1; } }
            self.last_shown = Some(k);
        }
        let _ = now;
        self.stats.decode_ms.push(h.decode_ms);
        self.stats.shown += 1; self.refreshes_on_current = 1; self.first_landed = true;
        self.current = Some(h);
        self.current.as_ref()
    }
    /// A refresh that showed nothing new (cadence accounting).
    pub fn idle_refresh(&mut self) { if self.last_shown.is_none() { self.stats.black += 1; } else { self.refreshes_on_current += 1; } }
    pub fn decoder_state(&self) -> State { self.decoder.state() }
}
