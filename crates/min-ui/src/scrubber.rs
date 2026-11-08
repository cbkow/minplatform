//! The scrubber — the timeline surface as ONE node drawing from the model each frame (canon §17.5 item 4, §25 row 166):
//! a 32-px band that IS the ruler. Ticks and labels along the top for the VISIBLE range only (QCView's rule: the largest
//! of 1 / 5 / 10 / 30 / 60 / 300 / 600 s that keeps ≥ 80 px; frame ticks once a frame is ≥ 6 px, frame numbers at
//! ≥ 24 px), the playhead through it, the in / out zone with its flags, the cache underline, the pennants (later: notes),
//! a 2-px extent marker when zoomed. The camera is QCView's (`pps` + `scrollX`: wheel zooms about the cursor by
//! `1.0015^dy` between fit and max(200 px/s, fit × 50), Alt-wheel pans, a page-turn follow at a 20-px margin while
//! playing); double-click fits (ours — QCView had the overview bar for that). The gesture is QCView's press / move /
//! release through the Transport (`begin_scrub` / `scrub_to_frame` / `end_scrub`): the playhead follows the cursor,
//! not the decode. The waveform is drawn from `peaks` when the audio probe provides them (step 2b) — never synthetic.
use crate::{input::{Button, Event}, paint, Family, Fonts};
use kurbo::{Affine, BezPath, Rect};
use min_theme::tokens as t;
use std::time::{Duration, Instant};
use vello::Scene;

/// What the node reads each frame (the Transport's view of the active timeline).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ScrubState {
    pub frames: i32, pub fps: f64, pub current: i32,
    pub in_point: i32, pub out_point: i32,
    pub playing: bool, pub scrubbing: bool,
    /// Decoded-ahead coverage in frames (behind, ahead) of the playhead, when known.
    pub cached: Option<(i32, i32)>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScrubCmd { Begin, To(i32), End }

const PPS_MAX: f64 = 200.0;           // QCView's kPpsMax (logical px/s)
const DOUBLE_PRESS_MS: u64 = 350;

pub struct Scrubber {
    /// Compact mode's line state (canon row 172; QCView's CompactStrip): a 2-px scrub line through the band's middle with the
    /// playhead and the in / out marks — no ruler, no labels, no waveform. The same gesture, the same camera.
    pub compact: bool,
    pub rect: Rect,
    /// px per second (physical); `None` = fit the whole duration to the track.
    pps: Option<f64>,
    scroll_x: f64,
    track_w: f64,
    pressed: bool,
    hover_x: Option<f64>,
    last_press: Option<(Instant, f64)>,
    /// The timeline frame under the resting pointer (the hover thumbnail's request), when any.
    pub hover_frame: Option<i32>,
    /// Frames an image sequence could not show (missing on disk / failed to decode), timeline frames, sorted —
    /// QCView's red ticks, ≥ 1 px each so a hole reads at any zoom. Not drawn in compact mode.
    pub failed: Vec<i32>,
    /// The cache band wraps around the whole timeline (looping) — QCView's split of the overflow to the far edge.
    pub cache_wraps: bool,
    /// The track's minimum end inset in logical px (default: the row padding). macOS rounds the window's bottom corners: the
    /// shell raises it past the corner radius so the timeline's ends (and the lanes below that share this camera) stay reachable.
    pub end_inset: f64,
}

impl Default for Scrubber { fn default() -> Self { Scrubber::new() } }

