//! Dual view's playback (canon row 211, QCView `src/dual/dual_playback_controller` + `dual_playback_timer`): ONE master
//! clock at the higher side's rate drives two SIDES, each our own `Transport` in follower mode (its decoders, scrub decoder,
//! gapless cuts and seek landing unchanged; its clock set by the master). Each side shows `lround(master s × its fps)`;
//! a gap or a side shorter than the master shows nothing (the background). The master's length is the longer side's.
//! Seeks while playing hold the master until both sides land (QCView's 2.5 s cap); steps are master frames; scrub and the
//! shuttle drive both sides' scrub decoders. Where ours differs from QCView (row 211): the loop wraps at out + 1 with the
//! carry (the timer's), loop off stops at the end, image sequences keep their rate, and changing a side keeps the rest.
use crate::{key_map::Action, playback_timer::PlaybackTimer, transport::{AudioHooks, Transport, TransportOptions, FAST_SEEK_ACCELERATION, FAST_SEEK_INITIAL_SPEED, FAST_SEEK_MAX_SPEED, FAST_SEEK_TICK_MS, REVIEW_SPEEDS}, Error};
use qcv_timeline::{ClipMediaKind, Editor, MediaInfo, Rate};
use std::time::{Duration, Instant};

/// QCView `kSeekHoldDeadlineMs`: a seek while playing holds the clock until both sides land, at most this long.
pub const SEEK_HOLD: Duration = Duration::from_millis(2500);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side { A = 0, B = 1 }

/// What a side did this refresh: nothing new, a new frame (`Transport::current`), or nothing to show (a gap / past its end).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SideUpdate { Same, New, Blank }

/// Probe a file into the timeline's media description (the harness; the shell builds these from the project's items).
pub fn media_info(path: &str) -> Result<MediaInfo, Error> {
    let m = qcv_meta::extract(path);
    if !m.loaded { return Err(Error::Open(format!("probe {path}"))) }
    Ok(MediaInfo { id: path.to_string(), name: std::path::Path::new(path).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(), path: path.to_string(), kind: ClipMediaKind::Video, rate: Rate::from_f64(m.frame_rate), duration: m.total_frames as i64, width: m.width as u32, height: m.height as u32, has_audio: m.audio_channels > 0, sequence_pattern: String::new(), sequence_start_frame: 1, sequence_end_frame: 1, sequence_exr_layer: String::new(), sequence_missing: vec![] })
}

struct Shuttle { dir: i32, started: Instant, last: Instant, last_tick: Instant, position_sec: f64 }

pub struct DualTransport {
    pub timer: PlaybackTimer,
    sides: [Option<Transport>; 2],
    media: [Option<MediaInfo>; 2],
    opts: TransportOptions,
    rate: Rate, fps: f64, frames: i32,
    loop_enabled: bool, in_point: i32, out_point: i32, review_speed: f64,
    user_wants_playback: bool,
    /// a seek while playing: (master frame, issued) — the clock holds until both sides land
    gate: Option<(i32, Instant)>,
    scrubbing: Option<bool>, shuttle: Option<Shuttle>,
    /// a trim / slip drag previewing on a side (QCView `requestScrubFrameForSide`): (the side, playing before it)
    edit_scrub: Option<(usize, bool)>,
    /// what each side showed last (a Blank is reported once)
    blank: [bool; 2],
    /// QCView's per-side mutes (`DualAudioMixer`: A heard, B muted by default) and the gains last pushed (a gap silences its side)
    muted: [bool; 2], gains: [f32; 2],
    /// an empty side's audio player, waiting for its file
    spare_audio: [Option<Box<dyn AudioHooks>>; 2],
    /// A LIVE side (canon row 258; QCView `DualLiveSource`): present and unclocked — its `media` is the live item, it has no
    /// `Transport`; the shell owns the feed and shows its newest frame whatever the master does. It takes no part in the
    /// master's rate or length, is never Blank, and seek / scrub / play / step pass it by.
    live: [bool; 2],
}

