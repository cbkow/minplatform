//! Dual view's two clip lanes under the scrubber (canon rows 167 / 212): A over B, 16 px each (12 on the first build; the walk: "a few pix taller for ergonomics"), on the scrubber's camera — NO
//! edit mode, a gesture means what its target means. The 8 px at a clip's ends trim its head / tail, dragging the body SLIDES it
//! in time carrying the later clips on its side (Shift: alone), Ctrl / Cmd + drag slips the source window, a click seeks there,
//! selects the clip and makes its side the TARGET (⌘K and the keys act there), a double-click opens the precision popover.
//! Precision (row 212): trims and slides SNAP within 8 px to the playhead, then the other side's cuts, in / out, this side's
//! other edges — a line marks the snap — and a badge shows the change and the resulting frame. Hovering a clip shows its sync
//! offset against the master (timeline position − source position). One undoable edit per gesture, on release.
use kurbo::{Affine, Rect};
use min_theme::tokens as t;
use min_ui::{paint, Event, Family, Fonts, ScrubState, Scrubber};
use qcv_player::Side;
use qcv_timeline::{rescale, Clip, Timeline, TRACK_A, TRACK_B};
use std::time::Instant;
use vello::Scene;

const THRESHOLD_PX: f64 = 5.0;
const DOUBLE_CLICK_MS: u128 = 400;
const SNAP_PX: f64 = 8.0;
pub const LANE: f64 = 16.0;
/// The row label cell (logical px) — the scrubber's end inset, so frame 0 starts just past it.
pub const LABEL_W: f64 = 28.0;

