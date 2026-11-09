//! Keyboard Shortcuts — QCView's read-only reference card at the foot of the Right Rail (`RightRail.qml` 254–520,
//! collapsed by default; Tools ▸ Keyboard Shortcuts / Ctrl+/ opens the rail and expands it alone, scrolled into view).
//! Ruled 2026-11-09 (row 218): the rail card, not a modal (a modal pauses the picture, so a key can't be tried while
//! reading it) and not a window (a second surface, and the keys would land in it); a FILTER field on top, since the
//! port's list outgrew QCView's ~55 rows. `GROUPS` is the one table: the card is drawn from it and the tests hold it
//! to the shell's real handlers — every player binding runs through `qcv_player::map_key`, every menu accelerator
//! must be listed — so QCView's hand-kept card drift (loop "L" vs V, "⌥T" vs ⌘T) can't recur. QCView's "Timeline
//! (edit mode)" group is gone with edit mode (row 160); playlists and dual view list their own keys instead.
use kurbo::{Affine, Rect};
use min_theme::tokens as t;
use min_ui::{paint, Event, Family, FieldResponse, Fonts, ImeEvent, KeyEvent, Section, TextField};
use qcv_player::Action;
use std::time::Instant;
use vello::Scene;

fn tr(s: &str) -> &str { min_i18n::trc("RightRail", s) }

