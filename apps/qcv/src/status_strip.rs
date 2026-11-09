//! The status strip as nodes (QCView's `TimelineStatus.qml` carried): the frame and SMPTE readouts as QUIET text on the
//! band (QCView's are recessed wells; the user asked for the quiet form 2026-11-07) with a hover fill, fps on the right; a click on a well opens the go-to popover — frame
//! entry, or timecode entry with Resolve-style digit padding ("1000" → 00:00:10:00), warn-toned digits on a bad entry
//! until the next edit, Enter commits, Escape / a press outside / focus loss closes and discards.
//! Divergence (reason): QCView opened the popup DOWNWARD because an in-scene popup could not rise over the native
//! surface; `min-ui` has no such surface, so the popover opens ABOVE the well and never hides the transport row.
use kurbo::Rect;
use min_theme::tokens as t;
use min_ui::{anim::{mix, Ramp}, paint, Button, Event, Family, FieldResponse, Fonts, ImeEvent, KeyEvent, Popover, TextField, Tooltips};
use qcv_meta::TimecodeFormatter;
use std::time::Instant;
use vello::Scene;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(dead_code)]   // Time arrives with playlists (mixed-fps timelines show seconds, QCView's rule)
pub enum Mode { Frame, Timecode, Time }
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Commit { Frame(i32), Time(f64) }

struct Well { rect: Rect, text: String, hovered: bool, hover: Ramp }
impl Well {
    fn new() -> Well { Well { rect: Rect::ZERO, text: String::new(), hovered: false, hover: Ramp::new(0.0, t::ANIMATION_DURATION_HOVER) } }
    fn hit(&self, x: f64, y: f64) -> bool { self.rect.contains(kurbo::Point::new(x, y)) }
}

pub struct StatusStrip {
    frame: Well, tc: Well, fps: String,
    /// QCView's readouts carry no tooltips; ours name the well and its click (the user 2026-11-07: "double check the tooltips there")
    pub tooltips: Tooltips, fps_rect: Rect,
    /// `Some("")` = from start; `Some(tc)` = that timecode as frame 0's label; `None` = the file's embedded timecode
    pub origin: Option<String>,
    /// dual view's sync readout (row 212), quiet text right of the timecode
    pub extra: String,
    pub rect: Rect,
    popover: Popover, field: TextField, mode: Mode, error: bool,
    fmt: Option<TimecodeFormatter>, start: i32, frame_count: i32, duration_s: f64, drop_frame: bool,
}

const POPOVER_W: f64 = 210.0;   // QCView's go-to popup width (logical), padding included

/// QCView's catalog context (canon row 195).
fn tr(s: &str) -> &str { min_i18n::trc("TimelineStatus", s) }

