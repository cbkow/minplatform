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
use min_present::ViewerAids;
use min_i18n::fill;
use min_ui::{anim::{mix, Ramp}, paint, ActionButton, ActionGroup, Button, Event, Response, Slider, Family, FieldResponse, Fonts, ImeEvent, KeyEvent, ListColumn, ListRow, Menu, MenuItem, Popover, Switch, TextButton, TextField, Variant};
use std::time::Instant;
use vello::Scene;

/// QCView's catalog context for its Color panel (canon row 195).
fn tr(s: &'static str) -> &'static str { min_i18n::trc("ColorPanel", s) }

#[derive(Clone, Debug, PartialEq)]
pub enum Cmd { Pick(Stored), Edit(Stored), /// "New OCIO chain…" / the Config column's "Open config…"
    NewOcio, OpenConfig, /// the bottom row's viewer aids (QCView: session-only, never in presets or exports)
    Aids(ViewerAids), /// the drawer's height after a drag of its top edge (logical px)
    Height(f64), /// the waveform's state as the user set it (kept across launches): scale override, HDR top, open
    Waveform(Option<bool>, f32, bool), ResetToDefault, ApplyToAll, MakeDefault, SetOn(bool), SavePreset(String), RenamePreset(String, String), UpdatePreset(String), DeletePreset(String), Toggle }

/// What the drawer shows, fed by the shell every sync.
#[derive(Clone, Debug, PartialEq)]
pub struct View {
    /// "A · clip.mov", or "Project default" when nothing is loaded
    pub target: String,
    /// dual view: the side being edited (the target, row 212) — its letter leads the label in the side colour
    pub side: Option<qcv_player::Side>,
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
    /// the source is HDR (PQ / HLG): the waveform opens on its HDR scale
    pub hdr_source: bool,
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
    /// the top edge's resize grip (the rails' splitter, turned): hovered, and the drag's (pointer y, height) at the press
    split_hover: bool, split_drag: Option<(f64, f64)>,
    tab: Rect, tab_hover: bool, tab_ramp: Ramp, present: Ramp,
    presets: ListColumn, gamut: ListColumn, transfer: ListColumn, render: ListColumn,
    /// OCIO's columns (QCView: Config · Input · Look · View)
    ocfg: ListColumn, oinput: ListColumn, olook: ListColumn, oview: ListColumn, ocfg_keys: Vec<String>,
    preset_rows: Vec<PresetRow>,
    knee: KneePanel, agx: AgxPanel, /// the target the columns' open / collapsed state was last set for
    columns_for: String,
    on: Switch, on_rect: Rect,
    /// QCView's viewer-aids row: Exposure (±4 st), Gamma (0.25–4), the channel view, one reset
    /// the waveform card (canon row 203): the scale (None = follow the source), the HDR top, the trace's rect
    wf_scale: ActionGroup, wf_peak: TextButton, wf_hdr: Option<bool>, wf_peak_nits: f32, wf_card: Rect, wf_trace: Rect, wf_src_hdr: bool, wf_col: Rect, wf_collapse: Rect, wf_expanded: bool, wf_hover: bool,
    /// the row of columns' sideways scroll (px), its full width, the body it scrolls in
    hscroll: f64, content_w: f64, body: Rect,
    /// QCView's peak readout: the newest frame's (level, brightest channel) and the clip's peak since it loaded
    peaks: Option<([f32; 2], [f32; 2])>,
    /// dual view: each side's (now, clip max) — QCView's A / B lines (WaveformPanel.qml), here one caption segment per side
    peaks_dual: Option<[Option<([f32; 2], [f32; 2])>; 2]>,
    /// the readout's band under the card (QCView's readout sits below its waveform; the caption row had no room)
    wf_readout: Rect, /// the readout's tooltip (QCView: the brightest channel lives there, not inline)
    tips: min_ui::Tooltips,
    exp: Slider, gam: Slider, chan: ActionGroup, aids_reset: ActionButton, aids: ViewerAids, exp_rect: Rect, gam_rect: Rect, last_aid_press: Option<(u8, Instant)>,
    save_as: TextButton, reset: TextButton, apply_all: TextButton, make_default: TextButton,
    header: Rect,
    menu: Menu, menu_for: Option<String>,
    name_pop: Popover, name_field: TextField, naming: Option<Naming>,
    view: Option<View>,
    gamut_names: Vec<String>, transfer_names: Vec<String>, look_names: Vec<String>,
    scale: f64,
}

/// QCView's waveform HDR tops (nits).
const PEAKS: [i32; 5] = [300, 600, 1000, 2000, 4000];
const WAVEFORM_W: f64 = 360.0;
const COLLAPSED: f64 = 32.0;

impl ColorDrawer {
    /// `wf` = the waveform as last left: (scale override, HDR top in nits, open).
    pub fn new(open: bool, height: f64, wf: (Option<bool>, f32, bool)) -> ColorDrawer {
        let mut gamut_names = qcv_color::input_gamut_names(); gamut_names.truncate(15);
        let transfer_names = qcv_color::input_transfer_names();
        ColorDrawer {
            open, open_ramp: Ramp::new(if open { 1.0 } else { 0.0 }, t::RAIL_COLLAPSE_DURATION), height, rect: Rect::ZERO, split_hover: false, split_drag: None,
            tab: Rect::ZERO, tab_hover: false, tab_ramp: Ramp::new(0.0, t::ANIMATION_DURATION_HOVER), present: Ramp::new(0.0, t::ANIMATION_DURATION_SLIDE),
            presets: ListColumn::new(tr("Presets")), gamut: ListColumn::new(tr("Input gamut")), transfer: ListColumn::new(tr("Input transfer")), render: ListColumn::new(tr("Rendering")),
            ocfg: ListColumn::new(tr("Config")), oinput: ListColumn::new(tr("Input")).with_filter(), olook: ListColumn::new(tr("Look")), oview: ListColumn::new(tr("View")), ocfg_keys: vec![],
            preset_rows: vec![],
            knee: KneePanel::new(), agx: AgxPanel::new(), columns_for: String::new(),
            on: Switch::new(false), on_rect: Rect::ZERO,
            wf_scale: ActionGroup::new(["SDR", "HDR"].iter().map(|l| { let mut b = ActionButton::new("", l).size_s(); b.label = Some(l.to_string()); b }).collect()).compact(),
            wf_peak: TextButton::new("1000 nits", Variant::Secondary).size_s(),
            wf_hdr: wf.0, wf_peak_nits: if PEAKS.contains(&(wf.1 as i32)) { wf.1 } else { 1000.0 }, wf_card: Rect::ZERO, wf_trace: Rect::ZERO, wf_src_hdr: false, wf_col: Rect::ZERO, wf_collapse: Rect::ZERO, wf_expanded: wf.2, wf_hover: false, hscroll: 0.0, content_w: 0.0, body: Rect::ZERO, peaks: None, peaks_dual: None, wf_readout: Rect::ZERO, tips: min_ui::Tooltips::default(),
            exp: Slider::new(0.5), gam: Slider::new(((1.0f32 - 0.25) / 3.75).clamp(0.0, 1.0)),
            chan: ActionGroup::new(["RGB", "R", "G", "B", "A", "Y"].iter().enumerate().map(|(i, l)| { let mut b = ActionButton::new("", l).size_s(); b.label = Some((if i == 0 { tr("RGB") } else { l }).to_string()); b.selected = i == 0; b }).collect()).compact(),
            aids_reset: ActionButton::new("arrow-counter-clockwise", tr("Reset exposure, gamma and channel")).size_s(), aids: ViewerAids::default(), exp_rect: Rect::ZERO, gam_rect: Rect::ZERO, last_aid_press: None,
            save_as: TextButton::new(tr("Save as preset…"), Variant::Secondary).size_s(), reset: TextButton::new(tr("Reset to default"), Variant::Secondary).size_s(),
            apply_all: TextButton::new(tr("Apply to all clips"), Variant::Secondary).size_s(), make_default: TextButton::new(tr("Make project default"), Variant::Secondary).size_s(),
            header: Rect::ZERO, menu: Menu::new(), menu_for: None, name_pop: Popover::new(""), name_field: TextField::new(Family::Sans, t::RAIL_LABEL_SIZE), naming: None,
            view: None, gamut_names, transfer_names, look_names: qcv_color::look_names(), scale: 1.0,
        }
    }
    pub fn toggle(&mut self, now: Instant) { self.open = !self.open; self.open_ramp.set_target(if self.open { 1.0 } else { 0.0 }, now); }
    /// The drawer's height on screen now (px), for the layout that pushes the picture up.
    pub fn visible_height(&self, now: Instant, scale: f64) -> f64 { (self.height * scale * self.open_ramp.value(now) as f64).round() }
    pub fn animating(&self, now: Instant) -> bool { self.open_ramp.animating(now) || self.tab_ramp.animating(now) || self.present.animating(now) || [&self.presets, &self.gamut, &self.transfer, &self.render].iter().any(|l| l.animating(now)) || self.knee.animating(now) || self.agx.animating(now) || self.exp.animating(now) || self.gam.animating(now) || self.on.animating(now) || [&self.save_as, &self.reset, &self.apply_all, &self.make_default].iter().any(|b| b.animating(now)) }
    pub fn is_modal(&self) -> bool { self.menu.is_open() || self.naming.is_some() }
    pub fn has_focus(&self) -> bool { self.naming.is_some() || self.knee.focused() || self.agx.focused() || self.oinput.focused() }
    /// The top edge is under the pointer or being dragged (the shell shows the row-resize cursor).
    /// The pointer is on the floating tab (it keeps the tabs from fading while parked there).
    pub fn tab_hovered(&self) -> bool { self.tab_hover }
    pub fn splitter_hovered(&self) -> bool { self.split_hover || self.split_drag.is_some() }
    fn split_zone(&self) -> Rect { let z = t::SPLITTER_HOT_ZONE as f64 * self.scale; Rect::new(self.rect.x0, self.rect.y0 - z, self.rect.x1, self.rect.y0 + z) }
    fn dragging(&self) -> bool { self.split_drag.is_some() || self.knee.dragging() || self.agx.dragging() || self.exp.dragging() || self.gam.dragging() }
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
        self.wf_src_hdr = v.hdr_source;
        self.sync_wf_chips();
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
        // the viewer aids, right-aligned: Exposure [slider] 0.0 st · Gamma [slider] 1.00 · RGB R G B A Y · ↺
        let (rw, rh) = self.aids_reset.measure(fonts, scale);
        let mut ax = bottom.x1 - rw; self.aids_reset.rect = Rect::new(ax, bottom.y0 + (row - rh) / 2.0, ax + rw, bottom.y0 + (row + rh) / 2.0);
        let cw: f64 = self.chan.buttons.iter().map(|b| b.measure(fonts, scale).0).sum::<f64>() + t::ACTION_GROUP_GAP_COMPACT as f64 * scale * 5.0;
        ax -= cw + t::SPACING_200 as f64 * scale; let ch = self.chan.buttons[0].measure(fonts, scale).1; self.chan.layout(fonts, ax, bottom.y0 + (row - ch) / 2.0, scale);
        let sl = 140.0 * scale; let rd = 72.0 * scale; let lab = 64.0 * scale;
        ax -= rd + sl + lab + t::SPACING_300 as f64 * scale; self.gam_rect = Rect::new(ax, bottom.y0, ax + lab + sl + rd, bottom.y1); self.gam.rect = Rect::new(ax + lab, bottom.y0, ax + lab + sl, bottom.y1);
        ax -= rd + sl + lab + t::SPACING_400 as f64 * scale; self.exp_rect = Rect::new(ax, bottom.y0, ax + lab + sl + rd, bottom.y1); self.exp.rect = Rect::new(ax + lab, bottom.y0, ax + lab + sl, bottom.y1);
        let body = Rect::new(full.x0 + pad, self.header.y1, full.x1 - pad, bottom.y0 - t::SPACING_100 as f64 * scale);
        // Save as preset… under the presets list
        let (bw, bh) = self.save_as.measure(fonts, scale);
        // THE LAYOUT (the user 2026-11-08: "make the waveform flush right … OCIO has a full width panel, but minColor doesn't"):
        // the waveform is pinned to the right edge at WAVEFORM_W (or its 32-px strip); the colour controls fill everything to
        // its left in both engines — the list columns share the spare width by weight, the Knee and AgX keep their widths.
        // Only when the controls' minimums do not fit does their area scroll sideways (the waveform never moves).
        let ocio = self.view.as_ref().is_some_and(|v| v.stored.chain.is_ocio());
        let cap = t::COMPONENT_HEIGHT_75 as f64 * scale;
        let ww = if self.wf_expanded { WAVEFORM_W * scale } else { COLLAPSED * scale };
        self.wf_col = Rect::new(body.x1 - ww, body.y0, body.x1, body.y1);
        // the readout band: one line, two in dual (a line per side); none while collapsed
        let lines = if !self.wf_expanded { 0.0 } else if self.peaks_dual.is_some() { 2.0 } else { 1.0 };
        let band = if lines > 0.0 { lines * 16.0 * scale + t::SPACING_100 as f64 * scale } else { 0.0 };
        self.wf_card = Rect::new(body.x1 - ww, body.y0 + cap, body.x1, body.y1 - band);
        self.wf_readout = Rect::new(body.x1 - ww, body.y1 - band, body.x1, body.y1);
        let area = Rect::new(body.x0, body.y0, (body.x1 - ww - gap).max(body.x0), body.y1);
        self.body = area;
        // (min width, weight): lists stretch, the control columns do not
        enum Col { Presets, List(u8), Knee, Agx }
        let cols: Vec<(Col, f64, f64)> = if ocio {
            vec![(Col::Presets, 180.0, 1.0), (Col::List(0), 110.0, 0.5), (Col::List(1), 190.0, 1.2), (Col::List(2), 180.0, 1.0), (Col::List(3), 200.0, 1.6), (Col::Knee, self.knee.width(), 0.0)]
        } else {
            vec![(Col::Presets, 200.0, 1.0), (Col::List(4), 160.0, 1.0), (Col::List(5), 170.0, 1.0), (Col::Knee, self.knee.width(), 0.0), (Col::Agx, self.agx.width(), 0.0)]
        };
        let gaps = gap * (cols.len() as f64 - 1.0);
        let min_sum: f64 = cols.iter().map(|c| c.1 * scale).sum::<f64>() + gaps;
        let weight: f64 = cols.iter().map(|c| c.2).sum();
        let extra = (area.width() - min_sum).max(0.0);
        self.content_w = min_sum + extra;
        let max = (self.content_w - area.width()).max(0.0);
        if self.hscroll > max { self.hscroll = max; }
        let mut x = area.x0 - self.hscroll;
        for (c, min, w) in cols {
            let cw = min * scale + if weight > 0.0 { extra * w / weight } else { 0.0 };
            match c {
                Col::Presets => { self.presets.layout(Rect::new(x, body.y0, x + cw, body.y1 - bh - t::SPACING_100 as f64 * scale), scale); self.save_as.rect = Rect::new(x, body.y1 - bh, x + bw, body.y1); }
                Col::List(k) => { let r = Rect::new(x, body.y0, x + cw, body.y1); match k { 0 => self.ocfg.layout(r, scale), 1 => self.oinput.layout_with(fonts, r, scale), 2 => self.olook.layout(r, scale), 3 => self.oview.layout(r, scale), 4 => self.gamut.layout(r, scale), _ => self.transfer.layout(r, scale) } }
                Col::Knee => { self.knee.layout(fonts, x, body.y0, body.y1, scale); }
                Col::Agx => { self.agx.layout(fonts, x, body.y0, body.y1, scale); }
            }
            x += cw + gap;
        }
        // the waveform's caption row: the scale chips + the HDR top, right-aligned before its caret
        let b = t::ICON_SIZE_SMALL as f64 * scale + 4.0 * scale;
        self.wf_collapse = Rect::new(body.x1 - b, body.y0 + (cap - b) / 2.0, body.x1, body.y0 + (cap + b) / 2.0);
        let ch = self.wf_scale.buttons[0].measure(fonts, scale).1;
        let mut cx = self.wf_collapse.x0 - t::SPACING_100 as f64 * scale;
        if self.wf_expanded && self.scope_hdr() { let (w, bh) = self.wf_peak.measure(fonts, scale); cx -= w; self.wf_peak.rect = Rect::new(cx, body.y0 + (cap - bh) / 2.0, cx + w, body.y0 + (cap + bh) / 2.0); cx -= t::SPACING_100 as f64 * scale; } else { self.wf_peak.rect = Rect::ZERO; }
        if self.wf_expanded { let w: f64 = self.wf_scale.buttons.iter().map(|b| b.measure(fonts, scale).0).sum::<f64>() + t::ACTION_GROUP_GAP_COMPACT as f64 * scale; cx -= w; self.wf_scale.layout(fonts, cx, body.y0 + (cap - ch) / 2.0, scale); } else { self.wf_scale.layout(fonts, -1e6, -1e6, scale); }
        let p = t::SPACING_100 as f64 * scale; let label_w = 34.0 * scale;
        self.wf_trace = if self.wf_expanded { Rect::new(self.wf_card.x0 + p + label_w, self.wf_card.y0 + p, self.wf_card.x1 - p, self.wf_card.y1 - p) } else { Rect::ZERO };
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
    /// The readout's numbers (now, the clip's peak); None clears it.
    pub fn set_peaks(&mut self, p: Option<([f32; 2], [f32; 2])>) -> bool { if self.peaks == p { false } else { self.peaks = p; true } }
    pub fn set_peaks_dual(&mut self, p: Option<[Option<([f32; 2], [f32; 2])>; 2]>) -> bool { if self.peaks_dual == p { false } else { self.peaks_dual = p; true } }
    fn wf_cmd(&self) -> Cmd { Cmd::Waveform(self.wf_hdr, self.wf_peak_nits, self.wf_expanded) }
    fn scope_hdr(&self) -> bool { self.wf_hdr.unwrap_or(self.wf_src_hdr) }
    fn sync_wf_chips(&mut self) {
        let hdr = self.scope_hdr();
        for (i, b) in self.wf_scale.buttons.iter_mut().enumerate() { b.selected = (i == 1) == hdr; }
        self.wf_peak.label = format!("{} nits", self.wf_peak_nits as i32);
    }
    /// The waveform to draw (window px, the scale, the HDR top) — None while the drawer or the card is hidden.
    pub fn scope(&self) -> Option<min_present::ScopeView> {
        if !self.open || !self.wf_expanded || self.rect.height() < self.height * self.scale * 0.98 || self.wf_trace.width() < 40.0 { return None }
        let r = self.wf_trace;
        Some(min_present::ScopeView { rect: [r.x0 as f32, r.y0 as f32, r.width() as f32, r.height() as f32], hdr: self.scope_hdr(), peak_nits: self.wf_peak_nits, brightness: 1.0 })
    }
    fn set_aids(&mut self, a: ViewerAids) {
        self.aids = a;
        if !self.exp.dragging() { self.exp.value = ((a.exposure + 4.0) / 8.0).clamp(0.0, 1.0); }
        if !self.gam.dragging() { self.gam.value = ((a.gamma - 0.25) / 3.75).clamp(0.0, 1.0); }
        for (i, b) in self.chan.buttons.iter_mut().enumerate() { b.selected = i as u8 == a.channel; }
        self.aids_reset.enabled = !a.is_identity();
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
        // the readout's tooltip: QCView `ScopeController` — the brightest channel, this frame and the clip's max, for the row under the pointer
        let tip = e.pos().filter(|_| !matches!(e, Event::Leave) && self.naming.is_none() && !self.menu.is_open()).and_then(|(x, y)| {
            if !self.wf_readout.contains(kurbo::Point::new(x, y)) { return None }
            let hdr = self.scope_hdr();
            let row = ((y - self.wf_readout.y0 - t::SPACING_100 as f64 * self.scale) / (16.0 * self.scale)).floor().max(0.0) as usize;
            let (now_v, peak) = match (self.peaks_dual, self.peaks) { (Some(sides), _) => sides.iter().flatten().nth(row).copied()?, (None, Some(p)) => p, _ => return None };
            let txt = if hdr { fill(trs("Brightest channel (linear Rec.2020): frame %1 · max %2 nits"), &[&format_peak(now_v[1], true), &format_peak(peak[1], true)]) }
                      else { fill(trs("Brightest channel (code value): frame %1 · max %2"), &[&format_peak(now_v[1], false), &format_peak(peak[1], false)]) };
            Some((7000 + row as u64, Rect::new(self.wf_readout.x0, self.wf_readout.y0, self.wf_readout.x1, self.wf_readout.y1), txt))
        });
        if self.tips.hover(tip.as_ref().map(|(k, r, s)| (*k, *r, s.as_str())), now) { dirty = true; }
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
        if let Event::Down { button: Button::Primary, .. } = e { if over_tab && self.present.value(now) > 0.5 { cmds.push(Cmd::Toggle); return (cmds, true, true) } }
        if self.rect.height() <= 1.0 { return (cmds, dirty, false) }
        // the top edge: drag to resize (200…600 px, kept in the store on release)
        if let Some((y0, h0)) = self.split_drag {
            match *e {
                Event::Move { y, .. } => { self.height = (h0 + (y0 - y) / self.scale).clamp(200.0, 600.0).round(); return (cmds, true, true) }
                Event::Up { .. } | Event::Leave => { self.split_drag = None; cmds.push(Cmd::Height(self.height)); return (cmds, true, true) }
                _ => return (cmds, false, true),
            }
        }
        let on_edge = self.open && e.pos().is_some_and(|(x, y)| self.split_zone().contains(kurbo::Point::new(x, y)));
        if on_edge != self.split_hover { self.split_hover = on_edge; dirty = true; }
        if on_edge { if let Event::Down { y, button: Button::Primary, .. } = *e { self.split_drag = Some((y, self.height)); return (cmds, true, true) } }
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
            let (c, d) = self.knee.event(e, now, fonts); dirty |= d; if let Some(c) = c { cmds.extend(self.with_chain(c)); }
            let (c, d) = self.agx.event(e, now, fonts); dirty |= d; if let Some(c) = c { cmds.extend(self.with_chain(c)); }
        }
        let (k, d) = self.on.event(e, now); dirty |= d; if let Some(on) = k { cmds.push(Cmd::SetOn(on)); }
        if let Some((px, py)) = e.pos() {
            let over = if self.wf_expanded { self.wf_collapse.contains(kurbo::Point::new(px, py)) } else { self.wf_card.contains(kurbo::Point::new(px, py)) };
            if over != self.wf_hover { self.wf_hover = over; dirty = true; }
            if over && matches!(e, Event::Down { button: Button::Primary, .. }) { self.wf_expanded = !self.wf_expanded; cmds.push(self.wf_cmd()); return (cmds, true, true) }
        }
        for (i, rsp) in self.wf_scale.event(e, now) { dirty = true; if rsp == Response::Clicked { self.wf_hdr = Some(i == 1); self.sync_wf_chips(); cmds.push(self.wf_cmd()); } }
        if self.scope_hdr() { let (c, d) = self.wf_peak.event(e, now); dirty |= d; if c { let i = PEAKS.iter().position(|p| *p as f32 == self.wf_peak_nits).map(|i| (i + 1) % PEAKS.len()).unwrap_or(2); self.wf_peak_nits = PEAKS[i] as f32; self.sync_wf_chips(); cmds.push(self.wf_cmd()); } }   // QCView's tops, cycled
        // the viewer aids: a double-click on a slider resets it (QCView)
        let mut a = self.aids;
        if let Event::Down { x, y, button: Button::Primary } = *e { for (id, sl) in [(0u8, &self.exp), (1u8, &self.gam)] { if sl.hit(x, y) {
            if self.last_aid_press.is_some_and(|(j, t0)| j == id && now.duration_since(t0).as_millis() < 400) { if id == 0 { a.exposure = 0.0 } else { a.gamma = 1.0 } self.last_aid_press = None; }
            else { self.last_aid_press = Some((id, now)); }
        } } }
        let (v, d) = self.exp.event(e, now, self.scale); dirty |= d; if let Some(v) = v { a.exposure = ((v * 8.0 - 4.0) * 10.0).round() / 10.0; }
        let (v, d) = self.gam.event(e, now, self.scale); dirty |= d; if let Some(v) = v { a.gamma = ((0.25 + v * 3.75) / 0.05).round() * 0.05; }
        for (i, rsp) in self.chan.event(e, now) { dirty = true; if rsp == Response::Clicked { a.channel = i as u8; } }
        if self.aids_reset.event(e, now) == Response::Clicked { a = ViewerAids::default(); }
        if a != self.aids { self.set_aids(a); cmds.push(Cmd::Aids(a)); }
        let (c, d) = self.save_as.event(e, now); dirty |= d; if c { self.open_naming(Naming::Save, fonts, bounds); dirty = true; }
        let (c, d) = self.reset.event(e, now); dirty |= d; if c { cmds.push(Cmd::ResetToDefault); }
        let (c, d) = self.apply_all.event(e, now); dirty |= d; if c { cmds.push(Cmd::ApplyToAll); }
        let (c, d) = self.make_default.event(e, now); dirty |= d; if c { cmds.push(Cmd::MakeDefault); }
        let d = dirty || !cmds.is_empty();
        (cmds, d, inside)
    }
    /// A sideways swipe / Shift + wheel scrolls the row of columns when it is wider than the drawer.
    pub fn hwheel(&mut self, x: f64, y: f64, dx_px: f64) -> bool {
        if !self.rect.contains(kurbo::Point::new(x, y)) { return false }
        let max = (self.content_w - self.body.width()).max(0.0);
        let h = (self.hscroll + dx_px).clamp(0.0, max);
        if (h - self.hscroll).abs() < 0.5 { return false }
        self.hscroll = h; true
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
        let p = self.present.value(now); if p <= 0.001 { return }
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
        // the top edge: the divider, brighter as the resize grip under the pointer
        let edge = if self.splitter_hovered() { t::RAIL_SPLITTER_COLOR_HOVER } else { t::DIVIDER_COLOR };
        paint::rect(scene, Rect::new(self.rect.x0, self.rect.y0, self.rect.x1, self.rect.y0 + if self.splitter_hovered() { 2.0 * s } else { s.max(1.0) }), edge);
        scene.push_clip_layer(peniko::Fill::NonZero, Affine::IDENTITY, &self.rect);
        let Some(v) = self.view.clone() else { scene.pop_layer(); return };
        // header: COLOR · the target · its status
        fn mid<B: parley::Brush>(l: &parley::Layout<B>, r: Rect) -> f64 { r.y0 + (r.height() - l.height() as f64) / 2.0 }
        let h = fonts.detail(tr("Color"), t::RAIL_GROUP_HEADER_SIZE * s as f32, paint::color(t::RAIL_GROUP_HEADER_COLOR));
        let mut x = self.header.x0; min_ui::fonts::draw_layout(scene, &h, Affine::translate((x, mid(&h, self.header)))); x += h.width() as f64 + t::SPACING_200 as f64 * s;
        if let Some(side) = v.side { let a = side == qcv_player::Side::A; let l = fonts.layout(if a { "A" } else { "B" }, Family::Sans, t::RAIL_LABEL_SIZE * s as f32, t::FONT_WEIGHT_BOLD as f32, paint::color(if a { t::SIDE_A_COLOR } else { t::SIDE_B_COLOR })); min_ui::fonts::draw_layout(scene, &l, Affine::translate((x, mid(&l, self.header)))); x += l.width() as f64 + t::SPACING_100 as f64 * s; }
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
        scene.push_clip_layer(peniko::Fill::NonZero, Affine::IDENTITY, &Rect::new(self.body.x0, self.body.y0, self.body.x1, self.body.y1));
        self.presets.paint(scene, fonts, now);
        self.save_as.paint(scene, fonts, s, now);
        match &v.stored.chain {
            ClipChain::MinColor(_) => {
                self.gamut.paint(scene, fonts, now); self.transfer.paint(scene, fonts, now);
                self.knee.paint(scene, fonts, now); self.agx.paint(scene, fonts, now);
            }
            ClipChain::Ocio(_) => {
                if v.ocio.is_some() { for l in [&mut self.ocfg, &mut self.oinput, &mut self.olook, &mut self.oview] { l.paint(scene, fonts, now); } self.knee.paint(scene, fonts, now); }
                else { let l = fonts.layout(tr("Loading the OCIO config…"), Family::Sans, t::RAIL_LABEL_SIZE * s as f32, t::FONT_WEIGHT_REGULAR as f32, paint::color(t::RAIL_GROUP_HEADER_COLOR)); min_ui::fonts::draw_layout(scene, &l, Affine::translate((self.save_as.rect.x1 + 40.0 * s, self.header.y1 + 40.0 * s))); }
            }
        }
        // the waveform card: caption + scale chips; the graticule under the presenter's trace
        scene.pop_layer();
        if !self.wf_expanded {
            let bg = if self.wf_hover { t::HOVER_BACKGROUND_COLOR } else { t::BACKGROUND_LAYER_2_COLOR };
            paint::rounded(scene, self.wf_card, t::CORNER_RADIUS_500 as f64 * s, bg);
            let l = fonts.layout(tr("Waveform"), Family::Sans, t::RAIL_LABEL_SIZE * s as f32, t::FONT_WEIGHT_BOLD as f32, paint::color(t::RAIL_GROUP_HEADER_COLOR));
            let cx = self.wf_card.x0 + (self.wf_card.width() - l.height() as f64) / 2.0; let top = self.wf_card.y0 + t::SPACING_200 as f64 * s;
            min_ui::fonts::draw_layout(scene, &l, Affine::translate((cx, top + l.width() as f64)) * Affine::rotate(-std::f64::consts::FRAC_PI_2));
        }
        if self.wf_expanded && self.wf_card.width() > 40.0 {
            if self.wf_hover { paint::rounded(scene, self.wf_collapse, t::CORNER_RADIUS_SMALL_DEFAULT as f64 * s, t::HOVER_BACKGROUND_COLOR); }
            if let Some(g) = fonts.icon("caret-left", t::ICON_SIZE_SMALL * s as f32, paint::color(if self.wf_hover { t::NEUTRAL_CONTENT_COLOR_HOVER } else { t::RAIL_OPEN_TAB_COLOR })) { min_ui::fonts::draw_layout(scene, &g, Affine::translate((self.wf_collapse.x0 + (self.wf_collapse.width() - g.width() as f64) / 2.0, self.wf_collapse.y0 + (self.wf_collapse.height() - g.height() as f64) / 2.0))); }
            let capr = Rect::new(self.wf_card.x0, self.wf_card.y0 - t::COMPONENT_HEIGHT_75 as f64 * s, self.wf_card.x1, self.wf_card.y0);
            let l = fonts.detail(tr("Waveform"), t::RAIL_GROUP_HEADER_SIZE * s as f32, paint::color(t::RAIL_GROUP_HEADER_COLOR));
            min_ui::fonts::draw_layout(scene, &l, Affine::translate((capr.x0 + t::SPACING_50 as f64 * s, mid(&l, capr))));
            // QCView's readout: now · the clip's peak · the brightest channel (nits on HDR, % of Y′ on SDR)
            // QCView's readout, under the card: now · the clip's peak (· the brightest channel in single), nits on HDR / % of Y′ on SDR;
            // dual: a line per side led by its letter in the side colour (WaveformPanel.qml's A / B lines)
            let hdr = self.scope_hdr();
            let f = |v: f32| format_peak(v, hdr);
            let unit = if hdr { " nits" } else { "" };
            let rows: Vec<(Option<(&str, min_theme::Color)>, String, bool)> = match (self.peaks_dual, self.peaks) {
                (Some(sides), _) => sides.iter().enumerate().filter_map(|(i, sp)| sp.map(|(now_v, peak)| (Some(if i == 0 { ("A ", t::SIDE_A_COLOR) } else { ("B ", t::SIDE_B_COLOR) }), format!("{} · {} {}{unit}", f(now_v[0]), tr("peak"), f(peak[0])), if hdr { peak[0] > self.wf_peak_nits } else { peak[0] > 1.0 }))).collect(),
                (None, Some((now_v, peak))) => vec![(None, format!("{} · {} {}{unit}", f(now_v[0]), tr("peak"), f(peak[0])), if hdr { peak[0] > self.wf_peak_nits } else { peak[0] > 1.0 })],
                _ => vec![],
            };
            let rr = self.wf_readout; let lh = 16.0 * s;
            for (k, (lead, txt, over)) in rows.iter().enumerate() {
                let y = rr.y0 + t::SPACING_100 as f64 * s + k as f64 * lh;
                let mut x = rr.x0 + t::SPACING_50 as f64 * s;
                if let Some((ltxt, lc)) = lead { let ll = fonts.layout(ltxt, Family::Mono, t::CODE_SIZE_S * s as f32, t::FONT_WEIGHT_BOLD as f32, paint::color(*lc)); min_ui::fonts::draw_layout(scene, &ll, Affine::translate((x, y + (lh - ll.height() as f64) / 2.0))); x += ll.width() as f64; }
                let rl = fonts.layout(txt, Family::Mono, t::CODE_SIZE_S * s as f32, t::FONT_WEIGHT_REGULAR as f32, paint::color(if *over { t::NOTICE_VISUAL_COLOR } else { t::RAIL_LABEL_COLOR }));
                min_ui::fonts::draw_layout(scene, &rl, Affine::translate((x, y + (lh - rl.height() as f64) / 2.0)));
            }
            self.wf_scale.paint(scene, fonts, s, now); if self.scope_hdr() { self.wf_peak.paint(scene, fonts, s, now); }
            paint::rounded(scene, self.wf_card, t::CORNER_RADIUS_500 as f64 * s, t::GRAY_25);
            let tr_ = self.wf_trace;
            let lines: Vec<(f64, String)> = if self.scope_hdr() {
                let top = self.wf_peak_nits as f64;
                [0.0, 100.0, 203.0, top / 2.0, top].iter().filter(|n| **n <= top).map(|n| (1.0 - n / top, format!("{}", n.round() as i64))).collect()
            } else { [0.0, 25.0, 50.0, 75.0, 100.0].iter().map(|p| ((1.1 - p / 100.0) / 1.2, format!("{}", *p as i64))).collect() };
            for (f, label) in lines {
                let y = tr_.y0 + f * tr_.height();
                paint::rect(scene, Rect::new(tr_.x0, y - 0.5 * s, tr_.x1, y + 0.5 * s), t::GRAY_200);
                let ll = fonts.layout(&label, Family::Mono, (t::CODE_SIZE_S - 1.0) * s as f32, t::FONT_WEIGHT_REGULAR as f32, paint::color(t::DISABLED_CONTENT_COLOR));
                min_ui::fonts::draw_layout(scene, &ll, Affine::translate((tr_.x0 - ll.width() as f64 - t::SPACING_75 as f64 * s, y - ll.height() as f64 / 2.0)));
            }
            let unit = fonts.layout(if self.scope_hdr() { "nits" } else { "%" }, Family::Sans, t::DETAIL_SIZE_S * s as f32, t::FONT_WEIGHT_REGULAR as f32, paint::color(t::DISABLED_CONTENT_COLOR));
            min_ui::fonts::draw_layout(scene, &unit, Affine::translate((self.wf_card.x0 + t::SPACING_75 as f64 * s, self.wf_card.y1 - unit.height() as f64 - t::SPACING_75 as f64 * s)));
        }
        // the row's sideways position, when it overflows: a thin pill under the columns
        let max = (self.content_w - self.body.width()).max(0.0);
        if max > 0.5 {
            let th = t::SCROLLBAR_THICKNESS as f64 * s; let w = self.body.width() * self.body.width() / self.content_w; let x0 = self.body.x0 + (self.body.width() - w) * self.hscroll / max;
            paint::rounded(scene, Rect::new(x0, self.body.y1 + 2.0 * s, x0 + w, self.body.y1 + 2.0 * s + th), th / 2.0, t::SCROLLBAR_COLOR);
        }
        // bottom row: the one On / Off switch
        self.on.paint(scene, s, now);
        let ol = fonts.layout(if v.on { tr("Color On") } else { tr("Color Off") }, Family::Sans, t::RAIL_LABEL_SIZE * s as f32, t::FONT_WEIGHT_REGULAR as f32, paint::color(if v.on { t::RAIL_LABEL_COLOR } else { t::RAIL_GROUP_HEADER_COLOR }));
        min_ui::fonts::draw_layout(scene, &ol, Affine::translate((self.on.rect.x1 + t::SPACING_100 as f64 * s, mid(&ol, self.on_rect))));
        for (r, sl, name, val) in [(self.exp_rect, &self.exp, tr("Exposure"), format!("{:+.1} st", self.aids.exposure)), (self.gam_rect, &self.gam, tr("Gamma"), format!("{:.2}", self.aids.gamma))] {
            let l = fonts.layout(name, Family::Sans, t::RAIL_LABEL_SIZE * s as f32, t::FONT_WEIGHT_REGULAR as f32, paint::color(t::RAIL_GROUP_HEADER_COLOR));
            min_ui::fonts::draw_layout(scene, &l, Affine::translate((r.x0, mid(&l, r))));
            sl.paint(scene, s, now);
            let vl = fonts.layout(&val, Family::Mono, t::CODE_SIZE_S * s as f32, t::FONT_WEIGHT_REGULAR as f32, paint::color(t::RAIL_LABEL_COLOR));
            min_ui::fonts::draw_layout(scene, &vl, Affine::translate((r.x1 - vl.width() as f64, mid(&vl, r))));
        }
        self.chan.paint(scene, fonts, s, now);
        if !self.aids.is_identity() { self.aids_reset.paint(scene, fonts, s, now); }
        scene.pop_layer();
    }
    /// The menu and the naming popover, over everything.
    pub fn tick(&mut self, now: Instant) -> bool { self.tips.tick(now) }
    pub fn paint_overlays(&mut self, scene: &mut Scene, fonts: &mut Fonts, bounds: Rect, now: Instant) {
        self.tips.paint(scene, fonts, self.scale, bounds, now);
        if self.menu.is_open() { self.menu.paint(scene, fonts, self.scale); }
        if self.naming.is_some() { self.name_pop.paint(scene, fonts, self.scale); self.name_field.paint(scene, fonts, self.scale); }
    }
}

/// QCView `formatPeak`: nits with one decimal under 10, whole above; SDR as a percentage of Y′ / the code value.
fn format_peak(v: f32, hdr: bool) -> String { if hdr { if v < 10.0 { format!("{v:.1}") } else { format!("{}", v.round() as i64) } } else { format!("{} %", (v * 100.0).round() as i64) } }
/// QCView's `ScopeController` catalog context (its readout tooltip).
fn trs(s: &str) -> &str { min_i18n::trc("ScopeController", s) }
