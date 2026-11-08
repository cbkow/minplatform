//! `Transport` — ported from `WindowManager`'s transport slice (`QCView-Player/src/window/window_manager.{h,cpp}`: play /
//! pause / togglePlayback with `m_userWantsPlayback`, seekToTime / seekToFrame (+AndResume) with the 50 ms playlist
//! debounce and `applyPlaylistSeek`'s clip resolve, stepFrames, seekToStart / End, the review speeds, loop + in / out
//! with `pushInOutToTimer`, the fast-seek shuttle (`startFastSeek` / `stopFastSeek` and its 33 ms tick), begin / endModal,
//! the end-of-stream loop wrap, the in / out snap, `playlistAdvanceToClip`'s next-non-gap / loop-to-first rules) plus
//! `TimelinePanel.qml`'s scrub gesture (pause, preview through the scrub decoder, the heavy seek on release). Canon §8.5:
//! ONE object owns the decoders, the scrub decoders, the audio hooks, the shuttle and the clock; the UI binds to it,
//! never to its parts; the constants (row 88) live here once. Row 84's "ONE Sequencer": the Transport dispatches the
//! clock's timeline frame to the clip's decoder slot through `qcv-timeline`'s resolve.
//!
//! Every source is a timeline: single media is a one-clip locked timeline (`Editor::load_single_media`), a playlist is
//! `load_playlist`'s. Two decoder SLOTS (row 85, S2's gapless pair): the active clip's, and the next non-gap clip's
//! prewarmed on its in-point (opened on a thread so the tick never stalls); at the cut the prewarmed slot becomes active
//! and its in-point frame is already due — no black, no duplicate. QCView reopens its one decoder at every clip change
//! (the frame-1 flash and the boundary stall S2 measured away); the swap-on-demand path stays for seeks past the prewarm.
//!
//! The one structural divergence, carried from step 1b: the streaming decoder runs `Policy::Clocked`, so the
//! `PlaybackTimer` LEADS playback here (QCView's single-video mode lets the decoder pace itself and mirrors the timer
//! from `currentFrameChanged`; its shuttle / dual / image-sequence paths are timer-leads already). A seek while playing
//! holds the integrator until the sought frame lands and re-seats the timer there (QCView's decoder re-baselines its
//! pacing on that same frame); the loop wrap is the timer's; FrameLocked advances one frame per refresh only when the
//! decoder has delivered it ("playback slows rather than skipping").
use crate::{ffmpeg::{OpenOptions, SeqOptions, SessionInfo}, frame_handle::FrameHandle, frame_index::FrameIndex, hw::HwDevice, key_map::Action, playback_timer::{ClockPolicy, PlaybackTimer}, scrub_decoder::{ScrubDecoder, ScrubOptions}, source::Source, video_decoder::{Policy, State, VideoDecoder}, Error};
use qcv_meta::VideoMetadata;
use qcv_timeline::{rescale, Clip, ClipMediaKind, Editor, InOutRange, MediaInfo, Rate, Timeline};
use std::{collections::BTreeMap, sync::{mpsc, Arc}, thread, time::{Duration, Instant}};

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
    fn set_volume(&mut self, _v: f32) {}
    /// The shell's persisted mute (QCView `audio/muted`), as against the M key's toggle.
    fn set_muted(&mut self, _m: bool) {}
    /// QCView `audio/syncOffsetMs`: positive delays the audio.
    fn set_sync_offset_ms(&mut self, _ms: i32) {}
    /// QCView `audio/scrubMuted`: no shuttle / scrub grains while set.
    fn set_scrub_muted(&mut self, _m: bool) {}
    /// The Inspector's Mix picker (QCView's routing pill): Auto / 5.1 → 2 / channels 7-8.
    fn set_routing_mode(&mut self, _m: crate::audio::RoutingMode) {}
    fn routing_mode(&self) -> crate::audio::RoutingMode { crate::audio::RoutingMode::Auto }
    /// Per-channel peak levels for the Inspector's meters (0..1, the decoder's frame peaks; 16 slots).
    fn peak_levels(&self) -> [f32; 16] { [0.0; 16] }
    /// Open the file's audio (false = none); the Transport calls it per clip.
    fn open(&mut self, _path: &str) -> bool { false }
    fn close(&mut self) {}
    /// Every refresh (deferred device stops and the like).
    fn tick(&mut self, _now: Instant) {}
    fn is_muted(&self) -> bool { false }
    fn volume(&self) -> f32 { 1.0 }
    fn device_latency_ms(&self) -> f64 { 0.0 }
    fn source_channels(&self) -> usize { 0 }
    fn layout_name(&self) -> String { String::new() }
    fn stats(&mut self) -> Option<crate::audio::AudioStats> { None }
    /// Drain the drift samples gathered since the last call (the stats line).
    fn take_drift(&mut self) -> Vec<f32> { vec![] }
}
pub struct NoAudio;
impl AudioHooks for NoAudio {}

#[derive(Default, Debug)]
pub struct Stats {
    /// shown-frame lateness vs the clock, ms (negative = early)
    pub lateness_ms: Vec<f32>, /// refreshes per shown frame → count
    pub cadence: BTreeMap<i64, u32>, pub dropped: u32, pub late: u32, pub black: u32, pub shown: u32, pub decode_ms: Vec<f32>, pub loops: u32,
    /// seek issued → the sought frame landed (the stream's own answer), ms
    pub seek_land_ms: Vec<f32>, /// clip swaps: prewarm hits / fresh opens
    pub swaps_prewarmed: u32, pub swaps_opened: u32,
}

#[derive(Clone)]
pub struct TransportOptions { pub hw: Option<HwDevice>, /// the intra-codec device (Windows: FFmpeg's Vulkan device for ProRes / FFV1 / APV, S6)
    pub hw_intra: Option<HwDevice>, pub threads: i32, /// None = no scrub decoder (tests)
    pub scrub: Option<ScrubOptions>, /// QCView: `playback/loopEnabled`, default ON ("press play, watch a cycle, scrub, watch again")
    pub loop_enabled: bool, pub clock_policy: ClockPolicy, /// one refresh in ms — the "late" threshold
    pub refresh_ms: f32 }
