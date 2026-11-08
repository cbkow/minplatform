//! Dual view's controls on the right of the status strip (canon row 173; QCView `ViewportOverlay.qml` carried, its strings
//! verbatim): the A / B chips (the side letter in the side colour, the clip's name middle-elided, QCView's empty-side
//! placeholders, B's × keeps dual with B blank; both chips are drop targets with an accent outline while something hovers),
//! the four compare modes as one group, the slider (the seam 0–1 in Split wipe, the gain ×1–×16 in Difference) with its
//! readout, and Update / Save. In Single only the A chip and the modes show; for a playlist only the A chip (QCView's
//! `dualUnavailable`).
use kurbo::{Affine, Rect, RoundedRect, Stroke};
use min_present::dual::CompareMode;
use min_theme::tokens as t;
use min_ui::{anim::{mix, Ramp}, paint, ActionButton, ActionGroup, Event, Family, Fonts, Slider, Tooltips};
use qcv_player::Side;
use std::time::Instant;
use vello::Scene;

fn tr(s: &str) -> &str { min_i18n::trc("ViewportOverlay", s) }

#[derive(Clone, Debug, PartialEq)]
pub enum Cmd { Mode(CompareMode), ClearB, Seam(f32), Gain(f32), Save, Update }

/// QCView's gain range (`diffGain` 1..16).
pub const GAIN_MAX: f32 = 16.0;
const MODES: [CompareMode; 4] = [CompareMode::Single, CompareMode::SideBySide, CompareMode::Split, CompareMode::Difference];

struct Chip { rect: Rect, name: Option<String>, hovered: bool, hover: Ramp }
impl Chip { fn new() -> Chip { Chip { rect: Rect::ZERO, name: None, hovered: false, hover: Ramp::new(0.0, t::ANIMATION_DURATION_HOVER) } } }

pub struct DualControls {
    pub rect: Rect,
    /// dual is on (any mode but Single); `available` = not a playlist; `saved` = the session came from a saved Dual View
    active: bool, available: bool, saved: bool, mode: CompareMode, seam: f32, gain: f32,
    a: Chip, b: Chip, close_b: ActionButton, modes: ActionGroup, slider: Slider, readout: String, readout_rect: Rect,
    update: ActionButton, save: ActionButton,
    drop_side: Option<Side>,
    pub tooltips: Tooltips,
}