#[derive(Clone, Debug, PartialEq)]
pub enum LaneCmd {
    Seek(i64), Select(Option<(Side, String)>), Target(Side),
    TrimHead { side: Side, clip: String, new_start: i64, new_source_in: i64 }, TrimTail { side: Side, clip: String, new_source_out: i64 },
    Slide { side: Side, clip: String, delta: i64, carry: bool }, Slip { side: Side, clip: String, new_in: i64, new_out: i64 },
    Precision(Side, String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Zone { Head, Tail, Body }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Op { TrimHead, TrimTail, Slide, Slip }
struct Gesture { side: Side, op: Op, clip: Clip, x0: f64, x: f64, active: bool, carry: bool }

pub fn track(side: Side) -> &'static str { if side == Side::A { TRACK_A } else { TRACK_B } }
fn clips(tl: &Timeline, side: Side) -> Vec<&Clip> { let mut v: Vec<&Clip> = tl.track(track(side)).map(|t| t.clips.iter().filter(|c| !c.is_gap).collect()).unwrap_or_default(); v.sort_by_key(|c| c.start); v }
/// The clip's sync offset against the master: where it plays minus where its source frame sits (QCView's slide in frames).
pub fn offset(c: &Clip, master: qcv_timeline::Rate) -> i64 { c.start - rescale(c.source_in, c.source_rate, master) }

pub struct DualBars {
    pub rect: Rect, pub selected: Option<(Side, String)>, pub target: Side, /// the LIVE sides (canon row 258, R4): a full-width bar labelled LIVE, no clip, no edit
    pub live: [bool; 2],
    hover: Option<(Side, String, Zone)>, press: Option<Gesture>, last_click: Option<(String, Instant)>, pointer: (f64, f64),
}

impl DualBars {
    pub fn new() -> DualBars { DualBars { rect: Rect::ZERO, selected: None, target: Side::B, live: [false; 2], hover: None, press: None, last_click: None, pointer: (0.0, 0.0) } }
    pub fn height(scale: f64) -> f64 { 2.0 * LANE * scale }
    pub fn hit(&self, x: f64, y: f64) -> bool { self.rect.contains(kurbo::Point::new(x, y)) }
    pub fn is_pressed(&self) -> bool { self.press.is_some() }
    pub fn hover_zone(&self) -> Option<Zone> { self.press.as_ref().map(|g| match g.op { Op::TrimHead => Zone::Head, Op::TrimTail => Zone::Tail, _ => Zone::Body }).or(self.hover.as_ref().map(|h| h.2)) }
    fn lane(&self, side: Side) -> Rect { let h = self.rect.height() / 2.0; if side == Side::A { Rect::new(self.rect.x0, self.rect.y0, self.rect.x1, self.rect.y0 + h) } else { Rect::new(self.rect.x0, self.rect.y0 + h, self.rect.x1, self.rect.y1) } }
    fn side_at(&self, y: f64) -> Side { if y < self.rect.y0 + self.rect.height() / 2.0 { Side::A } else { Side::B } }
    fn span(c: &Clip, sc: &Scrubber, st: &ScrubState, scale: f64) -> (f64, f64) { (sc.x_of_frame(c.start, st, scale), sc.x_of_frame(c.end(), st, scale)) }
    fn at(&self, side: Side, x: f64, tl: &Timeline, sc: &Scrubber, st: &ScrubState, scale: f64) -> Option<(Clip, Zone)> {
        let hw = 8.0 * scale;
        clips(tl, side).into_iter().find_map(|c| {
            let (x0, x1) = Self::span(c, sc, st, scale);
            if x < x0 || x >= x1 { return None }
            let wide = x1 - x0 > 3.0 * hw;
            Some((c.clone(), if wide && x < x0 + hw { Zone::Head } else if wide && x > x1 - hw { Zone::Tail } else { Zone::Body }))
        })
    }
    /// The frames an edge may snap to, nearest-first by priority: the playhead, the other side's cuts, in / out, this side's
    /// other clips' edges (the moving ones excluded).
    fn snap_targets(&self, g: &Gesture, tl: &Timeline, st: &ScrubState) -> Vec<Vec<i64>> {
        let other = if g.side == Side::A { Side::B } else { Side::A };
        let cuts = |s: Side, skip: &dyn Fn(&Clip) -> bool| clips(tl, s).into_iter().filter(|c| !skip(c)).flat_map(|c| [c.start, c.end()]).collect::<Vec<i64>>();
        let moving_from = g.clip.start;
        let skip_own = |c: &Clip| c.id == g.clip.id || (g.op == Op::Slide && g.carry && c.start >= moving_from);
        let mut io = vec![]; if st.in_point >= 0 { io.push(st.in_point as i64); } if st.out_point >= 0 { io.push(st.out_point as i64 + 1); }
        vec![vec![st.current as i64], cuts(other, &|_| false), io, cuts(g.side, &skip_own)]
    }
    /// A frame delta snapped: the moving edges, the targets by priority; within SNAP_PX the highest-priority nearest target
    /// wins. Returns (delta, the snap frame).
    fn snapped(&self, g: &Gesture, d: i64, tl: &Timeline, sc: &Scrubber, st: &ScrubState) -> (i64, Option<i64>) {
        let edges: Vec<i64> = match g.op { Op::TrimHead => vec![g.clip.start], Op::TrimTail => vec![g.clip.end()], Op::Slide => vec![g.clip.start, g.clip.end()], Op::Slip => return (d, None) };
        let tol = (SNAP_PX * sc.frames_per_px(st)).max(1.0) as i64;
        for group in self.snap_targets(g, tl, st) {
            let mut best: Option<(i64, i64)> = None;   // (distance, target)
            for &e in &edges { for &tg in &group { let dist = (e + d - tg).abs(); if dist <= tol && best.is_none_or(|b| dist < b.0) { best = Some((dist, tg)); } } }
            if let Some((_, tg)) = best { let e = edges.iter().min_by_key(|&&e| (e + d - tg).abs()).unwrap(); return (tg - e, Some(tg)) }
        }
        (d, None)
    }
    /// The edit a gesture would commit now, and the frame it snapped to.
    fn result(&self, g: &Gesture, tl: &Timeline, sc: &Scrubber, st: &ScrubState, scale: f64) -> (Option<LaneCmd>, Option<i64>) {
        let _ = scale;
        let raw = ((g.x - g.x0) * sc.frames_per_px(st)).round() as i64;
        let (d, snap) = self.snapped(g, raw, tl, sc, st);
        let c = &g.clip; let src = |n: i64| rescale(n, tl.rate, c.source_rate); let side = g.side;
        let cmd = match g.op {
            Op::TrimTail => { let out = (c.source_out + src(d)).clamp(c.source_in + 1, if c.source_duration > 0 { c.source_duration } else { i64::MAX }); (out != c.source_out).then(|| LaneCmd::TrimTail { side, clip: c.id.clone(), new_source_out: out }) }
            Op::TrimHead => { let inn = (c.source_in + src(d)).clamp(0, c.source_out - 1); (inn != c.source_in).then(|| LaneCmd::TrimHead { side, clip: c.id.clone(), new_start: c.start + rescale(inn - c.source_in, c.source_rate, tl.rate), new_source_in: inn }) }
            Op::Slip => { let len = c.source_out - c.source_in; let max_in = if c.source_duration > 0 { (c.source_duration - len).max(0) } else { i64::MAX }; let inn = (c.source_in + src(d)).clamp(0, max_in); (inn != c.source_in).then(|| LaneCmd::Slip { side, clip: c.id.clone(), new_in: inn, new_out: inn + len }) }
            Op::Slide => (d != 0).then(|| LaneCmd::Slide { side, clip: c.id.clone(), delta: d, carry: g.carry }),   // past frame 0 the timeline grows left (row 212)
        };
        (cmd, snap)
    }
    /// QCView `previewEditScrubFrame` for dual (`requestScrubFrameForSide`): while a trim / slip drags, the side and the SOURCE frame
    /// it should show — the new in (head), the new last frame (tail), the content under the playhead (slip; None off the clip).
    pub fn edit_preview(&self, tl: &Timeline, sc: &Scrubber, st: &ScrubState, scale: f64) -> Option<(Side, String, i64)> {
        let g = self.press.as_ref().filter(|g| g.active && g.op != Op::Slide)?;
        let c = &g.clip; let last = if c.source_duration > 0 { c.source_duration - 1 } else { i64::MAX };
        let src = match self.result(g, tl, sc, st, scale).0 {
            Some(LaneCmd::TrimHead { new_source_in, .. }) => new_source_in,
            Some(LaneCmd::TrimTail { new_source_out, .. }) => (new_source_out - 1).min(last),
            Some(LaneCmd::Slip { new_in, .. }) => { let ph = st.current as i64; if !c.contains(ph) { return None } (new_in + rescale(ph - c.start, tl.rate, c.source_rate)).min(last) }
            _ => match g.op { Op::TrimHead => c.source_in, Op::TrimTail => (c.source_out - 1).min(last), _ => return None },
        };
        Some((g.side, c.id.clone(), src))
    }
    /// The sync readout for the status strip (the walk 2026-11-08: the hover badge sat over the scrubber): the hovered clip's
    /// offset against the other side's clip at the same spot (its first one otherwise) — else the selected clip's, else the
    /// target side's clip under the playhead. "B −14 f vs A".
    pub fn offset_text(&self, tl: &Timeline, playhead: i64) -> Option<String> {
        let (side, clip) = if let Some((s, id, _)) = &self.hover { (*s, clips(tl, *s).into_iter().find(|c| c.id == *id)?) }
            else if let Some((s, id)) = &self.selected { (*s, clips(tl, *s).into_iter().find(|c| c.id == *id)?) }
            else { (self.target, clips(tl, self.target).into_iter().find(|c| c.contains(playhead))?) };
        let other = if side == Side::A { Side::B } else { Side::A };
        let name = |x: Side| if x == Side::A { "A" } else { "B" };
        let oc = clips(tl, other); let base = oc.iter().find(|k| k.contains(clip.start)).or(oc.first()).map(|k| offset(k, tl.rate))?;
        let o = offset(clip, tl.rate) - base;
        Some(format!("{} {}{} f vs {}", name(side), if o > 0 { "+" } else if o < 0 { "−" } else { "" }, o.abs(), name(other)))
    }
    /// Returns (commands, redraw). `ctrl` = Ctrl / Cmd (slip), `shift` = move alone.
    pub fn event(&mut self, e: &Event, now: Instant, ctrl: bool, shift: bool, tl: &Timeline, sc: &Scrubber, st: &ScrubState, scale: f64) -> (Vec<LaneCmd>, bool) {
        let mut cmds = vec![]; let mut dirty = false;
        if let Some((x, y)) = e.pos() { if (x, y) != self.pointer { self.pointer = (x, y); if self.hover.is_some() { dirty = true; } } }
        match *e {
            Event::Move { x, y } => {
                if let Some(g) = self.press.as_mut() {
                    if !g.active && (x - g.x0).abs() > THRESHOLD_PX * scale { g.active = true; self.last_click = None; }
                    if g.active { g.x = x; g.carry = g.op == Op::Slide && !shift; dirty = true; }
                } else {
                    let h = if self.hit(x, y) { let s = self.side_at(y); self.at(s, x, tl, sc, st, scale).map(|(c, z)| (s, c.id, z)) } else { None };
                    if h != self.hover { self.hover = h; dirty = true; }
                }
            }
            Event::Down { x, y, button: min_ui::Button::Primary } if self.hit(x, y) => {
                let side = self.side_at(y);
                if self.target != side { self.target = side; cmds.push(LaneCmd::Target(side)); }
                match self.at(side, x, tl, sc, st, scale) {
                    Some((c, z)) => { let op = match z { Zone::Head => Op::TrimHead, Zone::Tail => Op::TrimTail, Zone::Body if ctrl => Op::Slip, Zone::Body => Op::Slide }; self.press = Some(Gesture { side, op, clip: c, x0: x, x, active: false, carry: !shift }); }
                    None => { cmds.push(LaneCmd::Select(None)); cmds.push(LaneCmd::Seek(sc.frame_at_x(x, st, scale) as i64)); self.selected = None; }
                }
                dirty = true;
            }
            Event::Up { x, button: min_ui::Button::Primary, .. } => {
                if let Some(g) = self.press.take() {
                    if g.active { if let (Some(c), _) = self.result(&g, tl, sc, st, scale) { cmds.push(c); } }
                    else {
                        let id = g.clip.id.clone();
                        if self.last_click.as_ref().is_some_and(|(l, t0)| *l == id && now.duration_since(*t0).as_millis() <= DOUBLE_CLICK_MS) { cmds.push(LaneCmd::Precision(g.side, id.clone())); self.last_click = None; }
                        else { self.last_click = Some((id.clone(), now)); cmds.push(LaneCmd::Seek(sc.frame_at_x(x, st, scale) as i64)); }
                        self.selected = Some((g.side, id.clone())); cmds.push(LaneCmd::Select(Some((g.side, id))));
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
        let inset = 1.5 * scale; let radius = 2.0 * scale;
        let g = self.press.as_ref().filter(|g| g.active);
        let (preview, snap) = g.map(|g| self.result(g, tl, sc, st, scale)).unwrap_or((None, None));
        scene.push_clip_layer(peniko::Fill::NonZero, Affine::IDENTITY, &r);
        for side in [Side::A, Side::B] {
            let lane = self.lane(side); let (y0, y1) = (lane.y0 + inset, lane.y1 - inset);
            // the row's label cell in the scrubber's end inset: the side letter in its colour (as on the chips); the TARGET lane's
            // cell (⌘K and the keys act there) is filled with the side's colour under a white letter (canon row 263)
            let cell = Rect::new(lane.x0, lane.y0, lane.x0 + LABEL_W * scale, lane.y1);
            let target = self.target == side;
            if target { paint::rounded(scene, cell.inset(-1.0 * scale), 2.0 * scale, if side == Side::A { t::SIDE_A_SELECTED_BACKGROUND_COLOR } else { t::SIDE_B_SELECTED_BACKGROUND_COLOR }); }
            let l = fonts.layout(if side == Side::A { "A" } else { "B" }, Family::Sans, 10.0 * scale as f32, t::FONT_WEIGHT_BOLD as f32, paint::color(if target { t::WHITE } else if side == Side::A { t::SIDE_A_COLOR } else { t::SIDE_B_COLOR }));
            min_ui::fonts::draw_layout(scene, &l, Affine::translate((cell.x1 - l.width() as f64 - 4.0 * scale, cell.y0 + (cell.height() - l.height() as f64) / 2.0)));   // flush right, by the clips
            if self.live[side as usize] {
                // a live side has no clip: one bar across the lane in the side's wash, labelled LIVE — nothing to trim, slip, cut or
                // delete, nothing saved (QCView drew an editable full-width clip that did nothing; spec `step6-live-dual.md` §5.3)
                let lr = Rect::new(lane.x0 + LABEL_W * scale + 0.5 * scale, y0, lane.x1, y1);
                paint::rounded(scene, lr, radius, t::PLAYLIST_BAR_CLIP_COLOR);
                paint::rounded(scene, lr, radius, if side == Side::A { t::CLIP_BAR_SIDE_A_TINT_COLOR } else { t::CLIP_BAR_SIDE_B_TINT_COLOR });
                let ll = fonts.layout(min_i18n::trc("LiveStrip", "LIVE"), Family::Sans, 9.0 * scale as f32, t::FONT_WEIGHT_BOLD as f32, paint::color(if side == Side::A { t::SIDE_A_COLOR } else { t::SIDE_B_COLOR }));
                min_ui::fonts::draw_layout(scene, &ll, Affine::translate((lr.x0 + 6.0 * scale, lr.y0 + (lr.height() - ll.height() as f64) / 2.0)));
            }
            for c in clips(tl, side) {
                let (mut x0, mut x1) = Self::span(c, sc, st, scale);
                match &preview {
                    Some(LaneCmd::TrimTail { clip, new_source_out, .. }) if *clip == c.id => { x1 = sc.x_of_frame(c.start + rescale(*new_source_out - c.source_in, c.source_rate, tl.rate), st, scale); }
                    Some(LaneCmd::TrimHead { clip, new_start, .. }) if *clip == c.id => { x0 = sc.x_of_frame(*new_start, st, scale); }
                    Some(LaneCmd::Slide { side: s, clip, delta, carry }) if *s == side && (*clip == c.id || (*carry && g.is_some_and(|g| c.start >= g.clip.start))) => { x0 = sc.x_of_frame(c.start + delta, st, scale); x1 = sc.x_of_frame(c.end() + delta, st, scale); }
                    _ => {}
                }
                let selected = self.selected.as_ref().is_some_and(|(s, id)| *s == side && *id == c.id);
                let hovered = self.hover.as_ref().is_some_and(|h| h.0 == side && h.1 == c.id);
                let current = c.contains(st.current as i64);
                // Spectrum 2: neutral surfaces (the playlist bar's greys) under a slight wash of the side's colour (canon row 263)
                let fill = if selected { t::PLAYLIST_BAR_SELECTED_COLOR } else if hovered { t::PLAYLIST_BAR_CLIP_HOVER_COLOR } else if current { t::PLAYLIST_BAR_CURRENT_COLOR } else { t::PLAYLIST_BAR_CLIP_COLOR };
                // drawn inside the lane only: zoomed in on a long film a clip spans far off-screen, and vello's coverage breaks at those extents
                let (vx0, vx1) = (lane.x0 + LABEL_W * scale, lane.x1 + 4.0 * scale);   // never under the row label
                let cr = Rect::new((x0 + 0.5 * scale).max(vx0), y0, (x1 - 0.5 * scale).max(x0 + scale).min(vx1), y1);
                if cr.x1 <= cr.x0 { continue }
                paint::rounded(scene, cr, radius, fill);
                paint::rounded(scene, cr, radius, if side == Side::A { t::CLIP_BAR_SIDE_A_TINT_COLOR } else { t::CLIP_BAR_SIDE_B_TINT_COLOR });
                if let Some((s, id, z)) = &self.hover { if *s == side && *id == c.id && g.is_none() { let w = 2.0 * scale; let hc = t::PLAYLIST_BAR_HANDLE_COLOR; match z { Zone::Head => paint::rect(scene, Rect::new(cr.x0, y0, cr.x0 + w, y1), hc), Zone::Tail => paint::rect(scene, Rect::new(cr.x1 - w, y0, cr.x1, y1), hc), Zone::Body => {} } } }
            }
        }
        // the snap line across both lanes
        if let Some(f) = snap { let x = sc.x_of_frame(f, st, scale); paint::rect(scene, Rect::new(x - 0.5 * scale, r.y0, x + 0.5 * scale, r.y1), t::GRAY_900); }
        scene.pop_layer();
        // the badge above the lanes: the change and the resulting frame while dragging; the clip's offset on hover
        let text = if let (Some(g), Some(p)) = (g, &preview) {
            let (n, at) = match p {
                LaneCmd::TrimTail { new_source_out, .. } => (new_source_out - g.clip.source_out, g.clip.start + rescale(*new_source_out - g.clip.source_in, g.clip.source_rate, tl.rate) - 1),
                LaneCmd::TrimHead { new_start, .. } => (new_start - g.clip.start, *new_start),
                LaneCmd::Slide { delta, .. } => (*delta, g.clip.start + delta),
                LaneCmd::Slip { new_in, .. } => (new_in - g.clip.source_in, *new_in),
                _ => (0, 0),
            };
            Some(format!("{}{n} f  →  {at}", if n > 0 { "+" } else { "" }))
        } else { None };
        if let Some(txt) = text {
            let l = fonts.layout(&txt, Family::Mono, t::DETAIL_SIZE_S * scale as f32, t::FONT_WEIGHT_REGULAR as f32, paint::color(t::PLAYLIST_BAR_LABEL_COLOR));
            let pad = 6.0 * scale; let bw = l.width() as f64 + 2.0 * pad; let bh = l.height() as f64 + 4.0 * scale;
            let px = g.map(|g| g.x).unwrap_or(self.pointer.0);
            let bx = (px - bw / 2.0).clamp(r.x0, r.x1 - bw); let br = Rect::new(bx, r.y0 - bh - 4.0 * scale, bx + bw, r.y0 - 4.0 * scale);
            paint::rounded(scene, br, radius, t::GRAY_75);
            min_ui::fonts::draw_layout(scene, &l, Affine::translate((br.x0 + pad, br.y0 + (bh - l.height() as f64) / 2.0)));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use min_ui::Button;
    use qcv_timeline::{ClipMediaKind, Editor, MediaInfo, Rate};
    fn setup() -> (DualBars, Editor, Scrubber, ScrubState) {
        let m = |id: &str| MediaInfo { id: id.into(), name: id.into(), path: format!("/x/{id}.mov"), kind: ClipMediaKind::Video, rate: Rate::new(24, 1), duration: 300, width: 1920, height: 1080, has_audio: false, sequence_pattern: String::new(), sequence_start_frame: 1, sequence_end_frame: 1, sequence_exr_layer: String::new(), sequence_missing: vec![] };
        let mut ed = Editor::default(); ed.load_dual(Some(&m("a")), Some(&m("b")), Rate::new(24, 1));
        ed.split_clip_at(TRACK_B, 100);
        let st = ScrubState { frames: 300, fps: 24.0, current: 140, in_point: -1, out_point: -1, playing: false, scrubbing: false, cached: None };
        let mut sc = Scrubber::new(); sc.layout(Rect::new(0.0, 0.0, 1016.0, 32.0), 1.0);
        let mut bars = DualBars::new(); bars.rect = Rect::new(0.0, 32.0, 1016.0, 56.0);
        (bars, ed, sc, st)
    }
    fn drag(b: &mut DualBars, ed: &Editor, sc: &Scrubber, st: &ScrubState, y: f64, x0: f64, x1: f64, shift: bool) -> Vec<LaneCmd> {
        let now = Instant::now();
        b.event(&Event::Down { x: x0, y, button: Button::Primary }, now, false, shift, &ed.timeline, sc, st, 1.0);
        b.event(&Event::Move { x: x1, y }, now, false, shift, &ed.timeline, sc, st, 1.0);
        b.event(&Event::Up { x: x1, y, button: Button::Primary }, now, false, shift, &ed.timeline, sc, st, 1.0).0
    }
    #[test]
    fn a_slide_snaps_to_the_playhead_and_carries_unless_shift() {
        let (mut b, ed, sc, st) = setup();
        let id = ed.timeline.track(TRACK_B).unwrap().clips[1].id.clone();
        let x = |f: i64| sc.x_of_frame(f, &st, 1.0);
        // B's second clip (starts at 100) dragged by ~38 frames: its head lands near the playhead (140) and snaps exactly
        let c = drag(&mut b, &ed, &sc, &st, 50.0, x(150), x(188), false);
        assert!(c.contains(&LaneCmd::Slide { side: Side::B, clip: id.clone(), delta: 40, carry: true }), "{c:?}");
        // far from every target: no snap, the raw delta; Shift moves it alone
        let c = drag(&mut b, &ed, &sc, &st, 50.0, x(150), x(170), true);
        assert!(c.iter().any(|k| matches!(k, LaneCmd::Slide { delta: 19..=21, carry: false, .. })), "{c:?}");
        // a lane click makes its side the target
        let c = drag(&mut b, &ed, &sc, &st, 38.0, x(50), x(50), false);
        assert!(b.target == Side::A, "{c:?}");   // (the Target command rides on the press)
    }
    #[test]
    fn a_trim_snaps_to_the_other_sides_cut() {
        let (mut b, ed, sc, st) = setup();
        let a_id = ed.timeline.track(TRACK_A).unwrap().clips[0].id.clone();
        let x = |f: i64| sc.x_of_frame(f, &st, 1.0);
        // A's head dragged from 0 toward B's cut at 100 (the playhead at 140 is out of reach): snaps to 100
        let c = drag(&mut b, &ed, &sc, &st, 38.0, x(0) + 2.0, x(98) + 2.0, false);   // 2 frames short: inside the 8-px reach
        assert!(c.iter().any(|k| matches!(k, LaneCmd::TrimHead { clip, new_start: 100, new_source_in: 100, .. } if *clip == a_id)), "{c:?}");
    }
}