impl DualTransport {
    /// A and / or B (either may be empty: that side is a drop target). Opens PAUSED at frame 0, like QCView's entry.
    pub fn open(a: Option<MediaInfo>, b: Option<MediaInfo>, opts: TransportOptions) -> Result<DualTransport, Error> {
        let mut timer = PlaybackTimer::new(); timer.set_looping(opts.loop_enabled);
        let mut d = DualTransport { timer, sides: [None, None], media: [None, None], loop_enabled: opts.loop_enabled, opts, rate: Rate::new(24, 1), fps: 24.0, frames: 0, in_point: -1, out_point: -1, review_speed: 1.0, user_wants_playback: false, gate: None, scrubbing: None, shuttle: None, edit_scrub: None, blank: [false; 2], muted: [false, true], live: [false; 2], gains: [-1.0; 2], spare_audio: [None, None] };
        if let Some(m) = a { d.set_side(Side::A, Some(m))?; }
        if let Some(m) = b { d.set_side(Side::B, Some(m))?; }
        Ok(d)
    }
    /// Load (or clear) one side IN PLACE: the playhead (in seconds), in / out and the other side stay (row 211 c).
    pub fn set_side(&mut self, side: Side, m: Option<MediaInfo>) -> Result<(), Error> {
        let i = side as usize;
        let t = match &m {
            Some(m) => {
                let mut e = Editor::default(); e.load_single_media(m);
                let mut t = Transport::open_playlist(e.timeline, self.opts.clone())?;
                t.set_follower(true); t.set_loop_enabled(self.loop_enabled); t.set_review_speed(self.review_speed);
                Some(t)
            }
            None => None,
        };
        let sec = self.timer.position();
        let mut t = t;
        // the side's audio player moves to the new file (it reopens there); an emptied side keeps it for the next one
        let au = match std::mem::replace(&mut self.sides[i], None) { Some(mut old) => { let au = std::mem::replace(&mut old.audio, Box::new(crate::transport::NoAudio)); drop_off_thread(old); Some(au) } None => self.spare_audio[i].take() };
        if let Some(au) = au { match t.as_mut() { Some(t) => t.set_audio(au), None => self.spare_audio[i] = Some(au) } }
        self.sides[i] = t; self.live[i] = false;
        self.media[i] = m; self.blank[i] = false; self.gains[i] = -1.0;
        self.retime(sec);
        let f = self.current_frame();
        if let Some(t) = self.sides[i].as_mut() { let sf = side_frame(f, self.fps, t.rate().as_f64()); t.follow(sf); if t.has_picture_at(sf) { t.seek_to_frame(sf as i32); } if self.user_wants_playback { t.play(); } }
        Ok(())
    }
    /// Make one side LIVE in place (row 258): the item's description, no transport. The playhead, in / out and the other
    /// side stay; the master re-times on the clocked side alone (none → 24 fps, length 0, as QCView).
    pub fn set_live_side(&mut self, side: Side, m: MediaInfo) {
        let i = side as usize;
        let sec = self.timer.position();
        if let Some(mut old) = self.sides[i].take() { let au = std::mem::replace(&mut old.audio, Box::new(crate::transport::NoAudio)); drop_off_thread(old); self.spare_audio[i] = Some(au); }
        self.media[i] = Some(m); self.live[i] = true; self.blank[i] = false; self.gains[i] = -1.0;
        self.retime(sec);
    }
    /// Follow an edited dual timeline (row 212; tracks A / B at the master rate): each side takes its track at its own rate,
    /// the master re-times (the longer side), both sides re-seek to the playhead.
    /// `structural` (a split / delete — the sides' decoder slots are indexed by clip) reopens each side on its new timeline
    /// (A's audio carried); trims / slides / slips re-seat in place.
    pub fn set_timeline(&mut self, tl: &qcv_timeline::Timeline, structural: bool) {
        let sec = self.timer.position();
        for (i, id) in [(0usize, qcv_timeline::TRACK_A), (1, qcv_timeline::TRACK_B)] {
            let Some(t) = self.sides[i].as_mut() else { continue };
            let side = tl.side_timeline(id, t.rate());
            if !structural || side.tracks[0].clips.is_empty() { t.set_timeline(side); continue }
            match Transport::open_playlist(side, self.opts.clone()) {
                Ok(mut n) => {
                    n.set_follower(true); n.set_loop_enabled(self.loop_enabled); n.set_review_speed(self.review_speed);
                    let mut old = self.sides[i].take().unwrap();
                    n.set_range_override(old.range_override());   // the clip's Range choice survives the rebuild
                    let au = std::mem::replace(&mut old.audio, Box::new(crate::transport::NoAudio)); n.set_audio(au); self.gains[i] = -1.0;
                    if self.user_wants_playback { n.play(); }
                    drop_off_thread(old); self.sides[i] = Some(n);
                }
                Err(e) => log::warn!("Dual: side {id} rebuild: {e}"),
            }
        }
        self.retime(sec);
        let f = self.current_frame(); self.seek_to_frame(f);
    }
    /// QCView `masterFps` = the higher clocked side's (24 with none); length = the longer side in seconds × that rate.
    fn retime(&mut self, keep_sec: f64) {
        let rates: Vec<Rate> = self.sides.iter().flatten().map(|t| t.rate()).collect();
        let rate = rates.iter().copied().fold(None, |m: Option<Rate>, r| Some(match m { Some(m) if m.as_f64() >= r.as_f64() => m, _ => r })).unwrap_or(Rate::new(24, 1));
        let dur = self.sides.iter().flatten().map(|t| t.frame_count() as f64 / t.rate().as_f64().max(1e-9)).fold(0.0, f64::max);
        let old_fps = self.fps;
        self.rate = rate; self.fps = rate.as_f64(); self.frames = (dur * self.fps).round() as i32;
        if (old_fps - self.fps).abs() > 1e-9 && old_fps > 0.0 {   // in / out keep their times
            let k = self.fps / old_fps; let r = |f: i32| if f < 0 { f } else { (f as f64 * k).round() as i32 };
            self.in_point = r(self.in_point); self.out_point = r(self.out_point);
        }
        if self.in_point >= self.frames { self.in_point = -1; } if self.out_point >= self.frames { self.out_point = -1; }
        self.timer.set_frame_rate(self.fps);
        self.timer.set_duration(if self.fps > 0.0 { self.frames as f64 / self.fps } else { 0.0 });
        self.push_in_out();
        self.timer.seek(keep_sec.min(((self.frames - 1).max(0)) as f64 / self.fps));
    }

    /// Drop both sides with their decoders joined on threads (leaving dual view).
    pub fn retire(mut self) { for s in self.sides.iter_mut() { if let Some(t) = s.take() { drop_off_thread(t); } } }