impl Default for TransportOptions { fn default() -> Self { TransportOptions { hw: None, hw_intra: None, threads: 0, scrub: Some(ScrubOptions::default()), loop_enabled: true, clock_policy: ClockPolicy::WallClock, refresh_ms: 1000.0 / 60.0 } } }

/// What a pending swap does once its clip has opened: the latest scrub target, a seek (timeline frame, source frame), or rest
/// at the in-point it was opened on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum After { Scrub(i32), Seek(i64, i32), AtIn }
struct Pending { clip: usize, rx: mpsc::Receiver<Result<Slot, Error>>, autoplay: bool, after: After }
/// A slot that leaves the transport is dropped on a thread: its decoders join their threads (seconds on a NAS / Windows).
fn drop_off_thread(s: Slot) { thread::Builder::new().name("qcv-slot-drop".into()).spawn(move || drop(s)).ok(); }

/// A decoder slot: one clip's streaming decoder (+ its scrub decoder), with the clip's source window.
pub struct Slot { pub clip: usize, pub decoder: VideoDecoder, pub scrub: Option<ScrubDecoder>, pub info: SessionInfo, pub index: Arc<FrameIndex>, pub path: String, pub src_rate: Rate, pub src_in: i64, pub src_out: i64, /// parked on its in-point with the lookahead queued (a prewarmed slot); false once it has played
    pub positioned_at_in: bool }
impl Slot {
    fn open(clip_index: usize, clip: &Clip, opts: &TransportOptions) -> Result<Slot, Error> {
        let src = if clip.media_kind == ClipMediaKind::ImageSequence { Source::Sequence { pattern: clip.media_path.clone(), rate: clip.source_rate, start: clip.sequence_start_frame, end: clip.sequence_end_frame, label: clip.name.clone() } } else { Source::File(clip.media_path.clone()) };
        let open = OpenOptions { threads: opts.threads, hw: opts.hw.clone(), hw_intra: opts.hw_intra.clone(), seq: match &src { Source::Sequence { rate, start, .. } => Some(SeqOptions { rate: *rate, start_number: *start }), _ => None }, fast: false };
        let mut decoder = VideoDecoder::new(Policy::Clocked { lookahead: LOOKAHEAD });
        decoder.open(src.path(), &open)?;
        let info = decoder.info.clone().unwrap();
        let index = decoder.frame_index().cloned().ok_or_else(|| Error::Open("no frame index".into()))?;
        let scrub = match (&opts.scrub, &src) {
            (Some(so), Source::File(_)) => { let so = ScrubOptions { hw: opts.hw.clone(), hw_intra: opts.hw_intra.clone(), ..so.clone() }; match ScrubDecoder::open(src.path(), &so, index.clone(), decoder.external_sink()) { Ok(s) => Some(s), Err(e) => { log::warn!("Transport: no scrub decoder: {e}"); None } } }
            _ => None,
        };
        Ok(Slot { clip: clip_index, decoder, scrub, info, index, path: clip.media_path.clone(), src_rate: clip.source_rate, src_in: clip.source_in, src_out: clip.source_out, positioned_at_in: false })
    }
}

struct FastSeek { dir: i32, speed: f64, started: Instant, last: Instant, last_tick: Instant, position_sec: f64 }

pub struct Transport {
    pub timeline: Timeline, pub timer: PlaybackTimer, pub audio: Box<dyn AudioHooks>, opts: TransportOptions, range_override: crate::RangeOverride,
    slots: Vec<Slot>, active: usize, /// the spare slot's open in flight: (clip index, the receiver)
    prewarm: Option<(usize, mpsc::Receiver<Result<Slot, Error>>)>,
    /// a clip that was NOT prewarmed, opening on a thread (the UI keeps running; the current picture holds): what to do on landing
    pending: Option<Pending>,
    rate: Rate, frames: i32, fps: f64,
    review_speed: f64, loop_enabled: bool, in_point: i32, out_point: i32,
    /// user intent, captured BEFORE the decoder flips — the loop wrap reads it to decide on the auto-resume
    user_wants_playback: bool,
    fast: Option<FastSeek>, modal_active: bool, modal_was_playing: bool, scrubbing: Option<bool>,
    /// a seek in flight on the active slot: (target SOURCE frame, the decoder's seek + publish counters when issued, issued at)
    seek_gate: Option<(i32, u32, u64, Instant)>,
    playlist_active: bool, pending_playlist_seek: Option<(f64, Instant)>, resume_after_playlist_seek: bool,
    pub stats: Stats, last_shown: Option<i64>, refreshes_on_current: i64, pub refresh_ms: f32, current: Option<FrameHandle>, first_landed: bool,
    /// `play` held the audio until the next picture lands (QCView releases on the first frame published after play)
    audio_hold_pending: bool,
    trace_play: Option<Instant>, trace_n: u32,
}

