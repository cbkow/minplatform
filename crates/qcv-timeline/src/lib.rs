//! `qcv-timeline` — QCView's timeline model and edits carried onto RATIONAL time (canon §17.5, row 83: a frame count at a
//! rate, never a double; seconds are display-only; edits snap by construction). Ported from
//! `QCView-Player/src/timeline/timeline_types.h` (Clip / Track / EditPolicy / Timeline), `timeline_flattener.{h,cpp}`
//! (visible / audible clip at T, timeline → source, next / prev boundary), `timeline_edits.{h,cpp}` (the undo commands:
//! trim head / tail, slide, slip, split, delete, insert, move — as ops with inverses, row 84) and
//! `timeline_controller.{h,cpp}` (the edit API, `snapEdgeIndexAtTime`, `recomputeContiguousStartTimes`,
//! `patchClipDurationFromSource`, the live-preview + Esc-restore hooks, `loadSingleMedia` / `loadPlaylist`, `setFrameRate`).
//! The `PlaybackTimer` stays with the Transport (`qcv-player`): the editor reports the new duration, the Transport seats
//! the clock. Row 84's policy enforcement is explicit here: locked tracks refuse edits, source bounds clamp, overlaps are
//! refused when the policy forbids them. OTIO is ruled out (row 83): this is the family's own type.
#![forbid(unsafe_code)]

/// Frames per second as a rational: 24000/1001, 24/1, 30000/1001 …
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Rate { pub num: u32, pub den: u32 }
impl Rate {
    pub const fn new(num: u32, den: u32) -> Rate { Rate { num, den } }
    /// An exact rate from a container's own rational (FFmpeg `AVRational`); a zero or negative part → None.
    pub fn from_rational(num: i32, den: i32) -> Option<Rate> { if num > 0 && den > 0 { Some(Rate { num: num as u32, den: den as u32 }) } else { None } }
    /// The nearest well-known rate for a float (the NTSC family snaps; integers stay exact; anything else → milli-fps).
    pub fn from_f64(fps: f64) -> Rate {
        for (n, d) in [(24000, 1001), (30000, 1001), (60000, 1001), (48000, 1001)] { if (fps - n as f64 / d as f64).abs() < 1e-3 { return Rate { num: n, den: d }; } }
        if (fps - fps.round()).abs() < 1e-6 { Rate { num: fps.round() as u32, den: 1 } } else { Rate { num: (fps * 1000.0).round() as u32, den: 1000 } }
    }
    pub fn as_f64(&self) -> f64 { self.num as f64 / self.den as f64 }
    pub fn frame_seconds(&self) -> f64 { self.den as f64 / self.num as f64 }
}
impl std::fmt::Display for Rate { fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { if self.den == 1 { write!(f, "{}", self.num) } else { write!(f, "{}/{}", self.num, self.den) } } }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RationalTime { pub value: i64, pub rate: Rate }
impl RationalTime {
    pub const fn new(value: i64, rate: Rate) -> Self { Self { value, rate } }
    pub fn seconds(&self) -> f64 { self.value as f64 * self.rate.frame_seconds() }
    pub fn from_seconds(s: f64, rate: Rate) -> Self { Self { value: (s * rate.as_f64()).round() as i64, rate } }
    /// Exact when the rates are equal; otherwise the nearest frame at `rate` (edits snap by construction).
    pub fn rescaled_to(&self, rate: Rate) -> Self {
        if rate == self.rate { return *self }
        let v = self.value as i128 * rate.num as i128 * self.rate.den as i128;
        let d = self.rate.num as i128 * rate.den as i128;
        Self { value: ((v + d / 2) / d) as i64, rate }
    }
}
/// `frames` at `from` → the nearest count at `to`.
pub fn rescale(frames: i64, from: Rate, to: Rate) -> i64 { RationalTime::new(frames, from).rescaled_to(to).value }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourceMode { /// 1 video track + optional audio; locked
    SingleMedia, /// 1 video track, N clips, ripple-edited
    Playlist, /// two free tracks A / B at the master rate (dual view, canon rows 211 / 212): no ripple, the length = the longer side
    Dual }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClipMediaKind { Video, ImageSequence, Audio }

/// A clip on a track. `start` / `duration` in TIMELINE frames; `source_in` / `source_out` (exclusive) / `source_duration`
/// in SOURCE frames at `source_rate`. A gap has no media and keeps only `duration`.
#[derive(Clone, Debug, PartialEq)]
pub struct Clip {
    pub id: String, pub name: String, pub start: i64, pub duration: i64, pub source_in: i64, pub source_out: i64, pub is_gap: bool,
    pub media_kind: ClipMediaKind, pub media_path: String, pub media_item_id: String, pub source_rate: Rate, pub source_width: u32, pub source_height: u32,
    /// 0 = unknown (a Finder drop before its metadata arrives)
    pub source_duration: i64, pub has_audio: bool,
    pub sequence_pattern: String, pub sequence_start_frame: i64, pub sequence_end_frame: i64, pub sequence_exr_layer: String, pub audio_muted: bool,
    /// an image sequence's gaps on disk at detection (0-based) — seeded into the cache's failed set at open (QCView `startImageSequence`)
    pub sequence_missing: Vec<i32>,
}
impl Clip {
    pub fn gap(duration: i64) -> Clip { Clip { id: fresh_id(), name: "Gap".into(), is_gap: true, duration, ..Clip::empty() } }
    pub fn empty() -> Clip { Clip { id: String::new(), name: String::new(), start: 0, duration: 0, source_in: 0, source_out: 0, is_gap: false, media_kind: ClipMediaKind::Video, media_path: String::new(), media_item_id: String::new(), source_rate: Rate::new(24, 1), source_width: 0, source_height: 0, source_duration: 0, has_audio: false, sequence_pattern: String::new(), sequence_start_frame: 1, sequence_end_frame: 1, sequence_exr_layer: String::new(), audio_muted: false, sequence_missing: vec![] } }
    pub fn end(&self) -> i64 { self.start + self.duration }
    /// Half-open containment: at the exact end of A which starts B, the next clip wins.
    pub fn contains(&self, frame: i64) -> bool { frame >= self.start && frame < self.end() }
    /// The source range's length in timeline frames (the trims snap to the timeline rate).
    pub fn source_len_at(&self, rate: Rate) -> i64 { rescale((self.source_out - self.source_in).max(0), self.source_rate, rate) }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Track { pub id: String, pub name: String, pub is_video: bool, pub visible: bool, pub muted: bool, pub audio_muted: bool, pub locked: bool, pub z_index: i32, pub clips: Vec<Clip> }
impl Track {
    pub fn video(id: &str, locked: bool) -> Track { Track { id: id.into(), name: id.into(), is_video: true, visible: true, muted: false, audio_muted: false, locked, z_index: 1, clips: vec![] } }
    pub fn clip(&self, id: &str) -> Option<&Clip> { self.clips.iter().find(|c| c.id == id) }
    pub fn index_of(&self, id: &str) -> Option<usize> { self.clips.iter().position(|c| c.id == id) }
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EditPolicy { /// Playlist: drag / trim ripples downstream
    pub ripple: bool, /// timeline frames of fixed gap between clips
    pub inter_clip_gap: i64, pub allow_overlaps: bool }
impl Default for EditPolicy { fn default() -> Self { EditPolicy { ripple: false, inter_clip_gap: 0, allow_overlaps: false } } }

#[derive(Clone, Debug, PartialEq)]
pub struct Timeline { pub name: String, pub mode: SourceMode, pub policy: EditPolicy, pub rate: Rate, /// computed from the tracks; cached
    pub duration: i64, pub canvas_width: u32, pub canvas_height: u32, pub tracks: Vec<Track> }
impl Default for Timeline { fn default() -> Self { Timeline { name: String::new(), mode: SourceMode::SingleMedia, policy: EditPolicy::default(), rate: Rate::new(24, 1), duration: 0, canvas_width: 1920, canvas_height: 1080, tracks: vec![] } } }
pub const TRACK_A: &str = "A";
pub const TRACK_B: &str = "B";
/// The timeline panel's snap tolerance (`snapTolPx`); the caller converts to frames via its pixels-per-frame.
pub const SNAP_TOL_PX: f64 = 12.0;

impl Timeline {
    pub fn track(&self, id: &str) -> Option<&Track> { self.tracks.iter().find(|t| t.id == id) }
    fn track_mut(&mut self, id: &str) -> Option<&mut Track> { self.tracks.iter_mut().find(|t| t.id == id) }
    pub fn seconds(&self, frames: i64) -> f64 { RationalTime::new(frames, self.rate).seconds() }