    // ---- reads ------------------------------------------------------------------------------------------------------
    pub fn side(&self, s: Side) -> Option<&Transport> { self.sides[s as usize].as_ref() }
    pub fn side_mut(&mut self, s: Side) -> Option<&mut Transport> { self.sides[s as usize].as_mut() }
    pub fn media(&self, s: Side) -> Option<&MediaInfo> { self.media[s as usize].as_ref() }
    /// The side is a live source (present, unclocked).
    pub fn is_live(&self, s: Side) -> bool { self.live[s as usize] }
    /// The side shows something: a clocked transport or a live source ("is there a B?" for the outputs, the chips, colour).
    pub fn present(&self, s: Side) -> bool { self.sides[s as usize].is_some() || self.live[s as usize] }
    /// A side with a clock (QCView `hasClockedSide`): without one there is nothing for the transport bar to drive.
    pub fn has_clocked_side(&self) -> bool { self.sides.iter().any(|t| t.is_some()) }
    pub fn rate(&self) -> Rate { self.rate }
    pub fn frame_count(&self) -> i32 { self.frames }
    /// QCView `DualPlaybackTimer::currentFrame`: FLOOR of the position (the single timer rounds) — a frame shows for its whole
    /// duration and the out frame holds until the wrap at out + 1.
    pub fn current_frame(&self) -> i32 { ((self.timer.position() * self.fps + 1e-6).floor() as i32).clamp(0, (self.frames - 1).max(0)) }
    pub fn is_playing(&self) -> bool { self.timer.is_playing() }
    pub fn loop_enabled(&self) -> bool { self.loop_enabled }
    pub fn in_point(&self) -> i32 { self.in_point }
    pub fn out_point(&self) -> i32 { self.out_point }
    pub fn has_in_out_range(&self) -> bool { self.in_point >= 0 && self.out_point > self.in_point }
    pub fn review_speed(&self) -> f64 { self.review_speed }
    pub fn is_scrubbing(&self) -> bool { self.scrubbing.is_some() }
    pub fn is_fast_seeking(&self) -> bool { self.shuttle.is_some() }
    /// a seek is landing (the master holds, or a side is still opening / seeking)
    pub fn seek_in_flight(&self) -> bool { self.gate.is_some() || self.sides.iter().flatten().any(|t| t.seek_in_flight()) }
    pub fn at_end(&self) -> bool { !self.loop_enabled && self.frames > 0 && self.timer.position() >= self.timer.duration() && !self.timer.is_playing() }
    /// The side's frame for a master frame (QCView `translateFrame`: lround(master s × the side's fps)).
    pub fn side_frame_of(&self, s: Side, master: i32) -> Option<i64> { self.sides[s as usize].as_ref().map(|t| side_frame(master, self.fps, t.rate().as_f64())) }

    // ---- play / pause / seek / step -----------------------------------------------------------------------------------
    pub fn play(&mut self) {
        if self.frames <= 0 { return }
        self.user_wants_playback = true;
        if self.at_end() || (!self.loop_enabled && self.current_frame() >= self.frames - 1) { self.seek_to_frame(0); }   // play from the end restarts
        self.timer.play();
        for t in self.sides.iter_mut().flatten() { t.play(); }
    }
    pub fn pause(&mut self) {
        self.user_wants_playback = false;
        self.timer.pause(); self.gate = None;
        for t in self.sides.iter_mut().flatten() { t.pause(); }
    }
    pub fn toggle_playback(&mut self) { if self.timer.is_playing() { self.pause() } else { self.play() } }
    /// QCView `seekToFrame`: clamp, the clock, each side to its frame (a gap → the side's own next-clip prewarm); while
    /// playing the clock holds until both land.
    pub fn seek_to_frame(&mut self, frame: i32) {
        if self.fps <= 0.0 { return }
        let f = frame.clamp(0, (self.frames - 1).max(0));
        self.timer.seek(f as f64 / self.fps);
        let fps = self.fps;
        let playing = self.timer.is_playing();
        // a playing side's decoder resumes after the seek: one that hit its end (the wrap, a seek back from past it) has stopped
        // (the single view's wrap restarts it the same way)
        for t in self.sides.iter_mut().flatten() { let sf = side_frame(f, fps, t.rate().as_f64()); t.follow(sf); t.seek_to_frame_and_resume(sf.clamp(0, (t.frame_count() - 1).max(0) as i64) as i32, playing); }
        if self.timer.is_playing() { self.gate = Some((f, Instant::now())); }
    }
    pub fn seek_to_start(&mut self) { self.seek_to_frame(0) }
    pub fn seek_to_end(&mut self) { if self.frames > 0 { self.seek_to_frame(self.frames - 1) } }
    /// Master frames (QCView: `currentFrame + delta` at master fps; the lower-rate side may repeat).
    pub fn step_frames(&mut self, delta: i32) {
        if delta == 0 { return }
        if self.is_playing() { self.pause(); }
        let f = self.current_frame() + delta; self.seek_to_frame(f);
    }

    // ---- scrub (QCView: the press parks both sides, a move drives the clock + each side's scrub decoder, release seeks) ----
    pub fn begin_scrub(&mut self) {
        if self.scrubbing.is_some() { return }
        let was = self.is_playing();
        if was { self.timer.pause(); }
        self.gate = None;
        for t in self.sides.iter_mut().flatten() { t.begin_scrub(); }
        self.scrubbing = Some(was);
    }
    pub fn scrub_to_frame(&mut self, frame: i32) {
        if self.fps <= 0.0 { return }
        let f = frame.clamp(0, (self.frames - 1).max(0));   // QCView left this unclamped (row 211's notes)
        self.timer.seek(f as f64 / self.fps);
        let fps = self.fps;
        for t in self.sides.iter_mut().flatten() { let sf = side_frame(f, fps, t.rate().as_f64()); t.follow(sf); if t.has_picture_at(sf) { t.scrub_to_frame(sf as i32); } }
    }
    pub fn end_scrub(&mut self) {
        let Some(was) = self.scrubbing.take() else { return };
        for t in self.sides.iter_mut().flatten() { t.end_scrub(); }
        let f = self.current_frame(); self.seek_to_frame(f);
        if was { self.play(); self.gate = Some((f, Instant::now())); }
    }

