//! The Output section of the left rail (QCView `LeftRail.qml`'s "Output", collapsed by default; canon row 227 step 3): the
//! "Mute feed" chip (QCView's Guides / Notes chips are not carried — row 225), then a card per registered sink: its label, its
//! product, its state (live · refused · starting · not installed), Start / Stop, the line under it (the shape + the sink's own
//! status, or why it is not live), and the output's settings — Transfer (hidden when the sink names its own: NDI asks for SDR,
//! QCView showed controls that did nothing), Raster (only for a sink that lets QCView choose), Primaries for PQ.
use kurbo::Rect;
use min_theme::tokens as t;
use min_ui::{paint, ActionButton, Event, Family, Fonts, Picker, Response, Section, TextButton, Variant};
use std::time::Instant;
use vello::Scene;

fn tr(s: &str) -> &str { min_i18n::trc("LeftRail", s) }

/// What a card shows (the bus's state, gathered by the shell).
#[derive(Clone, Debug, PartialEq)]
pub struct CardView { pub id: String, pub label: String, pub product: String, pub state: &'static str, pub line: String, pub enabled: bool, pub installed: bool,
    pub transfer: usize, pub raster: usize, pub primaries: usize, pub show_transfer: bool, pub show_raster: bool }
#[derive(Clone, Debug, PartialEq)]
pub enum Cmd { Start(String), Stop(String), FeedMute(bool), Transfer(String, i64), Raster(String, i64), Primaries(String, i64) }