impl Scrubber {
    pub fn new() -> Scrubber { Scrubber { compact: false, rect: Rect::ZERO, pps: None, scroll_x: 0.0, track_w: 1.0, pressed: false, hover_x: None, last_press: None, hover_frame: None, failed: vec![], cache_wraps: false, end_inset: 0.0 } }
    pub fn hit(&self, x: f64, y: f64) -> bool { self.rect.contains(kurbo::Point::new(x, y)) }
    pub fn is_pressed(&self) -> bool { self.pressed }
    pub fn is_fit(&self) -> bool { self.pps.is_none() }
    fn pad(&self, scale: f64) -> f64 { (t::SPACING_100 as f64).max(self.end_inset) * scale }
    /// The track: the band inset by the row padding. A resize keeps the visible time range.
    pub fn layout(&mut self, rect: Rect, scale: f64) {
        let w = (rect.width() - 2.0 * self.pad(scale)).max(1.0);
        if (w - self.track_w).abs() > 0.5 && self.track_w > 1.0 { let k = w / self.track_w; if let Some(p) = self.pps.as_mut() { *p *= k; } self.scroll_x *= k; }
        self.rect = rect; self.track_w = w;
    }
    fn dur(&self, s: &ScrubState) -> f64 { if s.fps > 0.0 && s.frames > 0 { s.frames as f64 / s.fps } else { 0.0 } }
    fn fit_pps(&self, s: &ScrubState) -> f64 { let d = self.dur(s); if d > 0.0 { self.track_w / d } else { 0.0 } }
    fn pps(&self, s: &ScrubState) -> f64 { self.pps.unwrap_or_else(|| self.fit_pps(s)) }
    fn max_scroll(&self, s: &ScrubState) -> f64 { (self.dur(s) * self.pps(s) - self.track_w).max(0.0) }
    /// The x of timeline frame `f` on this scrubber's camera — the clip bar below draws on the same mapping.
    pub fn x_of_frame(&self, f: i64, s: &ScrubState, scale: f64) -> f64 { if s.fps > 0.0 { self.x_of_time(f as f64 / s.fps, s, scale) } else { self.rect.x0 + self.pad(scale) } }
    /// Frames per pixel on the camera (for gesture deltas).
    pub fn frames_per_px(&self, s: &ScrubState) -> f64 { let p = self.pps(s); if p > 0.0 { s.fps / p } else { 0.0 } }
    fn x_of_time(&self, secs: f64, s: &ScrubState, scale: f64) -> f64 { self.rect.x0 + self.pad(scale) + secs * self.pps(s) - self.scroll_x }
    fn time_at_x(&self, x: f64, s: &ScrubState, scale: f64) -> f64 { let p = self.pps(s); if p > 0.0 { ((x - self.rect.x0 - self.pad(scale)).clamp(0.0, self.track_w) + self.scroll_x) / p } else { 0.0 } }
    pub fn frame_at_x(&self, x: f64, s: &ScrubState, scale: f64) -> i32 { ((self.time_at_x(x, s, scale) * s.fps).round() as i64).clamp(0, (s.frames as i64 - 1).max(0)) as i32 }
    pub fn fit(&mut self) { self.pps = None; self.scroll_x = 0.0; }
    /// The playhead's x (the keyboard zoom's anchor — QCView's Ctrl+= / Ctrl+-).
    pub fn playhead_x(&self, s: &ScrubState, scale: f64) -> f64 { if s.fps > 0.0 { self.x_of_time(s.current as f64 / s.fps, s, scale) } else { self.rect.x0 + self.pad(scale) } }
    /// The visible time window (seconds) and the track's width in whole pixel columns — the waveform probe's request.
    pub fn visible_window(&self, s: &ScrubState) -> (f64, f64) { let p = self.pps(s); if p <= 0.0 { return (0.0, 0.0) } (self.scroll_x / p, (self.scroll_x + self.track_w) / p) }
    pub fn track_columns(&self) -> usize { self.track_w.max(1.0) as usize }
    /// QCView's `applyWheel`: `dy` in Qt angle units (120 per notch); `pan` = Alt held.
    pub fn wheel(&mut self, x: f64, dy: f64, pan: bool, s: &ScrubState, scale: f64) -> bool {
        if self.dur(s) <= 0.0 || dy == 0.0 { return false }
        if pan { let n = (self.scroll_x - dy).clamp(0.0, self.max_scroll(s)); let c = n != self.scroll_x; self.scroll_x = n; return c }
        let (fit, cur) = (self.fit_pps(s), self.pps(s));
        let max = (PPS_MAX * scale).max(fit * 50.0);
        let new = (cur * 1.0015f64.powf(dy)).clamp(fit, max);
        if (new - cur).abs() < 1e-9 { return false }
        let vx = (x - self.rect.x0 - self.pad(scale)).clamp(0.0, self.track_w);
        let cursor_t = (vx + self.scroll_x) / cur;
        self.pps = if (new - fit).abs() < 1e-9 { None } else { Some(new) };
        let max_scroll = (self.dur(s) * new - self.track_w).max(0.0);
        self.scroll_x = (cursor_t * new - vx).clamp(0.0, max_scroll);
        true
    }
    /// The page-turn follow during playback (QCView's `maybeFollowPlayhead`): true when the view moved.
    pub fn follow(&mut self, s: &ScrubState, scale: f64) -> bool {
        if !s.playing || self.pressed || self.pps.is_none() || s.fps <= 0.0 { return false }
        let margin = t::SCRUBBER_FOLLOW_MARGIN as f64 * scale;
        let px = s.current as f64 / s.fps * self.pps(s) - self.scroll_x;
        if px < margin || px > self.track_w - margin { let n = (s.current as f64 / s.fps * self.pps(s) - margin).clamp(0.0, self.max_scroll(s)); let c = n != self.scroll_x; self.scroll_x = n; return c }
        false
    }
    /// Press scrubs (and pauses, through the Transport), move while pressed scrubs, release seeks; a resting pointer sets
    /// `hover_frame`. Returns the commands and whether the node needs a redraw.
    pub fn event(&mut self, e: &Event, now: Instant, s: &ScrubState, scale: f64) -> (Vec<ScrubCmd>, bool) {
        let mut cmds = vec![]; let mut dirty = false;
        let loaded = s.frames > 0 && s.fps > 0.0;
        match *e {
            Event::Move { x, y } => {
                if self.pressed { cmds.push(ScrubCmd::To(self.frame_at_x(x, s, scale))); dirty = true; }
                let inside = self.hit(x, y);
                let hx = if inside && loaded && !self.pressed { Some(x) } else { None };
                if hx != self.hover_x { self.hover_x = hx; dirty = true; }
                self.hover_frame = hx.map(|x| self.frame_at_x(x, s, scale));
            }
            Event::Down { x, y, button: Button::Primary } if self.hit(x, y) && loaded => {
                if let Some((t0, px)) = self.last_press { if now.duration_since(t0) < Duration::from_millis(DOUBLE_PRESS_MS) && (px - x).abs() < 4.0 * scale { self.fit(); self.last_press = None; return (vec![], true) } }
                self.last_press = Some((now, x));
                self.pressed = true; self.hover_x = None; self.hover_frame = None;
                cmds.push(ScrubCmd::Begin); cmds.push(ScrubCmd::To(self.frame_at_x(x, s, scale))); dirty = true;
            }
            Event::Down { .. } => {}
            Event::Up { x, button: Button::Primary, .. } => { if self.pressed { self.pressed = false; cmds.push(ScrubCmd::To(self.frame_at_x(x, s, scale))); cmds.push(ScrubCmd::End); dirty = true; } }
            Event::Up { .. } => {}
            // Leaving the window while pressed keeps the grab (Qt's MouseArea kept it for QCView): drags outside keep
            // arriving as moves, clamped to the track, and the release ends the scrub wherever it lands.
            Event::Leave => { if self.hover_x.is_some() { dirty = true; } self.hover_x = None; self.hover_frame = None; }
        }
        (cmds, dirty)
    }