    // ---- the edit preview (QCView `requestScrubFrameForSide` / `endEditScrub`) ---------------------------------------------
    /// While a trim / slip drags on `side`: show SOURCE frame `src` of `clip` there (the playhead stays; playback pauses for the
    /// gesture). False when that side cannot serve it (another file).
    pub fn edit_preview(&mut self, side: Side, clip: &str, src: i64) -> bool {
        let i = side as usize;
        if self.edit_scrub.is_none() { let was = self.is_playing(); if was { self.pause(); } self.edit_scrub = Some((i, was)); }
        self.sides[i].as_mut().is_some_and(|t| t.edit_preview(clip, src))
    }
    /// The gesture ended: both sides back on the playhead's frame; playback resumes if the drag interrupted it.
    pub fn end_edit_preview(&mut self) {
        let Some((i, was)) = self.edit_scrub.take() else { return };
        if let Some(t) = self.sides[i].as_mut() { t.end_edit_preview(); }
        let f = self.current_frame(); self.seek_to_frame(f);
        if was { self.play(); }
    }
    pub fn edit_previewing(&self) -> bool { self.edit_scrub.is_some() }

    // ---- review speed, loop, in / out -----------------------------------------------------------------------------------
    pub fn set_review_speed(&mut self, speed: f64) {
        let speed = speed.clamp(0.25, 4.0);
        if (self.review_speed - speed).abs() < 1e-9 { return }
        self.review_speed = speed; self.timer.set_playback_speed(speed);
        for t in self.sides.iter_mut().flatten() { t.set_review_speed(speed); }
        let f = self.current_frame(); self.seek_to_frame(f);   // QCView: a same-frame seek re-seats both sides
    }
    pub fn cycle_review_speed(&mut self) {
        let (mut nearest, mut best) = (0usize, f64::MAX);
        for (i, s) in REVIEW_SPEEDS.iter().enumerate() { let d = (s - self.review_speed).abs(); if d < best { best = d; nearest = i; } }
        self.set_review_speed(REVIEW_SPEEDS[(nearest + 1) % REVIEW_SPEEDS.len()]);
    }
    pub fn set_loop_enabled(&mut self, on: bool) {
        if self.loop_enabled == on { return }
        self.loop_enabled = on; self.timer.set_looping(on); self.push_in_out();
        for t in self.sides.iter_mut().flatten() { t.set_loop_enabled(on); }   // their prewarm of the first clip at the end
    }
    pub fn set_in_point_at_current(&mut self) {
        if self.frames <= 0 { return }
        let n = self.current_frame();
        if self.out_point >= 0 && n >= self.out_point { self.out_point = -1; }
        if n == self.in_point { return }
        self.in_point = n; self.push_in_out();
        if self.has_in_out_range() && !self.loop_enabled { self.set_loop_enabled(true); }
    }
    pub fn set_out_point_at_current(&mut self) {
        if self.frames <= 0 { return }
        let n = self.current_frame();
        if self.in_point >= 0 && n <= self.in_point { self.in_point = -1; }
        if n == self.out_point { return }
        self.out_point = n; self.push_in_out();
        if self.has_in_out_range() && !self.loop_enabled { self.set_loop_enabled(true); }
    }
    pub fn clear_in_out_points(&mut self) { if self.in_point < 0 && self.out_point < 0 { return } self.in_point = -1; self.out_point = -1; self.push_in_out(); }
    /// Restore saved in / out (a Dual View item).
    pub fn set_in_out(&mut self, i: i32, o: i32) { self.in_point = if i >= 0 && i < self.frames { i } else { -1 }; self.out_point = if o >= 0 && o < self.frames { o } else { -1 }; self.push_in_out(); }
    /// out is inclusive: the wrap fires after it shows (QCView's dual timer wrapped AT out, skipping it — row 211 b)
    fn push_in_out(&mut self) {
        if self.fps <= 0.0 || !self.has_in_out_range() { self.timer.set_loop_range(0.0, 0.0); }
        else { self.timer.set_loop_range(self.in_point as f64 / self.fps, (self.out_point + 1) as f64 / self.fps); }
    }

    // ---- the shuttle (QCView: hold J / L — both sides through their scrub decoders, the clock integrates) -----------------
    pub fn start_fast_seek(&mut self, direction: i32, now: Instant) {
        if direction == 0 { return }
        let dir = direction.signum();
        if let Some(s) = &self.shuttle { if s.dir == dir { return } }
        if self.review_speed != 1.0 { self.set_review_speed(1.0); }
        let was = self.scrubbing.is_some();
        if !was { self.begin_scrub(); self.scrubbing = Some(false); }   // the shuttle never resumes playback on release
        let position_sec = self.current_frame() as f64 / self.fps.max(1e-9);
        self.shuttle = Some(Shuttle { dir, started: now, last: now, last_tick: now, position_sec });
    }
    fn shuttle_tick(&mut self, now: Instant) {
        let Some(s) = &mut self.shuttle else { return };
        let dt = (now - s.last).as_secs_f64(); let el = (now - s.started).as_secs_f64();
        s.last = now; s.last_tick = now;
        let speed = (FAST_SEEK_INITIAL_SPEED * FAST_SEEK_ACCELERATION.powf(el)).min(FAST_SEEK_MAX_SPEED);
        if self.fps <= 0.0 || self.frames <= 0 { return }
        s.position_sec = (s.position_sec + dt * speed * s.dir as f64).clamp(0.0, (self.frames - 1) as f64 / self.fps);
        let f = (s.position_sec * self.fps).round() as i32;
        self.scrub_to_frame(f);
    }
    pub fn stop_fast_seek(&mut self) { if self.shuttle.take().is_some() { self.end_scrub(); } }

