//! The Safety Guides section of the left rail (QCView `LeftRail.qml` 1737–1882, collapsed by default, above Background):
//! Guide (a picker: "(none)" + the SVGs by name), Opacity (0–1, two decimals), Width (1–8 px, whole), Color (the inline
//! picker). Every value persists (row 215 — QCView kept only the colour).
use kurbo::{Affine, Rect};
use min_theme::tokens as t;
use min_ui::{paint, ColorPicker, Event, Family, Fonts, ImeEvent, KeyEvent, Picker, Section, Slider};
use std::time::Instant;
use vello::Scene;

fn tr(s: &str) -> &str { min_i18n::trc("LeftRail", s) }

#[derive(Clone, Debug, PartialEq)]
pub enum Cmd { Guide(String), Opacity(f32), Width(f32), Color(String) }

pub struct GuidesPanel { pub section: Section, guide: Picker, opacity: Slider, width: Slider, color: ColorPicker, labels: [Rect; 4], readouts: [Rect; 2], height: f64, vals: (f32, f32) }

impl GuidesPanel {
    pub fn new() -> GuidesPanel {
        let mut names: Vec<String> = crate::guides::GUIDES.iter().map(|g| g.0.to_string()).collect(); names.sort();
        let mut opts = vec![tr("(none)").to_string()]; opts.extend(names);
        GuidesPanel { section: Section::new(tr("Safety Guides"), false), guide: Picker::new(opts, 0), opacity: Slider::new(0.7), width: Slider::new(1.0 / 7.0), color: ColorPicker::new("#ffff00"), labels: [Rect::ZERO; 4], readouts: [Rect::ZERO; 2], height: 0.0, vals: (0.7, 2.0) }
    }
    /// The store's values ("" = none).
    pub fn sync(&mut self, guide: &str, opacity: f32, width: f32, color: &str) {
        if guide.is_empty() { self.guide.selected = 0; } else { self.guide.select_text(guide); }
        if !self.opacity.dragging() { self.opacity.value = opacity.clamp(0.0, 1.0); }
        if !self.width.dragging() { self.width.value = ((width - 1.0) / 7.0).clamp(0.0, 1.0); }
        self.vals = (opacity, width);
        if !self.color.dragging() { self.color.set_hex(color); }
    }
    pub fn has_focus(&self) -> bool { self.color.focused() }
    pub fn layout(&mut self, fonts: &mut Fonts, x0: f64, x1: f64, y: f64, scale: f64) -> f64 {
        let hh = Section::height(scale);
        self.section.rect = Rect::new(x0, y, x1, y + hh);
        if !self.section.expanded { self.height = hh; return hh }
        let pad = t::SPACING_100 as f64 * scale; let (x0, x1) = (x0 + pad, x1 - pad);
        let rh = t::FIELD_HEIGHT_S as f64 * scale; let gap = t::SPACING_100 as f64 * scale;
        let lw = 64.0 * scale; let rw = 36.0 * scale;
        let mut yy = y + hh + pad / 2.0;
        for i in 0..3 {
            self.labels[i] = Rect::new(x0, yy, x0 + lw, yy + rh);
            match i {
                0 => { self.guide.rect = Rect::new(x0 + lw, yy, x1, yy + rh); }
                1 => { self.opacity.rect = Rect::new(x0 + lw, yy, x1 - rw - gap, yy + rh); self.readouts[0] = Rect::new(x1 - rw, yy, x1, yy + rh); }
                _ => { self.width.rect = Rect::new(x0 + lw, yy, x1 - rw - gap, yy + rh); self.readouts[1] = Rect::new(x1 - rw, yy, x1, yy + rh); }
            }
            yy += rh + gap;
        }
        self.labels[3] = Rect::new(x0, yy, x0 + lw, yy + rh); yy += rh;
        let ch = self.color.layout(fonts, x0, x1, yy, scale); yy += ch;
        self.height = yy - y + t::RAIL_SECTION_GAP as f64 * scale;
        self.height
    }
    /// Returns (commands, redraw needed).
    pub fn event(&mut self, e: &Event, now: Instant, fonts: &mut Fonts, bounds: Rect, scale: f64) -> (Vec<Cmd>, bool) {
        let mut cmds = vec![];
        let (toggled, mut dirty) = self.section.event(e, now); if toggled { dirty = true; }
        if !self.section.expanded { return (cmds, dirty) }
        let (pick, d, _) = self.guide.event(e, now, fonts, bounds, scale); dirty |= d;
        if let Some(i) = pick { cmds.push(Cmd::Guide(if i == 0 { String::new() } else { self.guide.value().to_string() })); }
        let (v, d) = self.opacity.event(e, now, scale); dirty |= d; if let Some(v) = v { cmds.push(Cmd::Opacity((v * 100.0).round() / 100.0)); }
        let (v, d) = self.width.event(e, now, scale); dirty |= d; if let Some(v) = v { cmds.push(Cmd::Width((1.0 + v * 7.0).round())); }
        let (c, d) = self.color.event(e, fonts); dirty |= d; if let Some(c) = c { cmds.push(Cmd::Color(c)); }
        (cmds, dirty)
    }
    pub fn key(&mut self, k: &KeyEvent, fonts: &mut Fonts) -> Vec<Cmd> { self.color.key(k, fonts).map(Cmd::Color).into_iter().collect() }
    pub fn ime(&mut self, e: &ImeEvent, fonts: &mut Fonts) { self.color.ime(e, fonts); }
    pub fn blur(&mut self) -> Vec<Cmd> { self.color.blur().map(Cmd::Color).into_iter().collect() }
    pub fn menu_open(&self) -> bool { self.guide.is_open() }
    pub fn animating(&self, now: Instant) -> bool { self.section.animating(now) || self.guide.animating(now) || self.opacity.animating(now) || self.width.animating(now) }
    pub fn paint(&mut self, scene: &mut Scene, fonts: &mut Fonts, scale: f64, now: Instant, first: bool) {
        let gap = t::RAIL_SECTION_GAP as f64 * scale;
        min_ui::section::paint_tile(scene, Rect::new(self.section.rect.x0, self.section.rect.y0, self.section.rect.x1, self.section.rect.y0 + self.height - gap / 2.0), scale);
        self.section.paint(scene, fonts, scale, now, first);
        if !self.section.expanded { return }
        for (i, l) in ["Guide", "Opacity", "Width", "Color"].iter().enumerate() {
            let lay = fonts.layout(tr(l), Family::Sans, t::RAIL_LABEL_SIZE * scale as f32, t::FONT_WEIGHT_REGULAR as f32, paint::color(t::RAIL_LABEL_COLOR));
            min_ui::fonts::draw_layout(scene, &lay, Affine::translate((self.labels[i].x0, self.labels[i].y0 + (self.labels[i].height() - lay.height() as f64) / 2.0)));
        }
        self.guide.paint(scene, fonts, scale, now);
        self.opacity.paint(scene, scale, now); self.width.paint(scene, scale, now);
        for (i, txt) in [format!("{:.2}", self.vals.0), format!("{}", self.vals.1.round() as i32)].iter().enumerate() {
            let lay = fonts.layout(txt, Family::Mono, t::CODE_SIZE_S * scale as f32, t::FONT_WEIGHT_REGULAR as f32, paint::color(t::NEUTRAL_SUBDUED_CONTENT_COLOR_DEFAULT));
            let r = self.readouts[i]; min_ui::fonts::draw_layout(scene, &lay, Affine::translate((r.x1 - lay.width() as f64, r.y0 + (r.height() - lay.height() as f64) / 2.0)));
        }
        self.color.paint(scene, fonts, scale);
    }
    /// The Guide picker's menu, over everything.
    pub fn paint_menu(&self, scene: &mut Scene, fonts: &mut Fonts, scale: f64) { if self.section.expanded { self.guide.paint_menu(scene, fonts, scale); } }
}