impl Transport {
    /// Single media: a one-clip locked timeline around the file (QCView's `loadSingleMedia`).
    pub fn open(src: &Source, opts: TransportOptions) -> Result<Transport, Error> {
        let probe_clip = Clip { media_path: src.path().to_string(), media_kind: if matches!(src, Source::Sequence { .. }) { ClipMediaKind::ImageSequence } else { ClipMediaKind::Video }, source_rate: match src { Source::Sequence { rate, .. } => *rate, _ => Rate::new(24, 1) }, sequence_start_frame: match src { Source::Sequence { start, .. } => *start as i64, _ => 1 }, ..Clip::empty() };
        let slot = Slot::open(0, &probe_clip, &opts)?;
        let rate = slot.info.rate(); let frames = slot.info.frame_count as i64;
        let mut editor = Editor::default();
        editor.load_single_media(&MediaInfo { id: String::new(), name: src.display(), path: src.path().to_string(), kind: probe_clip.media_kind, rate, duration: frames, width: slot.info.width, height: slot.info.height, has_audio: false, sequence_pattern: String::new(), sequence_start_frame: probe_clip.sequence_start_frame, sequence_end_frame: probe_clip.sequence_start_frame + frames - 1 });
        let mut t = Transport::from_timeline(editor.timeline, opts, false);
        let first = t.timeline.tracks[0].clips[0].clone();
        let mut slot = slot; slot.clip = 0; slot.src_rate = first.source_rate; slot.src_in = first.source_in; slot.src_out = first.source_out;
        t.slots.push(slot); t.active = 0;
        // QCView's open: the decoder opens PAUSED and WindowManager ends with seekToFrame(0) — the pending seek runs whatever the
        // play state and publishes frame 0 through the seek gate; the gate captures the counts BEFORE the seek so the landing counts
        let (seeks_before, seq_before) = (t.slots[0].decoder.seeks_performed(), t.slots[0].decoder.published_seq());
        t.seek_gate = Some((0, seeks_before, seq_before, Instant::now()));
        t.slots[0].decoder.seek_to_frame(0);
        Ok(t)
    }
    /// A playlist (`Editor::load_playlist`'s timeline): the first non-gap clip opens on slot A positioned at its in-point,
    /// the next one prewarms on B (QCView's `startPlaylist` → `playlistAdvanceToClip(0, autoplay = false)`).
    pub fn open_playlist(timeline: Timeline, opts: TransportOptions) -> Result<Transport, Error> {
        let mut t = Transport::from_timeline(timeline, opts, true);
        let Some(first) = t.find_non_gap_clip(0, 1) else { return Err(Error::Open("empty playlist".into())) };
        let clip = t.timeline.tracks[0].clips[first].clone();
        let slot = Slot::open(first, &clip, &t.opts)?;
        slot.decoder.seek_to_frame(clip.source_in as i32);
        t.seek_gate = Some((clip.source_in as i32, slot.decoder.seeks_performed(), slot.decoder.published_seq(), Instant::now()));
        t.slots.push(slot); t.active = 0;
        t.timer.seek(clip.start as f64 / t.fps);
        t.ensure_prewarm();
        Ok(t)
    }
    fn from_timeline(timeline: Timeline, opts: TransportOptions, playlist: bool) -> Transport {
        let rate = timeline.rate; let fps = rate.as_f64(); let frames = timeline.duration as i32;
        let mut timer = PlaybackTimer::new();
        timer.set_frame_rate(fps); timer.set_duration(if fps > 0.0 { frames as f64 / fps } else { 0.0 }); timer.set_clock_policy(opts.clock_policy); timer.set_looping(opts.loop_enabled);
        Transport { timeline, timer, audio: Box::new(NoAudio), loop_enabled: opts.loop_enabled, refresh_ms: opts.refresh_ms, opts, range_override: crate::RangeOverride::Auto,
            slots: vec![], active: 0, prewarm: None, pending: None, rate, frames, fps,
            review_speed: 1.0, in_point: -1, out_point: -1, user_wants_playback: false,
            fast: None, modal_active: false, modal_was_playing: false, scrubbing: None, seek_gate: None,
            playlist_active: playlist, pending_playlist_seek: None, resume_after_playlist_seek: false,
            stats: Stats::default(), last_shown: None, refreshes_on_current: 0, current: None, first_landed: false, audio_hold_pending: false, trace_play: None, trace_n: 0 }
    }
    /// The timeline changed under us (an edit): durations re-seat; the active clip index is re-found by id order.
    pub fn set_timeline(&mut self, timeline: Timeline) {
        self.timeline = timeline; self.frames = self.timeline.duration as i32;
        self.timer.set_duration(if self.fps > 0.0 { self.frames as f64 / self.fps } else { 0.0 });
        for s in &mut self.slots { if let Some(c) = self.timeline.tracks.first().and_then(|t| t.clips.get(s.clip)) { s.src_in = c.source_in; s.src_out = c.source_out; } }
    }