/// One binding. `cmd` = Ctrl on Windows, ⌘ on macOS (the shell's `mods.ctrl`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bind { pub cmd: bool, pub shift: bool, pub alt: bool, pub key: &'static str }
const fn k(key: &'static str) -> Bind { Bind { cmd: false, shift: false, alt: false, key } }
const fn c(key: &'static str) -> Bind { Bind { cmd: true, shift: false, alt: false, key } }
const fn cs(key: &'static str) -> Bind { Bind { cmd: true, shift: true, alt: false, key } }
const fn ca(key: &'static str) -> Bind { Bind { cmd: true, shift: false, alt: true, key } }
const fn s(key: &'static str) -> Bind { Bind { cmd: false, shift: true, alt: false, key } }
const fn a(key: &'static str) -> Bind { Bind { cmd: false, shift: false, alt: true, key } }

impl Bind {
    /// QCView's spellings: "Ctrl+Shift+O" on Windows, "⇧⌘O" on macOS (Apple's modifier order ⌥⇧⌘).
    pub fn label(&self, mac: bool) -> String {
        let key = match self.key { "Scroll" => tr("Scroll").to_string(), k => k.to_string() };
        if mac { format!("{}{}{}{key}", if self.alt { "⌥" } else { "" }, if self.shift { "⇧" } else { "" }, if self.cmd { "⌘" } else { "" }) }
        else { format!("{}{}{}{key}", if self.cmd { "Ctrl+" } else { "" }, if self.shift { "Shift+" } else { "" }, if self.alt { "Alt+" } else { "" }) }
    }
}

#[allow(dead_code)]   // `player` is read by the drift test
pub struct Entry { pub label: &'static str, pub binds: &'static [Bind], /// what `map_key` must answer for each binding (the drift test)
    pub player: Option<Action> }
const fn e(label: &'static str, binds: &'static [Bind]) -> Entry { Entry { label, binds, player: None } }
const fn p(label: &'static str, binds: &'static [Bind], a: Action) -> Entry { Entry { label, binds, player: Some(a) } }
pub struct Group { pub heading: &'static str, pub entries: &'static [Entry] }

/// QCView's card, group for group, with the port's own keys added where they live.
pub const GROUPS: &[Group] = &[
    Group { heading: "File", entries: &[
        e("Open Media…", &[c("O")]), e("Open Project…", &[cs("O")]), e("New Project", &[c("N")]),
        e("Save Project", &[c("S")]), e("Save Project As…", &[cs("S")]), e("Quit", &[c("Q")]),
    ] },
    Group { heading: "View", entries: &[
        e("Minimal Mode", &[c("0")]), e("Compact Mode", &[cs("C")]), e("Always on Top", &[ca("T")]), e("Show All Panels", &[c("9")]),
        e("Left Rail", &[c("1")]), e("Right Rail", &[c("2")]), e("Color Panel", &[c("3")]), e("Notes Panel", &[c("4")]), e("Status Bar", &[c("5")]),
        e("Keyboard Shortcuts", &[c("/")]), e("Settings", &[c(",")]),
        p("Fullscreen", &[k("F")], Action::ToggleFullscreen), e("Exit Fullscreen / Compact", &[k("Esc")]),
    ] },
    Group { heading: "Transport", entries: &[
        p("Play / Pause", &[k("Space"), k("W"), k("S"), k("K")], Action::TogglePlayback),
        p("Previous Frame", &[k("Q"), k("←")], Action::Step(-1)), p("Next Frame", &[k("E"), k("→")], Action::Step(1)),
        p("Rewind (hold)", &[k("A"), k("J")], Action::ShuttleBegin(-1)), p("Fast Forward (hold)", &[k("D"), k("L")], Action::ShuttleBegin(1)),
        p("Seek to Start", &[k("Home")], Action::SeekToStart), p("Seek to End", &[k("End")], Action::SeekToEnd),
        p("Toggle Loop", &[k("V")], Action::ToggleLoop), e("Toggle Loop", &[c("L")]),
        p("Set In Point", &[k("I")], Action::SetInPoint), p("Set Out Point", &[k("O")], Action::SetOutPoint),
        p("Clear In/Out Points", &[s("I")], Action::ClearInOut),
        p("Volume Up", &[k("↑")], Action::VolumeUp), p("Volume Down", &[k("↓")], Action::VolumeDown), p("Mute / Unmute", &[k("M")], Action::ToggleMute),
        p("Cycle Background", &[k("B")], Action::CycleBackground),
        p("Review Speed (cycle)", &[k("R")], Action::CycleReviewSpeed), p("Review Speed to 1×", &[s("R")], Action::ResetReviewSpeed),
    ] },
    Group { heading: "Screenshots", entries: &[
        p("Screenshot to Desktop", &[k("T")], Action::Screenshot), p("Screenshot to Clipboard", &[c("T")], Action::ScreenshotToClipboard),
    ] },
    Group { heading: "Annotations (in annotation mode)", entries: &[
        p("Undo Stroke", &[c("Z")], Action::AnnotationUndo), p("Redo Stroke", &[cs("Z")], Action::AnnotationRedo),
        e("Cancel In-Flight Stroke / Clear Selection", &[k("Esc")]), e("Delete Selected Stroke", &[k("Delete"), k("Backspace")]),
    ] },
    Group { heading: "Bins (when item selected)", entries: &[
        e("Delete Item", &[k("Delete"), k("Backspace")]), e("Rename Item", &[k("F2")]), e("Select All", &[c("A")]),
    ] },
    Group { heading: "Playlist", entries: &[
        p("Undo Edit", &[c("Z")], Action::AnnotationUndo), p("Redo Edit", &[cs("Z")], Action::AnnotationRedo),
        e("Split Clip at Playhead", &[c("K")]), e("Delete Selected Clip", &[k("Delete"), k("Backspace")]),
    ] },
    Group { heading: "Dual View", entries: &[
        p("Undo Edit", &[c("Z")], Action::AnnotationUndo), p("Redo Edit", &[cs("Z")], Action::AnnotationRedo),
        e("Trim Head to Playhead", &[k("[")]), e("Trim Tail to Playhead", &[k("]")]), e("Cut Target Side at Playhead", &[c("K")]),
        e("Slide Clip a Frame", &[a("←"), a("→")]), e("Slide Clip 10 Frames", &[Bind { cmd: false, shift: true, alt: true, key: "←" }, Bind { cmd: false, shift: true, alt: true, key: "→" }]),
        e("Delete Clip (leave gap)", &[k("Delete")]), e("Delete Clip (close gap)", &[s("Delete")]),
    ] },
    Group { heading: "Timeline (zoom + pan)", entries: &[
        e("Zoom In", &[c("=")]), e("Zoom Out", &[c("-")]), e("Zoom (cursor anchored)", &[k("Scroll")]), e("Pan horizontally", &[a("Scroll")]),
    ] },
];

#[derive(Clone, Debug)]
struct Row { label: String, caps: Vec<String>, rect: Rect, label_w: f64 }
#[derive(Clone, Debug)]
struct Shown { heading: String, rect: Rect, rows: Vec<Row> }

pub struct ShortcutsPanel { pub section: Section, filter: TextField, shown: Vec<Shown>, height: f64, mac: bool, empty: Option<Rect> }

impl ShortcutsPanel {
    /// A language switch (canon row 197): the labels built at construction, rebuilt; the state stays.
    pub fn relabel(&mut self) { let f = Self::new(); self.section.title = f.section.title; self.filter.placeholder = f.filter.placeholder; }
    pub fn new() -> ShortcutsPanel {
        let mut filter = TextField::new(Family::Sans, t::RAIL_LABEL_SIZE); filter.placeholder = tr("Filter shortcuts").to_string();
        ShortcutsPanel { section: Section::new(tr("Keyboard Shortcuts"), false), filter, shown: vec![], height: 0.0, mac: cfg!(target_os = "macos"), empty: None }
    }
    pub fn has_focus(&self) -> bool { self.filter.focused }
    pub fn blur(&mut self) { self.filter.focused = false; }
    /// The groups and rows the filter keeps (a heading match keeps its whole group); same-label rows merge their caps.
    fn matches(&self) -> Vec<(String, Vec<(String, Vec<String>)>)> {
        let q = self.filter.text().trim().to_lowercase();
        GROUPS.iter().filter_map(|g| {
            let head = tr(g.heading).to_string();
            let whole = q.is_empty() || head.to_lowercase().contains(&q) || g.heading.to_lowercase().contains(&q);
            let mut rows: Vec<(String, Vec<String>)> = vec![];
            for en in g.entries {
                let label = tr(en.label).to_string();
                let caps: Vec<String> = en.binds.iter().map(|b| b.label(self.mac)).collect();
                let hit = whole || label.to_lowercase().contains(&q) || en.label.to_lowercase().contains(&q) || caps.iter().any(|c| c.to_lowercase() == q || c.to_lowercase().contains(&q) && q.chars().count() > 1);
                if !hit { continue }
                match rows.iter_mut().find(|r| r.0 == label) { Some(r) => r.1.extend(caps), None => rows.push((label, caps)) }
            }
            (!rows.is_empty()).then_some((head, rows))
        }).collect()
    }
    fn cap_w(fonts: &mut Fonts, cap: &str, scale: f64) -> f64 { fonts.layout(cap, Family::Sans, t::CODE_SIZE_S * scale as f32, t::FONT_WEIGHT_REGULAR as f32, peniko::Color::WHITE).width() as f64 + 10.0 * scale }
    pub fn layout(&mut self, fonts: &mut Fonts, x0: f64, x1: f64, y: f64, scale: f64) -> f64 {
        let hh = Section::height(scale);
        self.section.rect = Rect::new(x0, y, x1, y + hh);
        if !self.section.expanded { self.height = hh; self.shown.clear(); return hh }
        let pad = t::SPACING_100 as f64 * scale; let (x0, x1) = (x0 + pad, x1 - pad);
        let mut cy = y + hh + pad / 2.0;
        let fh = t::FIELD_HEIGHT_S as f64 * scale;
        self.filter.layout(fonts, Rect::new(x0, cy, x1, cy + fh), scale); cy += fh;
        let row_h = (t::RAIL_ROW_HEIGHT as f64 - 6.0).max(18.0) * scale; let cap_gap = 4.0 * scale; let gap = t::SPACING_200 as f64 * scale;
        let groups = self.matches();
        self.shown.clear();
        for (gi, (heading, rows)) in groups.into_iter().enumerate() {
            cy += if gi == 0 { t::SPACING_200 as f64 * scale } else { t::RAIL_GROUP_GAP as f64 * scale };
            let gh = fonts.detail(&heading, t::RAIL_GROUP_HEADER_SIZE * scale as f32, peniko::Color::WHITE).height() as f64;
            let hr = Rect::new(x0, cy, x1, cy + gh); cy += gh + pad / 2.0;
            let mut out = vec![];
            for (label, caps) in rows {
                let caps_w: f64 = caps.iter().map(|c| Self::cap_w(fonts, c, scale)).sum::<f64>() + cap_gap * caps.len().saturating_sub(1) as f64;
                let label_w = (x1 - x0 - caps_w - gap).max(60.0 * scale);
                let lh = fonts.layout_width(&label, Family::Sans, t::RAIL_LABEL_SIZE * scale as f32, t::FONT_WEIGHT_REGULAR as f32, peniko::Color::WHITE, Some(label_w as f32)).height() as f64;
                let h = row_h.max(lh + 4.0 * scale);
                out.push(Row { label, caps, rect: Rect::new(x0, cy, x1, cy + h), label_w }); cy += h;
            }
            self.shown.push(Shown { heading, rect: hr, rows: out });
        }
        self.empty = None;
        if self.shown.is_empty() { cy += t::SPACING_200 as f64 * scale; let r = Rect::new(x0, cy, x1, cy + row_h); self.empty = Some(r); cy += row_h; }
        cy += t::SPACING_200 as f64 * scale;
        self.height = cy - y + t::RAIL_SECTION_GAP as f64 * scale;
        self.height
    }
    /// Returns true when a redraw is needed. A press in the filter focuses it (the shell turns IME on).
    pub fn event(&mut self, e: &Event, now: Instant, fonts: &mut Fonts) -> bool {
        let (toggled, mut dirty) = self.section.event(e, now);
        if toggled { dirty = true; if !self.section.expanded { self.filter.focused = false; } }
        if !self.section.expanded { return dirty }
        if matches!(e, Event::Down { .. }) && e.pos().is_some_and(|(x, y)| !self.filter.hit(x, y)) && self.filter.focused { self.filter.focused = false; dirty = true; }
        dirty |= self.filter.pointer(e, fonts);
        dirty
    }
    /// The filter's keys: Esc clears a filter, then gives the focus back; Enter just leaves it.
    pub fn key(&mut self, k: &KeyEvent, fonts: &mut Fonts) {
        match self.filter.key(k, fonts) {
            FieldResponse::Cancelled => { if self.filter.text().is_empty() { self.filter.focused = false; } else { self.filter.clear(); } }
            FieldResponse::Accepted => self.filter.focused = false,
            _ => {}
        }
    }
    pub fn ime(&mut self, e: &ImeEvent, fonts: &mut Fonts) { self.filter.ime(e, fonts); }
    pub fn animating(&self, now: Instant) -> bool { self.section.animating(now) }
    pub fn paint(&mut self, scene: &mut Scene, fonts: &mut Fonts, scale: f64, now: Instant, first: bool) {
        let gap = t::RAIL_SECTION_GAP as f64 * scale;
        min_ui::section::paint_tile(scene, Rect::new(self.section.rect.x0, self.section.rect.y0, self.section.rect.x1, self.section.rect.y0 + self.height - gap / 2.0), scale);
        self.section.paint(scene, fonts, scale, now, first);
        if !self.section.expanded { return }
        self.filter.paint(scene, fonts, scale);
        let cap_gap = 4.0 * scale; let cap_h = 16.0 * scale; let rad = 4.0 * scale;   // QCView's radiusSmall: a keycap, not a pill
        for g in &self.shown {
            let l = fonts.detail(&g.heading, t::RAIL_GROUP_HEADER_SIZE * scale as f32, paint::color(t::RAIL_GROUP_HEADER_COLOR));
            min_ui::fonts::draw_layout(scene, &l, Affine::translate((g.rect.x0, g.rect.y0)));
            for r in &g.rows {
                let l = fonts.layout_width(&r.label, Family::Sans, t::RAIL_LABEL_SIZE * scale as f32, t::FONT_WEIGHT_REGULAR as f32, paint::color(t::RAIL_VALUE_COLOR), Some(r.label_w as f32));
                min_ui::fonts::draw_layout(scene, &l, Affine::translate((r.rect.x0, r.rect.y0 + (r.rect.height() - l.height() as f64) / 2.0)));
                // keycaps right-aligned, each binding its own recessed cap (QCView: the KvRow wells' tone, mono text)
                let mut x = r.rect.x1;
                for cap in r.caps.iter().rev() {
                    let tl = fonts.layout(cap, Family::Sans, t::CODE_SIZE_S * scale as f32, t::FONT_WEIGHT_REGULAR as f32, paint::color(t::RAIL_LABEL_COLOR));
                    let w = tl.width() as f64 + 10.0 * scale; let cy = r.rect.y0 + (r.rect.height() - cap_h) / 2.0;
                    let cr = Rect::new(x - w, cy, x, cy + cap_h);
                    paint::rounded(scene, cr, rad, t::FIELD_BACKGROUND_COLOR);
                    min_ui::fonts::draw_layout(scene, &tl, Affine::translate((cr.x0 + 5.0 * scale, cr.y0 + (cap_h - tl.height() as f64) / 2.0)));
                    x -= w + cap_gap;
                }
            }
        }
        if let Some(r) = self.empty {
            let l = fonts.layout(tr("No shortcut matches"), Family::Sans, t::RAIL_LABEL_SIZE * scale as f32, t::FONT_WEIGHT_REGULAR as f32, paint::color(t::DISABLED_CONTENT_COLOR));
            min_ui::fonts::draw_layout(scene, &l, Affine::translate((r.x0, r.y0 + (r.height() - l.height() as f64) / 2.0)));
        }
    }
    /// Test / snapshot hook: type a filter.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn set_filter(&mut self, q: &str) { self.filter.set_text(q); }
    #[cfg(test)]
    pub fn shown_labels(&self) -> Vec<String> { self.shown.iter().flat_map(|g| g.rows.iter().map(|r| r.label.clone())).collect() }
}

#[cfg(test)]
mod tests {
    use super::*;
    use qcv_player::{map_key, Key, Mods};
    fn player_key(b: &Bind) -> Key {
        match b.key { "Space" => Key::Space, "←" => Key::Left, "→" => Key::Right, "↑" => Key::Up, "↓" => Key::Down, "Home" => Key::Home, "End" => Key::End,
            k => Key::Char(k.chars().next().unwrap().to_ascii_lowercase()) }
    }
    /// Every binding the card says the transport answers, the transport answers — the drift QCView's card had can't recur.
    #[test] fn every_player_binding_is_the_key_maps() {
        let mut n = 0;
        for g in GROUPS { for en in g.entries { let Some(want) = en.player else { continue };
            for b in en.binds {
                let m = Mods { shift: b.shift, ctrl: b.cmd, alt: b.alt, keypad: false };
                assert_eq!(map_key(player_key(b), m, true, false), Some(want), "{} / {}", en.label, b.label(false)); n += 1;
            }
        } }
        assert!(n >= 30, "{n}");
    }
    /// Every menu accelerator is on the card, under the menu's own spelling.
    #[test] fn every_menu_accelerator_is_listed() {
        let all: Vec<String> = GROUPS.iter().flat_map(|g| g.entries.iter().flat_map(|e| e.binds.iter().map(|b| b.label(false)))).collect();
        let mut n = 0;
        for (_, items) in crate::menu::spec() { for it in items {
            let sc = match it { crate::menu::Node::Item { sc: Some(sc), .. } | crate::menu::Node::Check { sc: Some(sc), .. } => sc, _ => continue };
            assert!(all.contains(&sc.label()), "the menu's {} is not on the card", sc.label()); n += 1;
        } }
        assert!(n >= 15, "{n}");
    }
    /// Every heading and label on the card reads in every carried language (a new shortcut arrives translated).
    #[test] fn every_label_is_translated() {
        let mut gaps = vec![];
        for (code, po) in crate::i18n::CATALOGS.iter().filter(|(c, _)| *c != "en") {
            let cat = min_i18n::Catalog::parse(code, po);
            for g in GROUPS { for text in std::iter::once(g.heading).chain(g.entries.iter().map(|e| e.label)) { if cat.text("RightRail", text).is_none() { gaps.push(format!("{code}: {text}")); } } }
            for text in ["Keyboard Shortcuts", "Filter shortcuts", "No shortcut matches", "Scroll"] { if cat.text("RightRail", text).is_none() { gaps.push(format!("{code}: {text}")); } }
        }
        assert!(gaps.is_empty(), "untranslated: {gaps:?}");
    }
    #[test] fn spellings() {
        assert_eq!(cs("O").label(false), "Ctrl+Shift+O"); assert_eq!(cs("O").label(true), "⇧⌘O");
        assert_eq!(ca("T").label(true), "⌥⌘T"); assert_eq!(s("R").label(true), "⇧R"); assert_eq!(a("Scroll").label(false), "Alt+Scroll");
    }
    #[test] fn the_filter() {
        let mut fonts = Fonts::new();
        let mut sp = ShortcutsPanel::new(); sp.section.expanded = true; sp.mac = false;
        sp.layout(&mut fonts, 0.0, 300.0, 0.0, 1.0);
        let all = sp.shown_labels().len(); assert!(all > 60, "{all}");
        sp.set_filter("loop"); sp.layout(&mut fonts, 0.0, 300.0, 0.0, 1.0);
        assert_eq!(sp.shown_labels(), vec!["Toggle Loop".to_string()]);
        assert_eq!(sp.shown[0].rows[0].caps, vec!["V", "Ctrl+L"], "same-label rows merge");
        sp.set_filter("ctrl+k"); sp.layout(&mut fonts, 0.0, 300.0, 0.0, 1.0);
        assert_eq!(sp.shown_labels(), vec!["Split Clip at Playhead", "Cut Target Side at Playhead"]);
        sp.set_filter("screenshots"); sp.layout(&mut fonts, 0.0, 300.0, 0.0, 1.0);
        assert_eq!(sp.shown_labels().len(), 2, "a heading keeps its group");
        sp.set_filter("zzz"); sp.layout(&mut fonts, 0.0, 300.0, 0.0, 1.0);
        assert!(sp.shown.is_empty() && sp.empty.is_some());
    }
}
