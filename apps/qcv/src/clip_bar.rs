//! The playlist's clip bar (canon row 167: one 20-px bar under the scrubber, NO edit mode — a gesture means what its target
//! means). QCView's `TimelinePanel.qml` `pickEditOp` without the edit toggle: the 8 px at either end of a clip trims its head /
//! tail, dragging the body reorders it (slot-based, insertion bar), Ctrl/Cmd + drag slips the source window, a click seeks
//! there and selects the clip, a double-click opens the precision popover. Gestures start past QCView's 5-px threshold and
//! commit ONE undoable `qcv-timeline` edit on release; while dragging the bar shows the result (ghost + insertion bar /
//! the trimmed extent + a frame badge). Draws on the scrubber's camera, so clips line up with the ruler above.
use kurbo::{Affine, Rect};
use min_theme::tokens as t;
use min_ui::{paint, Event, Family, Fonts, ScrubState, Scrubber};
use qcv_timeline::{rescale, Clip, Timeline};
use std::time::Instant;
use vello::Scene;

pub const TRACK: &str = "A";
const THRESHOLD_PX: f64 = 5.0;
const DOUBLE_CLICK_MS: u128 = 400;

#[derive(Clone, Debug, PartialEq)]
pub enum BarCmd { Seek(i64), Select(Option<String>), TrimHead { clip: String, new_start: i64, new_source_in: i64 }, TrimTail { clip: String, new_source_out: i64 },
    Move { clip: String, index: usize }, Slip { clip: String, new_in: i64, new_out: i64 }, Precision(String) }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Zone { Head, Tail, Body }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Op { TrimHead, TrimTail, Reorder, Slip }

struct Gesture { op: Op, clip: Clip, x0: f64, x: f64, active: bool }

pub struct ClipBar {
    pub rect: Rect, pub selected: Option<String>, hover: Option<(String, Zone)>, press: Option<Gesture>, last_click: Option<(String, Instant)>,
    /// an incoming drop (rows or OS files) over the bar / scrubber: the insertion index and its edge x
    drop_hint: Option<(usize, f64)>, pointer: (f64, f64),
}

