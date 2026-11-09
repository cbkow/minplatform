//! The Output section of the left rail (QCView `LeftRail.qml`'s "Output"; canon row 227 step 3), in the port's design (the user
//! 2026-11-09, "lets do it"): the section's header carries the Mute feed toggle (the live row's speaker); each registered sink is
//! a CARD — a status dot (green live · amber starting / waiting · red refused / failed · grey off; hover = why), the label and
//! the product, an on / off SWITCH (QCView's Start / Stop buttons: on launches the sink, off retires it), the shape and the
//! sink's own report as two quiet lines, then only the settings the sink lets QCView choose — Transfer (none for NDI: it names
//! SDR itself), Raster (a sink that asks for "auto"), Primaries + Peak / MaxCLL / MaxFALL for PQ — a label column and
//! full-width controls. QCView's Guides / Notes chips are not carried (row 225).
use kurbo::Rect;
use min_theme::tokens as t;
use min_ui::{paint, ActionButton, Event, Family, Fonts, KeyEvent, NumberField, Picker, Response, Section, Switch};
use std::time::Instant;
use vello::Scene;

fn tr(s: &str) -> &str { min_i18n::trc("LeftRail", s) }

/// What a card shows (gathered from the bus by `outputs::views`).
#[derive(Clone, Debug, PartialEq)]
pub struct CardView { pub id: String, pub label: String, pub product: String, /// 0 off · 1 live · 2 starting · 3 refused / failed
    pub dot: u8, pub reason: String, pub shape: String, pub status: String, pub enabled: bool, pub installed: bool,
    pub transfer: usize, pub raster: usize, pub primaries: usize, pub peak: i64, pub cll: i64, pub fall: i64, pub show_transfer: bool, pub show_raster: bool }
#[derive(Clone, Debug, PartialEq)]
pub enum Cmd { Start(String), Stop(String), FeedMute(bool), Transfer(String, i64), Raster(String, i64), Primaries(String, i64), Peak(String, i64), Cll(String, i64), Fall(String, i64) }