    /// The key map's transport actions (false = the shell's).
    pub fn apply(&mut self, a: Action, now: Instant) -> bool {
        match a {
            Action::ShuttleBegin(d) => self.start_fast_seek(d, now), Action::ShuttleEnd => self.stop_fast_seek(),
            Action::Step(n) => self.step_frames(n), Action::TogglePlayback => self.toggle_playback(),
            Action::SeekToStart => self.seek_to_start(), Action::SeekToEnd => self.seek_to_end(),
            Action::SetInPoint => self.set_in_point_at_current(), Action::SetOutPoint => self.set_out_point_at_current(), Action::ClearInOut => self.clear_in_out_points(),
            Action::ToggleLoop => { let on = !self.loop_enabled; self.set_loop_enabled(on) }
            Action::CycleReviewSpeed => self.cycle_review_speed(), Action::ResetReviewSpeed => self.set_review_speed(1.0),
            Action::ToggleMute => { let m = !self.sides.iter().flatten().any(|t| t.audio.is_muted()); self.for_each_audio(|au| au.set_muted(m)); }
            Action::VolumeUp | Action::VolumeDown => { let d = if a == Action::VolumeUp { 0.05 } else { -0.05 }; self.for_each_audio(|au| au.volume_step(d)); }
            _ => return false,
        }
        true
    }
    /// A's audio player.
    pub fn set_audio(&mut self, audio: Box<dyn AudioHooks>) { self.set_side_audio(Side::A, audio) }
    /// A side's audio player (QCView's `DualAudioMixer` decodes both; here each side's Transport drives its own, both following
    /// the master clock through their drift loops, the OS mixing the two streams at unity).
    pub fn set_side_audio(&mut self, side: Side, audio: Box<dyn AudioHooks>) { let i = side as usize; match self.sides[i].as_mut() { Some(t) => { t.set_audio(audio); self.gains[i] = -1.0; } None => self.spare_audio[i] = Some(audio) } }
    /// QCView `audio.mutedA` / `mutedB` (the chips' speaker pills).
    pub fn set_side_muted(&mut self, side: Side, m: bool) { self.muted[side as usize] = m; self.gains[side as usize] = -1.0; }
    pub fn side_muted(&self, side: Side) -> bool { self.muted[side as usize] }
    pub fn side_has_audio(&self, side: Side) -> bool { self.sides[side as usize].as_ref().is_some_and(|t| t.audio.has_audio()) }
    /// The master volume / mute and the dual A/V offset reach both sides (QCView mirrors the single view's into the mixer).
    pub fn for_each_audio(&mut self, mut f: impl FnMut(&mut dyn AudioHooks)) { for t in self.sides.iter_mut().flatten() { f(&mut *t.audio); } for a in self.spare_audio.iter_mut().flatten() { f(&mut **a); } }

    // ---- the refresh ----------------------------------------------------------------------------------------------------
    /// One refresh: the shuttle tick, the master clock (held while a seek lands; the wrap re-seats both sides; loop off stops at
    /// the end), then each side follows and ticks. Returns what each side did.
    pub fn tick(&mut self, now: Instant) -> [SideUpdate; 2] {
        if let Some(s) = &self.shuttle { if now - s.last_tick >= Duration::from_millis(FAST_SEEK_TICK_MS) { self.shuttle_tick(now); } }
        if let Some((f, at)) = self.gate {
            let landed = self.sides.iter().flatten().all(|t| !t.seek_in_flight());
            if landed || now - at >= SEEK_HOLD { self.gate = None; if !landed { log::warn!("Dual: a side missed the {} ms seek hold", SEEK_HOLD.as_millis()); } }
            self.timer.seek(f as f64 / self.fps);   // re-based: the clock runs from the landing, no stale delta
        } else if self.scrubbing.is_none() {
            self.timer.update(now);
            if self.timer.wrapped { let f = self.current_frame(); self.seek_to_frame(f); }
            else if !self.loop_enabled && self.timer.is_playing() && self.frames > 0 && self.timer.position() >= self.timer.duration() { self.pause(); self.timer.seek(((self.frames - 1) as f64) / self.fps); }   // row 211 b: loop off stops at the end
        }
        let f = self.current_frame(); let fps = self.fps;
        let mut out = [SideUpdate::Same; 2];
        for i in 0..2 {
            if self.live[i] { continue }   // a live side is the shell's: never Blank, never clocked
            let Some(t) = self.sides[i].as_mut() else { if !self.blank[i] { self.blank[i] = true; out[i] = SideUpdate::Blank; } continue };
            let sf = side_frame(f, fps, t.rate().as_f64());
            t.follow(sf);
            let new = t.tick(now).is_some();
            // a side previewing an edit shows its preview frames wherever the playhead is
            // the side's audio: its mute, or silence where it has no picture (a gap, past its end — QCView mutes a side in a gap)
            let g = if self.muted[i] || !t.has_picture_at(sf) { 0.0 } else { 1.0 };
            if g != self.gains[i] { t.audio.set_side_gain(g); self.gains[i] = g; }
            if self.edit_scrub.is_some_and(|(e, _)| e == i) { if new { self.blank[i] = false; out[i] = SideUpdate::New; } continue }
            if !t.has_picture_at(sf) { if !self.blank[i] { self.blank[i] = true; out[i] = SideUpdate::Blank; } continue }
            if new || (self.blank[i] && t.current().is_some()) { self.blank[i] = false; out[i] = SideUpdate::New; }
        }
        out
    }
}