impl ClipBar {
    pub fn new() -> ClipBar { ClipBar { rect: Rect::ZERO, selected: None, hover: None, press: None, last_click: None, drop_hint: None, pointer: (0.0, 0.0) } }
    pub fn height(scale: f64) -> f64 { t::CLIP_BAR_HEIGHT as f64 * scale }
    pub fn hit(&self, x: f64, y: f64) -> bool { self.rect.contains(kurbo::Point::new(x, y)) }
    /// A gesture holds the pointer until its release (the shell's grab).
    pub fn is_pressed(&self) -> bool { self.press.is_some() }
    pub fn hover_zone(&self) -> Option<Zone> { self.press.as_ref().map(|g| match g.op { Op::TrimHead => Zone::Head, Op::TrimTail => Zone::Tail, _ => Zone::Body }).or(self.hover.as_ref().map(|h| h.1)) }
    pub fn set_drop_hint(&mut self, hint: Option<(usize, f64)>) -> bool { let c = self.drop_hint != hint; self.drop_hint = hint; c }
    fn clips(tl: &Timeline) -> &[Clip] { tl.tracks.first().map(|t| t.clips.as_slice()).unwrap_or(&[]) }
    fn span(c: &Clip, sc: &Scrubber, st: &ScrubState, scale: f64) -> (f64, f64) { (sc.x_of_frame(c.start, st, scale), sc.x_of_frame(c.start + c.duration, st, scale)) }
    fn at(&self, x: f64, tl: &Timeline, sc: &Scrubber, st: &ScrubState, scale: f64) -> Option<(Clip, Zone)> {
        let hw = t::CLIP_BAR_HANDLE_WIDTH as f64 * scale;
        Self::clips(tl).iter().filter(|c| !c.is_gap).find_map(|c| {
            let (x0, x1) = Self::span(c, sc, st, scale);
            if x < x0 || x >= x1 { return None }
            let wide = x1 - x0 > 3.0 * hw;
            Some((c.clone(), if wide && x < x0 + hw { Zone::Head } else if wide && x > x1 - hw { Zone::Tail } else { Zone::Body }))
        })
    }
    /// The insert-at index for timeline frame `f` (QCView's snap edge; unlimited tolerance = the nearest edge) and its x.
    pub fn insert_index(tl: &Timeline, f: i64, tol: Option<i64>, sc: &Scrubber, st: &ScrubState, scale: f64) -> Option<(usize, f64)> {
        let clips = Self::clips(tl);
        let idx = tl.snap_edge_index_at(TRACK, f, tol.unwrap_or(i64::MAX / 4))?;
        let edge = if idx >= clips.len() { tl.duration } else { clips[idx].start };
        Some((idx, sc.x_of_frame(edge, st, scale)))
    }
    /// The edit a gesture would commit at pointer x (None = no change).
    fn result(&self, g: &Gesture, tl: &Timeline, sc: &Scrubber, st: &ScrubState, scale: f64) -> Option<BarCmd> {
        let d = ((g.x - g.x0) * sc.frames_per_px(st)).round() as i64;
        let c = &g.clip; let src = |n: i64| rescale(n, tl.rate, c.source_rate);
        match g.op {
            Op::TrimTail => { let out = (c.source_out + src(d)).clamp(c.source_in + 1, if c.source_duration > 0 { c.source_duration } else { i64::MAX }); (out != c.source_out).then(|| BarCmd::TrimTail { clip: c.id.clone(), new_source_out: out }) }
            Op::TrimHead => { let inn = (c.source_in + src(d)).clamp(0, c.source_out - 1); (inn != c.source_in).then(|| BarCmd::TrimHead { clip: c.id.clone(), new_start: c.start + rescale(inn - c.source_in, c.source_rate, tl.rate), new_source_in: inn }) }
            Op::Slip => {
                let len = c.source_out - c.source_in; let max_in = if c.source_duration > 0 { (c.source_duration - len).max(0) } else { i64::MAX };
                let inn = (c.source_in - src(d)).clamp(0, max_in);   // the content follows the pointer
                (inn != c.source_in).then(|| BarCmd::Slip { clip: c.id.clone(), new_in: inn, new_out: inn + len })
            }
            Op::Reorder => { let f = sc.frame_at_x(g.x, st, scale) as i64; let (idx, _) = Self::insert_index(tl, f, None, sc, st, scale)?; let from = Self::clips(tl).iter().position(|k| k.id == c.id)?; (idx != from && idx != from + 1).then(|| BarCmd::Move { clip: c.id.clone(), index: idx }) }
        }
    }
    /// Returns (commands, redraw). `ctrl` = Ctrl / Cmd (the slip modifier).
    pub fn event(&mut self, e: &Event, now: Instant, ctrl: bool, tl: &Timeline, sc: &Scrubber, st: &ScrubState, scale: f64) -> (Vec<BarCmd>, bool) {
        let mut cmds = vec![]; let mut dirty = false;
        if let Some((x, y)) = e.pos() { self.pointer = (x, y); }
        match *e {
            Event::Move { x, y } => {
                if let Some(g) = self.press.as_mut() {
                    if !g.active && (x - g.x0).abs() > THRESHOLD_PX * scale { g.active = true; self.last_click = None; }
                    if g.active { g.x = x; dirty = true; }
                } else {
                    let h = if self.hit(x, y) { self.at(x, tl, sc, st, scale).map(|(c, z)| (c.id, z)) } else { None };
                    if h != self.hover { self.hover = h; dirty = true; }
                }
            }
            Event::Down { x, y, button: min_ui::Button::Primary } if self.hit(x, y) => {
                match self.at(x, tl, sc, st, scale) {
                    Some((c, z)) => {
                        let op = match z { Zone::Head => Op::TrimHead, Zone::Tail => Op::TrimTail, Zone::Body if ctrl => Op::Slip, Zone::Body => Op::Reorder };
                        self.press = Some(Gesture { op, clip: c, x0: x, x, active: false });
                    }
                    None => { cmds.push(BarCmd::Select(None)); cmds.push(BarCmd::Seek(sc.frame_at_x(x, st, scale) as i64)); self.selected = None; }
                }
                dirty = true;
            }
            Event::Up { x, button: min_ui::Button::Primary, .. } => {
                if let Some(g) = self.press.take() {
                    if g.active { if let Some(c) = self.result(&g, tl, sc, st, scale) { cmds.push(c); } }
                    else {
                        let id = g.clip.id.clone();
                        if self.last_click.as_ref().is_some_and(|(l, t0)| *l == id && now.duration_since(*t0).as_millis() <= DOUBLE_CLICK_MS) { cmds.push(BarCmd::Precision(id.clone())); self.last_click = None; }
                        else { self.last_click = Some((id.clone(), now)); cmds.push(BarCmd::Seek(sc.frame_at_x(x, st, scale) as i64)); }
                        self.selected = Some(id.clone()); cmds.push(BarCmd::Select(Some(id)));
                    }
                    dirty = true;
                }
            }
            Event::Leave => { if self.hover.take().is_some() { dirty = true; } }
            _ => {}
        }
        (cmds, dirty)
    }
    pub fn paint(&self, scene: &mut Scene, fonts: &mut Fonts, tl: &Timeline, sc: &Scrubber, st: &ScrubState, scale: f64) {
        let r = self.rect; if r.width() <= 0.0 { return }
        paint::rect(scene, r, t::SCRUBBER_BACKGROUND_COLOR);
        let inset = 2.0 * scale; let radius = t::CORNER_RADIUS_SMALL_DEFAULT as f64 * scale; let y0 = r.y0 + inset; let y1 = r.y1 - inset;
        let clips = Self::clips(tl);
        if !clips.iter().any(|c| !c.is_gap) {
            let l = fonts.layout("Drop media here to add to the playlist", Family::Sans, t::DETAIL_SIZE_S * scale as f32, t::FONT_WEIGHT_REGULAR as f32, paint::color(t::DISABLED_CONTENT_COLOR));
            min_ui::fonts::draw_layout(scene, &l, Affine::translate((r.x0 + (r.width() - l.width() as f64) / 2.0, r.y0 + (r.height() - l.height() as f64) / 2.0)));
        }
        let g = self.press.as_ref().filter(|g| g.active);
        let preview = g.and_then(|g| self.result(g, tl, sc, st, scale));
        scene.push_clip_layer(peniko::Fill::NonZero, Affine::IDENTITY, &r);
        for c in clips {
            let (mut x0, mut x1) = Self::span(c, sc, st, scale);
            if c.is_gap { paint::rect(scene, Rect::new(x0, y0, x1, y1), t::CLIP_BAR_GAP_COLOR); continue }
            // the trim in progress shows its result in place
            match &preview { Some(BarCmd::TrimTail { clip, new_source_out }) if *clip == c.id => { x1 = x0 + (sc.x_of_frame(rescale(*new_source_out - c.source_in, c.source_rate, tl.rate), st, scale) - sc.x_of_frame(0, st, scale)); }
                Some(BarCmd::TrimHead { clip, new_source_in, .. }) if *clip == c.id => { x0 = x1 - (sc.x_of_frame(rescale(c.source_out - *new_source_in, c.source_rate, tl.rate), st, scale) - sc.x_of_frame(0, st, scale)); } _ => {} }
            let current = st.current as i64 >= c.start && (st.current as i64) < c.start + c.duration;
            let selected = self.selected.as_deref() == Some(c.id.as_str());
            let hovered = self.hover.as_ref().is_some_and(|h| h.0 == c.id);
            let dragged = g.is_some_and(|g| g.op == Op::Reorder && g.clip.id == c.id);
            let fill = if selected { t::CLIP_BAR_SELECTED_COLOR } else if current { t::CLIP_BAR_CURRENT_COLOR } else if hovered { t::CLIP_BAR_CLIP_HOVER_COLOR } else { t::CLIP_BAR_CLIP_COLOR };
            let cr = Rect::new(x0 + 0.5 * scale, y0, (x1 - 0.5 * scale).max(x0 + scale), y1);
            paint::rounded(scene, cr, radius, if dragged { t::CLIP_BAR_GAP_COLOR } else { fill });
            // the name, elided in the middle, when there is room
            let pad = 6.0 * scale; let avail = cr.width() - 2.0 * pad;
            if avail > 16.0 * scale && !dragged {
                let col = if selected { t::GRAY_100 } else { t::CLIP_BAR_LABEL_COLOR };
                let mk = |s: &str, f: &mut Fonts| f.layout(s, Family::Sans, t::DETAIL_SIZE_S * scale as f32, t::FONT_WEIGHT_REGULAR as f32, paint::color(col));
                let mut l = mk(&c.name, fonts);
                if l.width() as f64 > avail { let ch: Vec<char> = c.name.chars().collect(); let mut keep = ch.len(); while keep > 2 { keep -= 1; let h = keep / 2; let s: String = format!("{}…{}", ch[..h].iter().collect::<String>(), ch[ch.len() - (keep - h)..].iter().collect::<String>()); l = mk(&s, fonts); if (l.width() as f64) <= avail { break } } }
                if (l.width() as f64) <= avail { min_ui::fonts::draw_layout(scene, &l, Affine::translate((cr.x0 + pad, cr.y0 + (cr.height() - l.height() as f64) / 2.0))); }
            }
            // the trim handles on hover (QCView: they light when the pointer is in the 8-px zone)
            if let Some((id, z)) = &self.hover { if *id == c.id && g.is_none() { let w = 2.0 * scale; match z { Zone::Head => paint::rect(scene, Rect::new(cr.x0, y0, cr.x0 + w, y1), t::CLIP_BAR_HANDLE_COLOR), Zone::Tail => paint::rect(scene, Rect::new(cr.x1 - w, y0, cr.x1, y1), t::CLIP_BAR_HANDLE_COLOR), Zone::Body => {} } } }
        }
        // reorder: the insertion bar at the target edge + the ghost under the pointer; drops: the insertion bar
        let bar = |scene: &mut Scene, x: f64| paint::rect(scene, Rect::new(x - scale, r.y0, x + scale, r.y1), t::CLIP_BAR_INSERT_COLOR);
        if let (Some(g), Some(BarCmd::Move { index, .. })) = (g, &preview) {
            let edge = if *index >= clips.len() { tl.duration } else { clips[*index].start }; bar(scene, sc.x_of_frame(edge, st, scale));
            let (gx0, gx1) = Self::span(&g.clip, sc, st, scale); let w = gx1 - gx0; let gx = g.x - (g.x0 - gx0);
            paint::rounded(scene, Rect::new(gx, y0, gx + w, y1), radius, min_theme::Color { a: 0.85, ..t::CLIP_BAR_CLIP_HOVER_COLOR });
        }
        if let Some((_, x)) = self.drop_hint { bar(scene, x); }
        scene.pop_layer();
        // the trim / slip badge: the change in frames, by the pointer
        if let (Some(g), Some(p)) = (g, &preview) {
            let n = match p { BarCmd::TrimTail { new_source_out, .. } => new_source_out - g.clip.source_out, BarCmd::TrimHead { new_source_in, .. } => g.clip.source_in - new_source_in, BarCmd::Slip { new_in, .. } => new_in - g.clip.source_in, _ => 0 };
            if n != 0 || matches!(g.op, Op::Slip) {
                let txt = format!("{}{n} f", if n > 0 { "+" } else { "" });
                let l = fonts.layout(&txt, Family::Mono, t::DETAIL_SIZE_S * scale as f32, t::FONT_WEIGHT_REGULAR as f32, paint::color(t::CLIP_BAR_LABEL_COLOR));
                let pad = 6.0 * scale; let bw = l.width() as f64 + 2.0 * pad; let bh = r.height();
                let bx = (g.x - bw / 2.0).clamp(r.x0, r.x1 - bw); let br = Rect::new(bx, r.y0 - bh - 4.0 * scale, bx + bw, r.y0 - 4.0 * scale);
                paint::rounded(scene, br, radius, t::GRAY_75);
                min_ui::fonts::draw_layout(scene, &l, Affine::translate((br.x0 + pad, br.y0 + (bh - l.height() as f64) / 2.0)));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use min_ui::Button;
    use qcv_timeline::{ClipMediaKind, Editor, MediaInfo, Rate};
    /// three 100-frame clips at 24 fps on a 1000-px bar: 300 frames → ~3.3 px per frame
    fn setup() -> (ClipBar, Editor, Scrubber, ScrubState) {
        let m = |i: usize| MediaInfo { id: format!("m{i}"), name: format!("clip{i}"), path: format!("/x/{i}.mov"), kind: ClipMediaKind::Video, rate: Rate::new(24, 1), duration: 200, width: 1920, height: 1080, has_audio: false, sequence_pattern: String::new(), sequence_start_frame: 0, sequence_end_frame: 0 };
        let mut ed = Editor::default();
        ed.load_playlist("p", &[m(0), m(1), m(2)], &[qcv_timeline::InOutRange { in_point: Some(0), out_point: Some(100) }; 3], Rate::new(24, 1), 0, (1920, 1080), "pl");
        let st = ScrubState { frames: ed.timeline.duration as i32, fps: 24.0, current: 0, in_point: -1, out_point: -1, playing: false, scrubbing: false, cached: None };
        let mut sc = Scrubber::new(); sc.layout(Rect::new(0.0, 0.0, 1016.0, 32.0), 1.0);
        let mut bar = ClipBar::new(); bar.rect = Rect::new(0.0, 32.0, 1016.0, 52.0);
        (bar, ed, sc, st)
    }
    fn x_of(sc: &Scrubber, st: &ScrubState, f: i64) -> f64 { sc.x_of_frame(f, st, 1.0) }
    fn drag(bar: &mut ClipBar, ed: &Editor, sc: &Scrubber, st: &ScrubState, x0: f64, x1: f64, ctrl: bool) -> Vec<BarCmd> {
        let now = Instant::now(); let y = 42.0;
        bar.event(&Event::Down { x: x0, y, button: Button::Primary }, now, ctrl, &ed.timeline, sc, st, 1.0);
        bar.event(&Event::Move { x: x1, y }, now, ctrl, &ed.timeline, sc, st, 1.0);
        bar.event(&Event::Up { x: x1, y, button: Button::Primary }, now, ctrl, &ed.timeline, sc, st, 1.0).0
    }
    #[test]
    fn ends_trim_the_body_reorders_and_ctrl_slips() {
        let (mut bar, ed, sc, st) = setup();
        let ids: Vec<String> = ed.timeline.tracks[0].clips.iter().map(|c| c.id.clone()).collect();
        // the tail of clip 0, dragged right by ~20 frames
        let tail = x_of(&sc, &st, 100) - 2.0; let c = drag(&mut bar, &ed, &sc, &st, tail, tail + 20.0 / sc.frames_per_px(&st), false);
        assert!(matches!(&c[..], [BarCmd::TrimTail { clip, new_source_out }] if *clip == ids[0] && (118..=122).contains(new_source_out)), "{c:?}");
        // the head of clip 1, dragged right by ~10 frames: the source in moves in
        let head = x_of(&sc, &st, 100) + 2.0; let c = drag(&mut bar, &ed, &sc, &st, head, head + 10.0 / sc.frames_per_px(&st), false);
        assert!(matches!(&c[..], [BarCmd::TrimHead { clip, new_source_in, .. }] if *clip == ids[1] && (8..=12).contains(new_source_in)), "{c:?}");
        // the body of clip 0 dropped past clip 1's end → index 2
        let c = drag(&mut bar, &ed, &sc, &st, x_of(&sc, &st, 50), x_of(&sc, &st, 195), false);
        assert_eq!(c, vec![BarCmd::Move { clip: ids[0].clone(), index: 2 }]);
        // Ctrl + body: slip — the content follows the pointer, so dragging LEFT moves the source in forward
        let mid = x_of(&sc, &st, 250); let c = drag(&mut bar, &ed, &sc, &st, mid, mid - 30.0 / sc.frames_per_px(&st), true);
        assert!(matches!(&c[..], [BarCmd::Slip { clip, new_in, new_out }] if *clip == ids[2] && (28..=32).contains(new_in) && new_out - new_in == 100), "{c:?}");
    }
    #[test]
    fn a_click_seeks_and_selects_a_double_click_opens_the_precision_popover_and_small_moves_stay_clicks() {
        let (mut bar, ed, sc, st) = setup(); let now = Instant::now(); let y = 42.0;
        let x = x_of(&sc, &st, 150);
        let c = drag(&mut bar, &ed, &sc, &st, x, x + 3.0, false);   // under the 5-px threshold
        assert!(matches!(&c[..], [BarCmd::Seek(f), BarCmd::Select(Some(_))] if (148..=153).contains(f)), "{c:?}");
        bar.event(&Event::Down { x, y, button: Button::Primary }, now, false, &ed.timeline, &sc, &st, 1.0);
        let (c, _) = bar.event(&Event::Up { x, y, button: Button::Primary }, now, false, &ed.timeline, &sc, &st, 1.0);
        assert!(matches!(&c[..], [BarCmd::Precision(_), BarCmd::Select(Some(_))]), "{c:?}");
    }
    #[test]
    fn the_drop_index_snaps_within_tolerance_and_otherwise_reports_none() {
        let (_, ed, sc, st) = setup();
        assert_eq!(ClipBar::insert_index(&ed.timeline, 103, Some(5), &sc, &st, 1.0).map(|r| r.0), Some(1));
        assert_eq!(ClipBar::insert_index(&ed.timeline, 150, Some(5), &sc, &st, 1.0).map(|r| r.0), None, "mid-clip, out of tolerance → the caller appends");
        assert_eq!(ClipBar::insert_index(&ed.timeline, 299, Some(5), &sc, &st, 1.0).map(|r| r.0), Some(3));
    }
}