    /// `peaks`: optional (min, max) in −1..1 per TRACK pixel column for the visible range (the audio probe's memo).
    pub fn paint(&self, scene: &mut Scene, fonts: &mut Fonts, scale: f64, s: &ScrubState, peaks: Option<&[(f32, f32)]>) {
        let r = self.rect; let hair = t::HAIRLINE as f64 * scale;
        paint::rect(scene, r, t::SCRUBBER_BACKGROUND_COLOR);
        paint::hline(scene, r.x0, r.x1, r.y0, hair, t::DIVIDER_COLOR);
        let dur = self.dur(s);
        if dur <= 0.0 { return }
        let pps = self.pps(s);
        let (tx0, tx1) = (r.x0 + self.pad(scale), r.x0 + self.pad(scale) + self.track_w);
        let top = r.y0 + hair;
        if self.compact {
            // the line through the middle (brighter while hovered or pressed), the in / out marks, the playhead
            let cy = (r.y0 + r.y1) / 2.0; let lh = 2.0 * scale;
            paint::rect(scene, Rect::new(tx0, cy - lh / 2.0, tx1, cy + lh / 2.0), if self.hover_x.is_some() || self.pressed { t::BORDER_COLOR_HOVER } else { t::DIVIDER_COLOR });
            let mark_h = 10.0 * scale;
            if s.in_point >= 0 { let a = self.x_of_time(s.in_point as f64 / s.fps, s, scale); if a >= tx0 - 1.0 && a <= tx1 + 1.0 { paint::rect(scene, Rect::new(a, cy - mark_h / 2.0, a + lh, cy + mark_h / 2.0), t::SCRUBBER_IN_POINT_COLOR); } }
            if s.out_point >= 0 { let b = self.x_of_time((s.out_point + 1) as f64 / s.fps, s, scale); if b >= tx0 - 1.0 && b <= tx1 + 1.0 { paint::rect(scene, Rect::new(b - lh, cy - mark_h / 2.0, b, cy + mark_h / 2.0), t::SCRUBBER_OUT_POINT_COLOR); } }
            let px = self.x_of_time(s.current as f64 / s.fps, s, scale); let ph = 12.0 * scale;
            if px >= tx0 - 1.0 && px <= tx1 + 1.0 { paint::rect(scene, Rect::new(px - 1.5 * scale, cy - ph / 2.0, px + 1.5 * scale, cy + ph / 2.0), t::SCRUBBER_PLAYHEAD_COLOR); }
            return
        }
        // the waveform as the band's quiet background (the probe's peaks, when there are any)
        if let Some(pk) = peaks {
            let wave_h = r.height() * t::SCRUBBER_WAVEFORM_HEIGHT_RATIO; let base = r.y1;
            let mut path = BezPath::new(); path.move_to((tx0, base));
            for (i, (mn, mx)) in pk.iter().enumerate() { let a = mn.abs().max(mx.abs()).min(1.0) as f64; path.line_to((tx0 + i as f64, base - a * wave_h)); }
            path.line_to((tx0 + pk.len() as f64, base)); path.close_path();
            scene.fill(peniko::Fill::NonZero, Affine::IDENTITY, paint::color(t::SCRUBBER_WAVEFORM_COLOR), None, &path);
        }
        // ticks for the visible range
        let (t_start, t_end) = (self.scroll_x / pps, (self.scroll_x + self.track_w) / pps);
        let candidates = [1.0, 5.0, 10.0, 30.0, 60.0, 300.0, 600.0];
        let tick_s = candidates.iter().cloned().find(|c| c * pps >= t::SCRUBBER_TICK_MIN_SPACING as f64 * scale).unwrap_or(600.0);
        let px_per_frame = pps / s.fps;
        let frame_ticks = px_per_frame >= t::SCRUBBER_FRAME_TICK_MIN_PX_PER_FRAME as f64 * scale;
        let frame_labels = px_per_frame >= 24.0 * scale;
        let (major, minor) = (t::SCRUBBER_TICK_MAJOR_LENGTH as f64 * scale, t::SCRUBBER_TICK_MINOR_LENGTH as f64 * scale);
        let label_px = t::SCRUBBER_TICK_LABEL_SIZE * scale as f32;
        if frame_ticks {
            let (f0, f1) = ((t_start * s.fps).floor().max(0.0) as i64, ((t_end * s.fps).ceil() as i64).min(s.frames as i64 - 1));
            for f in f0..=f1 {
                let x = self.x_of_time(f as f64 / s.fps, s, scale);
                paint::rect(scene, Rect::new(x, top, x + hair, top + minor), t::SCRUBBER_TICK_COLOR);
                // the per-frame numbers sit behind the band's content: a step dimmer than the ticks (the user 2026-11-07: "not so bright")
                if frame_labels { let l = fonts.layout(&f.to_string(), Family::Mono, label_px, t::FONT_WEIGHT_REGULAR as f32, paint::color(t::SCRUBBER_FRAME_LABEL_COLOR)); crate::fonts::draw_layout(scene, &l, Affine::translate((x + t::SPACING_50 as f64 * scale, top + minor))); }
            }
        }
        let mut ts = (t_start / tick_s).floor() * tick_s;
        while ts <= t_end + 1e-9 && ts <= dur + 1e-9 {
            let x = self.x_of_time(ts, s, scale);
            if x >= tx0 - 1.0 && x <= tx1 + 1.0 {
                paint::rect(scene, Rect::new(x, top, x + hair, top + major), t::SCRUBBER_TICK_COLOR);
                let l = fonts.layout(&ruler_label(ts), Family::Mono, label_px, t::FONT_WEIGHT_REGULAR as f32, paint::color(t::SCRUBBER_TICK_LABEL_COLOR));
                crate::fonts::draw_layout(scene, &l, Affine::translate((x + t::SPACING_75 as f64 * scale, top + t::SPACING_50 as f64 * scale)));
            }
            ts += tick_s;
        }
        // the in / out zone and flags (QCView: the zone spans in → out + 1)
        if s.in_point >= 0 || s.out_point >= 0 {
            let ix = if s.in_point >= 0 { Some(self.x_of_time(s.in_point as f64 / s.fps, s, scale)) } else { None };
            let ox = if s.out_point >= 0 { Some(self.x_of_time((s.out_point + 1) as f64 / s.fps, s, scale)) } else { None };
            if let (Some(a), Some(b)) = (ix, ox) { if b > a { paint::rect(scene, Rect::new(a.max(tx0), top, b.min(tx1), r.y1), t::SCRUBBER_IN_OUT_ZONE_COLOR); } }
            let (fw, fh) = (t::SCRUBBER_FLAG_WIDTH as f64 * scale, t::SCRUBBER_FLAG_HEIGHT as f64 * scale);
            if let Some(a) = ix { paint::rect(scene, Rect::new(a, top, a + hair, r.y1), t::SCRUBBER_IN_POINT_COLOR); paint::rect(scene, Rect::new(a, r.y1 - fh, a + fw, r.y1), t::SCRUBBER_IN_POINT_COLOR); }
            if let Some(b) = ox { paint::rect(scene, Rect::new(b - hair, top, b, r.y1), t::SCRUBBER_OUT_POINT_COLOR); paint::rect(scene, Rect::new(b - fw, r.y1 - fh, b, r.y1), t::SCRUBBER_OUT_POINT_COLOR); }
        }
        // the cache underline: cached coverage around the playhead (QCView's band: behind dimmer than ahead; when the cache
        // wraps — looping — the overflow past an edge continues from the opposite edge, the in / out zone's when one is set)
        if let Some((behind, ahead)) = s.cached && s.fps > 0.0 {
            let th = t::SCRUBBER_CACHE_BAND_THICKNESS as f64 * scale;
            let (lo, hi) = if s.in_point >= 0 && s.out_point > s.in_point { (s.in_point, s.out_point + 1) } else { (0, s.frames) };
            let mut span = |a: i32, b: i32, color| { let (x0, x1) = (self.x_of_frame(a as i64, s, scale), self.x_of_frame(b as i64, s, scale)); if x1 > x0 { paint::rect(scene, Rect::new(x0.max(tx0), r.y1 - th, x1.min(tx1), r.y1), color); } };
            let cur = s.current.clamp(lo, hi - 1);
            let (b0, a1) = (cur - behind, cur + ahead + 1);
            span(b0.max(lo), cur, t::SCRUBBER_CACHE_BEHIND_COLOR);
            span(cur, a1.min(hi), t::SCRUBBER_CACHE_AHEAD_COLOR);
            if self.cache_wraps {
                if b0 < lo { span(hi - (lo - b0), hi, t::SCRUBBER_CACHE_BEHIND_COLOR); }
                if a1 > hi { span(lo, lo + (a1 - hi), t::SCRUBBER_CACHE_AHEAD_COLOR); }
            }
        }
        // failed / missing frames on top of the band, ≥ 1 px wide
        if !self.failed.is_empty() && s.fps > 0.0 {
            let th = t::SCRUBBER_CACHE_BAND_THICKNESS as f64 * scale;
            for &f in &self.failed {
                let (x0, x1) = (self.x_of_frame(f as i64, s, scale), self.x_of_frame(f as i64 + 1, s, scale));
                if x1 < tx0 || x0 > tx1 { continue }
                paint::rect(scene, Rect::new(x0, r.y1 - th, x1.max(x0 + scale), r.y1), t::SCRUBBER_FAILED_FRAME_COLOR);
            }
        }
        // the extent marker when zoomed
        if self.pps.is_some() {
            let th = t::SCRUBBER_EXTENT_MARKER_THICKNESS as f64 * scale;
            let (a, b) = (tx0 + self.track_w * (t_start / dur), tx0 + self.track_w * (t_end / dur).min(1.0));
            paint::rect(scene, Rect::new(a, r.y1 - th, b, r.y1), t::SCRUBBER_EXTENT_MARKER_COLOR);
        }
        // the hover line and the playhead
        if let Some(hx) = self.hover_x { let x = hx.clamp(tx0, tx1); paint::rect(scene, Rect::new(x, top, x + hair, r.y1), t::SCRUBBER_TICK_COLOR); }
        let px = self.x_of_time(s.current as f64 / s.fps, s, scale);
        if px >= tx0 - 1.0 && px <= tx1 + 1.0 { paint::rect(scene, Rect::new(px, top, px + t::SCRUBBER_PLAYHEAD_WIDTH as f64 * scale, r.y1), t::SCRUBBER_PLAYHEAD_COLOR); }
    }
}