/// QCView `translateFrame`: master frame → seconds → the side's frame, rounded to nearest.
fn side_frame(master: i32, master_fps: f64, side_fps: f64) -> i64 { if master_fps <= 0.0 || side_fps <= 0.0 { return -1 } ((master as f64 / master_fps) * side_fps).round() as i64 }
fn drop_off_thread(t: Transport) { t.retire() }

#[cfg(test)]
mod tests {
    use super::*;
    fn clip(name: &str, rate: &str, secs: f32) -> Option<String> {
        let p = std::env::temp_dir().join(format!("qcv-dual-{}-{name}.mov", std::process::id()));
        let ok = std::process::Command::new("ffmpeg").args(["-nostdin", "-hide_banner", "-loglevel", "error", "-y", "-f", "lavfi", "-i", &format!("testsrc2=size=160x90:rate={rate}"), "-t", &secs.to_string(), "-c:v", "mjpeg"]).arg(&p).status().map(|s| s.success()).unwrap_or(false);
        ok.then(|| p.to_string_lossy().into_owned())
    }
    fn settle(d: &mut DualTransport) { for _ in 0..400 { d.tick(Instant::now()); if !d.seek_in_flight() { for _ in 0..5 { d.tick(Instant::now()); std::thread::sleep(Duration::from_millis(5)); } return } std::thread::sleep(Duration::from_millis(5)); } }
    #[test]
    fn a_live_side_is_present_unclocked_and_never_blank() {
        // canon row 258 (QCView `DualLiveSource`): the master's rate and length come from the clocked side alone; a live side
        // is never Blank and seeks pass it by; live + live (or live + empty) has no clock at all
        let Some(a) = clip("live-a", "24", 1.0) else { eprintln!("ffmpeg missing: skipped"); return };
        let live = |id: &str| MediaInfo { id: id.into(), name: id.into(), path: format!("qcbae://{id}"), kind: ClipMediaKind::Video, rate: Rate::new(0, 1), duration: 0, width: 1920, height: 1080, ..media_info(&a).unwrap() };
        let opts = TransportOptions { scrub: None, ..Default::default() };
        let mut d = DualTransport::open(Some(media_info(&a).unwrap()), None, opts.clone()).unwrap();
        d.set_live_side(Side::B, live("ae"));
        assert!(d.is_live(Side::B) && d.present(Side::B) && d.side(Side::B).is_none() && d.media(Side::B).unwrap().id == "ae");
        assert!(d.has_clocked_side()); assert_eq!((d.rate().as_f64().round() as i32, d.frame_count()), (24, 24), "the file side is the clock");
        settle(&mut d);
        d.seek_to_frame(12); settle(&mut d); d.play();
        for _ in 0..20 { let u = d.tick(Instant::now()); assert_eq!(u[1], SideUpdate::Same, "a live side reports nothing: the shell shows its feed"); std::thread::sleep(Duration::from_millis(5)); }
        d.pause(); assert_eq!(d.side_frame_of(Side::B, 12), None);
        // the file leaves: live + empty has no clock (24 fps, length 0) and play does nothing
        d.set_side(Side::A, None).unwrap();
        assert!(!d.has_clocked_side() && d.present(Side::B) && !d.present(Side::A)); assert_eq!(d.frame_count(), 0);
        d.play(); assert!(!d.is_playing());
        // a file put on the live side makes it an ordinary side again
        d.set_side(Side::B, Some(media_info(&a).unwrap())).unwrap();
        assert!(!d.is_live(Side::B) && d.side(Side::B).is_some() && d.frame_count() == 24);
        let _ = std::fs::remove_file(a);
    }
    #[test]
    fn two_rates_share_one_clock_and_the_shorter_side_blanks() {
        let (Some(a), Some(b)) = (clip("a", "24", 2.0), clip("b", "30", 1.0)) else { eprintln!("ffmpeg missing: skipped"); return };
        let opts = TransportOptions { scrub: None, ..Default::default() };
        let mut d = DualTransport::open(Some(media_info(&a).unwrap()), Some(media_info(&b).unwrap()), opts).unwrap();
        // the master: the higher rate (30), the longer side (2 s → 60 frames)
        assert_eq!(d.rate().as_f64().round() as i32, 30); assert_eq!(d.frame_count(), 60);
        settle(&mut d);
        assert!(d.side(Side::A).unwrap().current().is_some() && d.side(Side::B).unwrap().current().is_some(), "both sides show frame 0");
        // master frame 15 = 0.5 s → A (24 fps) frame 12, B (30 fps) frame 15
        d.seek_to_frame(15); settle(&mut d);
        assert_eq!(d.side_frame_of(Side::A, 15), Some(12)); assert_eq!(d.side(Side::A).unwrap().current().unwrap().frame_no, 12);
        assert_eq!(d.side(Side::B).unwrap().current().unwrap().frame_no, 15);
        // past B's end (1 s = master 30): B blanks, A keeps going
        d.seek_to_frame(45);
        let mut saw_blank = false; for _ in 0..200 { let u = d.tick(Instant::now()); if u[1] == SideUpdate::Blank { saw_blank = true; } if saw_blank && !d.seek_in_flight() { break } std::thread::sleep(Duration::from_millis(5)); }
        assert!(saw_blank, "B is past its end at master 45");
        settle(&mut d);
        assert_eq!(d.side(Side::A).unwrap().current().unwrap().frame_no, 36, "A at 1.5 s");
        // steps are master frames
        d.step_frames(1); settle(&mut d); assert_eq!(d.current_frame(), 46);
        // changing B in place keeps the playhead and in / out
        d.set_in_point_at_current(); d.seek_to_frame(50); d.set_out_point_at_current();
        d.set_side(Side::B, Some(media_info(&a).unwrap())).unwrap();
        assert_eq!(d.rate().as_f64().round() as i32, 24, "both sides 24 now");
        assert_eq!(d.current_frame(), 40, "the playhead keeps its time (50 @ 30 → 40 @ 24)");
        assert_eq!((d.in_point(), d.out_point()), (37, 40), "in / out keep their times");
        for p in [a, b] { let _ = std::fs::remove_file(p); }
    }
    #[test]
    fn an_edited_timeline_moves_what_a_side_shows() {
        let (Some(a), Some(b)) = (clip("ea", "24", 2.0), clip("eb", "24", 2.0)) else { eprintln!("ffmpeg missing: skipped"); return };
        let (ma, mb) = (media_info(&a).unwrap(), media_info(&b).unwrap());
        let mut d = DualTransport::open(Some(ma.clone()), Some(mb.clone()), TransportOptions { scrub: None, ..Default::default() }).unwrap();
        let mut ed = qcv_timeline::Editor::default(); ed.load_dual(Some(&ma), Some(&mb), d.rate());
        // ⌘K on B at 12, the second half slid +6 (carrying): at master 20, B shows its frame 14; A its 20
        assert!(ed.split_clip_at(qcv_timeline::TRACK_B, 12));
        let id = ed.timeline.track(qcv_timeline::TRACK_B).unwrap().clips[1].id.clone();
        assert_eq!(ed.shift_clips(qcv_timeline::TRACK_B, &id, 6, true), 6);
        d.set_timeline(&ed.timeline, true);
        assert_eq!(d.frame_count(), 54, "the master grew to B's new end");
        d.seek_to_frame(20); settle(&mut d);
        assert_eq!(d.side(Side::A).unwrap().current().unwrap().frame_no, 20);
        assert_eq!(d.side(Side::B).unwrap().current().unwrap().frame_no, 14);
        // in the 6-frame hole B blanks
        d.seek_to_frame(15); let mut blank = false; for _ in 0..100 { if d.tick(Instant::now())[1] == SideUpdate::Blank { blank = true; } std::thread::sleep(Duration::from_millis(3)); }
        assert!(blank, "B is in its gap at master 15");
        for p in [a, b] { let _ = std::fs::remove_file(p); }
    }
    #[test]
    fn a_trim_drag_previews_on_its_side_and_the_playhead_stays() {
        let (Some(a), Some(b)) = (clip("pa", "24", 2.0), clip("pb", "24", 2.0)) else { eprintln!("ffmpeg missing: skipped"); return };
        let mut d = DualTransport::open(Some(media_info(&a).unwrap()), Some(media_info(&b).unwrap()), TransportOptions::default()).unwrap();
        d.seek_to_frame(10); settle(&mut d);
        let id = d.side(Side::B).unwrap().timeline.tracks[0].clips[0].id.clone();
        assert!(d.edit_preview(Side::B, &id, 30), "B's scrub decoder serves its own file");
        let mut previewed = false; for _ in 0..200 { let u = d.tick(Instant::now()); if u[1] == SideUpdate::New && d.side(Side::B).unwrap().current().is_some_and(|h| h.frame_no < 0) { previewed = true; break } std::thread::sleep(Duration::from_millis(3)); }
        assert!(previewed, "a scrub preview frame reached B");
        assert_eq!(d.current_frame(), 10, "the playhead never moved");
        d.end_edit_preview(); settle(&mut d);
        assert_eq!(d.side(Side::B).unwrap().current().unwrap().frame_no, 10, "B is back on the playhead's frame");
        for p in [a, b] { let _ = std::fs::remove_file(p); }
    }
    #[test]
    fn b_starts_muted_and_a_side_is_silent_in_its_gap() {
        use std::sync::{Arc, Mutex};
        struct Rec(Arc<Mutex<Vec<f32>>>);
        impl AudioHooks for Rec { fn has_audio(&self) -> bool { true } fn open(&mut self, _p: &str) -> bool { true } fn set_side_gain(&mut self, g: f32) { self.0.lock().unwrap().push(g); } }
        let (Some(a), Some(b)) = (clip("ga", "24", 2.0), clip("gb", "24", 1.0)) else { eprintln!("ffmpeg missing: skipped"); return };
        let mut d = DualTransport::open(Some(media_info(&a).unwrap()), Some(media_info(&b).unwrap()), TransportOptions { scrub: None, ..Default::default() }).unwrap();
        let (ga, gb) = (Arc::new(Mutex::new(vec![])), Arc::new(Mutex::new(vec![])));
        d.set_side_audio(Side::A, Box::new(Rec(ga.clone()))); d.set_side_audio(Side::B, Box::new(Rec(gb.clone())));
        settle(&mut d);
        assert_eq!(ga.lock().unwrap().last(), Some(&1.0), "A heard"); assert_eq!(gb.lock().unwrap().last(), Some(&0.0), "B muted by default (QCView)");
        d.set_side_muted(Side::B, false); d.tick(Instant::now());
        assert_eq!(gb.lock().unwrap().last(), Some(&1.0));
        d.seek_to_frame(36); settle(&mut d);   // past B's 1 s
        assert_eq!(gb.lock().unwrap().last(), Some(&0.0), "B silent past its end"); assert_eq!(ga.lock().unwrap().last(), Some(&1.0));
        for p in [a, b] { let _ = std::fs::remove_file(p); }
    }
    #[test]
    fn loop_off_stops_at_the_end_and_loop_on_shows_the_out_frame() {
        let Some(a) = clip("loop", "24", 1.0) else { eprintln!("ffmpeg missing: skipped"); return };
        let opts = TransportOptions { scrub: None, loop_enabled: false, ..Default::default() };
        let mut d = DualTransport::open(Some(media_info(&a).unwrap()), None, opts).unwrap();
        settle(&mut d);
        d.seek_to_frame(20); settle(&mut d); d.play();
        let t0 = Instant::now(); while t0.elapsed() < Duration::from_millis(600) { d.tick(Instant::now()); std::thread::sleep(Duration::from_millis(4)); }
        assert!(!d.is_playing(), "loop off: playback stops at the end"); assert_eq!(d.current_frame(), 23);
        // loop on with in 4 / out 8: frame 8 shows before the wrap to 4
        d.set_loop_enabled(true); d.seek_to_frame(4); d.set_in_point_at_current(); d.seek_to_frame(8); d.set_out_point_at_current(); d.seek_to_frame(4); settle(&mut d);
        d.play();
        let mut seen = std::collections::BTreeSet::new(); let mut wrapped = false; let mut last = 4;
        let t0 = Instant::now(); while t0.elapsed() < Duration::from_millis(700) { d.tick(Instant::now()); let f = d.current_frame(); if f < last { wrapped = true; } last = f; seen.insert(f); std::thread::sleep(Duration::from_millis(4)); }
        assert!(wrapped, "the loop wrapped"); assert!(seen.contains(&8), "the out frame shows: {seen:?}"); assert!(seen.iter().all(|&f| (4..=8).contains(&f)), "{seen:?}");
        d.pause();
        // the wrap at the END of the file: the decoder that reached EOF restarts (it froze on frame 0 before 2026-11-08)
        d.clear_in_out_points(); d.seek_to_frame(20); settle(&mut d); d.play();
        let t0 = Instant::now(); while t0.elapsed() < Duration::from_millis(500) { d.tick(Instant::now()); std::thread::sleep(Duration::from_millis(4)); }
        let shown = d.side(Side::A).unwrap().current().unwrap().frame_no; let master = d.current_frame();
        assert!(master >= 5 && master <= 12, "the master wrapped and runs: {master}");
        assert!((shown as i32 - master).abs() <= 2, "A follows past the wrap: shows {shown} at master {master}");
        d.pause();
        let _ = std::fs::remove_file(a);
    }
}