impl StatusStrip {
    pub fn new() -> StatusStrip {
        let mut field = TextField::new(Family::Mono, t::CODE_SIZE_S);
        field.placeholder = tr("frame number").into();
        StatusStrip { frame: Well::new(), tc: Well::new(), fps: String::new(), tooltips: Tooltips::default(), fps_rect: Rect::ZERO, origin: None, extra: String::new(), rect: Rect::ZERO, popover: Popover::new(tr("GO TO FRAME")), field, mode: Mode::Frame, error: false, fmt: None, start: 0, frame_count: 0, duration_s: 0.0, drop_frame: false }
    }
    pub fn is_open(&self) -> bool { self.popover.open }
    /// The SMPTE readout's text (compact mode shows it in its own band).
    pub fn timecode_text(&self) -> &str { &self.tc.text }
    /// QCView's bindings: the frame well = the active clock's frame, the SMPTE well = the formatter at the source's start timecode.
    pub fn sync(&mut self, p: Option<&dyn qcv_player::Playback>) {
        let Some(p) = p else { self.frame.text = tr("Frame —").into(); self.tc.text = "—".into(); self.fps.clear(); self.fmt = None; self.frame_count = 0; return };
        let i = p.timecode_info();
        let (num, den) = i.frame_rate;
        let fmt = TimecodeFormatter::new(num, den, i.drop_frame);
        // the timecode origin: the Inspector's Origin picker (QCView's: From start / Embedded / QT Start / QT TC / XMP Alt) or the embedded timecode
        let origin = self.origin.as_deref().or(i.source_timecode.as_deref());
        self.start = origin.filter(|o| !o.is_empty()).map(|tc| fmt.parse(tc).max(0)).unwrap_or(0);
        self.frame_count = p.frame_count(); self.duration_s = i.duration_secs; self.drop_frame = i.drop_frame;
        let cur = p.current_frame();
        self.frame.text = if self.frame_count > 0 { min_i18n::fill(tr("Frame %1 / %2"), &[&cur, &(self.frame_count - 1)]) } else { tr("Frame —").into() };
        self.tc.text = if fmt.is_valid() { fmt.format(self.start + cur) } else { "—".into() };
        self.fps = min_i18n::fill(tr("%1 fps"), &[&format!("{:.2}", p.rate().as_f64())]);
        self.fmt = fmt.is_valid().then_some(fmt);
    }
    /// The right edge of the frame / timecode wells (dual view's controls start beyond it).
    pub fn wells_right(&self) -> f64 { self.tc.rect.x1 }
    /// The extra readout's width (0 when there is none).
    pub fn extra_width(&self, fonts: &mut Fonts, scale: f64) -> f64 { if self.extra.is_empty() { 0.0 } else { fonts.layout(&self.extra, Family::Mono, t::CODE_SIZE_S * scale as f32, t::FONT_WEIGHT_REGULAR as f32, paint::color(t::NEUTRAL_SUBDUED_CONTENT_COLOR_DEFAULT)).width() as f64 + t::SPACING_300 as f64 * scale } }
    /// The fps readout's width (it ends the strip on the right).
    pub fn fps_width(&self, fonts: &mut Fonts, scale: f64) -> f64 { if self.fps.is_empty() { 0.0 } else { fonts.layout(&self.fps, Family::Mono, t::CODE_SIZE_S * scale as f32, t::FONT_WEIGHT_REGULAR as f32, paint::color(t::NEUTRAL_SUBDUED_CONTENT_COLOR_DEFAULT)).width() as f64 } }
    pub fn layout(&mut self, w: f64, y: f64, scale: f64) {
        let h = t::STATUS_STRIP_HEIGHT as f64 * scale;
        let pad = t::SPACING_100 as f64 * scale;
        let (ww, wh) = (t::TIMECODE_WELL_WIDTH as f64 * scale, t::TIMECODE_WELL_HEIGHT as f64 * scale);
        let wy = y + (h - wh) / 2.0;
        self.frame.rect = Rect::new(pad, wy, pad + ww, wy + wh);
        self.tc.rect = Rect::new(pad + ww + pad, wy, pad + 2.0 * ww + pad, wy + wh);
        self.rect = Rect::new(0.0, y, w, y + h);
    }
    fn open(&mut self, mode: Mode, fonts: &mut Fonts, bounds: Rect, scale: f64) {
        let anchor = if mode == Mode::Frame { self.frame.rect } else { self.tc.rect };
        self.mode = mode; self.error = false; self.field.error = false;
        self.popover.title = tr(match mode { Mode::Frame => "GO TO FRAME", Mode::Timecode => "GO TO TIMECODE", Mode::Time => "GO TO TIME" }).into();
        self.field.placeholder = tr(match mode { Mode::Frame => "frame number", Mode::Timecode => "HH:MM:SS:FF", Mode::Time => "MM:SS.mmm" }).into();
        let content_w = POPOVER_W - 2.0 * t::POPOVER_PADDING as f64;
        let field_h = t::FIELD_HEIGHT_S as f64;
        self.popover.open_at(fonts, anchor, content_w, field_h, bounds, scale);
        self.field.clear(); self.field.focused = true;
        self.field.layout(fonts, self.popover.content, scale);
    }
    pub fn close(&mut self) { self.popover.close(); self.field.focused = false; }
    /// The harness: open a mode with text already typed (no pointer needed).
    pub fn stage(&mut self, mode: Mode, text: &str, fonts: &mut Fonts, bounds: Rect, scale: f64) { self.open(mode, fonts, bounds, scale); self.field.set_text_end(text, fonts); self.frame.hovered = true; self.frame.hover.set_target(1.0, Instant::now() - std::time::Duration::from_secs(1)); }
    /// Returns (dirty, consumed). A press outside an open popover closes it AND goes no further (minNotes' flyout rule).
    pub fn event(&mut self, e: &Event, now: Instant, fonts: &mut Fonts, bounds: Rect, scale: f64) -> (bool, bool) {
        if self.popover.open {
            match *e {
                Event::Down { x, y, .. } if !self.popover.hit(x, y) => { self.close(); return (true, true) }
                Event::Down { .. } | Event::Up { .. } | Event::Move { .. } => { let d = self.field.pointer(e, fonts); return (d, self.popover.hit(e.pos().unwrap().0, e.pos().unwrap().1)) }
                Event::Leave => { self.field.pointer(e, fonts); return (true, false) }
            }
        }
        let mut dirty = false; let mut consumed = false;
        for w in [&mut self.frame, &mut self.tc] {
            let inside = e.pos().is_some_and(|(x, y)| w.hit(x, y));
            let was = w.hovered;
            w.hovered = inside && !matches!(e, Event::Leave);
            if was != w.hovered { dirty = true; }
            w.hover.set_target(if w.hovered { 1.0 } else { 0.0 }, now);
        }
        if let Event::Down { x, y, button: Button::Primary } = *e {
            self.tooltips.pressed(now);
            if self.frame.hit(x, y) && self.frame_count > 0 { self.open(Mode::Frame, fonts, bounds, scale); dirty = true; consumed = true; }
            else if self.tc.hit(x, y) && self.fmt.is_some() { self.open(Mode::Timecode, fonts, bounds, scale); dirty = true; consumed = true; }
        }
        let over: Option<(u64, Rect, String)> = if self.frame.hovered { Some((1, self.frame.rect, if self.frame_count > 0 { tr("Frame number — click to go to a frame").into() } else { tr("Frame number").into() })) }
            else if self.tc.hovered { Some((2, self.tc.rect, match (self.fmt.is_some(), self.drop_frame) { (true, true) => tr("Timecode (drop-frame) — click to go to a timecode").into(), (true, false) => tr("Timecode — click to go to a timecode").into(), _ => tr("Timecode").into() })) }
            else if e.pos().is_some_and(|(x, y)| self.fps_rect.contains(kurbo::Point::new(x, y))) && !matches!(e, Event::Leave) { Some((3, self.fps_rect, tr("Frame rate of the open media").into())) } else { None };
        if self.tooltips.hover(over.as_ref().map(|(k, r, s)| (*k, *r, s.as_str())), now) { dirty = true; }
        (dirty, consumed)
    }
    pub fn ime(&mut self, e: &ImeEvent, fonts: &mut Fonts) { if self.popover.open { self.field.ime(e, fonts); } }
    #[allow(dead_code)]   // the IME cursor area, once the window layer is asked for it
    pub fn caret_rect(&self, fonts: &mut Fonts) -> Option<Rect> { if self.popover.open { self.field.caret_rect(fonts) } else { None } }
    /// Keys while the popover is open: Enter commits (a seek, or an error shown in the digits), Escape discards.
    pub fn key(&mut self, k: &KeyEvent, fonts: &mut Fonts) -> Option<Commit> {
        if !self.popover.open { return None }
        match self.field.key(k, fonts) {
            FieldResponse::Accepted => {
                match self.commit() {
                    Ok(c) => { self.close(); c }
                    Err(()) => { self.error = true; self.field.error = true; None }
                }
            }
            FieldResponse::Cancelled => { self.close(); None }
            FieldResponse::Changed => { self.error = false; self.field.error = false; None }
            FieldResponse::None => None,
        }
    }
    fn commit(&self) -> Result<Option<Commit>, ()> {
        let s = self.field.text().trim().to_string();
        if s.is_empty() { return Ok(None) }
        match self.mode {
            Mode::Frame => {
                if !s.chars().all(|c| c.is_ascii_digit()) { return Err(()) }
                if self.frame_count <= 0 { return Ok(None) }
                let f: i64 = s.parse().map_err(|_| ())?;
                Ok(Some(Commit::Frame(f.min(self.frame_count as i64 - 1) as i32)))
            }
            Mode::Timecode => {
                let fmt = self.fmt.as_ref().ok_or(())?;
                let entry = if s.chars().all(|c| c.is_ascii_digit()) && s.len() <= 8 { let p = format!("{s:0>8}"); format!("{}:{}:{}:{}", &p[0..2], &p[2..4], &p[4..6], &p[6..8]) } else { s.clone() };
                let f = fmt.parse(&entry);
                if f < 0 { return Err(()) }
                let rel = f - self.start;
                if rel < 0 { return Err(()) }
                Ok(Some(Commit::Frame(if self.frame_count > 0 { rel.min(self.frame_count - 1) } else { rel })))
            }
            Mode::Time => {
                let secs = flex_time_to_seconds(&s).ok_or(())?;
                Ok(Some(Commit::Time(secs.clamp(0.0, self.duration_s.max(0.0)))))
            }
        }
    }
    pub fn animating(&self, now: Instant) -> bool { self.frame.hover.animating(now) || self.tc.hover.animating(now) }
    /// Per frame: the tooltip's delay / timeout; true when a redraw is needed now.
    pub fn tick(&mut self, now: Instant) -> bool { self.tooltips.tick(now) }
    pub fn paint_tooltip(&self, scene: &mut Scene, fonts: &mut Fonts, scale: f64, bounds: Rect, now: Instant) { self.tooltips.paint(scene, fonts, scale, bounds, now); }
    pub fn paint(&mut self, scene: &mut Scene, fonts: &mut Fonts, scale: f64, now: Instant) {
        let r = t::TIMECODE_WELL_CORNER_RADIUS as f64 * scale; let bw = t::BORDER_WIDTH_100 as f64 * scale;
        let pad = t::FIELD_EDGE_TO_TEXT as f64 * scale;
        let drop = self.drop_frame;
        for (i, w) in [&self.frame, &self.tc].into_iter().enumerate() {
            // quiet readouts (the user 2026-11-07: "let that info exist on the app background, but still keep their clickability"):
            // no well at rest, the hover fill rises under the pointer so the click reads as a click
            let _ = bw;
            let bg = mix(t::HOVER_BACKGROUND_COLOR_IDLE, t::HOVER_BACKGROUND_COLOR, w.hover.value(now));
            if bg.a > 0.0 { paint::rounded(scene, w.rect, r, bg); }
            let color = if i == 1 && drop { t::TIMECODE_DROP_FRAME_COLOR } else { t::NEUTRAL_CONTENT_COLOR_DEFAULT };
            let l = fonts.layout(&w.text, Family::Mono, t::TIMECODE_WELL_FONT_SIZE * scale as f32, t::FONT_WEIGHT_REGULAR as f32, paint::color(color));
            min_ui::fonts::draw_layout(scene, &l, kurbo::Affine::translate((w.rect.x0 + pad, w.rect.y0 + (w.rect.height() - l.height() as f64) / 2.0)));
        }
        if !self.extra.is_empty() {
            let l = fonts.layout(&self.extra, Family::Mono, t::CODE_SIZE_S * scale as f32, t::FONT_WEIGHT_REGULAR as f32, paint::color(t::NEUTRAL_SUBDUED_CONTENT_COLOR_DEFAULT));
            let x = self.tc.rect.x1 + t::SPACING_300 as f64 * scale; let y = self.rect.y0 + (self.rect.height() - l.height() as f64) / 2.0;
            min_ui::fonts::draw_layout(scene, &l, kurbo::Affine::translate((x, y)));
        }
        if !self.fps.is_empty() {
            let l = fonts.layout(&self.fps, Family::Mono, t::CODE_SIZE_S * scale as f32, t::FONT_WEIGHT_REGULAR as f32, paint::color(t::NEUTRAL_SUBDUED_CONTENT_COLOR_DEFAULT));
            let right = self.rect.x1 - t::SPACING_100 as f64 * scale;
            let y = self.rect.y0 + (self.rect.height() - l.height() as f64) / 2.0;
            self.fps_rect = Rect::new(right - l.width() as f64, y, right, y + l.height() as f64);
            min_ui::fonts::draw_layout(scene, &l, kurbo::Affine::translate((right - l.width() as f64, y)));
        }
    }
    /// The popover, drawn after every band (it rises over the video).
    pub fn paint_popover(&mut self, scene: &mut Scene, fonts: &mut Fonts, scale: f64) {
        if !self.popover.open { return }
        self.popover.paint(scene, fonts, scale);
        self.field.paint(scene, fonts, scale);
    }
}

/// "90", "1:30", "1:30.5", "01:02:03.250" → seconds (QCView's `flexTimeToSeconds`).
fn flex_time_to_seconds(s: &str) -> Option<f64> {
    let parts: Vec<&str> = s.split(':').collect();
    if parts.is_empty() || parts.len() > 3 { return None }
    let mut secs = 0.0;
    for p in parts { let p = p.trim(); if p.is_empty() || !p.chars().all(|c| c.is_ascii_digit() || c == '.') { return None } secs = secs * 60.0 + p.parse::<f64>().ok()?; }
    Some(secs)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn flex_time() { assert_eq!(flex_time_to_seconds("90"), Some(90.0)); assert_eq!(flex_time_to_seconds("1:30.5"), Some(90.5)); assert_eq!(flex_time_to_seconds("01:02:03.250"), Some(3723.25)); assert_eq!(flex_time_to_seconds("x"), None); }
}