struct Card { v: CardView, on: Switch, transfer: Picker, raster: Picker, primaries: Picker, nits: [NumberField; 3], tile: Rect, dot: Rect, header: Rect, lines: Vec<(Rect, String)>, rows: Vec<(Rect, &'static str)> }

pub struct OutputPanel { pub section: Section, mute: ActionButton, cards: Vec<Card>, empty: Rect, height: f64, scale: f64, pub tooltips: min_ui::Tooltips }

const LABEL_W: f64 = 84.0;
/// The controls' background on a card (layer 2 = GRAY_100): one step darker.
const CONTROL_BG: min_theme::Color = t::GRAY_75;

impl OutputPanel {
    pub fn new() -> OutputPanel {
        let mut section = Section::new(tr("Output"), true); section.expanded = false;
        // the speaker: the chip's size kept, a smaller glyph, the Notes tools' rounding (the walk 2026-11-09)
        let mut mute = ActionButton::new("speaker-high", tr("Silence the audio sent to outputs while you keep listening. The master mute (M) silences both.")).size_s();
        mute.icon_size = Some(t::ICON_SIZE_SMALL); mute.corners = min_ui::action_button::Corners::all(t::CORNER_RADIUS_500 as f64);
        OutputPanel { section, mute, cards: vec![], empty: Rect::ZERO, height: 0.0, scale: 1.0, tooltips: Default::default() }
    }
    pub fn relabel(&mut self) { let f = Self::new(); self.section.title = f.section.title; self.mute.tooltip = f.mute.tooltip; let vs: Vec<CardView> = self.cards.iter().map(|c| c.v.clone()).collect(); let m = self.mute.selected; self.cards.clear(); self.sync(vs, m, Instant::now()); }
    pub fn menu_open(&self) -> bool { self.cards.iter().any(|c| c.transfer.is_open() || c.raster.is_open() || c.primaries.is_open()) }
    /// A press on a nits field (it keeps its focus: no commit-and-refocus).
    pub fn field_hit(&self, x: f64, y: f64) -> bool { self.cards.iter().any(|c| c.nits.iter().any(|n| n.hit(x, y))) }
    pub fn has_focus(&self) -> bool { self.cards.iter().any(|c| c.nits.iter().any(|n| n.focused())) }
    pub fn sync(&mut self, views: Vec<CardView>, feed_muted: bool, now: Instant) -> bool {
        let mut changed = self.mute.selected != feed_muted;
        self.mute.selected = feed_muted; self.mute.icon = if feed_muted { "speaker-x" } else { "speaker-high" };
        if self.cards.len() != views.len() || self.cards.iter().zip(&views).any(|(c, v)| c.v.id != v.id) {
            self.cards = views.iter().map(|v| Card { v: v.clone(), on: Switch::new(v.enabled),
                transfer: Picker::new(vec![tr("SDR Rec.709").into(), tr("PQ Rec.2100").into(), tr("HLG Rec.2100").into()], 0), raster: Picker::new(vec![tr("Auto").into(), tr("1080p").into(), tr("UHD").into()], 0), primaries: Picker::new(vec![tr("Rec.2020").into(), tr("P3-D65").into()], 0),
                nits: [NumberField::new(v.peak, 100, 10000, 100), NumberField::new(v.cll, 0, 10000, 100), NumberField::new(v.fall, 0, 10000, 50)],
                tile: Rect::ZERO, dot: Rect::ZERO, header: Rect::ZERO, lines: vec![], rows: vec![] }).map(|mut c| {
                    // the card is the picker token's own grey: its controls sit one step darker, recessed (the walk 2026-11-09)
                    for p in [&mut c.transfer, &mut c.raster, &mut c.primaries] { p.bg = Some(CONTROL_BG); }
                    for n in c.nits.iter_mut() { n.bg = Some(CONTROL_BG); }
                    c
                }).collect();
            changed = true;
        }
        for (c, v) in self.cards.iter_mut().zip(views) {
            if c.v != v { changed = true; }
            c.on.set(v.enabled, now); c.on.enabled = v.installed || v.enabled;
            c.transfer.selected = v.transfer; c.raster.selected = v.raster; c.primaries.selected = v.primaries;
            c.nits[0].set(v.peak); c.nits[1].set(v.cll); c.nits[2].set(v.fall);
            c.v = v;
        }
        changed
    }
    pub fn layout(&mut self, fonts: &mut Fonts, x0: f64, x1: f64, y: f64, scale: f64) -> f64 {
        self.scale = scale;
        let hh = Section::height(scale);
        self.section.rect = Rect::new(x0, y, x1, y + hh);
        // the Mute feed toggle sits in the header, left of the collapse arrow
        let (mw, mh) = self.mute.measure(fonts, scale); let mx = x1 - t::SPACING_100 as f64 * scale - mw;   // flush right: the section's caret is on the left
        self.mute.rect = Rect::new(mx, y + (hh - mh) / 2.0, mx + mw, y + (hh + mh) / 2.0); self.mute.corners = min_ui::action_button::Corners::all(t::CORNER_RADIUS_500 as f64 * scale);
        if !self.section.expanded { self.height = hh; return hh }
        let pad = t::SPACING_100 as f64 * scale; let gap = t::RAIL_ROW_GAP as f64 * scale; let row = t::RAIL_ROW_HEIGHT as f64 * scale; let ins = t::SPACING_200 as f64 * scale;
        let (x0, x1) = (x0 + pad, x1 - pad);
        let mut cy = y + hh + pad / 2.0;
        if self.cards.is_empty() {
            let l = fonts.layout_width(&empty_text(), Family::Sans, t::DETAIL_SIZE_S * scale as f32, t::FONT_WEIGHT_REGULAR as f32, peniko::Color::WHITE, Some((x1 - x0) as f32));
            self.empty = Rect::new(x0, cy, x1, cy + l.height() as f64); cy += l.height() as f64 + gap;
        }
        for c in &mut self.cards {
            let top = cy; let (cx0, cx1) = (x0 + ins, x1 - ins); cy += ins * 0.75;
            let (sw, sh) = Switch::size(scale);
            c.on.rect = Rect::new(cx1 - sw, cy + (row - sh) / 2.0, cx1, cy + (row + sh) / 2.0);
            let d = 8.0 * scale; c.dot = Rect::new(cx0, cy + (row - d) / 2.0, cx0 + d, cy + (row + d) / 2.0);
            c.header = Rect::new(cx0 + d + 8.0 * scale, cy, c.on.rect.x0 - pad, cy + row); cy += row;
            c.lines.clear();
            for txt in [c.v.shape.clone(), c.v.status.clone()] { if txt.is_empty() { continue }
                let l = fonts.layout_width(&txt, Family::Sans, t::DETAIL_SIZE_S * scale as f32, t::FONT_WEIGHT_REGULAR as f32, peniko::Color::WHITE, Some((cx1 - c.header.x0) as f32));
                c.lines.push((Rect::new(c.header.x0, cy, cx1, cy + l.height() as f64), txt)); cy += l.height() as f64 + 2.0 * scale; }
            c.rows.clear();
            let lw = LABEL_W * scale; let ctl = |cy: f64| Rect::new(cx0 + lw, cy, cx1, cy + row);
            let any = c.v.show_transfer || c.v.show_raster;
            if any { cy += gap; }
            let mut put = |p: &mut Picker, label: &'static str, cy: &mut f64| { let (_, ph) = p.measure(fonts, scale); let r = ctl(*cy); p.rect = Rect::new(r.x0, r.y0 + (row - ph) / 2.0, r.x1, r.y0 + (row + ph) / 2.0); c.rows.push((Rect::new(cx0, *cy, cx0 + lw, *cy + row), label)); *cy += row + gap; };
            if c.v.show_transfer { put(&mut c.transfer, "Transfer", &mut cy); } else { c.transfer.rect = Rect::ZERO; }
            if c.v.show_raster { put(&mut c.raster, "Raster", &mut cy); } else { c.raster.rect = Rect::ZERO; }
            if c.v.show_transfer && c.v.transfer == 1 {
                put(&mut c.primaries, "Primaries", &mut cy);
                // Peak / MaxCLL / MaxFALL in nits: three fields across the control column
                c.rows.push((Rect::new(cx0, cy, cx0 + lw, cy + row), "Nits"));
                let r = ctl(cy); let fh = t::FIELD_HEIGHT_S as f64 * scale; let fw = (r.width() - 2.0 * pad) / 3.0;
                for (i, n) in c.nits.iter_mut().enumerate() { let fx = r.x0 + i as f64 * (fw + pad); n.layout(fonts, Rect::new(fx, r.y0 + (row - fh) / 2.0, fx + fw, r.y0 + (row + fh) / 2.0), scale); }
                cy += row + gap;
            } else { c.primaries.rect = Rect::ZERO; for n in c.nits.iter_mut() { n.rect = Rect::ZERO; } }
            cy += ins * 0.75;
            c.tile = Rect::new(x0, top, x1, cy); cy += gap * 2.0;
        }
        self.height = cy - y + t::RAIL_SECTION_GAP as f64 * scale - pad / 2.0;
        self.height
    }
    /// Keys while a nits field has focus: Enter / Tab commit.
    pub fn key(&mut self, k: &KeyEvent, fonts: &mut Fonts) -> Vec<Cmd> {
        let mut cmds = vec![];
        for c in &mut self.cards { for (i, n) in c.nits.iter_mut().enumerate() { if n.focused() && let Some(v) = n.key(k, fonts) { cmds.push(nits_cmd(&c.v.id, i, v)); } } }
        cmds
    }
    pub fn blur(&mut self) -> Vec<Cmd> { let mut cmds = vec![]; for c in &mut self.cards { for (i, n) in c.nits.iter_mut().enumerate() { if let Some(v) = n.blur() { cmds.push(nits_cmd(&c.v.id, i, v)); } } } cmds }
    pub fn ime(&mut self, e: &min_ui::ImeEvent, fonts: &mut Fonts) { for c in &mut self.cards { for n in c.nits.iter_mut() { if n.focused() { n.field.ime(e, fonts); } } } }
    /// Returns (commands, redraw, consumed). An open picker takes every event first.
    pub fn event(&mut self, e: &Event, now: Instant, fonts: &mut Fonts, bounds: Rect) -> (Vec<Cmd>, bool, bool) {
        let mut cmds = vec![]; let s = self.scale;
        for c in &mut self.cards { for (p, which) in [(&mut c.transfer, 0), (&mut c.raster, 1), (&mut c.primaries, 2)] { if p.is_open() {
            let (pick, d, _) = p.event(e, now, fonts, bounds, s);
            if let Some(i) = pick { cmds.push(match which { 0 => Cmd::Transfer(c.v.id.clone(), i as i64 + 1), 1 => Cmd::Raster(c.v.id.clone(), i as i64), _ => Cmd::Primaries(c.v.id.clone(), i as i64 + 1) }); }
            return (cmds, d || pick.is_some(), true)
        } } }
        // the header's Mute feed toggle before the section's own click (which collapses)
        let mut dirty = false;
        match self.mute.event(e, now) { Response::Clicked => { cmds.push(Cmd::FeedMute(!self.mute.selected)); return (cmds, true, true) } Response::None => {} _ => dirty = true }
        if !self.mute.hovered() { let (toggled, d) = self.section.event(e, now); dirty |= d || toggled; }
        if matches!(e, Event::Down { .. }) { self.tooltips.pressed(now); }
        if self.section.expanded {
            for c in &mut self.cards {
                let (on, d) = c.on.event(e, now); dirty |= d;
                if let Some(on) = on { cmds.push(if on { Cmd::Start(c.v.id.clone()) } else { Cmd::Stop(c.v.id.clone()) }); }
                for p in [&mut c.transfer, &mut c.raster, &mut c.primaries] { if p.rect.width() > 0.0 { let (_, d, _) = p.event(e, now, fonts, bounds, s); dirty |= d; } }
                for (i, n) in c.nits.iter_mut().enumerate() { if n.rect.width() > 0.0 { let (v, d) = n.event(e, fonts); dirty |= d; if let Some(v) = v { cmds.push(nits_cmd(&c.v.id, i, v)); } } }
            }
        }
        let over = self.mute.hovered().then(|| (1u64, self.mute.rect, self.mute.tooltip.clone()))
            .or_else(|| self.cards.iter().enumerate().find(|(_, c)| e.pos().is_some_and(|(x, y)| c.dot.inflate(4.0, 4.0).contains(kurbo::Point::new(x, y)) || c.header.contains(kurbo::Point::new(x, y)))).map(|(i, c)| (10 + i as u64, c.dot.union(c.header), if c.v.reason.is_empty() { tr("Publish the viewport and the mixed audio to this output. The sink runs as its own application; QCView starts it with Start and stops it with Stop.").to_string() } else { c.v.reason.clone() })));
        if self.tooltips.hover(over.as_ref().map(|(k, r, s)| (*k, *r, s.as_str())), now) { dirty = true; }
        let inside = e.pos().is_some_and(|(x, y)| x >= self.section.rect.x0 && x <= self.section.rect.x1 && y >= self.section.rect.y0 && y <= self.section.rect.y0 + self.height);
        (cmds, dirty, inside && matches!(e, Event::Down { .. }))
    }
    pub fn animating(&self, now: Instant) -> bool { self.section.animating(now) || self.mute.animating(now) || self.cards.iter().any(|c| c.on.animating(now)) }
    pub fn paint(&mut self, scene: &mut Scene, fonts: &mut Fonts, scale: f64, now: Instant, first: bool) {
        let gap = t::RAIL_SECTION_GAP as f64 * scale;
        min_ui::section::paint_tile(scene, Rect::new(self.section.rect.x0, self.section.rect.y0, self.section.rect.x1, self.section.rect.y0 + self.height - gap / 2.0), scale);
        self.section.paint(scene, fonts, scale, now, first);
        self.mute.paint(scene, fonts, scale, now);
        if !self.section.expanded { return }
        let small = t::DETAIL_SIZE_S * scale as f32; let lab = t::RAIL_LABEL_SIZE * scale as f32;
        let mid = |r: Rect, hgt: f64| r.y0 + (r.height() - hgt) / 2.0;
        if self.cards.is_empty() { let l = fonts.layout_width(&empty_text(), Family::Sans, small, t::FONT_WEIGHT_REGULAR as f32, paint::color(t::NEUTRAL_SUBDUED_CONTENT_COLOR_DEFAULT), Some(self.empty.width() as f32)); min_ui::fonts::draw_layout(scene, &l, kurbo::Affine::translate((self.empty.x0, self.empty.y0))); }
        for c in &mut self.cards {
            paint::rounded(scene, c.tile, 6.0 * scale, min_ui::section::card_color());
            let dc = match c.v.dot { 1 => t::POSITIVE_VISUAL_COLOR, 2 => t::NOTICE_VISUAL_COLOR, 3 => t::NEGATIVE_VISUAL_COLOR, _ => t::GRAY_500 };
            paint::rounded(scene, c.dot, c.dot.width() / 2.0, dc);
            let mut x = c.header.x0;
            let l = fonts.layout(&c.v.label, Family::Sans, lab, t::FONT_WEIGHT_BOLD as f32, paint::color(t::NEUTRAL_CONTENT_COLOR_DEFAULT)); min_ui::fonts::draw_layout(scene, &l, kurbo::Affine::translate((x, mid(c.header, l.height() as f64)))); x += l.width() as f64 + 8.0 * scale;
            let p = fonts.layout_fit(&c.v.product, Family::Sans, small, t::FONT_WEIGHT_REGULAR as f32, paint::color(t::NEUTRAL_SUBDUED_CONTENT_COLOR_DEFAULT), (c.header.x1 - x).max(10.0));
            min_ui::fonts::draw_layout(scene, &p, kurbo::Affine::translate((x, mid(c.header, p.height() as f64))));
            c.on.paint(scene, scale, now);
            for (r, txt) in &c.lines { let l = fonts.layout_width(txt, Family::Sans, small, t::FONT_WEIGHT_REGULAR as f32, paint::color(t::NEUTRAL_SUBDUED_CONTENT_COLOR_DEFAULT), Some(r.width() as f32)); min_ui::fonts::draw_layout(scene, &l, kurbo::Affine::translate((r.x0, r.y0))); }
            for (r, label) in &c.rows { let l = fonts.layout(tr(label), Family::Sans, lab, t::FONT_WEIGHT_REGULAR as f32, paint::color(t::RAIL_LABEL_COLOR)); min_ui::fonts::draw_layout(scene, &l, kurbo::Affine::translate((r.x0, mid(*r, l.height() as f64)))); }
            for p in [&c.transfer, &c.raster, &c.primaries] { if p.rect.width() > 0.0 { p.paint(scene, fonts, scale, now); } }
            for n in c.nits.iter_mut() { if n.rect.width() > 0.0 { n.paint(scene, fonts, scale); } }
        }
    }
    pub fn paint_overlays(&self, scene: &mut Scene, fonts: &mut Fonts, scale: f64, bounds: Rect, now: Instant) {
        for c in &self.cards { for p in [&c.transfer, &c.raster, &c.primaries] { if p.is_open() { p.paint_menu(scene, fonts, scale); } } }
        self.tooltips.paint(scene, fonts, scale, bounds, now);
    }
}

fn nits_cmd(id: &str, i: usize, v: i64) -> Cmd { match i { 0 => Cmd::Peak(id.into(), v), 1 => Cmd::Cll(id.into(), v), _ => Cmd::Fall(id.into(), v) } }

/// QCView's empty state, with this OS's folder (QCView named the macOS one everywhere).
fn empty_text() -> String {
    let folder = if cfg!(windows) { "%ProgramData%\\QCView\\Outputs" } else { "Application Support/QCView/Outputs" };
    min_i18n::fill(tr("No outputs installed. An output registers itself in %1."), &[&folder])
}
