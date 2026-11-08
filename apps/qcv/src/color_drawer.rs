//! The Color drawer (canon rows 199 / 205–208, ruled 2026-11-08): QCView's Color panel moved above the timeline as a bottom
//! drawer that pushes the picture up, toggled by an up-caret tab on the viewport (like the rails' tabs), QCView's menu item,
//! transport button and Ctrl+3. One whole chain per clip with a project default; presets are starting points (picking one
//! copies its chain); an edited chain shows as a "Custom (from …)" row with one action, Save as preset…; Rename · Update ·
//! Delete on a user preset's right-click menu; one list for both engines. minColor's columns: Input gamut · Input transfer ·
//! Highlight Knee · AgX · Rendering — the Knee and AgX are QCView's optional columns (KneeColumn / AgxColumn in
//! ColorPanel.qml) with their whole interface, collapsing to a slim strip. Every column is a layer-2 card under its caption
//! with the rails' overlay scrollbar (the user 2026-11-08: "some layer-2 background around lists … scrollbars for the lists").
//! OCIO's columns, the viewer aids and the waveform follow.
use crate::color_model::{ClipChain, OcioChain, Preset, Stored};
use kurbo::{Affine, Rect};
use min_theme::tokens as t;
use crate::color_columns::{AgxPanel, KneePanel};
use min_ui::{anim::{mix, Ramp}, paint, Button, Event, Family, FieldResponse, Fonts, ImeEvent, KeyEvent, ListColumn, ListRow, Menu, MenuItem, Popover, Switch, TextButton, TextField, Variant};
use std::time::Instant;
use vello::Scene;

/// QCView's catalog context for its Color panel (canon row 195).
fn tr(s: &'static str) -> &'static str { min_i18n::trc("ColorPanel", s) }

#[derive(Clone, Debug, PartialEq)]
pub enum Cmd { Pick(Stored), Edit(Stored), /// "New OCIO chain…" / the Config column's "Open config…"
    NewOcio, OpenConfig, ResetToDefault, ApplyToAll, MakeDefault, SetOn(bool), SavePreset(String), RenamePreset(String, String), UpdatePreset(String), DeletePreset(String), Toggle }

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
    /// the loaded file's HDR10 metadata (MaxCLL, mastering peak) for the Knee's "Use file MaxCLL"
    pub file_peak: (i32, f64),
    /// an OCIO chain's lists, from its config
    pub ocio: Option<OcioLists>,
}

/// What an OCIO chain's columns offer (from the loaded config; the configs are the bundled three + a user's file).
#[derive(Clone, Debug, PartialEq, Default)]
pub struct OcioLists { pub configs: Vec<(String, String)>, pub inputs: Vec<String>, pub looks: Vec<String>, pub views: Vec<String> }

#[derive(Clone, Debug, PartialEq)]
enum Naming { Save, Rename(String) }
#[derive(Clone, Debug, PartialEq)]
enum PresetRow { Custom, Preset(String), NewOcio, None }

