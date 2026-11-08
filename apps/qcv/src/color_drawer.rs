//! The Color drawer (canon rows 199 / 205–208, ruled 2026-11-08): QCView's Color panel moved above the timeline as a bottom
//! drawer that pushes the picture up, toggled by an up-caret tab on the viewport (like the rails' tabs), QCView's menu item,
//! transport button and Ctrl+3. One whole chain per clip with a project default; presets are starting points (picking one
//! copies its chain); an edited chain shows as a "Custom (from …)" row with one action, Save as preset…; Rename · Update ·
//! Delete on a user preset's right-click menu; one list for both engines. Slice 1: minColor's columns (Input gamut ·
//! Input transfer · Highlight Knee · Rendering); OCIO's columns, the viewer aids and the waveform follow.
use crate::color_model::{ClipChain, Preset, Stored};
use kurbo::{Affine, Rect};
use min_theme::tokens as t;
use min_ui::{anim::{mix, Ramp}, paint, Button, Event, Family, FieldResponse, Fonts, ImeEvent, KeyEvent, ListColumn, ListRow, Menu, MenuItem, NumberField, Popover, Switch, TextButton, TextField, Variant};
use std::time::Instant;
use vello::Scene;

/// QCView's catalog context for its Color panel (canon row 195).
fn tr(s: &'static str) -> &'static str { min_i18n::trc("ColorPanel", s) }

#[derive(Clone, Debug, PartialEq)]
pub enum Cmd { Pick(Stored), Edit(Stored), ResetToDefault, ApplyToAll, MakeDefault, SetOn(bool), SavePreset(String), RenamePreset(String, String), UpdatePreset(String), DeletePreset(String), Toggle }

/// What the drawer shows, fed by the shell every sync.
#[derive(Clone, Debug, PartialEq)]
pub struct View {
    /// "A · clip.mov", or "Project default" when nothing is loaded
    pub target: String,
    pub has_clip: bool,
    /// the clip has its own chain (else it follows the project default)
    pub own: bool,
    pub stored: Stored,
    /// the chain differs from the preset it came from
    pub custom: bool,
    pub on: bool,
    pub presets: Vec<Preset>,
}

#[derive(Clone, Debug, PartialEq)]
enum Naming { Save, Rename(String) }
#[derive(Clone, Debug, PartialEq)]
enum PresetRow { Custom, Preset(String), None }

pub struct ColorDrawer {
    pub open: bool,
    open_ramp: Ramp,
    /// logical px (the user can resize later; the store keeps it)
    pub height: f64,
    pub rect: Rect,
    tab: Rect, tab_hover: bool, tab_ramp: Ramp, present: Ramp,
    presets: ListColumn, gamut: ListColumn, transfer: ListColumn, render: ListColumn,
    preset_rows: Vec<PresetRow>,
    knee_rect: Rect, knee_on: Switch, knee_src: NumberField, knee_tgt: NumberField, knee_start: NumberField,
    on: Switch, on_rect: Rect,
    save_as: TextButton, reset: TextButton, apply_all: TextButton, make_default: TextButton,
    header: Rect,
    menu: Menu, menu_for: Option<String>,
    name_pop: Popover, name_field: TextField, naming: Option<Naming>,
    view: Option<View>,
    gamut_names: Vec<String>, transfer_names: Vec<String>, look_names: Vec<String>,
    scale: f64,
}

const PRESETS_W: f64 = 220.0;
const LIST_W: f64 = 170.0;
const KNEE_W: f64 = 190.0;

impl ColorDrawer {
    pub fn new(open: bool, height: f64) -> ColorDrawer {
        let mut gamut_names = qcv_color::input_gamut_names(); gamut_names.truncate(15);
        let transfer_names = qcv_color::input_transfer_names();
        ColorDrawer {
            open, open_ramp: Ramp::new(if open { 1.0 } else { 0.0 }, t::RAIL_COLLAPSE_DURATION), height, rect: Rect::ZERO,
            tab: Rect::ZERO, tab_hover: false, tab_ramp: Ramp::new(0.0, t::ANIMATION_DURATION_HOVER), present: Ramp::new(0.0, t::ANIMATION_DURATION_SLIDE),
            presets: ListColumn::new(tr("Presets")), gamut: ListColumn::new(tr("Input gamut")), transfer: ListColumn::new(tr("Input transfer")), render: ListColumn::new(tr("Rendering")),
            preset_rows: vec![],
            knee_rect: Rect::ZERO, knee_on: Switch::new(false), knee_src: NumberField::new(1000, 100, 10000, 100), knee_tgt: NumberField::new(100, 48, 10000, 10), knee_start: NumberField::new(0, 0, 99, 1).zero_label("BT.2390"),
            on: Switch::new(false), on_rect: Rect::ZERO,
            save_as: TextButton::new(tr("Save as preset…"), Variant::Secondary).size_s(), reset: TextButton::new(tr("Reset to default"), Variant::Secondary).size_s(),
            apply_all: TextButton::new(tr("Apply to all clips"), Variant::Secondary).size_s(), make_default: TextButton::new(tr("Make project default"), Variant::Secondary).size_s(),
            header: Rect::ZERO, menu: Menu::new(), menu_for: None, name_pop: Popover::new(""), name_field: TextField::new(Family::Sans, t::RAIL_LABEL_SIZE), naming: None,
            view: None, gamut_names, transfer_names, look_names: qcv_color::look_names(), scale: 1.0,
        }
    }
    pub fn toggle(&mut self, now: Instant) { self.open = !self.open; self.open_ramp.set_target(if self.open { 1.0 } else { 0.0 }, now); }
    /// The drawer's height on screen now (px), for the layout that pushes the picture up.
    pub fn visible_height(&self, now: Instant, scale: f64) -> f64 { (self.height * scale * self.open_ramp.value(now) as f64).round() }
    pub fn animating(&self, now: Instant) -> bool { self.open_ramp.animating(now) || self.tab_ramp.animating(now) || self.present.animating(now) || [&self.presets, &self.gamut, &self.transfer, &self.render].iter().any(|l| l.animating(now)) || self.knee_on.animating(now) || self.on.animating(now) || [&self.save_as, &self.reset, &self.apply_all, &self.make_default].iter().any(|b| b.animating(now)) }
    pub fn is_modal(&self) -> bool { self.menu.is_open() || self.naming.is_some() }
    pub fn has_focus(&self) -> bool { self.naming.is_some() || self.knee_src.focused() || self.knee_tgt.focused() || self.knee_start.focused() }
    pub fn set_pointer_present(&mut self, on: bool, now: Instant) { if on { if self.present.target() < 0.5 { self.present = Ramp::new(1.0, t::ANIMATION_DURATION_SLIDE); } } else { self.present.set_target(0.0, now); } }

    /// Mirror the shell's state; true when something visible changed.
    pub fn sync(&mut self, v: View, now: Instant) -> bool {
        if self.view.as_ref() == Some(&v) { return false }
        // the preset list: Custom (when edited), the built-ins, the user's
        let mut rows = vec![]; let mut acts = vec![];
        if v.custom { let from = if v.stored.preset.is_empty() { String::new() } else { format!(" ({} {})", tr("from"), v.stored.preset) }; rows.push(ListRow::tagged(format!("{}{from}", tr("Custom")), if v.stored.chain.is_ocio() { "OCIO" } else { "" })); acts.push(PresetRow::Custom); }
        rows.push(ListRow::Header("minColor".into())); acts.push(PresetRow::None);
        for p in v.presets.iter().filter(|p| p.builtin) { rows.push(ListRow::item(p.name.clone())); acts.push(PresetRow::Preset(p.name.clone())); }
        if v.presets.iter().any(|p| !p.builtin) {
            rows.push(ListRow::Header(tr("Yours").into())); acts.push(PresetRow::None);
            for p in v.presets.iter().filter(|p| !p.builtin) { rows.push(ListRow::tagged(p.name.clone(), if p.stored.chain.is_ocio() { "OCIO" } else { "minColor" })); acts.push(PresetRow::Preset(p.name.clone())); }
        }
        self.presets.set_rows(rows);
        let sel = if v.custom { Some(0) } else { acts.iter().position(|a| *a == PresetRow::Preset(v.stored.preset.clone())) };
        self.presets.set_selected(sel);
        self.preset_rows = acts;
        if let ClipChain::MinColor(c) = &v.stored.chain {
            self.gamut.set_rows(self.gamut_names.iter().map(ListRow::item).collect()); self.gamut.set_selected(Some(c.in_gamut as usize));
            self.transfer.set_rows(self.transfer_names.iter().map(ListRow::item).collect()); self.transfer.set_selected(Some(c.in_transfer as usize));
            let mut r = vec![ListRow::item(tr("Un-tone-mapped")), ListRow::item("AgX")];
            if c.open_drt != 0 { r.push(ListRow::item(format!("OpenDRT · {}", self.look_names.get(c.look as usize).cloned().unwrap_or_default()))); }
            self.render.set_rows(r);
            self.render.set_selected(Some(if c.agx_on != 0 { 1 } else if c.open_drt != 0 { 2 } else { 0 }));
            self.knee_on.set(c.knee_on != 0, now);
            self.knee_src.set(c.knee_src.round() as i64); self.knee_tgt.set(c.knee_tgt.round() as i64);
            self.knee_start.set(if c.knee_start < 0.0 { 0 } else { (c.knee_start * 100.0).round() as i64 });
        } else {
            for l in [&mut self.gamut, &mut self.transfer, &mut self.render] { l.set_rows(vec![]); }
        }
        self.on.set(v.on, now);
        self.save_as.enabled = v.custom;
        self.reset.enabled = v.has_clip && v.own;
        self.apply_all.enabled = v.has_clip;
        self.make_default.enabled = v.has_clip && v.own;
        self.view = Some(v);
        true
    }

    /// `content` = the picture's rect (the tab sits at its bottom centre); the drawer spans `x0..x1` from `y0` down.
    pub fn layout(&mut self, fonts: &mut Fonts, content: Rect, x0: f64, y0: f64, x1: f64, scale: f64, now: Instant) {
        self.scale = scale;
        let h = self.visible_height(now, scale);
        self.rect = Rect::new(x0, y0, x1, y0 + h);
        let tw = t::COMPONENT_HEIGHT_100 as f64 * scale; let th = t::COMPONENT_HEIGHT_50 as f64 * scale * 0.8;
        let cx = (content.x0 + content.x1) / 2.0; let inset = t::SPACING_75 as f64 * scale;
        self.tab = Rect::new(cx - tw / 2.0, content.y1 - inset - th, cx + tw / 2.0, content.y1 - inset);
        if h <= 1.0 { return }
        let pad = t::SPACING_200 as f64 * scale; let gap = t::SPACING_200 as f64 * scale;
        let row = t::COMPONENT_HEIGHT_100 as f64 * scale;
        // the full layout at the open height (the drawer slides; content is clipped to what shows)
        let full = Rect::new(x0, y0, x1, y0 + self.height * scale);
        self.header = Rect::new(full.x0 + pad, full.y0, full.x1 - pad, full.y0 + row);
        // header buttons, right to left
        let mut bx = self.header.x1;
        for b in [&mut self.make_default, &mut self.apply_all, &mut self.reset] {
            let (w, bh) = b.measure(fonts, scale); b.rect = Rect::new(bx - w, self.header.y0 + (row - bh) / 2.0, bx, self.header.y0 + (row + bh) / 2.0); bx -= w + t::SPACING_100 as f64 * scale;
        }
        let bottom = Rect::new(full.x0 + pad, full.y1 - row, full.x1 - pad, full.y1);
        let (sw, sh) = Switch::size(scale);
        self.on_rect = bottom; self.on.rect = Rect::new(bottom.x0, bottom.y0 + (row - sh) / 2.0, bottom.x0 + sw, bottom.y0 + (row + sh) / 2.0);
        let body = Rect::new(full.x0 + pad, self.header.y1, full.x1 - pad, bottom.y0 - t::SPACING_100 as f64 * scale);
        // Save as preset… under the presets list
        let (bw, bh) = self.save_as.measure(fonts, scale);
        let mut x = body.x0;
        let pw = PRESETS_W * scale;
        self.presets.layout(Rect::new(x, body.y0, x + pw, body.y1 - bh - t::SPACING_100 as f64 * scale), scale);
        self.save_as.rect = Rect::new(x, body.y1 - bh, x + bw.max(pw.min(bw)), body.y1);
        x += pw + gap;
        for l in [&mut self.gamut, &mut self.transfer] { l.layout(Rect::new(x, body.y0, x + LIST_W * scale, body.y1), scale); x += LIST_W * scale + gap; }
        self.knee_rect = Rect::new(x, body.y0, x + KNEE_W * scale, body.y1);
        let cap = t::COMPONENT_HEIGHT_75 as f64 * scale; let fh = t::FIELD_HEIGHT_S as f64 * scale; let lab = 64.0 * scale;
        let mut ky = body.y0 + cap;
        self.knee_on.rect = Rect::new(x, ky + (row - sh) / 2.0, x + sw, ky + (row + sh) / 2.0); ky += row;
        for f in [&mut self.knee_src, &mut self.knee_tgt, &mut self.knee_start] { f.layout(fonts, Rect::new(x + lab, ky + (row - fh) / 2.0, x + KNEE_W * scale, ky + (row + fh) / 2.0), scale); ky += row; }
        x += KNEE_W * scale + gap;
        self.render.layout(Rect::new(x, body.y0, x + LIST_W * scale, body.y1), scale);
    }

    fn edited(&self, f: impl FnOnce(&mut qcv_color::Chain)) -> Option<Cmd> {
        let v = self.view.as_ref()?;
        let ClipChain::MinColor(mut c) = v.stored.chain.clone() else { return None };
        f(&mut c);
        Some(Cmd::Edit(Stored { chain: ClipChain::MinColor(c), preset: v.stored.preset.clone() }))
    }

    fn open_naming(&mut self, n: Naming, fonts: &mut Fonts, bounds: Rect) {
        let s = self.scale;
        let (title, text) = match &n { Naming::Save => (tr("PRESET NAME"), self.view.as_ref().map(|v| if v.stored.preset.is_empty() { String::new() } else { format!("{} 2", v.stored.preset) }).unwrap_or_default()), Naming::Rename(old) => (tr("RENAME PRESET"), old.clone()) };
        self.name_pop.title = title.into();
        self.name_pop.open_at(fonts, self.save_as.rect, 220.0, t::FIELD_HEIGHT_S as f64, bounds, s);
        self.name_field.set_text_end(&text, fonts); self.name_field.select_all(fonts); self.name_field.focused = true;
        self.name_field.layout(fonts, self.name_pop.content, s);
        self.naming = Some(n);
    }
    fn close_naming(&mut self) { self.naming = None; self.name_pop.close(); self.name_field.focused = false; }

    /// Returns (commands, dirty, consumed).
    pub fn event(&mut self, e: &Event, now: Instant, fonts: &mut Fonts, bounds: Rect) -> (Vec<Cmd>, bool, bool) {
        let mut cmds = vec![]; let mut dirty = false;
        // the naming popover takes everything while open; a press outside cancels
        if self.naming.is_some() {
            match *e {
                Event::Down { x, y, .. } if !self.name_pop.hit(x, y) => { self.close_naming(); return (cmds, true, true) }
                Event::Leave => { self.name_field.pointer(e, fonts); return (cmds, true, false) }
                _ => { let d = self.name_field.pointer(e, fonts); return (cmds, d, true) }
            }
        }
        // the preset menu
        if self.menu.is_open() {
            let (pick, d, consumed) = self.menu.event(e);
            if let (Some(i), Some(name)) = (pick, self.menu_for.clone()) {
                match i { 0 => self.open_naming(Naming::Rename(name), fonts, bounds), 1 => cmds.push(Cmd::UpdatePreset(name)), 2 => cmds.push(Cmd::DeletePreset(name)), _ => {} }
                self.menu_for = None;
            }
            return (cmds, d || pick.is_some(), consumed)
        }
        // the tab
        let over_tab = e.pos().is_some_and(|(x, y)| self.tab.contains(kurbo::Point::new(x, y)));
        if over_tab != self.tab_hover { self.tab_hover = over_tab; self.tab_ramp.set_target(if over_tab { 1.0 } else { 0.0 }, now); dirty = true; }
        if let Event::Down { button: Button::Primary, .. } = e { if over_tab { cmds.push(Cmd::Toggle); return (cmds, true, true) } }
        if self.rect.height() <= 1.0 { return (cmds, dirty, false) }
        let inside = e.pos().is_some_and(|(x, y)| self.rect.contains(kurbo::Point::new(x, y)));
        if !inside && !matches!(e, Event::Up { .. } | Event::Leave) { return (cmds, dirty, false) }
        let mc = self.view.as_ref().is_some_and(|v| !v.stored.chain.is_ocio());
        // the lists
        let r = self.presets.event(e, now); dirty |= r.dirty;
        if let Some(i) = r.clicked { if let (Some(PresetRow::Preset(name)), Some(v)) = (self.preset_rows.get(i), self.view.as_ref()) { if let Some(p) = v.presets.iter().find(|p| p.name == *name) { cmds.push(Cmd::Pick(p.stored.clone())); } } }
        if let Some((i, x, y)) = r.context { if let Some(PresetRow::Preset(name)) = self.preset_rows.get(i).cloned() {
            let user = self.view.as_ref().is_some_and(|v| v.presets.iter().any(|p| p.name == name && !p.builtin));
            if user { self.menu.open_at(vec![MenuItem::action(tr("Rename…"), true), MenuItem::action(tr("Update to this chain"), true), MenuItem::Separator, MenuItem::action(tr("Delete"), true)], fonts, x, y, bounds, self.scale); self.menu_for = Some(name); dirty = true; }
        } }
        if mc {
            let r = self.gamut.event(e, now); dirty |= r.dirty; if let Some(i) = r.clicked { cmds.extend(self.edited(|c| c.in_gamut = i as i32)); }
            let r = self.transfer.event(e, now); dirty |= r.dirty; if let Some(i) = r.clicked { cmds.extend(self.edited(|c| c.in_transfer = i as i32)); }
            let r = self.render.event(e, now); dirty |= r.dirty;
            if let Some(i) = r.clicked { cmds.extend(self.edited(|c| match i { 0 => { c.agx_on = 0; c.open_drt = 0; } 1 => { c.agx_on = 1; c.open_drt = 0; } _ => { c.agx_on = 0; c.open_drt = 1; } })); }
            let (k, d) = self.knee_on.event(e, now); dirty |= d; if let Some(on) = k { cmds.extend(self.edited(|c| c.knee_on = on as i32)); }
            let (v, d) = self.knee_src.event(e, fonts); dirty |= d; if let Some(v) = v { cmds.extend(self.edited(|c| c.knee_src = v as f32)); }
            let (v, d) = self.knee_tgt.event(e, fonts); dirty |= d; if let Some(v) = v { cmds.extend(self.edited(|c| c.knee_tgt = v as f32)); }
            let (v, d) = self.knee_start.event(e, fonts); dirty |= d; if let Some(v) = v { cmds.extend(self.edited(|c| c.knee_start = if v == 0 { -1.0 } else { v as f32 / 100.0 })); }
        }
        let (k, d) = self.on.event(e, now); dirty |= d; if let Some(on) = k { cmds.push(Cmd::SetOn(on)); }
        let (c, d) = self.save_as.event(e, now); dirty |= d; if c { self.open_naming(Naming::Save, fonts, bounds); dirty = true; }
        let (c, d) = self.reset.event(e, now); dirty |= d; if c { cmds.push(Cmd::ResetToDefault); }
        let (c, d) = self.apply_all.event(e, now); dirty |= d; if c { cmds.push(Cmd::ApplyToAll); }
        let (c, d) = self.make_default.event(e, now); dirty |= d; if c { cmds.push(Cmd::MakeDefault); }
        let d = dirty || !cmds.is_empty();
        (cmds, d, inside)
    }
    pub fn wheel(&mut self, x: f64, y: f64, dy_px: f64) -> bool {
        if !self.rect.contains(kurbo::Point::new(x, y)) { return false }
        let mut moved = false;
        for l in [&mut self.presets, &mut self.gamut, &mut self.transfer, &mut self.render] { moved |= l.wheel(x, y, dy_px); }
        moved
    }
    /// Keys while a field has focus (the preset name, the knee numbers). Returns commands and whether the key was used.
    pub fn key(&mut self, k: &KeyEvent, fonts: &mut Fonts) -> (Vec<Cmd>, bool) {
        if let Some(n) = self.naming.clone() {
            match self.name_field.key(k, fonts) {
                FieldResponse::Accepted => { let name = self.name_field.text().trim().to_string(); self.close_naming(); if name.is_empty() { return (vec![], true) } return (vec![match n { Naming::Save => Cmd::SavePreset(name), Naming::Rename(old) => Cmd::RenamePreset(old, name) }], true) }
                FieldResponse::Cancelled => { self.close_naming(); return (vec![], true) }
                _ => return (vec![], true),
            }
        }
        let mut cmds = vec![];
        if self.knee_src.focused() { if let Some(v) = self.knee_src.key(k, fonts) { cmds.extend(self.edited(|c| c.knee_src = v as f32)); } return (cmds, true) }
        if self.knee_tgt.focused() { if let Some(v) = self.knee_tgt.key(k, fonts) { cmds.extend(self.edited(|c| c.knee_tgt = v as f32)); } return (cmds, true) }
        if self.knee_start.focused() { if let Some(v) = self.knee_start.key(k, fonts) { cmds.extend(self.edited(|c| c.knee_start = if v == 0 { -1.0 } else { v as f32 / 100.0 })); } return (cmds, true) }
        (cmds, false)
    }
    pub fn ime(&mut self, e: &ImeEvent, fonts: &mut Fonts) { if self.naming.is_some() { self.name_field.ime(e, fonts); } }
    /// A press elsewhere commits a typed knee number.
    pub fn blur(&mut self) -> Vec<Cmd> {
        let mut cmds = vec![];
        if let Some(v) = self.knee_src.blur() { cmds.extend(self.edited(|c| c.knee_src = v as f32)); }
        if let Some(v) = self.knee_tgt.blur() { cmds.extend(self.edited(|c| c.knee_tgt = v as f32)); }
        if let Some(v) = self.knee_start.blur() { cmds.extend(self.edited(|c| c.knee_start = if v == 0 { -1.0 } else { v as f32 / 100.0 })); }
        cmds
    }

    /// The tab over the picture (drawn with the picture's overlays).
    pub fn paint_tab(&self, scene: &mut Scene, fonts: &mut Fonts, now: Instant) {
        let p = self.present.value(now).max(if self.open { 1.0 } else { 0.0 }); if p <= 0.001 { return }
        let s = self.scale; let a = self.tab_ramp.value(now);
        let bg = mix(t::HOVER_BACKGROUND_COLOR_IDLE, t::HOVER_BACKGROUND_COLOR, a); let bg = bg.with_alpha(bg.a * p);
        if bg.a > 0.0 { paint::rounded(scene, self.tab, t::CORNER_RADIUS_SMALL_DEFAULT as f64 * s, bg); }
        let c = mix(t::RAIL_OPEN_TAB_COLOR, t::NEUTRAL_CONTENT_COLOR_HOVER, a); let c = c.with_alpha(c.a * p);
        if let Some(g) = fonts.icon(if self.open { "caret-down" } else { "caret-up" }, t::ICON_SIZE_SMALL * s as f32, paint::color(c)) { min_ui::fonts::draw_layout(scene, &g, Affine::translate((self.tab.x0 + (self.tab.width() - g.width() as f64) / 2.0, self.tab.y0 + (self.tab.height() - g.height() as f64) / 2.0))); }
    }
    pub fn paint(&mut self, scene: &mut Scene, fonts: &mut Fonts, now: Instant) {
        if self.rect.height() <= 1.0 { return }
        let s = self.scale;
        paint::rect(scene, self.rect, t::RAIL_BACKGROUND_COLOR);
        paint::rect(scene, Rect::new(self.rect.x0, self.rect.y0, self.rect.x1, self.rect.y0 + s.max(1.0)), t::DIVIDER_COLOR);
        scene.push_clip_layer(peniko::Fill::NonZero, Affine::IDENTITY, &self.rect);
        let Some(v) = self.view.clone() else { scene.pop_layer(); return };
        // header: COLOR · the target · its status
        fn mid<B: parley::Brush>(l: &parley::Layout<B>, r: Rect) -> f64 { r.y0 + (r.height() - l.height() as f64) / 2.0 }
        let h = fonts.detail(tr("Color"), t::RAIL_GROUP_HEADER_SIZE * s as f32, paint::color(t::RAIL_GROUP_HEADER_COLOR));
        let mut x = self.header.x0; min_ui::fonts::draw_layout(scene, &h, Affine::translate((x, mid(&h, self.header)))); x += h.width() as f64 + t::SPACING_200 as f64 * s;
        let tl = fonts.layout(&v.target, Family::Sans, t::RAIL_LABEL_SIZE * s as f32, t::FONT_WEIGHT_BOLD as f32, paint::color(t::RAIL_LABEL_COLOR));
        min_ui::fonts::draw_layout(scene, &tl, Affine::translate((x, mid(&tl, self.header)))); x += tl.width() as f64 + t::SPACING_200 as f64 * s;
        let status = if !v.has_clip { tr("Clips without their own chain follow this one") } else if v.own { tr("Own chain") } else { tr("Following the project default") };
        let sl = fonts.layout(status, Family::Sans, t::RAIL_LABEL_SIZE * s as f32, t::FONT_WEIGHT_REGULAR as f32, paint::color(t::RAIL_GROUP_HEADER_COLOR));
        min_ui::fonts::draw_layout(scene, &sl, Affine::translate((x, mid(&sl, self.header))));
        for b in [&self.reset, &self.apply_all, &self.make_default] { if v.has_clip { b.paint(scene, fonts, s, now); } }
        self.presets.paint(scene, fonts, now);
        self.save_as.paint(scene, fonts, s, now);
        match &v.stored.chain {
            ClipChain::MinColor(_) => {
                self.gamut.paint(scene, fonts, now); self.transfer.paint(scene, fonts, now); self.render.paint(scene, fonts, now);
                let cap = fonts.detail(tr("Highlight Knee"), t::RAIL_GROUP_HEADER_SIZE * s as f32, paint::color(t::RAIL_GROUP_HEADER_COLOR));
                let capr = Rect::new(self.knee_rect.x0, self.knee_rect.y0, self.knee_rect.x1, self.knee_rect.y0 + t::COMPONENT_HEIGHT_75 as f64 * s);
                min_ui::fonts::draw_layout(scene, &cap, Affine::translate((self.knee_rect.x0 + t::SPACING_50 as f64 * s, mid(&cap, capr))));
                self.knee_on.paint(scene, s, now);
                for (f, label) in [(&mut self.knee_src, tr("Source")), (&mut self.knee_tgt, tr("Target")), (&mut self.knee_start, tr("Start %"))] {
                    let l = fonts.layout(label, Family::Sans, t::RAIL_LABEL_SIZE * s as f32, t::FONT_WEIGHT_REGULAR as f32, paint::color(t::RAIL_LABEL_COLOR));
                    min_ui::fonts::draw_layout(scene, &l, Affine::translate((self.knee_rect.x0, f.rect.y0 + (f.rect.height() - l.height() as f64) / 2.0)));
                    f.paint(scene, fonts, s);
                }
            }
            ClipChain::Ocio(o) => {
                let l = fonts.layout(&format!("OCIO · {} · {} → {}", o.config, o.input, o.view), Family::Sans, t::RAIL_LABEL_SIZE * s as f32, t::FONT_WEIGHT_REGULAR as f32, paint::color(t::RAIL_LABEL_COLOR));
                min_ui::fonts::draw_layout(scene, &l, Affine::translate((self.gamut.rect.x0, self.gamut.rect.y0 + t::COMPONENT_HEIGHT_75 as f64 * s)));
            }
        }
        // bottom row: the one On / Off switch
        self.on.paint(scene, s, now);
        let ol = fonts.layout(if v.on { tr("Color On") } else { tr("Color Off") }, Family::Sans, t::RAIL_LABEL_SIZE * s as f32, t::FONT_WEIGHT_REGULAR as f32, paint::color(if v.on { t::RAIL_LABEL_COLOR } else { t::RAIL_GROUP_HEADER_COLOR }));
        min_ui::fonts::draw_layout(scene, &ol, Affine::translate((self.on.rect.x1 + t::SPACING_100 as f64 * s, mid(&ol, self.on_rect))));
        scene.pop_layer();
    }
    /// The menu and the naming popover, over everything.
    pub fn paint_overlays(&mut self, scene: &mut Scene, fonts: &mut Fonts) {
        if self.menu.is_open() { self.menu.paint(scene, fonts, self.scale); }
        if self.naming.is_some() { self.name_pop.paint(scene, fonts, self.scale); self.name_field.paint(scene, fonts, self.scale); }
    }
}