    // ---- reads --------------------------------------------------------------------------------------------------
    pub fn info(&self) -> &SessionInfo { &self.slots[self.active].info }
    /// The active clip's metadata (the Inspector's rows come from `qcv-meta` on the project side; this is the session's).
    pub fn probe(&self) -> VideoMetadata { qcv_meta::extract(&self.slots[self.active].path) }
    pub fn decoder(&self) -> &VideoDecoder { &self.slots[self.active].decoder }
    pub fn scrub(&mut self) -> Option<&mut ScrubDecoder> { self.slots[self.active].scrub.as_mut() }
    pub fn active_slot(&self) -> &Slot { &self.slots[self.active] }
    pub fn frame_index(&self) -> &Arc<FrameIndex> { &self.slots[self.active].index }
    pub fn rate(&self) -> Rate { self.rate }
    /// timeline frames
    pub fn frame_count(&self) -> i32 { self.frames }
    pub fn path(&self) -> &str { &self.slots[self.active].path }
    pub fn current(&self) -> Option<&FrameHandle> { self.current.as_ref() }
    /// the first frame has landed (the clock is seated)
    pub fn started(&self) -> bool { self.first_landed }
    pub fn is_playing(&self) -> bool { self.timer.is_playing() }
    /// played through to the end, not looping (QCView: "play stops at the end")
    pub fn at_end(&self) -> bool { !self.loop_enabled && self.timer.duration() > 0.0 && self.timer.position() >= self.timer.duration() && !self.timer.is_playing() }
    /// the TIMELINE frame under the playhead (`currentFrameUnified`)
    pub fn current_frame(&self) -> i32 { self.timer.current_frame().clamp(0, (self.frames - 1).max(0)) }
    pub fn review_speed(&self) -> f64 { self.review_speed }
    pub fn loop_enabled(&self) -> bool { self.loop_enabled }
    /// QCView's per-clip Range override (Inspector ▸ Color ▸ Range: Auto / Full / Limited) on the active clip's stream and scrub decoders.
    pub fn set_range_override(&mut self, r: crate::RangeOverride) {
        self.range_override = r;
        let slot = &mut self.slots[self.active];
        slot.decoder.set_range_override(r);
        if let Some(sc) = slot.scrub.as_mut() { sc.set_range_override(r); }
    }
    pub fn range_override(&self) -> crate::RangeOverride { self.range_override }
    pub fn in_point(&self) -> i32 { self.in_point }
    pub fn out_point(&self) -> i32 { self.out_point }
    pub fn has_in_out_range(&self) -> bool { self.in_point >= 0 && self.out_point > self.in_point }
    pub fn is_fast_seeking(&self) -> bool { self.fast.is_some() }
    pub fn modal_active(&self) -> bool { self.modal_active }
    pub fn seek_in_flight(&self) -> bool { self.seek_gate.is_some() || self.pending.is_some() || self.pending_playlist_seek.is_some() }   // a clip still opening off-thread (or the 50-ms debounce) counts
    pub fn is_playlist(&self) -> bool { self.playlist_active }
    pub fn active_clip_index(&self) -> usize { self.slots[self.active].clip }
    pub fn prewarmed_clip(&self) -> Option<usize> { self.slots.iter().enumerate().find(|(i, _)| *i != self.active).map(|(_, s)| s.clip) }
    fn clip(&self, i: usize) -> Option<&Clip> { self.timeline.tracks.first().and_then(|t| t.clips.get(i)) }
    /// timeline frame → (clip index, source frame) through the flattener
    fn resolve(&self, tl_frame: i64) -> Option<(usize, i64)> {
        let t = self.timeline.tracks.first()?;
        let c = self.timeline.visible_clip_at(tl_frame)?;
        let i = t.clips.iter().position(|x| x.id == c.id)?;
        Some((i, self.timeline.timeline_to_source(c, tl_frame)?))
    }
    fn tl_frame_of(&self, slot: &Slot, src_frame: i64) -> i64 { let start = self.clip(slot.clip).map(|c| c.start).unwrap_or(0); start + rescale(src_frame - slot.src_in, slot.src_rate, self.rate) }
    /// `playlistFindNonGapClip`
    fn find_non_gap_clip(&self, from: isize, dir: isize) -> Option<usize> {
        let t = self.timeline.tracks.first()?; let mut i = from;
        while i >= 0 && (i as usize) < t.clips.len() { if !t.clips[i as usize].is_gap { return Some(i as usize) } i += dir; }
        None
    }

    // ---- play / pause ---------------------------------------------------------------------------------------------
    pub fn play(&mut self) {
        self.user_wants_playback = true;
        if self.at_end() { self.seek_to_frame(0); }   // play from the end restarts (the timer clamps at duration)
        self.timer.play(); self.slots[self.active].decoder.play();
        if std::env::var_os("QCV_AUDIO_TRACE").is_some() { self.trace_play = Some(Instant::now()); self.trace_n = 12; log::info!("trace: play() at timer {:.4}", self.timer.position()); }
        if self.audio.has_audio() { let p = self.audio_position(); self.audio.seek(p); self.audio.set_hold(true); self.audio.play(); self.audio_hold_pending = true; }
    }
    pub fn pause(&mut self) {
        self.user_wants_playback = false;
        self.timer.pause(); self.slots[self.active].decoder.pause(); self.audio.pause();
    }
    pub fn toggle_playback(&mut self) { if self.timer.is_playing() { self.pause() } else { self.play() } }

