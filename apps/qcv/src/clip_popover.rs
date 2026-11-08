//! The clip bar's precision popover (canon row 167): a double-click on a clip opens Source in · Source out · Duration as
//! timecode on the clip's own rate (from source frame 0). Enter or a press outside commits every changed field as the bar's
//! trims (a changed duration moves the out point); Tab moves between fields; Escape discards. A malformed field turns red and
//! stays open (the go-to popover's rule).
use crate::clip_bar::BarCmd;
use kurbo::{Affine, Rect};
use min_theme::tokens as t;
use min_ui::{paint, Event, Family, FieldResponse, Fonts, ImeEvent, Key, KeyEvent, Named, Popover, TextField};
use qcv_meta::TimecodeFormatter;
use qcv_timeline::{rescale, Clip, Rate};
use vello::Scene;

const LABELS: [&str; 3] = ["Source in", "Source out", "Duration"];
const LABEL_W: f64 = 76.0; const FIELD_W: f64 = 120.0; const GAP: f64 = 6.0;

pub struct ClipPopover { pub popover: Popover, fields: [TextField; 3], focus: usize, clip: Option<Clip>, master: Rate, fmt: TimecodeFormatter, rows: [Rect; 3] }

impl ClipPopover {
    pub fn new() -> ClipPopover {
        let f = || TextField::new(Family::Mono, t::CODE_SIZE_S);
        ClipPopover { popover: Popover::new("Clip"), fields: [f(), f(), f()], focus: 0, clip: None, master: Rate::new(24, 1), fmt: TimecodeFormatter::new(24, 1, false), rows: [Rect::ZERO; 3] }
    }
    pub fn is_open(&self) -> bool { self.popover.open }
    pub fn close(&mut self) { self.popover.close(); for f in &mut self.fields { f.focused = false; } self.clip = None; }
    pub fn open(&mut self, clip: &Clip, master: Rate, anchor: Rect, fonts: &mut Fonts, bounds: Rect, scale: f64) {
        self.fmt = TimecodeFormatter::new(clip.source_rate.num as i32, clip.source_rate.den as i32, false);
        self.master = master; self.clip = Some(clip.clone());
        self.popover.title = clip.name.clone();
        let fh = t::FIELD_HEIGHT_S as f64;
        self.popover.open_at(fonts, anchor, LABEL_W + FIELD_W, 3.0 * fh + 2.0 * GAP, bounds, scale);
        let c = self.popover.content;
        let vals = [clip.source_in, clip.source_out, clip.source_out - clip.source_in];
        for i in 0..3 {
            let y = c.y0 + i as f64 * (fh + GAP) * scale;
            self.rows[i] = Rect::new(c.x0, y, c.x1, y + fh * scale);
            let fr = Rect::new(c.x0 + LABEL_W * scale, y, c.x1, y + fh * scale);
            self.fields[i].error = false; self.fields[i].set_text(&self.fmt.format(vals[i] as i32)); self.fields[i].layout(fonts, fr, scale); self.fields[i].focused = false;
        }
        self.focus = 1; self.fields[1].focused = true; self.fields[1].select_all(fonts);   // the out point: the field a reviewer reaches for most
    }
    /// The edits the fields describe (None = a malformed field, marked red).
    fn commits(&mut self) -> Option<Vec<BarCmd>> {
        let c = self.clip.clone()?;
        let mut v = [0i64; 3];
        for i in 0..3 { let n = self.fmt.parse(self.fields[i].text().trim()); self.fields[i].error = n < 0; v[i] = n as i64; }
        if v.iter().any(|n| *n < 0) { return None }
        let (inn, mut out, dur) = (v[0], v[1], v[2]);
        if out == c.source_out && dur != c.source_out - c.source_in { out = inn + dur; }   // the duration field moved the out point
        let max = if c.source_duration > 0 { c.source_duration } else { i64::MAX };
        let inn = inn.clamp(0, c.source_out - 1); let out = out.clamp(inn + 1, max);
        let mut cmds = vec![];
        if inn != c.source_in { cmds.push(BarCmd::TrimHead { clip: c.id.clone(), new_start: c.start + rescale(inn - c.source_in, c.source_rate, self.master), new_source_in: inn }); }
        if out != c.source_out { cmds.push(BarCmd::TrimTail { clip: c.id.clone(), new_source_out: out }); }
        Some(cmds)
    }
    fn commit_and_close(&mut self) -> Vec<BarCmd> { match self.commits() { Some(c) => { self.close(); c } None => vec![] } }
    /// Returns (commands, redraw, consumed): a press outside commits and closes (blur commits, as every field in the app).
    pub fn event(&mut self, e: &Event, fonts: &mut Fonts) -> (Vec<BarCmd>, bool, bool) {
        if !self.popover.open { return (vec![], false, false) }
        if let Event::Down { x, y, .. } = *e {
            if !self.popover.hit(x, y) { let c = self.commit_and_close(); return (c, true, true) }
            if let Some(i) = (0..3).find(|&i| self.fields[i].hit(x, y)) { for (k, f) in self.fields.iter_mut().enumerate() { f.focused = k == i; } self.focus = i; }
        }
        let mut d = false; for f in &mut self.fields { if f.focused || matches!(e, Event::Move { .. } | Event::Leave) { d |= f.pointer(e, fonts); } }
        (vec![], d, e.pos().is_some_and(|(x, y)| self.popover.hit(x, y)))
    }
    pub fn key(&mut self, k: &KeyEvent, fonts: &mut Fonts) -> Vec<BarCmd> {
        if !self.popover.open { return vec![] }
        if k.key == Key::Named(Named::Tab) { self.fields[self.focus].focused = false; self.focus = (self.focus + if k.shift { 2 } else { 1 }) % 3; self.fields[self.focus].focused = true; self.fields[self.focus].select_all(fonts); return vec![] }
        match self.fields[self.focus].key(k, fonts) { FieldResponse::Accepted => self.commit_and_close(), FieldResponse::Cancelled => { self.close(); vec![] } FieldResponse::Changed => { self.fields[self.focus].error = false; vec![] } FieldResponse::None => vec![] }
    }
    pub fn ime(&mut self, e: &ImeEvent, fonts: &mut Fonts) { if self.popover.open { self.fields[self.focus].ime(e, fonts); } }
    pub fn paint(&mut self, scene: &mut Scene, fonts: &mut Fonts, scale: f64) {
        if !self.popover.open { return }
        self.popover.paint(scene, fonts, scale);
        for i in 0..3 {
            let l = fonts.layout(LABELS[i], Family::Sans, t::RAIL_LABEL_SIZE * scale as f32, t::FONT_WEIGHT_REGULAR as f32, paint::color(t::RAIL_LABEL_COLOR));
            let r = self.rows[i]; min_ui::fonts::draw_layout(scene, &l, Affine::translate((r.x0, r.y0 + (r.height() - l.height() as f64) / 2.0)));
            self.fields[i].paint(scene, fonts, scale);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use qcv_timeline::{ClipMediaKind, Editor, MediaInfo};
    fn clip() -> Clip {
        let m = MediaInfo { id: "m".into(), name: "c".into(), path: "/x.mov".into(), kind: ClipMediaKind::Video, rate: Rate::new(24, 1), duration: 480, width: 1, height: 1, has_audio: false, sequence_pattern: String::new(), sequence_start_frame: 0, sequence_end_frame: 0 };
        let mut ed = Editor::default(); ed.load_playlist("p", &[m], &[qcv_timeline::InOutRange { in_point: Some(24), out_point: Some(72) }], Rate::new(24, 1), 0, (1, 1), "pl");
        ed.timeline.tracks[0].clips[0].clone()
    }
    #[test]
    fn fields_commit_as_trims_a_duration_moves_the_out_and_bad_text_stays_open() {
        let mut fonts = Fonts::new(); let mut p = ClipPopover::new(); let c = clip(); let b = Rect::new(0.0, 0.0, 1600.0, 900.0);
        p.open(&c, Rate::new(24, 1), Rect::new(100.0, 800.0, 200.0, 820.0), &mut fonts, b, 1.0);
        assert_eq!(p.fields[0].text(), "00:00:01:00"); assert_eq!(p.fields[2].text(), "00:00:02:00");
        p.fields[2].set_text("00:00:03:00");   // duration 3 s → out = in + 72
        assert_eq!(p.commits(), Some(vec![BarCmd::TrimTail { clip: c.id.clone(), new_source_out: 96 }]));
        p.fields[2].set_text("00:00:02:00"); p.fields[0].set_text("00:00:00:12");
        assert!(matches!(p.commits().as_deref(), Some([BarCmd::TrimHead { new_source_in: 12, .. }])));
        p.fields[0].set_text("nonsense"); assert_eq!(p.commits(), None); assert!(p.fields[0].error);
    }
}
