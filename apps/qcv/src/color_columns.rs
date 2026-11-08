//! The Color drawer's two optional columns (QCView ColorPanel.qml's KneeColumn and AgxColumn, carried whole; canon row 199,
//! the user 2026-11-08: "We need the full Highlight knee interface ported with sliders … AGX is like highlight knee: an
//! optional custom interface and logic"). Each is a layer-2 card under its caption that collapses to a 32-px strip (a
//! vertical title, an amber dot while it is on); expanded, the controls scroll inside the card. Each holds a copy of the
//! clip's minColor chain and hands back the edited chain.
use kurbo::{Affine, Rect};
use min_theme::tokens as t;
use min_ui::{anim::{mix, Ramp}, paint, ActionButton, ActionGroup, Button, Event, Family, Fonts, KeyEvent, NumberField, Response, ScrollBar, Slider, Switch, TextButton, Variant};
use qcv_color::Chain;
use std::time::Instant;
use vello::Scene;

fn tr(s: &'static str) -> &'static str { min_i18n::trc("ColorPanel", s) }

pub const EXPANDED_W: f64 = 210.0;
/// AgX's target-gamut chips need the room
pub const AGX_W: f64 = 250.0;
pub const COLLAPSED_W: f64 = 32.0;
const DOUBLE_CLICK_MS: u128 = 400;

// ST 2084, y = nits / 10000 (QCView linear_stage.h)
fn pq_enc(y: f32) -> f32 { let (m1, m2, c1, c2, c3) = (0.1593017578125f32, 78.84375f32, 0.8359375f32, 18.8515625f32, 18.6875f32); let p = y.max(0.0).powf(m1); ((c1 + c2 * p) / (1.0 + c3 * p)).powf(m2) }
fn pq_dec(e: f32) -> f32 { let (m1, m2, c1, c2, c3) = (0.1593017578125f32, 78.84375f32, 0.8359375f32, 18.8515625f32, 18.6875f32); let p = e.clamp(0.0, 1.0).powf(1.0 / m2); ((p - c1).max(0.0) / (c2 - c3 * p)).powf(1.0 / m1) }
/// The knee start the engine uses, as a fraction of the target in PQ (BT.2390's when the chain says auto).
fn start_effective(c: &Chain) -> f32 {
    let ml = pq_enc(c.knee_tgt.max(1.0) / 1e4) / pq_enc(c.knee_src.max(1.0) / 1e4);
    if c.knee_start < 0.0 { if ml <= 0.0 { 0.0 } else { (1.5 * ml - 0.5).max(0.0) / ml } } else { c.knee_start }
}
fn start_nits(c: &Chain) -> f32 { pq_dec(start_effective(c) * pq_enc(c.knee_tgt.max(1.0) / 1e4)) * 1e4 }

/// The frame both columns share: caption + collapse caret, the card, its scroll, the strip.
struct Frame { title: &'static str, pub expanded: bool, open_w: f64, /// a hover change the panel's event reports
    dirty: bool, rect: Rect, card: Rect, collapse: Rect, collapse_hover: bool, strip_hover: Ramp, sb: ScrollBar, content_h: f64, scale: f64 }
impl Frame {
    fn new(title: &'static str) -> Frame { Frame { title, expanded: false, open_w: EXPANDED_W, dirty: false, rect: Rect::ZERO, card: Rect::ZERO, collapse: Rect::ZERO, collapse_hover: false, strip_hover: Ramp::new(0.0, t::ANIMATION_DURATION_HOVER), sb: ScrollBar::default(), content_h: 0.0, scale: 1.0 } }
    fn width(&self) -> f64 { if self.expanded { self.open_w } else { COLLAPSED_W } }
    fn layout(&mut self, x: f64, y0: f64, y1: f64, scale: f64) -> f64 {
        self.scale = scale; let w = self.width() * scale; let cap = t::COMPONENT_HEIGHT_75 as f64 * scale;
        self.rect = Rect::new(x, y0, x + w, y1);
        self.card = if self.expanded { Rect::new(x, y0 + cap, x + w, y1) } else { Rect::new(x, y0 + cap, x + w, y1) };
        let b = t::ICON_SIZE_SMALL as f64 * scale + 4.0 * scale;
        self.collapse = Rect::new(x + w - b, y0 + (cap - b) / 2.0, x + w, y0 + (cap + b) / 2.0);
        w
    }
    /// The controls' origin (scrolled) and width inside the card.
    fn inner(&self) -> (f64, f64, f64) { let p = t::SPACING_100 as f64 * self.scale; (self.card.x0 + p, self.card.y0 + p - self.sb.offset, self.card.width() - 2.0 * p - t::SPACING_100 as f64 * self.scale) }
    fn finish_layout(&mut self, content_h: f64) { self.content_h = content_h; self.sb.set(self.card, content_h + 2.0 * t::SPACING_100 as f64 * self.scale, self.scale); }
    /// Frame events first. Some(dirty) when the frame took the event.
    fn event(&mut self, e: &Event, now: Instant) -> Option<bool> {
        let over_card = e.pos().is_some_and(|(x, y)| self.card.contains(kurbo::Point::new(x, y)));
        if !self.expanded {
            let h = over_card; self.strip_hover.set_target(if h { 1.0 } else { 0.0 }, now);
            if let Event::Down { button: Button::Primary, .. } = e { if over_card { self.expanded = true; return Some(true) } }
            return Some(self.strip_hover.animating(now) || matches!(e, Event::Move { .. }) && over_card)
        }
        let ch = e.pos().is_some_and(|(x, y)| self.collapse.contains(kurbo::Point::new(x, y)));
        let mut d = ch != self.collapse_hover; self.collapse_hover = ch;
        if let Event::Down { button: Button::Primary, .. } = e { if ch { self.expanded = false; self.sb.offset = 0.0; return Some(true) } }
        let (m, mine) = self.sb.event(e, now); d |= m;
        if mine { return Some(d) }
        self.dirty = d;
        None
    }
    fn over_card(&self, e: &Event) -> bool { e.pos().is_some_and(|(x, y)| self.card.contains(kurbo::Point::new(x, y))) }
    fn wheel(&mut self, x: f64, y: f64, dy: f64, now: Instant) -> bool { self.expanded && self.card.contains(kurbo::Point::new(x, y)) && self.sb.wheel(dy, now) }
    fn paint_frame(&self, scene: &mut Scene, fonts: &mut Fonts, on: bool, now: Instant) {
        let s = self.scale; let cap = t::COMPONENT_HEIGHT_75 as f64 * s;
        let radius = t::CORNER_RADIUS_500 as f64 * s;
        if self.expanded {
            let l = fonts.detail(self.title, t::RAIL_GROUP_HEADER_SIZE * s as f32, paint::color(t::RAIL_GROUP_HEADER_COLOR));
            min_ui::fonts::draw_layout(scene, &l, Affine::translate((self.rect.x0 + t::SPACING_50 as f64 * s, self.rect.y0 + (cap - l.height() as f64) / 2.0)));
            if on { let dot = 6.0 * s; let dx = self.rect.x0 + t::SPACING_50 as f64 * s + l.width() as f64 + t::SPACING_100 as f64 * s; paint::rounded(scene, Rect::new(dx, self.rect.y0 + (cap - dot) / 2.0, dx + dot, self.rect.y0 + (cap + dot) / 2.0), dot / 2.0, t::NOTICE_VISUAL_COLOR); }
            if self.collapse_hover { paint::rounded(scene, self.collapse, t::CORNER_RADIUS_SMALL_DEFAULT as f64 * s, t::HOVER_BACKGROUND_COLOR); }
            if let Some(g) = fonts.icon("caret-left", t::ICON_SIZE_SMALL * s as f32, paint::color(if self.collapse_hover { t::NEUTRAL_CONTENT_COLOR_HOVER } else { t::RAIL_OPEN_TAB_COLOR })) { min_ui::fonts::draw_layout(scene, &g, Affine::translate((self.collapse.x0 + (self.collapse.width() - g.width() as f64) / 2.0, self.collapse.y0 + (self.collapse.height() - g.height() as f64) / 2.0))); }
            paint::rounded(scene, self.card, radius, t::BACKGROUND_LAYER_2_COLOR);
        } else {
            // the strip: QCView's collapsed body — a vertical title, an amber mark while on, hover = brighter
            let bg = mix(t::BACKGROUND_LAYER_2_COLOR, t::HOVER_BACKGROUND_COLOR, self.strip_hover.value(now));
            paint::rounded(scene, self.card, radius, bg);
            let l = fonts.layout(self.title, Family::Sans, t::RAIL_LABEL_SIZE * s as f32, t::FONT_WEIGHT_BOLD as f32, paint::color(if on { t::RAIL_LABEL_COLOR } else { t::RAIL_GROUP_HEADER_COLOR }));
            let cx = self.card.x0 + (self.card.width() - l.height() as f64) / 2.0; let top = self.card.y0 + t::SPACING_200 as f64 * s + if on { 12.0 * s } else { 0.0 };
            min_ui::fonts::draw_layout(scene, &l, Affine::translate((cx, top + l.width() as f64)) * Affine::rotate(-std::f64::consts::FRAC_PI_2));
            if on { let dot = 6.0 * s; let dx = self.card.x0 + (self.card.width() - dot) / 2.0; paint::rounded(scene, Rect::new(dx, self.card.y0 + t::SPACING_200 as f64 * s, dx + dot, self.card.y0 + t::SPACING_200 as f64 * s + dot), dot / 2.0, t::NOTICE_VISUAL_COLOR); }
        }
    }
}

fn label(scene: &mut Scene, fonts: &mut Fonts, text: &str, x: f64, y: f64, h: f64, s: f64, color: min_theme::Color, mono: bool) -> f64 {
    let l = fonts.layout(text, if mono { Family::Mono } else { Family::Sans }, if mono { t::CODE_SIZE_S } else { t::RAIL_LABEL_SIZE } * s as f32, t::FONT_WEIGHT_REGULAR as f32, paint::color(color));
    min_ui::fonts::draw_layout(scene, &l, Affine::translate((x, y + (h - l.height() as f64) / 2.0))); l.width() as f64
}
fn label_right(scene: &mut Scene, fonts: &mut Fonts, text: &str, x1: f64, y: f64, h: f64, s: f64, color: min_theme::Color) {
    let l = fonts.layout(text, Family::Mono, t::CODE_SIZE_S * s as f32, t::FONT_WEIGHT_REGULAR as f32, paint::color(color));
    min_ui::fonts::draw_layout(scene, &l, Affine::translate((x1 - l.width() as f64, y + (h - l.height() as f64) / 2.0)));
}

// ---------------------------------------------------------------------------------------------------------------------
// The Highlight Knee (QCView KneeColumn)

pub struct KneePanel {
    f: Frame, chain: Chain, file: (i32, f64),
    on: Switch, src: NumberField, use_file: TextButton, tgt: NumberField, start: Slider, reset: ActionButton,
    rows: Vec<(f64, f64)>, last_press: Option<Instant>,
    /// an OCIO SDR view: the target is 100 nits by rule (QCView's "100 nits · SDR display"), no field
    sdr: bool,
}

impl KneePanel {
    pub fn new() -> KneePanel {
        KneePanel { f: Frame::new(tr("Highlight Knee")), chain: Chain::default(), file: (0, 0.0), on: Switch::new(false), src: NumberField::new(1000, 100, 10000, 100), use_file: TextButton::new("", Variant::Secondary).size_s(),
            tgt: NumberField::new(100, 100, 10000, 50), start: Slider::new(0.5), reset: ActionButton::new("arrow-counter-clockwise", tr("Reset knee start to BT.2390")).size_s(), rows: vec![], last_press: None, sdr: false }
    }
    pub fn set_expanded(&mut self, e: bool) { self.f.expanded = e; }
    pub fn set_sdr_view(&mut self, sdr: bool) { self.sdr = sdr; }
    /// The column's width now (logical px), for the drawer's row of columns.
    pub fn width(&self) -> f64 { self.f.width() }
    pub fn focused(&self) -> bool { self.src.focused() || self.tgt.focused() }
    pub fn dragging(&self) -> bool { self.start.dragging() || self.f.sb.dragging() }
    pub fn sync(&mut self, c: &Chain, file: (i32, f64), now: Instant) {
        self.chain = *c; self.file = file;
        self.on.set(c.knee_on != 0, now);
        self.src.set(c.knee_src.round() as i64); self.tgt.set(c.knee_tgt.round() as i64);
        if !self.start.dragging() { self.start.value = (start_effective(c) / 0.99).clamp(0.0, 1.0); }
        self.use_file.label = if file.0 > 0 { min_i18n::fill(tr("Use file MaxCLL (%1)"), &[&file.0]) } else if file.1 > 0.0 { min_i18n::fill(tr("Use mastering peak (%1)"), &[&(file.1.round() as i64)]) } else { tr("No MaxCLL in file").to_string() };
        self.use_file.enabled = file.0 > 0 || file.1 > 0.0;
        self.reset.enabled = c.knee_start >= 0.0;
    }
    pub fn layout(&mut self, fonts: &mut Fonts, x: f64, y0: f64, y1: f64, scale: f64) -> f64 {
        let w = self.f.layout(x, y0, y1, scale);
        if !self.f.expanded { return w }
        let (ix, iy, iw) = self.f.inner(); let s = scale;
        let lh = 20.0 * s; let row = t::COMPONENT_HEIGHT_100 as f64 * s; let fh = t::FIELD_HEIGHT_S as f64 * s; let unit = 30.0 * s;
        let (sw, sh) = Switch::size(s);
        let mut y = iy; self.rows.clear();
        self.on.rect = Rect::new(ix, y + (row - sh) / 2.0, ix + sw, y + (row + sh) / 2.0); self.rows.push((y, row)); y += row;
        self.rows.push((y, lh)); y += lh;                                                          // "Source peak"
        self.src.layout(fonts, Rect::new(ix, y + (row - fh) / 2.0, ix + iw - unit, y + (row + fh) / 2.0), s); self.rows.push((y, row)); y += row;
        self.rows.push((y, lh)); y += lh;                                                          // "+N stops over SDR white"
        let (_, bh) = self.use_file.measure(fonts, s); self.use_file.rect = Rect::new(ix, y + (row - bh) / 2.0, ix + iw, y + (row + bh) / 2.0); self.rows.push((y, row)); y += row;
        self.rows.push((y, lh)); y += lh;                                                          // "Target peak"
        self.tgt.layout(fonts, Rect::new(ix, y + (row - fh) / 2.0, ix + iw - unit, y + (row + fh) / 2.0), s); self.rows.push((y, row)); y += row;
        self.rows.push((y, lh)); y += lh;                                                          // "Knee start"
        self.start.rect = Rect::new(ix, y, ix + iw, y + row); self.rows.push((y, row)); y += row;
        let (rw, rh) = self.reset.measure(fonts, s); self.reset.rect = Rect::new(ix + iw - rw, y + (row - rh) / 2.0, ix + iw, y + (row + rh) / 2.0); self.rows.push((y, row)); y += row;
        self.f.finish_layout(y - iy);
        w
    }
    /// The edited chain, and whether to redraw.
    pub fn event(&mut self, e: &Event, now: Instant, fonts: &mut Fonts) -> (Option<Chain>, bool) {
        if let Some(d) = self.f.event(e, now) { return (None, d) }
        if !self.f.expanded { return (None, false) }
        let live = self.f.over_card(e) || self.dragging() || matches!(e, Event::Up { .. } | Event::Leave);
        if !live { return (None, false) }
        let mut c = self.chain; let mut changed = false; let mut d = std::mem::take(&mut self.f.dirty);
        let (v, dd) = self.on.event(e, now); d |= dd; if let Some(on) = v { c.knee_on = on as i32; changed = true; }
        let (v, dd) = self.src.event(e, fonts); d |= dd; if let Some(v) = v { c.knee_src = v as f32; changed = true; }
        if !self.sdr { let (v, dd) = self.tgt.event(e, fonts); d |= dd; if let Some(v) = v { c.knee_tgt = v as f32; changed = true; } }
        let (clicked, dd) = self.use_file.event(e, now); d |= dd; if clicked { c.knee_src = if self.file.0 > 0 { self.file.0 as f32 } else { self.file.1 as f32 }; changed = true; }
        match self.reset.event(e, now) { Response::Clicked => { c.knee_start = -1.0; changed = true; } Response::None => {} _ => d = true }
        if let Event::Down { x, y, button: Button::Primary } = *e { if self.start.hit(x, y) {
            if self.last_press.is_some_and(|t0| now.duration_since(t0).as_millis() < DOUBLE_CLICK_MS) { c.knee_start = -1.0; self.last_press = None; self.chain = c; return (Some(c), true) }
            self.last_press = Some(now);
        } }
        let (v, dd) = self.start.event(e, now, self.f.scale); d |= dd; if let Some(v) = v { c.knee_start = (v * 0.99 * 100.0).round() / 100.0; changed = true; }
        if changed { self.chain = c; (Some(c), true) } else { (None, d) }
    }
    pub fn key(&mut self, k: &KeyEvent, fonts: &mut Fonts) -> Option<Chain> {
        let mut c = self.chain;
        if self.src.focused() { c.knee_src = self.src.key(k, fonts)? as f32; } else if self.tgt.focused() { c.knee_tgt = self.tgt.key(k, fonts)? as f32; } else { return None }
        self.chain = c; Some(c)
    }
    pub fn blur(&mut self) -> Option<Chain> {
        let mut c = self.chain; let mut ch = false;
        if let Some(v) = self.src.blur() { c.knee_src = v as f32; ch = true; }
        if let Some(v) = self.tgt.blur() { c.knee_tgt = v as f32; ch = true; }
        ch.then(|| { self.chain = c; c })
    }
    pub fn wheel(&mut self, x: f64, y: f64, dy: f64, now: Instant) -> bool { self.f.wheel(x, y, dy, now) }
    pub fn animating(&self, now: Instant) -> bool { self.f.strip_hover.animating(now) || self.f.sb.animating(now) || self.on.animating(now) || self.start.animating(now) || self.use_file.animating(now) }
    pub fn paint(&mut self, scene: &mut Scene, fonts: &mut Fonts, now: Instant) {
        let c = self.chain; let on = c.knee_on != 0;
        self.f.paint_frame(scene, fonts, on, now);
        if !self.f.expanded { return }
        let s = self.f.scale; let (ix, _, iw) = self.f.inner();
        scene.push_clip_layer(peniko::Fill::NonZero, Affine::IDENTITY, &self.f.card);
        let r = |i: usize, rows: &[(f64, f64)]| rows.get(i).copied().unwrap_or((0.0, 0.0));
        self.on.paint(scene, s, now);
        let (y, h) = r(0, &self.rows); let l = fonts.layout(if on { tr("On") } else { tr("Off") }, Family::Sans, t::RAIL_LABEL_SIZE * s as f32, if on { t::FONT_WEIGHT_BOLD } else { t::FONT_WEIGHT_REGULAR } as f32, paint::color(if on { t::NOTICE_VISUAL_COLOR } else { t::RAIL_GROUP_HEADER_COLOR }));
        min_ui::fonts::draw_layout(scene, &l, Affine::translate((self.on.rect.x1 + t::SPACING_100 as f64 * s, y + (h - l.height() as f64) / 2.0)));
        let (y, h) = r(1, &self.rows); label(scene, fonts, tr("Source peak"), ix, y, h, s, t::RAIL_GROUP_HEADER_COLOR, false);
        self.src.paint(scene, fonts, s); let (y, h) = r(2, &self.rows); label(scene, fonts, tr("nits"), self.src.rect.x1 + t::SPACING_75 as f64 * s, y, h, s, t::DISABLED_CONTENT_COLOR, false);
        let (y, h) = r(3, &self.rows); label(scene, fonts, &min_i18n::fill(tr("+%1 stops over SDR white"), &[&format!("{:.1}", (c.knee_src.max(1.0) / 100.0).log2())]), ix, y, h, s, t::DISABLED_CONTENT_COLOR, true);
        self.use_file.paint(scene, fonts, s, now);
        let (y, h) = r(5, &self.rows); label(scene, fonts, tr("Target peak"), ix, y, h, s, t::RAIL_GROUP_HEADER_COLOR, false);
        let (y, h) = r(6, &self.rows);
        if self.sdr { label(scene, fonts, tr("100 nits · SDR display"), ix, y, h, s, t::RAIL_LABEL_COLOR, true); }
        else { self.tgt.paint(scene, fonts, s); label(scene, fonts, tr("nits"), self.tgt.rect.x1 + t::SPACING_75 as f64 * s, y, h, s, t::DISABLED_CONTENT_COLOR, false); }
        let (y, h) = r(7, &self.rows); label(scene, fonts, tr("Knee start"), ix, y, h, s, t::RAIL_GROUP_HEADER_COLOR, false);
        self.start.paint(scene, s, now);
        let (y, h) = r(9, &self.rows);
        let w1 = label(scene, fonts, &min_i18n::fill(tr("%1 nits"), &[&(start_nits(&c).round() as i64)]), ix, y, h, s, t::RAIL_LABEL_COLOR, true);
        if c.knee_start < 0.0 { label(scene, fonts, "BT.2390", ix + w1 + t::SPACING_100 as f64 * s, y, h, s, t::DISABLED_CONTENT_COLOR, false); }
        if c.knee_start >= 0.0 { self.reset.paint(scene, fonts, s, now); }
        let _ = iw;
        scene.pop_layer();
        self.f.sb.paint(scene, now);
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// AgX (QCView AgxColumn): minColor's parametric, HDR-capable AgX (Blender-compatible at its defaults)

/// (label, from, to, step, reset, read, write) — QCView's Repeater model, verbatim ranges.
type Get = fn(&Chain) -> f32;
type Set = fn(&mut Chain, f32);
const CURVE: [(&str, f32, f32, f32, f32, Get, Set); 7] = [
    ("White (EV over grey)", 1.0, 20.0, 0.1, 6.5, |c| c.agx_white_ev, |c, v| c.agx_white_ev = v),
    ("Black (EV under grey)", -20.0, -1.0, 0.1, -10.0, |c| c.agx_black_ev, |c, v| c.agx_black_ev = v),
    ("Contrast", 0.5, 5.0, 0.05, 2.4, |c| c.agx_contrast, |c, v| c.agx_contrast = v),
    ("Toe power", 0.5, 5.0, 0.05, 1.5, |c| c.agx_toe, |c, v| c.agx_toe = v),
    ("Shoulder power", 0.5, 5.0, 0.05, 1.5, |c| c.agx_shoulder, |c, v| c.agx_shoulder = v),
    ("Hue restore", 0.0, 1.0, 0.05, 0.6, |c| c.agx_hue_restore, |c, v| c.agx_hue_restore = v),
    ("HDR purity", 0.0, 1.0, 0.05, 0.5, |c| c.agx_hdr_purity, |c, v| c.agx_hdr_purity = v),
];

pub struct AgxPanel {
    f: Frame, chain: Chain,
    on: Switch, target: ActionGroup, peak: NumberField, sliders: Vec<Slider>,
    rows: Vec<(f64, f64)>, note_h: f64, last_press: Option<(usize, Instant)>,
}

impl AgxPanel {
    pub fn new() -> AgxPanel {
        let chip = |l: &str| { let mut b = ActionButton::new("", l).size_s(); b.label = Some(l.to_string()); b };
        AgxPanel { f: Frame { open_w: AGX_W, ..Frame::new("AgX") }, chain: Chain::default(), on: Switch::new(false), target: ActionGroup::new(vec![chip("Rec.2020"), chip("Rec.709"), chip("P3-D65")]).compact(),
            peak: NumberField::new(100, 100, 10000, 100), sliders: (0..CURVE.len()).map(|_| Slider::new(0.5)).collect(), rows: vec![], note_h: 0.0, last_press: None }
    }
    pub fn set_expanded(&mut self, e: bool) { self.f.expanded = e; }
    pub fn focused(&self) -> bool { self.peak.focused() }
    pub fn width(&self) -> f64 { self.f.width() }
    pub fn dragging(&self) -> bool { self.sliders.iter().any(|s| s.dragging()) || self.f.sb.dragging() }
    pub fn sync(&mut self, c: &Chain, now: Instant) {
        self.chain = *c;
        self.on.set(c.agx_on != 0, now);
        for (i, b) in self.target.buttons.iter_mut().enumerate() { b.selected = c.agx_target == i as i32; }
        self.peak.set(c.agx_peak.round() as i64);
        for (s, (_, from, to, _, _, get, _)) in self.sliders.iter_mut().zip(CURVE.iter()) { if !s.dragging() { s.value = ((get(c) - from) / (to - from)).clamp(0.0, 1.0); } }
    }
    pub fn layout(&mut self, fonts: &mut Fonts, x: f64, y0: f64, y1: f64, scale: f64) -> f64 {
        let w = self.f.layout(x, y0, y1, scale);
        if !self.f.expanded { return w }
        let (ix, iy, iw) = self.f.inner(); let s = scale;
        let lh = 20.0 * s; let row = t::COMPONENT_HEIGHT_100 as f64 * s; let fh = t::FIELD_HEIGHT_S as f64 * s; let unit = 30.0 * s;
        let (sw, sh) = Switch::size(s);
        let mut y = iy; self.rows.clear();
        self.on.rect = Rect::new(ix, y + (row - sh) / 2.0, ix + sw, y + (row + sh) / 2.0); self.rows.push((y, row)); y += row;
        let note = fonts.layout_width(tr("Blender's AgX at the defaults; HDR above 100 nits."), Family::Sans, t::DETAIL_SIZE_S * s as f32, t::FONT_WEIGHT_REGULAR as f32, peniko::Color::WHITE, Some(iw as f32));
        self.note_h = note.height() as f64; self.rows.push((y, self.note_h)); y += self.note_h + t::SPACING_100 as f64 * s;
        self.rows.push((y, lh)); y += lh;                                                          // "Target gamut"
        self.target.layout(fonts, ix, y, s); let gh = self.target.rect.height(); self.rows.push((y, gh)); y += gh + t::SPACING_100 as f64 * s;   // layout() returns the WIDTH
        self.rows.push((y, lh)); y += lh;                                                          // "Peak luminance"
        self.peak.layout(fonts, Rect::new(ix, y + (row - fh) / 2.0, ix + iw - unit, y + (row + fh) / 2.0), s); self.rows.push((y, row)); y += row;
        for sl in self.sliders.iter_mut() { self.rows.push((y, lh)); y += lh; sl.rect = Rect::new(ix, y, ix + iw, y + row * 0.8); y += row * 0.8 + t::SPACING_50 as f64 * s; }
        self.f.finish_layout(y - iy);
        w
    }
    pub fn event(&mut self, e: &Event, now: Instant, fonts: &mut Fonts) -> (Option<Chain>, bool) {
        if let Some(d) = self.f.event(e, now) { return (None, d) }
        if !self.f.expanded { return (None, false) }
        let live = self.f.over_card(e) || self.dragging() || matches!(e, Event::Up { .. } | Event::Leave);
        if !live { return (None, false) }
        let mut c = self.chain; let mut changed = false; let mut d = std::mem::take(&mut self.f.dirty);
        let (v, dd) = self.on.event(e, now); d |= dd; if let Some(on) = v { c.agx_on = on as i32; if on { c.open_drt = 0; } changed = true; }
        for (i, r) in self.target.event(e, now) { d = true; if r == Response::Clicked { c.agx_target = i as i32; changed = true; } }
        let (v, dd) = self.peak.event(e, fonts); d |= dd; if let Some(v) = v { c.agx_peak = v as f32; changed = true; }
        for (i, (sl, (_, from, to, step, reset, _, set))) in self.sliders.iter_mut().zip(CURVE.iter()).enumerate() {
            if let Event::Down { x, y, button: Button::Primary } = *e { if sl.hit(x, y) {
                if self.last_press.is_some_and(|(j, t0)| j == i && now.duration_since(t0).as_millis() < DOUBLE_CLICK_MS) { set(&mut c, *reset); self.last_press = None; self.chain = c; return (Some(c), true) }
                self.last_press = Some((i, now));
            } }
            let (v, dd) = sl.event(e, now, self.f.scale); d |= dd;
            if let Some(v) = v { let raw = from + (to - from) * v; set(&mut c, (raw / step).round() * step); changed = true; }
        }
        if changed { self.chain = c; (Some(c), true) } else { (None, d) }
    }
    pub fn key(&mut self, k: &KeyEvent, fonts: &mut Fonts) -> Option<Chain> { if !self.peak.focused() { return None } let mut c = self.chain; c.agx_peak = self.peak.key(k, fonts)? as f32; self.chain = c; Some(c) }
    pub fn blur(&mut self) -> Option<Chain> { let v = self.peak.blur()?; let mut c = self.chain; c.agx_peak = v as f32; self.chain = c; Some(c) }
    pub fn wheel(&mut self, x: f64, y: f64, dy: f64, now: Instant) -> bool { self.f.wheel(x, y, dy, now) }
    pub fn animating(&self, now: Instant) -> bool { self.f.strip_hover.animating(now) || self.f.sb.animating(now) || self.on.animating(now) || self.sliders.iter().any(|s| s.animating(now)) }
    pub fn paint(&mut self, scene: &mut Scene, fonts: &mut Fonts, now: Instant) {
        let c = self.chain; let on = c.agx_on != 0;
        self.f.paint_frame(scene, fonts, on, now);
        if !self.f.expanded { return }
        let s = self.f.scale; let (ix, _, iw) = self.f.inner();
        scene.push_clip_layer(peniko::Fill::NonZero, Affine::IDENTITY, &self.f.card);
        let r = |i: usize, rows: &[(f64, f64)]| rows.get(i).copied().unwrap_or((0.0, 0.0));
        self.on.paint(scene, s, now);
        let (y, h) = r(0, &self.rows); let l = fonts.layout(if on { tr("On") } else { tr("Off") }, Family::Sans, t::RAIL_LABEL_SIZE * s as f32, if on { t::FONT_WEIGHT_BOLD } else { t::FONT_WEIGHT_REGULAR } as f32, paint::color(if on { t::NOTICE_VISUAL_COLOR } else { t::RAIL_GROUP_HEADER_COLOR }));
        min_ui::fonts::draw_layout(scene, &l, Affine::translate((self.on.rect.x1 + t::SPACING_100 as f64 * s, y + (h - l.height() as f64) / 2.0)));
        let (y, _) = r(1, &self.rows);
        let note = fonts.layout_width(tr("Blender's AgX at the defaults; HDR above 100 nits."), Family::Sans, t::DETAIL_SIZE_S * s as f32, t::FONT_WEIGHT_REGULAR as f32, paint::color(t::DISABLED_CONTENT_COLOR), Some(iw as f32));
        min_ui::fonts::draw_layout(scene, &note, Affine::translate((ix, y)));
        let (y, h) = r(2, &self.rows); label(scene, fonts, tr("Target gamut"), ix, y, h, s, t::RAIL_GROUP_HEADER_COLOR, false);
        self.target.paint(scene, fonts, s, now);
        let (y, h) = r(4, &self.rows); label(scene, fonts, tr("Peak luminance"), ix, y, h, s, t::RAIL_GROUP_HEADER_COLOR, false);
        self.peak.paint(scene, fonts, s); let (y, h) = r(5, &self.rows); label(scene, fonts, tr("nits"), self.peak.rect.x1 + t::SPACING_75 as f64 * s, y, h, s, t::DISABLED_CONTENT_COLOR, false);
        for (i, (sl, (name, _, _, _, _, get, _))) in self.sliders.iter().zip(CURVE.iter()).enumerate() {
            let (y, h) = r(6 + i, &self.rows);
            label(scene, fonts, tr(name), ix, y, h, s, t::RAIL_GROUP_HEADER_COLOR, false);
            label_right(scene, fonts, &format!("{:.2}", get(&c)), ix + iw, y, h, s, t::RAIL_LABEL_COLOR);
            sl.paint(scene, s, now);
        }
        scene.pop_layer();
        self.f.sb.paint(scene, now);
    }
}