#[cfg(test)]
mod trace {
    use super::*;
    /// `QCV_DUAL_A=… QCV_DUAL_B=… cargo test -p qcv-player --lib dual_trace -- --ignored --nocapture`
    #[test] #[ignore]
    fn dual_trace() {
        let (Ok(a), Ok(b)) = (std::env::var("QCV_DUAL_A"), std::env::var("QCV_DUAL_B")) else { return };
        let mut d = DualTransport::open(Some(media_info(&a).unwrap()), Some(media_info(&b).unwrap()), TransportOptions::default()).unwrap();
        let t0 = Instant::now();
        let mut log = |d: &mut DualTransport, tag: &str| { let u = d.tick(Instant::now()); let fa = d.side(Side::A).and_then(|t| t.current()).map(|h| h.frame_no); let fb = d.side(Side::B).and_then(|t| t.current()).map(|h| h.frame_no);
            println!("{:6.0} ms {tag} master {:4} playing {} gate {} | A {:?} {:?} inflight {} | B {:?} {:?} inflight {}", t0.elapsed().as_secs_f64() * 1000.0, d.current_frame(), d.is_playing(), d.gate.is_some(), u[0], fa, d.side(Side::A).unwrap().seek_in_flight(), u[1], fb, d.side(Side::B).unwrap().seek_in_flight()); };
        for _ in 0..30 { log(&mut d, "open"); std::thread::sleep(Duration::from_millis(16)); }
        d.play();
        for _ in 0..120 { log(&mut d, "play"); std::thread::sleep(Duration::from_millis(16)); }
        d.seek_to_frame(100);
        for _ in 0..60 { log(&mut d, "seek"); std::thread::sleep(Duration::from_millis(16)); }
        d.seek_to_frame(250);
        for _ in 0..60 { log(&mut d, "seek past B"); std::thread::sleep(Duration::from_millis(16)); }
        d.pause(); d.seek_to_frame(200); for _ in 0..20 { log(&mut d, "paused seek"); std::thread::sleep(Duration::from_millis(16)); }
        d.begin_scrub(); for f in (100..200).step_by(5) { d.scrub_to_frame(f); log(&mut d, "scrub"); std::thread::sleep(Duration::from_millis(16)); } d.end_scrub();
        for _ in 0..30 { log(&mut d, "after scrub"); std::thread::sleep(Duration::from_millis(16)); }
        d.step_frames(1); for _ in 0..10 { log(&mut d, "step"); std::thread::sleep(Duration::from_millis(16)); }
        d.seek_to_frame(280); d.play(); for _ in 0..90 { log(&mut d, "wrap"); std::thread::sleep(Duration::from_millis(16)); }
    }
}