impl DualControls {
    pub fn new() -> DualControls {
        let modes = ActionGroup::new(vec![
            ActionButton::new("rectangle", tr("Single view")).size_s(), ActionButton::new("square-split-horizontal", tr("Side-by-side")).size_s(),
            ActionButton::new("arrows-out-line-horizontal", tr("Split wipe")).size_s(), ActionButton::new("intersect-square", tr("Difference")).size_s()]).compact();
        DualControls { rect: Rect::ZERO, active: false, available: true, saved: false, mode: CompareMode::Single, seam: 0.5, gain: 1.0,
            a: Chip::new(), b: Chip::new(), close_b: ActionButton::new("x", tr("Close source B")).size_s(), modes, slider: Slider::new(0.5), readout: String::new(), readout_rect: Rect::ZERO,
            update: ActionButton::new("floppy-disk", tr("Update this saved dual view")).size_s(), save: ActionButton::new("floppy-disk", tr("Save dual view")).size_s(),
            drop_side: None, tooltips: Tooltips::default() }
    }
    /// The state to show; returns true when something visible changed.
    #[allow(clippy::too_many_arguments)]
    pub fn sync(&mut self, active: bool, available: bool, mode: CompareMode, a: Option<String>, b: Option<String>, seam: f32, gain: f32, saved: bool) -> bool {
        let mode = if active { mode } else { CompareMode::Single };
        let was = (self.active, self.available, self.mode, self.a.name.clone(), self.b.name.clone(), self.seam, self.gain, self.saved);
        self.active = active; self.available = available; self.mode = mode; self.a.name = a; self.b.name = b; self.seam = seam; self.gain = gain; self.saved = saved;
        for (i, btn) in self.modes.buttons.iter_mut().enumerate() { btn.selected = MODES[i] == mode; }
        if !self.slider.dragging() { self.slider.value = match mode { CompareMode::Difference => (gain - 1.0) / (GAIN_MAX - 1.0), _ => seam }; }
        self.readout = match mode { CompareMode::Difference => format!("×{gain:.1}"), _ => format!("{seam:.2}") };
        // QCView: Save becomes "Save as a new dual view" (file-plus) once the session is a saved one
        self.save.icon = if saved { "file-plus" } else { "floppy-disk" };
        self.save.tooltip = tr(if saved { "Save as a new dual view" } else { "Save dual view" }).to_string();
        was != (self.active, self.available, self.mode, self.a.name.clone(), self.b.name.clone(), self.seam, self.gain, self.saved)
    }
    fn shows_b(&self) -> bool { self.active && self.available }
    fn shows_slider(&self) -> bool { self.shows_b() && matches!(self.mode, CompareMode::Split | CompareMode::Difference) }
    /// Right-aligned, ending at `right`, centred in the band [y, y + h]; never left of `min_x` (the chips shrink first).
    pub fn layout(&mut self, fonts: &mut Fonts, min_x: f64, right: f64, y: f64, h: f64, scale: f64) {
        let gap = t::SPACING_100 as f64 * scale; let cluster = t::SPACING_300 as f64 * scale;
        let bh = t::ACTION_BUTTON_HEIGHT_S as f64 * scale; let by = y + (h - bh) / 2.0;
        let mut x = right;
        let mut place = |b: &mut ActionButton, x: &mut f64| { let (w, _) = b.measure(fonts, scale); b.rect = Rect::new(*x - w, by, *x, by + bh); *x -= w; };
        if self.shows_b() {
            place(&mut self.save, &mut x); x -= gap;
            if self.saved { place(&mut self.update, &mut x); x -= gap; } else { self.update.rect = Rect::ZERO; }
            x -= cluster - gap;
        } else { self.save.rect = Rect::ZERO; self.update.rect = Rect::ZERO; }
        if self.shows_slider() {
            let rw = 40.0 * scale; self.readout_rect = Rect::new(x - rw, by, x, by + bh); x -= rw + gap;
            let sw = 160.0 * scale; self.slider.rect = Rect::new(x - sw, by, x, by + bh); x -= sw + cluster;
        } else { self.readout_rect = Rect::ZERO; self.slider.rect = Rect::ZERO; }
        if self.available {
            // the group measures itself from its left edge: place it, then shift
            let w = self.modes.layout(fonts, 0.0, by, scale);
            self.modes.layout(fonts, x - w, by, scale); x -= w + cluster;
        } else { self.modes.rect = Rect::ZERO; for b in &mut self.modes.buttons { b.rect = Rect::ZERO; } }
        // the chips share what is left, QCView's 240 each at most
        let n = if self.shows_b() { 2.0 } else { 1.0 };
        let cw = (((x - min_x) - gap * (n - 1.0)) / n).clamp(0.0, 240.0 * scale);
        let ch = t::FIELD_HEIGHT_S as f64 * scale; let cy = y + (h - ch) / 2.0;
        if self.shows_b() {
            self.b.rect = Rect::new(x - cw, cy, x, cy + ch); x -= cw + gap;
            let cs = 16.0 * scale; self.close_b.rect = if self.b.name.is_some() { Rect::new(self.b.rect.x1 - cs - 4.0 * scale, cy + (ch - cs) / 2.0, self.b.rect.x1 - 4.0 * scale, cy + (ch + cs) / 2.0) } else { Rect::ZERO };
        } else { self.b.rect = Rect::ZERO; self.close_b.rect = Rect::ZERO; }
        self.a.rect = if cw >= 48.0 * scale { Rect::new(x - cw, cy, x, cy + ch) } else { Rect::ZERO };
        self.rect = Rect::new(self.a.rect.x0.min(x), y, right, y + h);
    }
    /// The chip under the pointer (a drop's side).
    pub fn chip_at(&self, x: f64, y: f64) -> Option<Side> {
        let p = kurbo::Point::new(x, y);
        if self.a.rect.contains(p) { Some(Side::A) } else if self.b.rect.contains(p) { Some(Side::B) } else { None }
    }
    /// A drag hovering a chip (the accent outline); returns true when it changed.
    pub fn set_drop_side(&mut self, s: Option<Side>) -> bool { if self.drop_side == s { return false } self.drop_side = s; true }
    /// Returns (commands, redraw needed, consumed).
    pub fn event(&mut self, e: &Event, now: Instant, scale: f64) -> (Vec<Cmd>, bool, bool) {
        let mut cmds = vec![]; let mut dirty = false;
        let inside = e.pos().is_some_and(|(x, y)| self.rect.contains(kurbo::Point::new(x, y))) && !matches!(e, Event::Leave);
        for c in [&mut self.a, &mut self.b] { let h = inside && e.pos().is_some_and(|(x, y)| c.rect.contains(kurbo::Point::new(x, y))); if h != c.hovered { c.hovered = h; c.hover.set_target(if h { 1.0 } else { 0.0 }, now); dirty = true; } }
        if self.close_b.rect.area() > 0.0 { match self.close_b.event(e, now) { min_ui::Response::Clicked => { cmds.push(Cmd::ClearB); dirty = true; } min_ui::Response::None => {} _ => dirty = true } }
        for (i, r) in self.modes.event(e, now) { dirty = true; if r == min_ui::Response::Clicked { cmds.push(Cmd::Mode(MODES[i])); } }
        if self.slider.rect.area() > 0.0 {
            let (v, d) = self.slider.event(e, now, scale); dirty |= d;
            if let Some(v) = v { cmds.push(match self.mode { CompareMode::Difference => Cmd::Gain(1.0 + v * (GAIN_MAX - 1.0)), _ => Cmd::Seam(v) }); }
        }
        for (b, c) in [(&mut self.update, Cmd::Update), (&mut self.save, Cmd::Save)] { if b.rect.area() > 0.0 { match b.event(e, now) { min_ui::Response::Clicked => { cmds.push(c); dirty = true; } min_ui::Response::None => {} _ => dirty = true } } }
        // tooltips: the buttons' and the chips' full names (they elide)
        let over = e.pos().filter(|_| !matches!(e, Event::Leave)).and_then(|(x, y)| {
            let p = kurbo::Point::new(x, y);
            let btns = self.modes.buttons.iter().chain([&self.close_b, &self.update, &self.save]);
            for (i, b) in btns.enumerate() { if b.rect.contains(p) { return Some((100 + i as u64, b.rect, b.tooltip.clone())) } }
            if self.a.rect.contains(p) { if let Some(n) = &self.a.name { return Some((1, self.a.rect, n.clone())) } }
            if self.b.rect.contains(p) { if let Some(n) = &self.b.name { return Some((2, self.b.rect, n.clone())) } }
            None
        });
        if self.tooltips.hover(over.as_ref().map(|(k, r, s)| (*k, *r, s.as_str())), now) { dirty = true; }
        if matches!(e, Event::Down { .. }) && inside { self.tooltips.pressed(now); }
        (cmds, dirty, inside || self.slider.dragging())
    }
    pub fn dragging(&self) -> bool { self.slider.dragging() }
    pub fn tick(&mut self, now: Instant) -> bool { self.tooltips.tick(now) }
    pub fn animating(&self, now: Instant) -> bool {
        self.a.hover.animating(now) || self.b.hover.animating(now) || self.close_b.animating(now) || self.modes.animating(now) || self.slider.animating(now) || self.update.animating(now) || self.save.animating(now)
    }
    pub fn paint(&mut self, scene: &mut Scene, fonts: &mut Fonts, scale: f64, now: Instant) {
        let r = t::FIELD_CORNER_RADIUS_S as f64 * scale; let pad = t::FIELD_EDGE_TO_TEXT as f64 * scale;
        for (side, c) in [(Side::A, &self.a), (Side::B, &self.b)] {
            if c.rect.area() <= 0.0 { continue }
            let bg = mix(t::FIELD_BACKGROUND_COLOR, t::HOVER_BACKGROUND_COLOR, c.hover.value(now));
            paint::rounded(scene, c.rect, r, bg);
            let letter_c = if side == Side::A { t::SIDE_A_COLOR } else { t::SIDE_B_COLOR };
            let l = fonts.layout(if side == Side::A { tr("A") } else { tr("B") }, Family::Sans, t::RAIL_LABEL_SIZE * scale as f32, t::FONT_WEIGHT_BOLD as f32, paint::color(letter_c));
            let ly = c.rect.y0 + (c.rect.height() - l.height() as f64) / 2.0;
            min_ui::fonts::draw_layout(scene, &l, Affine::translate((c.rect.x0 + pad, ly)));
            let nx = c.rect.x0 + pad + l.width() as f64 + t::SPACING_100 as f64 * scale;
            let right = if side == Side::B && self.close_b.rect.area() > 0.0 { self.close_b.rect.x0 - 4.0 * scale } else { c.rect.x1 - pad };
            let (text, color, weight) = match &c.name {
                Some(n) => (n.clone(), t::NEUTRAL_CONTENT_COLOR_DEFAULT, t::FONT_WEIGHT_REGULAR as f32),
                None => (tr(if side == Side::A { "(drop a file…)" } else { "(drop a video, sequence or stream…)" }).to_string(), t::NEUTRAL_SUBDUED_CONTENT_COLOR_DEFAULT, t::FONT_WEIGHT_REGULAR as f32),
            };
            let nl = crate::project_panel::elided(fonts, &text, weight, color, (right - nx).max(0.0), scale);
            min_ui::fonts::draw_layout(scene, &nl, Affine::translate((nx, c.rect.y0 + (c.rect.height() - nl.height() as f64) / 2.0)));
            if self.drop_side == Some(side) {
                let bw = 2.0 * scale;
                scene.stroke(&Stroke::new(bw), Affine::IDENTITY, paint::color(t::FOCUS_INDICATOR_COLOR), None, &RoundedRect::from_rect(c.rect.inset(-bw / 2.0), r));
            }
        }
        if self.close_b.rect.area() > 0.0 { self.close_b.paint(scene, fonts, scale, now); }
        if self.modes.rect.area() > 0.0 { self.modes.paint(scene, fonts, scale, now); }
        if self.slider.rect.area() > 0.0 {
            self.slider.paint(scene, scale, now);
            let l = fonts.layout(&self.readout, Family::Mono, t::CODE_SIZE_S * scale as f32, t::FONT_WEIGHT_REGULAR as f32, paint::color(t::NEUTRAL_SUBDUED_CONTENT_COLOR_DEFAULT));
            min_ui::fonts::draw_layout(scene, &l, Affine::translate((self.readout_rect.x0, self.readout_rect.y0 + (self.readout_rect.height() - l.height() as f64) / 2.0)));
        }
        if self.update.rect.area() > 0.0 { self.update.paint(scene, fonts, scale, now); }
        if self.save.rect.area() > 0.0 { self.save.paint(scene, fonts, scale, now); }
    }
    pub fn paint_tooltip(&self, scene: &mut Scene, fonts: &mut Fonts, scale: f64, bounds: Rect, now: Instant) { self.tooltips.paint(scene, fonts, scale, bounds, now); }
}