    // ---- seeks (timeline time) ------------------------------------------------------------------------------------
    /// The canonical seek. `seconds` is a TIMELINE position.
    pub fn seek_to_time(&mut self, seconds: f64) {
        let seconds = seconds.max(0.0);
        if self.playlist_active { self.pending_playlist_seek = Some((seconds, Instant::now())); return }   // coalesced; `tick` fires it
        if self.fps <= 0.0 { return }
        let tl = ((seconds * self.fps).round() as i64).clamp(0, (self.frames as i64 - 1).max(0));
        let Some((_, src)) = self.resolve(tl) else { return };
        self.issue_seek(tl, src as i32);
    }
    pub fn seek_to_frame(&mut self, frame: i32) { if self.fps > 0.0 { self.seek_to_time(frame as f64 / self.fps) } }
    /// Pauses the decoder NOW so it stops publishing pre-seek frames through the debounce; the commit resumes it.
    pub fn seek_to_frame_and_resume(&mut self, frame: i32, resume: bool) {
        if resume && self.playlist_active && self.slots[self.active].decoder.is_playing() { self.slots[self.active].decoder.pause(); }
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
    fn issue_seek(&mut self, tl_frame: i64, src_frame: i32) {
        let d = &self.slots[self.active].decoder;
        self.seek_gate = Some((src_frame, d.seeks_performed(), d.published_seq(), Instant::now()));
        self.timer.seek(tl_frame as f64 / self.fps);   // immediate feedback for readouts; the landing re-seats it exactly
        self.slots[self.active].decoder.seek_to_frame(src_frame);
        if !self.is_playing() { let p = self.audio_position(); self.audio.seek(p); }
    }
    /// `applyPlaylistSeek`: the clip containing `pos` (past the end → the last non-gap; inside a gap → the next non-gap, else
    /// the previous); swap if it isn't the active one (the prewarmed slot when it matches, a fresh open otherwise); then
    /// the decoder seek to the source frame, the timer to `pos`, the deferred resume.
    fn apply_playlist_seek(&mut self, pos: f64) {
        if !self.playlist_active || self.fps <= 0.0 { return }
        let Some(track) = self.timeline.tracks.first() else { return };
        let tl = (pos * self.fps).round() as i64;
        let mut target = track.clips.iter().position(|c| c.contains(tl));
        if target.is_none() { target = self.find_non_gap_clip(track.clips.len() as isize - 1, -1); }
        let Some(mut target) = target else { return };
        if track.clips[target].is_gap { let Some(t) = self.find_non_gap_clip(target as isize, 1).or_else(|| self.find_non_gap_clip(target as isize, -1)) else { return }; target = t; }
        let was_playing = self.timer.is_playing();
        let (start, src_in, src_rate) = { let c = &self.timeline.tracks[0].clips[target]; (c.start, c.source_in, c.source_rate) };
        let src = src_in + rescale(tl - start, self.rate, src_rate);
        if target != self.slots[self.active].clip { if !self.swap_to(target, was_playing) { self.set_after(After::Seek(tl, src as i32)); self.timer.seek(tl as f64 / self.fps); return } }
        else { self.pending = None; }   // back on the active clip: a pending open for another clip is stale
        self.issue_seek(tl, src as i32);
        if self.resume_after_playlist_seek { self.resume_after_playlist_seek = false; self.slots[self.active].decoder.play(); if !self.timer.is_playing() { self.timer.play(); } }
    }
    /// `playlistAdvanceToClip`: make `clip` the active one — the prewarmed slot when it holds that clip, else a fresh open
    /// (QCView's path); position on `src_frame` or the in-point; autoplay as asked.
    /// Make `clip` the active one. A prewarmed / already-open slot swaps NOW (true). Otherwise the clip opens on a thread and
    /// the swap lands in `pump_pending` (false) — DEVIATION from QCView's synchronous `playlistAdvanceToClip`: its open costs
    /// ~50–100 ms; ours on 5.7K ProRes from a NAS on Windows took ~4 s and froze the UI mid-scrub (2026-11-08).
    fn swap_to(&mut self, clip: usize, autoplay: bool) -> bool {
        self.pump_prewarm();
        if let Some(i) = self.slots.iter().position(|s| s.clip == clip) {
            if i != self.active { self.slots[self.active].decoder.pause(); self.active = i; self.stats.swaps_prewarmed += 1; }
            self.slots[self.active].positioned_at_in = false;
            self.pending = None;
            self.last_shown = None; self.seek_gate = None;
            if autoplay { self.slots[self.active].decoder.play(); }
            self.ensure_prewarm();
            return true
        }
        if let Some(p) = self.pending.as_mut() { if p.clip == clip { p.autoplay = autoplay; return false } }
        let Some(c) = self.clip(clip).cloned() else { return false };
        let opts = self.opts.clone();
        let (tx, rx) = mpsc::channel();
        thread::Builder::new().name("qcv-swap-open".into()).spawn(move || { let r = Slot::open(clip, &c, &opts).map(|s| { s.decoder.seek_to_frame(c.source_in as i32); s }); if let Err(mpsc::SendError(r)) = tx.send(r) { if let Ok(s) = r { drop(s); } } }).ok();
        self.pending = Some(Pending { clip, rx, autoplay, after: After::AtIn });   // a newer target replaces an older pending open (its slot is dropped on its thread)
        false
    }
    fn set_after(&mut self, after: After) { if let Some(p) = self.pending.as_mut() { p.after = after; } }
    /// A pending swap whose clip has opened lands: the spare slot takes it (the old one dropped off-thread), the latest target applies.
    fn pump_pending(&mut self) {
        let Some(p) = &self.pending else { return };
        let r = match p.rx.try_recv() { Ok(r) => r, Err(mpsc::TryRecvError::Empty) => return, Err(mpsc::TryRecvError::Disconnected) => { self.pending = None; return } };
        let p = self.pending.take().unwrap();
        let s = match r { Ok(s) => s, Err(e) => { log::warn!("Transport: open of clip {}: {e}", p.clip); return } };
        self.slots[self.active].decoder.pause();
        if self.slots.len() >= 2 { let spare = 1 - self.active; let old = std::mem::replace(&mut self.slots[spare], s); drop_off_thread(old); self.active = spare; } else { self.slots.push(s); self.active = self.slots.len() - 1; }
        self.stats.swaps_opened += 1;
        self.last_shown = None; self.seek_gate = None;
        match p.after {
            After::Scrub(src) => { let sl = &mut self.slots[self.active]; if let Some(sd) = &mut sl.scrub { sd.request_frame(src); } else { sl.decoder.seek_to_frame(src); } }
            After::Seek(tl, src) => { self.issue_seek(tl, src); if self.resume_after_playlist_seek { self.resume_after_playlist_seek = false; self.slots[self.active].decoder.play(); if !self.timer.is_playing() { self.timer.play(); } } }
            After::AtIn => {}
        }
        if p.autoplay { self.slots[self.active].decoder.play(); }
        log::info!("Transport: clip {} opened off the UI thread", p.clip);
        self.ensure_prewarm();
    }
    /// The next non-gap clip after the active one (the first one when looping at the end) opens on the spare slot, on a
    /// thread, positioned at its in-point — ready before the cut.
    fn ensure_prewarm(&mut self) {
        if !self.playlist_active { return }
        let active_clip = self.slots[self.active].clip;
        let next = self.find_non_gap_clip(active_clip as isize + 1, 1).or_else(|| if self.loop_enabled { self.find_non_gap_clip(0, 1) } else { None });
        let Some(next) = next else { return };
        if next == active_clip { return }
        if let Some(i) = self.slots.iter().position(|s| s.clip == next) {
            if i != self.active && !self.slots[i].positioned_at_in { let s = &mut self.slots[i]; s.decoder.pause(); s.decoder.seek_to_frame(s.src_in as i32); s.positioned_at_in = true; }   // the loop's return to a clip already open: re-park it
            return
        }
        if let Some((c, _)) = &self.prewarm { if *c == next { return } }
        let Some(clip) = self.clip(next).cloned() else { return };
        let opts = self.opts.clone();
        let (tx, rx) = mpsc::channel();
        thread::Builder::new().name("qcv-prewarm".into()).spawn(move || { let r = Slot::open(next, &clip, &opts).map(|mut s| { s.decoder.seek_to_frame(clip.source_in as i32); s.positioned_at_in = true; s }); tx.send(r).ok(); }).ok();
        self.prewarm = Some((next, rx));
    }
    /// A finished prewarm lands in the spare slot.
    fn pump_prewarm(&mut self) {
        let Some((clip, rx)) = &self.prewarm else { return };
        let clip = *clip;
        match rx.try_recv() {
            Ok(Ok(s)) => { self.prewarm = None; if self.slots.len() >= 2 { let spare = 1 - self.active; let old = std::mem::replace(&mut self.slots[spare], s); drop_off_thread(old); } else { self.slots.push(s); } log::info!("Transport: prewarmed clip {clip}"); }
            Ok(Err(e)) => { self.prewarm = None; log::warn!("Transport: prewarm of clip {clip}: {e}"); }
            Err(mpsc::TryRecvError::Empty) => {}
            Err(mpsc::TryRecvError::Disconnected) => { self.prewarm = None; }
        }
    }

    // ---- the scrub gesture (TimelinePanel: press pauses, moves preview through the scrub decoder, release seeks) ----
    pub fn begin_scrub(&mut self) {
        if self.scrubbing.is_some() { return }
        let was_playing = self.is_playing();
        if was_playing { self.timer.pause(); self.audio.pause(); }
        // QCView's press pauses the STREAMING decoder unconditionally (TimelinePanel.qml pressAt: `videoDecoder.pause()`), not
        // only when the clock ran: the decoder opens in its decoding state to prefill the clocked lookahead while the timer is
        // paused, and a live decoder refills the slot behind every scrub preview the moment the preview clears it — the tick
        // then shows ITS frames (the clip "plays" toward the pointer; 398 stream frames vs 132 previews in the 2026-11-07 trace).
        self.slots[self.active].decoder.pause();
        self.scrubbing = Some(was_playing);
    }
    /// `frame` is a TIMELINE frame; a cross-clip drag swaps first (QCView pays the open there too).
    pub fn scrub_to_frame(&mut self, frame: i32) {
        let tl = (frame as i64).clamp(0, (self.frames as i64 - 1).max(0));
        let Some((clip, src)) = self.resolve(tl) else { return };
        if clip != self.slots[self.active].clip { if !self.swap_to(clip, false) { self.set_after(After::Scrub(src as i32)); if self.fps > 0.0 { self.timer.seek(tl as f64 / self.fps); } self.seek_gate = None; return } }
        else { self.pending = None; }
        let s = &mut self.slots[self.active];
        if let Some(sd) = &mut s.scrub { sd.request_frame(src as i32); } else { s.decoder.seek_to_frame(src as i32); }
        if self.fps > 0.0 { self.timer.seek(tl as f64 / self.fps); }
        self.seek_gate = None;
    }
    /// The heavy decoder seek lands here; playback resumes if the press interrupted it.
    pub fn end_scrub(&mut self) {
        let Some(was_playing) = self.scrubbing.take() else { return };
        let tl = self.current_frame() as i64;
        if let Some((_, src)) = self.resolve(tl) { self.issue_seek(tl, src as i32); }
        if was_playing { self.play(); }
    }
    pub fn is_scrubbing(&self) -> bool { self.scrubbing.is_some() }
    /// Install an audio player (QCView's `AudioPlayer`): opens the active clip's audio on it.
    pub fn set_audio(&mut self, mut audio: Box<dyn AudioHooks>) {
        let path = self.path().to_string();
        if !path.is_empty() { audio.open(&path); }
        self.audio = audio;
    }

    // ---- review speed ---------------------------------------------------------------------------------------------
    pub fn set_review_speed(&mut self, speed: f64) {
        let speed = speed.clamp(0.25, 4.0);
        if (self.review_speed - speed).abs() < 1e-9 { return }
        self.review_speed = speed;
        for s in &self.slots { s.decoder.set_playback_speed(speed); }
        self.timer.set_playback_speed(speed);
        if self.audio.has_audio() { self.audio.set_tempo(speed); let p = self.audio_position(); self.audio.seek(p); }
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
        self.timer.set_looping(on);   // the timer wraps the whole timeline; a playlist's wrap resolves to its first clip
        self.push_in_out_to_timer();
        if on { self.ensure_prewarm(); }
    }
    /// In / out points are off in playlist mode (the TransportBar hides them there).
    pub fn set_in_point_at_current(&mut self) {
        if self.frames <= 0 || self.playlist_active { return }
        let new_in = self.current_frame().clamp(0, self.frames - 1);
        if self.out_point >= 0 && new_in >= self.out_point { self.out_point = -1; }   // "I clears O when they'd cross"
        if new_in == self.in_point { return }
        self.in_point = new_in; self.push_in_out_to_timer();
        if self.has_in_out_range() && !self.loop_enabled { self.set_loop_enabled(true); }   // auto-enable once the range becomes valid
    }
    pub fn set_out_point_at_current(&mut self) {
        if self.frames <= 0 || self.playlist_active { return }
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
        self.slots[self.active].decoder.set_read_ahead_range(if hydrate { self.in_point } else { -1 }, if hydrate { self.out_point } else { -1 });
    }

    // ---- the shuttle (hold-to-fast-seek) ----------------------------------------------------------------------------
    pub fn start_fast_seek(&mut self, direction: i32, now: Instant) {
        if direction == 0 { return }
        let direction = direction.signum();
        if let Some(f) = &self.fast { if f.dir == direction { return } }   // re-press / repeat: no-op; opposite: restart snappy
        if self.review_speed != 1.0 { self.set_review_speed(1.0); }        // shuttle and review speed are mutually exclusive
        if self.is_playing() { self.timer.pause(); self.audio.pause(); }
        self.slots[self.active].decoder.pause();   // unconditional, as the scrub press (a live decoder would refill the slot behind the shuttle's seeks)
        let position_sec = if self.fps > 0.0 { self.current_frame().max(0) as f64 / self.fps } else { 0.0 };
        self.seek_gate = None;   // the gesture drives the clock; no in-flight landing may drag it back
        let signed = direction as f64 * FAST_SEEK_INITIAL_SPEED;
        if self.audio.has_audio() { let (p, s) = self.audio_source_at(position_sec); self.audio.begin_shuttle(&p, s, signed); }
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
        let pos = f.position_sec; let tl = (pos * self.fps).round() as i64;
        // timer-leads-decoder: the clock advances every tick, the decoder chases async; playlists skip the per-tick decoder
        // seeks (a cross-clip swap is the one expensive hit — deferred to the release)
        self.timer.seek(pos);
        if !self.playlist_active { if let Some((_, src)) = self.resolve(tl) { self.slots[self.active].decoder.seek_to_frame(src as i32); } }
        if self.audio.shuttle_active() { let (p, s) = self.audio_source_at(pos); self.audio.shuttle_target(&p, s, signed); }
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
    /// One refresh: the shuttle's 33 ms tick, the playlist debounce, the prewarm landing, the clock, the clip dispatch,
    /// then the frame due now (if a new one).
    pub fn tick(&mut self, now: Instant) -> Option<&FrameHandle> {
        self.audio.tick(now);
        if let Some(f) = &self.fast { if now - f.last_tick >= Duration::from_millis(FAST_SEEK_TICK_MS) { self.fast_seek_tick(now); } }
        if let Some((sec, at)) = self.pending_playlist_seek { if now - at >= Duration::from_millis(PLAYLIST_SEEK_DEBOUNCE_MS) { self.pending_playlist_seek = None; self.apply_playlist_seek(sec); } }
        self.pump_prewarm();
        self.pump_pending();
        // a seek in flight: the integrator holds; the sought frame re-seats the clock when it lands
        if let Some((target, seeks_before, seq_before, issued)) = self.seek_gate {
            let a = self.active;
            let performed = self.slots[a].decoder.seeks_performed() != seeks_before || !self.first_landed;   // the open's first frame needs no seek
            if !performed { if self.slots[a].decoder.front_is_external() { let h = self.slots[a].decoder.fetch_front()?; return self.show(h, now) } return None }   // pre-seek frames: the seek clears them
            let Some(h) = self.slots[a].decoder.fetch_front() else {
                // the landing was published and then consumed by a scrub publish (the slot replaces): the picture is the same
                // target's; seat the clock on the decoder. A prewarm's own in-point landing (below the target) never qualifies.
                let d = &self.slots[a].decoder;
                if d.published_seq() > seq_before && d.current_frame() >= target { self.seek_gate = None; self.stats.seek_land_ms.push((now - issued).as_secs_f32() * 1000.0); let cf = d.current_frame() as i64; let tl = self.tl_frame_of(&self.slots[a], cf); if self.fps > 0.0 { self.timer.seek(tl as f64 / self.fps); } }
                return None
            };
            if h.frame_no < 0 { return self.show(h, now) }   // a scrub preview: shown, the gate stays
            if h.frame_no < target { return None }           // a prewarmed slot's in-point frame (or any pre-target one): dropped, the landing is still to come
            self.seek_gate = None;   // post-seek, the oldest queued = the landing (at or past the target: keyframe-rounded landings included)
            self.stats.seek_land_ms.push((now - issued).as_secs_f32() * 1000.0);
            let tl = self.tl_frame_of(&self.slots[a], h.frame_no as i64);
            if self.fps > 0.0 { self.timer.seek(tl as f64 / self.fps); }
            if self.audio.has_audio() && self.is_playing() { let p = self.audio_position(); if let Some(t0) = self.trace_play { log::info!("trace: hold released at the seek landing, master {p:.4}, timer {:.4}, frame {}, {:.1} ms after play()", self.timer.position(), h.frame_no, t0.elapsed().as_secs_f32() * 1000.0); } self.audio.release_hold(p); self.audio_hold_pending = false; }
            return self.show(h, now);
        }
        if self.fast.is_some() || self.scrubbing.is_some() { let (h, _) = self.slots[self.active].decoder.fetch_due(i32::MAX); return self.show(h?, now) }   // the gesture drives the clock; frames land whenever
        let tl = match self.timer.clock_policy() {
            ClockPolicy::WallClock => {
                self.timer.update(now);
                if self.timer.wrapped { self.on_loop_wrap(); return None }
                if self.timer.is_playing() && !self.timer.looping() && self.timer.duration() > 0.0 && self.timer.position() >= self.timer.duration() { self.timer.pause(); self.slots[self.active].decoder.pause(); self.audio.pause(); }   // EndOfStream: play stops at the end
                self.current_frame() as i64
            }
            ClockPolicy::FrameLocked => if self.timer.is_playing() { self.current_frame() as i64 + 1 } else { self.current_frame() as i64 },
        };
        // the clip dispatch (QCView's boundary cross: the mirror sees sourceTime pass sourceOut → playlistAdvanceToClip)
        let Some((clip, src)) = self.resolve(tl.min(self.frames as i64 - 1)) else { return None };   // a gap or past the end: the picture holds
        if clip != self.slots[self.active].clip { let playing = self.timer.is_playing(); if !self.swap_to(clip, playing) { return None } }
        let (h, dropped) = self.slots[self.active].decoder.fetch_due(src as i32);
        self.stats.dropped += dropped;
        let h = h?;
        if self.timer.clock_policy() == ClockPolicy::FrameLocked && self.timer.is_playing() && h.frame_no >= 0 {
            self.timer.advance_by_frame();
            if !self.timer.looping() && self.timer.current_frame() >= self.frames { self.timer.pause(); self.slots[self.active].decoder.pause(); }
        }
        if self.audio.has_audio() && self.is_playing() { let p = self.audio_position(); if self.trace_n > 0 { self.trace_n -= 1; if let Some(t0) = self.trace_play { log::info!("trace: update master {p:.4} timer {:.4} frame {} wall since play {:.1} ms", self.timer.position(), self.timer.current_frame(), t0.elapsed().as_secs_f32() * 1000.0); } } self.audio.update(p); }
        self.show(h, now)
    }
    /// The timer wrapped (the whole timeline or [in, out]): re-seat the decoder at the wrap frame (swapping clips when the
    /// wrap lands in another one) and keep going if the user wants playback.
    fn on_loop_wrap(&mut self) {
        self.stats.loops += 1;
        if self.frames <= 1 { return }   // a 1-frame source: nothing to loop
        let tl = self.current_frame() as i64;
        let Some((clip, src)) = self.resolve(tl).or_else(|| { let c = self.find_non_gap_clip(0, 1)?; let k = self.clip(c)?; Some((c, k.source_in)) }) else { return };
        if clip != self.slots[self.active].clip { if !self.swap_to(clip, self.user_wants_playback) { return } }
        let a = self.active;
        self.seek_gate = Some((src as i32, self.slots[a].decoder.seeks_performed(), self.slots[a].decoder.published_seq(), Instant::now()));
        self.slots[a].decoder.seek_to_frame(src as i32);
        if self.user_wants_playback && !self.slots[a].decoder.is_playing() { self.slots[a].decoder.play(); }
        self.last_shown = None;
    }
    /// The active clip's SOURCE seconds under the playhead (QCView's `audioPosition`: playlist = sourceIn + (t − start)).
    fn audio_position(&self) -> f64 { self.audio_source_at(self.timer.position()).1 }
    fn audio_source_at(&self, timeline_sec: f64) -> (String, f64) {
        let tl = (timeline_sec * self.fps).round() as i64;
        match self.resolve(tl) { Some((clip, src)) => { let c = self.clip(clip); (c.map(|c| c.media_path.clone()).unwrap_or_default(), c.map(|c| src as f64 / c.source_rate.as_f64()).unwrap_or(0.0)) } None => (String::new(), timeline_sec) }
    }
    fn show(&mut self, h: FrameHandle, now: Instant) -> Option<&FrameHandle> {
        if h.frame_no >= 0 {
            if self.audio_hold_pending && self.audio.has_audio() && self.is_playing() { self.audio_hold_pending = false; let p = self.audio_position(); if let Some(t0) = self.trace_play { log::info!("trace: hold released in show() at master {p:.4}, timer {:.4}, frame {}, {:.1} ms after play()", self.timer.position(), h.frame_no, t0.elapsed().as_secs_f32() * 1000.0); } self.audio.release_hold(p); }
            let k = self.tl_frame_of(&self.slots[self.active], h.frame_no as i64);
            // a gap counts as dropped only while the clock runs it: a scrub / seek / shuttle jump is not a drop
            let continuous = self.timer.is_playing() && self.scrubbing.is_none() && self.fast.is_none() && self.seek_gate.is_none();
            if let Some(last) = self.last_shown { if continuous && k > last + 1 { self.stats.dropped += (k - last - 1) as u32; } *self.stats.cadence.entry(self.refreshes_on_current).or_insert(0) += 1; }
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
    pub fn decoder_state(&self) -> State { self.slots[self.active].decoder.state() }
    /// The hover thumbnail for a TIMELINE frame: the clip's GOP cache first (exact, ~1 ms), else the thumbnail cache's
    /// answer (the nearest cached through QCView's fallback while the exact one loads). Returns (entry, served by the peek).
    pub fn hover_thumbnail(&mut self, cache: &mut crate::thumbnail_cache::TimelineThumbnailCache, tl_frame: i32, max_size: u32) -> (crate::thumbnail_cache::ThumbEntry, bool) {
        let Some((clip, src)) = self.resolve(tl_frame as i64) else { return (Default::default(), false) };
        let path = self.clip(clip).map(|c| c.media_path.clone()).unwrap_or_default();
        let key = crate::thumbnail_cache::ThumbKey { source_path: path.clone(), source_frame: src as i32 };
        if !cache.contains(&path, src as i32) {
            if let Some(sd) = self.slots.iter().find(|s| s.clip == clip).and_then(|s| s.scrub.as_ref()) { if let Some(px) = sd.peek_thumbnail(src as i32, max_size) { cache.offer(key, px); return (cache.get_thumbnail(&path, src as i32, true), true) } }
        }
        (cache.get_thumbnail(&path, src as i32, true), false)
    }
}

/// A playlist from `path@in-out,…` specs (the harness's `--cut`): trims in SOURCE frames, the master rate = the first clip's.
pub fn playlist_from_specs(specs: &[(String, Option<(i64, i64)>)]) -> Result<Timeline, Error> {
    let mut items = vec![]; let mut trims = vec![]; let mut master: Option<Rate> = None;
    for (path, trim) in specs {
        let m = qcv_meta::extract(path);
        if !m.loaded { return Err(Error::Open(format!("probe {path}"))) }
        let rate = Rate::from_f64(m.frame_rate); if master.is_none() { master = Some(rate); }
        items.push(MediaInfo { id: path.clone(), name: std::path::Path::new(path).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(), path: path.clone(), kind: ClipMediaKind::Video, rate, duration: m.total_frames as i64, width: m.width as u32, height: m.height as u32, has_audio: m.audio_channels > 0, sequence_pattern: String::new(), sequence_start_frame: 1, sequence_end_frame: 1 });
        trims.push(InOutRange { in_point: trim.map(|t| t.0), out_point: trim.map(|t| t.1) });
    }
    let mut e = Editor::default();
    e.load_playlist("cut", &items, &trims, master.unwrap_or(Rate::new(24, 1)), 0, (0, 0), "");
    Ok(e.timeline)
}
