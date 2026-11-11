//! The Settings section of the left rail (the 2026-11-07 rails ruling: QCView's `LeftRail.qml` Settings section carried
//! row for row, with its own GroupHeaders — Performance / Interface / Audio — as sub-headers; "Updates" is macOS-only,
//! as in QCView: Sparkle's switch). Rows whose feature has not landed here are drawn
//! disabled; the shape of the panel is final. A row = a label (wrapping to two lines, its help as a tooltip — QCView's
//! `RowLabel`) and a control at the right: Switch, NumberField or Picker. "Reset all preferences…" asks first (QCView's
//! confirm) and then resets the store; the owner applies the result.
use crate::settings::k;
use kurbo::Rect;
use min_settings::Settings;
use min_theme::tokens as t;
use min_ui::{paint, Dialog, Event, Family, Fonts, KeyEvent, NumberField, Picker, Section, Switch, TextButton, Tooltips, Variant};
use std::time::Instant;
use vello::Scene;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cmd { Changed(&'static str), ResetAll }

enum Ctl { Switch(Switch), Number(NumberField), Picker(Picker, &'static [&'static str]) }

struct Row { key: &'static str, label: &'static str, help: &'static str, backed: bool, windows_only: bool, ctl: Ctl, rect: Rect, label_rect: Rect, label_hover: bool }
struct Group { title: &'static str, rows: Vec<Row>, header: Rect }

pub struct SettingsPanel {
    pub section: Section,
    groups: Vec<Group>,
    reset: TextButton,
    dialog: Dialog,
    pub tooltips: Tooltips,
    seen: u64, /// the Blender switch's status lines (QCView LeftRail: the state, green when live; the card's error rows in red) + their rects
    blender_lines: Vec<(String, u8)>, blender_rects: Vec<Rect>,
    height: f64,
    scale: f64,
}

/// QCView's control column (Layout.preferredWidth: 110), logical px.
const CONTROL_W: f64 = 110.0;

/// QCView's catalog context for the left rail (canon row 195).
fn tr(s: &'static str) -> &'static str { min_i18n::trc("LeftRail", s) }

/// The Language picker's rows (`crate::i18n::LANGUAGES`): native names, never translated — only "System default" is.
const LANG_CODES: &[&str] = &["", "en", "de", "es", "fr", "it", "ja", "ko", "pl", "pt_BR", "ru", "uk", "zh_CN"];
const LANG_NAMES: &[&str] = &["System default", "English", "Deutsch", "Español", "Français", "Italiano", "日本語", "한국어", "Polski", "Português (Brasil)", "Русский", "Українська", "简体中文 (中国)"];

impl SettingsPanel {
    /// The Blender input's status lines (0 muted, 1 live green, 2 error red); true when they changed.
    pub fn set_blender_lines(&mut self, v: Vec<(String, u8)>) -> bool { if v == self.blender_lines { return false } self.blender_lines = v; true }
    /// The A/V sync (Live) row edits the connected host's own offset (QCView: the live decoder's host, premiere when none).
    pub fn set_live_sync_key(&mut self, key: &'static str) {
        for r in self.groups.iter_mut().flat_map(|g| g.rows.iter_mut()) { if r.key.starts_with("audio/liveSyncOffsetMs/") && r.key != key { r.key = key; self.seen = u64::MAX; } }
    }
    /// A language switch (canon row 197): the labels built at construction, rebuilt; the state stays.
    /// Every row's text is the constructor's and the values come from the store each frame: a fresh panel, the section's state kept.
    pub fn relabel(&mut self) { let expanded = self.section.expanded; *self = Self::new(); self.section.expanded = expanded; }
    pub fn new() -> SettingsPanel {
        let sw = |key, label, help, backed| Row { key, label: tr(label), help: tr(help), backed, windows_only: false, ctl: Ctl::Switch(Switch::new(false)), rect: Rect::ZERO, label_rect: Rect::ZERO, label_hover: false };
        let num = |key, label, help, backed, min, max, step| Row { key, label: tr(label), help: tr(help), backed, windows_only: false, ctl: Ctl::Number(NumberField::new(0, min, max, step)), rect: Rect::ZERO, label_rect: Rect::ZERO, label_hover: false };
        let pick = |key, label, help, backed, opts: &'static [&'static str], values: &'static [&'static str]| Row { key, label: tr(label), help: tr(help), backed, windows_only: false, ctl: Ctl::Picker(Picker::new(opts.iter().map(|s| if LANG_NAMES[1..].contains(s) || ["PNG", "PNG 16-bit", "JPEG", "TIFF 16-bit"].contains(s) { s.to_string() } else { tr(s).to_string() }).collect(), 0), values), rect: Rect::ZERO, label_rect: Rect::ZERO, label_hover: false };
        let mut threads = num(k::FFMPEG_THREADS, "Video decode threads", "0 = Auto. Single-video playback only; applies on the next open.", true, 0, 32, 1);
        if let Ctl::Number(n) = &mut threads.ctl { let z = std::mem::replace(n, NumberField::new(0, 0, 32, 1)); *n = z.zero_label(tr("Auto")); }
        let mut hw = sw(k::HW_DECODE, "Enable hardware decoding", "GPU decode (D3D12VA) when the format allows; applies on the next open.", true);
        hw.windows_only = true;   // QCView: `visible: Qt.platform.os === "windows"` (VideoToolbox is always on where it applies)
        #[cfg(target_os = "macos")] let updater = crate::sparkle::bundled();
        #[cfg(not(target_os = "macos"))] let updater = false;
        let _ = updater;
        let mut groups = vec![
            Group { title: tr("Performance"), header: Rect::ZERO, rows: vec![
                num(k::IMAGE_SEQ_THREADS, "Image-seq decode threads", "Applies on the next sequence load.", true, 1, 64, 1),
                num(k::IMAGE_SEQ_READ_AHEAD, "Image-seq cache window", "Frames cached ahead of the playhead for image sequences.", true, 24, 240, 6),
                threads,
                hw,
                num(k::SCRUB_CACHE_MB, "Scrub cache size (MB)", "Decoded frames kept near the playhead while scrubbing; applies on the next open.", true, 128, 4096, 128),
                sw(k::HW_SCRUB_INTER, "Hardware scrub for h264/h265", "GPU decode while scrubbing h264/h265; applies on the next open.", true),
            ] },
            Group { title: tr("Interface"), header: Rect::ZERO, rows: vec![
                pick(k::LANGUAGE, "Language", "Language of the interface. System default follows the OS setting. Applies at once.", true, LANG_NAMES, LANG_CODES),
                sw(k::TIMELINE_HOVER_THUMBS, "Timeline hover thumbnails", "Show a frame thumbnail while the pointer rests on the timeline.", true),
                sw(k::DRAG_VIEWPORT_MOVES_WINDOW, "Drag viewport to move window", "Dragging the picture moves the window.", true),
                sw(k::TIMELINE_WAVEFORMS, "Timeline audio waveforms", "Draw the audio waveform in the scrubber. Peaks decode lazily in the background.", true),
                pick(k::SCREENSHOT_FORMAT, "Screenshot format", "Format for Desktop screenshots. JPEG is smaller, PNG and TIFF are lossless.", true, &["PNG", "PNG 16-bit", "JPEG", "TIFF 16-bit"], &["png", "png16", "jpeg", "tiff"]),
                sw(k::SCREENSHOT_GUIDES, "Include safety guides in screenshots", "Draw the active safety guide into Desktop and clipboard screenshots.", true),
            ] },
            Group { title: tr("Audio"), header: Rect::ZERO, rows: vec![
                num(k::SYNC_OFFSET_MS, "A/V sync (Single)", "Positive values delay audio to offset pipeline/display lag. Tune by ear; applies on the next seek or play.", true, -100, 100, 1),
                num(k::DUAL_SYNC_OFFSET_MS, "A/V sync (Dual)", "Positive values delay audio to offset pipeline/display lag. Dual's latency runs deeper than single. Tune by ear; applies on the next seek or play.", true, -100, 100, 1),
                sw(k::SCRUB_MUTED, "Mute scrub audio", "Silence the audio feedback while scrubbing and shuttling.", true),
                sw(k::BLENDER_SESSIONS, "Blender sessions", "Off at every launch. On (also File ▸ Connect to Blender): QCView watches for the card the QCBridge host add-on on this machine writes when a session starts, listens on its port and shows the replica's stream. Off again stops listening.", true),
                num(k::LIVE_SYNC_OFFSET_MS, "A/V sync (Live)", "Positive values delay the bridge audio; negative values hold the picture. Kept per host application. Tune by ear.", true, -200, 200, 1),
                num(k::LIVE_BUFFER_MS, "Live audio buffer", "How much bridge audio is queued before it plays; in milliseconds.", true, 20, 200, 10),
            ] },
        ];
        // QCView: `visible: Qt.platform.os === "osx"` — Sparkle is macOS's updater (Windows updates through the Store). Before Audio.
        if cfg!(target_os = "macos") {
            groups.insert(2, Group { title: tr("Updates"), header: Rect::ZERO, rows: vec![
                sw(k::AUTO_UPDATE_CHECKS, "Automatically check for updates", "Checks daily for updates, sending only your IP and app/OS version. Update manually anytime from the About menu.", updater),
            ] });
        }
        SettingsPanel {
            section: Section::new(tr("Settings"), true), groups, reset: TextButton::new(tr("Reset all preferences…"), Variant::Secondary).size_s(),
            dialog: Dialog::new(tr("Reset all preferences?"), tr("Every setting goes back to its default, including the window's size and position and the rails' layout. Media stays open."), vec![(tr("Cancel"), Variant::Secondary), (tr("Reset"), Variant::Negative)]),
            tooltips: Tooltips::default(), seen: u64::MAX, blender_lines: vec![], blender_rects: vec![], height: 0.0, scale: 1.0,
        }
    }
    fn rows_mut(&mut self) -> impl Iterator<Item = &mut Row> { self.groups.iter_mut().flat_map(|g| g.rows.iter_mut()).filter(|r| !r.windows_only || cfg!(windows)) }
    fn rows(&self) -> impl Iterator<Item = &Row> { self.groups.iter().flat_map(|g| g.rows.iter()).filter(|r| !r.windows_only || cfg!(windows)) }
    /// Mirror the store into the controls when it changed (another path wrote it: a key, a reset).
    pub fn sync(&mut self, s: &Settings, now: Instant) {
        if s.generation() == self.seen { return }
        self.seen = s.generation();
        for r in self.rows_mut() {
            match &mut r.ctl {
                Ctl::Switch(sw) => { sw.set(s.bool(r.key), now); sw.enabled = r.backed; }
                Ctl::Number(n) => { n.set(s.int(r.key)); n.enabled = r.backed; }
                Ctl::Picker(p, values) => { let v = s.string(r.key); if let Some(i) = values.iter().position(|x| *x == v) { p.selected = i; } p.enabled = r.backed; }
            }
        }
    }
    pub fn is_modal(&self) -> bool { self.dialog.open }
    pub fn has_focus(&self) -> bool { self.rows().any(|r| matches!(&r.ctl, Ctl::Number(n) if n.focused())) }
    pub fn menu_open(&self) -> bool { self.rows().any(|r| matches!(&r.ctl, Ctl::Picker(p, _) if p.is_open())) }
    /// Lay the section out at `(x0..x1, y)`; returns the height used (the header alone when collapsed).
    pub fn layout(&mut self, fonts: &mut Fonts, x0: f64, x1: f64, y: f64, scale: f64) -> f64 {
        self.scale = scale;
        let hh = Section::height(scale);
        self.section.rect = Rect::new(x0, y, x1, y + hh);
        let mut cy = y + hh;
        if !self.section.expanded { self.height = hh + t::RAIL_SECTION_GAP as f64 * scale / 2.0; return self.height }   // closed: the tile (height − gap / 2) is the header exactly, so its hover sits centred
        let pad = t::SPACING_100 as f64 * scale; let gap = t::RAIL_ROW_GAP as f64 * scale;
        let cw = CONTROL_W * scale; let row_h = t::RAIL_ROW_HEIGHT as f64 * scale;
        let label_w = (x1 - x0 - 2.0 * pad - cw - pad).max(40.0);
        let group_top = t::SPACING_200 as f64 * scale;
        for (gi, g) in self.groups.iter_mut().enumerate() {
            cy += if gi == 0 { group_top } else { t::RAIL_GROUP_GAP as f64 * scale };   // sub-headers part by space
            let gh = fonts.detail(g.title, t::RAIL_GROUP_HEADER_SIZE * scale as f32, peniko::Color::WHITE).height() as f64;
            g.header = Rect::new(x0 + pad, cy, x1 - pad, cy + gh); cy += gh + pad;
            for r in g.rows.iter_mut().filter(|r| !r.windows_only || cfg!(windows)) {
                // QCView's Language picker is wider than the rest (`Layout.preferredWidth: 190`): native names and "System default" in German / Russian
                let (cw, label_w) = if r.key == k::LANGUAGE { let w = (190.0 * scale).min(x1 - x0 - 2.0 * pad - pad - 72.0 * scale).max(cw); (w, (x1 - x0 - 2.0 * pad - w - pad).max(40.0)) } else { (cw, label_w) };
                let lh = fonts.layout_width(r.label, Family::Sans, t::RAIL_LABEL_SIZE * scale as f32, t::FONT_WEIGHT_REGULAR as f32, peniko::Color::WHITE, Some(label_w as f32)).height() as f64;
                let h = row_h.max(lh + 2.0 * gap);
                r.rect = Rect::new(x0 + pad, cy, x1 - pad, cy + h);
                r.label_rect = Rect::new(x0 + pad, cy, x0 + pad + label_w, cy + h);
                let right = x1 - pad;
                match &mut r.ctl {
                    Ctl::Switch(sw) => { let (w, hgt) = Switch::size(scale); sw.rect = Rect::new(right - w, cy + (h - hgt) / 2.0, right, cy + (h + hgt) / 2.0); }
                    Ctl::Number(n) => { let fh = t::FIELD_HEIGHT_S as f64 * scale; n.layout(fonts, Rect::new(right - cw, cy + (h - fh) / 2.0, right, cy + (h + fh) / 2.0), scale); }
                    Ctl::Picker(p, _) => { let (pw, ph) = p.measure(fonts, scale); let pw = pw.min(cw); p.rect = Rect::new(right - pw, cy + (h - ph) / 2.0, right, cy + (h + ph) / 2.0); }
                }
                cy += h + gap;
                if r.key == k::BLENDER_SESSIONS {
                    self.blender_rects.clear();
                    for (txt, _) in &self.blender_lines {
                        let l = fonts.layout_width(txt, Family::Sans, t::DETAIL_SIZE_S * scale as f32, t::FONT_WEIGHT_REGULAR as f32, peniko::Color::WHITE, Some((x1 - x0 - 3.0 * pad) as f32));
                        let lh = l.height() as f64; self.blender_rects.push(Rect::new(x0 + 2.0 * pad, cy, x1 - pad, cy + lh)); cy += lh + gap;
                    }
                }
            }
        }
        cy += group_top;
        let (bw, bh) = self.reset.measure(fonts, scale);
        self.reset.rect = Rect::new(x0 + pad, cy, x0 + pad + bw, cy + bh); cy += bh + t::RAIL_SECTION_GAP as f64 * scale;
        self.height = cy - y;
        self.height
    }
    /// Returns (dirty, consumed, commands). A modal dialog or an open menu takes every event first.
    pub fn event(&mut self, e: &Event, now: Instant, fonts: &mut Fonts, s: &mut Settings, bounds: Rect, scale: f64) -> (bool, bool, Vec<Cmd>) {
        let mut dirty = false; let mut cmds = vec![];
        if self.dialog.open {
            let (clicked, d) = self.dialog.event(e, now);
            if clicked == Some(1) { s.reset_all(); cmds.push(Cmd::ResetAll); }
            return (d || clicked.is_some(), true, cmds)
        }
        let mut consumed = false;
        // an open picker menu first (it consumes everything while open)
        let mut menu_took = false;
        for r in self.rows_mut() { if let Ctl::Picker(p, values) = &mut r.ctl { if p.is_open() {
            let (chosen, d, c) = p.event(e, now, fonts, bounds, scale); dirty |= d; consumed |= c; menu_took = true;
            if let Some(i) = chosen { if s.set_str(r.key, values[i]) { cmds.push(Cmd::Changed(r.key)); } }
        } } }
        if menu_took { return (dirty, true, cmds) }
        let (toggled, d) = self.section.event(e, now); dirty |= d; if toggled { dirty = true; }
        if self.section.expanded {
            let mut over: Option<(u64, Rect, String)> = None;
            let mut idx = 0u64;
            for r in self.rows_mut() {
                idx += 1;
                let (lx, ly) = e.pos().unwrap_or((-1.0, -1.0));
                let was = r.label_hover; r.label_hover = !matches!(e, Event::Leave) && r.label_rect.contains(kurbo::Point::new(lx, ly)); if was != r.label_hover { dirty = true; }
                if r.label_hover && !r.help.is_empty() { over = Some((idx, r.label_rect, r.help.to_string())); }
                match &mut r.ctl {
                    Ctl::Switch(sw) => { let (t, d) = sw.event(e, now); dirty |= d; if let Some(on) = t { if s.set_bool(r.key, on) { cmds.push(Cmd::Changed(r.key)); } } }
                    Ctl::Number(n) => { let (v, d) = n.event(e, fonts); dirty |= d; if let Some(v) = v { if s.set_int(r.key, v) { cmds.push(Cmd::Changed(r.key)); } n.set(s.int(r.key)); } }
                    Ctl::Picker(p, values) => { let (chosen, d, c) = p.event(e, now, fonts, bounds, scale); dirty |= d; consumed |= c; if let Some(i) = chosen { if s.set_str(r.key, values[i]) { cmds.push(Cmd::Changed(r.key)); } } }
                }
            }
            let (clicked, d) = self.reset.event(e, now); dirty |= d;
            if clicked { self.dialog.show(); self.dialog.layout(fonts, bounds, scale); dirty = true; consumed = true; }
            if matches!(e, Event::Down { .. }) { self.tooltips.pressed(now); }
            if self.tooltips.hover(over.as_ref().map(|(i, r, s)| (*i, *r, s.as_str())), now) { dirty = true; }
        }
        (dirty, consumed, cmds)
    }
    /// Keys go to the focused number field; Escape closes a menu or the dialog. Returns (handled, commands).
    pub fn key(&mut self, kev: &KeyEvent, fonts: &mut Fonts, s: &mut Settings) -> (bool, Vec<Cmd>) {
        let mut cmds = vec![];
        if self.dialog.open { if matches!(kev.key, min_ui::Key::Named(min_ui::Named::Escape)) { self.dialog.cancel(); } return (true, cmds) }
        let mut handled = false;
        for r in self.rows_mut() {
            match &mut r.ctl {
                Ctl::Picker(p, _) if p.is_open() => { if matches!(kev.key, min_ui::Key::Named(min_ui::Named::Escape)) { p.close(); } handled = true; }
                Ctl::Number(n) if n.focused() => { if let Some(v) = n.key(kev, fonts) { if s.set_int(r.key, v) { cmds.push(Cmd::Changed(r.key)); } n.set(s.int(r.key)); } handled = true; }
                _ => {}
            }
        }
        (handled, cmds)
    }
    /// The wheel over a number field steps it (QCView's spin boxes); returns the commands.
    pub fn wheel(&mut self, x: f64, y: f64, dy: f64, s: &mut Settings) -> Option<Vec<Cmd>> {
        for r in self.rows_mut() { if let Ctl::Number(n) = &mut r.ctl { if n.hit(x, y) && n.enabled { let mut cmds = vec![]; if let Some(v) = n.wheel(dy) { if s.set_int(r.key, v) { cmds.push(Cmd::Changed(r.key)); } n.set(s.int(r.key)); } return Some(cmds) } } }
        None
    }
    /// Focus lost (a press elsewhere, the window): commit any typed number.
    pub fn blur(&mut self, s: &mut Settings) -> Vec<Cmd> {
        let mut cmds = vec![];
        for r in self.rows_mut() { if let Ctl::Number(n) = &mut r.ctl { if let Some(v) = n.blur() { if s.set_int(r.key, v) { cmds.push(Cmd::Changed(r.key)); } n.set(s.int(r.key)); } } }
        cmds
    }
    /// Per frame: tooltip timing and the ramps; true when a redraw is needed now.
    pub fn tick(&mut self, now: Instant) -> bool {
        self.tooltips.tick(now) || self.section.animating(now) || self.reset.animating(now) || self.dialog.animating(now)
            || self.rows().any(|r| match &r.ctl { Ctl::Switch(sw) => sw.animating(now), Ctl::Picker(p, _) => p.animating(now), Ctl::Number(_) => false })
    }
    /// IME composition to the focused number field.
    pub fn ime(&mut self, e: &min_ui::ImeEvent, fonts: &mut Fonts) { for r in self.rows_mut() { if let Ctl::Number(n) = &mut r.ctl { if n.focused() { n.field.ime(e, fonts); } } } }

    pub fn paint(&mut self, scene: &mut Scene, fonts: &mut Fonts, scale: f64, now: Instant, first: bool) {
        let gap = t::RAIL_SECTION_GAP as f64 * scale;
        min_ui::section::paint_tile(scene, Rect::new(self.section.rect.x0, self.section.rect.y0, self.section.rect.x1, self.section.rect.y0 + self.height - gap / 2.0), scale);
        self.section.paint(scene, fonts, scale, now, first);
        if !self.section.expanded { return }
        for g in &mut self.groups {
            let l = fonts.detail(g.title, t::RAIL_GROUP_HEADER_SIZE * scale as f32, paint::color(t::RAIL_GROUP_HEADER_COLOR));
            min_ui::fonts::draw_layout(scene, &l, kurbo::Affine::translate((g.header.x0, g.header.y0)));
            for r in g.rows.iter_mut().filter(|r| !r.windows_only || cfg!(windows)) {
                let c = if r.backed { t::RAIL_LABEL_COLOR } else { t::DISABLED_CONTENT_COLOR };
                let l = fonts.layout_width(r.label, Family::Sans, t::RAIL_LABEL_SIZE * scale as f32, t::FONT_WEIGHT_REGULAR as f32, paint::color(c), Some(r.label_rect.width() as f32));
                min_ui::fonts::draw_layout(scene, &l, kurbo::Affine::translate((r.label_rect.x0, r.rect.y0 + (r.rect.height() - l.height() as f64) / 2.0)));
                match &mut r.ctl { Ctl::Switch(sw) => sw.paint(scene, scale, now), Ctl::Number(n) => n.paint(scene, fonts, scale), Ctl::Picker(p, _) => p.paint(scene, fonts, scale, now) }
                if r.key == k::BLENDER_SESSIONS { for ((txt, kind), rr) in self.blender_lines.iter().zip(&self.blender_rects) {
                    let c = match kind { 1 => t::POSITIVE_VISUAL_COLOR, 2 => t::NEGATIVE_VISUAL_COLOR, _ => t::NEUTRAL_SUBDUED_CONTENT_COLOR_DEFAULT };
                    let l = fonts.layout_width(txt, Family::Sans, t::DETAIL_SIZE_S * scale as f32, t::FONT_WEIGHT_REGULAR as f32, paint::color(c), Some(rr.width() as f32));
                    min_ui::fonts::draw_layout(scene, &l, kurbo::Affine::translate((rr.x0, rr.y0)));
                } }
            }
        }
        self.reset.paint(scene, fonts, scale, now);
    }
    /// Menus, the tooltip and the dialog, over everything.
    pub fn paint_overlays(&mut self, scene: &mut Scene, fonts: &mut Fonts, scale: f64, bounds: Rect, now: Instant) {
        for r in self.rows_mut() { if let Ctl::Picker(p, _) = &r.ctl { p.paint_menu(scene, fonts, scale); } }
        self.tooltips.paint(scene, fonts, scale, bounds, now);
        self.dialog.paint(scene, fonts, scale, bounds, now);
    }
}