fn ruler_label(secs: f64) -> String { let m = (secs / 60.0).floor() as i64; let s = secs - m as f64 * 60.0; if secs.fract().abs() > 1e-6 { format!("{m}:{s:04.1}") } else { format!("{m}:{:02}", s.round() as i64) } }

#[cfg(test)]
mod tests {
    use super::*;
    fn st() -> ScrubState { ScrubState { frames: 240, fps: 24.0, current: 0, in_point: -1, out_point: -1, playing: false, scrubbing: false, cached: None } }
    #[test]
    fn camera_zooms_about_the_cursor_and_fits_back() {
        let mut sc = Scrubber::new(); let s = st();
        sc.layout(Rect::new(0.0, 0.0, 1016.0, 32.0), 1.0);   // track 1000 px = 10 s → 100 px/s
        assert_eq!(sc.frame_at_x(8.0 + 500.0, &s, 1.0), 120);
        assert!(sc.wheel(508.0, 480.0, false, &s, 1.0) && !sc.is_fit());
        assert_eq!(sc.frame_at_x(508.0, &s, 1.0), 120, "the frame under the cursor stays put");
        assert!(sc.wheel(508.0, -100000.0, false, &s, 1.0) && sc.is_fit(), "zooming out below fit snaps to fit");
        assert!(sc.wheel(508.0, 480.0, false, &s, 1.0));
        assert!(sc.wheel(508.0, 60.0, true, &s, 1.0), "alt-wheel pans");
        sc.fit(); assert!(sc.is_fit() && sc.frame_at_x(8.0, &s, 1.0) == 0);
    }
    #[test]
    fn press_move_release_is_begin_to_end_and_hover_sets_the_frame() {
        let mut sc = Scrubber::new(); let s = st(); let now = Instant::now();
        sc.layout(Rect::new(0.0, 0.0, 1016.0, 32.0), 1.0);
        let (c, _) = sc.event(&Event::Move { x: 258.0, y: 10.0 }, now, &s, 1.0);
        assert!(c.is_empty() && sc.hover_frame == Some(60));
        let (c, _) = sc.event(&Event::Down { x: 258.0, y: 10.0, button: Button::Primary }, now, &s, 1.0);
        assert_eq!(c, vec![ScrubCmd::Begin, ScrubCmd::To(60)]);
        assert!(sc.hover_frame.is_none());
        let (c, _) = sc.event(&Event::Move { x: 508.0, y: 10.0 }, now, &s, 1.0);
        assert_eq!(c, vec![ScrubCmd::To(120)]);
        let (c, _) = sc.event(&Event::Leave, now, &s, 1.0);
        assert!(c.is_empty() && sc.is_pressed(), "leaving the window keeps the grab");
        let (c, _) = sc.event(&Event::Up { x: 2000.0, y: 10.0, button: Button::Primary }, now, &s, 1.0);
        assert_eq!(c, vec![ScrubCmd::To(239), ScrubCmd::End], "release past the end clamps");
        sc.wheel(508.0, 480.0, false, &s, 1.0);
        let t2 = now + Duration::from_millis(100);
        sc.event(&Event::Down { x: 100.0, y: 10.0, button: Button::Primary }, t2, &s, 1.0); sc.event(&Event::Up { x: 100.0, y: 10.0, button: Button::Primary }, t2, &s, 1.0);
        let (c, _) = sc.event(&Event::Down { x: 101.0, y: 10.0, button: Button::Primary }, t2 + Duration::from_millis(100), &s, 1.0);
        assert!(c.is_empty() && sc.is_fit(), "a double-press fits");
    }
}