    // ---- the flattener (pure queries) ---------------------------------------------------------------------------
    fn clip_in_track_at(track: &Track, frame: i64) -> Option<&Clip> { track.clips.iter().find(|c| !c.is_gap && c.contains(frame)) }
    /// The clip on the topmost visible video track whose range contains `frame`.
    pub fn visible_clip_at(&self, frame: i64) -> Option<&Clip> {
        let (mut best, mut best_z) = (None, i32::MIN);
        for t in &self.tracks {
            if !t.is_video || !t.visible || t.z_index < best_z { continue }
            if let Some(c) = Self::clip_in_track_at(t, frame) { if t.z_index > best_z || best.is_none() { best = Some(c); best_z = t.z_index; } }
        }
        best
    }
    pub fn visible_clip_on_track(&self, track_id: &str, frame: i64) -> Option<&Clip> { let t = self.track(track_id)?; if !t.is_video || !t.visible { return None } Self::clip_in_track_at(t, frame) }
    /// Every audio-emitting clip at `frame`: video tracks' clips unless track / clip audio-muted (lower-z video tracks still
    /// contribute), plus every non-muted audio track's clip.
    pub fn audible_clips_at(&self, frame: i64) -> Vec<&Clip> {
        let mut out = vec![];
        for t in self.tracks.iter().filter(|t| t.is_video && !t.audio_muted) { if let Some(c) = Self::clip_in_track_at(t, frame) { if !c.audio_muted && c.has_audio { out.push(c); } } }
        for t in self.tracks.iter().filter(|t| !t.is_video && !t.muted) { if let Some(c) = Self::clip_in_track_at(t, frame) { if !c.audio_muted { out.push(c); } } }
        out
    }
    /// Timeline frame → the clip's SOURCE frame (at the clip's rate); None outside the clip.
    pub fn timeline_to_source(&self, clip: &Clip, frame: i64) -> Option<i64> { if !clip.contains(frame) { return None } Some(clip.source_in + rescale(frame - clip.start, self.rate, clip.source_rate)) }
    /// The visible clip at `frame` with its source frame (S2's `resolve`).
    pub fn resolve(&self, frame: i64) -> Option<(&Clip, i64)> { let c = self.visible_clip_at(frame)?; Some((c, self.timeline_to_source(c, frame)?)) }
    pub fn next_clip_start(&self, frame: i64) -> Option<i64> { self.tracks.iter().flat_map(|t| &t.clips).filter(|c| !c.is_gap && c.start > frame).map(|c| c.start).min() }
    pub fn prev_clip_start(&self, frame: i64) -> Option<i64> { self.tracks.iter().flat_map(|t| &t.clips).filter(|c| !c.is_gap && c.start < frame).map(|c| c.start).max() }
    /// Every clip start after the first (the cut points a sequencer prewarms).
    pub fn boundaries(&self) -> Vec<i64> { self.tracks.first().map(|t| t.clips.iter().skip(1).map(|c| c.start).collect()).unwrap_or_default() }