pub struct ColorDrawer {
    pub open: bool,
    open_ramp: Ramp,
    /// logical px (the user can resize later; the store keeps it)
    pub height: f64,
    pub rect: Rect,
    tab: Rect, tab_hover: bool, tab_ramp: Ramp, present: Ramp,
    presets: ListColumn, gamut: ListColumn, transfer: ListColumn, render: ListColumn,
    /// OCIO's columns (QCView: Config · Input · Look · View)
    ocfg: ListColumn, oinput: ListColumn, olook: ListColumn, oview: ListColumn, ocfg_keys: Vec<String>,
    preset_rows: Vec<PresetRow>,
    knee: KneePanel, agx: AgxPanel, /// the target the columns' open / collapsed state was last set for
    columns_for: String,
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

impl ColorDrawer {
    pub fn new(open: bool, height: f64) -> ColorDrawer {
        let mut gamut_names = qcv_color::input_gamut_names(); gamut_names.truncate(15);
        let transfer_names = qcv_color::input_transfer_names();
        ColorDrawer {
            open, open_ramp: Ramp::new(if open { 1.0 } else { 0.0 }, t::RAIL_COLLAPSE_DURATION), height, rect: Rect::ZERO,
            tab: Rect::ZERO, tab_hover: false, tab_ramp: Ramp::new(0.0, t::ANIMATION_DURATION_HOVER), present: Ramp::new(0.0, t::ANIMATION_DURATION_SLIDE),
            presets: ListColumn::new(tr("Presets")), gamut: ListColumn::new(tr("Input gamut")), transfer: ListColumn::new(tr("Input transfer")), render: ListColumn::new(tr("Rendering")),
            ocfg: ListColumn::new(tr("Config")), oinput: ListColumn::new(tr("Input")).with_filter(), olook: ListColumn::new(tr("Look")), oview: ListColumn::new(tr("View")), ocfg_keys: vec![],
            preset_rows: vec![],
            knee: KneePanel::new(), agx: AgxPanel::new(), columns_for: String::new(),
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
    pub fn animating(&self, now: Instant) -> bool { self.open_ramp.animating(now) || self.tab_ramp.animating(now) || self.present.animating(now) || [&self.presets, &self.gamut, &self.transfer, &self.render].iter().any(|l| l.animating(now)) || self.knee.animating(now) || self.agx.animating(now) || self.on.animating(now) || [&self.save_as, &self.reset, &self.apply_all, &self.make_default].iter().any(|b| b.animating(now)) }
    pub fn is_modal(&self) -> bool { self.menu.is_open() || self.naming.is_some() }
    pub fn has_focus(&self) -> bool { self.naming.is_some() || self.knee.focused() || self.agx.focused() || self.oinput.focused() }
    fn dragging(&self) -> bool { self.knee.dragging() || self.agx.dragging() }
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
        rows.push(ListRow::Header("OCIO".into())); acts.push(PresetRow::None);
        rows.push(ListRow::item(tr("New OCIO chain…"))); acts.push(PresetRow::NewOcio);
        self.presets.set_rows(rows);
        let sel = if v.custom { Some(0) } else { acts.iter().position(|a| *a == PresetRow::Preset(v.stored.preset.clone())) };
        self.presets.set_selected(sel);
        self.preset_rows = acts;
        if let ClipChain::MinColor(c) = &v.stored.chain {
            self.gamut.set_rows(self.gamut_names.iter().map(ListRow::item).collect()); self.gamut.set_selected(Some(c.in_gamut as usize));
            self.transfer.set_rows(self.transfer_names.iter().map(ListRow::item).collect()); self.transfer.set_selected(Some(c.in_transfer as usize));
            // QCView's Rendering column: un-tone-mapped is what it offers; a chain carrying an OpenDRT look shows it; AgX on =
            // the picture formation has happened (its title says so)
            let mut r = vec![ListRow::item(tr("Un-tone-mapped"))];
            if c.open_drt != 0 { r.push(ListRow::item(format!("OpenDRT · {}", self.look_names.get(c.look as usize).cloned().unwrap_or_default()))); }
            self.render.set_rows(r);
            self.render.title = if c.agx_on != 0 { tr("Rendering · AgX").into() } else { tr("Rendering").into() };
            self.render.set_selected(Some(if c.open_drt != 0 && c.agx_on == 0 { 1 } else { 0 }));
            // the optional columns open for a chain that uses them, once per target; the user's toggles stand after that
            if self.columns_for != v.target { self.columns_for = v.target.clone(); self.knee.set_expanded(c.knee_on != 0); self.agx.set_expanded(c.agx_on != 0); }
            self.knee.sync(c, v.file_peak, now); self.agx.sync(c, now);
        } else if let (ClipChain::Ocio(o), Some(lists)) = (&v.stored.chain, v.ocio.as_ref()) {
            let mut crows: Vec<ListRow> = lists.configs.iter().map(|(_, l)| ListRow::item(l.clone())).collect();
            crows.push(ListRow::item(tr("Open config…")));
            self.ocfg_keys = lists.configs.iter().map(|(k, _)| k.clone()).collect();
            self.ocfg.set_rows(crows); self.ocfg.set_selected(self.ocfg_keys.iter().position(|k| *k == o.config).or(Some(usize::MAX)).filter(|i| *i != usize::MAX));
            self.oinput.set_rows(lists.inputs.iter().map(ListRow::item).collect()); self.oinput.set_selected(lists.inputs.iter().position(|i| *i == o.input));
            let mut lrows = vec![ListRow::item(tr("None"))]; lrows.extend(lists.looks.iter().map(ListRow::item));
            self.olook.set_rows(lrows); self.olook.set_selected(Some(lists.looks.iter().position(|l| *l == o.look).map(|i| i + 1).unwrap_or(0)));
            self.oview.set_rows(lists.views.iter().map(ListRow::item).collect()); self.oview.set_selected(lists.views.iter().position(|w| *w == o.view));
            let kc = qcv_color::Chain { knee_on: o.knee.enabled as i32, knee_src: o.knee.source_nits, knee_tgt: o.knee.target_nits, knee_start: o.knee.start, ..Default::default() };
            if self.columns_for != v.target { self.columns_for = v.target.clone(); self.knee.set_expanded(o.knee.enabled); }
            self.knee.set_sdr_view(qcv_color::ocio::view_is_sdr(&o.view));
            self.knee.sync(&kc, v.file_peak, now);
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
        let ocio = self.view.as_ref().is_some_and(|v| v.stored.chain.is_ocio());
        if ocio {
            // the View column takes what is left once the Knee has its width (the names are long: "… (Rec.709 D60 in Rec.709 D65)")
            self.ocfg.layout(Rect::new(x, body.y0, x + 130.0 * scale, body.y1), scale); x += 130.0 * scale + gap;
            self.oinput.layout_with(fonts, Rect::new(x, body.y0, x + 210.0 * scale, body.y1), scale); x += 210.0 * scale + gap;
            self.olook.layout(Rect::new(x, body.y0, x + 130.0 * scale, body.y1), scale); x += 130.0 * scale + gap;
            let vw = (body.x1 - x - gap - self.knee.width() * scale).max(180.0 * scale);
            self.oview.layout(Rect::new(x, body.y0, x + vw, body.y1), scale); x += vw + gap;
            self.knee.layout(fonts, x, body.y0, body.y1, scale);
        } else {
            for l in [&mut self.gamut, &mut self.transfer] { l.layout(Rect::new(x, body.y0, x + LIST_W * scale, body.y1), scale); x += LIST_W * scale + gap; }
            x += self.knee.layout(fonts, x, body.y0, body.y1, scale) + gap;
            x += self.agx.layout(fonts, x, body.y0, body.y1, scale) + gap;
            self.render.layout(Rect::new(x, body.y0, x + (body.x1 - x).clamp(140.0 * scale, LIST_W * scale), body.y1), scale);
        }
    }

    fn edited(&self, f: impl FnOnce(&mut qcv_color::Chain)) -> Option<Cmd> {
        let v = self.view.as_ref()?;
        let ClipChain::MinColor(mut c) = v.stored.chain.clone() else { return None };
        f(&mut c);
        Some(Cmd::Edit(Stored { chain: ClipChain::MinColor(c), preset: v.stored.preset.clone() }))
    }

    /// The knee's edit, onto whichever engine the chain is on.
    fn knee_edit(&self, c: qcv_color::Chain) -> Option<Cmd> {
        let v = self.view.as_ref()?;
        match &v.stored.chain {
            ClipChain::Ocio(o) => Some(Cmd::Edit(Stored { chain: ClipChain::Ocio(OcioChain { knee: qcv_color::Knee { enabled: c.knee_on != 0, source_nits: c.knee_src, target_nits: c.knee_tgt, start: c.knee_start }, ..o.clone() }), preset: v.stored.preset.clone() })),
            ClipChain::MinColor(m) => Some(Cmd::Edit(Stored { chain: ClipChain::MinColor(qcv_color::Chain { knee_on: c.knee_on, knee_src: c.knee_src, knee_tgt: c.knee_tgt, knee_start: c.knee_start, ..*m }), preset: v.stored.preset.clone() })),
        }
    }
    fn with_chain(&self, c: qcv_color::Chain) -> Option<Cmd> { let v = self.view.as_ref()?; Some(Cmd::Edit(Stored { chain: ClipChain::MinColor(c), preset: v.stored.preset.clone() })) }
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
        if !inside && !self.dragging() && !matches!(e, Event::Up { .. } | Event::Leave) { return (cmds, dirty, false) }
        let mc = self.view.as_ref().is_some_and(|v| !v.stored.chain.is_ocio());
        // the lists
        let r = self.presets.event(e, now, None); dirty |= r.dirty;
        if let Some(i) = r.clicked {
            match (self.preset_rows.get(i), self.view.as_ref()) {
                (Some(PresetRow::Preset(name)), Some(v)) => { if let Some(p) = v.presets.iter().find(|p| p.name == *name) { cmds.push(Cmd::Pick(p.stored.clone())); } }
                (Some(PresetRow::NewOcio), _) => cmds.push(Cmd::NewOcio),
                _ => {}
            }
        }
        if let Some((i, x, y)) = r.context { if let Some(PresetRow::Preset(name)) = self.preset_rows.get(i).cloned() {
            let user = self.view.as_ref().is_some_and(|v| v.presets.iter().any(|p| p.name == name && !p.builtin));
            if user { self.menu.open_at(vec![MenuItem::action(tr("Rename…"), true), MenuItem::action(tr("Update to this chain"), true), MenuItem::Separator, MenuItem::action(tr("Delete"), true)], fonts, x, y, bounds, self.scale); self.menu_for = Some(name); dirty = true; }
        } }
        let oc = self.view.as_ref().and_then(|v| if let ClipChain::Ocio(o) = &v.stored.chain { Some(o.clone()) } else { None });
        if let Some(o) = oc {
            let lists = self.view.as_ref().and_then(|v| v.ocio.clone()).unwrap_or_default();
            let edit = |o: OcioChain| { let p = self.view.as_ref().map(|v| v.stored.preset.clone()).unwrap_or_default(); Cmd::Edit(Stored { chain: ClipChain::Ocio(o), preset: p }) };
            let r = self.ocfg.event(e, now, None); dirty |= r.dirty;
            if let Some(i) = r.clicked { match self.ocfg_keys.get(i) { Some(k) => cmds.push(edit(OcioChain { config: k.clone(), ..o.clone() })), None => cmds.push(Cmd::OpenConfig) } }
            let r = self.oinput.event(e, now, Some(fonts)); dirty |= r.dirty;
            if let Some(i) = r.clicked { if let Some(n) = lists.inputs.get(i) { cmds.push(edit(OcioChain { input: n.clone(), ..o.clone() })); } }
            let r = self.olook.event(e, now, None); dirty |= r.dirty;
            if let Some(i) = r.clicked { cmds.push(edit(OcioChain { look: if i == 0 { String::new() } else { lists.looks.get(i - 1).cloned().unwrap_or_default() }, ..o.clone() })); }
            let r = self.oview.event(e, now, None); dirty |= r.dirty;
            if let Some(i) = r.clicked { if let Some(n) = lists.views.get(i) { cmds.push(edit(OcioChain { view: n.clone(), ..o.clone() })); } }
            let (c, d) = self.knee.event(e, now, fonts); dirty |= d;
            if let Some(c) = c { cmds.push(edit(OcioChain { knee: qcv_color::Knee { enabled: c.knee_on != 0, source_nits: c.knee_src, target_nits: c.knee_tgt, start: c.knee_start }, ..o.clone() })); }
        }
        if mc {
            let r = self.gamut.event(e, now, None); dirty |= r.dirty; if let Some(i) = r.clicked { cmds.extend(self.edited(|c| c.in_gamut = i as i32)); }
            let r = self.transfer.event(e, now, None); dirty |= r.dirty; if let Some(i) = r.clicked { cmds.extend(self.edited(|c| c.in_transfer = i as i32)); }
            let r = self.render.event(e, now, None); dirty |= r.dirty;
            if let Some(i) = r.clicked { cmds.extend(self.edited(|c| match i { 0 => { c.agx_on = 0; c.open_drt = 0; } _ => { c.agx_on = 0; c.open_drt = 1; } })); }
            let (c, d) = self.knee.event(e, now, fonts); dirty |= d; if let Some(c) = c { cmds.extend(self.with_chain(c)); }
            let (c, d) = self.agx.event(e, now, fonts); dirty |= d; if let Some(c) = c { cmds.extend(self.with_chain(c)); }
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
        for l in [&mut self.presets, &mut self.gamut, &mut self.transfer, &mut self.render, &mut self.ocfg, &mut self.oinput, &mut self.olook, &mut self.oview] { moved |= l.wheel(x, y, dy_px); }
        let now = Instant::now(); moved |= self.knee.wheel(x, y, dy_px, now); moved |= self.agx.wheel(x, y, dy_px, now);
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
        if self.oinput.focused() { self.oinput.key(k, fonts); return (vec![], true) }
        if self.knee.focused() { return (self.knee.key(k, fonts).and_then(|c| self.knee_edit(c)).into_iter().collect(), true) }
        if self.agx.focused() { return (self.agx.key(k, fonts).and_then(|c| self.with_chain(c)).into_iter().collect(), true) }
        (vec![], false)
    }
    pub fn ime(&mut self, e: &ImeEvent, fonts: &mut Fonts) { if self.naming.is_some() { self.name_field.ime(e, fonts); } else if self.oinput.focused() { self.oinput.ime(e, fonts); } }
    pub fn is_typing(&self) -> bool { self.naming.is_some() || self.oinput.focused() }
    /// A press elsewhere commits a typed knee number.
    pub fn blur(&mut self) -> Vec<Cmd> {
        let mut cmds = vec![];
        self.oinput.blur();
        if let Some(c) = self.knee.blur() { cmds.extend(self.knee_edit(c)); }
        if let Some(c) = self.agx.blur() { cmds.extend(self.with_chain(c)); }
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
        min_ui::fonts::draw_layout(scene, &sl, Affine::translate((x, mid(&sl, self.header)))); x += sl.width() as f64 + t::SPACING_200 as f64 * s;
        // QCView's amber KNEE pill: the picture is not 1:1 while the knee compresses
        let kneeing = match &v.stored.chain { ClipChain::MinColor(c) => c.knee_on != 0 && c.knee_src > c.knee_tgt, ClipChain::Ocio(o) => o.knee.enabled && o.knee.source_nits > if qcv_color::ocio::view_is_sdr(&o.view) { 100.0 } else { o.knee.target_nits } };
        { if v.on && kneeing {
            let pl = fonts.detail("KNEE", t::RAIL_GROUP_HEADER_SIZE * s as f32, paint::color(t::GRAY_50));
            let ph = t::COMPONENT_HEIGHT_50 as f64 * s * 0.9; let pw = pl.width() as f64 + 2.0 * t::SPACING_100 as f64 * s; let py = self.header.y0 + (self.header.height() - ph) / 2.0;
            paint::rounded(scene, Rect::new(x, py, x + pw, py + ph), ph / 2.0, t::NOTICE_BACKGROUND_COLOR_DEFAULT);
            min_ui::fonts::draw_layout(scene, &pl, Affine::translate((x + t::SPACING_100 as f64 * s, py + (ph - pl.height() as f64) / 2.0)));
        } }
        for b in [&self.reset, &self.apply_all, &self.make_default] { if v.has_clip { b.paint(scene, fonts, s, now); } }
        self.presets.paint(scene, fonts, now);
        self.save_as.paint(scene, fonts, s, now);
        match &v.stored.chain {
            ClipChain::MinColor(_) => {
                self.gamut.paint(scene, fonts, now); self.transfer.paint(scene, fonts, now);
                self.knee.paint(scene, fonts, now); self.agx.paint(scene, fonts, now);
                self.render.paint(scene, fonts, now);
            }
            ClipChain::Ocio(_) => {
                if v.ocio.is_some() { for l in [&mut self.ocfg, &mut self.oinput, &mut self.olook, &mut self.oview] { l.paint(scene, fonts, now); } self.knee.paint(scene, fonts, now); }
                else { let l = fonts.layout(tr("Loading the OCIO config…"), Family::Sans, t::RAIL_LABEL_SIZE * s as f32, t::FONT_WEIGHT_REGULAR as f32, paint::color(t::RAIL_GROUP_HEADER_COLOR)); min_ui::fonts::draw_layout(scene, &l, Affine::translate((self.save_as.rect.x1 + 40.0 * s, self.header.y1 + 40.0 * s))); }
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