struct Card { v: CardView, button: TextButton, transfer: Picker, raster: Picker, primaries: Picker, header: Rect, line: Rect, rows: Vec<(Rect, &'static str)> }

pub struct OutputPanel { pub section: Section, mute: ActionButton, cards: Vec<Card>, empty: Rect, height: f64, scale: f64, pub tooltips: min_ui::Tooltips }

impl OutputPanel {
    pub fn new() -> OutputPanel {
        let mut mute = ActionButton::new("", tr("Silence the audio sent to outputs while you keep listening. The master mute (M) silences both.")).size_s(); mute.label = Some(tr("Mute feed").to_string());
        let mut section = Section::new(tr("Output"), true); section.expanded = false;
        OutputPanel { section, mute, cards: vec![], empty: Rect::ZERO, height: 0.0, scale: 1.0, tooltips: Default::default() }
    }
    pub fn relabel(&mut self) { let f = Self::new(); self.section.title = f.section.title; self.mute.label = f.mute.label; self.mute.tooltip = f.mute.tooltip; let vs: Vec<CardView> = self.cards.iter().map(|c| c.v.clone()).collect(); self.cards.clear(); self.sync(vs, self.mute.selected); }
    pub fn menu_open(&self) -> bool { self.cards.iter().any(|c| c.transfer.is_open() || c.raster.is_open() || c.primaries.is_open()) }
    /// True when what is shown changed.
    pub fn sync(&mut self, views: Vec<CardView>, feed_muted: bool) -> bool {
        let mut changed = self.mute.selected != feed_muted; self.mute.selected = feed_muted;
        if self.cards.len() != views.len() || self.cards.iter().zip(&views).any(|(c, v)| c.v.id != v.id) {
            self.cards = views.iter().map(|v| Card { v: v.clone(), button: TextButton::new("", Variant::Secondary).size_s(), transfer: Picker::new(vec![tr("SDR Rec.709").into(), tr("PQ Rec.2100").into()], 0), raster: Picker::new(vec![tr("Auto").into(), tr("1080p").into(), tr("UHD").into()], 0), primaries: Picker::new(vec![tr("Rec.2020").into(), tr("P3-D65").into()], 0), header: Rect::ZERO, line: Rect::ZERO, rows: vec![] }).collect();
            changed = true;
        }
        for (c, v) in self.cards.iter_mut().zip(views) {
            if c.v != v { changed = true; }
            c.button.label = tr(if v.enabled { "Stop" } else { "Start" }).to_string(); c.button.variant = if v.enabled { Variant::Negative } else { Variant::Secondary }; c.button.enabled = v.installed || v.enabled;
            c.transfer.selected = v.transfer; c.raster.selected = v.raster; c.primaries.selected = v.primaries;
            c.v = v;
        }
        changed
    }
    pub fn layout(&mut self, fonts: &mut Fonts, x0: f64, x1: f64, y: f64, scale: f64) -> f64 {
        self.scale = scale;
        let hh = Section::height(scale);
        self.section.rect = Rect::new(x0, y, x1, y + hh);
        if !self.section.expanded { self.height = hh; return hh }
        let pad = t::SPACING_100 as f64 * scale; let gap = t::RAIL_ROW_GAP as f64 * scale; let row = t::RAIL_ROW_HEIGHT as f64 * scale;
        let (x0, x1) = (x0 + pad, x1 - pad);
        let mut cy = y + hh + pad / 2.0;
        let (mw, mh) = self.mute.measure(fonts, scale); self.mute.rect = Rect::new(x0, cy, x0 + mw, cy + mh); cy += mh + 2.0 * gap;
        if self.cards.is_empty() {
            let l = fonts.layout_width(&empty_text(), Family::Sans, t::DETAIL_SIZE_S * scale as f32, t::FONT_WEIGHT_REGULAR as f32, peniko::Color::WHITE, Some((x1 - x0) as f32));
            self.empty = Rect::new(x0, cy, x1, cy + l.height() as f64); cy += l.height() as f64 + gap;
        }
        for c in &mut self.cards {
            let (bw, bh) = c.button.measure(fonts, scale);
            c.header = Rect::new(x0, cy, x1 - bw - pad, cy + row); c.button.rect = Rect::new(x1 - bw, cy + (row - bh) / 2.0, x1, cy + (row + bh) / 2.0); cy += row;
            let l = fonts.layout_width(&c.v.line, Family::Sans, t::DETAIL_SIZE_S * scale as f32, t::FONT_WEIGHT_REGULAR as f32, peniko::Color::WHITE, Some((x1 - x0) as f32));
            c.line = Rect::new(x0, cy, x1, cy + l.height() as f64); cy += l.height() as f64 + gap;
            c.rows.clear();
            let mut put = |p: &mut Picker, label: &'static str, cy: &mut f64| { let (pw, ph) = p.measure(fonts, scale); p.rect = Rect::new(x1 - pw.min(160.0 * scale), *cy + (row - ph) / 2.0, x1, *cy + (row + ph) / 2.0); c.rows.push((Rect::new(x0, *cy, x1, *cy + row), label)); *cy += row + gap; };
            if c.v.show_transfer { put(&mut c.transfer, "Transfer", &mut cy); }
            if c.v.show_raster { put(&mut c.raster, "Raster", &mut cy); }
            if c.v.show_transfer && c.v.transfer == 1 { put(&mut c.primaries, "Primaries", &mut cy); }
            cy += gap;
        }
        self.height = cy - y + t::RAIL_SECTION_GAP as f64 * scale - pad / 2.0;
        self.height
    }
    /// Returns (commands, redraw, consumed). An open picker takes every event first.
    pub fn event(&mut self, e: &Event, now: Instant, fonts: &mut Fonts, bounds: Rect) -> (Vec<Cmd>, bool, bool) {
        let mut cmds = vec![]; let s = self.scale;
        for c in &mut self.cards { for (p, which) in [(&mut c.transfer, 0), (&mut c.raster, 1), (&mut c.primaries, 2)] { if p.is_open() {
            let (pick, d, consumed) = p.event(e, now, fonts, bounds, s);
            if let Some(i) = pick { cmds.push(match which { 0 => Cmd::Transfer(c.v.id.clone(), i as i64 + 1), 1 => Cmd::Raster(c.v.id.clone(), i as i64), _ => Cmd::Primaries(c.v.id.clone(), i as i64 + 1) }); }
            return (cmds, d || pick.is_some(), consumed || true)
        } } }
        let (toggled, mut dirty) = self.section.event(e, now); if toggled { dirty = true; }
        if !self.section.expanded { return (cmds, dirty, false) }
        if let Response::Clicked = self.mute.event(e, now) { cmds.push(Cmd::FeedMute(!self.mute.selected)); dirty = true; }
        for c in &mut self.cards {
            let (clicked, d) = c.button.event(e, now); dirty |= d;
            if clicked { cmds.push(if c.v.enabled { Cmd::Stop(c.v.id.clone()) } else { Cmd::Start(c.v.id.clone()) }); }
            for p in [&mut c.transfer, &mut c.raster, &mut c.primaries] { if p.rect.width() > 0.0 { let (_, d, _) = p.event(e, now, fonts, bounds, s); dirty |= d; } }
        }
        if matches!(e, Event::Down { .. }) { self.tooltips.pressed(now); }
        let over = self.mute.hovered().then(|| (1u64, self.mute.rect, self.mute.tooltip.clone()))
            .or_else(|| self.cards.iter().enumerate().find(|(_, c)| e.pos().is_some_and(|(x, y)| c.header.contains(kurbo::Point::new(x, y)))).map(|(i, c)| (10 + i as u64, c.header, format!("{} — {}", c.v.label, tr("Publish the viewport and the mixed audio to this output. The sink runs as its own application; QCView starts it with Start and stops it with Stop.")))));
        if self.tooltips.hover(over.as_ref().map(|(k, r, s)| (*k, *r, s.as_str())), now) { dirty = true; }
        let inside = e.pos().is_some_and(|(x, y)| x >= self.section.rect.x0 && x <= self.section.rect.x1 && y >= self.section.rect.y0 && y <= self.section.rect.y0 + self.height);
        (cmds, dirty, inside && matches!(e, Event::Down { .. }))
    }
    pub fn animating(&self, now: Instant) -> bool { self.section.animating(now) || self.mute.animating(now) || self.cards.iter().any(|c| c.button.animating(now)) }
    pub fn paint(&self, scene: &mut Scene, fonts: &mut Fonts, scale: f64, now: Instant, first: bool) {
        let gap = t::RAIL_SECTION_GAP as f64 * scale;
        min_ui::section::paint_tile(scene, Rect::new(self.section.rect.x0, self.section.rect.y0, self.section.rect.x1, self.section.rect.y0 + self.height - gap / 2.0), scale);
        self.section.paint(scene, fonts, scale, now, first);
        if !self.section.expanded { return }
        self.mute.paint(scene, fonts, scale, now);
        let small = t::DETAIL_SIZE_S * scale as f32; let lab = t::RAIL_LABEL_SIZE * scale as f32;
        let mid = |r: Rect, hgt: f64| r.y0 + (r.height() - hgt) / 2.0;
        if self.cards.is_empty() { let l = fonts.layout_width(&empty_text(), Family::Sans, small, t::FONT_WEIGHT_REGULAR as f32, paint::color(t::NEUTRAL_SUBDUED_CONTENT_COLOR_DEFAULT), Some(self.empty.width() as f32)); min_ui::fonts::draw_layout(scene, &l, kurbo::Affine::translate((self.empty.x0, self.empty.y0))); }
        for c in &self.cards {
            // QCView's header: the label, the product muted, "  ·  ", the state word
            let mut x = c.header.x0;
            let l = fonts.layout(&c.v.label, Family::Sans, lab, t::FONT_WEIGHT_BOLD as f32, paint::color(t::RAIL_LABEL_COLOR)); min_ui::fonts::draw_layout(scene, &l, kurbo::Affine::translate((x, mid(c.header, l.height() as f64)))); x += l.width() as f64 + 6.0 * scale;
            let state_c = match c.v.state { "live" => t::POSITIVE_VISUAL_COLOR, "refused" => t::NEGATIVE_VISUAL_COLOR, _ => t::NEUTRAL_SUBDUED_CONTENT_COLOR_DEFAULT };
            let p = fonts.layout_fit(&format!("{}  ·  ", c.v.product), Family::Sans, small, t::FONT_WEIGHT_REGULAR as f32, paint::color(t::NEUTRAL_SUBDUED_CONTENT_COLOR_DEFAULT), (c.header.x1 - x).max(10.0));
            min_ui::fonts::draw_layout(scene, &p, kurbo::Affine::translate((x, mid(c.header, p.height() as f64)))); x += p.width() as f64;
            let st = fonts.layout_fit(tr(c.v.state), Family::Sans, small, t::FONT_WEIGHT_REGULAR as f32, paint::color(state_c), (c.header.x1 - x).max(10.0));
            min_ui::fonts::draw_layout(scene, &st, kurbo::Affine::translate((x, mid(c.header, st.height() as f64))));
            c.button.paint(scene, fonts, scale, now);
            let ln = fonts.layout_width(&c.v.line, Family::Sans, small, t::FONT_WEIGHT_REGULAR as f32, paint::color(t::NEUTRAL_SUBDUED_CONTENT_COLOR_DEFAULT), Some(c.line.width() as f32));
            min_ui::fonts::draw_layout(scene, &ln, kurbo::Affine::translate((c.line.x0, c.line.y0)));
            for (r, label) in &c.rows { let l = fonts.layout(tr(label), Family::Sans, lab, t::FONT_WEIGHT_REGULAR as f32, paint::color(t::RAIL_LABEL_COLOR)); min_ui::fonts::draw_layout(scene, &l, kurbo::Affine::translate((r.x0, mid(*r, l.height() as f64)))); }
            for (p, show) in [(&c.transfer, c.v.show_transfer), (&c.raster, c.v.show_raster), (&c.primaries, c.v.show_transfer && c.v.transfer == 1)] { if show { p.paint(scene, fonts, scale, now); } }
        }
    }
    pub fn paint_overlays(&self, scene: &mut Scene, fonts: &mut Fonts, scale: f64, bounds: Rect, now: Instant) {
        for c in &self.cards { for p in [&c.transfer, &c.raster, &c.primaries] { if p.is_open() { p.paint_menu(scene, fonts, scale); } } }
        self.tooltips.paint(scene, fonts, scale, bounds, now);
    }
}

/// QCView's empty state, with this OS's folder (QCView named the macOS one everywhere).
fn empty_text() -> String {
    let folder = if cfg!(windows) { "%ProgramData%\\QCView\\Outputs" } else { "Application Support/QCView/Outputs" };
    min_i18n::fill(tr("No outputs installed. An output registers itself in %1."), &[&folder])
}