    /// Auto-extend the duration if any clip extends past the current end (called after every successful edit).
    pub fn recalculate_duration(&mut self) { let end = self.tracks.iter().flat_map(|t| &t.clips).map(|c| c.end()).max().unwrap_or(0); if end > self.duration || self.mode != SourceMode::SingleMedia { self.duration = end; } }
    /// Dual view (row 211): one side's track as a timeline at the side's own rate (the side's Transport reads it) — positions
    /// rescaled from the master rate, each clip's length = its source window at that rate.
    pub fn side_timeline(&self, track_id: &str, side_rate: Rate) -> Timeline {
        let mut track = Track::video(TRACK_A, false);
        if let Some(t) = self.track(track_id) {
            for c in t.clips.iter().filter(|c| !c.is_gap) {
                let mut k = c.clone(); k.start = rescale(c.start, self.rate, side_rate); k.duration = c.source_len_at(side_rate).max(1);
                track.clips.push(k);
            }
            track.clips.sort_by_key(|c| c.start);
        }
        let duration = track.clips.iter().map(|c| c.end()).max().unwrap_or(0);
        Timeline { name: self.name.clone(), mode: SourceMode::Dual, rate: side_rate, duration, tracks: vec![track], ..Default::default() }
    }
    /// Every clip on the track contiguous from 0 (each start = the sum of the preceding durations).
    pub fn recompute_contiguous_start_times(&mut self, track_id: &str) {
        if let Some(t) = self.track_mut(track_id) { let mut cursor = 0; for c in &mut t.clips { c.start = cursor; cursor += c.duration; } }
        self.recalculate_duration();
    }
    /// `snapEdgeIndexAtTime`: the insert-AT index whose nearest edge lies within `tol` frames of `frame` — clip[i].start →
    /// i, clip[i].end → i + 1, t = 0 → 0, the track end → len; None when nothing is within tolerance.
    pub fn snap_edge_index_at(&self, track_id: &str, frame: i64, tol: i64) -> Option<usize> {
        let t = self.track(track_id)?; let tol = tol.max(0);
        if t.clips.is_empty() { return if frame.abs() <= tol { Some(0) } else { None } }
        let (mut best, mut best_d) = (None, tol);
        let mut consider = |d: i64, idx: usize| { if d <= best_d { best_d = d; best = Some(idx); } };
        consider(frame.abs(), 0);
        for (i, c) in t.clips.iter().enumerate() { consider((frame - c.start).abs(), i); consider((frame - c.end()).abs(), i + 1); }
        best
    }
}

/// Media facts the loaders take (QCView's `MediaItem` slice the timeline reads).
#[derive(Clone, Debug, PartialEq)]
pub struct MediaInfo { pub id: String, pub name: String, pub path: String, pub kind: ClipMediaKind, pub rate: Rate, /// source frames; 0 = unknown
    pub duration: i64, pub width: u32, pub height: u32, pub has_audio: bool, pub sequence_pattern: String, pub sequence_start_frame: i64, pub sequence_end_frame: i64,
    /// the item's EXR layer and gaps on disk (a playlist clip opens its sequence with them)
    pub sequence_exr_layer: String, pub sequence_missing: Vec<i32> }
/// A playlist entry's trim in source frames; None = the full clip.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct InOutRange { pub in_point: Option<i64>, pub out_point: Option<i64> }

/// One reversible edit (row 84): `apply` and `invert` on the model; the editor keeps the two stacks.
#[derive(Clone, Debug, PartialEq)]
pub enum Edit {
    /// trim head / tail, slide, slip: one clip's whole state replaced
    Replace { track: String, clip: String, old: Clip, new: Clip, label: &'static str },
    Split { track: String, clip: String, old: Clip, first: Clip, second: Clip },
    Delete { track: String, clip: Clip, index: usize },
    Insert { track: String, index: usize, clip: Clip },
    /// slot-based reorder in the pre-remove numbering
    Move { track: String, from: usize, to: usize },
    /// several edits as one undo step (dual: a slide carrying the later clips, a delete that closes its gap)
    Batch { edits: Vec<Edit>, label: &'static str },
}

#[derive(Debug, Default)]
pub struct Editor { pub timeline: Timeline, undo: Vec<Edit>, redo: Vec<Edit>, /// bumps on every mutation (QCView's `timelineChanged`)
    pub generation: u64, pub playlist_media_item_id: String }

impl Editor {
    pub fn new(timeline: Timeline) -> Editor { Editor { timeline, ..Default::default() } }
    fn changed(&mut self) { self.generation += 1; }

    // ---- loaders (undo cleared on load, row 84) ----------------------------------------------------------------
    /// One locked video track, one clip spanning the media.
    pub fn load_single_media(&mut self, m: &MediaInfo) {
        let mut t = Timeline { name: m.name.clone(), mode: SourceMode::SingleMedia, rate: m.rate, duration: m.duration.max(0), ..Default::default() };
        let mut track = Track::video(TRACK_A, true);
        track.clips.push(Clip { id: fresh_id(), name: m.name.clone(), start: 0, duration: t.duration, source_in: 0, source_out: t.duration, media_kind: m.kind, media_path: m.path.clone(), media_item_id: m.id.clone(), source_rate: m.rate, source_width: m.width, source_height: m.height, source_duration: t.duration, has_audio: m.has_audio, sequence_pattern: m.sequence_pattern.clone(), sequence_start_frame: m.sequence_start_frame, sequence_end_frame: m.sequence_end_frame, sequence_exr_layer: m.sequence_exr_layer.clone(), sequence_missing: m.sequence_missing.clone(), ..Clip::empty() });
        t.tracks.push(track);
        self.timeline = t; self.playlist_media_item_id.clear(); self.undo.clear(); self.redo.clear(); self.changed();
    }
    /// Dual view (row 211): tracks A and B at the master rate, each one clip spanning its side's media (QCView's
    /// `loadSingleMedia` + `loadSecondarySource`); an empty side has an empty track.
    pub fn load_dual(&mut self, a: Option<&MediaInfo>, b: Option<&MediaInfo>, master_rate: Rate) {
        let mut t = Timeline { name: "Dual".into(), mode: SourceMode::Dual, policy: EditPolicy { ripple: false, inter_clip_gap: 0, allow_overlaps: false }, rate: master_rate, ..Default::default() };
        for (id, m) in [(TRACK_A, a), (TRACK_B, b)] { let mut tr = Track::video(id, false); if let Some(m) = m { tr.clips.push(full_clip(m, master_rate)); } t.tracks.push(tr); }
        t.recalculate_duration();
        self.timeline = t; self.playlist_media_item_id.clear(); self.undo.clear(); self.redo.clear(); self.changed();
    }
    /// Dual: a side's media changed in place (row 211 c) — that track becomes one full clip (or empty); the other keeps its
    /// edits; the undo history goes (its entries may name the old clips).
    pub fn set_track_media(&mut self, track: &str, m: Option<&MediaInfo>) {
        let rate = self.timeline.rate;
        if let Some(t) = self.track_mut_pub(track) { t.clips = m.map(|m| vec![full_clip(m, rate)]).unwrap_or_default(); }
        self.timeline.duration = 0; self.timeline.recalculate_duration();
        self.undo.clear(); self.redo.clear(); self.changed();
    }
    /// Dual: replace a track's clips wholesale (a saved Dual View's `clipsA` / `clipsB`); not an undo entry.
    pub fn set_track_clips(&mut self, track: &str, clips: Vec<Clip>) {
        if let Some(t) = self.track_mut_pub(track) { t.clips = clips; t.clips.sort_by_key(|c| c.start); }
        self.timeline.duration = 0; self.timeline.recalculate_duration(); self.changed();
    }
    /// Dual: the master rate changed (a side with a higher rate arrived) — every position keeps its time.
    pub fn retime_master(&mut self, rate: Rate) {
        let old = self.timeline.rate; if old == rate { return }
        for t in &mut self.timeline.tracks { for c in &mut t.clips { c.start = rescale(c.start, old, rate); c.duration = if c.is_gap { rescale(c.duration, old, rate) } else { c.source_len_at(rate) }; } }
        self.timeline.rate = rate; self.timeline.duration = 0; self.timeline.recalculate_duration();
        self.undo.clear(); self.redo.clear(); self.changed();
    }
    fn track_mut_pub(&mut self, id: &str) -> Option<&mut Track> { self.timeline.tracks.iter_mut().find(|t| t.id == id) }
    /// Dual (row 212): move `clip` by `delta` frames — with every LATER clip on its track when `carry` (one version of the
    /// cut travels together), alone otherwise. The move is clamped so nothing crosses the clip before it (or frame 0) and,
    /// moving alone, the clip after it. One undo step. Returns the delta applied.
    pub fn shift_clips(&mut self, track: &str, clip: &str, delta: i64, carry: bool) -> i64 {
        if delta == 0 || !self.editable(track) { return 0 }
        let Some(t) = self.timeline.track(track) else { return 0 };
        let mut clips: Vec<&Clip> = t.clips.iter().filter(|c| !c.is_gap).collect(); clips.sort_by_key(|c| c.start);
        let Some(i) = clips.iter().position(|c| c.id == clip) else { return 0 };
        let lo = if i > 0 { clips[i - 1].end() } else { 0 };
        let moving: Vec<Clip> = if carry { clips[i..].iter().map(|c| (*c).clone()).collect() } else { vec![clips[i].clone()] };
        let mut d = delta.max(lo - clips[i].start);
        if !carry { if let Some(next) = clips.get(i + 1) { d = d.min(next.start - clips[i].end()); } }
        if d == 0 { return 0 }
        let edits: Vec<Edit> = moving.iter().map(|c| { let mut n = c.clone(); n.start += d; Edit::Replace { track: track.into(), clip: c.id.clone(), old: c.clone(), new: n, label: "Slide clip" } }).collect();
        if self.push(Edit::Batch { edits, label: "Slide clip" }) { d } else { 0 }
    }
    /// Dual (row 212): delete `clip` and pull the later clips on its track left by its length (Shift+Delete). One undo step.
    pub fn delete_and_close(&mut self, track: &str, clip: &str) -> bool {
        if !self.editable(track) { return false }
        let Some(t) = self.timeline.track(track) else { return false };
        let Some(index) = t.index_of(clip) else { return false };
        let gone = t.clips[index].clone();
        let mut edits = vec![Edit::Delete { track: track.into(), clip: gone.clone(), index }];
        for c in t.clips.iter().filter(|c| c.start >= gone.end() && !c.is_gap) { let mut n = c.clone(); n.start -= gone.duration; edits.push(Edit::Replace { track: track.into(), clip: c.id.clone(), old: c.clone(), new: n, label: "Delete clip" }); }
        self.push(Edit::Batch { edits, label: "Delete clip" })
    }
    /// A ripple-edited playlist at the master rate; `gap_frames` of gap between adjacent clips when > 0; a trim's None
    /// means the full source.
    pub fn load_playlist(&mut self, name: &str, items: &[MediaInfo], trims: &[InOutRange], master_rate: Rate, gap_frames: i64, canvas: (u32, u32), playlist_media_item_id: &str) {
        let mut t = Timeline { name: name.into(), mode: SourceMode::Playlist, policy: EditPolicy { ripple: true, inter_clip_gap: gap_frames.max(0), allow_overlaps: false }, rate: master_rate, canvas_width: if canvas.0 > 0 { canvas.0 } else { 1920 }, canvas_height: if canvas.1 > 0 { canvas.1 } else { 1080 }, ..Default::default() };
        let mut track = Track::video(TRACK_A, false);
        let mut cursor = 0;
        for (i, m) in items.iter().enumerate() {
            let r = trims.get(i).copied().unwrap_or_default();
            let src_in = r.in_point.unwrap_or(0).max(0); let src_out = r.out_point.unwrap_or(m.duration);
            let mut c = Clip { id: fresh_id(), name: m.name.clone(), start: cursor, source_in: src_in, source_out: src_out, media_kind: m.kind, media_path: m.path.clone(), media_item_id: m.id.clone(), source_rate: m.rate, source_width: m.width, source_height: m.height, source_duration: m.duration, has_audio: m.has_audio, sequence_pattern: m.sequence_pattern.clone(), sequence_start_frame: m.sequence_start_frame, sequence_end_frame: m.sequence_end_frame, sequence_exr_layer: m.sequence_exr_layer.clone(), sequence_missing: m.sequence_missing.clone(), ..Clip::empty() };
            c.duration = c.source_len_at(master_rate);
            cursor += c.duration; track.clips.push(c);
            if gap_frames > 0 && i + 1 < items.len() { let mut g = Clip::gap(gap_frames); g.start = cursor; cursor += gap_frames; track.clips.push(g); }
        }
        t.duration = cursor; t.tracks.push(track);
        self.timeline = t; self.playlist_media_item_id = playlist_media_item_id.into(); self.undo.clear(); self.redo.clear(); self.changed();
    }
    pub fn clear(&mut self) { self.timeline = Timeline::default(); self.playlist_media_item_id.clear(); self.undo.clear(); self.redo.clear(); self.changed(); }
    /// Retune the rate without rebuilding (the image-sequence inspector): the frame count stays, the first clip follows.
    /// Returns the position fraction the caller rescales the clock by.
    pub fn set_rate(&mut self, rate: Rate) {
        if rate == self.timeline.rate { return }
        let frames = self.timeline.duration;
        self.timeline.rate = rate; self.timeline.duration = frames;
        if let Some(c) = self.timeline.tracks.first_mut().and_then(|t| t.clips.first_mut()) { c.duration = frames; c.source_out = frames; c.source_rate = rate; c.source_duration = frames; }
        self.changed();
    }

    // ---- the edit ops (each pushes one undo entry and applies) --------------------------------------------------
    fn editable(&self, track: &str) -> bool { self.timeline.track(track).map(|t| !t.locked).unwrap_or(false) }
    fn clip_copy(&self, track: &str, clip: &str) -> Option<Clip> { self.timeline.track(track)?.clip(clip).cloned() }
    /// Source bounds (row 84): in ≥ 0, out ≤ the source duration when known, out ≥ in.
    fn clamp_source(c: &mut Clip) {
        c.source_in = c.source_in.max(0);
        if c.source_duration > 0 { c.source_out = c.source_out.min(c.source_duration); c.source_in = c.source_in.min(c.source_duration); }
        c.source_out = c.source_out.max(c.source_in);
    }
    fn overlaps(&self, track: &str, candidate: &Clip) -> bool {
        if self.timeline.policy.allow_overlaps || self.timeline.mode == SourceMode::Playlist { return false }   // playlists re-flow contiguously
        self.timeline.track(track).map(|t| t.clips.iter().any(|c| c.id != candidate.id && !c.is_gap && c.start < candidate.end() && candidate.start < c.end())).unwrap_or(false)
    }
    fn push(&mut self, e: Edit) -> bool { if !self.apply(&e) { return false } self.undo.push(e); self.redo.clear(); true }
    fn replace(&mut self, track: &str, clip: &str, new: Clip, label: &'static str) -> bool {
        if !self.editable(track) { return false }
        let Some(old) = self.clip_copy(track, clip) else { return false };
        if self.overlaps(track, &new) { return false }
        self.push(Edit::Replace { track: track.into(), clip: clip.into(), old, new, label })
    }
    /// The head moves to `new_start` with `new_source_in`; the end stays (duration = old end − new start).
    pub fn trim_clip_head(&mut self, track: &str, clip: &str, new_start: i64, new_source_in: i64) -> bool {
        let Some(old) = self.clip_copy(track, clip) else { return false };
        let mut n = old.clone(); n.start = new_start.max(0); n.source_in = new_source_in; Self::clamp_source(&mut n); n.duration = (old.end() - n.start).max(0);
        self.replace(track, clip, n, "Trim head")
    }
    pub fn trim_clip_tail(&mut self, track: &str, clip: &str, new_source_out: i64) -> bool {
        let Some(old) = self.clip_copy(track, clip) else { return false };
        let mut n = old.clone(); n.source_out = new_source_out; Self::clamp_source(&mut n); n.duration = n.source_len_at(self.timeline.rate);
        self.replace(track, clip, n, "Trim tail")
    }
    pub fn slide_clip(&mut self, track: &str, clip: &str, new_start: i64) -> bool {
        let Some(old) = self.clip_copy(track, clip) else { return false };
        let mut n = old; n.start = new_start.max(0);
        self.replace(track, clip, n, "Slide clip")
    }
    /// Shift the source window; position and duration unchanged.
    pub fn slip_clip(&mut self, track: &str, clip: &str, new_source_in: i64, new_source_out: i64) -> bool {
        let Some(old) = self.clip_copy(track, clip) else { return false };
        let mut n = old; n.source_in = new_source_in.max(0); n.source_out = new_source_out.max(n.source_in); Self::clamp_source(&mut n);
        self.replace(track, clip, n, "Slip clip")
    }
    /// Cut the clip containing `at` (strictly inside) into [start, at) and [at, end); both halves share the media.
    pub fn split_clip_at(&mut self, track: &str, at: i64) -> bool {
        if !self.editable(track) { return false }
        let Some(old) = self.timeline.track(track).and_then(|t| t.clips.iter().find(|c| !c.is_gap && at > c.start && at < c.end()).cloned()) else { return false };
        let off = at - old.start; let src_off = rescale(off, self.timeline.rate, old.source_rate);
        let mut first = old.clone(); first.duration = off; first.source_out = old.source_in + src_off;
        let mut second = old.clone(); second.id = fresh_id(); second.start = at; second.duration = old.duration - off; second.source_in = old.source_in + src_off;
        let id = old.id.clone();
        self.push(Edit::Split { track: track.into(), clip: id, old, first, second })
    }
    /// Surrounding clips don't shift in single / dual mode (no ripple); playlists re-flow.
    pub fn delete_clip(&mut self, track: &str, clip: &str) -> bool {
        if !self.editable(track) { return false }
        let Some(t) = self.timeline.track(track) else { return false };
        let Some(index) = t.index_of(clip) else { return false };
        let c = t.clips[index].clone();
        self.push(Edit::Delete { track: track.into(), clip: c, index })
    }
    /// Slot-based reorder (insert-AT numbering, the same as `snap_edge_index_at`); no-op into the clip's own slots.
    pub fn move_clip_to_index(&mut self, track: &str, clip: &str, new_index: usize) -> bool {
        if !self.editable(track) { return false }
        let Some(t) = self.timeline.track(track) else { return false };
        let Some(from) = t.index_of(clip) else { return false };
        let to = new_index.min(t.clips.len());
        if to == from || to == from + 1 { return false }
        self.push(Edit::Move { track: track.into(), from, to })
    }
    /// Drop-to-add at a clip-array index (past the end appends); the track re-flows contiguously. Returns the new id.
    pub fn insert_clip_at_index(&mut self, track: &str, at_index: usize, m: &MediaInfo, trim: InOutRange) -> Option<String> {
        if !self.editable(track) || m.path.is_empty() || m.kind == ClipMediaKind::Audio { return None }   // audio media is not allowed in playlists
        let mut c = Clip { id: fresh_id(), name: m.name.clone(), source_in: trim.in_point.unwrap_or(0).max(0), source_out: trim.out_point.unwrap_or(m.duration), media_kind: m.kind, media_path: m.path.clone(), media_item_id: m.id.clone(), source_rate: if m.rate.num > 0 { m.rate } else { self.timeline.rate }, source_width: m.width, source_height: m.height, source_duration: m.duration, has_audio: m.has_audio, sequence_pattern: m.sequence_pattern.clone(), sequence_start_frame: m.sequence_start_frame, sequence_end_frame: m.sequence_end_frame, sequence_exr_layer: m.sequence_exr_layer.clone(), sequence_missing: m.sequence_missing.clone(), ..Clip::empty() };
        c.duration = c.source_len_at(self.timeline.rate);
        if c.duration <= 0 { return None }
        let idx = at_index.min(self.timeline.track(track)?.clips.len());
        let id = c.id.clone();
        if self.push(Edit::Insert { track: track.into(), index: idx, clip: c }) { Some(id) } else { None }
    }
    /// A dropped file's metadata arrived: patch the source duration + rate IN PLACE when the clip still uses the full
    /// range (never a trimmed one), then re-flow. Not an undo entry (QCView patches directly).
    pub fn patch_clip_duration_from_source(&mut self, track: &str, clip: &str, new_source_duration: i64, new_rate: Option<Rate>) -> bool {
        if new_source_duration <= 0 { return false }
        let rate = self.timeline.rate;
        let Some(c) = self.timeline.track_mut(track).and_then(|t| t.clips.iter_mut().find(|c| c.id == clip)) else { return false };
        let full = c.source_in == 0 && (c.source_out <= 0 || c.source_out == c.source_duration);
        if !full { return false }
        if c.source_duration == new_source_duration && new_rate.map(|r| r == c.source_rate).unwrap_or(true) { return false }
        c.source_duration = new_source_duration; c.source_out = new_source_duration; if let Some(r) = new_rate { c.source_rate = r; }
        c.duration = c.source_len_at(rate);
        self.timeline.recompute_contiguous_start_times(track); self.changed();
        true
    }

    // ---- live preview (no undo entry) + Esc restore -------------------------------------------------------------
    /// Direct manipulation during a drag; the release pushes ONE command. The snapshot the gesture took restores on Esc.
    pub fn preview_clip(&mut self, track: &str, clip: &str, state: Clip) -> bool { if !self.editable(track) { return false } self.apply_replacement(track, clip, state) }
    pub fn restore_clip(&mut self, track: &str, clip: &str, snapshot: Clip) -> bool { self.apply_replacement(track, clip, snapshot) }

    pub fn can_undo(&self) -> bool { !self.undo.is_empty() }
    pub fn can_redo(&self) -> bool { !self.redo.is_empty() }
    pub fn undo_label(&self) -> Option<&'static str> { self.undo.last().map(label) }
    pub fn undo_edit(&mut self) -> bool { let Some(e) = self.undo.pop() else { return false }; self.invert(&e); self.redo.push(e); true }
    pub fn redo_edit(&mut self) -> bool { let Some(e) = self.redo.pop() else { return false }; self.apply(&e); self.undo.push(e); true }
    pub fn clear_undo_stack(&mut self) { self.undo.clear(); self.redo.clear(); }

    // ---- the mutators -----------------------------------------------------------------------------------------
    fn apply(&mut self, e: &Edit) -> bool {
        match e {
            Edit::Replace { track, clip, new, .. } => self.apply_replacement(track, clip, new.clone()),
            Edit::Split { track, clip, first, second, .. } => { self.apply_replacement(track, clip, first.clone()) && self.apply_insert_after(track, clip, second.clone()) }
            Edit::Delete { track, clip, .. } => self.apply_remove(track, &clip.id),
            Edit::Insert { track, index, clip } => { let ok = self.apply_insert_at(track, *index, clip.clone()); self.timeline.recompute_contiguous_start_times(track); self.changed(); ok }
            Edit::Move { track, from, to } => self.move_by_slot(track, *from, *to),
            Edit::Batch { edits, .. } => edits.iter().fold(true, |ok, e| self.apply(e) && ok),
        }
    }
    fn invert(&mut self, e: &Edit) -> bool {
        match e {
            Edit::Replace { track, clip, old, .. } => self.apply_replacement(track, clip, old.clone()),
            Edit::Split { track, clip, old, second, .. } => { self.apply_remove(track, &second.id) && self.apply_replacement(track, clip, old.clone()) }
            Edit::Delete { track, clip, index } => self.apply_insert_at(track, *index, clip.clone()),
            Edit::Insert { track, clip, .. } => { let ok = self.apply_remove(track, &clip.id); self.timeline.recompute_contiguous_start_times(track); self.changed(); ok }
            // QCView's MoveClipEdit::undo moves the clip back to slot `from` — right for forward moves, wrong for backward ones
            // (pre-remove numbering: the clip now sits BEFORE its old slot, so the old slot is `from + 1`; [x, y] moved y → 0
            // never undid at all, the slot being its own + 1). Fixed here and listed for QCView (§17.6).
            Edit::Batch { edits, .. } => edits.iter().rev().fold(true, |ok, e| self.invert(e) && ok),
            Edit::Move { track, from, to } => { if to == from || *to == from + 1 { return false } let post = if to > from { to - 1 } else { *to }; let back = if to > from { *from } else { from + 1 }; self.move_by_slot(track, post, back) }
        }
    }
    fn apply_replacement(&mut self, track: &str, clip: &str, new: Clip) -> bool {
        let mode = self.timeline.mode;
        let Some(c) = self.timeline.track_mut(track).and_then(|t| t.clips.iter_mut().find(|c| c.id == clip)) else { return false };
        *c = new;
        if mode == SourceMode::Playlist { self.timeline.recompute_contiguous_start_times(track); } else { self.timeline.recalculate_duration(); }
        self.changed(); true
    }
    fn apply_insert_at(&mut self, track: &str, index: usize, clip: Clip) -> bool {
        let Some(t) = self.timeline.track_mut(track) else { return false };
        let i = index.min(t.clips.len()); t.clips.insert(i, clip);
        self.timeline.recalculate_duration(); self.changed(); true
    }
    fn apply_insert_after(&mut self, track: &str, after: &str, clip: Clip) -> bool {
        let Some(i) = self.timeline.track(track).and_then(|t| t.index_of(after)) else { return false };
        self.apply_insert_at(track, i + 1, clip)
    }
    fn apply_remove(&mut self, track: &str, clip: &str) -> bool {
        let Some(t) = self.timeline.track_mut(track) else { return false };
        let Some(i) = t.index_of(clip) else { return false };
        t.clips.remove(i);
        if self.timeline.mode == SourceMode::Playlist { self.timeline.recompute_contiguous_start_times(track); } else { self.timeline.recalculate_duration(); }
        self.changed(); true
    }
    fn move_by_slot(&mut self, track: &str, from: usize, to: usize) -> bool {
        if from == to || to == from + 1 { return false }
        let Some(t) = self.timeline.track_mut(track) else { return false };
        if from >= t.clips.len() { return false }
        let c = t.clips.remove(from);
        let adj = if to > from { to - 1 } else { to };   // pre-remove → post-remove index
        t.clips.insert(adj.min(t.clips.len()), c);
        if self.timeline.mode == SourceMode::Playlist { self.timeline.recompute_contiguous_start_times(track); } else { self.timeline.recalculate_duration(); }
        self.changed(); true
    }
}
fn label(e: &Edit) -> &'static str { match e { Edit::Replace { label, .. } => label, Edit::Split { .. } => "Cut clip", Edit::Delete { .. } => "Delete clip", Edit::Insert { .. } => "Add clip", Edit::Move { .. } => "Reorder clip", Edit::Batch { label, .. } => label } }

/// One clip spanning `m` at `rate` from frame 0 (the dual loaders).
fn full_clip(m: &MediaInfo, rate: Rate) -> Clip {
    let mut c = Clip { id: fresh_id(), name: m.name.clone(), start: 0, source_in: 0, source_out: m.duration, media_kind: m.kind, media_path: m.path.clone(), media_item_id: m.id.clone(), source_rate: m.rate, source_width: m.width, source_height: m.height, source_duration: m.duration, has_audio: m.has_audio, sequence_pattern: m.sequence_pattern.clone(), sequence_start_frame: m.sequence_start_frame, sequence_end_frame: m.sequence_end_frame, sequence_exr_layer: m.sequence_exr_layer.clone(), sequence_missing: m.sequence_missing.clone(), ..Clip::empty() };
    c.duration = c.source_len_at(rate); c
}

/// A fresh clip id (QCView: a UUID without braces; the project file carries them as strings).
pub fn fresh_id() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(0);
    let n = N.fetch_add(1, Ordering::Relaxed);
    let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos() as u64).unwrap_or(0);
    let mut x = t ^ (n.wrapping_mul(0x9E37_79B9_7F4A_7C15)) ^ (std::process::id() as u64) << 32;
    x ^= x << 13; x ^= x >> 7; x ^= x << 17;
    format!("{:08x}-{:04x}-4{:03x}-{:04x}-{:012x}", (x >> 32) as u32, (x >> 16) as u16, (x & 0xfff) as u16, ((t >> 20) & 0xffff) as u16, (t ^ x.rotate_left(29)) & 0xffff_ffff_ffff)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn media(name: &str, frames: i64, rate: Rate) -> MediaInfo { MediaInfo { id: name.into(), name: name.into(), path: format!("/m/{name}.mov"), kind: ClipMediaKind::Video, rate, duration: frames, width: 1920, height: 1080, has_audio: true, sequence_pattern: String::new(), sequence_start_frame: 1, sequence_end_frame: 1, sequence_exr_layer: String::new(), sequence_missing: vec![] } }
    #[test] fn rescale_round_trips_at_equal_rates() { let r = Rate::from_f64(23.976); let t = RationalTime::new(12345, r); assert_eq!(t.rescaled_to(r), t); assert_eq!(r, Rate { num: 24000, den: 1001 }); }
    #[test] fn exact_rates_from_rationals() { assert_eq!(Rate::from_rational(24000, 1001), Some(Rate::new(24000, 1001))); assert_eq!(Rate::from_rational(0, 1), None); assert_eq!(Rate::from_f64(25.0), Rate::new(25, 1)); assert_eq!(Rate::from_f64(23.5), Rate::new(23500, 1000)); }
    #[test] fn mixed_rates_snap() { let a = Rate::new(24, 1); let b = Rate::new(30, 1); assert_eq!(rescale(24, a, b), 30); assert_eq!(rescale(1, a, b), 1); assert_eq!(rescale(7, b, a), 6); }
    #[test] fn playlist_resolve_is_half_open_and_gaps_skip() {
        let r = Rate::new(24, 1); let mut e = Editor::default();
        e.load_playlist("p", &[media("a", 10, r), media("b", 5, r)], &[InOutRange::default(), InOutRange { in_point: Some(100), out_point: Some(105) }], r, 2, (0, 0), "pl");
        let t = &e.timeline; assert_eq!(t.duration, 17); assert_eq!(t.tracks[0].clips.len(), 3); assert!(t.tracks[0].clips[1].is_gap);
        assert_eq!(t.resolve(9).map(|(c, s)| (c.name.as_str(), s)), Some(("a", 9))); assert!(t.resolve(10).is_none()); assert_eq!(t.resolve(12).map(|(c, s)| (c.name.as_str(), s)), Some(("b", 100)));
        assert_eq!(t.boundaries(), vec![10, 12]); assert_eq!(t.next_clip_start(0), Some(12)); assert_eq!(t.prev_clip_start(12), Some(0));
        assert_eq!(t.audible_clips_at(13).len(), 1);
    }
    #[test] fn edits_apply_and_invert() {
        let r = Rate::new(24, 1); let mut e = Editor::default();
        e.load_playlist("p", &[media("a", 48, r), media("b", 24, r), media("c", 12, r)], &[], r, 0, (0, 0), "");
        let ids: Vec<String> = e.timeline.tracks[0].clips.iter().map(|c| c.id.clone()).collect();
        assert!(e.trim_clip_head(TRACK_A, &ids[0], 10, 10)); assert_eq!(e.timeline.tracks[0].clips[0].duration, 38); assert_eq!(e.timeline.tracks[0].clips[1].start, 38);   // ripple
        assert!(e.trim_clip_tail(TRACK_A, &ids[1], 12)); assert_eq!(e.timeline.duration, 38 + 12 + 12);
        assert!(e.split_clip_at(TRACK_A, 5)); assert_eq!(e.timeline.tracks[0].clips.len(), 4); assert_eq!(e.timeline.tracks[0].clips[1].source_in, 15); assert_eq!(e.timeline.tracks[0].clips[0].source_out, 15);
        assert!(e.move_clip_to_index(TRACK_A, &ids[2], 0)); assert_eq!(e.timeline.tracks[0].clips[0].name, "c"); assert_eq!(e.timeline.tracks[0].clips[0].start, 0);
        assert!(e.delete_clip(TRACK_A, &ids[1])); assert_eq!(e.timeline.tracks[0].clips.len(), 3);
        let after = e.timeline.clone();
        for _ in 0..5 { assert!(e.undo_edit()); }
        assert_eq!(e.timeline.tracks[0].clips.len(), 3); assert_eq!(e.timeline.tracks[0].clips[0].duration, 48); assert_eq!(e.timeline.duration, 84);
        for _ in 0..5 { assert!(e.redo_edit()); }
        assert_eq!(e.timeline, after);
        let r = Rate::new(24, 1); let mut e2 = Editor::default(); e2.load_playlist("q", &[media("x", 5, r), media("y", 5, r)], &[], r, 0, (0, 0), "");
        let y = e2.timeline.tracks[0].clips[1].id.clone(); assert!(e2.move_clip_to_index(TRACK_A, &y, 0)); assert_eq!(e2.timeline.tracks[0].clips[0].name, "y"); assert!(e2.undo_edit()); assert_eq!(e2.timeline.tracks[0].clips[0].name, "x");   // the backward-move undo QCView gets wrong
    }
    #[test] fn policy_locked_bounds_snap_patch() {
        let r = Rate::new(24, 1); let mut e = Editor::default();
        e.load_single_media(&media("s", 100, r)); let id = e.timeline.tracks[0].clips[0].id.clone();
        assert!(!e.slide_clip(TRACK_A, &id, 5));   // locked
        e.load_playlist("p", &[media("a", 48, r), media("b", 24, r)], &[], r, 0, (0, 0), "");
        let id = e.timeline.tracks[0].clips[0].id.clone();
        assert!(e.slip_clip(TRACK_A, &id, -5, 300)); let c = &e.timeline.tracks[0].clips[0]; assert_eq!((c.source_in, c.source_out), (0, 48));   // clamped to the source
        assert_eq!(e.timeline.snap_edge_index_at(TRACK_A, 47, 3), Some(1)); assert_eq!(e.timeline.snap_edge_index_at(TRACK_A, 2, 3), Some(0)); assert_eq!(e.timeline.snap_edge_index_at(TRACK_A, 30, 3), None); assert_eq!(e.timeline.snap_edge_index_at(TRACK_A, 71, 3), Some(2));
        let dropped = MediaInfo { duration: 0, ..media("d", 0, r) };
        let nid = e.insert_clip_at_index(TRACK_A, 9, &dropped, InOutRange::default()); assert!(nid.is_none());   // a zero-length drop is refused
        let nid = e.insert_clip_at_index(TRACK_A, 9, &media("d", 10, r), InOutRange::default()).unwrap();
        assert_eq!(e.timeline.tracks[0].clips[2].id, nid); assert_eq!(e.timeline.tracks[0].clips[2].start, 72);
        assert!(e.patch_clip_duration_from_source(TRACK_A, &nid, 30, None)); assert_eq!(e.timeline.duration, 102);
        assert!(e.trim_clip_tail(TRACK_A, &nid, 20)); assert!(!e.patch_clip_duration_from_source(TRACK_A, &nid, 40, None));   // trimmed: never patched
        let snap = e.timeline.tracks[0].clips[0].clone(); let mut moved = snap.clone(); moved.start = 7;
        assert!(e.preview_clip(TRACK_A, &snap.id, moved)); assert!(e.restore_clip(TRACK_A, &snap.id, snap.clone())); assert_eq!(e.timeline.tracks[0].clips[0], snap);
        assert_eq!(e.undo_label(), Some("Trim tail"));
    }
    #[test]
    fn dual_moves_carry_the_later_clips_and_undo_in_one_step() {
        let mut ed = Editor::default();
        ed.load_dual(Some(&media("a", 240, Rate::new(24, 1))), Some(&media("b", 300, Rate::new(30, 1))), Rate::new(30, 1));
        assert_eq!(ed.timeline.duration, 300, "the longer side at the master rate (A 10 s = 300, B 10 s = 300)");
        // ⌘K on B at 100 and 200: three clips
        assert!(ed.split_clip_at(TRACK_B, 100) && ed.split_clip_at(TRACK_B, 200));
        let ids: Vec<String> = ed.timeline.track(TRACK_B).unwrap().clips.iter().map(|c| c.id.clone()).collect();
        // the middle clip slides +12 carrying the third; the master grows to 312
        assert_eq!(ed.shift_clips(TRACK_B, &ids[1], 12, true), 12);
        let starts = |ed: &Editor| ed.timeline.track(TRACK_B).unwrap().clips.iter().map(|c| c.start).collect::<Vec<_>>();
        assert_eq!(starts(&ed), vec![0, 112, 212]); assert_eq!(ed.timeline.duration, 312);
        assert!(ed.undo_edit()); assert_eq!(starts(&ed), vec![0, 100, 200], "one undo step for the carried move");
        // left: clamped at the previous clip's end
        assert_eq!(ed.shift_clips(TRACK_B, &ids[1], -50, true), 0, "flush against clip 0 already");
        // alone (Shift): clamped by the next clip
        assert_eq!(ed.shift_clips(TRACK_B, &ids[0], 30, false), 0, "clip 0 is flush with clip 1");
        assert!(ed.redo_edit()); assert_eq!(ed.shift_clips(TRACK_B, &ids[1], 30, false), 0, "clip 1 is flush with clip 2: alone it cannot move right");
        assert_eq!(ed.shift_clips(TRACK_B, &ids[2], 30, false), 30, "the last clip moves alone freely"); assert!(ed.undo_edit());
        // Shift+Delete closes the gap: clip 1 goes, clip 2 pulls left by its length
        let before = starts(&ed);
        assert!(ed.delete_and_close(TRACK_B, &ids[1]));
        let after = starts(&ed); assert_eq!(after.len(), 2); assert_eq!(after[1], before[2] - 100);
        assert!(ed.undo_edit()); assert_eq!(starts(&ed), before);
        // A is untouched throughout
        assert_eq!(ed.timeline.track(TRACK_A).unwrap().clips.len(), 1);
    }
    #[test]
    fn a_side_timeline_is_its_track_at_its_own_rate_and_the_master_rate_change_keeps_times() {
        let mut ed = Editor::default();
        ed.load_dual(Some(&media("a", 240, Rate::new(24, 1))), Some(&media("b", 150, Rate::new(30, 1))), Rate::new(30, 1));
        assert!(ed.split_clip_at(TRACK_A, 150));   // master 150 = 5 s = A frame 120
        let id = ed.timeline.track(TRACK_A).unwrap().clips[1].id.clone();
        ed.shift_clips(TRACK_A, &id, 30, true);    // +1 s
        let side = ed.timeline.side_timeline(TRACK_A, Rate::new(24, 1));
        let c: Vec<(i64, i64, i64)> = side.tracks[0].clips.iter().map(|c| (c.start, c.duration, c.source_in)).collect();
        assert_eq!(c, vec![(0, 120, 0), (144, 120, 120)], "A at 24: the cut at 120, the second half from 6 s");
        assert_eq!(side.duration, 264);
        // the master drops to 24 (B left): every position keeps its time
        ed.retime_master(Rate::new(24, 1));
        let k: Vec<i64> = ed.timeline.track(TRACK_A).unwrap().clips.iter().map(|c| c.start).collect();
        assert_eq!(k, vec![0, 144]);
    }
    #[test] fn ids_are_unique() { let a = fresh_id(); let b = fresh_id(); assert_ne!(a, b); assert_eq!(a.len(), 36); }
}
